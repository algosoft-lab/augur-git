//! Pure parsing, formatting, classification, and URL-rewriting helpers for
//! per-repository `.git/config` credential configuration.

use std::path::PathBuf;

use super::{CredentialHelperOrigin, GitRepo, RemoteUsernameAction, RepoCredentialMode};

/// Parse NUL-delimited `--show-origin [--show-scope] --null --get-regexp` output.
pub fn parse_origin_helpers(text: &str, has_scope: bool) -> Vec<CredentialHelperOrigin> {
    let mut fields: Vec<&str> = text.split('\0').collect();
    if fields.last() == Some(&"") {
        fields.pop();
    }
    let chunk = if has_scope { 3 } else { 2 };
    let mut helpers = Vec::new();
    for entry in fields.chunks_exact(chunk) {
        let (scope, origin, kv) = if has_scope {
            (entry[0], entry[1], entry[2])
        } else {
            ("", entry[0], entry[1])
        };
        let (key, value) = kv.split_once('\n').unwrap_or((kv, ""));
        helpers.push(CredentialHelperOrigin {
            scope: scope.to_string(),
            origin: origin.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    helpers
}

/// Extract the file path from a `store --file=...` helper definition.
pub fn parse_store_helper_path(helper: &str) -> Option<String> {
    let trimmed = helper.trim();
    let rest = trimmed.strip_prefix("store")?.trim_start();
    let arg = if let Some(after_eq) = rest.strip_prefix("--file=") {
        after_eq.trim()
    } else if let Some(after_flag) = rest.strip_prefix("--file") {
        if !after_flag.starts_with(char::is_whitespace) {
            return None;
        }
        after_flag.trim()
    } else {
        return None;
    };
    if arg.is_empty() {
        return None;
    }
    if let Some(inner) = arg.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Some(inner.replace("'\\''", "'"));
    }
    if let Some(inner) = arg.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Some(inner.replace("\\\"", "\""));
    }
    Some(arg.to_string())
}

/// Format a `store --file=...` helper value safely for Git's shell invocation.
pub fn format_store_helper_value(path: &str) -> String {
    let trimmed = path.trim();
    let safe_unquoted = !trimmed.is_empty()
        && trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._/-:".contains(&byte));
    if safe_unquoted {
        format!("store --file={trimmed}")
    } else {
        format!("store --file='{}'", trimmed.replace('\'', "'\\''"))
    }
}

/// Derive the repository's credential mode from local `credential.*` keys.
pub fn classify_mode(
    use_http_path: Option<bool>,
    username: Option<&str>,
    local_helpers: &[String],
) -> (RepoCredentialMode, Option<String>) {
    if local_helpers.is_empty() {
        if use_http_path == Some(true) || username.is_some_and(|v| !v.is_empty()) {
            return (RepoCredentialMode::UseHttpPath, None);
        }
        return (RepoCredentialMode::Inherit, None);
    }
    if local_helpers.len() == 1 && local_helpers[0].is_empty() {
        return (RepoCredentialMode::IsolatedPrompt, None);
    }
    if local_helpers.len() == 2
        && local_helpers[0].is_empty()
        && let Some(path) = parse_store_helper_path(&local_helpers[1])
    {
        return (RepoCredentialMode::IsolatedStore, Some(path));
    }
    (RepoCredentialMode::Custom, None)
}

/// Suggest an isolated credential-store path for `repo`.
pub fn suggested_store_file_path(repo: &GitRepo) -> String {
    let dir_name = repo
        .path()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("repo");
    let slug: String = dir_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let suffix = if slug.is_empty() { "repo" } else { slug.as_str() };
    let filename = format!(".git-credentials-{suffix}");

    if !repo.is_wsl()
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(filename).to_string_lossy().into_owned();
    }
    format!("~/{filename}")
}

/// Rewrite an HTTP(S) remote URL to embed or strip its userinfo username.
///
/// Returns `Ok(None)` if `Keep` or if the URL already matches the requested state.
pub fn rewrite_http_remote_url_username(
    url: &str,
    action: RemoteUsernameAction,
    username: Option<&str>,
) -> Result<Option<String>, String> {
    if action == RemoteUsernameAction::Keep {
        return Ok(None);
    }
    let trimmed = url.trim();
    let Some((protocol, rest)) = trimmed.split_once("://") else {
        return Err("Only HTTP/HTTPS remote URLs can embed or strip a username".into());
    };
    let lower_protocol = protocol.to_ascii_lowercase();
    if lower_protocol != "http" && lower_protocol != "https" {
        return Err("Only HTTP/HTTPS remote URLs can embed or strip a username".into());
    }
    let (authority, suffix) = match rest.find(['/', '?', '#']) {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    let host = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    };
    if host.is_empty() {
        return Err("The remote URL does not name a host".into());
    }
    let target = match action {
        RemoteUsernameAction::Keep => return Ok(None),
        RemoteUsernameAction::Strip => format!("{protocol}://{host}{suffix}"),
        RemoteUsernameAction::Embed => {
            let user = username
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "Enter a username before embedding it into the remote URL".to_string()
                })?;
            let encoded = encode_url_userinfo(user);
            format!("{protocol}://{encoded}@{host}{suffix}")
        }
    };
    if target == trimmed {
        Ok(None)
    } else {
        Ok(Some(target))
    }
}

/// Extract `username` and whether a non-empty `password=` line was returned by
/// `git credential fill`, discarding the secret immediately.
pub fn parse_credential_fill_output(stdout: &str) -> (Option<String>, bool) {
    let mut username = None;
    let mut password_present = false;
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "username" if !value.is_empty() => username = Some(value.to_string()),
            "password" if !value.is_empty() => password_present = true,
            _ => {}
        }
    }
    (username, password_present)
}

pub(super) fn parse_http_url_parts(url: &str) -> Option<(String, String, Option<String>)> {
    let trimmed = url.trim();
    let (protocol, rest) = trimmed.split_once("://")?;
    let protocol = protocol.to_ascii_lowercase();
    if protocol != "http" && protocol != "https" {
        return None;
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, Some(path)),
        None => (rest, None),
    };
    let host = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    }
    .trim();
    if host.is_empty() || host.contains(' ') {
        return None;
    }
    let path = path
        .map(|value| value.trim_matches('/').to_string())
        .filter(|value| !value.is_empty());
    Some((protocol, host.to_string(), path))
}

fn encode_url_userinfo(user: &str) -> String {
    let mut out = String::with_capacity(user.len());
    for byte in user.as_bytes() {
        if matches!(
            byte,
            b'%' | b'@' | b':' | b'/' | b'?' | b'#' | b'[' | b']' | b' '
        ) || *byte < 0x20
            || *byte == 0x7F
        {
            out.push_str(&format!("%{byte:02X}"));
        } else {
            out.push(*byte as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_helpers_parser_handles_empty_resets_and_values() {
        let raw = "global\x00file:/Users/dev/.gitconfig\x00credential.helper\nosxkeychain\x00local\x00file:.git/config\x00credential.helper\n\x00local\x00file:.git/config\x00credential.helper\nstore --file='/tmp/my creds'\x00";
        let parsed = parse_origin_helpers(raw, true);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].scope, "global");
        assert_eq!(parsed[0].value, "osxkeychain");
        assert_eq!(parsed[1].scope, "local");
        assert_eq!(parsed[1].value, "");
        assert_eq!(parsed[2].value, "store --file='/tmp/my creds'");
    }

    #[test]
    fn store_helper_path_parses_and_formats_paths_with_spaces() {
        let formatted = format_store_helper_value("/tmp/my dir/.git-creds'1");
        assert_eq!(formatted, "store --file='/tmp/my dir/.git-creds'\\''1'");
        assert_eq!(
            parse_store_helper_path(&formatted).as_deref(),
            Some("/tmp/my dir/.git-creds'1")
        );
        assert_eq!(
            parse_store_helper_path("store --file=/home/dev/.git-credentials-work").as_deref(),
            Some("/home/dev/.git-credentials-work")
        );
        assert_eq!(parse_store_helper_path("store"), None);
    }

    #[test]
    fn mode_classification_matches_strategies() {
        assert_eq!(
            classify_mode(None, None, &[]),
            (RepoCredentialMode::Inherit, None)
        );
        assert_eq!(
            classify_mode(Some(true), Some("alice"), &[]),
            (RepoCredentialMode::UseHttpPath, None)
        );
        assert_eq!(
            classify_mode(Some(true), None, &["".into()]),
            (RepoCredentialMode::IsolatedPrompt, None)
        );
        assert_eq!(
            classify_mode(
                Some(true),
                Some("alice"),
                &["".into(), "store --file=/tmp/creds".into()]
            ),
            (RepoCredentialMode::IsolatedStore, Some("/tmp/creds".into()))
        );
        assert_eq!(
            classify_mode(None, None, &["cache".into()]),
            (RepoCredentialMode::Custom, None)
        );
    }

    #[test]
    fn remote_url_username_rewrites_without_touching_non_http() {
        assert_eq!(
            rewrite_http_remote_url_username(
                "https://github.com/org/repo.git",
                RemoteUsernameAction::Embed,
                Some("alice@corp")
            ),
            Ok(Some("https://alice%40corp@github.com/org/repo.git".into()))
        );
        assert_eq!(
            rewrite_http_remote_url_username(
                "https://alice:secret@github.com/org/repo.git",
                RemoteUsernameAction::Strip,
                None
            ),
            Ok(Some("https://github.com/org/repo.git".into()))
        );
        assert!(
            rewrite_http_remote_url_username(
                "git@github.com:org/repo.git",
                RemoteUsernameAction::Embed,
                Some("alice")
            )
            .is_err()
        );
    }

    #[test]
    fn fill_output_discards_secret_and_reports_presence() {
        assert_eq!(
            parse_credential_fill_output("protocol=https\nhost=github.com\nusername=alice\npassword=top-secret\n"),
            (Some("alice".into()), true)
        );
        assert_eq!(
            parse_credential_fill_output("username=alice\npassword=\n"),
            (Some("alice".into()), false)
        );
    }
}
