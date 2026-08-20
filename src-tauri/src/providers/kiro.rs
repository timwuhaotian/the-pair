use super::Provider;
use crate::provider_adapter::{
    CwdStrategy, InputTransport, OutputTransport, PermissionStrategy, ProviderRuntimeSpec,
    ProviderTurnCommand, ProviderTurnRequest, SessionStrategy,
};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::TurnTokenUsage;
use serde_json::Value;

/// Kiro CLI (`kiro-cli`) — AWS's spec-driven terminal coding agent.
/// Uses `kiro-cli chat --no-interactive` for plain-text stdout output.
/// Kiro manages models via its own agent config; the model ID is not passed
/// as a `--model` flag but via agent configuration selection.
pub struct KiroProvider;

impl Provider for KiroProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Kiro
    }

    fn executable(&self) -> &str {
        "kiro-cli"
    }

    fn runtime_spec(&self) -> ProviderRuntimeSpec {
        ProviderRuntimeSpec {
            executable: "kiro-cli".into(),
            input_transport: InputTransport::Stdio,
            // kiro-cli chat --no-interactive prints plain text to stdout.
            output_transport: OutputTransport::Stdio,
            // Multi-turn pairs resume via `--resume-id <SESSION_ID>`.
            session_strategy: SessionStrategy::ResumeExisting,
            // --trust-all-tools pre-approves every tool call for unattended operation.
            permission_strategy: PermissionStrategy::PreApproved,
            cwd_strategy: CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Strip "kiro/" qualifier if present.
        let _model = request
            .model
            .strip_prefix("kiro/")
            .unwrap_or(request.model);

        // Guard leading-dash prompts.
        let prompt = if request.message.starts_with('-') {
            format!("\n{}", request.message)
        } else {
            request.message.to_string()
        };

        let mut args: Vec<String> = vec![
            "chat".into(),
            "--no-interactive".into(),
            "--trust-all-tools".into(),
        ];

        // Continue a previous conversation when resuming a pair: the Daytona
        // integration (and Kiro's own chat docs) use `--resume-id <SESSION_ID>`
        // to carry context across turns.
        if let Some(sid) = request.session_id {
            args.push("--resume-id".into());
            args.push(sid.into());
        }

        if let Some(effort) = request.reasoning_effort {
            args.push("--effort".into());
            args.push(effort.into());
        }

        // Prompt is the trailing positional argument.
        args.push(prompt);

        ProviderTurnCommand {
            executable: "kiro-cli".into(),
            args,
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, _event: &Value) -> Option<TurnTokenUsage> {
        // Plain-text output carries no token usage data.
        None
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_kiro()
    }

    fn brand(&self) -> &str {
        "kiro"
    }

    fn provider_label(&self) -> &str {
        "Kiro"
    }

    fn billing_kind(&self) -> &str {
        "plan"
    }

    fn billing_label(&self) -> &str {
        "Included with plan"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "Kiro plan".into()
    }

    fn reasoning_effort_levels(&self, _model_id: &str) -> Option<Vec<String>> {
        Some(vec![
            "low".into(),
            "medium".into(),
            "high".into(),
            "xhigh".into(),
            "max".into(),
        ])
    }

    fn login_command(&self) -> Option<String> {
        Some("kiro-cli login".into())
    }

    fn install_url(&self) -> Option<String> {
        Some("https://kiro.dev/downloads".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kiro_command_uses_no_interactive_with_trust_all() {
        let provider = KiroProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "claude-sonnet-4-5",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "kiro-cli");
        assert!(command
            .args
            .contains(&"--no-interactive".to_string()));
        assert!(command.args.contains(&"--trust-all-tools".to_string()));
        assert!(command.last_message_path.is_none());
        // Prompt is the last positional arg.
        assert_eq!(command.args.last().unwrap(), "do the work");
    }

    #[test]
    fn kiro_injects_effort_level() {
        let provider = KiroProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "claude-sonnet-4-5",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: Some("high"),
        });

        let effort_idx = command
            .args
            .iter()
            .position(|a| a == "--effort")
            .expect("should have --effort flag");
        assert_eq!(command.args[effort_idx + 1], "high");
    }

    #[test]
    fn kiro_resumes_session_via_resume_id() {
        let provider = KiroProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "claude-sonnet-4-5",
            session_id: Some("session-xyz"),
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        let resume_idx = command
            .args
            .iter()
            .position(|a| a == "--resume-id")
            .expect("resuming a pair should pass --resume-id");
        assert_eq!(command.args[resume_idx + 1], "session-xyz");

        // Without a session id there is no --resume-id flag.
        let fresh = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "claude-sonnet-4-5",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        assert!(!fresh.args.contains(&"--resume-id".to_string()));
    }

    #[test]
    fn kiro_prepends_newline_for_leading_dash_prompt() {
        let provider = KiroProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "claude-sonnet-4-5",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "- Do the next step",
            reasoning_effort: None,
        });

        let last = command.args.last().unwrap();
        assert_eq!(last, "\n- Do the next step");
    }

    #[test]
    fn kiro_both_roles_use_trust_all_tools() {
        let provider = KiroProvider;
        for role in ["mentor", "executor"] {
            let command = provider.build_turn_command(&ProviderTurnRequest {
                provider_kind: ProviderKind::Kiro,
                model: "claude-sonnet-4-5",
                session_id: None,
                role,
                pair_id: "pair-1",
                message: "work",
                reasoning_effort: None,
            });
            assert!(
                command.args.contains(&"--trust-all-tools".to_string()),
                "role {} should have --trust-all-tools",
                role
            );
        }
    }

    #[test]
    fn kiro_reports_no_token_usage() {
        let provider = KiroProvider;
        assert!(provider
            .extract_token_usage(&serde_json::json!({}))
            .is_none());
    }
}
