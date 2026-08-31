//! Sidebar sections for branches and repository refs.
//!
//! Section headers toggle their contents, while checkoutable refs expose
//! actions through a full-row context menu.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenuItem},
    theme::ThemeColor,
    v_flex,
};

use crate::core::git::{BranchInfo, CheckoutTarget, RefsInfo};
use crate::core::i18n::{self, Locale};
use crate::core::refs::{RemoteBranchGroup, group_remote_branches};
use crate::git::shared;

/// Sidebar events routed to Workspace.
pub enum SidebarEvent {
    /// Select a branch in the repository navigator.
    BranchSelected(String),
    /// Check out a branch, tag, or commit target.
    CheckoutRef(CheckoutTarget),
    /// Copy a displayed ref name to the system clipboard.
    CopyRef(String),
    /// Rename a local branch (its current name is carried as payload).
    RenameBranch(String),
    /// Delete a local branch.
    DeleteBranch(String),
    /// Delete a tag.
    DeleteTag(String),
    /// Merge a local branch into the current branch.
    MergeIntoCurrent { name: String, no_ff: bool },
}

pub struct Sidebar {
    /// Local branch list.
    branches: Vec<BranchInfo>,
    /// Current branch.
    branch: String,
    /// Read-only remote branches, tags, and stashes.
    refs: RefsInfo,
    /// Remote branches grouped per remote for the tree section.
    remote_groups: Vec<RemoteBranchGroup>,
    /// Branch highlight expiration after the toolbar branch action.
    flash_branches_until: Option<Instant>,
    /// Collapsed section and remote-group keys (in-memory only).
    collapsed: Vec<String>,
    /// UI locale synchronized by Workspace.
    locale: Locale,
    /// Disable checkout actions while a repository operation is running.
    busy: bool,
}

/// Extra context-menu actions offered per ref type. Local branches support
/// rename, delete, and merging into the current branch; tags support delete.
#[derive(Clone, Copy)]
enum RefActions {
    LocalBranch { is_head: bool },
    Tag,
    RemoteBranch,
}

impl RefActions {
    fn is_head(self) -> bool {
        matches!(self, Self::LocalBranch { is_head: true })
    }
}

impl EventEmitter<SidebarEvent> for Sidebar {}

impl Sidebar {
    pub fn new(
        _window: &mut Window,
        _cx: &mut Context<Self>,
        locale: Locale,
    ) -> Self {
        Self {
            branches: Vec::new(),
            branch: String::new(),
            refs: RefsInfo::default(),
            remote_groups: Vec::new(),
            flash_branches_until: None,
            collapsed: Vec::new(),
            locale,
            busy: false,
        }
    }

    /// Synchronize the UI locale.
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    /// Disable checkout actions while another repository operation is active.
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy != busy {
            self.busy = busy;
            cx.notify();
        }
    }

    /// Apply the repository branch snapshot.
    pub fn set_status(
        &mut self,
        branch: String,
        branches: Vec<BranchInfo>,
        cx: &mut Context<Self>,
    ) {
        self.branch = branch;
        self.branches = branches;
        cx.notify();
    }

    /// Apply the read-only refs snapshot.
    pub fn set_refs(&mut self, refs: RefsInfo, cx: &mut Context<Self>) {
        self.remote_groups =
            group_remote_branches(&refs.remotes, &refs.remote_branches);
        self.refs = refs;
        cx.notify();
    }

    /// Expand the branch section and highlight it briefly.
    pub fn flash_branches(&mut self, cx: &mut Context<Self>) {
        self.flash_branches_until =
            Some(Instant::now() + Duration::from_millis(800));
        self.collapsed.retain(|k| k != "section-branches");
        cx.notify();
    }

    fn is_collapsed(&self, key: &str) -> bool {
        self.collapsed.iter().any(|k| k == key)
    }

    /// Toggle a section or remote group when its header is clicked.
    fn toggle_section(&mut self, key: &str, cx: &mut Context<Self>) {
        match self.collapsed.iter().position(|k| k == key) {
            Some(i) => {
                self.collapsed.remove(i);
            }
            None => self.collapsed.push(key.to_string()),
        }
        cx.notify();
    }

    fn select_branch(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(branch) = self.branches.get(index) else {
            return;
        };
        cx.emit(SidebarEvent::BranchSelected(branch.name.clone()));
    }

    fn sidebar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();

        // Panel title header, styled after the right panel headers.
        v_flex()
            .id("sidebar")
            .w_full()
            .h_full()
            .bg(colors.background)
            .child(
                h_flex()
                    .id("sidebar-header")
                    .w_full()
                    .h(px(30.))
                    .flex_shrink_0()
                    .px_3()
                    .items_center()
                    .bg(colors.tab_bar)
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(colors.foreground)
                            .child(shared(i18n::text(
                                self.locale,
                                "sidebar-repo",
                            ))),
                    ),
            )
            .child(
                v_flex()
                    .id("sidebar-sections")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_1()
                    .pb_2()
                    .child(self.branch_section(cx))
                    .child(self.remote_branches_section(cx))
                    .child(self.tag_list_section(
                        cx,
                        "section-tags",
                        &self.refs.tags,
                    ))
                    .child(self.list_section(
                        cx,
                        "section-stashes",
                        &self.refs.stashes,
                    )),
            )
    }

    /// Local branch section.
    fn branch_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        // Expired timestamps are treated as inactive without notifying during render.
        let flash = self
            .flash_branches_until
            .map(|until| Instant::now() < until)
            .unwrap_or(false);
        let sidebar = cx.entity();
        let locale = self.locale;
        let rows = self
            .branches
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let name = b.name.clone();
                let is_head = b.is_head;
                let sidebar_for_click = sidebar.clone();
                let name_for_click = name.clone();
                let row = ref_row(
                    &colors,
                    SharedString::from(format!("branch-{name}")),
                )
                .child(ref_marker(&colors, is_head))
                .child(ref_label(&colors, name.clone(), is_head))
                .on_click(move |event, _window, cx| {
                    if event.click_count() >= 2 {
                        if !is_head {
                            emit_checkout(
                                &sidebar_for_click,
                                CheckoutTarget::LocalBranch(
                                    name_for_click.clone(),
                                ),
                                cx,
                            );
                        }
                        return;
                    }
                    sidebar_for_click
                        .update(cx, |sidebar, cx| sidebar.select_branch(i, cx));
                });

                ref_context_menu(
                    row,
                    locale,
                    sidebar.clone(),
                    CheckoutTarget::LocalBranch(name.clone()),
                    name.clone(),
                    "context-copy-branch",
                    self.busy,
                    RefActions::LocalBranch { is_head },
                )
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("branch-section")
            .w_full()
            .gap_0p5()
            .px_2()
            .child(section_header(
                cx,
                "section-branches",
                i18n::text(self.locale, "section-branches"),
                self.branches.len(),
                self.is_collapsed("section-branches"),
                flash,
            ))
            .when(!self.is_collapsed("section-branches"), |s| s.children(rows))
    }

    /// Read-only list section for stashes.
    fn list_section(
        &self,
        cx: &Context<Self>,
        key: &str,
        items: &[String],
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let rows = items
            .iter()
            .map(|item| {
                ref_row(&colors, SharedString::from(format!("{key}-{item}")))
                    .child(ref_marker(&colors, false))
                    .child(ref_label(&colors, item.clone(), false))
            })
            .collect::<Vec<_>>();

        let collapsed = self.is_collapsed(key);
        v_flex()
            .id(SharedString::from(format!("list-{key}")))
            .w_full()
            .gap_0p5()
            .px_2()
            .child(section_header(
                cx,
                key,
                i18n::text(self.locale, key),
                items.len(),
                collapsed,
                false,
            ))
            .when(!collapsed, |s| s.children(rows))
    }

    /// Remote branches rendered as a tree: one collapsible node per remote
    /// with its tracking branches nested underneath.
    fn remote_branches_section(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let sidebar = cx.entity();
        let locale = self.locale;
        let total: usize =
            self.remote_groups.iter().map(|g| g.branches.len()).sum();
        let groups = self
            .remote_groups
            .iter()
            .map(|group| {
                let key = format!("remote-group/{}", group.remote);
                let group_collapsed = self.is_collapsed(&key);
                let sidebar_for_toggle = sidebar.clone();
                let key_for_toggle = key.clone();
                let rows = group
                    .branches
                    .iter()
                    .map(|entry| {
                        let sidebar_for_click = sidebar.clone();
                        let name_for_click = entry.full_name.clone();
                        let row = ref_row(
                            &colors,
                            SharedString::from(format!(
                                "remote-branch-{}",
                                entry.full_name
                            )),
                        )
                        .pl_6()
                        .child(ref_marker(&colors, false))
                        .child(ref_label(&colors, entry.label.clone(), false))
                        .on_click(
                            move |event, _window, cx| {
                                if event.click_count() < 2 {
                                    return;
                                }
                                emit_checkout(
                                    &sidebar_for_click,
                                    CheckoutTarget::RemoteBranch(
                                        name_for_click.clone(),
                                    ),
                                    cx,
                                );
                            },
                        );

                        ref_context_menu(
                            row,
                            locale,
                            sidebar.clone(),
                            CheckoutTarget::RemoteBranch(
                                entry.full_name.clone(),
                            ),
                            entry.full_name.clone(),
                            "context-copy-branch",
                            self.busy,
                            RefActions::RemoteBranch,
                        )
                    })
                    .collect::<Vec<_>>();

                v_flex()
                    .id(SharedString::from(format!("tree-{key}")))
                    .w_full()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .id(SharedString::from(key))
                            .w_full()
                            .h(px(22.))
                            .flex_shrink_0()
                            .px_2()
                            .gap_1()
                            .items_center()
                            .rounded_sm()
                            .cursor(CursorStyle::PointingHand)
                            .hover(|this| this.bg(colors.list_hover))
                            .on_click(move |_e, _w, cx| {
                                sidebar_for_toggle.update(cx, |sidebar, cx| {
                                    sidebar.toggle_section(&key_for_toggle, cx);
                                });
                            })
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(colors.muted_foreground)
                                    .child(if group_collapsed {
                                        Icon::new(IconName::ChevronRight)
                                    } else {
                                        Icon::new(IconName::ChevronDown)
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(colors.muted_foreground)
                                    .child(crate::git::lucide("git-branch")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .text_color(colors.foreground)
                                    .truncate()
                                    .child(shared(group.remote.clone())),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(colors.muted_foreground)
                                    .child(group.branches.len().to_string()),
                            ),
                    )
                    .when(!group_collapsed, |s| s.children(rows))
            })
            .collect::<Vec<_>>();

        let collapsed = self.is_collapsed("section-remote-branches");
        v_flex()
            .id("list-section-remote-branches")
            .w_full()
            .gap_0p5()
            .px_2()
            .child(section_header(
                cx,
                "section-remote-branches",
                i18n::text(self.locale, "section-remote-branches"),
                total,
                collapsed,
                false,
            ))
            .when(!collapsed, |s| s.children(groups))
    }

    /// Tag list section with checkout and copy actions.
    fn tag_list_section(
        &self,
        cx: &Context<Self>,
        key: &str,
        items: &[String],
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let sidebar = cx.entity();
        let locale = self.locale;
        let rows = items
            .iter()
            .map(|item| {
                let name = item.clone();
                let sidebar_for_click = sidebar.clone();
                let name_for_click = name.clone();
                let row = ref_row(
                    &colors,
                    SharedString::from(format!("{key}-{name}")),
                )
                .child(ref_marker(&colors, false))
                .child(ref_label(&colors, name.clone(), false))
                .on_click(move |event, _window, cx| {
                    if event.click_count() < 2 {
                        return;
                    }
                    emit_checkout(
                        &sidebar_for_click,
                        CheckoutTarget::Tag(name_for_click.clone()),
                        cx,
                    );
                });

                ref_context_menu(
                    row,
                    locale,
                    sidebar.clone(),
                    CheckoutTarget::Tag(name.clone()),
                    name,
                    "context-copy-tag",
                    self.busy,
                    RefActions::Tag,
                )
            })
            .collect::<Vec<_>>();

        let collapsed = self.is_collapsed(key);
        v_flex()
            .id(SharedString::from(format!("list-{key}")))
            .w_full()
            .gap_0p5()
            .px_2()
            .child(section_header(
                cx,
                key,
                i18n::text(self.locale, key),
                items.len(),
                collapsed,
                false,
            ))
            .when(!collapsed, |s| s.children(rows))
    }
}

#[allow(clippy::too_many_arguments)]
fn ref_context_menu<E>(
    element: E,
    locale: Locale,
    sidebar: Entity<Sidebar>,
    target: CheckoutTarget,
    copy_value: String,
    copy_label_key: &'static str,
    busy: bool,
    actions: RefActions,
) -> impl IntoElement
where
    E: InteractiveElement + ParentElement + Styled + IntoElement + 'static,
{
    let checkout_label = i18n::text(locale, "context-checkout");
    let copy_label = i18n::text(locale, copy_label_key);
    let checkout_disabled = busy || actions.is_head();

    element.context_menu(move |menu, _window, _cx| {
        let sidebar_for_checkout = sidebar.clone();
        let sidebar_for_copy = sidebar.clone();
        let target = target.clone();
        let copy_value_for_copy = copy_value.clone();

        let menu = menu
            .item(
                PopupMenuItem::new(checkout_label.clone())
                    .icon(crate::git::lucide("git-branch"))
                    .disabled(checkout_disabled)
                    .on_click(move |_event, _window, cx| {
                        sidebar_for_checkout.update(cx, |_sidebar, cx| {
                            cx.emit(SidebarEvent::CheckoutRef(target.clone()));
                        });
                    }),
            )
            .item(
                PopupMenuItem::new(copy_label.clone())
                    .icon(IconName::Copy)
                    .on_click(move |_event, _window, cx| {
                        sidebar_for_copy.update(cx, |_sidebar, cx| {
                            cx.emit(SidebarEvent::CopyRef(
                                copy_value_for_copy.clone(),
                            ));
                        });
                    }),
            );

        match actions {
            RefActions::LocalBranch { is_head } => {
                let sidebar_for_rename = sidebar.clone();
                let sidebar_for_delete = sidebar.clone();
                let sidebar_for_merge = sidebar.clone();
                let sidebar_for_merge_no_ff = sidebar.clone();
                let rename_value = copy_value.clone();
                let delete_value = copy_value.clone();
                let merge_value = copy_value.clone();
                let merge_no_ff_value = copy_value.clone();

                menu.separator()
                    .item(
                        PopupMenuItem::new(i18n::text(
                            locale,
                            "context-rename",
                        ))
                        .icon(crate::git::lucide("pencil"))
                        .disabled(busy)
                        .on_click(
                            move |_event, _window, cx| {
                                sidebar_for_rename.update(
                                    cx,
                                    |_sidebar, cx| {
                                        cx.emit(SidebarEvent::RenameBranch(
                                            rename_value.clone(),
                                        ));
                                    },
                                );
                            },
                        ),
                    )
                    .item(
                        PopupMenuItem::new(i18n::text(
                            locale,
                            "context-delete",
                        ))
                        .icon(crate::git::lucide("trash-2"))
                        .disabled(busy || is_head)
                        .on_click(
                            move |_event, _window, cx| {
                                sidebar_for_delete.update(
                                    cx,
                                    |_sidebar, cx| {
                                        cx.emit(SidebarEvent::DeleteBranch(
                                            delete_value.clone(),
                                        ));
                                    },
                                );
                            },
                        ),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new(i18n::text(
                            locale,
                            "context-merge-into-current",
                        ))
                        .icon(crate::git::lucide("git-merge"))
                        .disabled(busy || is_head)
                        .on_click(
                            move |_event, _window, cx| {
                                sidebar_for_merge.update(cx, |_sidebar, cx| {
                                    cx.emit(SidebarEvent::MergeIntoCurrent {
                                        name: merge_value.clone(),
                                        no_ff: false,
                                    });
                                });
                            },
                        ),
                    )
                    .item(
                        PopupMenuItem::new(i18n::text(
                            locale,
                            "context-merge-no-ff-into-current",
                        ))
                        .icon(crate::git::lucide("git-merge"))
                        .disabled(busy || is_head)
                        .on_click(
                            move |_event, _window, cx| {
                                sidebar_for_merge_no_ff.update(
                                    cx,
                                    |_sidebar, cx| {
                                        cx.emit(
                                            SidebarEvent::MergeIntoCurrent {
                                                name: merge_no_ff_value.clone(),
                                                no_ff: true,
                                            },
                                        );
                                    },
                                );
                            },
                        ),
                    )
            }
            RefActions::Tag => {
                let sidebar_for_delete = sidebar.clone();
                let delete_value = copy_value.clone();
                menu.separator().item(
                    PopupMenuItem::new(i18n::text(locale, "context-delete"))
                        .icon(crate::git::lucide("trash-2"))
                        .disabled(busy)
                        .on_click(move |_event, _window, cx| {
                            sidebar_for_delete.update(cx, |_sidebar, cx| {
                                cx.emit(SidebarEvent::DeleteTag(
                                    delete_value.clone(),
                                ));
                            });
                        }),
                )
            }
            RefActions::RemoteBranch => menu,
        }
    })
}

/// Shared frame for every sidebar list row.
fn ref_row(colors: &ThemeColor, id: SharedString) -> Stateful<Div> {
    let hover = colors.list_hover;
    h_flex()
        .id(id)
        .w_full()
        .h(px(22.))
        .flex_shrink_0()
        .px_2()
        .gap_1()
        .items_center()
        .rounded_sm()
        .hover(move |this| this.bg(hover))
}

/// Leading status marker for a ref row: a filled dot for the checked-out
/// ref, a hollow ring otherwise.
fn ref_marker(colors: &ThemeColor, is_head: bool) -> Div {
    let dot = div()
        .size(px(8.))
        .flex_shrink_0()
        .rounded_full()
        .map(|dot| {
            if is_head {
                dot.bg(colors.green)
            } else {
                dot.border_1().border_color(colors.foreground)
            }
        });
    h_flex()
        .w(px(14.))
        .flex_shrink_0()
        .justify_center()
        .child(dot)
}

/// Truncated row label; the checked-out row uses the app-wide SEMIBOLD
/// emphasis.
fn ref_label(colors: &ThemeColor, text: String, is_head: bool) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .text_size(px(12.))
        .text_color(colors.foreground)
        .map(|label| {
            if is_head {
                label.font_weight(FontWeight::SEMIBOLD)
            } else {
                label
            }
        })
        .truncate()
        .child(shared(text))
}

/// Check out `target` immediately, honouring the busy state.
///
/// The branch/tag rows emit the checkout event directly on double-click,
/// without a confirmation dialog.
fn emit_checkout(
    sidebar: &Entity<Sidebar>,
    target: CheckoutTarget,
    cx: &mut App,
) {
    sidebar.update(cx, |sidebar, cx| {
        if sidebar.busy {
            return;
        }
        cx.emit(SidebarEvent::CheckoutRef(target));
    });
}

impl Render for Sidebar {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.sidebar(window, cx)
    }
}

/// Section header with a chevron, title, and item count.
fn section_header(
    cx: &Context<Sidebar>,
    key: &str,
    title: String,
    count: usize,
    collapsed: bool,
    flash: bool,
) -> impl IntoElement {
    let colors = cx.theme().colors.clone();
    let this = cx.entity();
    let key_for_click = key.to_string();
    h_flex()
        .id(SharedString::from(format!("section-{key}")))
        .w_full()
        .px_2()
        .py_0p5()
        .rounded_md()
        .bg(if flash {
            colors.list_active
        } else {
            colors.background
        })
        .items_center()
        .gap_1()
        .cursor(CursorStyle::PointingHand)
        .hover(|this| this.bg(colors.list_hover))
        .on_click(move |_e, _w, cx| {
            this.update(cx, |sidebar, cx| {
                sidebar.toggle_section(&key_for_click, cx);
            });
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
                .text_size(px(13.))
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
