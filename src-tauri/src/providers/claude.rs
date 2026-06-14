use super::Provider;
use crate::provider_adapter::{ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

/// Claude Code CLI — uses `claude -p` with `stream-json` output format.
pub struct ClaudeProvider;

impl Provider for ClaudeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Claude
    }

    fn executable(&self) -> &str {
        "claude"
    }

    fn runtime_spec(&self) -> crate::provider_adapter::ProviderRuntimeSpec {
        crate::provider_adapter::ProviderRuntimeSpec {
            executable: "claude".into(),
            input_transport: crate::provider_adapter::InputTransport::Stdio,
            output_transport: crate::provider_adapter::OutputTransport::JsonEvents,
            session_strategy: crate::provider_adapter::SessionStrategy::ResumeExisting,
            permission_strategy: crate::provider_adapter::PermissionStrategy::PreApproved,
            cwd_strategy: crate::provider_adapter::CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Strip provider prefix if present (e.g. "claude/model-id" → "model-id").
        // The frontend sometimes sends qualified IDs through the updateModels path.
        let model = request
            .model
            .strip_prefix("claude/")
            .unwrap_or(request.model);
        let mut args = vec![
            "-p".into(),
            "--verbose".into(),
            "--model".into(),
            model.into(),
            "--output-format".into(),
            "stream-json".into(),
        ];
        if request.role == "mentor" {
            args.push("--permission-mode".into());
            args.push("plan".into());
        } else {
            args.push("--permission-mode".into());
            args.push("auto".into());
        }
        if let Some(sid) = request.session_id {
            args.push("--resume".into());
            args.push(sid.into());
        }
        // NOTE: Claude Code (2.1.x) exposes no CLI flag for reasoning/thinking
        // effort. Injecting `--reasoning-effort` hard-crashes the turn.
        args.push(request.message.into());

        ProviderTurnCommand {
            executable: "claude".into(),
            args,
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        let event_type = event.get("type").and_then(|v| v.as_str())?;

        let (usage_obj, is_final) = match event_type {
            "result" => {
                let usage = event.get("usage")?;
                (usage, true)
            }
            // stream-json emits per-message usage on `assistant` events (message.usage).
            "assistant" => {
                let usage = event.get("message")?.get("usage")?;
                (usage, false)
            }
            "content_block_delta" | "content_block_stop" => {
                let usage = event.get("usage")?;
                (usage, false)
            }
            _ => return None,
        };

        let output_tokens = usage_obj
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| usage_obj.get("completion_tokens").and_then(|v| v.as_u64()))?;

        let input_tokens = usage_obj
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| usage_obj.get("prompt_tokens").and_then(|v| v.as_u64()));

        Some(TurnTokenUsage {
            output_tokens,
            input_tokens,
            last_updated_at: crate::util::now_millis(),
            source: if is_final {
                TokenUsageSource::Final
            } else {
                TokenUsageSource::Live
            },
            provider: Some("claude".to_string()),
        })
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // Claude has its own structured output: first try the final `result` text,
        // then fall back to assistant text blocks from streaming events.
        let mut out = Vec::new();
        if let Some(text) = extract_claude_final_output(event) {
            push_trimmed(&mut out, &text);
        } else {
            collect_claude_assistant_text_blocks(event, &mut out);
        }
        Some(out)
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        // Detect a Claude Code `result` event that ended in error.
        if event.get("type").and_then(|v| v.as_str()) != Some("result") {
            return None;
        }

        let is_error = event
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let subtype = event.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
        if !is_error && subtype != "error" {
            return None;
        }

        if let Some(err) = event
            .get("error")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            return Some(err.trim().to_string());
        }

        if let Some(denials) = event
            .get("permission_denials")
            .and_then(|v| v.as_array())
            .filter(|arr| !arr.is_empty())
        {
            let tools: Vec<String> = denials
                .iter()
                .filter_map(|d| d.get("tool_name").and_then(|t| t.as_str()).map(String::from))
                .collect();
            if !tools.is_empty() {
                return Some(format!("Permission denied for tool(s): {}", tools.join(", ")));
            }
            return Some("Claude Code reported permission denials".to_string());
        }

        Some("Claude Code reported an error".to_string())
    }

    fn suppress_stderr(&self) -> bool {
        true
    }

    fn suppress_plain_output_logging(&self) -> bool {
        true
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_claude()
    }

    fn brand(&self) -> &str {
        "anthropic"
    }

    fn provider_label(&self) -> &str {
        "Claude Code"
    }

    fn billing_kind(&self) -> &str {
        "plan"
    }

    fn billing_label(&self) -> &str {
        "Included with plan"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "Claude Code login".into()
    }

    fn normalize_model_display_name(&self, display_name: &str) -> String {
        crate::provider_registry::beautify_claude_display_name(display_name)
    }

    fn login_command(&self) -> Option<String> {
        Some("claude login".into())
    }

    fn install_url(&self) -> Option<String> {
        Some("https://claude.ai/download".into())
    }
}

// ── Claude-specific helpers ────────────────────────────────────────────────

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

fn extract_claude_final_output(event: &Value) -> Option<String> {
    let event_type = event.get("type").and_then(|value| value.as_str())?;
    if event_type != "result" {
        return None;
    }
    event
        .get("result")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn collect_claude_assistant_text_blocks(event: &Value, out: &mut Vec<String>) {
    let event_type = event.get("type").and_then(|value| value.as_str());
    if event_type != Some("assistant") {
        return;
    }

    let Some(content) = event
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_array())
    else {
        return;
    };

    for block in content {
        if block.get("type").and_then(|value| value.as_str()) != Some("text") {
            continue;
        }
        if let Some(text) = block.get("text").and_then(|value| value.as_str()) {
            push_trimmed(out, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_command_uses_stream_json_and_resume_flags() {
        let provider = ClaudeProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Claude,
            model: "sonnet",
            session_id: Some("claude-session"),
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "claude");
        assert_eq!(
            command.args,
            vec![
                "-p".to_string(),
                "--verbose".to_string(),
                "--model".to_string(),
                "sonnet".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--permission-mode".to_string(),
                "plan".to_string(),
                "--resume".to_string(),
                "claude-session".to_string(),
                "plan the work".to_string()
            ]
        );
        assert!(command.last_message_path.is_none());
    }

    #[test]
    fn claude_command_omits_reasoning_effort_flag() {
        let provider = ClaudeProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Claude,
            model: "sonnet",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("high"),
        });

        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
        assert!(!command.args.contains(&"high".to_string()));
    }
}
