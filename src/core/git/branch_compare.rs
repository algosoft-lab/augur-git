//! Read-only comparison of two local or remote-tracking branches.

use std::collections::HashSet;
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc::Sender};
use std::thread;

use crate::core::diff::{
    FileChange, merge_numstat, parse_numstat_z, parse_raw_records,
};

use super::{
    BranchRefInfo, BranchRefKind, GitEvent, MAX_BLOB_SIZE, git_command,
    read_blob_spec,
};

/// Selects the two Git snapshots used by a branch comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchCompareMode {
    /// Compare the selected branch tips directly.
    Direct,
    /// Compare the target tip with the merge base of the two branches.
    MergeBase,
}

impl BranchCompareMode {
    pub fn is_merge_base(self) -> bool {
        matches!(self, Self::MergeBase)
    }
}

/// Build the structured ref query used by the branch comparison selector.
pub(super) fn branch_ref_args(repo_path: &str) -> Vec<String> {
    vec![
        "-C".to_string(),
        repo_path.to_string(),
        "for-each-ref".to_string(),
        "--format=%(refname)%00%(refname:short)%00%(symref)".to_string(),
        "refs/heads".to_string(),
        "refs/remotes".to_string(),
    ]
}

/// Parse `for-each-ref` records into safe, fully-qualified branch refs.
pub(super) fn parse_branch_refs(text: &str) -> Vec<BranchRefInfo> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    for record in text.lines() {
        let fields: Vec<&str> =
            record.trim_end_matches('\r').split('\0').collect();
        let Some(full_name) = fields.first().copied() else {
            continue;
        };
        let Some(short_name) = fields.get(1).copied() else {
            continue;
        };
        let symref = fields.get(2).copied().unwrap_or_default();
        if !symref.is_empty()
            || short_name.is_empty()
            || (!full_name.starts_with("refs/heads/")
                && !full_name.starts_with("refs/remotes/"))
            || !seen.insert(full_name.to_string())
        {
            continue;
        }
        let kind = if full_name.starts_with("refs/heads/") {
            BranchRefKind::Local
        } else {
            BranchRefKind::Remote
        };
        refs.push(BranchRefInfo {
            name: short_name.to_string(),
            full_name: full_name.to_string(),
            kind,
        });
    }
    refs.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.name.cmp(&right.name))
    });
    refs
}

/// Start a comparison on a dedicated read-only worker.
pub(super) fn spawn_comparison(
    repo_path: String,
    request_id: u64,
    base: BranchRefInfo,
    target: BranchRefInfo,
    mode: BranchCompareMode,
    event_tx: Sender<GitEvent>,
    generation: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        run_comparison(
            &repo_path,
            request_id,
            &base,
            &target,
            mode,
            &event_tx,
            &generation,
        );
    });
}

/// Build the metadata command comparing two resolved commit snapshots.
pub(super) fn raw_diff_args(
    repo_path: &str,
    old_oid: &str,
    new_oid: &str,
) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "diff".to_string(),
        "--raw".to_string(),
        "-z".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
        old_oid.to_string(),
        new_oid.to_string(),
    ]
}

/// Build the numstat command comparing two resolved commit snapshots.
pub(super) fn numstat_args(
    repo_path: &str,
    old_oid: &str,
    new_oid: &str,
) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "diff".to_string(),
        "--numstat".to_string(),
        "-z".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
        old_oid.to_string(),
        new_oid.to_string(),
    ]
}

/// Build a safe single-file patch query. Paths remain after `--` and are
/// passed as independent literal arguments.
pub(super) fn file_diff_args(
    repo_path: &str,
    old_oid: &str,
    new_oid: &str,
    file: &FileChange,
) -> Vec<String> {
    let mut args = vec![
        "--literal-pathspecs".to_string(),
        "--no-pager".to_string(),
        "-C".to_string(),
        repo_path.to_string(),
        "diff".to_string(),
        "--no-color".to_string(),
        "--no-ext-diff".to_string(),
        "--find-renames".to_string(),
        old_oid.to_string(),
        new_oid.to_string(),
        "--".to_string(),
    ];
    if file.status.is_rename() {
        if let Some(old_path) = file.old_path.as_deref() {
            args.push(old_path.to_string());
        }
    }
    args.push(file.new_path.clone());
    args
}

fn run_comparison(
    repo_path: &str,
    request_id: u64,
    base: &BranchRefInfo,
    target: &BranchRefInfo,
    mode: BranchCompareMode,
    event_tx: &Sender<GitEvent>,
    generation: &AtomicU64,
) {
    if !is_current(generation, request_id) {
        return;
    }
    let base_oid = match resolve_commit(repo_path, &base.full_name) {
        Ok(oid) => oid,
        Err(detail) => {
            send_error(event_tx, request_id, None, detail);
            send_finished(event_tx, request_id);
            return;
        }
    };
    let target_oid = match resolve_commit(repo_path, &target.full_name) {
        Ok(oid) => oid,
        Err(detail) => {
            send_error(event_tx, request_id, None, detail);
            send_finished(event_tx, request_id);
            return;
        }
    };
    let old_oid = if mode.is_merge_base() {
        match resolve_merge_base(repo_path, &base_oid, &target_oid) {
            Ok(oid) => oid,
            Err(detail) => {
                send_error(event_tx, request_id, None, detail);
                send_finished(event_tx, request_id);
                return;
            }
        }
    } else {
        base_oid
    };

    if !is_current(generation, request_id) {
        return;
    }
    let raw = git_command()
        .args(raw_diff_args(repo_path, &old_oid, &target_oid))
        .output();
    let stats = git_command()
        .args(numstat_args(repo_path, &old_oid, &target_oid))
        .output();
    let files = match (raw, stats) {
        (Ok(raw), Ok(stats))
            if raw.status.success() && stats.status.success() =>
        {
            let raw_files = parse_raw_records(&raw.stdout);
            let stat_files = parse_numstat_z(&stats.stdout);
            if raw_files.is_empty() {
                stat_files
            } else {
                merge_numstat(raw_files, stat_files)
            }
        }
        (Ok(raw), Ok(stats)) => {
            let output = if !raw.status.success() { raw } else { stats };
            send_error(
                event_tx,
                request_id,
                None,
                output_detail(&output, "git diff"),
            );
            send_finished(event_tx, request_id);
            return;
        }
        (Err(error), _) | (_, Err(error)) => {
            send_error(event_tx, request_id, None, error.to_string());
            send_finished(event_tx, request_id);
            return;
        }
    };

    log::info!(
        "[git_compare] metadata loaded: request_id={}, mode={mode:?}, files={}",
        request_id,
        files.len()
    );
    let _ = event_tx.send(GitEvent::BranchCompareFiles {
        request_id,
        files: files.clone(),
    });

    for file in files {
        if !is_current(generation, request_id) {
            return;
        }
        let output = git_command()
            .args(file_diff_args(repo_path, &old_oid, &target_oid, &file))
            .output();
        match output {
            Ok(output) if output.status.success() => {
                let patch =
                    String::from_utf8_lossy(&output.stdout).into_owned();
                let (old_source, new_source) = if file.is_binary() {
                    (None, None)
                } else {
                    (
                        file.old_blob
                            .as_deref()
                            .and_then(|oid| read_blob_limited(repo_path, oid)),
                        file.new_blob
                            .as_deref()
                            .and_then(|oid| read_blob_limited(repo_path, oid)),
                    )
                };
                log::debug!(
                    "[git_compare] file loaded: request_id={}, path={}, patch_bytes={}",
                    request_id,
                    file.path,
                    patch.len()
                );
                let _ = event_tx.send(GitEvent::BranchCompareFileDiff {
                    request_id,
                    file,
                    patch,
                    old_source,
                    new_source,
                });
            }
            Ok(output) => {
                send_error(
                    event_tx,
                    request_id,
                    Some(file),
                    output_detail(&output, "git diff"),
                );
            }
            Err(error) => {
                send_error(event_tx, request_id, Some(file), error.to_string());
            }
        }
    }
    send_finished(event_tx, request_id);
}

fn resolve_commit(repo_path: &str, reference: &str) -> Result<String, String> {
    if !is_supported_branch_ref(reference) {
        return Err("Unsupported branch reference".to_string());
    }
    let spec = format!("{reference}^{{commit}}");
    let output = git_command()
        .args([
            "--no-pager",
            "-C",
            repo_path,
            "rev-parse",
            "--verify",
            "--end-of-options",
            &spec,
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(output_detail(&output, "git rev-parse"));
    }
    parse_object_id(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "Git returned an invalid branch commit id".to_string())
}

fn is_supported_branch_ref(reference: &str) -> bool {
    let has_name = reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/"))
        .is_some_and(|name| !name.is_empty());
    has_name && !reference.bytes().any(|byte| byte.is_ascii_control())
}

fn resolve_merge_base(
    repo_path: &str,
    base_oid: &str,
    target_oid: &str,
) -> Result<String, String> {
    let output = git_command()
        .args([
            "--no-pager",
            "-C",
            repo_path,
            "merge-base",
            base_oid,
            target_oid,
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(output_detail(&output, "git merge-base"));
    }
    parse_object_id(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "Git returned no common ancestor".to_string())
}

fn parse_object_id(text: &str) -> Option<String> {
    let value = text.trim();
    (matches!(value.len(), 40 | 64)
        && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then(|| value.to_string())
}

fn read_blob_limited(repo_path: &str, oid: &str) -> Option<String> {
    let output = read_blob_spec(repo_path, oid)?;
    (output.len() <= MAX_BLOB_SIZE).then_some(output)
}

fn output_detail(output: &Output, operation: &str) -> String {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        output
            .status
            .code()
            .map(|code| format!("{operation} exited with status {code}"))
            .unwrap_or_else(|| format!("{operation} terminated unexpectedly"))
    } else {
        detail
    }
}

fn is_current(generation: &AtomicU64, request_id: u64) -> bool {
    generation.load(Ordering::Acquire) == request_id
}

fn send_error(
    event_tx: &Sender<GitEvent>,
    request_id: u64,
    file: Option<FileChange>,
    detail: String,
) {
    let _ = event_tx.send(GitEvent::BranchCompareError {
        request_id,
        file,
        detail,
    });
}

fn send_finished(event_tx: &Sender<GitEvent>, request_id: u64) {
    let _ = event_tx.send(GitEvent::BranchCompareFinished { request_id });
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::core::diff::FileChangeStatus;

    #[test]
    fn branch_ref_parser_filters_symbolic_head_and_sorts_refs() {
        let refs = parse_branch_refs(
            "refs/remotes/origin/HEAD\0origin/HEAD\0refs/remotes/origin/main\nrefs/heads/z\0z\0\nrefs/heads/a\0a\0\nrefs/remotes/origin/main\0origin/main\0\n",
        );
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].name, "a");
        assert_eq!(refs[1].name, "z");
        assert_eq!(refs[2].name, "origin/main");
        assert!(matches!(refs[0].kind, BranchRefKind::Local));
    }

    #[test]
    fn compare_args_keep_oids_and_paths_as_independent_arguments() {
        let file = FileChange {
            path: "old name.txt => new name.txt".into(),
            old_path: Some("old name.txt".into()),
            new_path: "new name.txt".into(),
            status: FileChangeStatus::Renamed,
            old_blob: None,
            new_blob: None,
            added: Some(1),
            deleted: Some(1),
        };
        let args = file_diff_args("repo", "a", "b", &file);
        assert!(
            args.windows(2)
                .any(|pair| { pair[0] == "a" && pair[1] == "b" })
        );
        let separator = args.iter().position(|arg| arg == "--");
        assert_eq!(
            separator.map(|index| &args[index + 1..]),
            Some(&["old name.txt".to_string(), "new name.txt".to_string()][..])
        );
        assert_eq!(
            raw_diff_args("repo", "a", "b").last(),
            Some(&"b".to_string())
        );
        assert_eq!(
            numstat_args("repo", "a", "b").last(),
            Some(&"b".to_string())
        );
    }

    #[test]
    fn object_id_parser_rejects_malformed_output() {
        assert!(parse_object_id("not-an-object\n").is_none());
        assert!(parse_object_id(&"a".repeat(40)).is_some());
        assert!(
            parse_object_id(&format!("{}\nextra", "a".repeat(40))).is_none()
        );
    }

    #[test]
    fn branch_ref_validation_only_allows_local_and_remote_refs() {
        assert!(is_supported_branch_ref("refs/heads/main"));
        assert!(is_supported_branch_ref("refs/remotes/origin/main"));
        assert!(!is_supported_branch_ref("main"));
        assert!(!is_supported_branch_ref("refs/tags/v1"));
        assert!(!is_supported_branch_ref("refs/heads/bad\nref"));
    }

    #[test]
    fn comparison_reads_unchecked_out_branches_without_mutating_worktree() {
        let repo = TestRepo::new();
        repo.write("base.txt", "base\n");
        repo.write("rename-source.txt", "same content\n");
        repo.git(["add", "--all"]);
        repo.git(["commit", "-qm", "base"]);

        repo.git(["branch", "feature-a"]);
        repo.git(["checkout", "feature-a"]);
        repo.git(["mv", "rename-source.txt", "renamed file.txt"]);
        repo.write("a.txt", "from a\n");
        repo.git(["add", "--all"]);
        repo.git(["commit", "-qm", "feature-a"]);

        repo.git(["checkout", "main"]);
        repo.git(["branch", "feature-b"]);
        repo.git(["checkout", "feature-b"]);
        repo.write("new name-中.txt", "from b\n");
        repo.git(["add", "--all"]);
        repo.git(["commit", "-qm", "feature-b"]);
        repo.git(["checkout", "main"]);
        let remote_oid = repo.git(["rev-parse", "feature-b"]);
        repo.git([
            "update-ref",
            "refs/remotes/origin/feature-b",
            remote_oid.as_str(),
        ]);

        let head_before = repo.git(["rev-parse", "--abbrev-ref", "HEAD"]);
        let status_before = repo.git(["status", "--porcelain=v1"]);
        let base = local_ref("feature-a");
        let target = local_ref("feature-b");

        let direct_events = run_events(
            &repo.path,
            11,
            &base,
            &target,
            BranchCompareMode::Direct,
        );
        let (direct_files, direct_diffs) = inspect_events(direct_events, 11);
        let direct_paths = direct_files
            .iter()
            .map(|file| file.new_path.clone())
            .collect::<HashSet<_>>();
        assert_eq!(
            direct_paths,
            HashSet::from([
                "a.txt".to_string(),
                "new name-中.txt".to_string(),
                "rename-source.txt".to_string(),
            ])
        );
        assert!(
            direct_files
                .iter()
                .any(|file| file.status == FileChangeStatus::Renamed)
        );
        let rename = direct_files
            .iter()
            .find(|file| file.status == FileChangeStatus::Renamed)
            .expect("rename metadata");
        assert_eq!(rename.added, Some(0));
        assert_eq!(rename.deleted, Some(0));
        assert!(
            direct_files
                .iter()
                .any(|file| file.status == FileChangeStatus::Deleted)
        );
        assert!(
            direct_files
                .iter()
                .any(|file| file.status == FileChangeStatus::Added)
        );
        assert!(direct_diffs.iter().any(|(file, patch)| {
            file.new_path == "new name-中.txt" && patch.contains("from b")
        }));

        let remote_target = BranchRefInfo {
            name: "origin/feature-b".to_string(),
            full_name: "refs/remotes/origin/feature-b".to_string(),
            kind: BranchRefKind::Remote,
        };
        let remote_events = run_events(
            &repo.path,
            13,
            &base,
            &remote_target,
            BranchCompareMode::Direct,
        );
        let (remote_files, _) = inspect_events(remote_events, 13);
        assert_eq!(
            remote_files
                .iter()
                .map(|file| file.new_path.as_str())
                .collect::<Vec<_>>(),
            direct_files
                .iter()
                .map(|file| file.new_path.as_str())
                .collect::<Vec<_>>()
        );

        let merge_events = run_events(
            &repo.path,
            12,
            &base,
            &target,
            BranchCompareMode::MergeBase,
        );
        let (merge_files, _) = inspect_events(merge_events, 12);
        assert_eq!(
            merge_files
                .iter()
                .map(|file| file.new_path.as_str())
                .collect::<Vec<_>>(),
            vec!["new name-中.txt"]
        );

        assert_eq!(
            repo.git(["rev-parse", "--abbrev-ref", "HEAD"]),
            head_before
        );
        assert_eq!(repo.git(["status", "--porcelain=v1"]), status_before);
    }

    #[test]
    fn cancelled_comparison_does_not_publish_events() {
        let repo = TestRepo::new();
        repo.write("file.txt", "content\n");
        repo.git(["add", "--all"]);
        repo.git(["commit", "-qm", "base"]);
        repo.git(["branch", "feature"]);
        repo.write("file.txt", "changed\n");
        repo.git(["add", "--all"]);
        repo.git(["commit", "-qm", "changed"]);

        let base = local_ref("main");
        let target = local_ref("feature");
        let (tx, rx) = mpsc::channel();
        let generation = AtomicU64::new(0);
        run_comparison(
            &repo.path.to_string_lossy(),
            21,
            &base,
            &target,
            BranchCompareMode::Direct,
            &tx,
            &generation,
        );
        assert!(rx.try_iter().next().is_none());
    }

    #[test]
    fn missing_reference_and_unrelated_histories_are_non_fatal() {
        let repo = TestRepo::new();
        repo.write("file.txt", "content\n");
        repo.git(["add", "--all"]);
        repo.git(["commit", "-qm", "base"]);

        let missing = BranchRefInfo {
            name: "missing".to_string(),
            full_name: "refs/heads/missing".to_string(),
            kind: BranchRefKind::Local,
        };
        assert_request_failure(
            run_events(
                &repo.path,
                31,
                &local_ref("main"),
                &missing,
                BranchCompareMode::Direct,
            ),
            31,
        );

        let tree = repo.git(["rev-parse", "main^{tree}"]);
        let unrelated_oid =
            repo.git(["commit-tree", tree.as_str(), "-m", "unrelated"]);
        repo.git([
            "update-ref",
            "refs/heads/unrelated",
            unrelated_oid.as_str(),
        ]);
        assert_request_failure(
            run_events(
                &repo.path,
                32,
                &local_ref("main"),
                &local_ref("unrelated"),
                BranchCompareMode::MergeBase,
            ),
            32,
        );
    }

    fn local_ref(name: &str) -> BranchRefInfo {
        BranchRefInfo {
            name: name.to_string(),
            full_name: format!("refs/heads/{name}"),
            kind: BranchRefKind::Local,
        }
    }

    fn run_events(
        path: &Path,
        request_id: u64,
        base: &BranchRefInfo,
        target: &BranchRefInfo,
        mode: BranchCompareMode,
    ) -> Vec<GitEvent> {
        let (tx, rx) = mpsc::channel();
        let generation = AtomicU64::new(request_id);
        run_comparison(
            &path.to_string_lossy(),
            request_id,
            base,
            target,
            mode,
            &tx,
            &generation,
        );
        drop(tx);
        rx.into_iter().collect()
    }

    fn inspect_events(
        events: Vec<GitEvent>,
        request_id: u64,
    ) -> (Vec<FileChange>, Vec<(FileChange, String)>) {
        let mut files = None;
        let mut diffs = Vec::new();
        let mut finished = false;
        for event in events {
            match event {
                GitEvent::BranchCompareFiles {
                    request_id: event_id,
                    files: value,
                } => {
                    assert_eq!(event_id, request_id);
                    files = Some(value);
                }
                GitEvent::BranchCompareFileDiff {
                    request_id: event_id,
                    file,
                    patch,
                    ..
                } => {
                    assert_eq!(event_id, request_id);
                    diffs.push((file, patch));
                }
                GitEvent::BranchCompareFinished {
                    request_id: event_id,
                } => {
                    assert_eq!(event_id, request_id);
                    finished = true;
                }
                GitEvent::BranchCompareError { detail, .. } => {
                    panic!("unexpected comparison error: {detail}");
                }
                _ => {}
            }
        }
        assert!(finished);
        (files.expect("comparison metadata event"), diffs)
    }

    fn assert_request_failure(events: Vec<GitEvent>, request_id: u64) {
        let mut error = false;
        let mut finished = false;
        for event in events {
            match event {
                GitEvent::BranchCompareError {
                    request_id: event_id,
                    file,
                    detail,
                } => {
                    assert_eq!(event_id, request_id);
                    assert!(file.is_none());
                    assert!(!detail.is_empty());
                    error = true;
                }
                GitEvent::BranchCompareFinished {
                    request_id: event_id,
                } => {
                    assert_eq!(event_id, request_id);
                    finished = true;
                }
                GitEvent::BranchCompareFiles { .. }
                | GitEvent::BranchCompareFileDiff { .. } => {
                    panic!("request failure published file data")
                }
                _ => {}
            }
        }
        assert!(error);
        assert!(finished);
    }

    struct TestRepo {
        path: PathBuf,
    }

    impl TestRepo {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let path = std::env::temp_dir().join(format!(
                "augur-git-branch-compare-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create temporary repository");
            let repo = Self { path };
            repo.git(["init", "-q"]);
            repo.git(["branch", "-M", "main"]);
            repo.git(["config", "user.email", "test@example.com"]);
            repo.git(["config", "user.name", "augur-git test"]);
            repo
        }

        fn write(&self, path: &str, contents: &str) {
            fs::write(self.path.join(path), contents).expect("write test file");
        }

        fn git<const N: usize>(&self, args: [&str; N]) -> String {
            let output = git_command()
                .arg("-C")
                .arg(&self.path)
                .args(args)
                .output()
                .expect("run git test command");
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
