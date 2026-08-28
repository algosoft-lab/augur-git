# augur-git 简体中文（zh-CN）翻译
# 格式：key = value，一行一条；{ $name } 为占位符（src/core/i18n.rs::text_args 替换）。

# ===== 应用 / Welcome =====
app-tagline = 桌面 Git 客户端
welcome-open = 打开
welcome-browse = 浏览…
recent-repos = 最近仓库
repo-path-placeholder = 仓库路径，如 D:\repo
repo-folder-prompt = 选择 Git 仓库文件夹

# ===== Tab 栏 / 状态栏 =====
no-repo-open = 未打开仓库
status-scanning = 扫描中…
status-no-repo-selected = 未选择仓库
status-scanning-at = 扫描中 @ { $repo }
status-summary = { $branch } · ↑{ $ahead }↓{ $behind } · 暂存{ $staged } 变更{ $unstaged }

# ===== 命令结果 / 消息 =====
command-success = { $label } 成功
command-failed = { $label } 失败：{ $error }
branch-selected = 分支 { $name }（详情 M2）

# ===== 工具栏 =====
toolbar-fetch = 获取
toolbar-pull = 拉取
toolbar-push = 推送
toolbar-branch = 分支
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
status-mod = 改
status-add = 增
status-del = 删
status-ren = 移
status-cpy = 拷
status-conflict = 冲
status-unknown = ?

# ===== 详情面板 =====
tab-details = 详情
tab-branch-health = 分支概览
detail-empty = 选择提交或文件查看详情
detail-author = 作者 { $author }
detail-date = 时间 { $date }
file-modified = 修改
file-added = 新增
file-deleted = 删除
file-renamed = 重命名
file-conflict = 冲突
file-untracked = 未跟踪
file-staged = 已暂存
file-unstaged = 未暂存

# ===== 提交面板 =====
commit-title = 提交
commit-placeholder = 提交信息（Enter 提交）
commit-btn = 提交
commit-hint-staged = 将提交暂存的变更
commit-hint-none = 无暂存变更（暂存功能 M2）

# ===== 底部面板（选中提交文件清单 + 单文件 diff） =====
bottom-no-commit = 未选择提交
bottom-merge-empty = 合并提交无逐文件统计
bottom-no-file = 点击左侧文件查看 diff
bottom-bin = 二进制
diff-all-files = 全部变更文件
diff-no-output = (无输出)
diff-layout-inline = 内联
diff-layout-side-by-side = 并排

# ===== 设置弹层 =====
settings-title = 设置
language-title = 界面语言
language-system = 跟随系统
language-chinese = 简体中文
language-english = English
settings-close = 关闭
theme-title = 主题
theme-github-dark = GitHub Dark
theme-catppuccin-latte = Latte
theme-catppuccin-frappe = Frappé
theme-catppuccin-macchiato = Macchiato
theme-catppuccin-mocha = Mocha

# ===== 错误（core 产生 key，展示侧拼接本地化） =====
err-path-not-exist = 路径不存在: { $detail }
err-not-a-repo = 不是 Git 仓库: { $detail }
err-git-run = git 执行失败: { $detail }
err-git-log = git log 失败: { $detail }
err-numstat = git show --numstat 失败: { $detail }
err-file-diff = 读取文件 diff 失败: { $detail }
