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
/// Verified against kiro-cli 2.23.0 (2026-09-23): `chat --model <MODEL>`
/// selects the model (unknown ids are rejected). Headless
/// `--agent-engine v2 --output-format stream-json` (v2 is now the default
/// engine) is available but its event format is undocumented, so the
/// plain-text transport is kept; switching to the structured stream would
/// enable session-id capture and token usage, neither of which surface today.
///
/// Permissions are per role. The executor runs with `--trust-all-tools`. The
/// mentor only trusts [`MENTOR_TRUSTED_TOOLS`], Kiro's read-only built-ins
/// (`read` is also known as `fs_read`). Every other tool (`write`, `shell`,
/// `aws`, `code`, ...) would need approval, and `--no-interactive` refuses
/// approval requests ("tool permission approval is not supported in
/// non-interactive mode", kiro-cli 2.24.0). The call fails and the turn
/// continues. So the mentor never hangs, and it cannot edit the worktree
/// unless the user's own Kiro settings pre-approve writes or commands.
pub struct KiroProvider;

/// Kiro's read-only built-in tools, passed as `--trust-tools=<list>` on
/// mentor turns. Names come from the Kiro built-in tools reference; `code` is
/// left out because it can also rewrite code.
const MENTOR_TRUSTED_TOOLS: &str = "read,grep,glob";

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
            // Multi-turn pairs *intend* to resume via `--resume-id <SESSION_ID>`,
            // but the plain-text transport never surfaces a session id, so the
            // flag is unreachable today (every turn starts fresh). Switching to
            // `--output-format stream-json` would expose the session id and make
            // this effective; see the module docstring.
            session_strategy: SessionStrategy::ResumeExisting,
            // Tool calls are pre-approved so turns run unattended: the executor
            // with --trust-all-tools, the mentor only for read-only tools.
            permission_strategy: PermissionStrategy::PreApproved,
            cwd_strategy: CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Strip "kiro/" qualifier if present.
        let model = request
            .model
            .strip_prefix("kiro/")
            .unwrap_or(request.model);

        // Guard leading-dash prompts.
        let prompt = if request.message.starts_with('-') {
            format!("\n{}", request.message)
        } else {
            request.message.to_string()
        };

        // The mentor is read-only: it may only use the read-only tools. The
        // `=` form keeps the comma-separated list bound to the flag.
        let trust = if request.role == "mentor" {
            format!("--trust-tools={MENTOR_TRUSTED_TOOLS}")
        } else {
            "--trust-all-tools".to_string()
        };
        let mut args: Vec<String> = vec!["chat".into(), "--no-interactive".into(), trust];

        // Pairs saved before 2.8.1 may hold a whole `--list-models` table row
        // as their model id; Kiro rejects unknown ids, so only a real model
        // token is forwarded and anything else keeps the account default.
        if !model.is_empty() && !model.contains(char::is_whitespace) {
            args.push("--model".into());
            args.push(model.into());
        }

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
        // Plain-text output carries no token usage data (verified against
        // kiro-cli 2.23.0 on 2026-09-23). Structured usage would require the
        // headless `--agent-engine v2 --output-format stream-json` stream.
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
            model: "claude-sonnet-4.5",
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
            model: "claude-sonnet-4.5",
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
            model: "claude-sonnet-4.5",
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
            model: "claude-sonnet-4.5",
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
            model: "claude-sonnet-4.5",
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
    fn kiro_mentor_trusts_only_read_only_tools() {
        let provider = KiroProvider;
        let request = |role| ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "claude-sonnet-4.5",
            session_id: None,
            role,
            pair_id: "pair-1",
            message: "work",
            reasoning_effort: None,
        };

        let mentor = provider.build_turn_command(&request("mentor"));
        assert!(
            !mentor.args.contains(&"--trust-all-tools".to_string()),
            "the read-only mentor must not pre-approve write/shell tools"
        );
        assert!(mentor
            .args
            .contains(&"--trust-tools=read,grep,glob".to_string()));
        assert!(mentor.args.contains(&"--no-interactive".to_string()));
        assert_eq!(mentor.args.last().unwrap(), "work");

        let executor = provider.build_turn_command(&request("executor"));
        assert!(executor.args.contains(&"--trust-all-tools".to_string()));
        assert!(!executor
            .args
            .iter()
            .any(|arg| arg.starts_with("--trust-tools")));
    }

    #[test]
    fn kiro_reports_no_token_usage() {
        let provider = KiroProvider;
        assert!(provider
            .extract_token_usage(&serde_json::json!({}))
            .is_none());
    }

    #[test]
    fn kiro_forwards_model_flag() {
        let provider = KiroProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "kiro/claude-sonnet-4.5",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        let idx = command.args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(command.args[idx + 1], "claude-sonnet-4.5");

        // A legacy table-row id is not a model token and is not forwarded.
        let legacy = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "* auto   1.00x credits   Models chosen by task",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        assert!(!legacy.args.contains(&"--model".to_string()));
    }
}
