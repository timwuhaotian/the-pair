use super::Provider;
use crate::provider_adapter::{ProviderRuntimeSpec, ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

/// Codex (OpenAI Codex CLI) — uses `codex exec` with session-json protocol.
pub struct CodexProvider;

/// Returns true if the model ID starts with `o` followed by a digit (o1, o3, o4, o5, …).
/// Used for reasoning-effort eligibility and provider inference.
fn is_o_series_model(model_id: &str) -> bool {
    model_id
        .strip_prefix('o')
        .and_then(|s| s.chars().next())
        .is_some_and(|c| c.is_ascii_digit())
}

impl Provider for CodexProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Codex
    }

    fn executable(&self) -> &str {
        "codex"
    }

    fn runtime_spec(&self) -> ProviderRuntimeSpec {
        ProviderRuntimeSpec {
            executable: "codex".into(),
            input_transport: crate::provider_adapter::InputTransport::SessionJson,
            output_transport: crate::provider_adapter::OutputTransport::SessionJson,
            session_strategy: crate::provider_adapter::SessionStrategy::ResumeExisting,
            permission_strategy: crate::provider_adapter::PermissionStrategy::Auto,
            cwd_strategy: crate::provider_adapter::CwdStrategy::Worktree,
        }
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        let mut args = vec!["exec".into()];
        if let Some(sid) = request.session_id {
            args.push("resume".into());
            args.push(sid.into());
        }
        // Strip provider prefix if present (e.g. "codex/model-id" → "model-id").
        let model = request
            .model
            .strip_prefix("codex/")
            .unwrap_or(request.model);
        args.push("--model".into());
        args.push(model.into());
        // Sandbox is explicit per role: mentor is read-only (the CLI default),
        // executor needs workspace-write to apply edits in the worktree. It is
        // set through `-c sandbox_mode=` because `codex exec resume` rejects
        // `--sandbox` (verified against codex-cli 0.157.1), and a resume without
        // any sandbox setting would fall back to the user's config.toml.
        let sandbox = if request.role == "mentor" {
            "read-only"
        } else {
            "workspace-write"
        };
        args.push("-c".into());
        args.push(format!("sandbox_mode=\"{}\"", sandbox));
        // `codex exec` removed the `--reasoning-effort` flag. Reasoning is configured
        // via the `model_reasoning_effort` key, injected through `-c`.
        if let Some(effort) = request.reasoning_effort {
            args.push("-c".into());
            args.push(format!("model_reasoning_effort={}", effort));
        }

        let last_message_path = std::env::temp_dir().join(format!(
            "the-pair-{}-{}-{}.txt",
            request.pair_id,
            request.role,
            uuid::Uuid::new_v4()
        ));
        args.push("--json".into());
        args.push("--output-last-message".into());
        args.push(last_message_path.to_string_lossy().into_owned());
        args.push(request.message.into());

        ProviderTurnCommand {
            executable: "codex".into(),
            args,
            last_message_path: Some(last_message_path),
        }
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        let usage = event.get("usage")?;

        let output_tokens = usage
            .get("completion_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| usage.get("output_tokens").and_then(|v| v.as_u64()))?;

        let input_tokens = usage
            .get("prompt_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| usage.get("input_tokens").and_then(|v| v.as_u64()));

        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        // codex exec's terminal event is `turn.completed` (not "result"/"complete"/"done").
        let is_final = matches!(
            event_type,
            "result" | "complete" | "done" | "turn.completed" | "completed"
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
            provider: Some("codex".to_string()),
        })
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        // Only `turn.failed` is terminal; standalone `error` events are also
        // emitted for transient reconnects that the CLI recovers from.
        if event.get("type").and_then(|v| v.as_str()) != Some("turn.failed") {
            return None;
        }
        let raw = event
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Codex turn failed");
        // Upstream API failures arrive as a JSON document inside `message`.
        let nested = serde_json::from_str::<Value>(raw).ok().and_then(|v| {
            v.pointer("/error/message")
                .and_then(|m| m.as_str())
                .map(String::from)
        });
        Some(nested.unwrap_or_else(|| raw.to_string()))
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_codex()
    }

    fn brand(&self) -> &str {
        "openai"
    }

    fn provider_label(&self) -> &str {
        "Codex"
    }

    fn billing_kind(&self) -> &str {
        "plan"
    }

    fn billing_label(&self) -> &str {
        "Included with plan"
    }

    fn access_label(&self, _source_provider_label: &str) -> String {
        "ChatGPT plan".into()
    }

    fn login_command(&self) -> Option<String> {
        // `codex auth` was removed; `codex login` is the current auth entry point
        // (with `--with-api-key` / `--device-auth` variants).
        Some("codex login".into())
    }

    fn install_url(&self) -> Option<String> {
        Some("https://github.com/openai/codex".into())
    }

    fn reasoning_effort_levels(&self, model_id: &str) -> Option<Vec<String>> {
        // codex exec sets reasoning via `-c model_reasoning_effort=<value>`.
        // Reasoning levels per `codex debug models` (verified against codex-cli
        // 0.157.1): every gpt-5.x supports at least low/medium/high/xhigh, and
        // every gpt-6 model (gpt-6-luna, gpt-reserve) additionally supports
        // max. The o<digit> prefix (o1, o3, o4, o5, …) keeps the classic three.
        let id = model_id.strip_prefix("codex/").unwrap_or(model_id);
        if id.starts_with("gpt-6") {
            Some(vec![
                "low".into(),
                "medium".into(),
                "high".into(),
                "xhigh".into(),
                "max".into(),
            ])
        } else if id.starts_with("gpt-5") {
            Some(vec![
                "low".into(),
                "medium".into(),
                "high".into(),
                "xhigh".into(),
            ])
        } else if is_o_series_model(id) {
            Some(vec!["low".into(), "medium".into(), "high".into()])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_resume_command_captures_last_message_file() {
        let provider = CodexProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Codex,
            model: "gpt-4o-mini",
            session_id: Some("session-123"),
            role: "executor",
            pair_id: "pair-1",
            message: "hello world",
            reasoning_effort: None,
        });

        assert_eq!(command.executable, "codex");
        assert_eq!(
            command.args,
            vec![
                "exec".to_string(),
                "resume".to_string(),
                "session-123".to_string(),
                "--model".to_string(),
                "gpt-4o-mini".to_string(),
                "-c".to_string(),
                "sandbox_mode=\"workspace-write\"".to_string(),
                "--json".to_string(),
                "--output-last-message".to_string(),
                command
                    .last_message_path
                    .as_ref()
                    .expect("codex should capture last message")
                    .to_string_lossy()
                    .into_owned(),
                "hello world".to_string()
            ]
        );
    }

    #[test]
    fn codex_command_injects_reasoning_effort_via_config_override() {
        let provider = CodexProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Codex,
            model: "o3",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("medium"),
        });

        assert!(command.args.contains(&"-c".to_string()));
        assert!(command
            .args
            .contains(&"model_reasoning_effort=medium".to_string()));
        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
    }

    #[test]
    fn codex_strips_qualified_prefix_from_model() {
        let provider = CodexProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Codex,
            model: "codex/codex-mini-latest",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        });
        let idx = command.args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(command.args[idx + 1], "codex-mini-latest");
        assert!(!command.args.iter().any(|a| a.starts_with("codex/")));
    }

    #[test]
    fn codex_mentor_sandbox_is_read_only_via_config_override() {
        let provider = CodexProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Codex,
            model: "gpt-5.5",
            session_id: Some("thread_1"),
            role: "mentor",
            pair_id: "pair-1",
            message: "review",
            reasoning_effort: None,
        });
        assert!(command
            .args
            .contains(&"sandbox_mode=\"read-only\"".to_string()));
        assert!(!command.args.contains(&"--sandbox".to_string()));
    }

    #[test]
    fn codex_turn_failed_surfaces_error_message() {
        let provider = CodexProvider;
        let failed = serde_json::json!({
            "type": "turn.failed",
            "error": { "message": "You've hit your usage limit." }
        });
        assert_eq!(
            provider.extract_error_detail(&failed).as_deref(),
            Some("You've hit your usage limit.")
        );

        let nested = serde_json::json!({
            "type": "turn.failed",
            "error": {
                "message": "{\"error\":{\"message\":\"The 'gpt-x' model is not supported.\"}}"
            }
        });
        assert_eq!(
            provider.extract_error_detail(&nested).as_deref(),
            Some("The 'gpt-x' model is not supported.")
        );

        let transient = serde_json::json!({"type": "error", "message": "Reconnecting... 1/5"});
        assert!(provider.extract_error_detail(&transient).is_none());
    }

    #[test]
    fn codex_offers_reasoning_effort_for_gpt5_and_o_series() {
        let provider = CodexProvider;
        assert_eq!(
            provider.reasoning_effort_levels("gpt-5.5").map(|l| l.len()),
            Some(4)
        );
        assert_eq!(
            provider.reasoning_effort_levels("codex/gpt-5.6-terra").map(|l| l.len()),
            Some(4)
        );
        assert_eq!(
            provider
                .reasoning_effort_levels("gpt-6-luna")
                .map(|l| l.len()),
            Some(5)
        );
        assert_eq!(
            provider
                .reasoning_effort_levels("codex/gpt-6-luna")
                .and_then(|l| l.last().cloned()),
            Some("max".to_string())
        );
        assert_eq!(provider.reasoning_effort_levels("o3").map(|l| l.len()), Some(3));
        assert!(provider.reasoning_effort_levels("gpt-4o").is_none());
    }
}
