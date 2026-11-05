//! Prompt parsing and entry matching.
//!
//! Git gives an askpass helper nothing but the prompt text, so the host has to
//! be recovered from it. The formats are Git's own:
//!
//! ```text
//! Username for 'https://github.com':
//! Password for 'https://dev@github.com:8443':
//! ```
//!
//! A prompt this module cannot parse is reported as [`PromptKind::Other`] and
//! keeps the plain "ask for one value, remember nothing" behaviour, so an
//! unfamiliar prompt degrades instead of guessing.

use super::model::{CredentialEntry, CredentialScope, RepositoryIdentity};

/// Which value Git asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Username,
    Password,
    Other,
}

impl PromptKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Username => "username",
            Self::Password => "password",
            Self::Other => "prompt",
        }
    }
}

/// What one prompt says about the remote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptContext {
    pub kind: Option<PromptKind>,
    pub protocol: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub username: Option<String>,
}

impl PromptContext {
    pub fn kind(&self) -> PromptKind {
        self.kind.unwrap_or(PromptKind::Other)
    }

    /// Whether the prompt describes a host this module can look up.
    pub fn is_remote(&self) -> bool {
        self.kind.is_some() && self.protocol.is_some() && self.host.is_some()
    }
}

/// The parts of an HTTP(S) remote URL this module needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedUrl {
    pub protocol: String,
    pub host: String,
    pub path: Option<String>,
    pub username: Option<String>,
}

/// Parse an HTTP(S) URL without guessing.
///
/// Anything that is not an `http`/`https` address with a host yields `None`, so
/// an SSH address is never mistaken for a host with a password.
pub fn parse_remote_url(url: &str) -> Option<ParsedUrl> {
    let url = url.trim();
    let (protocol, rest) = url.split_once("://")?;
    let protocol = protocol.to_ascii_lowercase();
    if protocol != "http" && protocol != "https" {
        return None;
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, Some(path)),
        None => (rest, None),
    };
    let (userinfo, host) = match authority.rsplit_once('@') {
        Some((userinfo, host)) => (Some(userinfo), host),
        None => (None, authority),
    };
    let host = normalize_host(host)?;
    Some(ParsedUrl {
        protocol,
        host,
        path: path
            .map(|path| path.trim_matches('/').to_string())
            .filter(|path| !path.is_empty()),
        username: userinfo.and_then(|userinfo| {
            let (name, _) = userinfo.split_once(':').unwrap_or((userinfo, ""));
            let name = name.trim();
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        }),
    })
}

/// Parse one askpass prompt into the remote it refers to.
pub fn parse_prompt(prompt: &str) -> PromptContext {
    let trimmed = prompt.trim();
    let kind = if trimmed.starts_with("Username for ") {
        PromptKind::Username
    } else if trimmed.starts_with("Password for ") {
        PromptKind::Password
    } else if trimmed.starts_with("Password:") || trimmed.starts_with("Username:") {
        // A prompt without a URL still needs one value, but says nothing about
        // which host it belongs to.
        PromptKind::Other
    } else {
        PromptKind::Other
    };

    let mut context = PromptContext {
        kind: Some(kind),
        ..PromptContext::default()
    };
    if kind == PromptKind::Other {
        return context;
    }
    let Some(url) = quoted_url(trimmed) else {
        context.kind = Some(PromptKind::Other);
        return context;
    };
    let Some(parsed) = parse_remote_url(url) else {
        context.kind = Some(PromptKind::Other);
        return context;
    };
    context.protocol = Some(parsed.protocol);
    context.host = Some(parsed.host);
    context.path = parsed.path;
    context.username = parsed.username;
    if kind == PromptKind::Username {
        // The URL of a username prompt carries the user only when Git already
        // decided on one; anything else would be the remote's own text.
        context.username = None;
    }
    context
}

/// The URL Git quotes inside its prompt.
///
/// Git writes `Username for 'https://example.com':`; `git-lfs` asks its own
/// questions with Go's `%q`, so the same address arrives as
/// `Username for "https://example.com"`. Both forms name the same remote, and
/// an LFS operation must not fall back to the remember-nothing dialog because
/// of a quote style.
fn quoted_url(prompt: &str) -> Option<&str> {
    let start = prompt.find(['\'', '"'])?;
    let quote = prompt[start..].chars().next()?;
    let rest = &prompt[start + quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

/// Lowercase a host and drop a default port so `example.com` and
/// `example.com:443` name the same account.
pub fn normalize_host(host: &str) -> Option<String> {
    let host = host.trim();
    if host.is_empty() || host.contains('/') || host.contains(' ') || host.contains('@') {
        return None;
    }
    let host = host.to_ascii_lowercase();
    Some(match host.rsplit_once(':') {
        Some((name, "443")) | Some((name, "80")) => name.to_string(),
        _ => host,
    })
}

/// The remote a Git command is talking to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteTarget {
    pub protocol: String,
    pub host: String,
    pub path: Option<String>,
    pub username: Option<String>,
    pub repository: Option<RepositoryIdentity>,
}

impl RemoteTarget {
    /// Build a target from a parsed prompt.
    pub fn from_prompt(
        context: &PromptContext,
        repository: Option<RepositoryIdentity>,
    ) -> Option<Self> {
        Some(Self {
            protocol: context.protocol.clone()?,
            host: context.host.clone()?,
            path: context.path.clone(),
            username: context.username.clone(),
            repository,
        })
    }

    /// The address a credential test would fetch, when the entry names a path.
    pub fn url(&self) -> String {
        match &self.path {
            Some(path) if !path.is_empty() => format!("{}://{}/{}", self.protocol, self.host, path),
            _ => format!("{}://{}", self.protocol, self.host),
        }
    }
}

/// Whether an entry may answer this target, and how well it fits.
///
/// A `None` result means the entry is for a different host, protocol, account,
/// or repository. A higher score is a more specific answer.
pub fn match_score(entry: &CredentialEntry, target: &RemoteTarget) -> Option<i64> {
    if !entry.protocol.eq_ignore_ascii_case(&target.protocol) {
        return None;
    }
    let entry_host = normalize_host(&entry.host)?;
    if entry_host != target.host {
        return None;
    }
    if let Some(username) = target.username.as_deref()
        && username != entry.username
    {
        return None;
    }
    if !entry.scope.matches(target.repository.as_ref()) {
        return None;
    }
    let mut score = match &entry.scope {
        CredentialScope::Global => 0,
        CredentialScope::Repository { .. } => 1000,
    };
    if let Some(prefix) = &entry.path_prefix {
        let path = target.path.as_deref()?;
        if !path_matches(path, prefix) {
            return None;
        }
        score += 100 + prefix.len() as i64;
    }
    if target
        .username
        .as_deref()
        .is_some_and(|username| username == entry.username)
    {
        score += 10;
    }
    Some(score)
}

/// Whether `path` is the prefix itself or lies under it.
fn path_matches(path: &str, prefix: &str) -> bool {
    let path = path.trim_matches('/');
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        return true;
    }
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// The best entry for a target, preferring the most specific and the most
/// recently updated one.
pub fn best_match<'a, I>(entries: I, target: &RemoteTarget) -> Option<&'a CredentialEntry>
where
    I: IntoIterator<Item = &'a CredentialEntry>,
{
    entries
        .into_iter()
        .filter_map(|entry| match_score(entry, target).map(|score| (score, entry)))
        .max_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.updated_at.cmp(&right.1.updated_at))
        })
        .map(|(_, entry)| entry)
}

/// Normalize a host typed into the add form.
pub fn normalize_typed_host(host: &str) -> Option<String> {
    let host = host.trim();
    let host = host
        .strip_prefix("https://")
        .or_else(|| host.strip_prefix("http://"))
        .unwrap_or(host);
    let host = host.split('/').next().unwrap_or(host);
    normalize_host(host)
}

/// Normalize a path prefix typed into the add form.
pub fn normalize_path_prefix(prefix: &str) -> Option<String> {
    let prefix = prefix.trim().trim_matches('/');
    if prefix.is_empty() {
        None
    } else {
        Some(prefix.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use augur_core::config::LocationConfig;

    fn entry(host: &str, username: &str) -> CredentialEntry {
        CredentialEntry {
            id: "id".into(),
            protocol: "https".into(),
            host: host.into(),
            path_prefix: None,
            username: username.into(),
            scope: CredentialScope::Global,
            storage: super::super::model::SecretStorage::Keychain,
            created_at: 1,
            updated_at: 1,
            last_used_at: None,
            last_failure_at: None,
        }
    }

    #[test]
    fn a_username_prompt_names_the_host() {
        let context = parse_prompt("Username for 'https://github.com': ");
        assert_eq!(context.kind(), PromptKind::Username);
        assert_eq!(context.protocol.as_deref(), Some("https"));
        assert_eq!(context.host.as_deref(), Some("github.com"));
        assert_eq!(context.username, None);
    }

    #[test]
    fn an_lfs_prompt_in_double_quotes_names_the_same_remote() {
        // git-lfs asks with Go's %q instead of Git's single quotes, and gives
        // the address no trailing colon.
        let context = parse_prompt("Username for \"http://gitlab01.zerolab:8000\"");
        assert_eq!(context.kind(), PromptKind::Username);
        assert_eq!(context.protocol.as_deref(), Some("http"));
        assert_eq!(context.host.as_deref(), Some("gitlab01.zerolab:8000"));
        assert_eq!(context.username, None);
        assert!(context.is_remote());

        let context = parse_prompt("Password for \"http://dev@gitlab01.zerolab:8000\": ");
        assert_eq!(context.kind(), PromptKind::Password);
        assert_eq!(context.host.as_deref(), Some("gitlab01.zerolab:8000"));
        assert_eq!(context.username.as_deref(), Some("dev"));
    }

    #[test]
    fn a_prompt_without_a_url_is_still_not_a_remote() {
        // A passphrase question is quoted too, and must stay a raw prompt.
        let context = parse_prompt("Enter passphrase for key '/home/dev/.ssh/id_ed25519': ");
        assert_eq!(context.kind(), PromptKind::Other);
        assert!(!context.is_remote());
    }

    #[test]
    fn a_password_prompt_names_the_account() {
        let context = parse_prompt("Password for 'https://dev@github.com:8443': ");
        assert_eq!(context.kind(), PromptKind::Password);
        assert_eq!(context.host.as_deref(), Some("github.com:8443"));
        assert_eq!(context.username.as_deref(), Some("dev"));
    }

    #[test]
    fn a_password_prompt_carries_the_path_when_the_url_has_one() {
        let context = parse_prompt("Password for 'https://dev@git.example.com/team/repo.git': ");
        assert_eq!(context.path.as_deref(), Some("team/repo.git"));
    }

    #[test]
    fn a_url_with_inline_password_keeps_only_the_username() {
        let context = parse_prompt("Password for 'https://dev:s3cret@git.example.com': ");
        assert_eq!(context.username.as_deref(), Some("dev"));
        assert_eq!(context.host.as_deref(), Some("git.example.com"));
    }

    #[test]
    fn an_ss_askpass_prompt_is_left_alone() {
        for prompt in [
            "Enter passphrase for key '/home/dev/.ssh/id_ed25519': ",
            "Password:",
            "",
        ] {
            assert_eq!(parse_prompt(prompt).kind(), PromptKind::Other, "{prompt}");
        }
    }

    #[test]
    fn a_malformed_url_is_not_guessed_at() {
        assert_eq!(
            parse_prompt("Password for 'https://': ").kind(),
            PromptKind::Other
        );
        assert_eq!(
            parse_prompt("Password for 'ssh://git@host': ").kind(),
            PromptKind::Other
        );
    }

    #[test]
    fn default_ports_and_case_name_the_same_host() {
        assert_eq!(
            normalize_host("GitHub.com:443").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            normalize_host("example.com:80").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            normalize_host("example.com:8443").as_deref(),
            Some("example.com:8443")
        );
        assert_eq!(normalize_host("bad host"), None);
    }

    #[test]
    fn a_target_matches_only_its_own_protocol_and_host() {
        let target = RemoteTarget {
            protocol: "https".into(),
            host: "github.com".into(),
            path: Some("team/repo.git".into()),
            username: None,
            repository: None,
        };
        assert_eq!(match_score(&entry("github.com", "dev"), &target), Some(0));
        assert_eq!(match_score(&entry("gitlab.com", "dev"), &target), None);
        let mut http = entry("github.com", "dev");
        http.protocol = "http".into();
        assert_eq!(match_score(&http, &target), None);
    }

    #[test]
    fn a_named_username_must_agree() {
        let target = RemoteTarget {
            protocol: "https".into(),
            host: "github.com".into(),
            path: None,
            username: Some("dev".into()),
            repository: None,
        };
        assert!(match_score(&entry("github.com", "dev"), &target).is_some());
        assert_eq!(match_score(&entry("github.com", "other"), &target), None);
    }

    #[test]
    fn a_path_prefix_matches_only_at_a_segment_boundary() {
        let mut scoped = entry("git.example.com", "dev");
        scoped.path_prefix = Some("team".into());
        let target = |path: &str| RemoteTarget {
            protocol: "https".into(),
            host: "git.example.com".into(),
            path: Some(path.into()),
            username: None,
            repository: None,
        };
        assert!(match_score(&scoped, &target("team/repo.git")).is_some());
        assert!(match_score(&scoped, &target("team")).is_some());
        assert_eq!(match_score(&scoped, &target("teamwork/repo.git")), None);
        assert_eq!(match_score(&scoped, &target("other/repo.git")), None);
    }

    #[test]
    fn a_repository_scoped_entry_answers_only_that_repository() {
        let mut scoped = entry("github.com", "dev");
        scoped.scope = CredentialScope::Repository {
            location: LocationConfig::Local,
            path: "/home/dev/work".into(),
        };
        let at = |path: &str| RemoteTarget {
            protocol: "https".into(),
            host: "github.com".into(),
            path: None,
            username: None,
            repository: Some(RepositoryIdentity {
                location: LocationConfig::Local,
                path: path.into(),
            }),
        };
        assert!(match_score(&scoped, &at("/home/dev/work")).is_some());
        assert_eq!(match_score(&scoped, &at("/home/dev/personal")), None);
    }

    #[test]
    fn the_most_specific_entry_wins() {
        let mut repository = entry("github.com", "work");
        repository.id = "repo".into();
        repository.scope = CredentialScope::Repository {
            location: LocationConfig::Local,
            path: "/home/dev/work".into(),
        };
        repository.updated_at = 1;
        let mut scoped = entry("github.com", "dev");
        scoped.id = "scoped".into();
        scoped.path_prefix = Some("team/repo".into());
        scoped.updated_at = 2;
        let global = entry("github.com", "dev");
        let target = RemoteTarget {
            protocol: "https".into(),
            host: "github.com".into(),
            path: Some("team/repo.git".into()),
            username: None,
            repository: Some(RepositoryIdentity {
                location: LocationConfig::Local,
                path: "/home/dev/work".into(),
            }),
        };
        let entries = [&repository, &scoped, &global];
        assert_eq!(
            best_match(entries, &target).map(|entry| entry.id.as_str()),
            Some("repo")
        );
    }

    #[test]
    fn a_typed_host_accepts_a_pasted_url() {
        assert_eq!(
            normalize_typed_host(" https://GitHub.com/team/repo.git ").as_deref(),
            Some("github.com")
        );
        assert_eq!(normalize_typed_host("").as_deref(), None);
    }

    #[test]
    fn a_typed_path_prefix_drops_slashes() {
        assert_eq!(
            normalize_path_prefix("/team/repo/").as_deref(),
            Some("team/repo")
        );
        assert_eq!(normalize_path_prefix("  "), None);
    }

    #[test]
    fn a_remote_url_is_parsed_without_guessing() {
        let parsed = parse_remote_url(" https://dev@GitHub.com:443/team/repo.git ").unwrap();
        assert_eq!(parsed.protocol, "https");
        assert_eq!(parsed.host, "github.com");
        assert_eq!(parsed.path.as_deref(), Some("team/repo.git"));
        assert_eq!(parsed.username.as_deref(), Some("dev"));
        assert_eq!(parse_remote_url("git@github.com:team/repo.git"), None);
        assert_eq!(parse_remote_url("https://"), None);
        assert_eq!(parse_remote_url(""), None);
    }

    #[test]
    fn a_target_url_is_built_from_its_parts() {
        let target = RemoteTarget {
            protocol: "https".into(),
            host: "git.example.com".into(),
            path: Some("team/repo.git".into()),
            username: None,
            repository: None,
        };
        assert_eq!(target.url(), "https://git.example.com/team/repo.git");
    }
}
