# augur-git 开发指南

暗黑主题 Git 图形客户端，Rust + GPUI，**架构镜像 `../augur-com`**（同源同风格，领域层按 Git 场景重设计）。

## 当前状态（2026-08-13）

| 项 | 状态 |
|---|---|
| 编译 | ✅ `cargo check` / `cargo build` 通过（0 error / 0 warning） |
| 单测 | ✅ `cargo test` 6 passed（config roundtrip / defaults / MRU / status 解析） |
| 启动验证 | ✅ 应用存活 12s+，窗口渲染正常（1280×800，暗黑主题） |
| M0 框架 | ✅ 三区布局 + 仓库状态视图 + 配置持久化 + 后台线程双通道 |

## 快速开始

```bash
cargo build                 # 编译（首次拉 GPUI 依赖较慢，之后增量）
cargo run                   # 运行（打开 1280×800 暗黑窗口）
cargo test                  # 单测（config + status 解析）
```

**联调路径**：任意本地 Git 仓库（如本仓库 `D:\dev\gitee\augur-git`）→ 侧栏输入路径 → 回车/打开 →
状态区显示分支 + 变更文件列表（M/A/D/?? 着色），状态栏显示分支与变更数。

## 架构（镜像 augur-com）

- **三区布局**：`TitleBar`（系统按钮 DWM 绘制，勿自绘）+ `侧栏(250px 可收起)` + `状态区` + `状态栏`
- **后台线程 + 双通道**（镜像 `augur-com/src/core/serial.rs`）：
  - 专用工作线程跑阻塞式 `git` 子进程 → `std::sync::mpsc` 事件推 UI（20ms 轮询 `try_recv`）
  - UI → 后台指令：std mpsc `send`（无界通道，即发即返）
  - 读写全走后台线程，UI 线程零阻塞
- **实体化 + 事件链**：`Entity<T>` / `cx.new` / `EventEmitter` / `cx.subscribe`，
  回调统一 `let this = cx.entity();` + `this.update(cx, ...)`
- **单一事实源**：`Workspace` 持有 `AppConfig`，侧栏/状态区任何变更经事件链回流 → `config::save()` 即存盘
- **Git 访问**：当前调用系统 `git` 可执行文件（PATH 查找，零额外依赖）；
  后续里程碑可换 `git2`/libgit2 做对象级访问（提交树、diff 高亮）

## 模块结构

```
src/
├── main.rs            # 入口（12 行，镜像 augur-com）
├── workspace.rs       # Workspace 装配 + 事件链 + 状态栏 + 侧栏（仓库路径/最近仓库）
├── core/
│   ├── mod.rs
│   ├── git.rs         # Git 命令层：spawn_open/工作线程/status 解析/双通道
│   └── config.rs      # AppConfig + config.json 持久化（%APPDATA%\augur-git\）
└── git/
    └── mod.rs         # GitView：分支徽标 + 变更文件列表（着色）+ 占位区块（M2/M3）
docs/
└── DEV_GUIDE.md       # 本文件
```

## 事件链速览

```
Sidebar(路径输入回车/打开/刷新/最近仓库) --emit--> SidebarEvent
    --subscribe--> Workspace: git_view.open_repo / refresh
                    （最近仓库：sidebar 内先 set_value 回填输入框再 emit）
GitView(状态/错误/打开成功) --emit--> GitUiEvent
    --subscribe--> Workspace: 状态栏 / config.repo.path + MRU 更新并 save / sidebar.set_recent
工作线程 --Status/Error--> poll_events(20ms) -> 分支+文件列表刷新 -> emit GitUiEvent::StatusChanged
```

## 已踩的坑（实现记录，改代码前先看）

0. **gpui-component 尺寸 helper 的命名**：半档间距/内边距是 `0p5` 后缀（`gap_0p5`/`py_0p5`），
   不是 `0_5`！整档 `h_7`(28px)/`h_9`(36px) 正常。尺寸方法由 gpui 的 `style_helpers!` 宏生成，
   写死前先查 `crates/gpui_macros/src/styles.rs` 的 suffix 列表。
1. **InputState 无窗口不可回填**：`set_value` 需要 `&mut Window`，只能在 `on_click`/`on_enter`
   这类带 window 的回调里调用（gpui 的 `Context` 无 `window()` 访问器）。
2. **Entity::read 必须传 cx**：`entity.read(cx)`（gpui 0.2.x），无参版本不存在。
3. **porcelain v1 非 ASCII 转义**：`git status --porcelain` 对非 ASCII 文件名输出
   `"文件\346\226\207\344\273\266"`（引号+八进制），当前未反转义；重命名行
   `R  old -> new` 只取箭头后路径。M4 里程碑处理。
4. **`git status` 的 `##` 行**：detached HEAD 输出 `## HEAD (no branch)`，分支名取 `...` 前段
   时该场景天然正确（无 `...` 即整行）。

## 后续里程碑

- **M2 提交历史**：`git log --oneline --decorate` 列表 + 行缓存渲染（镜像 augur-com 的 RowCache）
- **M3 差异视图**：`git diff` 输出解析 + 着色渲染
- **M4 提交/分支操作**：暂存/提交/拉取/推送 + MSIX 打包（脚本从 augur-com 拷贝）
