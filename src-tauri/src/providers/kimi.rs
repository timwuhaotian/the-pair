use super::Provider;
use crate::provider_adapter::{ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::TurnTokenUsage;
use serde_json::Value;

/// Kimi Code CLI (`kimi`) — Moonshot AI's terminal coding agent.
/// Uses `kimi -p` with `--output-format stream-json`: each stdout line is a
/// chat-style JSON message (`role`: "assistant" | "tool" | "meta").
pub struct KimiProvider;

impl Provider for KimiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Kimi
    }

    fn executable(&self) -> &str {
        "kimi"
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // Strip provider prefix if present (e.g. "kimi/kimi-code/k3" → "kimi-code/k3").
        // Kimi model aliases themselves contain slashes, so only the leading
        // "kimi/" qualifier is removed.
        let model = request.model.strip_prefix("kimi/").unwrap_or(request.model);
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
            "stream-json".into(),
            "--model".into(),
            model.into(),
        ];
        // NOTE: `-p` implies the auto permission policy; `--yolo`, `--auto` and
        // `--plan` are all rejected alongside it. Mentor read-only therefore
        // relies on the role prompt, matching the OpenCode provider.
        if let Some(sid) = request.session_id {
            args.push("--session".into());
            args.push(sid.into());
        }

        ProviderTurnCommand {
            executable: "kimi".into(),
            args,
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, _event: &Value) -> Option<TurnTokenUsage> {
        // Kimi's stream-json messages carry no usage data (verified against
        // kimi-code 0.29.1); token counts stay hidden for this provider.
        None
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // Only assistant text is the turn result. Tool results (`role: "tool"`)
        // and the resume hint (`role: "meta"`) must not leak into the message,
        // so the generic text walker is always bypassed.
        let mut out = Vec::new();
        if event.get("role").and_then(|v| v.as_str()) == Some("assistant") {
            collect_kimi_content(event.get("content"), &mut out);
        }
        Some(out)
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_kimi()
    }

    fn brand(&self) -> &str {
        "kimi"
    }

    fn provider_label(&self) -> &str {
        "Kimi Code"
    }

    fn billing_kind(&self) -> &str {
        // Like OpenCode, Kimi Code is a gateway: model aliases resolve to
        // user-configured providers (Kimi plan OAuth or third-party API keys).
        "byok"
    }

    fn billing_label(&self) -> &str {
        "Pay as you go"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "Kimi Code login".into()
    }

    fn login_command(&self) -> Option<String> {
        Some("kimi login".into())
    }

    fn install_url(&self) -> Option<String> {
        Some("https://github.com/MoonshotAI/kimi-code".into())
    }
}

// ── Kimi-specific helpers ──────────────────────────────────────────────────

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
fn collect_kimi_content(content: Option<&Value>, out: &mut Vec<String>) {
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
    fn kimi_command_uses_print_mode_with_stream_json() {
        let provider = KimiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kimi,
            model: "kimi/kimi-code/k3",
            session_id: Some("session_abc"),
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "kimi");
        assert_eq!(
            command.args,
            vec![
                "-p".to_string(),
                "do the work".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--model".to_string(),
                "kimi-code/k3".to_string(),
                "--session".to_string(),
                "session_abc".to_string()
            ]
        );
        assert!(command.last_message_path.is_none());
    }

    #[test]
    fn kimi_mentor_command_has_no_permission_or_plan_flags() {
        // `-p` rejects --yolo/--auto/--plan; mentor read-only is prompt-enforced.
        let provider = KimiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kimi,
            model: "kimi-code/kimi-for-coding",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: None,
        });

        for flag in ["--plan", "--yolo", "--auto", "--permission-mode"] {
            assert!(
                !command.args.contains(&flag.to_string()),
                "kimi -p must not receive {}",
                flag
            );
        }
        assert!(!command.args.contains(&"--session".to_string()));
    }

    #[test]
    fn kimi_prepends_newline_for_leading_dash_prompt() {
        let provider = KimiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kimi,
            model: "kimi-code/k3",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "- Do the next step",
            reasoning_effort: None,
        });

        assert_eq!(command.args[1], "\n- Do the next step");
    }

    #[test]
    fn kimi_command_omits_reasoning_effort_flag() {
        let provider = KimiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kimi,
            model: "kimi-code/k3",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("high"),
        });

        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
        assert!(!command.args.contains(&"high".to_string()));
    }

    #[test]
    fn kimi_collects_only_assistant_text() {
        let provider = KimiProvider;

        let assistant = json!({"role": "assistant", "content": "done"});
        assert_eq!(
            provider.collect_json_candidates(&assistant),
            Some(vec!["done".to_string()])
        );

        let tool_call = json!({
            "role": "assistant",
            "tool_calls": [{"type": "function", "id": "call_1",
                "function": {"name": "Bash", "arguments": "{\"command\":\"ls\"}"}}]
        });
        assert_eq!(provider.collect_json_candidates(&tool_call), Some(vec![]));

        let tool_result = json!({"role": "tool", "tool_call_id": "call_1", "content": "file.txt"});
        assert_eq!(provider.collect_json_candidates(&tool_result), Some(vec![]));

        let meta = json!({
            "role": "meta",
            "type": "session.resume_hint",
            "session_id": "session_abc",
            "content": "To resume this session: kimi -r session_abc"
        });
        assert_eq!(provider.collect_json_candidates(&meta), Some(vec![]));
    }

    #[test]
    fn kimi_collects_text_blocks_from_content_arrays() {
        let provider = KimiProvider;
        let event = json!({
            "role": "assistant",
            "content": [{"type": "text", "text": "part one"}, {"type": "text", "text": "part two"}]
        });
        assert_eq!(
            provider.collect_json_candidates(&event),
            Some(vec!["part one".to_string(), "part two".to_string()])
        );
    }

    #[test]
    fn kimi_reports_no_token_usage() {
        let provider = KimiProvider;
        let event = json!({"role": "assistant", "content": "done"});
        assert!(provider.extract_token_usage(&event).is_none());
    }
}
