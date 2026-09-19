use super::Provider;
use crate::provider_adapter::{
    ProviderRuntimeSpec, ProviderTurnCommand, ProviderTurnRequest, SessionStrategy,
};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

/// Grok Build CLI (`grok`) — xAI's terminal coding agent.
/// Uses `grok -p` with `--output-format streaming-json`: each stdout line is a
/// `type`-tagged JSON object (`text`, `thought`, `tool_call`, `usage`, `end`,
/// `error`). Verified against the xai-org/grok-build headless documentation
/// (2026-09-19): `-p <prompt>`, `-m <model>`, `--resume <id>`,
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
            "streaming-json".into(),
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
            // `end` is the terminal event and carries the turn's full spend.
            "end" => (event.get("usage")?, true),
            // One `usage` event per model response, before the final `end`.
            "usage" => (event.get("usage")?, false),
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
        // Only `text` events carry the turn result. `thought` (reasoning),
        // `tool_call`/`tool_call_update`, `plan`, and the terminal `end`/`error`
        // events must not leak into the message, so the generic text walker is
        // always bypassed.
        let mut out = Vec::new();
        if event.get("type").and_then(|v| v.as_str()) == Some("text") {
            if let Some(data) = event.get("data").and_then(|v| v.as_str()) {
                push_trimmed(&mut out, data);
            }
        }
        Some(out)
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        if event.get("type").and_then(|v| v.as_str()) != Some("error") {
            return None;
        }

        event
            .get("message")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .or_else(|| Some("Grok Build reported an error".to_string()))
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

    fn reasoning_effort_levels(&self, _model_id: &str) -> Option<Vec<String>> {
        // Grok accepts none/minimal/low/medium/high/xhigh/max canonically, but
        // each model only honors the levels its menu advertises; expose the
        // universally safe low/medium/high trio (verified 2026-09-19).
        Some(vec!["low".into(), "medium".into(), "high".into()])
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
                "streaming-json".to_string(),
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

    #[test]
    fn grok_collects_only_text_event_data() {
        let provider = GrokProvider;

        let text = json!({"type": "text", "data": "done"});
        assert_eq!(
            provider.collect_json_candidates(&text),
            Some(vec!["done".to_string()])
        );

        let thought = json!({"type": "thought", "data": "thinking aloud"});
        assert_eq!(provider.collect_json_candidates(&thought), Some(vec![]));

        let tool_call = json!({
            "type": "tool_call",
            "toolCallId": "call_1",
            "toolName": "read_file",
            "rawInput": {"path": "src/main.rs"},
            "content": []
        });
        assert_eq!(provider.collect_json_candidates(&tool_call), Some(vec![]));

        let end = json!({"type": "end", "stopReason": "end_turn", "sessionId": "abc123"});
        assert_eq!(provider.collect_json_candidates(&end), Some(vec![]));
    }

    #[test]
    fn grok_extracts_live_usage_and_folds_cache_buckets_into_input() {
        let provider = GrokProvider;

        let live = json!({
            "type": "usage",
            "usage": {
                "input_tokens": 812,
                "output_tokens": 45,
                "cache_read_input_tokens": 1000,
                "cache_creation_input_tokens": 8,
                "reasoning_tokens": 0
            }
        });
        let usage = provider.extract_token_usage(&live).expect("usage event");
        assert_eq!(usage.output_tokens, 45);
        assert_eq!(usage.input_tokens, Some(812 + 1000 + 8));
        assert_eq!(usage.source, TokenUsageSource::Live);

        let end = json!({
            "type": "end",
            "stopReason": "end_turn",
            "sessionId": "abc123",
            "usage": {"input_tokens": 7210, "output_tokens": 1893}
        });
        let usage = provider.extract_token_usage(&end).expect("end event");
        assert_eq!(usage.output_tokens, 1893);
        assert_eq!(usage.input_tokens, Some(7210));
        assert_eq!(usage.source, TokenUsageSource::Final);

        // Non-usage events carry no token data.
        let text = json!({"type": "text", "data": "done"});
        assert!(provider.extract_token_usage(&text).is_none());
    }

    #[test]
    fn grok_extracts_error_detail_from_error_events() {
        let provider = GrokProvider;

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

        let text = json!({"type": "text", "data": "done"});
        assert!(provider.extract_error_detail(&text).is_none());
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
                "high".to_string()
            ])
        );
    }
}