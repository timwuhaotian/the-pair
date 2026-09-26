use super::Provider;
use crate::provider_adapter::{
    CwdStrategy, InputTransport, OutputTransport, PermissionStrategy, ProviderRuntimeSpec,
    ProviderTurnCommand, ProviderTurnRequest, SessionStrategy,
};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

/// Antigravity CLI (`agy`) - Google's successor to the Gemini CLI.
/// Uses `agy --print --output-format stream-json`, which emits one NDJSON
/// envelope per line keyed by `event` (verified against agy 1.2.11, 2026-09-26):
///
/// - `{"event":"init","conversation_id":"…","init":{…}}` — the id
///   `--conversation` takes to resume.
/// - `{"event":"step_update","step_update":{"text_delta":"…","usage":{…}}}`
/// - `{"event":"result","result":{"status":"SUCCESS"|"ERROR","response":"…",
///   "error":"…","usage":{"input_tokens":…,"output_tokens":…}}}`
pub struct GeminiProvider;

impl Provider for GeminiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn executable(&self) -> &str {
        "agy"
    }

    fn runtime_spec(&self) -> ProviderRuntimeSpec {
        ProviderRuntimeSpec {
            executable: "agy".into(),
            input_transport: InputTransport::Stdio,
            output_transport: OutputTransport::JsonEvents,
            session_strategy: SessionStrategy::NewFirst,
            // agy runs with --dangerously-skip-permissions for auto-approval.
            permission_strategy: PermissionStrategy::PreApproved,
            cwd_strategy: CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Strip provider prefix if present (e.g. "gemini/model-id" -> "model-id").
        let model = request
            .model
            .strip_prefix("gemini/")
            .unwrap_or(request.model);
        ProviderTurnCommand {
            executable: "agy".into(),
            args: build_agy_args(
                model,
                request.message,
                request.role,
                request.reasoning_effort,
                request.session_id,
            ),
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        let (usage, is_final) = match event.get("event").and_then(|v| v.as_str())? {
            "result" => (event.get("result")?.get("usage")?, true),
            "step_update" => (event.get("step_update")?.get("usage")?, false),
            _ => return None,
        };

        let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64())?;
        // `input_tokens` excludes cache hits, which agy reports separately as
        // `cache_read_tokens`; fold them in so the count reflects the prompt.
        let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).map(|uncached| {
            uncached
                + usage
                    .get("cache_read_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
        });

        Some(TurnTokenUsage {
            output_tokens,
            input_tokens,
            last_updated_at: crate::util::now_millis(),
            source: if is_final {
                TokenUsageSource::Final
            } else {
                TokenUsageSource::Live
            },
            provider: Some("gemini".to_string()),
        })
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // `result.response` already holds the turn's full reply; the
        // `step_update.text_delta` fragments would only duplicate it, so every
        // other envelope bypasses the generic walker.
        let mut out = Vec::new();
        if event.get("event").and_then(|v| v.as_str()) == Some("result") {
            if let Some(text) = event.pointer("/result/response").and_then(|v| v.as_str()) {
                push_trimmed(&mut out, text);
            }
        }
        Some(out)
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        if event.get("event").and_then(|v| v.as_str()) != Some("result") {
            return None;
        }
        let status = event
            .pointer("/result/status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if status == "SUCCESS" {
            return None;
        }
        let detail = event
            .pointer("/result/error")
            .and_then(|v| v.as_str())
            .and_then(|s| s.lines().next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("agy turn ended with status {status}"));
        Some(detail)
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_gemini()
    }

    fn brand(&self) -> &str {
        "google"
    }

    fn provider_label(&self) -> &str {
        "Antigravity"
    }

    fn billing_kind(&self) -> &str {
        "plan"
    }

    fn billing_label(&self) -> &str {
        "Included with plan"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "Google account".into()
    }

    fn login_command(&self) -> Option<String> {
        // agy has no `auth` subcommand — sign-in happens when you launch the
        // interactive TUI (`agy`) and follow the browser flow, or by setting
        // `modelProvider: "gemini"` + `GEMINI_API_KEY` for headless runs.
        Some("agy".into())
    }

    fn install_url(&self) -> Option<String> {
        Some("https://github.com/google-antigravity/antigravity-cli".into())
    }
}

// ── agy-specific helpers ───────────────────────────────────────────────────

/// Build the `agy` CLI args for a single turn.
///
/// - **Mentor** (read-only planning): `--mode plan` restricts the agent to
///   read-only operations.
/// - **Executor** (code writing): `--mode accept-edits` allows file edits,
///   and `--dangerously-skip-permissions` auto-approves tool calls.
/// - **Reasoning effort**: `--effort <low|medium|high|max>` (`max` added in
///   agy 1.2.11, verified 2026-09-26). Omitted when the caller passes `None`;
///   the legacy `--thinking-budget` flag was never supported on `agy` and is
///   rejected outright, so we don't fall back to it. Note: agy model slugs
///   already encode effort (`gemini-3.8-flash-high`), and passing `--effort`
///   alongside such a slug is rejected as a conflict — which is why Gemini
///   models don't offer separate effort levels in the model picker.
/// - **Session**: `--conversation <id>` resumes the conversation captured from
///   the `init` event of an earlier turn.
///
/// No `--print-timeout` is passed: since agy 1.2.6 the default is to wait
/// until the turn completes, and an expired timeout returns *partial* output
/// with a success exit code, which would hand off a truncated turn.
pub fn build_agy_args(
    model: &str,
    message: &str,
    role: &str,
    reasoning_effort: Option<&str>,
    session_id: Option<&str>,
) -> Vec<String> {
    // agy uses Go's flag package, which treats an argv element starting with "-"
    // as a flag - prepend a newline to keep the first byte as '\n'.
    let prompt = if message.starts_with('-') {
        format!("\n{}", message)
    } else {
        message.to_string()
    };

    let mut args: Vec<String> = vec!["--output-format".into(), "stream-json".into()];

    if let Some(sid) = session_id {
        args.push("--conversation".into());
        args.push(sid.into());
    }

    // Role-based mode selection: mentor is read-only (plan), executor can edit files.
    if role == "mentor" {
        args.push("--mode".into());
        args.push("plan".into());
    } else {
        args.push("--mode".into());
        args.push("accept-edits".into());
        // Executor needs auto-approval to run tools without prompting.
        args.push("--dangerously-skip-permissions".into());
    }

    if let Some(effort) = reasoning_effort {
        args.push("--effort".into());
        args.push(effort.into());
    }

    args.push("--model".into());
    args.push(model.into());
    args.push("--print".into());
    args.push(prompt);

    args
}

fn push_trimmed(out: &mut Vec<String>, s: &str) {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return;
    }
    if out.last().map(|last| last == trimmed).unwrap_or(false) {
        return;
    }
    out.push(trimmed.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn agy_mentor_args_use_plan_mode_without_skip_permissions() {
        let args = build_agy_args(
            "gemini-3.8-flash-low",
            "explain the current diff",
            "mentor",
            None,
            None,
        );
        assert_eq!(
            args,
            vec![
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--mode".to_string(),
                "plan".to_string(),
                "--model".to_string(),
                "gemini-3.8-flash-low".to_string(),
                "--print".to_string(),
                "explain the current diff".to_string()
            ]
        );
        // Mentor should NOT have --dangerously-skip-permissions (plan mode is read-only).
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(!args.contains(&"--effort".to_string()));
    }

    #[test]
    fn agy_executor_args_use_accept_edits_and_skip_permissions() {
        let args = build_agy_args("gemini-3.8-flash-low", "do the work", "executor", None, None);
        assert_eq!(
            args,
            vec![
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--mode".to_string(),
                "accept-edits".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--model".to_string(),
                "gemini-3.8-flash-low".to_string(),
                "--print".to_string(),
                "do the work".to_string()
            ]
        );
    }

    #[test]
    fn agy_prepends_newline_for_leading_dash_prompt() {
        let args = build_agy_args("gemini-3.8-flash-low", "- Do the next step", "executor", None, None);
        assert_eq!(
            args.last().expect("prompt is last"),
            "\n- Do the next step"
        );

        let args = build_agy_args("gemini-3.8-flash-low", "Plan the refactor", "executor", None, None);
        assert_eq!(
            args.last().expect("prompt is last"),
            "Plan the refactor"
        );
    }

    #[test]
    fn agy_forwards_effort_when_reasoning_effort_is_set() {
        // `--effort` accepts low|medium|high (max added in agy 1.2.11). The
        // legacy `--thinking-budget` flag is rejected outright by agy, so we
        // never emit it.
        let args = build_agy_args(
            "gemini-3.8-flash-low",
            "do the work",
            "executor",
            Some("high"),
            None,
        );
        let effort_idx = args
            .iter()
            .position(|arg| arg == "--effort")
            .expect("--effort should be present when reasoning_effort is set");
        assert_eq!(args[effort_idx + 1], "high");
        assert!(!args.contains(&"--thinking-budget".to_string()));
        assert!(!args.contains(&"32768".to_string()));
    }

    #[test]
    fn agy_omits_effort_when_reasoning_effort_is_none() {
        let args = build_agy_args("gemini-3.8-flash-low", "do the work", "executor", None, None);
        assert!(!args.contains(&"--effort".to_string()));
    }

    #[test]
    fn gemini_command_omits_thinking_budget_flag() {
        // Regression guard: `--thinking-budget` was never an agy flag and is
        // rejected by the CLI. Even with reasoning effort set we use the new
        // `--effort` flag, never the legacy `--thinking-budget`.
        let provider = GeminiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "gemini-2.5-pro",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("high"),
        });

        assert!(!command.args.contains(&"--thinking-budget".to_string()));
        assert!(!command.args.contains(&"32768".to_string()));
    }

    #[test]
    fn agy_mentor_uses_plan_mode() {
        let provider = GeminiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "gemini-3.8-flash-low",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "agy");
        assert!(command.args.contains(&"--mode".to_string()));
        assert!(command.args.contains(&"plan".to_string()));
        assert!(!command.args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn agy_executor_uses_accept_edits_and_skip_permissions() {
        let provider = GeminiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "gemini-3.8-flash-low",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "agy");
        assert!(command.args.contains(&"--mode".to_string()));
        assert!(command.args.contains(&"accept-edits".to_string()));
        assert!(command.args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn agy_resumes_conversation_when_session_id_is_known() {
        let provider = GeminiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "gemini-3.8-flash-low",
            session_id: Some("235ff5af-1c2d"),
            role: "executor",
            pair_id: "pair-1",
            message: "continue",
            reasoning_effort: None,
        });

        let idx = command
            .args
            .iter()
            .position(|arg| arg == "--conversation")
            .expect("--conversation should be passed when a session id is known");
        assert_eq!(command.args[idx + 1], "235ff5af-1c2d");
        assert!(!command.args.contains(&"--print-timeout".to_string()));
    }

    // Envelopes below are verbatim shapes captured from
    // `agy --output-format stream-json --print` (agy 1.2.11, 2026-09-26).

    #[test]
    fn agy_result_event_yields_response_and_final_usage() {
        let provider = GeminiProvider;
        let result = json!({
            "event": "result",
            "result": {
                "conversation_id": "235ff5af-1c2d",
                "status": "SUCCESS",
                "response": "OK\n",
                "num_turns": 1,
                "usage": {
                    "input_tokens": 13878,
                    "output_tokens": 1,
                    "thinking_tokens": 0,
                    "cache_read_tokens": 20342,
                    "total_tokens": 13879
                }
            }
        });

        assert_eq!(
            provider.collect_json_candidates(&result),
            Some(vec!["OK".to_string()])
        );
        let usage = provider.extract_token_usage(&result).expect("result carries usage");
        assert_eq!(usage.output_tokens, 1);
        assert_eq!(usage.input_tokens, Some(13878 + 20342));
        assert!(matches!(usage.source, TokenUsageSource::Final));
        assert!(provider.extract_error_detail(&result).is_none());
    }

    #[test]
    fn agy_step_updates_report_live_usage_without_duplicating_text() {
        let provider = GeminiProvider;
        let step = json!({
            "event": "step_update",
            "step_update": {
                "step_type": "agent_response",
                "text_delta": "OK",
                "usage": { "input_tokens": 13878, "output_tokens": 1 }
            }
        });

        assert_eq!(provider.collect_json_candidates(&step), Some(vec![]));
        let usage = provider.extract_token_usage(&step).expect("step carries usage");
        assert!(matches!(usage.source, TokenUsageSource::Live));

        let init = json!({"event": "init", "conversation_id": "235ff5af-1c2d", "init": {}});
        assert_eq!(provider.collect_json_candidates(&init), Some(vec![]));
        assert!(provider.extract_token_usage(&init).is_none());
    }

    #[test]
    fn agy_error_result_surfaces_error_detail() {
        let provider = GeminiProvider;
        let failed = json!({
            "event": "result",
            "result": {
                "status": "ERROR",
                "response": "",
                "error": "invalid model selection \"nope\"\nsee `agy models`"
            }
        });
        assert_eq!(
            provider.extract_error_detail(&failed).as_deref(),
            Some("invalid model selection \"nope\"")
        );

        let no_message = json!({"event": "result", "result": {"status": "CANCELLED"}});
        assert_eq!(
            provider.extract_error_detail(&no_message).as_deref(),
            Some("agy turn ended with status CANCELLED")
        );
    }
}
