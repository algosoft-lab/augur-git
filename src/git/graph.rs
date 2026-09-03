//! Commit graph presentation with delayed full-message previews.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::Duration;

use crate::core::git::{CheckoutTarget, CommitMessage};
use crate::git::commit_message_dialog::CommitMessageDialog;
use crate::git::commit_preview::CommitHoverPreview;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, WindowExt, h_flex,
    input::{InputEvent, InputState},
    menu::{ContextMenuExt, PopupMenuItem},
    theme::ThemeColor,
    tooltip::Tooltip,
    v_flex,
};

use crate::core::commit_search::{CommitSearchField, search_log_rows};
use crate::core::graph::{
    AUTHOR_COL_WIDTH, DATE_COL_WIDTH, GraphRow, HASH_COL_WIDTH, LogRow,
    RefKind, RefLabel, column_visibility, compute_graph, format_relative_time,
    parse_ref_labels,
};
use crate::core::i18n::{self, Locale};
use crate::git::shared;

#[path = "graph_painter.rs"]
mod graph_painter;
#[path = "graph_search.rs"]
mod graph_search;

/// 提交树行高（h_9=36px：圆心距 36，节点直径 24，边缘间距 12 = 半径，验证项目同比例）
pub const ROW_HEIGHT: f32 = 36.0;
/// 树列宽（≥ 节点直径 24，圆不溢出列）
pub const COL_WIDTH: f32 = 24.0;
/// 节点空心圆半径（stroke 描边细线圆，直径 24）
const NODE_RADIUS: f32 = 12.0;
/// 树列左侧留白（首个节点圆不贴左边界）
const GRAPH_LEFT_PAD: f32 = 12.0;
/// Rows from the list end that trigger the next log page request.
const LOAD_AHEAD_ROWS: usize = 30;

#[derive(Clone, Debug)]
pub enum GraphEvent {
    CommitSelected {
        /// Full OID used by the bottom file list and diff viewer.
        oid: String,
        short: String,
        subject: String,
    },
    /// Request the full message for a hovered commit or the message dialog.
    CommitMessageRequested(String),
    /// Check out the selected commit.
    CheckoutRef(CheckoutTarget),
    /// Copy the selected commit OID to the system clipboard.
    CopyRef(String),
    /// Copy the selected commit's complete message to the system clipboard.
    CopyCommitMessage(String),
    /// The visible list approached the last loaded commit; fetch the next page.
    MoreLogPageRequested,
    /// Clear the bottom commit details because the selected row is hidden.
    SelectionCleared,
}

pub struct GraphView {
    /// All commits loaded by the worker, before the local search filter.
    all_rows: Vec<LogRow>,
    rows: Vec<LogRow>,
    layout: Vec<GraphRow>,
    /// Whether the worker reported another commit page for the scope.
    has_more: bool,
    /// One page request is in flight; cleared when any page arrives.
    page_in_flight: bool,
    /// Configured remote names for classifying ref decorations.
    remote_names: Vec<String>,
    history_count: usize,
    selected: Option<usize>,
    search_input: Entity<InputState>,
    search_field: CommitSearchField,
    search_query: String,
    /// Full messages cached after selection or hover requests.
    commit_messages: HashMap<String, CommitMessage>,
    /// Commit preview entity shared by every graph-row tooltip.
    hover_preview: Entity<CommitHoverPreview>,
    /// Content view of the "show full commit message" dialog.
    message_dialog: Entity<CommitMessageDialog>,
    /// OID currently under the pointer, if any.
    hovered_oid: Option<String>,
    /// UI locale synchronized by Workspace.
    locale: Locale,
    /// Disable checkout actions while a repository operation is running.
    busy: bool,
    /// 自身实测渲染宽（测量 canvas 回写；初始视为宽屏，首帧后校正）。
    /// 响应式列显隐以它为准——拖侧栏挤压中央列时窗口宽并未变
    content_width: f32,
    scroll_handle: UniformListScrollHandle,
    list_id: SharedString,
}

#[derive(Clone)]
struct GraphRenderOptions {
    tree_w: f32,
    show_author: bool,
    show_message: bool,
    now: i64,
    colors: ThemeColor,
    mono: SharedString,
    lane_colors: [Hsla; 10],
}

impl EventEmitter<GraphEvent> for GraphView {}

impl GraphView {
    pub fn new(
        tab_id: u64,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let hover_preview = cx.new(|_| CommitHoverPreview::new(locale));
        let message_dialog = cx.new(|_| CommitMessageDialog::new(locale));
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(i18n::text(locale, "commit-search-placeholder"))
        });
        let search_input_entity = search_input.clone();
        cx.subscribe(&search_input_entity, |graph, _event, event, cx| {
            if matches!(event, InputEvent::Change) {
                graph.search_query =
                    graph.search_input.read(cx).value().to_string();
                graph.rebuild_rows(cx);
                log::debug!(
                    "[commit_search] input changed: field={:?}, matches={}/{}",
                    graph.search_field,
                    graph.rows.len(),
                    graph.history_count
                );
                cx.notify();
            }
        })
        .detach();
        Self {
            all_rows: Vec::new(),
            rows: Vec::new(),
            layout: Vec::new(),
            has_more: false,
            page_in_flight: false,
            remote_names: Vec::new(),
            history_count: 0,
            selected: None,
            search_input,
            search_field: CommitSearchField::Subject,
            search_query: String::new(),
            commit_messages: HashMap::new(),
            hover_preview,
            message_dialog,
            hovered_oid: None,
            locale,
            busy: false,
            content_width: f32::INFINITY,
            scroll_handle: UniformListScrollHandle::new(),
            list_id: SharedString::from(format!("graph-rows-{tab_id}")),
        }
    }

    /// Synchronize the locale with the workspace.
    pub fn set_locale(
        &mut self,
        locale: Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.locale = locale;
        self.search_input.update(cx, |input, cx| {
            input.set_placeholder(
                i18n::text(locale, "commit-search-placeholder"),
                window,
                cx,
            );
        });
        self.hover_preview.update(cx, |preview, cx| {
            preview.set_locale(locale, cx);
        });
        self.message_dialog.update(cx, |dialog, cx| {
            dialog.set_locale(locale, cx);
        });
        cx.notify();
    }

    /// Disable checkout actions while another repository operation is active.
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy != busy {
            self.busy = busy;
            cx.notify();
        }
    }

    /// Update the remote names used to classify ref decorations as remote
    /// branches (first path segment matched against this list).
    pub fn set_remote_names(
        &mut self,
        remotes: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.remote_names != remotes {
            self.remote_names = remotes;
            cx.notify();
        }
    }

    /// Snapshot of the loaded commits, used by the branch comparison view.
    pub fn log_rows(&self) -> Vec<LogRow> {
        self.all_rows.clone()
    }

    /// Apply one worker log page. `replace` restarts the list after a refresh
    /// or scope change; otherwise the page appends for lazy loading.
    pub fn set_log_page(
        &mut self,
        mut rows: Vec<LogRow>,
        replace: bool,
        has_more: bool,
        cx: &mut Context<Self>,
    ) {
        self.page_in_flight = false;
        self.has_more = has_more;
        if replace {
            self.all_rows = rows;
        } else {
            // Refs can move between page requests; skip OIDs already shown.
            let known = self
                .all_rows
                .iter()
                .map(|row| row.oid.as_str())
                .collect::<std::collections::HashSet<_>>();
            rows.retain(|row| !known.contains(row.oid.as_str()));
            self.all_rows.extend(rows);
        }
        self.rebuild_rows(cx);
        log::debug!(
            "[graph_perf] graph layout cached: rows={}",
            self.layout.len()
        );
        cx.notify();
    }

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let had_selection = self.selected.is_some();
        let selected_oid = self
            .selected
            .and_then(|index| self.rows.get(index))
            .map(|row| row.oid.clone());
        self.history_count = self.all_rows.len();
        self.rows = search_log_rows(
            &self.all_rows,
            &self.search_query,
            self.search_field,
        );
        log::debug!(
            "[commit_search] rows rebuilt: source={}, history={}, matches={}, active={}, field={:?}",
            self.all_rows.len(),
            self.history_count,
            self.rows.len(),
            !self.search_query.is_empty(),
            self.search_field,
        );
        let visible_oids = self
            .rows
            .iter()
            .map(|row| row.oid.as_str())
            .collect::<HashSet<_>>();
        let layout_rows = self
            .rows
            .iter()
            .cloned()
            .map(|mut row| {
                if !self.search_query.is_empty() {
                    row.parents.retain(|parent| {
                        visible_oids.contains(parent.as_str())
                    });
                }
                row
            })
            .collect::<Vec<_>>();
        self.layout = compute_graph(&layout_rows);
        self.selected = selected_oid
            .and_then(|oid| self.rows.iter().position(|row| row.oid == oid));
        if had_selection && self.selected.is_none() {
            cx.emit(GraphEvent::SelectionCleared);
        }
        if self
            .hovered_oid
            .as_ref()
            .is_some_and(|oid| !self.rows.iter().any(|row| &row.oid == oid))
        {
            self.hovered_oid = None;
            self.hover_preview.update(cx, |preview, cx| {
                preview.clear(cx);
            });
        }
    }

    /// Request another page when the rendered range approaches the end of the
    /// loaded history. The in-flight flag is set by the caller before emitting.
    fn wants_more_rows(&self, rendered_end: usize) -> bool {
        !self.rows.is_empty()
            && self.has_more
            && !self.page_in_flight
            && rendered_end + LOAD_AHEAD_ROWS >= self.rows.len()
    }

    fn set_search_field(
        &mut self,
        field: CommitSearchField,
        cx: &mut Context<Self>,
    ) {
        if self.search_field == field {
            return;
        }
        self.search_field = field;
        self.rebuild_rows(cx);
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        log::debug!(
            "[commit_search] field changed: field={field:?}, matches={}/{}",
            self.rows.len(),
            self.history_count
        );
        cx.notify();
    }

    /// Cache an asynchronous message response and update the active preview
    /// and the message dialog.
    pub fn set_commit_message(
        &mut self,
        oid: &str,
        message: CommitMessage,
        cx: &mut Context<Self>,
    ) {
        self.commit_messages
            .insert(oid.to_string(), message.clone());
        if self.hovered_oid.as_deref() == Some(oid) {
            self.hover_preview.update(cx, |preview, cx| {
                preview.set_message(oid, message.clone(), cx);
            });
        }
        self.message_dialog.update(cx, |dialog, cx| {
            dialog.set_message(oid, message, cx);
        });
    }

    /// Open the modal dialog presenting a commit's complete message.
    ///
    /// The dialog reuses a single content entity: while it is already open,
    /// selecting another commit updates the visible content in place instead
    /// of stacking another dialog layer.
    pub fn open_commit_message_dialog(
        &mut self,
        row: &LogRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cached = self.commit_messages.get(&row.oid).cloned();
        log::debug!(
            "[commit_message_dialog] open requested: oid={}, cached={}",
            row.oid,
            cached.is_some()
        );
        self.message_dialog.update(cx, |dialog, cx| {
            dialog.set_commit(row, cached, cx);
        });
        if !self.commit_messages.contains_key(&row.oid) {
            log::debug!(
                "[commit_message_dialog] requesting dialog commit message"
            );
            cx.emit(GraphEvent::CommitMessageRequested(row.oid.clone()));
        }
        if window.has_active_dialog(cx) {
            log::debug!(
                "[commit_message_dialog] skip open: another dialog is active"
            );
            return;
        }
        let view = self.message_dialog.clone();
        let title = i18n::text(self.locale, "commit-message-dialog-title");
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(title.clone())
                .width(px(560.))
                .max_h(px(520.))
                .child(view.clone())
        });
    }

    fn set_hovered(
        &mut self,
        index: usize,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.rows.get(index).cloned() else {
            return;
        };

        if hovered {
            let changed = self.hovered_oid.as_deref() != Some(row.oid.as_str());
            self.hovered_oid = Some(row.oid.clone());
            if changed {
                let message = self.commit_messages.get(&row.oid).cloned();
                let needs_message = message.is_none();
                self.hover_preview.update(cx, |preview, cx| {
                    preview.set_commit(&row, message, cx);
                });
                if needs_message {
                    log::debug!(
                        "[commit_preview] requesting hovered commit message"
                    );
                    cx.emit(GraphEvent::CommitMessageRequested(row.oid));
                }
            }
        } else if self.hovered_oid.as_deref() == Some(row.oid.as_str()) {
            self.hovered_oid = None;
            self.hover_preview.update(cx, |preview, cx| {
                preview.clear(cx);
            });
        }
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        self.selected = Some(index);
        cx.emit(GraphEvent::CommitSelected {
            oid: row.oid.clone(),
            short: row.short.clone(),
            subject: row.subject.clone(),
        });
        cx.notify();
    }
}

impl Render for GraphView {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mono = cx.theme().mono_font_family.clone();
        let max_lanes = self
            .layout
            .iter()
            .map(|row| row.lane_count)
            .max()
            .unwrap_or(1);
        let tree_w = GRAPH_LEFT_PAD + max_lanes as f32 * COL_WIDTH + 8.0;
        // 响应式列显隐：以自身实测宽为准（窄则先藏 Author 再藏 Message，见 column_visibility）
        let (show_author, show_message) =
            column_visibility(self.content_width, tree_w);
        // 相对时间基准（unix 秒）
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let lane_colors = crate::theme::lane_colors(cx);
        let body = if self.rows.is_empty() {
            let empty_key = if self.search_query.is_empty() {
                "graph-empty"
            } else {
                "commit-search-no-results"
            };
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_1()
                .child(
                    div()
                        .size(px(24.))
                        .text_color(colors.muted_foreground)
                        .child(crate::git::lucide("git-commit-horizontal")),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(colors.muted_foreground)
                        .child(shared(i18n::text(self.locale, empty_key))),
                )
        } else {
            let render_options = GraphRenderOptions {
                tree_w,
                show_author,
                show_message,
                now,
                colors,
                mono,
                lane_colors,
            };
            let rows = uniform_list(
                self.list_id.clone(),
                self.rows.len(),
                cx.processor(move |graph, range: Range<usize>, _window, cx| {
                    if graph.wants_more_rows(range.end) {
                        // Reserve the request before emitting so repeated render
                        // passes cannot queue the same page twice.
                        graph.page_in_flight = true;
                        cx.emit(GraphEvent::MoreLogPageRequested);
                    }
                    range
                        .map(|index| {
                            graph.render_row(index, &render_options, cx)
                        })
                        .collect()
                }),
            )
            .track_scroll(&self.scroll_handle)
            .flex_1()
            .min_h_0();
            v_flex()
                .flex_1()
                .min_h_0()
                .child(self.column_header(
                    &colors,
                    tree_w,
                    show_author,
                    show_message,
                ))
                .child(rows)
        };

        v_flex()
            .id("graph-view")
            .size_full()
            .bg(colors.background)
            .child(measure_width_canvas(cx.entity()))
            .child(graph_search::render(
                cx.entity(),
                &self.search_input,
                self.locale,
                self.search_field,
                &self.search_query,
                self.rows.len(),
                self.history_count,
                colors,
            ))
            .child(body)
            .into_any_element()
    }
}

impl GraphView {
    fn render_row(
        &self,
        index: usize,
        options: &GraphRenderOptions,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.rows.get(index).cloned() else {
            return div().into_any_element();
        };
        let Some(graph_row) = self.layout.get(index).cloned() else {
            return div().into_any_element();
        };
        // Snapshot for the context menu before row fields move into children.
        let show_message_row = row.clone();
        let graph = cx.entity();
        let selected = self.selected == Some(index);
        let colors = options.colors;
        let tree_w = options.tree_w;
        let lane_colors = options.lane_colors;
        let row_bg = if selected {
            colors.list_active
        } else {
            colors.background
        };
        // The author initials sit on top of the graph node canvas. On the
        // filled HEAD node they sit on the lane fill, so their color flips
        // with the fill luminance; hollow nodes keep the theme foreground.
        let node_col = Some(graph_row.node_lane as f32);
        let node_letters: String = row.author.chars().take(2).collect();
        let initials_color = if graph_row.is_head {
            crate::theme::initials_text_color(
                lane_colors[graph_row.node_color % lane_colors.len()],
            )
        } else {
            colors.foreground
        };
        // VS Code-style ref chips (HEAD, branches, remotes, tags) rendered
        // right after the message column so remote-branch divergence is
        // identifiable without opening the hover preview.
        let ref_labels = parse_ref_labels(&row.decorations, &self.remote_names);
        let ref_chips = if ref_labels.is_empty() {
            None
        } else {
            Some(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(
                        ref_labels
                            .iter()
                            .map(|label| ref_label_chip(label, colors)),
                    )
                    .into_any_element(),
            )
        };
        let checkout_label = i18n::text(self.locale, "context-checkout");
        let copy_label = i18n::text(self.locale, "context-copy-commit");
        let copy_message_label =
            i18n::text(self.locale, "context-copy-commit-message");
        let show_message_label =
            i18n::text(self.locale, "context-show-commit-message");
        let commit_target = CheckoutTarget::Commit(row.oid.clone());
        let copy_value = row.oid.clone();
        let copy_message_value = row.oid.clone();
        let checkout_disabled = self.busy;
        let graph_for_click = graph.clone();
        let graph_for_hover = graph.clone();
        let hover_preview = self.hover_preview.clone();

        let row_element = h_flex()
            .id(SharedString::from(format!("graph-row-{}", row.oid)))
            .w_full()
            .h_9()
            .flex_shrink_0()
            .items_center()
            .pr_2()
            .gap_2()
            .bg(row_bg)
            .hover(|this| {
                if !selected {
                    this.bg(colors.list_hover)
                } else {
                    this
                }
            })
            .on_click(move |_e, _w, cx| {
                graph_for_click.update(cx, |v, cx| v.select(index, cx));
            })
            .on_hover(move |hovered, _window, cx| {
                graph_for_hover.update(cx, |graph, cx| {
                    graph.set_hovered(index, *hovered, cx);
                });
            })
            .tooltip(move |window, cx| {
                let hover_preview = hover_preview.clone();
                Tooltip::element(move |_window, _cx| hover_preview.clone())
                    .build(window, cx)
            })
            .tooltip_show_delay(Duration::from_millis(500))
            // Graph column: row canvas plus HEAD initials overlay.
            .child(
                div()
                    .w(px(tree_w))
                    .flex_shrink_0()
                    .h_full()
                    .relative()
                    .child(
                        canvas(
                            |_b: Bounds<Pixels>,
                             _w: &mut Window,
                             _c: &mut App| {},
                            move |bounds: Bounds<Pixels>,
                                  (): (),
                                  window: &mut Window,
                                  _cx: &mut App| {
                                graph_painter::draw_graph_row(
                                    &graph_row,
                                    bounds,
                                    window,
                                    &lane_colors,
                                );
                            },
                        )
                        .w_full()
                        .h_full(),
                    )
                    .when_some(node_col, |el, col| {
                        let xc =
                            GRAPH_LEFT_PAD + col * COL_WIDTH + COL_WIDTH / 2.0;
                        let letters = node_letters.clone();
                        el.child(
                            div()
                                .absolute()
                                .left(px(xc - 12.))
                                .top(px(ROW_HEIGHT / 2.0 - 15.))
                                .w(px(24.))
                                .h(px(30.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(crate::theme::scaled_text_size(13.))
                                .text_color(initials_color)
                                .child(shared(letters)),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(HASH_COL_WIDTH))
                    .flex_shrink_0()
                    .font_family(options.mono.clone())
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.blue)
                    .child(shared(row.short)),
            )
            // Message column takes remaining space and truncates when needed.
            .when(options.show_message, |el| {
                el.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.foreground)
                        .child(shared(row.subject.clone())),
                )
            })
            // Ref chips follow the subject (or the hash on narrow rows).
            .when_some(ref_chips, |el, chips| el.child(chips))
            // Author column is fixed width and truncates long names.
            .when(options.show_author, |el| {
                el.child(
                    div()
                        .w(px(AUTHOR_COL_WIDTH))
                        .flex_shrink_0()
                        .truncate()
                        .text_size(crate::theme::scaled_text_size(12.))
                        .text_color(colors.foreground)
                        .child(shared(row.author)),
                )
            })
            // Date column displays the relative commit time.
            .child(
                div()
                    .w(px(DATE_COL_WIDTH))
                    .flex_shrink_0()
                    .text_size(crate::theme::scaled_text_size(12.))
                    .text_color(colors.muted_foreground)
                    .child(shared(format_relative_time(
                        row.timestamp,
                        options.now,
                        self.locale,
                    ))),
            );

        row_element
            .context_menu(move |menu, _window, _cx| {
                let graph_for_checkout = graph.clone();
                let graph_for_copy = graph.clone();
                let graph_for_copy_message = graph.clone();
                let graph_for_show_message = graph.clone();
                let commit_target = commit_target.clone();
                let copy_value = copy_value.clone();
                let copy_message_value = copy_message_value.clone();

                menu.item(
                    PopupMenuItem::new(checkout_label.clone())
                        .icon(crate::git::lucide("git-commit-horizontal"))
                        .disabled(checkout_disabled)
                        .on_click(move |_event, _window, cx| {
                            graph_for_checkout.update(cx, |_graph, cx| {
                                cx.emit(GraphEvent::CheckoutRef(
                                    commit_target.clone(),
                                ));
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new(copy_label.clone())
                        .icon(IconName::Copy)
                        .on_click(move |_event, _window, cx| {
                            graph_for_copy.update(cx, |_graph, cx| {
                                cx.emit(GraphEvent::CopyRef(
                                    copy_value.clone(),
                                ));
                            });
                        }),
                )
                .item(
                    PopupMenuItem::new(copy_message_label.clone())
                        .icon(IconName::Copy)
                        .disabled(checkout_disabled)
                        .on_click(move |_event, _window, cx| {
                            graph_for_copy_message.update(cx, |_graph, cx| {
                                cx.emit(GraphEvent::CopyCommitMessage(
                                    copy_message_value.clone(),
                                ));
                            });
                        }),
                )
                .item({
                    // The menu builder is `Fn`, so clone the snapshot per
                    // construction and hand the clone to the click handler.
                    let row = show_message_row.clone();
                    PopupMenuItem::new(show_message_label.clone())
                        .icon(IconName::Eye)
                        .on_click(move |_event, window, cx| {
                            graph_for_show_message.update(cx, |graph, cx| {
                                graph.open_commit_message_dialog(
                                    &row, window, cx,
                                );
                            });
                        })
                })
            })
            .into_any_element()
    }

    /// 列头（参考 rgitui graph header：26px 条、muted 半粗小标签、下边框；
    /// 列宽与行内列对齐：Graph 树列 / Hash / Message(flex) / Author / Date，
    /// 后三列随窗口变窄按 column_visibility 收缩至隐藏，与行同步）
    fn column_header(
        &self,
        colors: &ThemeColor,
        tree_w: f32,
        show_author: bool,
        show_message: bool,
    ) -> impl IntoElement {
        let label = |text: &str| {
            div()
                .text_size(crate::theme::scaled_text_size(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.muted_foreground)
                .child(shared(text))
        };
        // 列间分割短线：1px 宽、14px 高，absolute 定位在本列左缘左侧 4px
        // （= 与左邻列的 8px 间隙中点），不参与布局；随本列隐藏自动消失
        let col = |text: &str, w: f32, with_divider: bool| {
            div()
                .relative()
                .w(px(w))
                .flex_shrink_0()
                .when(with_divider, |el| {
                    el.child(
                        div()
                            .absolute()
                            .left(px(-4.))
                            .top(px(6.))
                            .w(px(1.))
                            .h(px(14.))
                            .bg(colors.border),
                    )
                })
                .child(label(text))
        };
        h_flex()
            .id("graph-header")
            .w_full()
            .h(px(26.))
            .flex_shrink_0()
            .pr_2()
            .gap_2()
            .items_center()
            .relative()
            .bg(colors.tab_bar)
            .border_b_1()
            .border_color(colors.border)
            // Graph 列：与行内树画布同宽
            .child(
                div()
                    .w(px(tree_w))
                    .flex_shrink_0()
                    .pl_1()
                    .child(label(&i18n::text(self.locale, "col-graph"))),
            )
            // Hash 列：与行内短 oid 同宽（左缘分割线隔开树列）
            .child(col(
                &i18n::text(self.locale, "col-hash"),
                HASH_COL_WIDTH,
                true,
            ))
            // Message 列：占满剩余（与行内 flex_1 对齐）
            .when(show_message, |el| {
                el.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .relative()
                        .child(
                            div()
                                .absolute()
                                .left(px(-4.))
                                .top(px(6.))
                                .w(px(1.))
                                .h(px(14.))
                                .bg(colors.border),
                        )
                        .child(label(&i18n::text(self.locale, "col-message"))),
                )
            })
            // Author 列：定宽（与行内 140 对齐）
            .when(show_author, |el| {
                el.child(col(
                    &i18n::text(self.locale, "col-author"),
                    AUTHOR_COL_WIDTH,
                    true,
                ))
            })
            // Date 列：与行内日期同宽
            .child(col(
                &i18n::text(self.locale, "col-date"),
                DATE_COL_WIDTH,
                true,
            ))
    }
}

/// Tint color per ref kind, mirroring the VS Code graph badge colors.
fn ref_label_color(colors: ThemeColor, kind: RefKind) -> Hsla {
    match kind {
        RefKind::Head => colors.red,
        RefKind::LocalBranch => colors.blue,
        RefKind::RemoteBranch => colors.green,
        RefKind::Tag => colors.yellow,
    }
}

/// One rounded ref badge drawn in the commit row.
fn ref_label_chip(label: &RefLabel, colors: ThemeColor) -> AnyElement {
    let color = ref_label_color(colors, label.kind);
    div()
        .flex_shrink_0()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(4.))
        .border_1()
        .border_color(Hsla { a: 0.35, ..color })
        .bg(Hsla { a: 0.14, ..color })
        .text_size(crate::theme::scaled_text_size(10.))
        .text_color(color)
        .child(shared(label.name.clone()))
        .into_any_element()
}

/// 宽度测量 canvas（w_full、0 高，不占布局）：prepaint 取自身 bounds 宽回写
/// GraphView。回写 defer 出布局阶段（prepaint 内直接 update 实体会重入布局），
/// 且仅宽度变化时 notify——收敛后不再触发重绘，拖拽连续变化也只是逐帧跟随
fn measure_width_canvas(entity: Entity<GraphView>) -> impl IntoElement {
    canvas(
        move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
            let w = f32::from(bounds.size.width);
            if w > 0.0 && w.is_finite() {
                cx.defer(move |cx: &mut App| {
                    entity.update(cx, |view, cx| {
                        if view.content_width != w {
                            view.content_width = w;
                            cx.notify();
                        }
                    });
                });
            }
        },
        |_bounds: Bounds<Pixels>,
         _state: (),
         _window: &mut Window,
         _cx: &mut App| {},
    )
    .w_full()
    .h(px(0.))
}
