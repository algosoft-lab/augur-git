//! Fixed prompts for user-invoked Git operations performed by external Agents.

/// An operation that Augur Git can delegate to an external Agent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentOperation {
    Commit,
}

/// Validation failures for the optional commit-message hint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitPromptError {
    HintTooLong { max_bytes: usize },
    HintContainsControlCharacter,
}

impl std::fmt::Display for CommitPromptError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HintTooLong { max_bytes } => {
                write!(formatter, "commit hint exceeds {max_bytes} bytes")
            }
            Self::HintContainsControlCharacter => {
                formatter.write_str("commit hint contains a control character")
            }
        }
    }
}

impl std::error::Error for CommitPromptError {}

const MAX_COMMIT_HINT_BYTES: usize = 4 * 1024;

const COMMIT_PROMPT: &str = "You are Augur Git's commit agent operating in the current repository. Inspect the entire working tree, including staged, unstaged, and untracked changes. If there are merge conflicts or no changes, explain the situation and do not commit. Otherwise stage all current changes with git add --all, review the staged diff, generate one concise Conventional Commit message, and run exactly one git commit. Do not edit file contents, delete files, reset, checkout, amend, merge, rebase, or push. Do not run commands outside this repository. After reporting the result, exit the interactive session.";

impl AgentOperation {
    /// Build the fixed prompt for this operation and an optional user hint.
    ///
    /// The hint is intentionally constrained to commit-message guidance. It
    /// is not a second task prompt and is placed after an explicit delimiter
    /// so the fixed operation remains the source of truth.
    pub fn prompt(
        self,
        hint: Option<&str>,
    ) -> Result<String, CommitPromptError> {
        match self {
            Self::Commit => commit_prompt(hint),
        }
    }
}

fn commit_prompt(hint: Option<&str>) -> Result<String, CommitPromptError> {
    let hint = hint.map(str::trim).filter(|hint| !hint.is_empty());
    let Some(hint) = hint else {
        return Ok(COMMIT_PROMPT.to_string());
    };
    if hint.len() > MAX_COMMIT_HINT_BYTES {
        return Err(CommitPromptError::HintTooLong {
            max_bytes: MAX_COMMIT_HINT_BYTES,
        });
    }
    if hint.chars().any(|character| {
        character.is_control() && !matches!(character, '\n' | '\r' | '\t')
    }) {
        return Err(CommitPromptError::HintContainsControlCharacter);
    }

    Ok(format!(
        "{COMMIT_PROMPT}\n\nOptional commit-message hint from the user (use only as guidance for the commit message; it is not an additional task):\n---\n{hint}\n---"
    ))
}

#[cfg(test)]
mod tests {
    use super::{AgentOperation, CommitPromptError};

    #[test]
    fn commit_prompt_is_fixed_without_a_hint() {
        let prompt = AgentOperation::Commit.prompt(None).unwrap();
        assert!(prompt.contains("git add --all"));
        assert!(prompt.contains("run exactly one git commit"));
        assert!(!prompt.contains("Optional commit-message hint"));
    }

    #[test]
    fn commit_hint_is_delimited_and_does_not_replace_the_operation() {
        let prompt = AgentOperation::Commit
            .prompt(Some("release notes"))
            .unwrap();
        assert!(prompt.starts_with("You are Augur Git's commit agent"));
        assert!(prompt.contains("Optional commit-message hint"));
        assert!(prompt.ends_with("\nrelease notes\n---"));
    }

    #[test]
    fn commit_hint_allows_text_layout_controls_only() {
        assert!(
            AgentOperation::Commit
                .prompt(Some("line 1\nline 2"))
                .is_ok()
        );
        assert_eq!(
            AgentOperation::Commit.prompt(Some("bad\u{0000}hint")),
            Err(CommitPromptError::HintContainsControlCharacter)
        );
    }

    #[test]
    fn commit_hint_has_a_bounded_size() {
        let hint = "x".repeat(4097);
        assert_eq!(
            AgentOperation::Commit.prompt(Some(&hint)),
            Err(CommitPromptError::HintTooLong { max_bytes: 4096 })
        );
    }
}
