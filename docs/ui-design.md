# UI 设计决策（M1.5 视觉规范）

拷问定稿（grilling 三轮），骨架不动，视觉全量换 GitHub Dark。

## 色板（GitHub Dark → ThemeColor 全量覆写）

| token | 色值 | 用途 |
|---|---|---|
| background | `#0d1117` | 主背景（图/状态栏/列表底） |
| tab_bar / title_bar | `#161b22` | 面板二级表面（各面板头/侧栏条/标题栏） |
| input / list_hover | `#21262d` | 浮起元素、hover |
| list_active | `#264f78` | 选中行 |
| border | `#30363d` | 边框 |
| foreground | `#e6edf3` | 正文 |
| muted_foreground | `#8b949e` | 弱文字（含表头） |
| blue（accent 用色） | `#2F81F7` | 徽标/hash/主按钮 |
| green / red / warning | `#3fb950` / `#f85149` / `#d29922` | 语义色 |
| drag_border | `#388bfd` | 拖拽提示 |

提交图 lane 彩色保留现值（高饱和中亮度，`#0d1117` 上对比度足够）；节点空心描边圆不变。

## 布局

```
TitleBar（36px 合并行）：logo ⎇ + augur-git + 仓库 tab（pill：名+×关闭）
                        …spacer… 分支徽标 [窗口控制区]
Toolbar 32px：Fetch/Pull/Push/Branch（图标+文字，ghost）+ ↑↓ 徽标
              …spacer… busy Spinner / 刷新 / 设置
[侧栏(可拖) | 中列 GraphView+底部面板 | 右栏 详情 tab+CommitPanel]
StatusBar 24px：左路径 … 右 消息(绿成功/红失败)+状态点
```

- 原 TitleBar 与 TabBar 两条合并为一条，省 28px 给提交图
- M2 多 tab 预留规范：tab 为 pill 链横向排列，溢出横向滚动，尾部 `+` pill（plus 图标）；本次只实现单 tab

## 字号阶梯（4px 网格）

- 微 11px：状态栏、徽标计数、时间戳、列头
- 正文 12px：列表行、按钮、输入框、diff
- 标题 13px semibold：侧栏分区头
- 列表行高 22px；面板内边距 p_2；区块间隙 gap_2
- 提交图行高保持 36px（树绘制几何依赖 ROW_HEIGHT）

## 图标（lucide 线性）

gpui-component 内置集缺 git 类图标 → `assets/icons/*.svg` 自带 4 枚 lucide（MIT）：
download(Fetch)、git-branch(Branch/标题栏徽标)、refresh-cw(刷新)、git-commit-horizontal(空态)。
main.rs 合并 AssetSource：本地 assets 优先，回落 gpui_component_assets。

替换清单：工具栏按钮图标化、tab 关闭×→close、侧栏折叠箭头→chevron-right/down、
收起/展开→panel-left-close/open、commit 收起→chevron-up/down、busy→Spinner(动画)、
ahead/behind↑↓→arrow-up/down。

## 组件规范

- **空态**：居中 lucide 图标（24px muted）+ 一行 11px 提示；用于 DetailPanel 未选、
  BottomPanel 未选提交、文件清单空（合并提交）、diff 未选文件、GraphView 无行
- **Diff**：unified 保持；每行前置 双列行号 gutter（旧/新，右对齐 mono 灰，
  由 core::git::parse_diff_hunks 纯函数跟踪 @@ 头计数，含单测）；hunk 头行紫字
  (#bc8cff)+10% 紫底通栏；diff 正文 12px mono；不做 side-by-side（以后有需求再加）
- **CommitPanel**：位置不动（右栏底）；无 staged 时提交按钮不挂 on_click（灰态即禁用）
- **操作反馈**：无横幅区——状态栏消息按成功绿/失败红着色，忙碌=工具栏 Spinner
- **Welcome 页**：轻抛光——logo 卡（#161b22 底+边框+git-branch 蓝图标）、recents 行加 folder 图标

## i18n

空态复用现有键（graph-empty/detail-empty/bottom-no-commit/bottom-no-file/
bottom-merge-empty/diff-no-output），零新增键。
