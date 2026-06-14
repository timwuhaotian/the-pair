use super::Provider;
use crate::provider_adapter::{ProviderRuntimeSpec, ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

/// Codex (OpenAI Codex CLI) — uses `codex exec` with session-json protocol.
pub struct CodexProvider;

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
        // executor needs workspace-write to apply edits in the worktree.
        args.push("--sandbox".into());
        if request.role == "mentor" {
            args.push("read-only".into());
        } else {
            args.push("workspace-write".into());
        }
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

    fn reasoning_effort_levels(&self, model_id: &str) -> Option<Vec<String>> {
        // codex exec sets reasoning via `-c model_reasoning_effort=<value>`.
        if model_id.starts_with("o3") || model_id.starts_with("o4") || model_id.starts_with("o1")
        {
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
                "--sandbox".to_string(),
                "workspace-write".to_string(),
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
}
