//! Sidebar sections for branches and repository refs.
//!
//! Section headers toggle their contents, while checkoutable refs expose
//! actions through a full-row context menu.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, WindowExt,
    dialog::DialogButtonProps,
    h_flex,
    menu::{ContextMenuExt, PopupMenuItem},
    theme::ThemeColor,
    v_flex,
};

use crate::core::git::{BranchInfo, CheckoutTarget, RefsInfo};
use crate::core::i18n::{self, Locale};
use crate::git::shared;

/// Sidebar events routed to Workspace.
pub enum SidebarEvent {
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
    /// Disable checkout actions while a repository operation is running.
    busy: bool,
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
                .on_click(move |event, window, cx| {
                    if event.click_count() >= 2 {
                        if !is_head {
                            request_checkout(
                                &sidebar_for_click,
                                locale,
                                CheckoutTarget::LocalBranch(
                                    name_for_click.clone(),
                                ),
                                window,
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
                    name,
                    "context-copy-branch",
                    self.busy || is_head,
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
                .on_click(move |event, window, cx| {
                    if event.click_count() < 2 {
                        return;
                    }
                    request_checkout(
                        &sidebar_for_click,
                        locale,
                        kind.target(name_for_click.clone()),
                        window,
                        cx,
                    );
                });

                ref_context_menu(
                    row,
                    locale,
                    sidebar.clone(),
                    kind.target(name.clone()),
                    name,
                    kind.copy_label_key(),
                    self.busy,
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

fn checkout_display_name(target: &CheckoutTarget) -> &str {
    match target {
        CheckoutTarget::LocalBranch(name)
        | CheckoutTarget::RemoteBranch(name)
        | CheckoutTarget::Tag(name)
        | CheckoutTarget::Commit(name) => name,
    }
}

/// Ask for confirmation before checking out `target`.
///
/// The checkout event is emitted only from the dialog's OK action (button or
/// Enter), so cancelling leaves the repository untouched.
fn request_checkout(
    sidebar: &Entity<Sidebar>,
    locale: Locale,
    target: CheckoutTarget,
    window: &mut Window,
    cx: &mut App,
) {
    if window.has_active_dialog(cx) || sidebar.read(cx).busy {
        return;
    }

    let name = checkout_display_name(&target);
    let title = i18n::text_args(locale, "checkout-title", &[("name", name)]);
    let description =
        i18n::text_args(locale, "checkout-description", &[("name", name)]);
    let ok_label = i18n::text(locale, "context-checkout");
    let cancel_label = i18n::text(locale, "checkout-cancel");
    let sidebar = sidebar.downgrade();

    window.open_alert_dialog(cx, move |alert, _window, _cx| {
        // The dialog builder is `Fn`, so clone the captures per invocation.
        let sidebar = sidebar.clone();
        let target = target.clone();
        alert
            .title(title.clone())
            .description(description.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text(ok_label.clone())
                    .cancel_text(cancel_label.clone())
                    .show_cancel(true)
                    .on_ok(move |_event, _window, cx| {
                        if let Some(sidebar) = sidebar.upgrade() {
                            sidebar.update(cx, |_sidebar, cx| {
                                cx.emit(SidebarEvent::CheckoutRef(
                                    target.clone(),
                                ))
                            });
                        }
                        true
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
