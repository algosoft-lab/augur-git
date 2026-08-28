//! Sidebar sections for branches, refs, and working-tree changes.
//!
//! The sections are ordered as local branches, remotes, remote branches, tags,
//! stashes, staged changes, and unstaged changes. Section headers toggle their
//! contents, while checkoutable refs expose actions through a full-row context
//! menu.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenuItem},
    v_flex,
};

use crate::core::git::{BranchInfo, CheckoutTarget, FileStatus, RefsInfo};
use crate::core::i18n::{self, Locale};
use crate::git::shared;

/// 侧栏事件
pub enum SidebarEvent {
    /// 收起/展开侧栏
    ToggleCollapse,
    /// Select a branch for the detail panel.
    BranchSelected(String),
    /// Check out a branch, tag, or commit target.
    CheckoutRef(CheckoutTarget),
    /// Copy a displayed ref name to the system clipboard.
    CopyRef(String),
    /// Select a changed file for the detail panel.
    FileSelected {
        path: String,
        staged: bool,
        code: char,
    },
}

pub struct Sidebar {
    /// 本地分支列表
    branches: Vec<BranchInfo>,
    /// 当前分支
    branch: String,
    /// 变更文件（staged/unstaged 已分类）
    staged: Vec<FileStatus>,
    unstaged: Vec<FileStatus>,
    /// 引用清单（远程/远程分支/标签/stash，只读展示）
    refs: RefsInfo,
    /// 选中行（(是否暂存, 索引)）
    selected: Option<(bool, usize)>,
    /// 分支区高亮截止时刻（标题栏徽标点击，时间戳过期自动消失）
    flash_branches_until: Option<Instant>,
    /// 已折叠分区（i18n 键列表；内存态，重启恢复全展开）
    collapsed: Vec<&'static str>,
    /// 界面语言（Workspace 切换语言时同步）
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
            staged: Vec::new(),
            unstaged: Vec::new(),
            refs: RefsInfo::default(),
            selected: None,
            flash_branches_until: None,
            collapsed: Vec::new(),
            locale,
        }
    }

    /// 切换语言（Workspace::set_language 同步）
    pub fn set_locale(&mut self, locale: Locale, cx: &mut Context<Self>) {
        self.locale = locale;
        cx.notify();
    }

    /// 接收仓库状态快照（workspace 从 GitUiEvent 转发）
    pub fn set_status(
        &mut self,
        branch: String,
        branches: Vec<BranchInfo>,
        files: Vec<FileStatus>,
        cx: &mut Context<Self>,
    ) {
        self.branch = branch;
        self.branches = branches;
        self.staged = files.iter().filter(|f| f.is_staged()).cloned().collect();
        self.unstaged =
            files.iter().filter(|f| !f.is_staged()).cloned().collect();
        // 选中行可能已消失
        if let Some((staged, i)) = self.selected {
            let list = if staged { &self.staged } else { &self.unstaged };
            if i >= list.len() {
                self.selected = None;
            }
        }
        cx.notify();
    }

    /// 接收引用快照（workspace 从 GitUiEvent::RefsChanged 转发）
    pub fn set_refs(&mut self, refs: RefsInfo, cx: &mut Context<Self>) {
        self.refs = refs;
        cx.notify();
    }

    /// 标题栏分支徽标点击：自动展开分支区并高亮 800ms
    pub fn flash_branches(&mut self, cx: &mut Context<Self>) {
        self.flash_branches_until =
            Some(Instant::now() + Duration::from_millis(800));
        self.collapsed.retain(|k| *k != "section-branches");
        cx.notify();
    }

    fn is_collapsed(&self, key: &str) -> bool {
        self.collapsed.iter().any(|k| *k == key)
    }

    /// 点击分区标题：开合该分区
    fn toggle_section(&mut self, key: &'static str, cx: &mut Context<Self>) {
        match self.collapsed.iter().position(|k| *k == key) {
            Some(i) => {
                self.collapsed.remove(i);
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
        let Some(file) = list.get(index) else {
            return;
        };
        self.selected = Some((staged, index));
        cx.emit(SidebarEvent::FileSelected {
            path: file.path.clone(),
            staged,
            code: file.code(),
        });
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

        // 收起按钮（PanelLeftClose 图标）
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
            // 细顶行：仅右侧收起按钮（标题文案已并入全局标题栏）
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
                    ))
                    .child(self.change_section(cx, true, &self.staged))
                    .child(self.change_section(cx, false, &self.unstaged)),
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

    /// Staged and unstaged change sections.
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
            .map(|(i, f)| {
                let this = cx.entity();
                let (code_color, label) =
                    status_style(&colors, f.code(), self.locale);
                let selected = self.selected == Some((staged, i));
                h_flex()
                    .id(SharedString::from(format!("file-{staged}-{i}")))
                    .w_full()
                    .h(px(22.))
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
                    .hover(|this| {
                        if !selected {
                            this.bg(colors.list_hover)
                        } else {
                            this
                        }
                    })
                    .on_click(move |_e, _w, cx| {
                        this.update(cx, |sidebar, cx| {
                            sidebar.select_file(staged, i, cx)
                        });
                    })
                    .child(
                        div()
                            .w(px(24.))
                            .text_size(px(11.))
                            .text_color(code_color)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.))
                            .text_color(colors.foreground)
                            .child(shared(f.path.clone())),
                    )
            })
            .collect::<Vec<_>>();

        v_flex()
            .id(SharedString::from(format!("change-section-{staged}")))
            .w_full()
            .gap_0p5()
            .p_2()
            .child(section_header(
                cx,
                title_key,
                i18n::text(self.locale, title_key),
                files.len(),
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

/// 分区标题：折叠 chevron + 13px semibold 前景色标题 + 计数；整行点击开合
/// （GitHub Dark 风格：弃蓝色文字与焦点竖条）
/// （flash 为分支区高亮态背景；id 用稳定的 i18n 键——标题文本随语言变，不能当 id）
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

/// 状态码 → 颜色/标签（标签双语：zh 单字 改/增/删/移/拷/冲，en 字母 M/A/D/R/C/U）
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
