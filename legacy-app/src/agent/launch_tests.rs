use std::collections::HashMap;
use std::path::PathBuf;

use super::*;

#[test]
fn built_in_launch_overrides_use_provider_specific_arguments() {
    let settings = AgentSettings {
        launch_overrides: HashMap::from([
            (
                BuiltInAgent::Codex,
                AgentLaunchOverrides {
                    model: Some("gpt-5.4".into()),
                    reasoning_effort: Some("high".into()),
                    variant: None,
                },
            ),
            (
                BuiltInAgent::ClaudeCode,
                AgentLaunchOverrides {
                    model: Some("sonnet".into()),
                    reasoning_effort: Some("max".into()),
                    variant: None,
                },
            ),
            (
                BuiltInAgent::OpenCode,
                AgentLaunchOverrides {
                    model: Some("openai/gpt-5.4".into()),
                    reasoning_effort: None,
                    variant: Some("high".into()),
                },
            ),
        ]),
        ..Default::default()
    };

    let codex = settings.profile("codex").expect("codex profile");
    assert_eq!(
        codex
            .launch_spec_for_prompt_with_overrides(
                "diagnostic",
                &settings.launch_overrides[&BuiltInAgent::Codex],
            )
            .expect("codex launch")
            .args,
        vec![
            "--model",
            "gpt-5.4",
            "--config",
            "model_reasoning_effort=\"high\"",
            "diagnostic",
        ]
    );

    let claude = settings.profile("claude-code").expect("claude profile");
    assert_eq!(
        claude
            .launch_spec_for_prompt_with_overrides(
                "diagnostic",
                &settings.launch_overrides[&BuiltInAgent::ClaudeCode],
            )
            .expect("claude launch")
            .args,
        vec!["--model", "sonnet", "--effort", "max", "diagnostic"]
    );

    let opencode = settings.profile("opencode").expect("opencode profile");
    assert_eq!(
        opencode
            .launch_spec_for_prompt_with_overrides(
                "diagnostic",
                &settings.launch_overrides[&BuiltInAgent::OpenCode],
            )
            .expect("opencode launch")
            .args,
        vec![
            "--model",
            "openai/gpt-5.4",
            "--variant",
            "high",
            "--prompt",
            "diagnostic",
        ]
    );
}

#[test]
fn open_code_uses_variant_instead_of_reasoning_effort() {
    let profile = AgentSettings::default()
        .profile("opencode")
        .expect("opencode profile");
    let error = profile
        .launch_spec_for_prompt_with_overrides(
            "diagnostic",
            &AgentLaunchOverrides {
                reasoning_effort: Some("high".into()),
                ..Default::default()
            },
        )
        .expect_err("interactive OpenCode should reject reasoning");
    assert!(error.to_string().contains("model variant"));
}

#[test]
fn launch_override_validation_rejects_empty_and_control_values() {
    let empty = AgentLaunchOverrides {
        model: Some("  ".into()),
        ..Default::default()
    };
    assert!(empty.validate_for(BuiltInAgent::Codex).is_err());

    let control = AgentLaunchOverrides {
        model: Some("gpt-\n5".into()),
        ..Default::default()
    };
    assert!(control.validate_for(BuiltInAgent::ClaudeCode).is_err());

    let unsupported = AgentLaunchOverrides {
        reasoning_effort: Some("ultracode".into()),
        ..Default::default()
    };
    assert!(unsupported.validate_for(BuiltInAgent::ClaudeCode).is_err());

    let invalid_variant = AgentLaunchOverrides {
        variant: Some("fast\npreview".into()),
        ..Default::default()
    };
    assert!(
        invalid_variant
            .validate_for(BuiltInAgent::OpenCode)
            .is_err()
    );

    let wrong_agent_variant = AgentLaunchOverrides {
        variant: Some("high".into()),
        ..Default::default()
    };
    assert!(
        wrong_agent_variant
            .validate_for(BuiltInAgent::Codex)
            .is_err()
    );
}

#[test]
fn custom_profiles_reject_typed_launch_overrides() {
    let profile = AgentSettings {
        custom_profiles: vec![CustomAgentProfile {
            id: "custom".into(),
            name: "Custom".into(),
            executable: PathBuf::from("custom-agent"),
            args: Vec::new(),
            prompt_mode: PromptMode::TrailingArgument,
        }],
        ..Default::default()
    }
    .profile("custom")
    .expect("custom profile");
    assert_eq!(
        profile
            .launch_spec_for_prompt_with_overrides(
                "diagnostic",
                &AgentLaunchOverrides {
                    model: Some("model".into()),
                    ..Default::default()
                },
            )
            .expect_err("custom overrides should be rejected"),
        LaunchSpecError::CustomProfileOverrides
    );
}
