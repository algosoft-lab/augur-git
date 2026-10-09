//! Generate provider-neutral prompts from a fresh, read-only Git snapshot.

use std::path::{Path, PathBuf};
use std::process::Command;

use augur_core::config::LocationConfig;
use augur_core::git::agent_prompt::{
    ActiveOperation, AgentPromptRequest, ConflictOrigin, PromptContext, build_prompt,
};
use augur_core::git::operation_probe;
use augur_core::git::{GitRepo, RepoLocation};
use tauri::State;

use super::repo::CommandError;
use crate::state::AppState;

#[tauri::command]
pub async fn generate_agent_prompt(
    state: State<'_, AppState>,
    repo_id: u64,
    request: AgentPromptRequest,
) -> std::result::Result<String, CommandError> {
    let (repo, open_path, location) = state
        .with_repo(repo_id, |session| {
            (
                session.repo().clone(),
                session.path().to_string(),
                session.location().clone(),
            )
        })
        .ok_or_else(|| CommandError::missing_repo(repo_id))?;

    tauri::async_runtime::spawn_blocking(move || generate(&repo, &open_path, &location, &request))
        .await
        .map_err(|error| CommandError::new("err-worker", error.to_string()))?
        .map_err(|detail| CommandError::new("err-agent-prompt", detail))
}

fn generate(
    repo: &GitRepo,
    open_path: &str,
    location: &LocationConfig,
    request: &AgentPromptRequest,
) -> Result<String, String> {
    let mut context = collect_context(repo, open_path, location)?;

    match request {
        AgentPromptRequest::Merge { source, .. } | AgentPromptRequest::Rebase { source } => {
            context.target_oid = Some(
                operation_probe::resolve_branch_oid(repo, source)
                    .map_err(|error| format!("could not resolve target branch: {error}"))?,
            );
        }
        AgentPromptRequest::Pull { .. } => {
            let upstream = git_output(
                repo,
                [
                    "rev-parse",
                    "--abbrev-ref",
                    "--symbolic-full-name",
                    "@{upstream}",
                ],
            )?;
            let upstream_oid = git_output(repo, ["rev-parse", "--verify", "@{upstream}^{commit}"])?;
            context.upstream = Some(upstream);
            context.upstream_oid = Some(upstream_oid);
        }
        AgentPromptRequest::ApplyPatch { path, failure } => {
            context.target_oid = None;
            let patch_path = patch_path_for_location(path, repo)?;
            return build_prompt(
                &AgentPromptRequest::ApplyPatch {
                    path: patch_path,
                    failure: failure.clone(),
                },
                &context,
            )
            .map_err(|error| error.to_string());
        }
        AgentPromptRequest::Commit { .. } | AgentPromptRequest::ResolveConflicts { .. } => {}
    }

    if let AgentPromptRequest::ResolveConflicts {
        origin: Some(ConflictOrigin::StashPop),
    } = request
    {
        if context.operation.is_none() {
            context.operation = Some(ActiveOperation::StashPop);
        }
    }

    build_prompt(request, &context).map_err(|error| error.to_string())
}

fn collect_context(
    repo: &GitRepo,
    open_path: &str,
    location: &LocationConfig,
) -> Result<PromptContext, String> {
    let (branch, upstream, files, _, _) =
        augur_core::git::read_working_tree_status(repo).map_err(|error| error.detail)?;
    let merge = operation_probe::probe_merge_state(repo)?;
    let rebase = operation_probe::probe_rebase_state(repo)?;
    let operation = detect_operation(repo, &merge, &rebase)?;
    let root = repository_root(repo, open_path)?;
    let conflicted_paths = files
        .iter()
        .filter(|file| file.is_conflicted())
        .map(|file| file.path.clone())
        .collect();
    let (location_name, distro) = match location {
        LocationConfig::Local => ("local".to_string(), None),
        LocationConfig::Wsl { distro } => ("wsl".to_string(), Some(distro.clone())),
    };

    Ok(PromptContext {
        repository_path: root,
        location: location_name,
        distro,
        branch,
        head: merge.head,
        upstream,
        upstream_oid: None,
        target_oid: None,
        files,
        conflicted_paths,
        operation,
    })
}

fn detect_operation(
    repo: &GitRepo,
    merge: &operation_probe::MergeState,
    rebase: &operation_probe::RebaseState,
) -> Result<Option<ActiveOperation>, String> {
    if let Some(target_oid) = &merge.merge_head {
        return Ok(Some(ActiveOperation::Merge {
            target_oid: target_oid.clone(),
        }));
    }

    if rebase.rebase_in_progress {
        return Ok(Some(ActiveOperation::Rebase {
            replayed_oid: rebase.rebase_head.clone(),
            onto_oid: read_git_path_file(repo, "rebase-merge/onto")?
                .or(read_git_path_file(repo, "rebase-apply/onto")?),
            original_branch: read_git_path_file(repo, "rebase-merge/head-name")?
                .or(read_git_path_file(repo, "rebase-apply/head-name")?),
        }));
    }

    if let Some(commit_oid) = read_ref(repo, "CHERRY_PICK_HEAD")? {
        return Ok(Some(ActiveOperation::CherryPick { commit_oid }));
    }
    if let Some(commit_oid) = read_ref(repo, "REVERT_HEAD")? {
        return Ok(Some(ActiveOperation::Revert { commit_oid }));
    }

    if operation_probe::has_other_git_operation(repo)? {
        return Ok(Some(ActiveOperation::Unknown));
    }
    Ok(None)
}

fn repository_root(repo: &GitRepo, open_path: &str) -> Result<String, String> {
    let root = git_output(repo, ["rev-parse", "--show-toplevel"])?;
    match repo.location() {
        RepoLocation::Local => {
            let root = std::fs::canonicalize(&root).map_err(|error| {
                format!("could not resolve the absolute repository path from {open_path}: {error}")
            })?;
            Ok(display_path(&root))
        }
        RepoLocation::Wsl { .. } if root.starts_with('/') => Ok(root),
        RepoLocation::Wsl { .. } => {
            Err("Git did not return an absolute WSL repository path".into())
        }
    }
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return format!(r"\{rest}");
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
    }
    value
}

fn patch_path_for_location(path: &str, repo: &GitRepo) -> Result<String, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("patch path must be absolute".into());
    }

    match repo.location() {
        RepoLocation::Local => std::fs::canonicalize(&path)
            .map(|canonical| display_path(&canonical))
            .map_err(|error| format!("patch file is unavailable: {error}")),
        RepoLocation::Wsl { distro } => {
            let value = path.to_string_lossy().into_owned();
            if let Some((path_distro, linux_path)) = augur_core::git::parse_unc_path(&value) {
                if path_distro == *distro {
                    return Ok(linux_path);
                }
            }
            if let Some(linux_path) = windows_path_to_wsl(&value) {
                return Ok(linux_path);
            }
            Ok(value)
        }
    }
}

fn windows_path_to_wsl(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let rest = path[2..].replace('\\', "/");
    Some(format!("/mnt/{drive}{rest}"))
}

fn read_ref(repo: &GitRepo, reference: &str) -> Result<Option<String>, String> {
    let mut command = git_command(repo);
    let output = command
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(reference)
        .output()
        .map_err(|error| format!("failed to inspect {reference}: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(nonempty_stdout(&output.stdout))
}

fn read_git_path_file(repo: &GitRepo, name: &str) -> Result<Option<String>, String> {
    let path = git_output(repo, ["rev-parse", "--git-path", name])?;
    let bytes = match repo.location() {
        // `cat` is not a Windows program; local repositories read through the
        // filesystem directly, mirroring `operation_probe::git_path_exists`.
        RepoLocation::Local => {
            let path = PathBuf::from(&path);
            let path = if path.is_absolute() {
                path
            } else {
                Path::new(repo.path()).join(path)
            };
            std::fs::read(path).ok()
        }
        RepoLocation::Wsl { .. } => {
            let output = repo
                .command_in_location("cat")
                .arg("--")
                .arg(&path)
                .output()
                .map_err(|error| format!("failed to read Git operation metadata: {error}"))?;
            if !output.status.success() {
                return Ok(None);
            }
            Some(output.stdout)
        }
    };
    Ok(bytes.as_deref().and_then(nonempty_stdout))
}

fn git_output<const N: usize>(repo: &GitRepo, args: [&str; N]) -> Result<String, String> {
    let output = git_command(repo)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("git exited with {}", output.status)
        } else {
            detail
        });
    }
    nonempty_stdout(&output.stdout).ok_or_else(|| "git returned empty output".into())
}

fn git_command(repo: &GitRepo) -> Command {
    let mut command = repo.command();
    command.arg("-C").arg(repo.path());
    command
}

fn nonempty_stdout(bytes: &[u8]) -> Option<String> {
    let value = String::from_utf8_lossy(bytes).trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Output};

    #[test]
    fn maps_windows_patch_path_to_wsl_mount() {
        assert_eq!(
            windows_path_to_wsl(r"C:\Users\dev\patches\fix.diff").as_deref(),
            Some("/mnt/c/Users/dev/patches/fix.diff")
        );
    }

    #[test]
    fn leaves_non_drive_paths_unmapped() {
        assert_eq!(windows_path_to_wsl(r"\\server\share\fix.diff"), None);
    }

    #[test]
    fn detects_merge_cherry_pick_and_revert_markers() {
        let Some(repo) = TestRepo::new() else {
            return;
        };
        let head = repo.git(&["rev-parse", "HEAD"]).expect("HEAD exists");
        let git_dir = repo.path.join(".git");

        fs::write(git_dir.join("MERGE_HEAD"), &head).expect("merge marker is written");
        let merge = operation_probe::probe_merge_state(&repo.repo).expect("merge state reads");
        let rebase = operation_probe::probe_rebase_state(&repo.repo).expect("rebase state reads");
        assert_eq!(
            detect_operation(&repo.repo, &merge, &rebase).expect("operation is detected"),
            Some(ActiveOperation::Merge {
                target_oid: head.clone()
            })
        );
        fs::remove_file(git_dir.join("MERGE_HEAD")).expect("merge marker is removed");

        for (marker, expected) in [
            (
                "CHERRY_PICK_HEAD",
                ActiveOperation::CherryPick {
                    commit_oid: head.clone(),
                },
            ),
            (
                "REVERT_HEAD",
                ActiveOperation::Revert {
                    commit_oid: head.clone(),
                },
            ),
        ] {
            fs::write(git_dir.join(marker), &head).expect("operation marker is written");
            let merge = operation_probe::probe_merge_state(&repo.repo).expect("merge state reads");
            let rebase =
                operation_probe::probe_rebase_state(&repo.repo).expect("rebase state reads");
            assert_eq!(
                detect_operation(&repo.repo, &merge, &rebase).expect("operation is detected"),
                Some(expected)
            );
            fs::remove_file(git_dir.join(marker)).expect("operation marker is removed");
        }
    }

    #[test]
    fn rebase_requires_a_state_directory_and_uses_its_metadata() {
        let Some(repo) = TestRepo::new() else {
            return;
        };
        let head = repo.git(&["rev-parse", "HEAD"]).expect("HEAD exists");
        let git_dir = repo.path.join(".git");

        fs::write(git_dir.join("REBASE_HEAD"), &head).expect("stale replay marker is written");
        let merge = operation_probe::probe_merge_state(&repo.repo).expect("merge state reads");
        let rebase = operation_probe::probe_rebase_state(&repo.repo).expect("rebase state reads");
        assert!(!rebase.rebase_in_progress);
        assert_eq!(
            detect_operation(&repo.repo, &merge, &rebase).expect("operation is detected"),
            None
        );

        let state_dir = git_dir.join("rebase-merge");
        fs::create_dir_all(&state_dir).expect("rebase state directory is created");
        fs::write(state_dir.join("onto"), &head).expect("rebase target is written");
        fs::write(state_dir.join("head-name"), "refs/heads/main\n")
            .expect("original branch is written");
        let merge = operation_probe::probe_merge_state(&repo.repo).expect("merge state reads");
        let rebase = operation_probe::probe_rebase_state(&repo.repo).expect("rebase state reads");
        assert_eq!(
            detect_operation(&repo.repo, &merge, &rebase).expect("operation is detected"),
            Some(ActiveOperation::Rebase {
                replayed_oid: Some(head.clone()),
                onto_oid: Some(head),
                original_branch: Some("refs/heads/main".into()),
            })
        );
    }

    struct TestRepo {
        path: PathBuf,
        repo: GitRepo,
    }

    impl TestRepo {
        fn new() -> Option<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "augur-agent-prompt-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).ok()?;
            let repo = GitRepo::local(path.to_string_lossy().into_owned());
            let sandbox = Self { path, repo };
            if !sandbox
                .git_output(&["init", "--quiet"])
                .is_some_and(|out| out.status.success())
                || !sandbox
                    .git_output(&["config", "user.email", "agent-prompt@example.com"])
                    .is_some_and(|out| out.status.success())
                || !sandbox
                    .git_output(&["config", "user.name", "Agent Prompt Test"])
                    .is_some_and(|out| out.status.success())
            {
                return None;
            }
            fs::write(sandbox.path.join("tracked.txt"), "initial\n").ok()?;
            if !sandbox
                .git_output(&["add", "tracked.txt"])
                .is_some_and(|out| out.status.success())
                || !sandbox
                    .git_output(&["-c", "commit.gpgsign=false", "commit", "-m", "Initial"])
                    .is_some_and(|out| out.status.success())
            {
                return None;
            }
            Some(sandbox)
        }

        fn git(&self, args: &[&str]) -> Option<String> {
            let output = self.git_output(args)?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        }

        fn git_output(&self, args: &[&str]) -> Option<Output> {
            Command::new("git")
                .arg("-C")
                .arg(&self.path)
                .args(args)
                .output()
                .ok()
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
