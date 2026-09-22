use super::Provider;
use crate::provider_adapter::{
    ProviderRuntimeSpec, ProviderTurnCommand, ProviderTurnRequest, SessionStrategy,
};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

/// Grok Build CLI (`grok`) — xAI's terminal coding agent.
/// Uses `grok -p` with `--output-format streaming-messages-json` (Messages API
/// stream-json): a `system`/`init` line, one whole `assistant` message per
/// model response, and a terminal `result` carrying the final text,
/// `session_id`, `is_error`/`errors[]`, and the turn's `usage`. The plain
/// `streaming-json` format was dropped because its `text` lines are raw
/// ~10 ms stream fragments that cannot be trimmed and re-joined safely.
/// Verified against Grok Build 1.0.40 and the xai-org/grok-build headless
/// documentation (2026-09-23): `-p <prompt>`, `-m <model>`, `--resume <id>`,
/// `--reasoning-effort <level>`, `--yolo`, and the read-only `--tools` allowlist.
pub struct GrokProvider;

impl Provider for GrokProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Grok
    }

    fn executable(&self) -> &str {
        "grok"
    }

    fn runtime_spec(&self) -> ProviderRuntimeSpec {
        ProviderRuntimeSpec {
            executable: "grok".into(),
            input_transport: crate::provider_adapter::InputTransport::Stdio,
            output_transport: crate::provider_adapter::OutputTransport::JsonEvents,
            session_strategy: SessionStrategy::ResumeExisting,
            permission_strategy: crate::provider_adapter::PermissionStrategy::PreApproved,
            cwd_strategy: crate::provider_adapter::CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Strip provider prefix if present (e.g. "grok/grok-4.6" → "grok-4.6").
        let model = request.model.strip_prefix("grok/").unwrap_or(request.model);
        // The prompt is the value of `-p`; guard leading dashes so the arg parser
        // never mistakes a handoff message like "- Do the next step" for a flag.
        let prompt = if request.message.starts_with('-') {
            format!("\n{}", request.message)
        } else {
            request.message.to_string()
        };

        let mut args: Vec<String> = vec![
            "-p".into(),
            prompt,
            "--output-format".into(),
            "streaming-messages-json".into(),
            "-m".into(),
            model.into(),
            // Headless turns must never block on interactive permission
            // prompts; both roles run with auto-approval. The mentor's
            // read-only guarantee comes from the tool allowlist below.
            "--yolo".into(),
        ];
        if request.role == "mentor" {
            // Grok's documented read-only tool allowlist (internal tool IDs,
            // verified 2026-09-19): the mentor can read and search the repo
            // but has no file-editing or shell tools available at all.
            args.push("--tools".into());
            args.push("read_file,grep,list_dir".into());
        }
        if let Some(sid) = request.session_id {
            args.push("--resume".into());
            args.push(sid.into());
        }
        if let Some(level) = request.reasoning_effort {
            args.push("--reasoning-effort".into());
            args.push(level.into());
        }

        ProviderTurnCommand {
            executable: "grok".into(),
            args,
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        let event_type = event.get("type").and_then(|v| v.as_str())?;

        let (usage_obj, is_final) = match event_type {
            // `result` is the terminal line and carries the turn's full spend.
            "result" => (event.get("usage")?, true),
            // One `assistant` message per model response, before the `result`.
            "assistant" => (event.get("message")?.get("usage")?, false),
            _ => return None,
        };

        let output_tokens = usage_obj
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| usage_obj.get("completion_tokens").and_then(|v| v.as_u64()))?;

        // Grok reports `input_tokens` as the uncached portion only; fold the
        // cache buckets in so the displayed count reflects the full prompt.
        let input_tokens = usage_obj.get("input_tokens").and_then(|v| v.as_u64()).map(
            |uncached| {
                uncached
                    + usage_obj
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                    + usage_obj
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
            },
        );

        Some(TurnTokenUsage {
            output_tokens,
            input_tokens,
            last_updated_at: crate::util::now_millis(),
            source: if is_final {
                TokenUsageSource::Final
            } else {
                TokenUsageSource::Live
            },
            provider: Some("grok".to_string()),
        })
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // `result.result` is the final assistant message text. Intermediate
        // `assistant` messages (narration between tool calls), `thinking`
        // blocks, and tool traffic must not leak into the handoff, so the
        // generic text walker is always bypassed.
        let mut out = Vec::new();
        if event.get("type").and_then(|v| v.as_str()) == Some("result")
            && event.get("is_error").and_then(|v| v.as_bool()) != Some(true)
        {
            if let Some(text) = event.get("result").and_then(|v| v.as_str()) {
                push_trimmed(&mut out, text);
            }
        }
        Some(out)
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        const FALLBACK: &str = "Grok Build reported an error";
        match event.get("type").and_then(|v| v.as_str()) {
            // Error subtypes: `error_max_turns`, `error_during_execution`, …
            Some("result") if event.get("is_error").and_then(|v| v.as_bool()) == Some(true) => {
                let errors = event
                    .get("errors")
                    .and_then(|v| v.as_array())
                    .map(|errors| {
                        errors
                            .iter()
                            .filter_map(|e| e.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .filter(|s| !s.is_empty());
                Some(errors.unwrap_or_else(|| FALLBACK.to_string()))
            }
            // Session-level failures (e.g. auth) arrive as a bare error object.
            Some("error") => Some(
                event
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(FALLBACK)
                    .to_string(),
            ),
            _ => None,
        }
    }

    fn suppress_stderr(&self) -> bool {
        // Grok writes updater notices and logs to stderr; stdout is pure NDJSON.
        true
    }

    fn suppress_plain_output_logging(&self) -> bool {
        true
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_grok()
    }

    fn brand(&self) -> &str {
        "xai"
    }

    fn provider_label(&self) -> &str {
        "Grok Build"
    }

    fn billing_kind(&self) -> &str {
        "byok"
    }

    fn billing_label(&self) -> &str {
        "Pay as you go"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "Grok Build login".into()
    }

    fn reasoning_effort_levels(&self, model_id: &str) -> Option<Vec<String>> {
        // Each model only honors the levels its menu advertises. Grok Build
        // 1.0.40's bundled catalog gives grok-4.6 xhigh/high/medium/low and
        // grok-4.5 high/medium/low; custom aliases get the safe trio.
        let mut levels: Vec<String> = vec!["low".into(), "medium".into(), "high".into()];
        if model_id.strip_prefix("grok/").unwrap_or(model_id) == "grok-4.6" {
            levels.push("xhigh".into());
        }
        Some(levels)
    }

    fn login_command(&self) -> Option<String> {
        Some("grok login".into())
    }

    fn install_url(&self) -> Option<String> {
        Some("https://github.com/xai-org/grok-build".into())
    }
}

// ── Grok-specific helpers ──────────────────────────────────────────────────

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
    fn grok_executor_command_uses_print_mode_with_streaming_json() {
        let provider = GrokProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Grok,
            model: "grok-4.6",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "grok");
        assert_eq!(
            command.args,
            vec![
                "-p".to_string(),
                "do the work".to_string(),
                "--output-format".to_string(),
                "streaming-messages-json".to_string(),
                "-m".to_string(),
                "grok-4.6".to_string(),
                "--yolo".to_string(),
            ]
        );
        assert!(command.last_message_path.is_none());
    }

    #[test]
    fn grok_mentor_command_restricts_to_read_only_tools() {
        let provider = GrokProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Grok,
            model: "grok-4.6",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: None,
        });

        let tools_idx = command
            .args
            .iter()
            .position(|a| a == "--tools")
            .expect("mentor should carry a --tools allowlist");
        assert_eq!(command.args[tools_idx + 1], "read_file,grep,list_dir");
        // The executor gets no allowlist.
        let executor = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Grok,
            model: "grok-4.6",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        assert!(!executor.args.contains(&"--tools".to_string()));
    }

    #[test]
    fn grok_command_resumes_session_and_strips_provider_prefix() {
        let provider = GrokProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Grok,
            model: "grok/grok-4.6",
            session_id: Some("abc123"),
            role: "executor",
            pair_id: "pair-1",
            message: "continue",
            reasoning_effort: None,
        });

        assert_eq!(command.args[5], "grok-4.6");
        let resume_idx = command
            .args
            .iter()
            .position(|a| a == "--resume")
            .expect("--resume should be present with a session id");
        assert_eq!(command.args[resume_idx + 1], "abc123");
    }

    #[test]
    fn grok_command_passes_reasoning_effort_when_set() {
        let provider = GrokProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Grok,
            model: "grok-4.6",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("high"),
        });

        let effort_idx = command
            .args
            .iter()
            .position(|a| a == "--reasoning-effort")
            .expect("--reasoning-effort should be present when set");
        assert_eq!(command.args[effort_idx + 1], "high");

        let unset = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Grok,
            model: "grok-4.6",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        assert!(!unset.args.contains(&"--reasoning-effort".to_string()));
    }

    #[test]
    fn grok_prepends_newline_for_leading_dash_prompt() {
        let provider = GrokProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Grok,
            model: "grok-4.6",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "- Do the next step",
            reasoning_effort: None,
        });

        assert_eq!(command.args[1], "\n- Do the next step");
    }

    // Line shapes follow the streaming-messages-json examples in the
    // xai-org/grok-build headless documentation (2026-09-23).

    #[test]
    fn grok_collects_only_the_final_result_text() {
        let provider = GrokProvider;

        let assistant = json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me read the file."},
                    {"type": "tool_use", "id": "call_1", "name": "read_file", "input": {"path": "src/main.rs"}}
                ],
                "usage": {"input_tokens": 812, "output_tokens": 45}
            },
            "session_id": "abc123"
        });
        assert_eq!(provider.collect_json_candidates(&assistant), Some(vec![]));

        let init = json!({"type": "system", "subtype": "init", "session_id": "abc123"});
        assert_eq!(provider.collect_json_candidates(&init), Some(vec![]));

        let result = json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": "I've updated `src/main.rs`.\n\nAll tests pass.",
            "session_id": "abc123"
        });
        assert_eq!(
            provider.collect_json_candidates(&result),
            Some(vec!["I've updated `src/main.rs`.\n\nAll tests pass.".to_string()])
        );
    }

    #[test]
    fn grok_extracts_live_usage_and_folds_cache_buckets_into_input() {
        let provider = GrokProvider;

        let live = json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [],
                "usage": {
                    "input_tokens": 812,
                    "output_tokens": 45,
                    "cache_read_input_tokens": 1000,
                    "cache_creation_input_tokens": 8
                }
            }
        });
        let usage = provider.extract_token_usage(&live).expect("assistant usage");
        assert_eq!(usage.output_tokens, 45);
        assert_eq!(usage.input_tokens, Some(812 + 1000 + 8));
        assert_eq!(usage.source, TokenUsageSource::Live);

        let result = json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "session_id": "abc123",
            "usage": {"input_tokens": 7210, "output_tokens": 1893, "cache_read_input_tokens": 0}
        });
        let usage = provider.extract_token_usage(&result).expect("result usage");
        assert_eq!(usage.output_tokens, 1893);
        assert_eq!(usage.input_tokens, Some(7210));
        assert_eq!(usage.source, TokenUsageSource::Final);

        let init = json!({"type": "system", "subtype": "init"});
        assert!(provider.extract_token_usage(&init).is_none());
    }

    #[test]
    fn grok_extracts_error_detail_from_error_results_and_events() {
        let provider = GrokProvider;

        let failed = json!({
            "type": "result",
            "subtype": "error_during_execution",
            "is_error": true,
            "errors": ["tool crashed", " "],
            "result": "partial"
        });
        assert_eq!(provider.extract_error_detail(&failed).as_deref(), Some("tool crashed"));
        assert_eq!(provider.collect_json_candidates(&failed), Some(vec![]));

        let error = json!({"type": "error", "message": "Couldn't start session: bad auth"});
        assert_eq!(
            provider.extract_error_detail(&error).as_deref(),
            Some("Couldn't start session: bad auth")
        );

        let bare = json!({"type": "error"});
        assert_eq!(
            provider.extract_error_detail(&bare).as_deref(),
            Some("Grok Build reported an error")
        );

        let ok = json!({"type": "result", "is_error": false, "result": "done"});
        assert!(provider.extract_error_detail(&ok).is_none());
    }

    #[test]
    fn grok_reports_login_command_install_url_and_effort_levels() {
        let provider = GrokProvider;
        assert_eq!(provider.login_command().as_deref(), Some("grok login"));
        assert_eq!(
            provider.install_url().as_deref(),
            Some("https://github.com/xai-org/grok-build")
        );
        assert_eq!(
            provider.reasoning_effort_levels("grok-4.6"),
            Some(vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string()
            ])
        );
        assert_eq!(
            provider.reasoning_effort_levels("grok-4.5").map(|l| l.len()),
            Some(3)
        );
    }
}