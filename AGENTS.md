# AGENTS.md

本文件为在此仓库工作的 AI 编码代理（ZCode / Claude 等）提供指引。

## 项目是什么

augur-git 是暗黑主题 **桌面 Git 图形客户端**（Rust + GPUI，非 TUI）。
同源项目家族：`../augur-term`（终端）、`../augur-com`（串口调试助手）——本项目的
主界面布局镜像 `D:\dev\github\rgitui`，工程架构镜像 `../augur-com`。

## 参考项目（重要）

**`D:\dev\github\rgitui`** —— GPU 加速桌面 Git 客户端，与本项目同技术栈（GPUI）。
实现界面/交互前先读它的源码，按需借鉴：

### rgitui 主界面布局（自上而下，`crates/rgitui_workspace/src/workspace/layout.rs`）

```
TitleBar   仓库名 + 分支徽标（点击跳侧栏分支区）、has_changes/detached/合并状态指示
Toolbar    左：Fetch/Pull/Push/Branch/Stash/CreatePR + ahead/behind 徽标
           右：文件管理器/终端/编辑器/搜索/刷新/设置
[横幅区]   操作进行中/失败横幅、冲突状态横幅（Continue/Abort）
TabBar     每打开一个仓库一个 tab + 尾部 Home/Add 按钮
Main content (h_flex, flex_1)：
├─ 左 Sidebar（固定宽可拖拽 180..720 级）  分区：本地分支/远程/标签/暂存/工作区变更
├─ 中央列 (flex_1)：
│  ├─ GraphView（提交图，uniform_list + canvas 画 lane 连线，顶部 loading 条）
│  ├─ 拖拽条（3px，纵向）
│  └─ 底部面板（固定高可拖拽）  tab：Diff/History/Blame/Reflog/Submodules/…
└─ 右面板（固定宽可拖拽 180..720）：
   ├─ 右 tab 栏（Details/Issues/PRs/Branch Health）
   ├─ DetailPanel（选中提交详情：消息/作者/时间/文件树）
   ├─ 拖拽条
   └─ CommitPanel（提交信息输入区，可收起）
StatusBar  分支 · ahead/behind · staged/unstaged 数 · stash 数 · 仓库路径 · 操作消息
```

无仓库时显示 **Welcome 页**：Logo + 应用名 + Open Repository / New Workspace + 最近仓库
（`layout.rs::render_welcome_interactive`）。

### rgitui 提交树渲染规范（GraphView，照抄）

参考：`crates/rgitui_graph/src/lib.rs` + `crates/rgitui_git/src/graph.rs`

**数据**：`compute_graph(commits) -> Vec<GraphRow>` 算 lane 布局（本项目 `src/core/graph.rs` 已移植）
- `GraphRow { node_lane, edges: Vec<GraphEdge>, lane_count, node_color, has_incoming, is_head, is_merge }`
- `GraphEdge { from_lane, to_lane, color_index, is_merge }`

**渲染**（每行 h_flex 内嵌 canvas）：
```
div().relative().w(px(tree_w)).flex_shrink_0().h_full().child(
    canvas(
        |_bounds, _w, _cx| {},
        |bounds, (), window, _cx| { /* 用 window.paint_path 画 */ },
    ).w_full().h_full()
)
```
- canvas paint 的 `bounds.origin` 是**窗口坐标**（滚动后自动正确），行 y = `origin.y`，节点 x = `origin.x + lane_x`
- **节点圆**：`build_filled_circle`（`PathBuilder::fill` + 36 点多边形）+ `paint_path`
  - 背景环（遮穿线）：fill 大圆 + `paint_path(row_bg)`
  - 空心圆：fill 节点色大圆 + fill 背景色小圆挖空
- **连线**：`PathBuilder::fill` 4 点矩形（细线）/ 点阵小圆（斜线）+ `paint_path`
- **配色**：`lane_color(index)` 配色表（照抄 `GRAPH_LANE_COLORS`）

**关键 API**：`gpui::canvas(prepaint, paint)`、`PathBuilder::fill()/stroke()`、`window.paint_path(path, color)`、`build_filled_circle`

**坑（已踩）**：
- `PathBuilder::stroke` 在本 fork 渲染异常 → 全用 `fill`（圆 = fill 多边形，线 = fill 矩形）
- `h_flex()` 强制 `items_center` → 子元素 `h_full` 失效；树列容器用 `div().flex().flex_row()`
- 多个 `on_click(move)` 捕获同一 `Entity` → 第二个报 moved，用 `entity.clone()`

### rgitui 可借鉴的架构决策

- **多 tab 工作区**：`Workspace` 持有 `Vec<ProjectTab>`（每仓库一个），tab 内装各 panel 的 `Entity`
- **事件链**：子实体 `EventEmitter<XxxEvent>`，Workspace 在 `events.rs` 统一 `subscribe_xxx`
- **线程规则**：git2/FS 工作绝不进 UI 线程；`background_executor().spawn` + 世代号丢弃过期结果
- **布局状态**：`LayoutState` 记录 sidebar/detail/diff/commit 面板尺寸，拖拽条 `on_drag` + `on_drag_move`
- **状态栏/工具提示**：命令的快捷键绑定动态显示在 tooltip 里
- **h_flex() 坑**：`h_flex()` = `flex_row().items_center()`（强制垂直居中），滚动容器/自绘对齐
  出问题时改用 `div().flex().flex_row()`

## 本项目工程架构（镜像 augur-com）

```
src/
├── main.rs            # 入口（12 行）
├── workspace.rs       # Workspace 装配 + 三栏布局 + 事件链 + 状态栏
├── core/
│   ├── git.rs         # Git 命令层：工作线程双通道（std mpsc）+ 输出解析（纯函数可单测）
│   └── config.rs      # AppConfig + config.json（%APPDATA%\augur-git\）+ MRU
└── git/               # 界面面板（镜像 rgitui 的 panel 拆分）
    ├── mod.rs         # GitView：仓库数据流中心（status/log 事件轮询派发）
    ├── graph.rs       # GraphView：提交图
    ├── sidebar.rs     # Sidebar：分支 + 变更文件分区
    ├── panel.rs       # DetailPanel / CommitPanel / DiffViewer
    └── toolbar.rs     # Toolbar
```

**线程模式**（全项目铁律，改代码前先看 `core/git.rs`）：
- 专用工作线程跑阻塞式 git 子进程 → `std::sync::mpsc` 事件推 UI（20ms 轮询 `try_recv`）
- UI → 后台指令：std mpsc `send`（无界通道，即发即返）；UI 线程零阻塞

**约定**：
- 注释用中文，模块头写里程碑（M0 框架 / M1 主界面 / …）
- 视图层可测逻辑（解析/算法）抽成纯函数放 `core/` 并写 `#[test]`
- `gpui_component` 尺寸 helper：半档是 `0p5` 后缀（`gap_0p5` 非 `gap_0_5`）
- `Entity::read` 必须传 `cx`；程序化回填 `InputState` 只能在带 `&mut Window` 的回调里
- 面板聚焦指示：选中面板画 `border_t_2 + border_focused` 色（rgitui 同款）

## 常用命令

```bash
cargo check        # 快速验证（本项目标准做法）
cargo test         # 单测（config / status / log 解析）
cargo run          # 运行
cargo fmt --all
```
