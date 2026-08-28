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
    v_flex,
};

use crate::core::git::{BranchInfo, CheckoutTarget, RefsInfo};
use crate::core::i18n::{self, Locale};
use crate::git::shared;

/// Sidebar events routed to Workspace.
pub enum SidebarEvent {
    /// Collapse or expand the sidebar.
    ToggleCollapse,
    /// Select a branch in the repository navigator.
    BranchSelected(String),
    /// Check out a branch, tag, or commit target.
    CheckoutRef(CheckoutTarget),
    /// Copy a displayed ref name to the system clipboard.
    CopyRef(String),
}

pub struct Sidebar {
    /// Local branch list.
    branches: Vec<BranchInfo>,
    /// Current branch.
    branch: String,
    /// Read-only remotes, remote branches, tags, and stashes.
    refs: RefsInfo,
    /// Branch highlight expiration after the toolbar branch action.
    flash_branches_until: Option<Instant>,
    /// Collapsed section keys (in-memory only).
    collapsed: Vec<&'static str>,
    /// UI locale synchronized by Workspace.
    locale: Locale,
}

#[derive(Clone, Copy)]
enum CheckoutableRefKind {
    RemoteBranch,
    Tag,
}

impl CheckoutableRefKind {
    fn target(self, name: String) -> CheckoutTarget {
        match self {
            Self::RemoteBranch => CheckoutTarget::RemoteBranch(name),
            Self::Tag => CheckoutTarget::Tag(name),
        }
    }

    fn copy_label_key(self) -> &'static str {
        match self {
            Self::RemoteBranch => "context-copy-branch",
            Self::Tag => "context-copy-tag",
        }
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
            flash_branches_until: None,
            collapsed: Vec::new(),
            locale,
        }
    }

    /// Synchronize the UI locale.
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
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
        self.refs = refs;
        cx.notify();
    }

    /// Expand the branch section and highlight it briefly.
    pub fn flash_branches(&mut self, cx: &mut Context<Self>) {
        self.flash_branches_until =
            Some(Instant::now() + Duration::from_millis(800));
        self.collapsed.retain(|k| *k != "section-branches");
        cx.notify();
    }

    fn is_collapsed(&self, key: &str) -> bool {
        self.collapsed.iter().any(|k| *k == key)
    }

    /// Toggle a section when its header is clicked.
    fn toggle_section(&mut self, key: &'static str, cx: &mut Context<Self>) {
        match self.collapsed.iter().position(|k| *k == key) {
            Some(i) => {
                self.collapsed.remove(i);
            }
            None => self.collapsed.push(key),
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

        // Collapse button.
        let btn_collapse = cx.entity();
        let collapse_btn = div()
            .id("btn-collapse")
            .p_1()
            .rounded_md()
            .hover(|this| this.bg(colors.input))
            .text_size(px(12.))
            .text_color(colors.muted_foreground)
            .child(Icon::new(IconName::PanelLeftClose))
            .on_click(move |_e, _w, cx| {
                btn_collapse.update(cx, |_sidebar, cx| {
                    cx.emit(SidebarEvent::ToggleCollapse)
                });
            });

        v_flex()
            .id("sidebar")
            .w_full()
            .h_full()
            .bg(colors.background)
            // Keep only the collapse button in this compact top row.
            .child(
                v_flex().w_full().p_1().child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .child(div().flex_1())
                        .child(collapse_btn),
                ),
            )
            .child(
                v_flex()
                    .id("sidebar-sections")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.branch_section(cx))
                    .child(self.list_section(
                        cx,
                        "section-remotes",
                        &self.refs.remotes,
                    ))
                    .child(self.checkoutable_list_section(
                        cx,
                        "section-remote-branches",
                        &self.refs.remote_branches,
                        CheckoutableRefKind::RemoteBranch,
                    ))
                    .child(self.checkoutable_list_section(
                        cx,
                        "section-tags",
                        &self.refs.tags,
                        CheckoutableRefKind::Tag,
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
        let rows = self
            .branches
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let name = b.name.clone();
                let is_head = b.is_head;
                let sidebar_for_click = sidebar.clone();
                let row = h_flex()
                    .id(SharedString::from(format!("branch-{name}")))
                    .w_full()
                    .h(px(22.))
                    .flex_shrink_0()
                    .px_2()
                    .gap_1()
                    .items_center()
                    .rounded_sm()
                    .hover(|this| this.bg(colors.list_hover))
                    .on_click(move |_e, _w, cx| {
                        sidebar_for_click.update(cx, |sidebar, cx| {
                            sidebar.select_branch(i, cx)
                        });
                    })
                    .child(
                        div()
                            .w(px(14.))
                            .text_size(px(12.))
                            .text_color(if is_head {
                                colors.green
                            } else {
                                colors.muted_foreground
                            })
                            .child(if is_head { "●" } else { "○" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(if is_head {
                                colors.foreground
                            } else {
                                colors.muted_foreground
                            })
                            .child(shared(name.clone())),
                    );

                ref_context_menu(
                    row,
                    self.locale,
                    sidebar.clone(),
                    CheckoutTarget::LocalBranch(name.clone()),
                    name,
                    "context-copy-branch",
                    is_head,
                )
            })
            .collect::<Vec<_>>();

        v_flex()
            .id("branch-section")
            .w_full()
            .gap_0p5()
            .p_2()
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

    /// Read-only list section for remotes and stashes.
    fn list_section(
        &self,
        cx: &Context<Self>,
        key: &'static str,
        items: &[String],
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let rows = items
            .iter()
            .map(|item| {
                h_flex()
                    .id(SharedString::from(format!("{key}-{item}")))
                    .w_full()
                    .h(px(22.))
                    .flex_shrink_0()
                    .px_2()
                    .rounded_sm()
                    .hover(|this| this.bg(colors.list_hover))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(colors.muted_foreground)
                            .truncate()
                            .child(shared(item.clone())),
                    )
            })
            .collect::<Vec<_>>();

        let collapsed = self.is_collapsed(key);
        v_flex()
            .id(SharedString::from(format!("list-{key}")))
            .w_full()
            .gap_0p5()
            .p_2()
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

    /// List section for refs that support checkout and copy actions.
    fn checkoutable_list_section(
        &self,
        cx: &Context<Self>,
        key: &'static str,
        items: &[String],
        kind: CheckoutableRefKind,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let sidebar = cx.entity();
        let rows = items
            .iter()
            .map(|item| {
                let name = item.clone();
                let row = h_flex()
                    .id(SharedString::from(format!("{key}-{name}")))
                    .w_full()
                    .h(px(22.))
                    .flex_shrink_0()
                    .px_2()
                    .rounded_sm()
                    .hover(|this| this.bg(colors.list_hover))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(colors.muted_foreground)
                            .truncate()
                            .child(shared(name.clone())),
                    );

                ref_context_menu(
                    row,
                    self.locale,
                    sidebar.clone(),
                    kind.target(name.clone()),
                    name,
                    kind.copy_label_key(),
                    false,
                )
            })
            .collect::<Vec<_>>();

        let collapsed = self.is_collapsed(key);
        v_flex()
            .id(SharedString::from(format!("list-{key}")))
            .w_full()
            .gap_0p5()
            .p_2()
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

fn ref_context_menu<E>(
    element: E,
    locale: Locale,
    sidebar: Entity<Sidebar>,
    target: CheckoutTarget,
    copy_value: String,
    copy_label_key: &'static str,
    checkout_disabled: bool,
) -> impl IntoElement
where
    E: InteractiveElement + ParentElement + Styled + IntoElement + 'static,
{
    let checkout_label = i18n::text(locale, "context-checkout");
    let copy_label = i18n::text(locale, copy_label_key);

    element.context_menu(move |menu, _window, _cx| {
        let sidebar_for_checkout = sidebar.clone();
        let sidebar_for_copy = sidebar.clone();
        let target = target.clone();
        let copy_value = copy_value.clone();

        menu.item(
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
                        cx.emit(SidebarEvent::CopyRef(copy_value.clone()));
                    });
                }),
        )
    })
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
    key: &'static str,
    title: String,
    count: usize,
    collapsed: bool,
    flash: bool,
) -> impl IntoElement {
    let colors = cx.theme().colors.clone();
    let this = cx.entity();
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
            this.update(cx, |sidebar, cx| sidebar.toggle_section(key, cx));
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
