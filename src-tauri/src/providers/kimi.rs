use super::Provider;
use crate::provider_adapter::{ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::TurnTokenUsage;
use serde_json::Value;

/// Kimi Code CLI (`kimi`) — Moonshot AI's terminal coding agent.
/// Uses `kimi -p` with `--output-format stream-json`: each stdout line is a
/// chat-style JSON message (`role`: "assistant" | "tool" | "meta").
/// Verified against kimi-code 2.0.2 (2026-09-23). The 2.x major release
/// kept the `-p` / `--output-format stream-json` / `--session <id>` /
/// `--model <alias>` surface used by The Pair; argument order around `-p`
/// is strict (the prompt value follows `-p` directly), but the Pair's
/// invocation already follows that order. Flag surface re-checked against
/// kimi-code 2.3.0 `--help` (2026-10-22): same `-p` / `--output-format` /
/// `--session` / `--model` surface. The stream still carries no token usage
/// (`extract_token_usage` stays `None`), and failures still print `error: …`
/// to stderr with exit 1 (last verified live on 2.2.0, 2026-10-15).
///
/// KNOWN LIMITATION: the Kimi mentor is NOT read-only at the CLI level.
/// Prompt mode (`-p`) always uses the auto permission policy, so every tool,
/// including file writes and shell, runs without approval. kimi-code 2.2.0
/// rejects each restricting flag alongside `-p` ("Cannot combine --prompt
/// with --plan." / "--yolo." / "--auto."). Note the 2.2.0 flag meanings:
/// `--yolo` is now "Ask When Needed" (routine edits auto-run, risky actions
/// still ask) and `--auto` is "Never Ask". There is no tool allowlist flag
/// either: `--agent`/`--agent-file` profiles cannot be combined with
/// `--session`. Only the mentor role prompt keeps a Kimi mentor from editing
/// the worktree. (2.2.0 does persist the agent profile across `--session`
/// resume, so an `--agent-file` bound on the first turn would stick — a
/// possible future path to a read-only mentor.)
///
/// Failures are not reported on stdout either. A failed prompt prints an
/// `error: …` line to stderr and exits 1; the only stdout meta events are
/// `system.version`, `session.resume_hint` and the transient
/// `turn.step.retrying`. So there is nothing for `extract_error_detail` to
/// match, and failures are caught by exit status.
///
/// Note: `--session` resume is pinned to the session's original cwd; if a
/// pair's worktree is recreated at a new path the resumed turn fails.
pub struct KimiProvider;

impl Provider for KimiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Kimi
    }

    fn executable(&self) -> &str {
        "kimi"
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        // The stored id is the Pair's routing-qualified form of the catalog
        // row: `kimi/<alias>` where <alias> is the user's config.toml key
        // verbatim. Strip exactly one `kimi/` layer — but only when what
        // remains is itself a known-shaped alias. A bare alias that merely
        // starts with `kimi/` (e.g. config key `kimi/k3` stored legacy-bare)
        // must reach the CLI intact; stripping it makes kimi throw
        // model.not_found. Distinguish by shape: the qualified form always
        // has the provider word repeated or a managed `kimi-code/` segment:
        //   "kimi/kimi-code/k3"  → "kimi-code/k3"   (qualified managed)
        //   "kimi/kimi/k3"       → "kimi/k3"        (qualified user alias)
        //   "kimi-code/k3"       → unchanged        (bare managed)
        //   "kimi/k3"            → unchanged        (bare user alias)
        let model = strip_kimi_qualifier(request.model);
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
        // No mentor-specific flags. `-p` forces the auto permission policy and
        // rejects `--plan`/`--yolo`/`--auto`, so the mentor cannot be made
        // read-only here; see the KNOWN LIMITATION note on `KimiProvider`.
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
        // kimi-code 2.0.2 on 2026-09-23, and against kimi-code 0.42.0
        // previously); token counts stay hidden for this provider.
        None
    }

    fn collect_json_candidates(&self, event: &Value) -> Option<Vec<String>> {
        // Only assistant text is the turn result. Tool results (`role: "tool"`)
        // and the resume hint (`role: "meta"`) must not leak into the message,
        // so the generic text walker is always bypassed.
        //
        // An assistant message that carries `tool_calls` is a mid-turn step:
        // the stream-json writer flushes a step's text together with its tool
        // calls, so that text is narration ("I'll check the tests first.").
        // The turn ends on a step without tool calls, and that step holds the
        // reply.
        let mut out = Vec::new();
        let has_tool_calls = event
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .is_some_and(|calls| !calls.is_empty());
        if event.get("role").and_then(|v| v.as_str()) == Some("assistant") && !has_tool_calls {
            collect_kimi_content(event.get("content"), &mut out);
        }
        Some(out)
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        // A step the runtime is about to retry arrives as a meta event
        // (`{"role":"meta","type":"turn.step.retrying","failed_attempt":…,
        // "error_name":…,"error_message":…,"status_code":…}` — verified in
        // kimi-code 2.2.0's `PromptJsonWriter.writeRetrying`). Surfacing it
        // explains a turn that eventually dies; if a later attempt succeeds,
        // `clears_turn_error` drops it again. Terminal failures still end the
        // process non-zero with `error: <code>: <message>` on stderr.
        if event.get("type").and_then(|v| v.as_str()) != Some("turn.step.retrying") {
            return None;
        }
        let message = event
            .get("error_message")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let name = event
            .get("error_name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let status = event.get("status_code").and_then(|v| v.as_u64());
        let mut detail = match (name, message) {
            (Some(name), Some(message)) => format!("{name}: {message}"),
            (None, Some(message)) => message.to_string(),
            (Some(name), None) => name.to_string(),
            (None, None) => "Kimi Code step failed (retrying)".to_string(),
        };
        if let Some(status) = status {
            detail = format!("{detail} (HTTP {status})");
        }
        Some(detail)
    }

    fn clears_turn_error(&self, event: &Value) -> bool {
        // Any assistant message after a `turn.step.retrying` means a later
        // attempt produced output, so the transient failure is resolved.
        event.get("role").and_then(|v| v.as_str()) == Some("assistant")
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

/// Strip one routing qualifier from a stored kimi model id, never eating
/// into the alias itself. See `build_turn_command` for the shape table.
fn strip_kimi_qualifier(model: &str) -> &str {
    let Some(rest) = model.strip_prefix("kimi/") else {
        return model;
    };
    // Qualified forms: `kimi/kimi…` (the alias itself starts with the provider
    // word) or `kimi/kimi-code…` (managed alias). A lone `kimi/k3` is a bare
    // user alias whose key literally begins `kimi/` — pass it through.
    if rest.starts_with("kimi/") || rest.starts_with("kimi-code") {
        rest
    } else if model.matches('/').count() >= 2 {
        // `kimi/<provider>/<model>` shapes (e.g. a future managed route):
        // more than one slash means the first segment was the qualifier.
        rest
    } else {
        model
    }
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
    fn kimi_surfaces_retrying_errors_and_clears_them_on_recovery() {
        // Shape from kimi-code 2.2.0's `PromptJsonWriter.writeRetrying`:
        // a failed step the runtime is about to retry.
        let provider = KimiProvider;
        let retrying = json!({
            "role": "meta",
            "type": "turn.step.retrying",
            "failed_attempt": 1,
            "next_attempt": 2,
            "max_attempts": 3,
            "delay_ms": 500,
            "error_name": "RateLimitError",
            "error_message": "too many requests",
            "status_code": 429
        });
        assert_eq!(
            provider.extract_error_detail(&retrying).as_deref(),
            Some("RateLimitError: too many requests (HTTP 429)")
        );

        // A later assistant message means a retry produced output.
        let recovered = json!({"role": "assistant", "content": "done"});
        assert!(provider.clears_turn_error(&recovered));
        assert!(provider.extract_error_detail(&recovered).is_none());
        // The retrying event itself and tool traffic don't clear the error.
        assert!(!provider.clears_turn_error(&retrying));
        assert!(!provider.clears_turn_error(&json!({"role": "tool", "content": "x"})));

        // Fields degrade gracefully when the writer omits some.
        let bare = json!({"role": "meta", "type": "turn.step.retrying"});
        assert_eq!(
            provider.extract_error_detail(&bare).as_deref(),
            Some("Kimi Code step failed (retrying)")
        );
        let message_only = json!({
            "role": "meta", "type": "turn.step.retrying", "error_message": "boom"
        });
        assert_eq!(
            provider.extract_error_detail(&message_only).as_deref(),
            Some("boom")
        );
        let name_only = json!({
            "role": "meta", "type": "turn.step.retrying", "error_name": "TimeoutError"
        });
        assert_eq!(
            provider.extract_error_detail(&name_only).as_deref(),
            Some("TimeoutError")
        );
    }

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
    fn kimi_bare_alias_starting_with_kimi_prefix_is_not_stripped() {
        // A user alias literally named `kimi/k3` in config.toml, stored
        // legacy-bare, must reach the CLI intact (kimi throws model.not_found
        // for the stripped `k3`).
        let provider = KimiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kimi,
            model: "kimi/k3",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        let idx = command.args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(command.args[idx + 1], "kimi/k3");
    }

    #[test]
    fn kimi_qualified_user_alias_strips_exactly_one_layer() {
        // Catalog alias `kimi/k3` is stored qualified as `kimi/kimi/k3`.
        let provider = KimiProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Kimi,
            model: "kimi/kimi/k3",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        let idx = command.args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(command.args[idx + 1], "kimi/k3");
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

        // Narration flushed together with a step's tool calls is not the reply.
        let narrated_tool_call = json!({
            "role": "assistant",
            "content": "I'll check the tests first.",
            "tool_calls": [{"type": "function", "id": "call_2",
                "function": {"name": "Bash", "arguments": "{\"command\":\"npm test\"}"}}]
        });
        assert_eq!(
            provider.collect_json_candidates(&narrated_tool_call),
            Some(vec![])
        );

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
