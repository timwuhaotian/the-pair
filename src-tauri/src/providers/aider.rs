use super::Provider;
use crate::provider_adapter::{
    ProviderRuntimeSpec, ProviderTurnCommand, ProviderTurnRequest,
};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::TurnTokenUsage;
use serde_json::Value;

/// Aider CLI (`aider`) — open-source AI pair programmer in your terminal.
/// Headless invocation: `aider --message "<prompt>" --no-pretty --stream
/// --yes-always --no-auto-commits --no-dirty-commits`. Aider emits plain
/// Markdown to stdout (verified against aider-chat 0.86.x); there is no
/// `--json` flag, so the orchestrator reads the streamed text directly
/// via `OutputTransport::Stdio`.
/// Aider is stateless per `--message` invocation (git history is the persistence),
/// so `SessionStrategy::NewFirst` is the correct choice.
pub struct AiderProvider;

impl Provider for AiderProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Aider
    }

    fn executable(&self) -> &str {
        "aider"
    }

    fn runtime_spec(&self) -> ProviderRuntimeSpec {
        ProviderRuntimeSpec {
            executable: "aider".into(),
            input_transport: crate::provider_adapter::InputTransport::Stdio,
            // Aider has no structured output mode; --stream emits plain Markdown
            // tokens directly to stdout.
            output_transport: crate::provider_adapter::OutputTransport::Stdio,
            session_strategy: crate::provider_adapter::SessionStrategy::NewFirst,
            permission_strategy: crate::provider_adapter::PermissionStrategy::Auto,
            cwd_strategy: crate::provider_adapter::CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Strip provider prefix if present (e.g. "aider/claude-sonnet-4-6" → "claude-sonnet-4-6").
        let model = request
            .model
            .strip_prefix("aider/")
            .unwrap_or(request.model);
        // Guard leading dashes so the arg parser never mistakes a handoff
        // message like "- Do the next step" for a flag (same as Kimi).
        let prompt = if request.message.starts_with('-') {
            format!("\n{}", request.message)
        } else {
            request.message.to_string()
        };

        let mut args: Vec<String> = vec![
            "--message".into(),
            prompt,
            "--model".into(),
            model.into(),
            // Headless flags: auto-approve all actions, don't dirty the git
            // history with auto-commits, and stream plain Markdown to stdout.
            // Aider has no --json flag (verified against aider-chat 0.86.x).
            "--yes-always".into(),
            "--no-auto-commits".into(),
            "--no-dirty-commits".into(),
            "--no-pretty".into(),
            "--stream".into(),
        ];

        // Aider forwards `--reasoning-effort` to models that accept it.
        if let Some(effort) = request.reasoning_effort {
            args.push("--reasoning-effort".into());
            args.push(effort.into());
        }

        ProviderTurnCommand {
            executable: "aider".into(),
            args,
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, _event: &Value) -> Option<TurnTokenUsage> {
        // Aider has no structured token usage field on its streamed output
        // (verified against aider-chat 0.86.x). Token counts stay hidden for
        // this provider, matching Kimi.
        None
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // Legacy: Aider was assumed to expose `--json` NDJSON events, but the
        // flag does not exist in aider-chat 0.86.x — assistant text arrives as
        // plain Markdown on stdout. This branch is unreachable for real Aider
        // output today (the orchestrator uses `OutputTransport::Stdio`); it is
        // retained so the orchestrator can still harvest text if a future
        // Aider release reintroduces a structured event format.
        let mut out = Vec::new();

        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if event_type == "content" || event_type == "message" || event_type == "response" {
            collect_aider_text(event.get("content"), &mut out);
            collect_aider_text(event.get("text"), &mut out);
        }

        Some(out)
    }

    fn suppress_stderr(&self) -> bool {
        true
    }

    fn suppress_plain_output_logging(&self) -> bool {
        true
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_aider()
    }

    fn brand(&self) -> &str {
        "aider"
    }

    fn provider_label(&self) -> &str {
        "Aider"
    }

    fn billing_kind(&self) -> &str {
        "byok"
    }

    fn billing_label(&self) -> &str {
        "Pay as you go"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "API key".into()
    }

    fn login_command(&self) -> Option<String> {
        None
    }

    fn install_url(&self) -> Option<String> {
        Some("https://aider.chat".into())
    }
}

// ── Aider-specific helpers ──────────────────────────────────────────────────

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

/// Assistant `content` is a plain string today; tolerate OpenAI-style
/// `[{"type": "text", "text": …}]` block arrays for forward compatibility.
fn collect_aider_text(content: Option<&Value>, out: &mut Vec<String>) {
    match content {
        Some(Value::String(text)) => push_trimmed(out, text),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    push_trimmed(out, text);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn aider_command_uses_message_no_pretty_stream() {
        let provider = AiderProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Aider,
            model: "claude-sonnet-4-6",
            session_id: Some("ignored"),
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "aider");
        assert_eq!(
            command.args,
            vec![
                "--message".to_string(),
                "do the work".to_string(),
                "--model".to_string(),
                "claude-sonnet-4-6".to_string(),
                "--yes-always".to_string(),
                "--no-auto-commits".to_string(),
                "--no-dirty-commits".to_string(),
                "--no-pretty".to_string(),
                "--stream".to_string()
            ]
        );
        // Aider streams plain text; the orchestrator reads it via Stdio, not a
        // sentinel last-message file.
        assert!(command.last_message_path.is_none());
    }

    #[test]
    fn aider_command_includes_no_auto_commits_and_yes_always() {
        let provider = AiderProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Aider,
            model: "gpt-5.4",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "fix the bug",
            reasoning_effort: None,
        });

        assert!(command.args.contains(&"--yes-always".to_string()));
        assert!(command.args.contains(&"--no-auto-commits".to_string()));
        assert!(command.args.contains(&"--no-dirty-commits".to_string()));
    }

    #[test]
    fn aider_command_strips_aider_prefix_from_model() {
        let provider = AiderProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Aider,
            model: "aider/claude-sonnet-4-6",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        let model_idx = command
            .args
            .iter()
            .position(|a| a == "--model")
            .expect("--model flag should be present");
        assert_eq!(command.args[model_idx + 1], "claude-sonnet-4-6");
    }

    #[test]
    fn aider_mentor_command_has_no_special_permission_flags() {
        let provider = AiderProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Aider,
            model: "claude-sonnet-4-6",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: None,
        });

        for flag in ["--plan", "--yolo", "--auto", "--permission-mode", "--ask"] {
            assert!(
                !command.args.contains(&flag.to_string()),
                "aider must not receive {}",
                flag
            );
        }
    }

    #[test]
    fn aider_prepends_newline_for_leading_dash_prompt() {
        let provider = AiderProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Aider,
            model: "claude-sonnet-4-6",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "- Do the next step",
            reasoning_effort: None,
        });

        let msg_idx = command
            .args
            .iter()
            .position(|a| a == "--message")
            .expect("--message flag should be present");
        assert_eq!(command.args[msg_idx + 1], "\n- Do the next step");
    }

    #[test]
    fn aider_command_forwards_reasoning_effort() {
        let provider = AiderProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Aider,
            model: "claude-sonnet-4-6",
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
            .expect("should have --reasoning-effort flag");
        assert_eq!(command.args[effort_idx + 1], "high");
    }

    #[test]
    fn aider_collects_assistant_text_from_json_events() {
        let provider = AiderProvider;

        let content_event = json!({"type": "content", "content": "done"});
        assert_eq!(
            provider.collect_json_candidates(&content_event),
            Some(vec!["done".to_string()])
        );

        let message_event = json!({"type": "message", "content": "working on it"});
        assert_eq!(
            provider.collect_json_candidates(&message_event),
            Some(vec!["working on it".to_string()])
        );

        let text_event = json!({"type": "response", "text": "here is the plan"});
        assert_eq!(
            provider.collect_json_candidates(&text_event),
            Some(vec!["here is the plan".to_string()])
        );
    }

    #[test]
    fn aider_ignores_non_content_events() {
        let provider = AiderProvider;

        let file_event = json!({"type": "file", "path": "src/main.rs", "action": "edit"});
        assert_eq!(
            provider.collect_json_candidates(&file_event),
            Some(vec![])
        );

        let tool_event = json!({"type": "command", "command": "ls", "output": "file.txt"});
        assert_eq!(
            provider.collect_json_candidates(&tool_event),
            Some(vec![])
        );
    }

    #[test]
    fn aider_collects_text_blocks_from_content_arrays() {
        let provider = AiderProvider;
        let event = json!({
            "type": "content",
            "content": [{"type": "text", "text": "part one"}, {"type": "text", "text": "part two"}]
        });
        assert_eq!(
            provider.collect_json_candidates(&event),
            Some(vec!["part one".to_string(), "part two".to_string()])
        );
    }

    #[test]
    fn aider_reports_no_token_usage() {
        let provider = AiderProvider;
        let event = json!({"type": "content", "content": "done"});
        assert!(provider.extract_token_usage(&event).is_none());
    }

    #[test]
    fn aider_ignores_session_id_since_stateless() {
        let provider = AiderProvider;
        let with_session = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Aider,
            model: "claude-sonnet-4-6",
            session_id: Some("some-session-id"),
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        let without_session = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Aider,
            model: "claude-sonnet-4-6",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        // Aider is stateless — session_id must not affect the command.
        assert_eq!(with_session.args, without_session.args);
    }
}