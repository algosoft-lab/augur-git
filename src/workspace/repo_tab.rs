use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

use crate::core::config::LayoutSettings;
use crate::core::git::CheckoutTarget;
use crate::core::i18n::{self, Locale};
use crate::git::changes_panel::ChangesPanel;
use crate::git::diff_view::DiffLayoutMode;
use crate::git::graph::{GraphEvent, GraphView};
use crate::git::panel::{
    BottomPanel, BottomPanelEvent, CommitPanel, CommitPanelEvent,
};
use crate::git::sidebar::{Sidebar, SidebarEvent};
use crate::git::toolbar::{Toolbar, ToolbarEvent};
use crate::git::{GitStatus, GitUiEvent, GitView, shared};

use super::tabs::{TabId, TabState, TabSummary};

mod layout;

#[derive(Clone, Debug)]
pub enum RepoTabEvent {
    Opened { id: TabId, path: String },
    SummaryChanged(TabSummary),
    RequestSettings,
    LayoutChanged(LayoutSettings),
}

#[derive(Clone, Debug)]
pub struct SidebarResize;
#[derive(Clone, Debug)]
pub struct RightPanelResize;
#[derive(Clone, Debug)]
pub struct DiffViewerResize;

impl Render for SidebarResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

impl Render for RightPanelResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

impl Render for DiffViewerResize {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
    }
}

pub(super) const MIN_COMMIT_HEIGHT: f32 = 120.0;
pub(super) const DIFF_RESIZE_HANDLE_HEIGHT: f32 = 3.0;

pub struct RepoTab {
    id: TabId,
    repo_path: String,
    opened: bool,
    branch: String,
    git_view: Entity<GitView>,
    sidebar: Entity<Sidebar>,
    graph: Entity<GraphView>,
    toolbar: Entity<Toolbar>,
    commit: Entity<CommitPanel>,
    changes: Entity<ChangesPanel>,
    bottom: Entity<BottomPanel>,
    status: GitStatus,
    status_message: Option<String>,
    status_message_ok: Option<bool>,
    layout: LayoutSettings,
    /// Force-push confirmation dialog is open (run only on explicit confirm).
    confirm_force_push: bool,
    locale: Locale,
}

impl EventEmitter<RepoTabEvent> for RepoTab {}

impl RepoTab {
    pub fn new(
        id: TabId,
        repo_path: String,
        locale: Locale,
        diff_layout: DiffLayoutMode,
        mut layout: LayoutSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        layout.normalize();
        let git_view = cx.new(|cx| GitView::new(locale, cx));
        let sidebar = cx.new(|cx| Sidebar::new(window, cx, locale));
        let graph = cx.new(|cx| GraphView::new(id, locale, cx));
        let toolbar = cx.new(|_cx| Toolbar::new(locale));
        let commit = cx.new(|cx| CommitPanel::new(window, cx, locale));
        let changes = cx.new(|_cx| ChangesPanel::new(locale));
        let bottom = cx.new(|_cx| {
            BottomPanel::new(locale, diff_layout, layout.file_list_ratio)
        });

        cx.subscribe(&sidebar, |tab, _event, event, cx| match event {
            SidebarEvent::ToggleCollapse => {
                tab.layout.sidebar_collapsed = !tab.layout.sidebar_collapsed;
                cx.emit(RepoTabEvent::LayoutChanged(tab.layout.clone()));
                cx.notify();
            }
            SidebarEvent::BranchSelected(name) => {
                tab.status_message = Some(i18n::text_args(
                    tab.locale,
                    "branch-selected",
                    &[("name", name)],
                ));
                cx.notify();
            }
            SidebarEvent::CheckoutRef(target) => {
                tab.start_checkout(target.clone(), cx);
            }
            SidebarEvent::CopyRef(value) => {
                tab.copy_ref(value, cx);
            }
        })
        .detach();

        cx.subscribe(&toolbar, |tab, _event, event, cx| match event {
            ToolbarEvent::Fetch => {
                tab.git_view.update(cx, |view, _| {
                    view.run(
                        "fetch --all",
                        vec!["fetch".into(), "--all".into()],
                    );
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(true, cx);
                });
            }
            ToolbarEvent::PullMerge => {
                tab.git_view.update(cx, |view, _| {
                    view.run("pull", vec!["pull".into()]);
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(true, cx);
                });
            }
            ToolbarEvent::PullRebase => {
                tab.git_view.update(cx, |view, _| {
                    view.run(
                        "pull --rebase",
                        vec!["pull".into(), "--rebase".into()],
                    );
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(true, cx);
                });
            }
            ToolbarEvent::Push => {
                tab.git_view.update(cx, |view, _| {
                    view.run("push", vec!["push".into()]);
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(true, cx);
                });
            }
            ToolbarEvent::PushForce => {
                // Never run directly: open the confirmation dialog first.
                tab.confirm_force_push = true;
                cx.notify();
            }
            ToolbarEvent::Branch => {
                tab.sidebar.update(cx, |sidebar, cx| {
                    sidebar.flash_branches(cx);
                });
            }
            ToolbarEvent::Refresh => {
                tab.git_view.update(cx, |view, _| view.refresh());
            }
            ToolbarEvent::Settings => {
                cx.emit(RepoTabEvent::RequestSettings);
            }
        })
        .detach();

        cx.subscribe(&graph, |tab, _event, event, cx| match event {
            GraphEvent::CommitSelected {
                oid,
                short,
                subject,
                ..
            } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_commit(oid, short, subject, cx);
                });
                tab.git_view.update(cx, |view, _| {
                    view.commit_files(oid.clone());
                    view.commit_message(oid.clone());
                });
            }
            GraphEvent::CommitHovered(oid) => {
                tab.git_view.update(cx, |view, _| {
                    view.commit_message(oid.clone());
                });
            }
            GraphEvent::CheckoutRef(target) => {
                tab.start_checkout(target.clone(), cx);
            }
            GraphEvent::CopyRef(value) => {
                tab.copy_ref(value, cx);
            }
            GraphEvent::CopyCommitMessage(oid) => {
                tab.start_copy_commit_message(oid.clone(), cx);
            }
        })
        .detach();

        cx.subscribe(&commit, |tab, _event, event, cx| match event {
            CommitPanelEvent::Submit { message, amend } => {
                log::info!("[commit_panel] submit requested: amend={amend}");
                tab.git_view.update(cx, |view, _| {
                    view.commit(message.clone(), *amend);
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(true, cx);
                });
            }
        })
        .detach();

        cx.subscribe(&bottom, |tab, _event, event, cx| match event {
            BottomPanelEvent::ShowFileDiff {
                oid,
                merge_parent,
                file,
            } => {
                tab.git_view.update(cx, |view, _| {
                    view.file_diff(
                        oid.clone(),
                        merge_parent.clone(),
                        file.clone(),
                    );
                });
            }
            BottomPanelEvent::ShowAllFileDiffs {
                oid,
                merge_parent,
                files,
            } => {
                tab.git_view.update(cx, |view, _| {
                    view.file_diffs(
                        oid.clone(),
                        merge_parent.clone(),
                        files.clone(),
                    );
                });
            }
            BottomPanelEvent::LayoutChanged { file_list_ratio } => {
                tab.layout.file_list_ratio = *file_list_ratio;
                cx.emit(RepoTabEvent::LayoutChanged(tab.layout.clone()));
            }
        })
        .detach();

        cx.subscribe(&git_view, |tab, _event, event, cx| match event {
            GitUiEvent::StatusChanged {
                branch,
                ahead,
                behind,
                files,
                branches,
            } => {
                let branch_name = branch.clone();
                let has_staged = files.iter().any(|file| file.is_staged());
                let staged_count =
                    files.iter().filter(|file| file.is_staged()).count();
                let unstaged_count = files.len() - staged_count;
                let ahead_text = ahead.to_string();
                let behind_text = behind.to_string();
                let staged_text = staged_count.to_string();
                let unstaged_text = unstaged_count.to_string();

                tab.branch = branch_name;
                tab.status = GitStatus::Ready(i18n::text_args(
                    tab.locale,
                    "status-summary",
                    &[
                        ("branch", branch),
                        ("ahead", &ahead_text),
                        ("behind", &behind_text),
                        ("staged", &staged_text),
                        ("unstaged", &unstaged_text),
                    ],
                ));
                tab.sidebar.update(cx, |sidebar, cx| {
                    sidebar.set_status(branch.clone(), branches.clone(), cx);
                });
                tab.changes.update(cx, |changes, cx| {
                    changes.set_files(files.clone(), cx);
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_ahead_behind(*ahead, *behind, cx);
                });
                tab.commit.update(cx, |commit, cx| {
                    commit.set_has_staged(has_staged, cx);
                });
                tab.emit_summary(cx);
                cx.notify();
            }
            GitUiEvent::LogChanged { rows } => {
                tab.graph.update(cx, |graph, cx| {
                    graph.set_rows(rows.clone(), cx);
                });
            }
            GitUiEvent::RefsChanged(refs) => {
                tab.sidebar.update(cx, |sidebar, cx| {
                    sidebar.set_refs(refs.clone(), cx);
                });
            }
            GitUiEvent::CommitFilesChanged {
                oid,
                files,
                merge_parent,
            } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_files(
                        oid,
                        merge_parent.clone(),
                        files.clone(),
                        cx,
                    );
                });
            }
            GitUiEvent::CommitMessageChanged { oid, message } => {
                tab.graph.update(cx, |graph, cx| {
                    graph.set_commit_message(&oid, message.clone(), cx);
                });
            }
            GitUiEvent::FileDiffChanged {
                oid,
                file,
                patch,
                old_source,
                new_source,
            } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_diff(
                        oid,
                        file,
                        patch.clone(),
                        old_source.clone(),
                        new_source.clone(),
                        cx,
                    );
                });
            }
            GitUiEvent::CommandDone {
                label,
                success,
                message,
            } => {
                if label == "checkout" {
                    log::info!(
                        "[git_checkout] result received: success={success}"
                    );
                }
                let copy_commit_message = label == "copy-commit-message";
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(false, cx);
                });
                if copy_commit_message {
                    if *success {
                        tab.finish_copy_commit_message(message, cx);
                    } else {
                        tab.status_message = Some(i18n::text_args(
                            tab.locale,
                            "context-copy-commit-message-failed",
                            &[("error", first_line(message))],
                        ));
                        tab.status_message_ok = Some(false);
                        cx.notify();
                    }
                } else {
                    let refresh_after = matches!(
                        label.as_str(),
                        "commit"
                            | "checkout"
                            | "fetch --all"
                            | "pull"
                            | "pull --rebase"
                            | "push"
                            | "push --force"
                    );
                    tab.status_message = Some(if *success {
                        i18n::text_args(
                            tab.locale,
                            "command-success",
                            &[("label", label)],
                        )
                    } else {
                        i18n::text_args(
                            tab.locale,
                            "command-failed",
                            &[("label", label), ("error", first_line(message))],
                        )
                    });
                    tab.status_message_ok = Some(*success);
                    if *success && refresh_after {
                        tab.git_view.update(cx, |view, _| view.refresh());
                    }
                    cx.notify();
                }
            }
            GitUiEvent::RepoOpened(path) => {
                cx.emit(RepoTabEvent::Opened {
                    id: tab.id,
                    path: path.clone(),
                });
                tab.emit_summary(cx);
            }
            GitUiEvent::Error(message) => {
                tab.status = GitStatus::Error(message.clone());
                tab.emit_summary(cx);
                cx.notify();
            }
        })
        .detach();

        Self {
            id,
            repo_path,
            opened: false,
            branch: String::new(),
            git_view,
            sidebar,
            graph,
            toolbar,
            commit,
            changes,
            bottom,
            status: GitStatus::None,
            status_message: None,
            status_message_ok: None,
            layout,
            confirm_force_push: false,
            locale,
        }
    }

    fn start_checkout(
        &mut self,
        target: CheckoutTarget,
        cx: &mut Context<Self>,
    ) {
        self.git_view.update(cx, |view, _| view.checkout(target));
        self.toolbar.update(cx, |toolbar, cx| {
            toolbar.set_busy(true, cx);
        });
    }

    /// Run the confirmed force push (`git push --force`).
    fn start_force_push(&mut self, cx: &mut Context<Self>) {
        self.confirm_force_push = false;
        self.git_view.update(cx, |view, _| {
            view.run("push --force", vec!["push".into(), "--force".into()]);
        });
        self.toolbar.update(cx, |toolbar, cx| {
            toolbar.set_busy(true, cx);
        });
        cx.notify();
    }

    fn cancel_force_push(&mut self, cx: &mut Context<Self>) {
        if self.confirm_force_push {
            self.confirm_force_push = false;
            cx.notify();
        }
    }

    fn copy_ref(&mut self, value: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(value.to_string()));
        self.status_message = Some(i18n::text_args(
            self.locale,
            "context-copied",
            &[("name", value)],
        ));
        self.status_message_ok = Some(true);
        cx.notify();
    }

    fn start_copy_commit_message(
        &mut self,
        oid: String,
        cx: &mut Context<Self>,
    ) {
        self.git_view.update(cx, |view, _| {
            view.copy_commit_message(oid);
        });
        self.toolbar.update(cx, |toolbar, cx| {
            toolbar.set_busy(true, cx);
        });
    }

    fn finish_copy_commit_message(
        &mut self,
        message: &str,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(message.to_string()));
        self.status_message =
            Some(i18n::text(self.locale, "context-copied-commit-message"));
        self.status_message_ok = Some(true);
        cx.notify();
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        if self.opened {
            return;
        }
        self.opened = true;
        self.status = GitStatus::Scanning;
        self.emit_summary(cx);
        let path = self.repo_path.clone();
        self.git_view
            .update(cx, |view, cx| view.open_repo(&path, cx));
    }

    /// Make this repository the active UI tab and start consuming its events.
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        self.open(cx);
        self.git_view
            .update(cx, |view, cx| view.set_active(true, cx));
    }

    /// Stop consuming events while retaining the repository state and worker.
    pub fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.git_view
            .update(cx, |view, cx| view.set_active(false, cx));
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.git_view.update(cx, |view, _| view.close_repo());
        self.opened = false;
    }

    pub fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        self.git_view.update(cx, |view, _| view.set_locale(locale));
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.set_locale(locale, cx);
        });
        self.toolbar.update(cx, |toolbar, cx| {
            toolbar.set_locale(locale, cx);
        });
        self.graph.update(cx, |graph, cx| {
            graph.set_locale(locale, cx);
        });
        self.commit.update(cx, |commit, cx| {
            commit.set_locale(locale, window, cx);
        });
        self.changes.update(cx, |changes, cx| {
            changes.set_locale(locale, cx);
        });
        self.bottom.update(cx, |bottom, cx| {
            bottom.set_locale(locale, cx);
        });
        cx.notify();
    }

    /// Apply the persisted diff layout chosen in the settings overlay.
    pub fn set_diff_layout(
        &mut self,
        diff_layout: DiffLayoutMode,
        cx: &mut Context<Self>,
    ) {
        self.bottom.update(cx, |bottom, cx| {
            bottom.set_diff_layout(diff_layout, cx);
        });
    }

    pub fn set_layout(
        &mut self,
        mut layout: LayoutSettings,
        cx: &mut Context<Self>,
    ) {
        layout.normalize();
        self.layout = layout.clone();
        self.bottom.update(cx, |bottom, cx| {
            bottom.set_file_list_ratio(layout.file_list_ratio, cx);
        });
        cx.notify();
    }

    pub fn focus_branches(&mut self, cx: &mut Context<Self>) {
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.flash_branches(cx);
        });
    }

    pub fn summary(&self) -> TabSummary {
        let state = match self.status {
            GitStatus::Error(_) => TabState::Error,
            GitStatus::Ready(_) => TabState::Ready,
            GitStatus::None | GitStatus::Scanning => TabState::Loading,
        };
        TabSummary {
            id: self.id,
            title: repo_title(&self.repo_path),
            branch: (!self.branch.is_empty()).then(|| self.branch.clone()),
            state,
        }
    }

    fn emit_summary(&self, cx: &mut Context<Self>) {
        cx.emit(RepoTabEvent::SummaryChanged(self.summary()));
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let (text, color) = match &self.status {
            GitStatus::None => (
                i18n::text(self.locale, "no-repo-open"),
                colors.muted_foreground,
            ),
            GitStatus::Scanning => {
                (i18n::text(self.locale, "status-scanning"), colors.warning)
            }
            GitStatus::Ready(label) => (format!("● {label}"), colors.green),
            GitStatus::Error(message) => (format!("✗ {message}"), colors.red),
        };
        let msg = self.status_message.clone();
        let msg_color = match self.status_message_ok {
            Some(true) => colors.green,
            Some(false) => colors.red,
            None => colors.muted_foreground,
        };

        h_flex()
            .id("status-bar")
            .w_full()
            .h_6()
            .flex_shrink_0()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.background)
            .px_3()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(colors.muted_foreground)
                    .truncate()
                    .child(SharedString::from(self.repo_path.clone())),
            )
            .child(
                h_flex()
                    .gap_3()
                    .when_some(msg, |row, message| {
                        row.child(
                            div()
                                .text_size(px(11.))
                                .text_color(msg_color)
                                .child(SharedString::from(message)),
                        )
                    })
                    .child(
                        div().text_size(px(11.)).text_color(color).child(text),
                    ),
            )
    }

    /// Modal confirmation for the destructive force push (settings-overlay
    /// pattern: full-cover backdrop with a card, backdrop click cancels).
    fn force_push_confirm_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let locale = self.locale;
        let this = cx.entity();

        let title_row = h_flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(px(16.))
                    .text_color(colors.red)
                    .child(Icon::new(IconName::TriangleAlert)),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(colors.foreground)
                    .child(shared(i18n::text(locale, "push-force-title"))),
            );

        let cancel_btn = {
            let this = this.clone();
            Button::new("force-push-cancel")
                .label(i18n::text(locale, "push-force-cancel"))
                .ghost()
                .flex_1()
                .on_click(move |_e, _w, cx| {
                    this.update(cx, |tab, cx| tab.cancel_force_push(cx));
                })
        };
        let confirm_btn = Button::new("force-push-confirm")
            .label(i18n::text(locale, "push-force-confirm"))
            .danger()
            .flex_1()
            .on_click(move |_e, _w, cx| {
                this.update(cx, |tab, cx| tab.start_force_push(cx));
            });

        v_flex()
            .id("force-push-overlay")
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .bg(colors.background.opacity(0.9))
            .flex()
            .items_center()
            .justify_center()
            // Clicking the backdrop cancels the force push.
            .on_mouse_down(MouseButton::Left, {
                let this = cx.entity();
                move |_e, _w, cx| {
                    this.update(cx, |tab, cx| tab.cancel_force_push(cx));
                }
            })
            .child(
                v_flex()
                    .id("force-push-card")
                    .items_start()
                    .gap_3()
                    .p_6()
                    .bg(colors.background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded_lg()
                    .min_w(px(380.))
                    .max_w(px(460.))
                    .when(cx.theme().shadow, |el| el.shadow_md())
                    // Stop clicks inside the card from closing the overlay.
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .child(title_row)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(colors.muted_foreground)
                            .child(shared(i18n::text_args(
                                locale,
                                "push-force-warning",
                                &[("branch", &self.branch)],
                            ))),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(cancel_btn)
                            .child(confirm_btn),
                    ),
            )
    }
}

impl Render for RepoTab {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .id(SharedString::from(format!("repo-content-{}", self.id)))
            .relative()
            .size_full()
            .min_h_0()
            .child(self.toolbar.clone())
            .child(self.main_content(window, cx))
            .child(self.status_bar(cx))
            .when(self.confirm_force_push, |element| {
                element.child(self.force_push_confirm_overlay(cx))
            })
    }
}

fn repo_title(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        path.to_string()
    } else {
        crate::git::dir_name(trimmed).to_string()
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}
