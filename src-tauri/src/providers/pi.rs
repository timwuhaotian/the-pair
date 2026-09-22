use super::Provider;
use crate::provider_adapter::{ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

/// Pi Agent CLI (`pi`) — earendil-works/pi terminal coding agent.
/// Uses `pi --mode json` to emit structured JSON event lines on stdout.
/// Pi is a BYOK multi-provider agent: model ids are `provider/model` format
/// (e.g. `anthropic/claude-sonnet-4`). The `pi/` qualifier added by The Pair
/// is stripped at spawn time so Pi receives the inner `provider/model` value.
///
/// Event shapes verified against pi 0.79.2 (2026-09-23): the stream opens with
/// a `{"type":"session","id":…}` header, assistant messages carry
/// `usage.{input,output}`, and the closing `agent_end` repeats every message
/// of the turn in `messages`.
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
        // Guard leading-dash prompts so they aren't parsed as flags, and
        // leading-`@` prompts so they aren't read as `@file` attachments.
        let prompt = if request.message.starts_with('-') || request.message.starts_with('@') {
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

        // `--session-id` resumes the session captured from the header event,
        // creating it if missing.
        if let Some(sid) = request.session_id {
            args.push("--session-id".into());
            args.push(sid.into());
        }

        // Pi has no sandbox; its documented read-only mode is a tool allowlist.
        if request.role == "mentor" {
            args.push("--tools".into());
            args.push("read,grep,find,ls".into());
        }

        args.push(prompt);

        ProviderTurnCommand {
            executable: "pi".into(),
            args,
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        // `message_end` reports one assistant message (live); `agent_end`
        // repeats the whole turn, so its assistant usages sum to the final count.
        let (messages, is_final): (Vec<&Value>, bool) =
            match event.get("type").and_then(|v| v.as_str())? {
                "message_end" => (event.get("message").into_iter().collect(), false),
                "agent_end" => (event.get("messages")?.as_array()?.iter().collect(), true),
                _ => return None,
            };
        let usages: Vec<&Value> = messages
            .into_iter()
            .filter(|msg| is_assistant(msg))
            .filter_map(|msg| msg.get("usage"))
            .collect();
        if usages.is_empty() {
            return None;
        }
        let sum = |key: &str| {
            usages
                .iter()
                .filter_map(|usage| usage.get(key).and_then(|v| v.as_u64()))
                .sum::<u64>()
        };

        Some(TurnTokenUsage {
            output_tokens: sum("output"),
            input_tokens: Some(sum("input")),
            last_updated_at: crate::util::now_millis(),
            source: if is_final {
                TokenUsageSource::Final
            } else {
                TokenUsageSource::Live
            },
            provider: Some("pi".to_string()),
        })
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // The reply is the last assistant message of the closing `agent_end`,
        // which is exactly what `pi -p` prints. Earlier `message_end` /
        // `turn_end` events hold the same messages again, so reading them too
        // would duplicate the answer; every other event bypasses the walker.
        let mut out = Vec::new();
        if let Some(last) = final_assistant_message(event) {
            collect_pi_content(last.get("content"), &mut out);
        }
        Some(out)
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        // Pi exits 0 in JSON mode even when the request failed; the failure is
        // only recorded as `stopReason` on the last assistant message.
        let last = final_assistant_message(event)?;
        let reason = last.get("stopReason").and_then(|v| v.as_str())?;
        if reason != "error" && reason != "aborted" {
            return None;
        }
        let detail = last
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("pi request {reason}"));
        Some(detail)
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
        // The base set is `off, minimal, low, medium, high, xhigh` (verified
        // against pi 0.79.2 on 2026-09-19). `max` is an opt-in level added in
        // pi 0.80.6 and is only valid on GPT-5.6 / adaptive Claude models. We
        // expose it only when the installed `pi --help` advertises it, so
        // picking "max" never silently degrades a turn on a too-old CLI.
        let base = vec![
            "off".to_string(),
            "minimal".to_string(),
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ];
        let mut levels = base;
        if crate::provider_registry::pi_supports_max_thinking_level() {
            levels.push("max".to_string());
        }
        Some(levels)
    }

    fn install_url(&self) -> Option<String> {
        Some("https://pi.dev".into())
    }
}

// ── Pi-specific helpers ───────────────────────────────────────────────────

fn is_assistant(message: &Value) -> bool {
    message.get("role").and_then(|v| v.as_str()) == Some("assistant")
}

/// The last assistant message of a terminal `agent_end` event. An `agent_end`
/// flagged `willRetry` is superseded by the retry's own `agent_end`.
fn final_assistant_message(event: &Value) -> Option<&Value> {
    if event.get("type").and_then(|v| v.as_str()) != Some("agent_end")
        || event.get("willRetry").and_then(|v| v.as_bool()) == Some(true)
    {
        return None;
    }
    event
        .get("messages")?
        .as_array()?
        .iter()
        .rev()
        .find(|message| is_assistant(message))
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
    fn pi_collects_only_the_final_assistant_message_from_agent_end() {
        let provider = PiProvider;
        let turn_end = json!({
            "type": "turn_end",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "Reading files"}]}
        });
        assert_eq!(provider.collect_json_candidates(&turn_end), Some(vec![]));

        let agent_end = json!({
            "type": "agent_end",
            "messages": [
                {"role": "user", "content": "plan it"},
                {"role": "assistant", "content": [{"type": "text", "text": "Reading files"}]},
                {"role": "toolResult", "content": [{"type": "text", "text": "tool result"}]},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "hmm"},
                    {"type": "text", "text": "Here is the plan"}
                ]}
            ]
        });
        assert_eq!(
            provider.collect_json_candidates(&agent_end),
            Some(vec!["Here is the plan".to_string()])
        );
    }

    #[test]
    fn pi_bypasses_generic_walker_for_intermediate_events() {
        let provider = PiProvider;
        let event = json!({"type": "turn_start"});
        assert_eq!(provider.collect_json_candidates(&event), Some(vec![]));
    }

    #[test]
    fn pi_reads_live_and_final_token_usage() {
        let provider = PiProvider;
        let usage = |input: u64, output: u64| {
            json!({"input": input, "output": output, "cacheRead": 0, "cacheWrite": 0})
        };

        let message_end = json!({
            "type": "message_end",
            "message": {"role": "assistant", "usage": usage(46958, 3)}
        });
        let live = provider.extract_token_usage(&message_end).expect("live usage");
        assert_eq!(live.output_tokens, 3);
        assert_eq!(live.input_tokens, Some(46958));
        assert!(matches!(live.source, TokenUsageSource::Live));

        let agent_end = json!({
            "type": "agent_end",
            "messages": [
                {"role": "user", "content": "go"},
                {"role": "assistant", "usage": usage(100, 10)},
                {"role": "assistant", "usage": usage(200, 20)}
            ]
        });
        let final_usage = provider.extract_token_usage(&agent_end).expect("final usage");
        assert_eq!(final_usage.output_tokens, 30);
        assert_eq!(final_usage.input_tokens, Some(300));
        assert!(matches!(final_usage.source, TokenUsageSource::Final));

        let turn_start = json!({"type": "turn_start"});
        assert!(provider.extract_token_usage(&turn_start).is_none());
    }

    #[test]
    fn pi_surfaces_errors_recorded_on_the_final_message() {
        let provider = PiProvider;
        let failed = json!({
            "type": "agent_end",
            "willRetry": false,
            "messages": [{
                "role": "assistant",
                "content": [],
                "stopReason": "error",
                "errorMessage": "No API key for provider: anthropic"
            }]
        });
        assert_eq!(
            provider.extract_error_detail(&failed).as_deref(),
            Some("No API key for provider: anthropic")
        );

        let retrying = json!({
            "type": "agent_end",
            "willRetry": true,
            "messages": [{"role": "assistant", "stopReason": "error", "errorMessage": "rate limited"}]
        });
        assert!(provider.extract_error_detail(&retrying).is_none());

        let ok = json!({
            "type": "agent_end",
            "messages": [{"role": "assistant", "stopReason": "stop"}]
        });
        assert!(provider.extract_error_detail(&ok).is_none());
    }

    #[test]
    fn pi_resumes_session_and_restricts_mentor_tools() {
        let provider = PiProvider;
        let mentor = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Pi,
            model: "anthropic/claude-sonnet-4",
            session_id: Some("0199a1b2-c3d4"),
            role: "mentor",
            pair_id: "pair-1",
            message: "@review the diff",
            reasoning_effort: None,
        });
        let sid = mentor.args.iter().position(|a| a == "--session-id").unwrap();
        assert_eq!(mentor.args[sid + 1], "0199a1b2-c3d4");
        let tools = mentor.args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(mentor.args[tools + 1], "read,grep,find,ls");
        assert_eq!(mentor.args.last().unwrap(), "\n@review the diff");

        let executor = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Pi,
            model: "anthropic/claude-sonnet-4",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do it",
            reasoning_effort: None,
        });
        assert!(!executor.args.contains(&"--tools".to_string()));
        assert!(!executor.args.contains(&"--session-id".to_string()));
    }

    #[test]
    fn pi_base_thinking_levels_omit_max() {
        // The picker's base set must always include the six core levels (off
        // through xhigh). `max` is added by the runtime probe only on pi
        // 0.80.6+; this test pins the deterministic floor regardless of the
        // installed CLI.
        let provider = PiProvider;
        let levels = provider
            .reasoning_effort_levels("anthropic/claude-sonnet-4")
            .expect("pi always offers a thinking axis");
        for level in ["off", "minimal", "low", "medium", "high", "xhigh"] {
            assert!(
                levels.iter().any(|entry| entry == level),
                "pi thinking levels should include {level}; got {levels:?}"
            );
        }
    }
}
