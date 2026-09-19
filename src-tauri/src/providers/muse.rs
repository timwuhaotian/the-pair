use super::Provider;
use crate::provider_adapter::{
    CwdStrategy, InputTransport, OutputTransport, PermissionStrategy, ProviderRuntimeSpec,
    ProviderTurnCommand, ProviderTurnRequest, SessionStrategy,
};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::TurnTokenUsage;
use serde_json::Value;

/// Muse Code CLI (`muse`) — Meta's terminal coding agent, backed by Muse Spark.
///
/// Uses `muse exec --json`, which emits one JSONL envelope per line:
/// `{schema_version, id, stream{kind,id}, sequence, payload_type, payload}`.
/// Verified against Muse Code 1.0.3 (2026-09-19) — note the published docs at
/// dev.meta.ai/docs/muse-code still name `muse-spark-1.2` as the default while
/// the shipped binary uses `muse-spark-1.3`.
///
/// Two behaviours make this provider unusual:
///
/// - `--session-id` is *create-or-resume*, not merely a label. Re-running with
///   a previously used id continues that session's stream rather than starting
///   a new one, so `SessionStrategy::NewFirst` works: omit the flag on the
///   first turn, capture the id, pass it thereafter.
/// - Mentor turns get `--disable-write --disable-shell`, so the read-only role
///   is enforced by the CLI itself instead of by prompt convention alone. No
///   other provider in The Pair can make that guarantee.
pub struct MuseProvider;

/// Effort ladder advertised by `muse exec --help` (default `high`).
const MUSE_REASONING_EFFORTS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

impl Provider for MuseProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Muse
    }

    fn executable(&self) -> &str {
        "muse"
    }

    fn runtime_spec(&self) -> ProviderRuntimeSpec {
        ProviderRuntimeSpec {
            executable: "muse".into(),
            input_transport: InputTransport::Stdio,
            output_transport: OutputTransport::JsonEvents,
            session_strategy: SessionStrategy::NewFirst,
            // Turns always pass `--approval-mode never`; nothing is prompted.
            permission_strategy: PermissionStrategy::PreApproved,
            cwd_strategy: CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Muse model ids are bare (`muse-spark-1.3`); only the qualifier the
        // frontend adds for routing is stripped.
        let model = request.model.strip_prefix("muse/").unwrap_or(request.model);

        let mut args: Vec<String> = vec![
            "exec".into(),
            "--json".into(),
            "--model".into(),
            model.into(),
            // Headless turns must never block on an approval prompt.
            "--approval-mode".into(),
            "never".into(),
        ];

        if let Some(effort) = request.reasoning_effort {
            args.push("--reasoning-effort".into());
            args.push(effort.into());
        }

        if let Some(sid) = request.session_id {
            args.push("--session-id".into());
            args.push(sid.into());
        }

        // The Mentor plans and reviews but never edits. Muse can enforce that
        // at the tool layer, so the role prompt is backed by a real guarantee.
        if request.role == "mentor" {
            args.push("--disable-write".into());
            args.push("--disable-shell".into());
        }

        // The prompt is positional and must come last. A handoff message that
        // opens with "-" would otherwise be parsed as a flag.
        let prompt = if request.message.starts_with('-') {
            format!("\n{}", request.message)
        } else {
            request.message.to_string()
        };
        args.push(prompt);

        ProviderTurnCommand {
            executable: "muse".into(),
            args,
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, _event: &Value) -> Option<TurnTokenUsage> {
        // `muse exec --json` carries no usage data on any payload type
        // (verified against Muse Code 1.0.3 on 2026-09-19), so token counts
        // stay hidden for this provider, as they do for Kimi.
        None
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // Always bypass the generic text walker. Two payloads would otherwise
        // corrupt the transcript: `turn.input.user` carries `prompt`, which
        // would echo the agent's own handoff back as its reply, and
        // `run.output.delta` repeats text that the terminal payload already
        // contains in full, which `collapse_candidates` would join into a
        // duplicated message.
        let mut out = Vec::new();
        if is_run_terminal(event) {
            if let Some(text) = event
                .get("payload")
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
            {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
        }
        Some(out)
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        // Only a non-completed *run* terminal is a turn failure. Individual
        // `task.lifecycle.failed` events are not: a run routinely completes
        // successfully while an internal reminder subtask reports failure
        // (observed on 1.0.3 as "provider does not support base instructions"
        // on a run that still returned its answer). Treating those as errors
        // would fail healthy turns.
        if !is_run_terminal(event) {
            return None;
        }
        let payload = event.get("payload")?;
        let terminal = payload.get("terminal").and_then(|t| t.as_str())?;
        if terminal == "completed" {
            return None;
        }
        let reason = payload
            .get("reason")
            .and_then(|r| r.as_str())
            .map(str::trim)
            .filter(|r| !r.is_empty());
        Some(match reason {
            Some(reason) => format!("muse run {}: {}", terminal, reason),
            None => format!("muse run {}", terminal),
        })
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_muse()
    }

    fn brand(&self) -> &str {
        "muse"
    }

    fn provider_label(&self) -> &str {
        "Muse Code"
    }

    fn billing_kind(&self) -> &str {
        // Muse Spark is served by the Meta Model API against the signed-in
        // account or META_API_KEY.
        "plan"
    }

    fn billing_label(&self) -> &str {
        "Meta account"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "Muse Code login".into()
    }

    fn reasoning_effort_levels(&self, _model_id: &str) -> Option<Vec<String>> {
        Some(MUSE_REASONING_EFFORTS.iter().map(|e| e.to_string()).collect())
    }

    fn login_command(&self) -> Option<String> {
        Some("muse login".into())
    }

    fn install_url(&self) -> Option<String> {
        Some("https://dev.meta.ai/docs/muse-code".into())
    }
}

// ── Muse-specific helpers ──────────────────────────────────────────────────

/// True for `run.terminal.*` envelopes (`completed`, and the failure/cancel
/// variants), which carry the run's final text and outcome.
fn is_run_terminal(event: &Value) -> bool {
    event
        .get("payload_type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| t.starts_with("run.terminal."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request<'a>(role: &'a str, session_id: Option<&'a str>) -> ProviderTurnRequest<'a> {
        ProviderTurnRequest {
            provider_kind: ProviderKind::Muse,
            model: "muse/muse-spark-1.3",
            session_id,
            role,
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        }
    }

    #[test]
    fn executor_command_uses_headless_json_exec_without_readonly_flags() {
        let command = MuseProvider.build_turn_command(&request("executor", None));

        assert_eq!(command.executable, "muse");
        assert_eq!(
            command.args,
            vec![
                "exec".to_string(),
                "--json".to_string(),
                "--model".to_string(),
                // The frontend's routing qualifier is stripped.
                "muse-spark-1.3".to_string(),
                "--approval-mode".to_string(),
                "never".to_string(),
                "do the work".to_string(),
            ]
        );
        assert!(!command.args.iter().any(|a| a == "--disable-write"));
    }

    #[test]
    fn mentor_command_enforces_read_only_at_the_cli() {
        let command = MuseProvider.build_turn_command(&request("mentor", None));

        assert!(command.args.iter().any(|a| a == "--disable-write"));
        assert!(command.args.iter().any(|a| a == "--disable-shell"));
        // The prompt stays last so it is parsed as the positional argument.
        assert_eq!(command.args.last().map(String::as_str), Some("do the work"));
    }

    #[test]
    fn session_id_is_passed_for_resume_turns() {
        let command =
            MuseProvider.build_turn_command(&request("executor", Some("01a0b758-cd4f-7f32")));

        let idx = command
            .args
            .iter()
            .position(|a| a == "--session-id")
            .expect("--session-id should be present");
        assert_eq!(command.args[idx + 1], "01a0b758-cd4f-7f32");
    }

    #[test]
    fn reasoning_effort_is_forwarded_when_set() {
        let mut req = request("executor", None);
        req.reasoning_effort = Some("xhigh");
        let command = MuseProvider.build_turn_command(&req);

        let idx = command
            .args
            .iter()
            .position(|a| a == "--reasoning-effort")
            .expect("--reasoning-effort should be present");
        assert_eq!(command.args[idx + 1], "xhigh");
    }

    #[test]
    fn leading_dash_message_is_never_parsed_as_a_flag() {
        let mut req = request("executor", None);
        req.message = "- review the diff";
        let command = MuseProvider.build_turn_command(&req);

        let prompt = command.args.last().expect("prompt should be last");
        assert!(prompt.starts_with('\n'), "got {prompt:?}");
        assert!(prompt.contains("- review the diff"));
    }

    #[test]
    fn only_run_terminal_text_is_collected_as_the_turn_result() {
        let provider = MuseProvider;

        // The prompt echo must never become the reply.
        let user_input = json!({
            "payload_type": "turn.input.user",
            "payload": { "kind": "turn_input_user", "prompt": "do the work" }
        });
        assert_eq!(provider.collect_json_candidates(&user_input), Some(vec![]));

        // Deltas are skipped; the terminal payload already holds the full text.
        let delta = json!({
            "payload_type": "run.output.delta",
            "payload": { "kind": "run_output_delta", "text": "PONG" }
        });
        assert_eq!(provider.collect_json_candidates(&delta), Some(vec![]));

        let terminal = json!({
            "payload_type": "run.terminal.completed",
            "payload": { "kind": "run_terminal", "terminal": "completed", "text": "PONG", "reason": null }
        });
        assert_eq!(
            provider.collect_json_candidates(&terminal),
            Some(vec!["PONG".to_string()])
        );
    }

    #[test]
    fn failed_subtask_alongside_a_completed_run_is_not_a_turn_error() {
        let provider = MuseProvider;
        // Observed verbatim on Muse Code 1.0.3: an internal reminder subtask
        // fails while the run still returns its answer.
        let subtask_failure = json!({
            "payload_type": "task.lifecycle.failed",
            "payload": {
                "kind": "task_lifecycle",
                "event": {
                    "kind": "failed",
                    "reason": "invalid run configuration: provider does not support base instructions"
                }
            }
        });
        assert_eq!(provider.extract_error_detail(&subtask_failure), None);

        let completed = json!({
            "payload_type": "run.terminal.completed",
            "payload": { "kind": "run_terminal", "terminal": "completed", "text": "ok", "reason": null }
        });
        assert_eq!(provider.extract_error_detail(&completed), None);
    }

    #[test]
    fn non_completed_run_terminal_reports_the_reason() {
        let failed = json!({
            "payload_type": "run.terminal.failed",
            "payload": { "kind": "run_terminal", "terminal": "failed", "text": "", "reason": "model unavailable" }
        });
        assert_eq!(
            MuseProvider.extract_error_detail(&failed).as_deref(),
            Some("muse run failed: model unavailable")
        );

        let cancelled = json!({
            "payload_type": "run.terminal.cancelled",
            "payload": { "kind": "run_terminal", "terminal": "cancelled", "reason": null }
        });
        assert_eq!(
            MuseProvider.extract_error_detail(&cancelled).as_deref(),
            Some("muse run cancelled")
        );
    }

    #[test]
    fn no_token_usage_is_reported() {
        let terminal = json!({
            "payload_type": "run.terminal.completed",
            "payload": { "kind": "run_terminal", "terminal": "completed", "text": "ok" }
        });
        assert!(MuseProvider.extract_token_usage(&terminal).is_none());
    }

    #[test]
    fn reasoning_effort_ladder_matches_the_cli() {
        let levels = MuseProvider
            .reasoning_effort_levels("muse-spark-1.3")
            .expect("muse supports reasoning effort");
        assert_eq!(
            levels,
            vec!["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
        );
    }
}
