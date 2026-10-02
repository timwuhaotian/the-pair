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
        // The Pair runs a pair in the user's own directory by default, and that
        // directory is not validated as a git repo. Without this flag codex
        // refuses to start a turn anywhere outside a git work tree:
        // "Not inside a trusted directory and --skip-git-repo-check was not
        // specified." (exit 1, on stderr, before any JSON event). The flag is
        // `global`, so it is accepted by `exec resume` too. Re-checked against
        // codex-cli 0.149.1 and 0.160.0.
        args.push("--skip-git-repo-check".into());
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
        // `--sandbox`, and a resume without any sandbox setting would fall back
        // to the user's config.toml. Re-checked against codex-cli 0.149.1
        // (2026-09-30): `codex exec resume --help` still offers `-c`/`--model`/
        // `--json`/`--output-last-message` but no `--sandbox`.
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
        // `prompt` is declared as a bare `Option<String>` positional, so clap
        // reads a leading `-` as a flag and aborts before the turn starts
        // ("error: unexpected argument '- ' found"). Handoff text routinely
        // opens with a markdown bullet or a `---` diff header, so separate the
        // message explicitly.
        args.push("--".into());
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
        // A standalone `error` event is critical, not a transient reconnect —
        // but the CLI always re-surfaces it on the following `turn.failed`, so
        // parsing only the terminal event reports it exactly once.
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
        // The ladder is per-model and moves with the catalog, so ask the CLI
        // (see `codex_reasoning_levels`) and only guess when it can't answer.
        crate::provider_registry::codex_reasoning_levels(model_id)
            .or_else(|| fallback_reasoning_levels(model_id))
    }
}

/// Ladder used when the installed `codex` can't be asked (too old for
/// `debug models`, or a model absent from its catalog).
///
/// Deliberately conservative: offering a level the model rejects fails the
/// turn, while omitting one the model supports only narrows the picker. It
/// therefore stops at `xhigh` rather than guessing at `max`/`ultra`, which
/// only some catalog entries carry.
fn fallback_reasoning_levels(model_id: &str) -> Option<Vec<String>> {
    let id = model_id.strip_prefix("codex/").unwrap_or(model_id);
    if id.starts_with("gpt-5") || id.starts_with("gpt-6") {
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
                "--skip-git-repo-check".to_string(),
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
                "--".to_string(),
                "hello world".to_string()
            ]
        );
    }

    #[test]
    fn codex_prompt_starting_with_dash_is_separated_from_flags() {
        // A handoff message routinely opens with a markdown bullet or a `---`
        // diff header; as a bare positional clap reads the leading `-` as a
        // flag and aborts the turn before it starts.
        let provider = CodexProvider;
        let command = provider.build_turn_command(&ProviderTurnRequest {
            provider_kind: ProviderKind::Codex,
            model: "gpt-4o-mini",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "- Fix A\n- Fix B",
            reasoning_effort: None,
        });

        let sep = command.args.iter().position(|a| a == "--").unwrap();
        assert_eq!(
            command.args[sep + 1],
            "- Fix A\n- Fix B",
            "message must follow the `--` separator"
        );
    }

    #[test]
    fn codex_skips_the_git_repo_check() {
        // Pairs default to the user's own directory, which is not required to
        // be a git repo; without this codex exits 1 before emitting any event.
        for role in ["mentor", "executor"] {
            let provider = CodexProvider;
            let command = provider.build_turn_command(&ProviderTurnRequest {
                provider_kind: ProviderKind::Codex,
                model: "gpt-4o-mini",
                session_id: None,
                role,
                pair_id: "pair-1",
                message: "hello",
                reasoning_effort: None,
            });
            assert!(command.args.contains(&"--skip-git-repo-check".to_string()));
        }
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
    fn codex_fallback_ladder_is_conservative_and_prefix_based() {
        // The fallback only runs when the CLI can't be asked, so it stops at
        // xhigh rather than guessing at max/ultra.
        for model in ["gpt-5.5", "gpt-5.6-terra", "gpt-6-luna", "codex/gpt-5.6-sol"] {
            assert_eq!(
                fallback_reasoning_levels(model),
                Some(
                    ["low", "medium", "high", "xhigh"]
                        .map(String::from)
                        .to_vec()
                ),
                "fallback ladder for {model}"
            );
        }
        assert_eq!(
            fallback_reasoning_levels("o3"),
            Some(["low", "medium", "high"].map(String::from).to_vec())
        );
        assert!(fallback_reasoning_levels("gpt-4o").is_none());
        assert!(fallback_reasoning_levels("codex-ultra-latest").is_none());
    }

    #[test]
    fn codex_reasoning_levels_resolve_to_a_real_ladder() {
        // Whether a `codex` is installed decides the answer, so assert the
        // shape rather than an exact length: an installed CLI supplies its
        // catalog, and without one the prefix fallback covers gpt-5* anyway.
        let levels = CodexProvider
            .reasoning_effort_levels("gpt-5.6-terra")
            .expect("gpt-5.6-terra must resolve to a ladder");
        assert!(!levels.is_empty());
        assert!(levels.iter().all(|level| !level.is_empty()));
        assert!(levels.contains(&"low".to_string()));
    }
}
