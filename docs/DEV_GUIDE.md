# augur-git 开发指南

暗黑主题 Git 图形客户端，Rust + GPUI。**主界面镜像 `D:\dev\github\rgitui`**（见 AGENTS.md），
工程架构镜像 `../augur-com`（双通道线程 + 事件链 + 配置持久化）。

## 当前状态（2026-08-13）

| 项 | 状态 |
|---|---|
| 编译 | ✅ `cargo build` 通过（0 error / 0 warning） |
| 单测 | ✅ `cargo test` 9 passed（config / status 解析 / log 解析） |
| 启动验证 | ✅ 应用存活 15s+，窗口渲染正常（1280×800 暗黑，自绘标题栏） |
| M0 框架 | ✅ 三区布局 + 后台线程双通道 + 配置持久化 |
| M1 主界面 | ✅ 镜像 rgitui 布局：TitleBar/Toolbar/TabBar/三栏（侧栏·提交图·详情）/状态栏/Welcome |
| M2+ | ⏳ 暂存/提交/分支操作增强、提交历史、diff 高亮、多仓库 tab、打包 |

## 快速开始

```bash
cargo build                 # 编译（首次拉 GPUI 依赖较慢，之后增量）
cargo run                   # 运行
cargo test                  # 单测
```

**联调路径**：任意本地 Git 仓库（如本仓库 `D:\dev\gitee\augur-git`）→ 侧栏输入路径 → 回车/打开 →
- 中列提交图显示分支线 + 提交（单击 → 右栏详情，双击 → 底部 diff）
- 侧栏分支区（点击 ⇥ 切换分支）/ 暂存 / 变更区（点击行 → 详情，✎ → diff）
- 工具栏 Fetch/Pull/Push（真实执行，状态栏回报），状态栏显示分支/↑↓/暂存/变更数

## 主界面布局（镜像 rgitui layout.rs）

```
TitleBar（自绘，无原生标题栏）  仓库名 + 分支徽标（点击 → 侧栏分支区闪烁）+ 双击最大化
Toolbar                        Fetch/Pull/Push/分支 + ↑ahead↓behind 徽标 + 刷新/设置
TabBar                         单仓库 tab（多仓库 M2）+ 连接状态圆点
三栏（拖拽条调宽，rgitui 同款 on_drag + on_drag_move）：
├─ 侧栏 250px(180..400)        仓库路径输入/打开/刷新 + 分支区 + 暂存区 + 变更区 + 最近仓库
├─ 中列 flex_1                 GraphView（git log --graph 行 + 装饰徽标）+ 3px 拖拽条
│                              + 底部面板 260px(100..500)：Diff/历史/Blame tab
└─ 右栏 320px(220..600)        详情 tab 栏 + DetailPanel + CommitPanel（Ctrl+Enter 提交）
StatusBar                      仓库路径 · 分支 · ↑↓ · 暂存/变更数 · 操作消息
```

无仓库时显示 **Welcome 页**：Logo + 应用名 + 最近仓库列表（点击直接打开）。

## 架构（镜像 augur-com）

- **后台线程 + 双通道**：专用工作线程跑阻塞式 `git` 子进程 → `std::sync::mpsc` 事件推 UI
  （20ms 轮询 `try_recv`）；UI → 后台指令 std mpsc `send` 即发即返，UI 线程零阻塞
- **数据中枢**：`GitView`（src/git/mod.rs）不渲染，持有工作线程句柄并轮询事件；
  快照数据（分支/变更/日志/命令结果）经 `GitUiEvent` 事件链分发给各面板
- **面板交互**：各面板 `EventEmitter<XxxEvent>` → Workspace 汇总 → `GitView::run()` 下发命令
- **单一事实源**：`Workspace` 持有 `AppConfig`，仓库路径/MRU 变更即存盘
- **Git 访问**：调用系统 `git` 可执行文件；解析均为纯函数（core/git.rs，可单测）

## 模块结构

```
src/
├── main.rs            # 入口（12 行）
├── workspace.rs       # Workspace 装配 + 三栏布局 + 拖拽调宽 + 事件链 + 状态栏 + Welcome
├── core/
│   ├── mod.rs
│   ├── git.rs         # Git 命令层：status/branch/log/通用命令 + 纯函数解析
│   └── config.rs      # AppConfig + config.json（%APPDATA%\augur-git\）+ MRU
└── git/
    ├── mod.rs         # GitView：数据中枢（事件轮询派发）
    ├── graph.rs       # GraphView：提交图（lane 前缀 + 装饰徽标 + 单击/双击）
    ├── sidebar.rs     # Sidebar：仓库入口 + 分支/暂存/变更分区
    ├── panel.rs       # DetailPanel + CommitPanel + DiffViewer
    └── toolbar.rs     # Toolbar：fetch/pull/push + ahead/behind
docs/
└── DEV_GUIDE.md
AGENTS.md              # 参考项目 rgitui 说明 + 工程规范（改界面先读）
```

## 事件链速览

```
Sidebar(打开/刷新/切换分支/选文件/diff) --SidebarEvent--> Workspace
Toolbar(Fetch/Pull/Push/Refresh)         --ToolbarEvent--> Workspace
GraphView(选中提交/双击diff)              --GraphEvent-->  Workspace
CommitPanel(Ctrl+Enter/按钮提交)          --CommitPanelEvent--> Workspace
    ↓ 汇总
GitView --run(label, args)--> 工作线程 git 子进程
    ↓ 事件回流
GitUiEvent{Status/Log/CommandDone/RepoOpened/Error} --> 各面板 + 状态栏 + config 存盘
（commit/checkout/fetch/pull/push 成功后自动 Refresh）
```

## 已踩的坑（实现记录，改代码前先看）

0. **gpui-component 尺寸 helper**：半档是 `0p5` 后缀（`gap_0p5`/`py_0p5`），不是 `0_5`；
   `h_7`(28px)/`h_9`(36px) 存在；`ThemeColor` 无 `element_selected`/`ghost_element_hover`
   （rgitui 有）—— 用 `list_active`/`list_hover`；无 `border_focused` —— 用 `drag_border`。
1. **`on_drag` 的 `W: Render` 约束**：拖拽类型必须实现 `Render`（rgitui 同款空元素实现，
   见 workspace.rs 顶部 SidebarResize 等）。
2. **`on_drag_move` 回调无 `&mut Self`**：需 `cx.listener(|this, e, window, cx| ...)` 包装。
3. **事件订阅回调里的字段是引用**（`event: &E`）：传参/构造时统一 `.clone()`/解引用。
4. **多个 `on_click(move)` 捕获同一 `Entity`**：第二次捕获报 moved —— 用 `entity.clone()`。
5. **`h_flex()`/`v_flex()` 陷阱**：gpui-component 的自由函数自动 `items_center()`（rgitui
   文档同款坑），滚动容器/自绘对齐出问题时用 `div().flex().flex_row()/.flex_col()`。
   另外 `div().v_flex()` 在本 fork 不存在 —— 用 `.flex().flex_col()`。
6. **InputState 无窗口不可回填**：`set_value` 需要 `&mut Window`，只能在 `on_click` 回调里调；
   `Entity::read` 必须传 `cx`。
7. **porcelain v1 非 ASCII 转义**：`git status --porcelain` 对非 ASCII 文件名输出
   `"文件\346\226\207\344\273\266"`（引号+八进制），当前未反转义；重命名行只取箭头后路径。M4 处理。
8. **git log --graph 解析**：graph 区字符集 `| * / \ _ . -` + 空格（不含 a-f），
   跳过 graph 区后必以 40-hex 开头；字段 NUL 分隔；`--date=format:` 预格式化日期。
   已用真实输出验证（graph='* ' oid_len=40 fields=6）。
9. **本 fork 无 `whitespace_pre_wrap`**：diff 文本按行拆分渲染（等宽字体逐行 child）。

## 后续里程碑

- **M2 暂存/提交增强**：文件行暂存/取消暂存（git add/reset）、提交后清空输入、多仓库 tab、
  设置面板、分支详情
- **M3 diff 高亮**：git diff 输出着色（+ 绿 / - 红）+ 行缓存渲染
- **M4 提交历史/分支操作**：checkout 确认、stash、tag、MSIX 打包
