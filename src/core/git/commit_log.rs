//! Paged commit-graph log queries executed by the Git worker.
//!
//! The commit graph shows branch and remote-branch divergence like VS Code:
//! the worker fetches commits page by page under an explicit scope instead of
//! filtering one global `--all --max-count=N` window on the UI side. This
//! keeps tracked-upstream divergence and remote branch lanes visible even in
//! repositories where recent commits are dominated by unrelated branches.

use std::sync::mpsc::Sender;

use crate::core::graph::LogRow;

use super::{GitError, GitEvent, git_command};

/// Commits fetched per graph page.
pub(super) const LOG_PAGE_SIZE: usize = 500;

/// History scope used when querying commits for the commit graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogScope {
    /// All local branches, remote branches, and tags.
    AllBranches,
    /// The checked-out history and the tracked upstream, when available.
    CurrentBranch {
        /// Tracked upstream rev such as `origin/main`.
        upstream: Option<String>,
    },
}

/// Worker-side pagination state for one repository's commit log.
#[derive(Debug)]
pub(super) struct LogState {
    scope: LogScope,
    skip: usize,
    has_more: bool,
}

impl Default for LogState {
    fn default() -> Self {
        Self {
            scope: LogScope::AllBranches,
            skip: 0,
            has_more: false,
        }
    }
}

/// Build the `git log` arguments for one graph page.
pub(super) fn log_args(
    repo_path: &str,
    scope: &LogScope,
    skip: usize,
) -> Vec<String> {
    let mut args = vec![
        "--no-pager".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "log".to_string(),
    ];
    match scope {
        LogScope::AllBranches => {
            // Explicit ref groups instead of `--all`: the graph shows branch,
            // remote, tag, and HEAD history only (no stash/notes refs).
            args.push("--branches".to_string());
            args.push("--remotes".to_string());
            args.push("--tags".to_string());
            args.push("HEAD".to_string());
        }
        LogScope::CurrentBranch { upstream } => {
            args.push("HEAD".to_string());
            if let Some(upstream) = upstream {
                args.push(upstream.clone());
            }
        }
    }
    args.push("--topo-order".to_string());
    if skip > 0 {
        args.push("--skip".to_string());
        args.push(skip.to_string());
    }
    args.push(format!("--max-count={LOG_PAGE_SIZE}"));
    args.push("--date=format:%Y-%m-%d %H:%M".to_string());
    args.push("-z".to_string());
    args.push(
        "--pretty=format:%H%x00%h%x00%an%x00%ai%x00%at%x00%s%x00%D%x00%P%x00%B"
            .to_string(),
    );
    args
}

/// Resolve the upstream rev once per scope change so a stale tracked branch
/// degrades to a HEAD-only query instead of failing the whole page.
fn resolve_scope_upstream(repo_path: &str, scope: &LogScope) -> LogScope {
    let LogScope::CurrentBranch {
        upstream: Some(upstream),
    } = scope
    else {
        return scope.clone();
    };
    if is_rev_resolvable(repo_path, &format!("{upstream}^{{commit}}")) {
        return scope.clone();
    }
    log::warn!(
        "[git_log] unresolvable upstream {upstream}, querying HEAD only"
    );
    LogScope::CurrentBranch { upstream: None }
}

fn is_rev_resolvable(repo_path: &str, rev: &str) -> bool {
    git_command()
        .args(["-C", repo_path, "rev-parse", "--verify", "--quiet", rev])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Whether `git log` failed only because the repository has no commits yet
/// (or HEAD cannot be resolved), which is an empty result, not an error.
fn is_unborn_head_output(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr);
    text.contains("does not have any commits yet")
        || text.contains("bad revision 'HEAD'")
        || text.contains("ambiguous argument 'HEAD'")
}

/// Fetch one page and emit a replace or append event.
pub(super) fn run_page(
    repo_path: &str,
    state: &mut LogState,
    replace: bool,
    event_tx: &Sender<GitEvent>,
) {
    if replace {
        state.skip = 0;
    }
    let output = git_command()
        .args(log_args(repo_path, &state.scope, state.skip))
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let rows = parse_log(&text);
            state.has_more = rows.len() >= LOG_PAGE_SIZE;
            state.skip += rows.len();
            log::debug!(
                "[git_log] page fetched: rows={}, skip={}, replace={}, has_more={}",
                rows.len(),
                state.skip,
                replace,
                state.has_more
            );
            let _ = event_tx.send(GitEvent::LogPage {
                rows,
                replace,
                has_more: state.has_more,
            });
        }
        Ok(output) if is_unborn_head_output(&output.stderr) => {
            log::debug!("[git_log] repository has no commits yet");
            state.skip = 0;
            state.has_more = false;
            let _ = event_tx.send(GitEvent::LogPage {
                rows: Vec::new(),
                replace: true,
                has_more: false,
            });
        }
        Ok(output) => {
            let msg = String::from_utf8_lossy(&output.stderr);
            let _ = event_tx
                .send(GitEvent::Error(GitError::new("err-git-log", msg)));
        }
        Err(e) => {
            let _ = event_tx.send(GitEvent::Error(GitError::new(
                "err-git-run",
                e.to_string(),
            )));
        }
    }
}

/// Install a new scope and reload the first page.
pub(super) fn set_scope(
    repo_path: &str,
    state: &mut LogState,
    scope: LogScope,
    event_tx: &Sender<GitEvent>,
) {
    state.scope = resolve_scope_upstream(repo_path, &scope);
    run_page(repo_path, state, true, event_tx);
}

/// Fetch the next page if the current query reported more commits.
pub(super) fn request_more(
    repo_path: &str,
    state: &mut LogState,
    event_tx: &Sender<GitEvent>,
) {
    if !state.has_more {
        return;
    }
    run_page(repo_path, state, false, event_tx);
}

/// Parse structured `git log --pretty=format:...` output.
///
/// Each line starts with a 40-character hexadecimal object id followed by
/// NUL-separated commit fields. Malformed records are ignored defensively.
pub(super) fn parse_log(text: &str) -> Vec<LogRow> {
    const FIELD_COUNT: usize = 9;
    let fields = text.split('\0').collect::<Vec<_>>();
    let mut rows = Vec::new();
    for record in fields.chunks(FIELD_COUNT) {
        if record.len() < FIELD_COUNT {
            continue;
        }
        let oid = record[0];
        if oid.len() != 40 || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let parents =
            record[7].split_whitespace().map(str::to_string).collect();
        // `%at` is the author timestamp used for relative-time display.
        let timestamp = record[4].parse().unwrap_or(0);
        rows.push(LogRow {
            oid: oid.to_string(),
            short: record[1].to_string(),
            author: record[2].to_string(),
            date: record[3].to_string(),
            timestamp,
            subject: record[5].to_string(),
            message: record[8].to_string(),
            decorations: record[6].to_string(),
            parents,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_branches_scope_uses_explicit_ref_groups() {
        let args = log_args("repo with spaces", &LogScope::AllBranches, 0);

        assert!(args.windows(2).any(|pair| {
            pair == ["-C".to_string(), "repo with spaces".to_string()]
        }));
        assert!(args.iter().any(|arg| arg == "--branches"));
        assert!(args.iter().any(|arg| arg == "--remotes"));
        assert!(args.iter().any(|arg| arg == "--tags"));
        assert!(!args.iter().any(|arg| arg == "--all"));
        // HEAD is included so a detached checkout stays visible, like `--all`.
        assert!(args.iter().any(|arg| arg == "HEAD"));
        assert!(args.iter().any(|arg| arg == "--topo-order"));
        assert!(!args.iter().any(|arg| arg == "--graph"));
        assert!(!args.iter().any(|arg| arg == "--skip"));
        assert!(
            args.iter()
                .any(|arg| arg == &format!("--max-count={LOG_PAGE_SIZE}"))
        );
    }

    #[test]
    fn current_branch_scope_lists_head_and_upstream_as_revs() {
        let scope = LogScope::CurrentBranch {
            upstream: Some("origin/main".to_string()),
        };
        let args = log_args("repo", &scope, 250);
        let head_index = args.iter().position(|arg| arg == "log").unwrap();
        let revs = &args[head_index + 1..];
        assert!(revs.contains(&"HEAD".to_string()));
        assert!(revs.contains(&"origin/main".to_string()));
        assert!(!revs.iter().any(|arg| arg == "--branches"));
        let skip_position =
            args.iter().position(|arg| arg == "--skip").unwrap();
        assert_eq!(args[skip_position + 1], "250");
    }

    #[test]
    fn current_branch_scope_without_upstream_uses_head_only() {
        let scope = LogScope::CurrentBranch { upstream: None };
        let args = log_args("repo", &scope, 0);
        let head_index = args.iter().position(|arg| arg == "log").unwrap();
        assert_eq!(args[head_index + 1], "HEAD");
    }

    #[test]
    fn unresolvable_upstream_degrades_to_head_only() {
        // A nonexistent rev string cannot resolve in any repository.
        let scope = LogScope::CurrentBranch {
            upstream: Some("origin/definitely-missing-ref".to_string()),
        };
        let resolved = resolve_scope_upstream("/nonexistent-repo", &scope);
        assert_eq!(resolved, LogScope::CurrentBranch { upstream: None });
    }

    #[test]
    fn unborn_repo_output_is_treated_as_empty_history() {
        assert!(is_unborn_head_output(
            b"fatal: your current branch 'main' does not have any commits yet"
        ));
        assert!(is_unborn_head_output(b"fatal: bad revision 'HEAD'"));
        assert!(!is_unborn_head_output(
            b"fatal: unrecognized argument: --nonsense"
        ));
    }

    #[test]
    fn parse_log_plain() {
        let text = "0123456789abcdef0123456789abcdef01234567\0short\0Lionel Fung\02026-08-13 20:00\01756123456\0M0 框架\0HEAD -> main\0\0M0 框架\n\0";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].oid, "0123456789abcdef0123456789abcdef01234567");
        assert_eq!(rows[0].subject, "M0 框架");
        assert_eq!(rows[0].decorations, "HEAD -> main");
        assert_eq!(rows[0].timestamp, 17_561_234_56);
        assert_eq!(rows[0].message, "M0 框架\n");
        assert!(rows[0].parents.is_empty());
    }

    #[test]
    fn parse_log_with_parents() {
        let text = "0123456789abcdef0123456789abcdef01234567\0s\0a\0d\01756123456\0merge 分支\0HEAD -> main\089abcdef0123456789abcdef0123456789abcdef ffff0123456789abcdef0123456789abcdef01\0merge 分支\n\0";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].subject, "merge 分支");
        assert_eq!(rows[0].parents.len(), 2);
        assert_eq!(
            rows[0].parents[0],
            "89abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn parse_log_skips_bad_lines() {
        let text = "garbage\0s\0a\0d\00\0bad\0\0\0body\0\
0123456789abcdef0123456789abcdef01234567\0s\0a\0d\00\0提交\0\0\0提交\0";
        let rows = parse_log(text);
        assert_eq!(rows.len(), 1);
    }
}
