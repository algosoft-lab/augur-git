//! M1: Toolbar controls (mirrors rgitui toolbar.rs; M1.5 iconified ghost style).
//!
//! Left group: Branch menu (new/rename/stash/merge/rebase) + Fetch/Pull/Push.
//! + ahead/behind badges + Compare.
//! Right group: busy spinner / refresh / settings.
//! Actions flow through ToolbarEvent to Workspace → GitCommand (Git runs in the background).

use gpui::prelude::*;
use gpui::*;
use gpui_component::spinner::Spinner;
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::PopupMenuItem,
};

use crate::core::i18n::{self, Locale};
use crate::dropdown::DropdownMenuExt;
use crate::git::{lucide, shared};

/// Availability of Branch menu entries, synchronized from repository state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BranchMenuContext {
    /// Whether a current branch exists (required for rename).
    pub can_rename: bool,
    /// Whether at least one other local branch exists (required for merge/rebase).
    pub can_integrate: bool,
    /// Whether the working tree has changes that can be stashed.
    pub can_stash: bool,
    /// Number of stash entries (required for stash pop).
    pub stash_count: usize,
    /// Whether unresolved merge conflicts block integration and pull actions.
    pub has_conflicts: bool,
}

/// Toolbar-to-Workspace events.
#[derive(Clone, Debug)]
pub enum ToolbarEvent {
    BranchNew,
    BranchRename,
    Stash,
    StashPop,
    ApplyPatch,
    Merge { no_ff: bool },
    Rebase,
    Fetch,
    PullMerge,
    PullRebase,
    Push,
    PushForce,
    Compare,
    Extensions,
    Refresh,
    Settings,
}

pub struct Toolbar {
    ahead: usize,
    behind: usize,
    /// Whether a remote exists (fetch/pull/push are disabled without one).
    has_remote: bool,
    /// Whether an operation is in progress (right-side spinner).
    busy: bool,
    /// Branch menu entry availability.
    branch_ctx: BranchMenuContext,
    /// Whether unresolved merge conflicts block pull actions.
    has_conflicts: bool,
    /// UI locale, synchronized when Workspace changes language.
    locale: Locale,
}

impl EventEmitter<ToolbarEvent> for Toolbar {}

impl Toolbar {
    pub fn new(locale: Locale) -> Self {
        Self {
            ahead: 0,
            behind: 0,
            has_remote: true,
            busy: false,
            branch_ctx: BranchMenuContext::default(),
            has_conflicts: false,
            locale,
        }
    }

    /// Change the locale, synchronized by Workspace::set_language.
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    /// Synchronize Branch menu availability after repository status/ref refreshes.
    pub fn set_branch_context(
        &mut self,
        ctx: BranchMenuContext,
        cx: &mut Context<Self>,
    ) {
        if self.branch_ctx == ctx {
            return;
        }
        self.branch_ctx = ctx;
        cx.notify();
    }

    pub fn set_ahead_behind(
        &mut self,
        ahead: usize,
        behind: usize,
        cx: &mut Context<Self>,
    ) {
        if self.ahead == ahead && self.behind == behind {
            return;
        }
        self.ahead = ahead;
        self.behind = behind;
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy != busy {
            self.busy = busy;
            cx.notify();
        }
    }

    /// Synchronize whether unresolved merge conflicts block pull actions.
    pub fn set_conflicts(
        &mut self,
        has_conflicts: bool,
        cx: &mut Context<Self>,
    ) {
        if self.has_conflicts != has_conflicts {
            self.has_conflicts = has_conflicts;
            cx.notify();
        }
    }

    /// Toolbar button: 14px icon + 12px text in ghost style (no resting fill; hover highlight).
    fn tool_button(
        &self,
        id: &'static str,
        icon: Icon,
        label_key: &str,
        colors: &gpui_component::theme::ThemeColor,
        enabled: bool,
        event: ToolbarEvent,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let this = cx.entity();
        div()
            .id(id)
            .px_2()
            .py_0p5()
            .rounded_md()
            .opacity(if enabled { 1.0 } else { 0.45 })
            .hover(|this| this.bg(colors.list_hover))
            .text_size(crate::theme::scaled_text_size(12.))
            .text_color(colors.foreground)
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(14.))
                            .text_color(colors.muted_foreground)
                            .child(icon),
                    )
                    .child(shared(i18n::text(self.locale, label_key))),
            )
            .when(enabled, |el| {
                el.on_click(move |_e, _w, cx| {
                    this.update(cx, |_toolbar, cx| cx.emit(event.clone()));
                })
            })
    }

    /// Branch dropdown button (first position in the toolbar):
    /// new/rename branch, stash/stash pop, merge/merge --no-ff, rebase.
    ///
    /// The icon and label are passed as children styled like `tool_button`
    /// (14px muted icon, 12px foreground label). The built-in `icon`/`label`
    /// accessors would render a 14px label with full-strength foreground,
    /// which looks larger and bolder than the surrounding tool buttons.
    fn branch_menu_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let locale = self.locale;
        let ctx = self.branch_ctx;
        let this = cx.entity();
        let colors = cx.theme().colors.clone();
        let label = i18n::text(locale, "toolbar-branch");

        Button::new("tb-branch")
            .ghost()
            .small()
            .disabled(self.busy)
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(14.))
                    .text_color(colors.muted_foreground)
                    .child(lucide("git-branch")),
            )
            .child(
                div()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .child(shared(label)),
            )
            .dropdown_menu_below(move |menu, _, _| {
                let this = this.clone();
                let new_item = this.clone();
                let rename_item = this.clone();
                let stash_item = this.clone();
                let pop_item = this.clone();
                let merge_item = this.clone();
                let merge_ff_item = this.clone();
                let rebase_item = this.clone();
                let apply_item = this.clone();

                menu.item(
                    PopupMenuItem::new(i18n::text(locale, "menu-branch-new"))
                        .icon(lucide("git-branch-plus"))
                        .disabled(ctx.has_conflicts)
                        .on_click(move |_e, _w, cx| {
                            new_item.update(cx, |_t, cx| {
                                cx.emit(ToolbarEvent::BranchNew)
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new(i18n::text(
                        locale,
                        "menu-branch-rename",
                    ))
                    .icon(lucide("pencil"))
                    .disabled(!ctx.can_rename)
                    .on_click(move |_e, _w, cx| {
                        rename_item.update(cx, |_t, cx| {
                            cx.emit(ToolbarEvent::BranchRename)
                        });
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new(i18n::text(locale, "menu-stash"))
                        .icon(lucide("archive"))
                        .disabled(!ctx.can_stash)
                        .on_click(move |_e, _w, cx| {
                            stash_item.update(cx, |_t, cx| {
                                cx.emit(ToolbarEvent::Stash)
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new(i18n::text(locale, "menu-stash-pop"))
                        .icon(lucide("archive-restore"))
                        .disabled(ctx.stash_count == 0 || ctx.has_conflicts)
                        .on_click(move |_e, _w, cx| {
                            pop_item.update(cx, |_t, cx| {
                                cx.emit(ToolbarEvent::StashPop)
                            });
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new(i18n::text(locale, "menu-merge"))
                        .icon(lucide("git-merge"))
                        .disabled(!ctx.can_integrate || ctx.has_conflicts)
                        .on_click(move |_e, _w, cx| {
                            merge_item.update(cx, |_t, cx| {
                                cx.emit(ToolbarEvent::Merge { no_ff: false })
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new(i18n::text(locale, "menu-merge-no-ff"))
                        .icon(lucide("git-merge"))
                        .disabled(!ctx.can_integrate || ctx.has_conflicts)
                        .on_click(move |_e, _w, cx| {
                            merge_ff_item.update(cx, |_t, cx| {
                                cx.emit(ToolbarEvent::Merge { no_ff: true })
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new(i18n::text(locale, "menu-rebase"))
                        .icon(lucide("git-commit-horizontal"))
                        .disabled(!ctx.can_integrate || ctx.has_conflicts)
                        .on_click(move |_e, _w, cx| {
                            rebase_item.update(cx, |_t, cx| {
                                cx.emit(ToolbarEvent::Rebase)
                            });
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new(i18n::text(locale, "menu-apply-patch"))
                        .icon(lucide("upload"))
                        .disabled(ctx.has_conflicts)
                        .on_click(move |_e, _w, cx| {
                            apply_item.update(cx, |_t, cx| {
                                cx.emit(ToolbarEvent::ApplyPatch)
                            });
                        }),
                )
            })
    }

    /// Ahead/behind badges (arrow icon + count in an 11px label).
    fn count_badge(
        &self,
        id: &'static str,
        icon: IconName,
        n: usize,
        color: Hsla,
        colors: &gpui_component::theme::ThemeColor,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_1()
            .py_0p5()
            .rounded_sm()
            .bg(colors.input)
            .child(
                h_flex()
                    .gap_0p5()
                    .items_center()
                    .text_size(crate::theme::scaled_text_size(11.))
                    .text_color(color)
                    .child(
                        div()
                            .text_size(crate::theme::scaled_text_size(11.))
                            .child(Icon::new(icon)),
                    )
                    .child(shared(format!("{n}"))),
            )
    }
}

impl Render for Toolbar {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let enabled = self.has_remote && !self.busy;
        let pull_enabled = enabled && !self.has_conflicts;

        h_flex()
            .id("toolbar")
            .w_full()
            .h(px(32.))
            .flex_shrink_0()
            .px_2()
            .gap_1()
            .items_center()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            .child(self.branch_menu_button(cx))
            .child(self.tool_button(
                "tb-fetch",
                lucide("download"),
                "toolbar-fetch",
                &colors,
                enabled,
                ToolbarEvent::Fetch,
                cx,
            ))
            .child(self.tool_button(
                "tb-pull-merge",
                Icon::new(IconName::ArrowDown),
                "toolbar-pull-merge",
                &colors,
                pull_enabled,
                ToolbarEvent::PullMerge,
                cx,
            ))
            .child(self.tool_button(
                "tb-pull-rebase",
                lucide("git-commit-horizontal"),
                "toolbar-pull-rebase",
                &colors,
                pull_enabled,
                ToolbarEvent::PullRebase,
                cx,
            ))
            .child(self.tool_button(
                "tb-push",
                Icon::new(IconName::ArrowUp),
                "toolbar-push",
                &colors,
                enabled,
                ToolbarEvent::Push,
                cx,
            ))
            .child(self.tool_button(
                "tb-push-force",
                Icon::new(IconName::TriangleAlert),
                "toolbar-push-force",
                &colors,
                enabled,
                ToolbarEvent::PushForce,
                cx,
            ))
            .child(self.tool_button(
                "tb-compare",
                lucide("git-branch"),
                "toolbar-compare",
                &colors,
                !self.busy,
                ToolbarEvent::Compare,
                cx,
            ))
            .child(self.tool_button(
                "tb-extensions",
                Icon::new(IconName::LayoutDashboard),
                "toolbar-extensions",
                &colors,
                true,
                ToolbarEvent::Extensions,
                cx,
            ))
            // Ahead/behind badges.
            .child(self.count_badge(
                "tb-ahead",
                IconName::ArrowUp,
                self.ahead,
                colors.green,
                &colors,
            ))
            .child(self.count_badge(
                "tb-behind",
                IconName::ArrowDown,
                self.behind,
                colors.red,
                &colors,
            ))
            .child(div().flex_1())
            // Busy indicator (animated spinner instead of static text).
            .when(self.busy, |el| {
                el.child(Spinner::new().with_size(px(14.)).color(colors.blue))
            })
            .child(self.tool_button(
                "tb-refresh",
                lucide("refresh-cw"),
                "toolbar-refresh",
                &colors,
                true,
                ToolbarEvent::Refresh,
                cx,
            ))
            .child(self.tool_button(
                "tb-settings",
                Icon::new(IconName::Settings),
                "toolbar-settings",
                &colors,
                true,
                ToolbarEvent::Settings,
                cx,
            ))
    }
}
