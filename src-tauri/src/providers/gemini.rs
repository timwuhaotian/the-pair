use super::Provider;
use crate::provider_adapter::{
    CwdStrategy, InputTransport, OutputTransport, PermissionStrategy, ProviderRuntimeSpec,
    ProviderTurnCommand, ProviderTurnRequest, SessionStrategy,
};
use crate::provider_registry::{resolve_gemini_executable, DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

/// Gemini CLI — dual backend: Antigravity (`agy --print`) or legacy
/// (`gemini --output-format stream-json`).
pub struct GeminiProvider;

impl Provider for GeminiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn executable(&self) -> &str {
        // This is the "default" executable; actual resolution happens in
        // runtime_spec/build_turn_command via resolve_gemini_executable().
        "gemini"
    }

    fn runtime_spec(&self) -> ProviderRuntimeSpec {
        let (executable, is_antigravity) = resolve_gemini_executable();
        ProviderRuntimeSpec {
            executable,
            input_transport: InputTransport::Stdio,
            // Antigravity (`agy --print`) emits the final response as plain text,
            // so it is Stdio, not JsonEvents.
            output_transport: if is_antigravity {
                OutputTransport::Stdio
            } else {
                OutputTransport::JsonEvents
            },
            session_strategy: SessionStrategy::NewFirst,
            // Antigravity runs with --dangerously-skip-permissions.
            permission_strategy: if is_antigravity {
                PermissionStrategy::PreApproved
            } else {
                PermissionStrategy::ManualConfirm
            },
            cwd_strategy: CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        let (executable, is_antigravity) = resolve_gemini_executable();
        // Strip provider prefix if present (e.g. "gemini/model-id" → "model-id").
        let model = request
            .model
            .strip_prefix("gemini/")
            .unwrap_or(request.model);
        ProviderTurnCommand {
            executable,
            args: build_gemini_args(model, request.message, is_antigravity),
            last_message_path: None,
        }
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        let usage = event.get("usageMetadata")?;

        let output_tokens = usage
            .get("candidatesTokenCount")
            .or_else(|| usage.get("output_tokens"))
            .and_then(|v| v.as_u64())?;

        let input_tokens = usage
            .get("promptTokenCount")
            .or_else(|| usage.get("input_tokens"))
            .and_then(|v| v.as_u64());

        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let is_final = event_type == "result" || event_type == "complete" || event_type == "done";

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
        Some(extract_gemini_event_texts(event))
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_gemini()
    }

    fn brand(&self) -> &str {
        "google"
    }

    fn provider_label(&self) -> &str {
        "Gemini CLI"
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
        let (_, is_antigravity) = resolve_gemini_executable();
        Some(if is_antigravity {
            "agy auth".into()
        } else {
            "gemini auth login".into()
        })
    }

    fn install_url(&self) -> Option<String> {
        let (_, is_antigravity) = resolve_gemini_executable();
        Some(if is_antigravity {
            "https://github.com/google-gemini/antigravity".into()
        } else {
            "https://github.com/google-gemini/gemini-cli".into()
        })
    }
}

// ── Gemini-specific helpers ────────────────────────────────────────────────

/// Build the Gemini-provider CLI args for either backend:
/// - **Antigravity (`agy`)** — `--print` emits plain text, `--dangerously-skip-permissions`.
/// - **Legacy `gemini`** — `--output-format stream-json`.
pub fn build_gemini_args(model: &str, message: &str, is_antigravity: bool) -> Vec<String> {
    if is_antigravity {
        // agy uses Go's flag package, which treats an argv element starting with "-"
        // as a flag — prepend a newline to keep the first byte as '\n'.
        let prompt = if message.starts_with('-') {
            format!("\n{}", message)
        } else {
            message.to_string()
        };
        vec![
            "--print-timeout".into(),
            "10m".into(),
            "--dangerously-skip-permissions".into(),
            "--model".into(),
            model.into(),
            "--print".into(),
            prompt,
        ]
    } else {
        vec![
            "--model".into(),
            model.into(),
            "--output-format".into(),
            "stream-json".into(),
            message.into(),
        ]
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

fn collect_text_candidates(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => push_trimmed(out, s),
        Value::Array(items) => {
            for item in items {
                collect_text_candidates(item, out);
            }
        }
        Value::Object(map) => {
            for key in [
                "text", "content", "message", "delta", "part", "parts", "output_text",
                "response", "output",
            ] {
                if let Some(v) = map.get(key) {
                    collect_text_candidates(v, out);
                }
            }
        }
        _ => {}
    }
}

fn extract_gemini_event_texts(event: &Value) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(candidates) = event.get("candidates").and_then(|value| value.as_array()) {
        for candidate in candidates {
            collect_text_candidates(candidate, &mut out);
        }
    }

    if let Some(server_content) = event.get("serverContent") {
        if let Some(model_turn) = server_content.get("modelTurn") {
            collect_text_candidates(model_turn, &mut out);
        } else {
            collect_text_candidates(server_content, &mut out);
        }
    }

    if let Some(model_turn) = event.get("modelTurn") {
        collect_text_candidates(model_turn, &mut out);
    }

    if out.is_empty() {
        collect_text_candidates(event, &mut out);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_legacy_args_use_stream_json() {
        let args = build_gemini_args("gemini-2.5-pro", "explain the current diff", false);
        assert_eq!(
            args,
            vec![
                "--model".to_string(),
                "gemini-2.5-pro".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "explain the current diff".to_string()
            ]
        );
    }

    #[test]
    fn gemini_antigravity_args_use_print_and_skip_permissions() {
        let args =
            build_gemini_args("Gemini 3.5 Flash (Low)", "explain the current diff", true);
        assert_eq!(
            args,
            vec![
                "--print-timeout".to_string(),
                "10m".to_string(),
                "--dangerously-skip-permissions".to_string(),
                "--model".to_string(),
                "Gemini 3.5 Flash (Low)".to_string(),
                "--print".to_string(),
                "explain the current diff".to_string()
            ]
        );
    }

    #[test]
    fn gemini_antigravity_prepends_newline_for_leading_dash_prompt() {
        let args = build_gemini_args("Gemini 3.5 Flash (Low)", "- Do the next step", true);
        assert_eq!(
            args.last().expect("prompt is last"),
            "\n- Do the next step"
        );

        let args = build_gemini_args("Gemini 3.5 Flash (Low)", "Plan the refactor", true);
        assert_eq!(
            args.last().expect("prompt is last"),
            "Plan the refactor"
        );
    }

    #[test]
    fn gemini_command_omits_thinking_budget_flag() {
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
        assert!(!command.args.contains(&"high".to_string()));
    }
}
