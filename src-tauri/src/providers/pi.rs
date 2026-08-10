use super::Provider;
use crate::provider_adapter::{ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::TurnTokenUsage;
use serde_json::Value;

/// Pi Agent CLI (`pi`) — earendil-works/pi terminal coding agent.
/// Uses `pi --mode json` to emit structured JSON event lines on stdout.
/// Pi is a BYOK multi-provider agent: model ids are `provider/model` format
/// (e.g. `anthropic/claude-sonnet-4`). The `pi/` qualifier added by The Pair
/// is stripped at spawn time so Pi receives the inner `provider/model` value.
pub struct PiProvider;

impl Provider for PiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Pi
    }

    fn executable(&self) -> &str {
        "pi"
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Strip the leading "pi/" qualifier added by The Pair, preserving the
        // inner "provider/model" that Pi needs (e.g. "pi/anthropic/claude-sonnet-4"
        // → "anthropic/claude-sonnet-4").
        let model = request.model.strip_prefix("pi/").unwrap_or(request.model);
        // Guard leading-dash prompts so clap doesn't mistake them for flags.
        let prompt = if request.message.starts_with('-') {
            format!("\n{}", request.message)
        } else {
            request.message.to_string()
        };

        let mut args: Vec<String> = vec![
            "--mode".into(),
            "json".into(),
            "--model".into(),
            model.into(),
        ];

        if let Some(effort) = request.reasoning_effort {
            args.push("--thinking".into());
            args.push(effort.into());
        }

        args.push(prompt);

        ProviderTurnCommand {
            executable: "pi".into(),
            args,
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, _event: &Value) -> Option<TurnTokenUsage> {
        // Pi's JSON event stream does not currently include token usage data.
        None
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // Pi emits typed events: {"type":"turn_end","message":{...}}, etc.
        // Only collect text from assistant messages on terminal events.
        let event_type = event.get("type").and_then(|v| v.as_str());

        match event_type {
            Some("turn_end") | Some("agent_end") | Some("message_end") => {}
            _ => return Some(Vec::new()), // bypass generic walker for all other events
        }

        // `turn_end` and `agent_end` nest the message under "message";
        // `message_end` also nests it under "message".
        let mut out = Vec::new();
        if let Some(msg) = event.get("message") {
            if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                collect_pi_content(msg.get("content"), &mut out);
            }
        }
        // `agent_end` also carries a top-level "messages" array
        if event_type == Some("agent_end") {
            if let Some(messages) = event.get("messages").and_then(|v| v.as_array()) {
                for msg in messages {
                    if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                        collect_pi_content(msg.get("content"), &mut out);
                    }
                }
            }
        }
        Some(out)
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_pi()
    }

    fn brand(&self) -> &str {
        "pi"
    }

    fn provider_label(&self) -> &str {
        "Pi"
    }

    fn billing_kind(&self) -> &str {
        "byok"
    }

    fn billing_label(&self) -> &str {
        "Pay as you go"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "Pi config".into()
    }

    fn reasoning_effort_levels(&self, _model_id: &str) -> Option<Vec<String>> {
        // Pi supports --thinking universally across all models/providers.
        Some(vec![
            "off".into(),
            "low".into(),
            "medium".into(),
            "high".into(),
            "xhigh".into(),
            "max".into(),
        ])
    }

    fn install_url(&self) -> Option<String> {
        Some("https://pi.dev".into())
    }
}

// ── Pi-specific helpers ───────────────────────────────────────────────────

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

/// Pi assistant `content` is an array of typed blocks. We extract text blocks
/// and skip thinking/tool blocks.
fn collect_pi_content(content: Option<&Value>, out: &mut Vec<String>) {
    match content {
        Some(Value::String(text)) => push_trimmed(out, text),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        push_trimmed(out, text);
                    }
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
    fn pi_command_uses_json_mode_with_model() {
        let provider = PiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Pi,
            model: "pi/anthropic/claude-sonnet-4",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "pi");
        assert_eq!(
            command.args,
            vec![
                "--mode".to_string(),
                "json".to_string(),
                "--model".to_string(),
                "anthropic/claude-sonnet-4".to_string(),
                "do the work".to_string()
            ]
        );
        assert!(command.last_message_path.is_none());
    }

    #[test]
    fn pi_strips_only_leading_qualifier() {
        let provider = PiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Pi,
            model: "pi/openai/gpt-4o",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "hello",
            reasoning_effort: None,
        });

        // Only "pi/" is stripped; the inner "openai/gpt-4o" survives.
        let model_arg = command.args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(command.args[model_arg + 1], "openai/gpt-4o");
    }

    #[test]
    fn pi_injects_thinking_level() {
        let provider = PiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Pi,
            model: "anthropic/claude-sonnet-4",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: Some("high"),
        });

        let thinking_idx = command
            .args
            .iter()
            .position(|a| a == "--thinking")
            .expect("should have --thinking flag");
        assert_eq!(command.args[thinking_idx + 1], "high");
    }

    #[test]
    fn pi_prepends_newline_for_leading_dash_prompt() {
        let provider = PiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Pi,
            model: "anthropic/claude-sonnet-4",
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
    fn pi_collects_text_from_turn_end() {
        let provider = PiProvider;
        let event = json!({
            "type": "turn_end",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Here is the plan"}
                ]
            }
        });
        assert_eq!(
            provider.collect_json_candidates(&event),
            Some(vec!["Here is the plan".to_string()])
        );
    }

    #[test]
    fn pi_ignores_non_assistant_messages() {
        let provider = PiProvider;
        let event = json!({
            "type": "turn_end",
            "message": {
                "role": "tool",
                "content": [{"type": "text", "text": "tool result"}]
            }
        });
        assert_eq!(provider.collect_json_candidates(&event), Some(vec![]));
    }

    #[test]
    fn pi_bypasses_generic_walker_for_intermediate_events() {
        let provider = PiProvider;
        let event = json!({"type": "turn_start"});
        assert_eq!(provider.collect_json_candidates(&event), Some(vec![]));
    }

    #[test]
    fn pi_reports_no_token_usage() {
        let provider = PiProvider;
        let event = json!({"type": "turn_end", "message": {"role": "assistant"}});
        assert!(provider.extract_token_usage(&event).is_none());
    }
}
