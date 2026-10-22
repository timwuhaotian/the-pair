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
/// `chat --model <MODEL>` selects the model — on the default (V2) engine only
/// from kiro-cli 2.26.1, which fixed `--model` being ignored by
/// `--no-interactive` runs. Unknown ids are not rejected: V2 passes them to the
/// backend and V3 warns on stderr and keeps the account default (kiro.dev
/// docs, re-checked against kiro-cli 2.28.0 on 2026-10-06). The CLI now also
/// carries a V3 engine alongside V2; the turn command pins the V2 engine
/// (`--v2`, falling back to `--agent-engine v2`) on every turn, because V3's
/// tool-id names break the mentor's `--trust-tools` allowlist (see
/// `kiro_engine_pin`). AWS has announced CLI 3.0 defaults to V3 at the end of
/// October 2026; if a release drops the V2 pin entirely, mentor turns are
/// refused rather than silently running with write access
/// (`validate_turn_request`). Flags re-verified against `kiro-cli chat
/// --help` on 2.33.1 (2026-10-22). Headless `--output-format stream-json`
/// emits the run's ACP events as JSON Lines (and implies `--no-interactive`),
/// but that schema is unparsed today, so the plain-text transport is kept; switching to the structured stream would
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

fn build_kiro_turn_command(
    request: &ProviderTurnRequest,
    engine_pin: crate::provider_registry::KiroEnginePin,
) -> ProviderTurnCommand {
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
    let mut args: Vec<String> = vec!["chat".into(), "--no-interactive".into()];

    // Pin the V2 engine whenever the CLI offers a pin (see `kiro_engine_pin`;
    // the probe keys on the help text, not the version). A user's V3 startup
    // offer persists `chat.agentEngine = v3`, and V3 matches `--trust-tools`
    // against exact internal tool ids that the mentor's V2 names never match —
    // silently dropping the read-only pre-approval below. `Unavailable`
    // (V3 present, no pin) never reaches here for the mentor —
    // `validate_turn_request` refuses it — and executor turns, which trust
    // everything anyway, may ride whichever engine the CLI defaults to.
    match engine_pin {
        crate::provider_registry::KiroEnginePin::V2Shorthand => args.push("--v2".into()),
        crate::provider_registry::KiroEnginePin::AgentEngineFlag => {
            args.push("--agent-engine".into());
            args.push("v2".into());
        }
        crate::provider_registry::KiroEnginePin::NotNeeded
        | crate::provider_registry::KiroEnginePin::Unavailable
        | crate::provider_registry::KiroEnginePin::Unknown => {}
    }

    args.push(trust);

    // Pairs saved before 2.8.1 may hold a whole `--list-models` table row
    // as their model id. Kiro forwards an unknown id to the backend rather
    // than rejecting it, so only a real model token is passed and anything
    // else keeps the account default.
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

/// Whether a turn may start under `engine_pin`. The mentor's read-only
/// guarantee is the V2 `--trust-tools` allowlist; on a CLI whose V3 engine
/// can no longer be pinned away, those names match nothing and the mentor
/// would run with whatever the user's settings allow. Refuse the turn
/// loudly instead. The executor trusts all tools on any engine, so it is
/// never refused.
fn validate_kiro_turn(
    role: &str,
    engine_pin: crate::provider_registry::KiroEnginePin,
) -> Result<(), String> {
    if role == "mentor" && engine_pin == crate::provider_registry::KiroEnginePin::Unavailable {
        return Err(
            crate::provider_registry::KIRO_V2_PIN_UNAVAILABLE_MESSAGE.to_string(),
        );
    }
    Ok(())
}

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

    fn validate_turn_request(&self, request: &ProviderTurnRequest) -> Result<(), String> {
        validate_kiro_turn(request.role, crate::provider_registry::kiro_engine_pin())
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        build_kiro_turn_command(request, crate::provider_registry::kiro_engine_pin())
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
    fn kiro_pins_the_v2_engine_with_the_spelling_the_cli_offers() {
        use crate::provider_registry::KiroEnginePin;
        let request = ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "claude-sonnet-4.5",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        };

        // `--v2` is the spelling 2.32 documents for holding the pre-3.0
        // default; it wins when the help advertises it.
        let shorthand = build_kiro_turn_command(&request, KiroEnginePin::V2Shorthand);
        assert!(shorthand.args.contains(&"--v2".to_string()));
        assert!(!shorthand.args.contains(&"--agent-engine".to_string()));

        let flagged = build_kiro_turn_command(&request, KiroEnginePin::AgentEngineFlag);
        let pin_idx = flagged
            .args
            .iter()
            .position(|a| a == "--agent-engine")
            .expect("flag pin should be present");
        assert_eq!(flagged.args[pin_idx + 1], "v2");
        // The pin sits before the trust flags so it always applies.
        let trust_idx = flagged
            .args
            .iter()
            .position(|a| a.starts_with("--trust"))
            .expect("trust flag");
        assert!(pin_idx < trust_idx);

        // Pre-V3 CLIs and inconclusive probes must not receive an unknown
        // flag; `Unavailable` executor turns ride the CLI default too
        // (mentor turns are refused before this point).
        for pin in [
            KiroEnginePin::NotNeeded,
            KiroEnginePin::Unavailable,
            KiroEnginePin::Unknown,
        ] {
            let command = build_kiro_turn_command(&request, pin);
            assert!(!command.args.contains(&"--v2".to_string()));
            assert!(!command.args.contains(&"--agent-engine".to_string()));
        }
    }

    #[test]
    fn kiro_refuses_mentor_turns_when_the_v2_pin_is_gone() {
        use crate::provider_registry::KiroEnginePin;
        // The scenario CLI 3.0 threatens: V3 present, no V2 pin. The mentor
        // must be refused rather than silently run writable; every other
        // combination still runs.
        let refused = validate_kiro_turn("mentor", KiroEnginePin::Unavailable)
            .expect_err("mentor must be refused without a V2 pin");
        assert!(refused.contains("V2 engine"), "{refused}");
        assert!(validate_kiro_turn("executor", KiroEnginePin::Unavailable).is_ok());
        for pin in [
            KiroEnginePin::V2Shorthand,
            KiroEnginePin::AgentEngineFlag,
            KiroEnginePin::NotNeeded,
            KiroEnginePin::Unknown,
        ] {
            assert!(validate_kiro_turn("mentor", pin).is_ok(), "{pin:?}");
        }

        // The trait method resolves the pin from the installed CLI and
        // applies the same rule (the exact outcome depends on the machine:
        // no kiro, or a pinnable one → Ok; an unpinnable V3 CLI → refused).
        let provider = KiroProvider;
        let request = ProviderTurnRequest {
            provider_kind: ProviderKind::Kiro,
            model: "claude-sonnet-4.5",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan",
            reasoning_effort: None,
        };
        assert_eq!(
            provider.validate_turn_request(&request),
            validate_kiro_turn("mentor", crate::provider_registry::kiro_engine_pin())
        );
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
