# augur-git 简体中文（zh-CN）翻译
# 格式：key = value，一行一条；{ $name } 为占位符（src/core/i18n.rs::text_args 替换）。

# ===== 应用 / Welcome =====
app-tagline = 桌面 Git 客户端
welcome-open = 打开
welcome-drop-hint = 或拖入仓库文件夹打开
tab-new = 新建标签页
recent-repos = 最近仓库
repo-folder-prompt = 选择 Git 仓库文件夹

# ===== Application menu / About =====
menu-open = 打开应用菜单
menu-file = 文件
menu-open-repository = 打开仓库…
menu-new-tab = 新建标签页
menu-recent-repositories = 最近仓库
menu-no-recent-repositories = 没有最近仓库
menu-edit = 编辑
menu-settings = 设置
menu-help = 帮助
menu-about = 关于 augur-git
menu-quit = 退出
about-title = 关于
about-tagline = 桌面 Git 客户端
about-author = 作者
about-version = 版本
about-commit = 提交

# ===== Tab 栏 / 状态栏 =====
no-repo-open = 未打开仓库
status-scanning = 扫描中…
status-no-repo-selected = 未选择仓库
status-scanning-at = 扫描中 @ { $repo }
status-summary = { $branch } · ↑{ $ahead }↓{ $behind } · 暂存{ $staged } 变更{ $unstaged }

# ===== 命令结果 / 消息 =====
command-success = { $label } 成功
command-failed = { $label } 失败：{ $error }
branch-selected = 已选择分支 { $name }
context-checkout = 切换
context-copy-branch = 复制分支名称
context-copy-tag = 复制标签名称
context-copy-commit = 复制提交哈希
context-copy-commit-message = 复制提交信息
context-show-commit-message = 查看完整提交信息
context-copied = 已复制 { $name }
context-copied-commit-message = 已复制提交信息
context-copy-commit-message-failed = 复制提交信息失败：{ $error }
checkout-title = 切换到 { $name }？
checkout-description = 将工作区切换到 { $name } 吗？
checkout-cancel = 取消
push-force-title = 强制推送？
push-force-warning = 强制推送会覆盖远程分支 { $branch } 的历史，仅存在于远程的提交可能丢失。
push-force-confirm = 强制推送
push-force-cancel = 取消

# ===== 工具栏 =====
toolbar-fetch = 获取
toolbar-pull-merge = 拉取（合并）
toolbar-pull-rebase = 拉取（变基）
toolbar-push = 推送
toolbar-push-force = 推送（强制）
toolbar-branch = 分支
toolbar-compare = 比较
toolbar-refresh = 刷新
toolbar-settings = 设置
toolbar-busy = 操作中…

# ===== 提交图 =====
graph-empty = 暂无提交
col-graph = 图形
col-hash = 哈希
col-message = 信息
col-author = 作者
col-date = 日期
rel-now = 刚刚
rel-min = { $n } 分钟前
rel-hour = { $n } 小时前
rel-day = { $n } 天前
rel-week = { $n } 周前
rel-month = { $n } 个月前
rel-year = { $n } 年前

# ===== 侧栏 =====
sidebar-repo = 仓库
section-branches = 分支
section-remotes = 远程
section-remote-branches = 远程分支
section-tags = 标签
section-stashes = 贮藏
section-staged = 暂存
section-changes = 变更
changes-title = 工作区
changes-empty = 暂无变更
changes-refresh = 刷新变更
changes-more = 更多变更操作
changes-stage = 暂存变更
changes-unstage = 取消暂存
changes-discard = 丢弃变更
changes-stage-all = 暂存全部变更
changes-unstage-all = 取消全部暂存
changes-discard-all = 丢弃全部变更
changes-action-conflict = 冲突文件不可用
changes-stage-success = 已暂存变更
changes-stage-all-success = 已暂存全部变更
changes-unstage-success = 已取消暂存变更
changes-unstage-all-success = 已取消全部暂存
changes-discard-success = 已丢弃变更
changes-discard-all-success = 已丢弃全部变更
changes-operation-failed = 工作区操作失败：{ $error }
status-mod = 改
status-add = 增
status-del = 删
status-ren = 移
status-cpy = 拷
status-conflict = 冲
status-unknown = ?

# ===== 提交信息悬停预览 =====
commit-message-preview = 提交信息
commit-message-dialog-title = 完整提交信息
commit-message-loading = 正在加载完整提交信息…
commit-author = 作者 { $author }
commit-date = 时间 { $date }
commit-coauthors = 共同作者

# ===== 提交面板 =====
commit-title = 提交
commit-placeholder = 提交信息
commit-btn = 提交
commit-amend-btn = 修改
commit-action-commit = 提交
commit-action-amend = 修改上次提交

# ===== 底部面板（选中提交文件清单 + 单文件 diff） =====
bottom-no-commit = 未选择提交
bottom-merge-empty = 合并提交相对第一父提交没有文件变化
bottom-no-changes = 此提交没有文件变化
bottom-no-file = 点击左侧文件查看 diff
bottom-bin = 二进制
diff-all-files = 全部变更文件
diff-merge-first-parent = 相对第一父提交
diff-no-output = (无输出)
diff-working-tree-staged = 暂存
diff-working-tree-changes = 变更
diff-working-tree-loading = 正在加载工作区 diff…
diff-working-tree-error = 无法加载工作区 diff

# ===== 版本比较 =====
branch-compare-title = 版本比较
branch-compare-back = 返回仓库
branch-compare-base = 基准
branch-compare-target = 目标
branch-compare-local = 本地
branch-compare-remote = 远程
branch-compare-tag = 标签
branch-compare-commit = 提交
branch-compare-run = 比较
branch-compare-refresh = 刷新
branch-compare-loading = 正在加载比较…
branch-compare-revision-placeholder = Branch、Tag、Commit 或 SHA
branch-compare-branches = 分支
branch-compare-tags = 标签
branch-compare-commits = 提交
branch-compare-use-commit = 使用提交 SHA { $sha }
branch-compare-no-matches = 没有匹配的版本
branch-compare-invalid-revision = 请输入分支、标签、提交，或 7–64 位十六进制 SHA
branch-compare-revision-unavailable = 此引用已不可用
branch-compare-all-files = 全部变更文件
branch-compare-no-changes = 所选版本没有文件变化
branch-compare-select-hint = 选择两个版本后开始比较
branch-compare-select-file = 选择文件查看 diff
branch-compare-error = 无法加载版本比较

# ===== 分支操作（工具栏 Branch 菜单） =====
menu-branch-new = 新建分支…
menu-branch-rename = 重命名分支…
menu-stash = 贮藏…
menu-stash-pop = 弹出贮藏
menu-merge = 合并…
menu-merge-no-ff = 合并（--no-ff）…
menu-rebase = 变基…
branch-new-title = 新建分支
branch-rename-title = 重命名分支
branch-name-label = 分支名称
branch-new-hint = 将基于 { $branch } 创建并切换到新分支。
branch-rename-hint = 将重命名分支 { $branch }。
branch-name-invalid = 分支名称不合法：不能含空格或 ~ ^ : ? * [ \ 等字符，不能有 ".."、以 "-" 开头、以 ".lock"、"/" 或 "." 结尾。
branch-name-exists = 分支“{ $name }”已存在。
merge-title = 合并到 { $branch }
merge-source-label = 要合并的分支
merge-no-ff-label = 即使可以快进也生成合并提交（--no-ff）
rebase-title = 将 { $branch } 变基到
rebase-warning = 变基会改写 { $branch } 的提交历史，请勿对已推送共享的分支执行变基。
stash-title = 贮藏更改
stash-message-label = 备注（可选）
stash-hint = 将贮藏 { $count } 个有改动的文件。
dialog-cancel = 取消
dialog-confirm = 确认

# ===== 侧栏引用操作 =====
context-rename = 重命名…
context-delete = 删除…
context-merge-into-current = 合并到当前分支
context-merge-no-ff-into-current = 合并到当前分支（--no-ff）
delete-branch-title = 删除分支
delete-tag-title = 删除标签
delete-branch-warning = 将从仓库中删除分支 { $name }。仅被该分支引用的提交可能因此无法访问。
delete-tag-warning = 将从仓库中删除标签 { $name }。
delete-force-label = 强制删除，即使存在未合并的提交（-D）

# ===== 危险工作区操作 =====
discard-title = 丢弃变更？
discard-file-warning = 确定要丢弃 { $path } 的变更吗？已暂存内容会保留。此操作无法撤销。
discard-untracked-file-warning = 确定要永久删除未跟踪文件 { $path } 吗？此操作无法撤销。
discard-all-warning = 确定要丢弃全部工作区变更吗？已暂存内容会保留。tracked 文件：{ $tracked } 个；将永久删除的未跟踪文件：{ $untracked } 个。此操作无法撤销。
discard-cancel = 取消
discard-confirm = 丢弃

# ===== 设置面板 =====
settings-title = 设置
settings-description = 应用偏好设置
settings-general = 常规
settings-appearance = 外观
settings-layout = 布局
language-title = 界面语言
language-system = 跟随系统
language-chinese = 简体中文
language-english = English
auto-refresh-on-focus-title = 窗口聚焦时刷新
setting-enabled = 启用
setting-disabled = 禁用
settings-close = 关闭
theme-title = 主题
ui-font-title = 界面字体
mono-font-title = 等宽字体
font-system-default = 系统默认
font-search-placeholder = 搜索已安装字体…
layout-persistence-description = 面板尺寸、左栏折叠状态和窗口几何信息会在应用关闭时保存。
diff-layout-title = Diff 布局
diff-layout-inline = 内联
diff-layout-side-by-side = 并排
graph-history-title = 提交图历史范围
graph-history-current = 当前分支及其上游
graph-history-all = 所有分支
graph-history-description = 当前模式显示当前分支、其跟踪的上游分支及它们可达的历史。
theme-github-dark = GitHub Dark
theme-catppuccin-latte = Latte
theme-catppuccin-frappe = Frappé
theme-catppuccin-macchiato = Macchiato
theme-catppuccin-mocha = Mocha

# ===== 错误（core 产生 key，展示侧拼接本地化） =====
err-path-not-exist = 路径不存在: { $detail }
err-not-a-repo = 不是 Git 仓库: { $detail }
err-git-run = git 执行失败: { $detail }
err-git-status = 读取 Git 状态失败: { $detail }
err-git-status-path = Git 返回了无法安全处理的路径: { $detail }
err-git-log = git log 失败: { $detail }
err-numstat = git show --numstat 失败: { $detail }
err-file-diff = 读取文件 diff 失败: { $detail }
err-commit-message = 读取提交信息失败: { $detail }
