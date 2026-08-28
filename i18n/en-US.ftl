# augur-git English (en-US) translations
# Format: key = value, one per line; { $name } placeholders are
# substituted by src/core/i18n.rs::text_args.

# ===== App / Welcome =====
app-tagline = Desktop Git client
welcome-open = Open
tab-new = New Tab
recent-repos = Recent repositories
repo-folder-prompt = Choose a Git repository folder

# ===== Application menu / About =====
menu-open = Open application menu
menu-file = File
menu-open-repository = Open Repository…
menu-new-tab = New Tab
menu-recent-repositories = Recent Repositories
menu-no-recent-repositories = No Recent Repositories
menu-edit = Edit
menu-settings = Settings
menu-help = Help
menu-about = About augur-git
menu-quit = Quit
about-title = About
about-tagline = Desktop Git client
about-author = Author
about-version = Version
about-commit = Commit

# ===== Tab bar / Status bar =====
no-repo-open = No repository open
status-scanning = Scanning…
status-no-repo-selected = No repository selected
status-scanning-at = Scanning @ { $repo }
status-summary = { $branch } · ↑{ $ahead }↓{ $behind } · staged { $staged }, changed { $unstaged }

# ===== Command results / messages =====
command-success = { $label } succeeded
command-failed = { $label } failed: { $error }
branch-selected = Branch { $name } (details in M2)
context-checkout = Checkout
context-copy-branch = Copy branch name
context-copy-tag = Copy tag name
context-copy-commit = Copy commit hash
context-copy-commit-message = Copy commit message
context-copied = Copied { $name }
context-copied-commit-message = Copied commit message
context-copy-commit-message-failed = Failed to copy commit message: { $error }
push-force-title = Force push?
push-force-warning = Force pushing overwrites the remote history of { $branch }. Commits that exist only on the remote may be lost.
push-force-confirm = Force Push
push-force-cancel = Cancel

# ===== Toolbar =====
toolbar-fetch = Fetch
toolbar-pull-merge = Pull (Merge)
toolbar-pull-rebase = Pull (Rebase)
toolbar-push = Push
toolbar-push-force = Push (Force)
toolbar-branch = Branch
toolbar-refresh = Refresh
toolbar-settings = Settings
toolbar-busy = Working…

# ===== Commit graph =====
graph-empty = No commits
col-graph = Graph
col-hash = Hash
col-message = Message
col-author = Author
col-date = Date
rel-now = just now
rel-min = { $n }min ago
rel-hour = { $n }h ago
rel-day = { $n }d ago
rel-week = { $n }w ago
rel-month = { $n }m ago
rel-year = { $n }y ago

# ===== Sidebar =====
sidebar-repo = Repository
section-branches = Branches
section-remotes = Remotes
section-remote-branches = Remote branches
section-tags = Tags
section-stashes = Stashes
section-staged = Staged
section-changes = Changes
status-mod = M
status-add = A
status-del = D
status-ren = R
status-cpy = C
status-conflict = U
status-unknown = ?

# ===== Detail panel =====
tab-details = Details
tab-branch-health = Branch Health
detail-empty = Select a commit or file to see details
detail-author = Author { $author }
detail-date = Date { $date }
detail-coauthors = Co-authored-by
file-modified = Modified
file-added = Added
file-deleted = Deleted
file-renamed = Renamed
file-conflict = Conflict
file-untracked = Untracked
file-staged = Staged
file-unstaged = Unstaged

# ===== Commit panel =====
commit-title = Commit
commit-placeholder = Commit Message
commit-btn = Commit
commit-amend-btn = Amend
commit-action-commit = Commit
commit-action-amend = Amend last commit

# ===== Bottom panel (selected commit file list + file diff) =====
bottom-no-commit = No commit selected
bottom-merge-empty = Merge commit has no changes relative to its first parent
bottom-no-changes = This commit has no file changes
bottom-no-file = Select a file on the left to view its diff
bottom-bin = Binary
diff-all-files = All changed files
diff-merge-first-parent = vs first parent
diff-no-output = (no output)

# ===== Settings overlay =====
settings-title = Settings
language-title = Language
language-system = System
language-chinese = 简体中文
language-english = English
settings-close = Close
theme-title = Theme
diff-layout-title = Diff layout
diff-layout-inline = Inline
diff-layout-side-by-side = Side-by-side
theme-github-dark = GitHub Dark
theme-catppuccin-latte = Latte
theme-catppuccin-frappe = Frappé
theme-catppuccin-macchiato = Macchiato
theme-catppuccin-mocha = Mocha

# ===== Errors (keys produced by core, localized at display side) =====
err-path-not-exist = Path not found: { $detail }
err-not-a-repo = Not a Git repository: { $detail }
err-git-run = Failed to run git: { $detail }
err-git-log = git log failed: { $detail }
err-numstat = git show --numstat failed: { $detail }
err-file-diff = Failed to read file diff: { $detail }
err-commit-message = Failed to read commit message: { $detail }
