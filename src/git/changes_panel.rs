//! Working-tree changes shown below the commit editor.

use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, Icon, IconName, h_flex, v_flex};

use crate::core::git::FileStatus;
use crate::core::i18n::{self, Locale};
use crate::git::shared;

/// VS Code-style staged and unstaged file groups for the right panel.
#[derive(Clone, Debug)]
pub enum ChangesPanelEvent {
    /// Request a diff for a file in either the index or working tree.
    FileSelected { staged: bool, file: FileStatus },
}

pub struct ChangesPanel {
    staged: Vec<FileStatus>,
    unstaged: Vec<FileStatus>,
    selected: Option<(bool, usize)>,
    collapsed: Vec<&'static str>,
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
            locale,
        }
    }

    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    pub fn set_files(
        &mut self,
        files: Vec<FileStatus>,
        cx: &mut Context<Self>,
    ) {
        let selected_file = self.selected.and_then(|(staged, index)| {
            let list = if staged { &self.staged } else { &self.unstaged };
            list.get(index).map(|file| (staged, file.path.clone()))
        });

        let (staged, unstaged) = split_files(files);
        self.staged = staged;
        self.unstaged = unstaged;

        self.selected = selected_file.and_then(|(staged, path)| {
            let list = if staged { &self.staged } else { &self.unstaged };
            list.iter()
                .position(|file| file.path == path)
                .map(|index| (staged, index))
                .or_else(|| {
                    let other =
                        if staged { &self.unstaged } else { &self.staged };
                    other
                        .iter()
                        .position(|file| file.path == path)
                        .map(|index| (!staged, index))
                })
        });
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
        self.selected = Some((staged, index));
        cx.emit(ChangesPanelEvent::FileSelected { staged, file });
        cx.notify();
    }

    fn section_header(
        &self,
        cx: &Context<Self>,
        key: &'static str,
        title: String,
        count: usize,
        collapsed: bool,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let this = cx.entity();
        h_flex()
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
                    .child(shared(title)),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(colors.muted_foreground)
                    .child(count.to_string()),
            )
    }

    fn change_section(
        &self,
        cx: &Context<Self>,
        staged: bool,
        files: &[FileStatus],
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let title_key: &'static str = if staged {
            "section-staged"
        } else {
            "section-changes"
        };
        let collapsed = self.is_collapsed(title_key);
        let rows = files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let this = cx.entity();
                let (status_color, status_label) =
                    status_style(&colors, file.code_for(staged), self.locale);
                let selected = self.selected == Some((staged, index));
                h_flex()
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
                    )
            })
            .collect::<Vec<_>>();

        v_flex()
            .id(SharedString::from(format!("working-tree-section-{staged}")))
            .w_full()
            .gap_0p5()
            .px_1()
            .child(self.section_header(
                cx,
                title_key,
                i18n::text(self.locale, title_key),
                files.len(),
                collapsed,
            ))
            .when(!collapsed, |section| section.children(rows))
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
            .child(self.change_section(cx, true, &self.staged))
            .child(self.change_section(cx, false, &self.unstaged));

        v_flex()
            .id("changes-panel")
            .size_full()
            .min_h_0()
            .bg(colors.background)
            .child(
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
                            .child(shared(i18n::text(
                                self.locale,
                                "changes-title",
                            ))),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(colors.muted_foreground)
                            .child(total.to_string()),
                    ),
            )
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
        if file.has_worktree_changes() {
            unstaged.push(file);
        }
    }
    (staged, unstaged)
}

#[cfg(test)]
mod tests {
    use super::split_files;
    use crate::core::git::FileStatus;

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
    fn split_files_uses_group_specific_status_codes() {
        let mixed = file('A', 'D', "mixed.rs");
        assert_eq!(mixed.code_for(true), 'A');
        assert_eq!(mixed.code_for(false), 'D');
    }
}
