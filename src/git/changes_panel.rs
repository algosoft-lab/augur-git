//! Working-tree changes shown below the commit editor.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
    spinner::Spinner,
    v_flex,
};

use crate::core::git::{FileStatus, WorkingTreeAction, WorkingTreeScope};
use crate::core::i18n::{self, Locale};
use crate::git::{lucide, shared};

/// ChangesPanel → RepoTab events.
#[derive(Clone, Debug)]
pub enum ChangesPanelEvent {
    /// Request a diff for a file in either the index or working tree.
    FileSelected { staged: bool, file: FileStatus },
    /// Request a staged/working-tree mutation.
    OperationRequested {
        action: WorkingTreeAction,
        scope: WorkingTreeScope,
    },
    /// Request a fresh repository status snapshot.
    RefreshRequested,
}

pub struct ChangesPanel {
    staged: Vec<FileStatus>,
    unstaged: Vec<FileStatus>,
    selected: Option<(bool, String)>,
    collapsed: Vec<&'static str>,
    has_conflicts: bool,
    busy: bool,
    refresh_selected: bool,
    locale: Locale,
}

impl EventEmitter<ChangesPanelEvent> for ChangesPanel {}

impl ChangesPanel {
    pub fn new(locale: Locale) -> Self {
        Self {
            staged: Vec::new(),
            unstaged: Vec::new(),
            selected: None,
            collapsed: Vec::new(),
            has_conflicts: false,
            busy: false,
            refresh_selected: false,
            locale,
        }
    }

    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy != busy {
            self.busy = busy;
            cx.notify();
        }
    }

    /// Re-query the currently selected working-tree diff after a refresh.
    pub fn set_refresh_selected(&mut self, refresh: bool) {
        self.refresh_selected = refresh;
    }

    pub fn set_files(
        &mut self,
        files: Vec<FileStatus>,
        cx: &mut Context<Self>,
    ) {
        let refresh_selected = self.refresh_selected;
        self.refresh_selected = false;
        let selected = self.selected.clone();
        let previous_file = selected.as_ref().and_then(|(staged, path)| {
            let list = if *staged {
                &self.staged
            } else {
                &self.unstaged
            };
            list.iter().find(|file| &file.path == path).cloned()
        });
        self.has_conflicts = files.iter().any(FileStatus::is_conflicted);
        let (staged, unstaged) = split_files(files);
        self.staged = staged;
        self.unstaged = unstaged;

        self.selected = selected.clone().and_then(|(staged_group, path)| {
            let preferred = if staged_group {
                &self.staged
            } else {
                &self.unstaged
            };
            if preferred.iter().any(|file| file.path == path) {
                return Some((staged_group, path));
            }

            let other = if staged_group {
                &self.unstaged
            } else {
                &self.staged
            };
            other
                .iter()
                .any(|file| file.path == path)
                .then_some((!staged_group, path))
        });
        if let Some((staged_group, path)) = &self.selected {
            let list = if *staged_group {
                &self.staged
            } else {
                &self.unstaged
            };
            if let Some(file) = list.iter().find(|file| &file.path == path)
                && (refresh_selected
                    || self.selected != selected
                    || previous_file.as_ref() != Some(file))
            {
                // A refresh may keep the same group/path while changing the
                // status or rename source. Re-request the diff so the panel
                // never displays a stale snapshot.
                cx.emit(ChangesPanelEvent::FileSelected {
                    staged: *staged_group,
                    file: file.clone(),
                });
            }
        }
        cx.notify();
    }

    fn is_collapsed(&self, key: &str) -> bool {
        self.collapsed.iter().any(|item| *item == key)
    }

    fn toggle_section(&mut self, key: &'static str, cx: &mut Context<Self>) {
        match self.collapsed.iter().position(|item| *item == key) {
            Some(index) => {
                self.collapsed.remove(index);
            }
            None => self.collapsed.push(key),
        }
        cx.notify();
    }

    fn select_file(
        &mut self,
        staged: bool,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let list = if staged { &self.staged } else { &self.unstaged };
        let Some(file) = list.get(index).cloned() else {
            return;
        };
        self.selected = Some((staged, file.path.clone()));
        cx.emit(ChangesPanelEvent::FileSelected { staged, file });
        cx.notify();
    }

    fn request_file_operation(
        &mut self,
        action: WorkingTreeAction,
        staged: bool,
        file: FileStatus,
        cx: &mut Context<Self>,
    ) {
        if self.busy || !can_operate(action, staged, &file) {
            return;
        }
        cx.emit(ChangesPanelEvent::OperationRequested {
            action,
            scope: WorkingTreeScope::File(file),
        });
    }

    fn request_group_operation(
        &mut self,
        action: WorkingTreeAction,
        staged: bool,
        files: &[FileStatus],
        cx: &mut Context<Self>,
    ) {
        if self.busy
            || files.is_empty()
            || files.iter().any(|file| !can_operate(action, staged, file))
        {
            return;
        }
        cx.emit(ChangesPanelEvent::OperationRequested {
            action,
            scope: WorkingTreeScope::All(files.to_vec()),
        });
    }

    fn action_button(
        &self,
        cx: &Context<Self>,
        id: String,
        icon: IconName,
        tooltip: String,
        disabled: bool,
        action: WorkingTreeAction,
        staged: bool,
        scope: WorkingTreeScope,
    ) -> impl IntoElement {
        let this = cx.entity();
        let mut button = Button::new(SharedString::from(id))
            .icon(icon)
            .ghost()
            .compact()
            .xsmall()
            .tooltip(tooltip)
            .disabled(disabled);
        if !disabled {
            button = button.on_click(move |_event, _window, cx| {
                this.update(cx, |_panel, cx| match scope.clone() {
                    WorkingTreeScope::File(file) => {
                        _panel.request_file_operation(action, staged, file, cx);
                    }
                    WorkingTreeScope::All(files) => {
                        _panel.request_group_operation(
                            action, staged, &files, cx,
                        );
                    }
                });
            });
        }
        div()
            .w(px(22.))
            .h(px(22.))
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .child(button)
    }

    fn section_header(
        &self,
        cx: &Context<Self>,
        staged: bool,
        files: &[FileStatus],
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let key: &'static str = if staged {
            "section-staged"
        } else {
            "section-changes"
        };
        let collapsed = self.is_collapsed(key);
        let this = cx.entity();
        let mut header = h_flex()
            .id(SharedString::from(format!("changes-section-{key}")))
            .w_full()
            .h(px(24.))
            .flex_shrink_0()
            .px_2()
            .rounded_md()
            .items_center()
            .gap_1()
            .cursor(CursorStyle::PointingHand)
            .hover(|element| element.bg(colors.list_hover))
            .on_click(move |_event, _window, cx| {
                this.update(cx, |panel, cx| panel.toggle_section(key, cx));
            })
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(colors.muted_foreground)
                    .child(if collapsed {
                        Icon::new(IconName::ChevronRight)
                    } else {
                        Icon::new(IconName::ChevronDown)
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(self.locale, key))),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(colors.muted_foreground)
                    .child(files.len().to_string()),
            );

        let scope = WorkingTreeScope::All(files.to_vec());
        if staged {
            header = header.child(self.action_button(
                cx,
                "changes-unstage-all".to_string(),
                IconName::Minus,
                i18n::text(self.locale, "changes-unstage-all"),
                self.busy || files.is_empty(),
                WorkingTreeAction::Unstage,
                staged,
                scope,
            ));
        } else {
            header = header
                .child(self.action_button(
                    cx,
                    "changes-discard-all".to_string(),
                    IconName::Undo,
                    i18n::text(self.locale, "changes-discard-all"),
                    self.busy || files.is_empty() || self.has_conflicts,
                    WorkingTreeAction::Discard,
                    staged,
                    scope.clone(),
                ))
                .child(self.action_button(
                    cx,
                    "changes-stage-all".to_string(),
                    IconName::Plus,
                    i18n::text(self.locale, "changes-stage-all"),
                    self.busy || files.is_empty() || self.has_conflicts,
                    WorkingTreeAction::Stage,
                    staged,
                    scope,
                ));
        }
        header
    }

    fn action_wrapper(
        &self,
        cx: &Context<Self>,
        id: String,
        icon: IconName,
        tooltip_key: &'static str,
        disabled: bool,
        action: WorkingTreeAction,
        _staged: bool,
        file: &FileStatus,
        row_group: SharedString,
    ) -> impl IntoElement {
        div()
            .invisible()
            .group_hover(row_group, |element| element.visible())
            .child(self.action_button(
                cx,
                id,
                icon,
                i18n::text(self.locale, tooltip_key),
                disabled,
                action,
                _staged,
                WorkingTreeScope::File(file.clone()),
            ))
    }

    fn file_row(
        &self,
        cx: &Context<Self>,
        staged: bool,
        index: usize,
        file: &FileStatus,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let (status_color, status_label) =
            status_style(&colors, file.code_for(staged), self.locale);
        let selected =
            self.selected
                .as_ref()
                .is_some_and(|(selected_staged, path)| {
                    *selected_staged == staged && path == &file.path
                });
        let conflicted = file.is_conflicted();
        let row_group =
            SharedString::from(format!("working-tree-row-{staged}-{index}"));
        let this = cx.entity();
        let file_for_context = file.clone();
        let file_for_stage = file.clone();
        let file_for_unstage = file.clone();
        let file_for_discard = file.clone();
        let panel = cx.entity();
        let row = h_flex()
            .id(SharedString::from(format!(
                "working-tree-file-{staged}-{index}"
            )))
            .w_full()
            .h(px(24.))
            .flex_shrink_0()
            .px_2()
            .gap_1()
            .items_center()
            .rounded_sm()
            .group(row_group.clone())
            .bg(if selected {
                colors.list_active
            } else {
                colors.background
            })
            .hover(|element| {
                if selected {
                    element
                } else {
                    element.bg(colors.list_hover)
                }
            })
            .on_click(move |_event, _window, cx| {
                this.update(cx, |panel, cx| {
                    panel.select_file(staged, index, cx);
                });
            })
            .child(
                div()
                    .w(px(22.))
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .text_color(status_color)
                    .child(shared(status_label)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(colors.foreground)
                    .truncate()
                    .child(shared(file.path.clone())),
            );

        let action_disabled =
            |action| self.busy || !can_operate(action, staged, file);
        let stage_tooltip = if conflicted {
            "changes-action-conflict"
        } else {
            "changes-stage"
        };
        let discard_tooltip = if conflicted {
            "changes-action-conflict"
        } else {
            "changes-discard"
        };
        let unstage_tooltip = if conflicted {
            "changes-action-conflict"
        } else {
            "changes-unstage"
        };
        let actions = if staged {
            h_flex()
                .gap_0p5()
                .flex_shrink_0()
                .child(self.action_wrapper(
                    cx,
                    format!("changes-unstage-{staged}-{index}"),
                    IconName::Minus,
                    unstage_tooltip,
                    action_disabled(WorkingTreeAction::Unstage),
                    WorkingTreeAction::Unstage,
                    staged,
                    &file_for_unstage,
                    row_group.clone(),
                ))
                .into_any_element()
        } else {
            h_flex()
                .gap_0p5()
                .flex_shrink_0()
                .child(self.action_wrapper(
                    cx,
                    format!("changes-discard-{staged}-{index}"),
                    IconName::Undo,
                    discard_tooltip,
                    action_disabled(WorkingTreeAction::Discard),
                    WorkingTreeAction::Discard,
                    staged,
                    &file_for_discard,
                    row_group.clone(),
                ))
                .child(self.action_wrapper(
                    cx,
                    format!("changes-stage-{staged}-{index}"),
                    IconName::Plus,
                    stage_tooltip,
                    action_disabled(WorkingTreeAction::Stage),
                    WorkingTreeAction::Stage,
                    staged,
                    &file_for_stage,
                    row_group.clone(),
                ))
                .into_any_element()
        };

        let row = row.child(actions);
        let locale = self.locale;
        let busy = self.busy;
        row.context_menu(move |menu, _window, _cx| {
            let stage_panel = panel.clone();
            let unstage_panel = panel.clone();
            let discard_panel = panel.clone();
            let stage_file = file_for_context.clone();
            let unstage_file = file_for_context.clone();
            let discard_file = file_for_context.clone();
            let operation_disabled = conflicted;
            if staged {
                menu.item(
                    PopupMenuItem::new(i18n::text(locale, "changes-unstage"))
                        .icon(IconName::Minus)
                        .disabled(busy || operation_disabled)
                        .on_click(move |_event, _window, cx| {
                            unstage_panel.update(cx, |panel, cx| {
                                panel.request_file_operation(
                                    WorkingTreeAction::Unstage,
                                    true,
                                    unstage_file.clone(),
                                    cx,
                                );
                            });
                        }),
                )
            } else {
                menu.item(
                    PopupMenuItem::new(i18n::text(locale, "changes-stage"))
                        .icon(IconName::Plus)
                        .disabled(busy || operation_disabled)
                        .on_click(move |_event, _window, cx| {
                            stage_panel.update(cx, |panel, cx| {
                                panel.request_file_operation(
                                    WorkingTreeAction::Stage,
                                    false,
                                    stage_file.clone(),
                                    cx,
                                );
                            });
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new(i18n::text(locale, "changes-discard"))
                        .icon(IconName::Undo)
                        .disabled(busy || operation_disabled)
                        .on_click(move |_event, _window, cx| {
                            discard_panel.update(cx, |panel, cx| {
                                panel.request_file_operation(
                                    WorkingTreeAction::Discard,
                                    false,
                                    discard_file.clone(),
                                    cx,
                                );
                            });
                        }),
                )
            }
        })
    }

    fn section(
        &self,
        cx: &Context<Self>,
        staged: bool,
        files: &[FileStatus],
    ) -> impl IntoElement {
        let key = if staged {
            "section-staged"
        } else {
            "section-changes"
        };
        let collapsed = self.is_collapsed(key);
        let rows = files
            .iter()
            .enumerate()
            .map(|(index, file)| self.file_row(cx, staged, index, file))
            .collect::<Vec<_>>();

        v_flex()
            .id(SharedString::from(format!("working-tree-section-{staged}")))
            .w_full()
            .gap_0p5()
            .px_1()
            .child(self.section_header(cx, staged, files))
            .when(!collapsed, |section| section.children(rows))
    }

    fn panel_header(
        &self,
        cx: &Context<Self>,
        total: usize,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        let refresh = Button::new("changes-refresh")
            .icon(lucide("refresh-cw"))
            .ghost()
            .compact()
            .xsmall()
            .tooltip(i18n::text(self.locale, "changes-refresh"))
            .disabled(self.busy)
            .on_click(move |_event, _window, cx| {
                this.update(cx, |panel, cx| {
                    panel.refresh_selected = true;
                    cx.emit(ChangesPanelEvent::RefreshRequested);
                });
            });

        let staged = self.staged.clone();
        let unstaged = self.unstaged.clone();
        let has_conflicts = self.has_conflicts;
        let busy = self.busy;
        let this = cx.entity();
        let locale = self.locale;
        let more = Button::new("changes-more")
            .icon(IconName::Ellipsis)
            .ghost()
            .compact()
            .xsmall()
            .tooltip(i18n::text(locale, "changes-more"))
            .disabled(busy)
            .dropdown_menu_with_anchor(
                Anchor::BottomRight,
                move |menu, _window, _cx| {
                    let stage_panel = this.clone();
                    let unstage_panel = this.clone();
                    let discard_panel = this.clone();
                    let stage_files = unstaged.clone();
                    let unstage_files = staged.clone();
                    let discard_files = unstaged.clone();
                    menu.item(
                        PopupMenuItem::new(i18n::text(
                            locale,
                            "changes-stage-all",
                        ))
                        .icon(IconName::Plus)
                        .disabled(unstaged.is_empty() || has_conflicts)
                        .on_click(
                            move |_event, _window, cx| {
                                stage_panel.update(cx, |panel, cx| {
                                    panel.request_group_operation(
                                        WorkingTreeAction::Stage,
                                        false,
                                        &stage_files,
                                        cx,
                                    );
                                });
                            },
                        ),
                    )
                    .item(
                        PopupMenuItem::new(i18n::text(
                            locale,
                            "changes-unstage-all",
                        ))
                        .icon(IconName::Minus)
                        .disabled(staged.is_empty())
                        .on_click(
                            move |_event, _window, cx| {
                                unstage_panel.update(cx, |panel, cx| {
                                    panel.request_group_operation(
                                        WorkingTreeAction::Unstage,
                                        true,
                                        &unstage_files,
                                        cx,
                                    );
                                });
                            },
                        ),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new(i18n::text(
                            locale,
                            "changes-discard-all",
                        ))
                        .icon(IconName::Undo)
                        .disabled(unstaged.is_empty() || has_conflicts)
                        .on_click(
                            move |_event, _window, cx| {
                                discard_panel.update(cx, |panel, cx| {
                                    panel.request_group_operation(
                                        WorkingTreeAction::Discard,
                                        false,
                                        &discard_files,
                                        cx,
                                    );
                                });
                            },
                        ),
                    )
                },
            );

        h_flex()
            .id("changes-panel-header")
            .w_full()
            .h(px(28.))
            .flex_shrink_0()
            .px_3()
            .items_center()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(self.locale, "changes-title"))),
            )
            .when(self.busy, |header| {
                header
                    .child(Spinner::new().with_size(px(13.)).color(colors.blue))
            })
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(colors.muted_foreground)
                    .child(total.to_string()),
            )
            .child(refresh)
            .child(more)
    }
}

impl Render for ChangesPanel {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let total = self.staged.len() + self.unstaged.len();
        let sections = v_flex()
            .id("working-tree-sections")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .when(!self.staged.is_empty(), |sections| {
                sections.child(self.section(cx, true, &self.staged))
            })
            .when(!self.unstaged.is_empty(), |sections| {
                sections.child(self.section(cx, false, &self.unstaged))
            });

        v_flex()
            .id("changes-panel")
            .size_full()
            .min_h_0()
            .bg(colors.background)
            .child(self.panel_header(cx, total))
            .child(if total == 0 {
                v_flex()
                    .id("changes-empty")
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .child(
                        div()
                            .size(px(22.))
                            .text_color(colors.muted_foreground)
                            .child(Icon::new(IconName::Inbox)),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "changes-empty",
                            ))),
                    )
                    .into_any_element()
            } else {
                sections.into_any_element()
            })
    }
}

fn can_operate(
    action: WorkingTreeAction,
    staged: bool,
    file: &FileStatus,
) -> bool {
    if file.is_conflicted() {
        return false;
    }
    match action {
        WorkingTreeAction::Stage | WorkingTreeAction::Discard => !staged,
        WorkingTreeAction::Unstage => staged,
    }
}

fn status_style(
    colors: &gpui_component::theme::ThemeColor,
    code: char,
    locale: Locale,
) -> (Hsla, String) {
    let color = match code {
        'M' | 'R' | 'C' => colors.warning,
        'A' => colors.green,
        'D' | 'U' => colors.red,
        _ => colors.muted_foreground,
    };
    let key = match code {
        'M' => "status-mod",
        'A' => "status-add",
        'D' => "status-del",
        'R' => "status-ren",
        'C' => "status-cpy",
        'U' => "status-conflict",
        _ => "status-unknown",
    };
    (color, i18n::text(locale, key))
}

fn split_files(files: Vec<FileStatus>) -> (Vec<FileStatus>, Vec<FileStatus>) {
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    for file in files {
        if file.has_staged_changes() {
            staged.push(file.clone());
        }
        if file.is_conflicted() || file.has_worktree_changes() {
            unstaged.push(file);
        }
    }
    (staged, unstaged)
}

#[cfg(test)]
mod tests {
    use super::{can_operate, split_files};
    use crate::core::git::{FileStatus, WorkingTreeAction};

    fn file(index: char, worktree: char, path: &str) -> FileStatus {
        FileStatus {
            index,
            worktree,
            path: path.to_string(),
            old_path: None,
        }
    }

    #[test]
    fn split_files_keeps_mixed_entries_in_both_groups() {
        let (staged, unstaged) = split_files(vec![
            file('M', ' ', "staged.rs"),
            file(' ', 'M', "changed.rs"),
            file('M', 'M', "mixed.rs"),
            file('?', '?', "new.rs"),
        ]);
        assert_eq!(
            staged
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            vec!["staged.rs", "mixed.rs"]
        );
        assert_eq!(
            unstaged
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            vec!["changed.rs", "mixed.rs", "new.rs"]
        );
    }

    #[test]
    fn conflicted_entries_are_only_in_changes_and_not_operable() {
        let conflict = file('U', ' ', "conflict.rs");
        let (staged, unstaged) = split_files(vec![conflict.clone()]);
        assert!(staged.is_empty());
        assert_eq!(unstaged, vec![conflict.clone()]);
        assert!(!can_operate(WorkingTreeAction::Stage, false, &conflict));
        assert!(!can_operate(WorkingTreeAction::Discard, false, &conflict));
    }

    #[test]
    fn row_actions_match_their_group() {
        let staged = file('M', ' ', "staged.rs");
        let changed = file(' ', 'M', "changed.rs");
        assert!(can_operate(WorkingTreeAction::Unstage, true, &staged));
        assert!(!can_operate(WorkingTreeAction::Stage, true, &staged));
        assert!(can_operate(WorkingTreeAction::Stage, false, &changed));
        assert!(can_operate(WorkingTreeAction::Discard, false, &changed));
    }
}
