use gpui::prelude::*;
use gpui::*;
use gpui_component::{ActiveTheme, Icon, IconName, h_flex, v_flex};

use crate::core::i18n::{self, Locale};
use crate::git::graph::{GraphEvent, GraphView};
use crate::git::panel::{
    BottomPanel, BottomPanelEvent, CommitPanel, CommitPanelEvent,
    DetailContent, DetailPanel,
};
use crate::git::sidebar::{Sidebar, SidebarEvent};
use crate::git::toolbar::{Toolbar, ToolbarEvent};
use crate::git::{GitStatus, GitUiEvent, GitView};

use super::tabs::{TabId, TabState, TabSummary};

#[derive(Clone, Debug)]
pub enum RepoTabEvent {
    Opened { id: TabId, path: String },
    SummaryChanged(TabSummary),
    RequestSettings,
}

#[derive(Clone, Debug)]
struct LayoutState {
    sidebar_width: f32,
    detail_width: f32,
    diff_height: f32,
}

#[derive(Clone, Debug)]
pub struct SidebarResize;
#[derive(Clone, Debug)]
pub struct DetailPanelResize;
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

impl Render for DetailPanelResize {
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

const MIN_SIDEBAR_WIDTH: f32 = 180.0;
const MAX_SIDEBAR_WIDTH: f32 = 400.0;
const MIN_DETAIL_WIDTH: f32 = 220.0;
const MAX_DETAIL_WIDTH: f32 = 600.0;
const MIN_DIFF_HEIGHT: f32 = 100.0;
const MAX_DIFF_HEIGHT: f32 = 500.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;

pub struct RepoTab {
    id: TabId,
    repo_path: String,
    opened: bool,
    branch: String,
    git_view: Entity<GitView>,
    sidebar: Entity<Sidebar>,
    graph: Entity<GraphView>,
    toolbar: Entity<Toolbar>,
    detail: Entity<DetailPanel>,
    commit: Entity<CommitPanel>,
    bottom: Entity<BottomPanel>,
    status: GitStatus,
    status_message: Option<String>,
    status_message_ok: Option<bool>,
    layout: LayoutState,
    sidebar_collapsed: bool,
    locale: Locale,
}

impl EventEmitter<RepoTabEvent> for RepoTab {}

impl RepoTab {
    pub fn new(
        id: TabId,
        repo_path: String,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let git_view = cx.new(|cx| GitView::new(locale, cx));
        let sidebar = cx.new(|cx| Sidebar::new(window, cx, locale));
        let graph = cx.new(|_cx| GraphView::new(id, locale));
        let toolbar = cx.new(|_cx| Toolbar::new(locale));
        let detail = cx.new(|_cx| DetailPanel::new(locale));
        let commit = cx.new(|cx| CommitPanel::new(window, cx, locale));
        let bottom = cx.new(|_cx| BottomPanel::new(locale));

        cx.subscribe(&sidebar, |tab, _event, event, cx| match event {
            SidebarEvent::ToggleCollapse => {
                tab.sidebar_collapsed = !tab.sidebar_collapsed;
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
            SidebarEvent::CheckoutBranch(name) => {
                tab.git_view.update(cx, |view, _| {
                    view.run("checkout", vec!["checkout".into(), name.clone()]);
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(true, cx);
                });
            }
            SidebarEvent::FileSelected { path, staged, code } => {
                tab.detail.update(cx, |detail, cx| {
                    detail.set_content(
                        DetailContent::File {
                            path: path.clone(),
                            staged: *staged,
                            code: *code,
                        },
                        cx,
                    )
                });
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
            ToolbarEvent::Pull => {
                tab.git_view.update(cx, |view, _| {
                    view.run("pull", vec!["pull".into()]);
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
                author,
                date,
                decorations,
            } => {
                tab.detail.update(cx, |detail, cx| {
                    detail.set_content(
                        DetailContent::Commit {
                            short: short.clone(),
                            subject: subject.clone(),
                            author: author.clone(),
                            date: date.clone(),
                            decorations: decorations.clone(),
                        },
                        cx,
                    )
                });
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_commit(oid, short, subject, cx);
                });
                tab.git_view.update(cx, |view, _| {
                    view.commit_files(oid.clone());
                });
            }
        })
        .detach();

        cx.subscribe(&commit, |tab, _event, event, cx| match event {
            CommitPanelEvent::Submit(message) => {
                tab.git_view.update(cx, |view, _| {
                    view.run(
                        "commit",
                        vec!["commit".into(), "-m".into(), message.clone()],
                    );
                });
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(true, cx);
                });
            }
        })
        .detach();

        cx.subscribe(&bottom, |tab, _event, event, cx| match event {
            BottomPanelEvent::ShowFileDiff { oid, path } => {
                tab.git_view.update(cx, |view, _| {
                    view.file_diff(oid.clone(), path.clone());
                });
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
                    sidebar.set_status(
                        branch.clone(),
                        branches.clone(),
                        files.clone(),
                        cx,
                    );
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
            GitUiEvent::CommitFilesChanged { oid, files } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_files(oid, files.clone(), cx);
                });
            }
            GitUiEvent::FileDiffChanged { oid, path, diff } => {
                tab.bottom.update(cx, |bottom, cx| {
                    bottom.set_diff(oid, path, diff.clone(), cx);
                });
            }
            GitUiEvent::CommandDone {
                label,
                success,
                message,
            } => {
                tab.toolbar.update(cx, |toolbar, cx| {
                    toolbar.set_busy(false, cx);
                });
                let refresh_after = matches!(
                    label.as_str(),
                    "commit" | "checkout" | "fetch --all" | "pull" | "push"
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
            detail,
            commit,
            bottom,
            status: GitStatus::None,
            status_message: None,
            status_message_ok: None,
            layout: LayoutState {
                sidebar_width: 250.0,
                detail_width: 320.0,
                diff_height: 260.0,
            },
            sidebar_collapsed: false,
            locale,
        }
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
        self.detail.update(cx, |detail, cx| {
            detail.set_locale(locale, cx);
        });
        self.commit.update(cx, |commit, cx| {
            commit.set_locale(locale, window, cx);
        });
        self.bottom.update(cx, |bottom, cx| {
            bottom.set_locale(locale, cx);
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

    fn collapsed_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        v_flex()
            .id("sidebar-rail")
            .w(px(28.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().colors.background)
            .items_center()
            .pt_2()
            .child(
                div()
                    .id("btn-expand")
                    .p_1()
                    .rounded_md()
                    .hover(|element| element.bg(cx.theme().colors.input))
                    .text_size(px(12.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(Icon::new(IconName::PanelLeftOpen))
                    .on_click(move |_event, _window, cx| {
                        this.update(cx, |tab, cx| {
                            tab.sidebar_collapsed = false;
                            cx.notify();
                        });
                    }),
            )
    }

    fn main_content(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors.clone();
        let sidebar_width = px(self.layout.sidebar_width);
        let detail_width = px(self.layout.detail_width);
        let diff_height = px(self.layout.diff_height);

        h_flex()
            .id("main-content")
            .size_full()
            .min_h_0()
            .on_drag_move::<SidebarResize>(cx.listener(
                |tab, event: &DragMoveEvent<SidebarResize>, _, cx| {
                    let new_width = f32::from(event.event.position.x)
                        .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
                    tab.layout.sidebar_width = new_width;
                    cx.notify();
                },
            ))
            .on_drag_move::<DetailPanelResize>(cx.listener(
                |tab, event: &DragMoveEvent<DetailPanelResize>, window, cx| {
                    let width = window.bounds().size.width;
                    let new_width = (f32::from(width)
                        - f32::from(event.event.position.x))
                    .clamp(MIN_DETAIL_WIDTH, MAX_DETAIL_WIDTH);
                    tab.layout.detail_width = new_width;
                    cx.notify();
                },
            ))
            .on_drag_move::<DiffViewerResize>(cx.listener(
                |tab, event: &DragMoveEvent<DiffViewerResize>, window, cx| {
                    let height = window.bounds().size.height;
                    let new_height = (f32::from(height)
                        - STATUS_BAR_HEIGHT
                        - f32::from(event.event.position.y))
                    .clamp(MIN_DIFF_HEIGHT, MAX_DIFF_HEIGHT);
                    tab.layout.diff_height = new_height;
                    cx.notify();
                },
            ))
            .child(
                div()
                    .relative()
                    .w(sidebar_width)
                    .h_full()
                    .flex_shrink_0()
                    .child(if self.sidebar_collapsed {
                        self.collapsed_rail(cx).into_any_element()
                    } else {
                        self.sidebar.clone().into_any_element()
                    })
                    .child(
                        div()
                            .id("sidebar-resize-handle")
                            .absolute()
                            .top_0()
                            .right(px(-3.))
                            .h_full()
                            .w(px(5.))
                            .cursor_col_resize()
                            .hover(|element| element.bg(colors.drag_border))
                            .on_drag(SidebarResize, |value, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| value.clone())
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(div().flex_1().min_h_0().child(self.graph.clone()))
                    .child(
                        div()
                            .id("diff-resize-handle")
                            .w_full()
                            .h(px(3.))
                            .flex_shrink_0()
                            .border_t_1()
                            .border_color(colors.border)
                            .cursor_row_resize()
                            .hover(|element| element.bg(colors.drag_border))
                            .on_drag(DiffViewerResize, |value, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| value.clone())
                            }),
                    )
                    .child(
                        v_flex()
                            .h(diff_height)
                            .flex_shrink_0()
                            .child(self.bottom.clone()),
                    ),
            )
            .child(
                div()
                    .relative()
                    .w(detail_width)
                    .h_full()
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(colors.border)
                    .flex()
                    .flex_col()
                    .child(div().flex_1().min_h_0().child(self.detail.clone()))
                    .child(
                        div()
                            .id("detail-resize-handle")
                            .absolute()
                            .top_0()
                            .left(px(-3.))
                            .h_full()
                            .w(px(5.))
                            .cursor_col_resize()
                            .hover(|element| element.bg(colors.drag_border))
                            .on_drag(DetailPanelResize, |value, _, _, cx| {
                                cx.stop_propagation();
                                cx.new(|_| value.clone())
                            }),
                    )
                    .child(self.commit.clone()),
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
            .size_full()
            .min_h_0()
            .child(self.toolbar.clone())
            .child(self.main_content(window, cx))
            .child(self.status_bar(cx))
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
