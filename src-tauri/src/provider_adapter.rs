use crate::provider_registry::ProviderKind;
use std::path::PathBuf;

// ── Transport & strategy enums ────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputTransport {
    Stdio,
    JsonEvents,
    SessionJson,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTransport {
    Stdio,
    JsonEvents,
    SessionJson,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStrategy {
    NewFirst,
    ResumeExisting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionStrategy {
    Auto,
    /// Legacy strategy used by the sunset Gemini CLI.  Kept for enum completeness;
    /// the Antigravity (`agy`) backend always uses PreApproved.
    #[allow(dead_code)]
    ManualConfirm,
    PreApproved,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CwdStrategy {
    Worktree,
    OriginalRepo,
    Custom,
}

// ── Request / response structs ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRuntimeSpec {
    pub executable: String,
    pub input_transport: InputTransport,
    pub output_transport: OutputTransport,
    pub session_strategy: SessionStrategy,
    pub permission_strategy: PermissionStrategy,
    pub cwd_strategy: CwdStrategy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnRequest<'a> {
    pub provider_kind: ProviderKind,
    pub model: &'a str,
    pub session_id: Option<&'a str>,
    pub role: &'a str,
    pub pair_id: &'a str,
    pub message: &'a str,
    pub reasoning_effort: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnCommand {
    pub executable: String,
    pub args: Vec<String>,
    pub last_message_path: Option<PathBuf>,
}

// ── Facade — delegates to the `Provider` trait via the providers module ───

/// Legacy facade kept for backward compatibility. New code should use
/// `crate::providers::provider_for_kind(kind)` directly.
pub struct ProviderAdapter;

impl ProviderAdapter {
    pub fn runtime_spec(kind: ProviderKind) -> Result<ProviderRuntimeSpec, String> {
        let provider = crate::providers::provider_for_kind(kind)
            .ok_or_else(|| format!("No provider registered for {:?}", kind))?;
        Ok(provider.runtime_spec())
    }

    pub fn build_turn_command(
        request: ProviderTurnRequest<'_>,
    ) -> Result<ProviderTurnCommand, String> {
        let provider = crate::providers::provider_for_kind(request.provider_kind)
            .ok_or_else(|| format!("No provider registered for {:?}", request.provider_kind))?;
        Ok(provider.build_turn_command(&request))
    }

    pub fn infer_provider_kind(model: &str) -> ProviderKind {
        // Antigravity (`agy`) model ids are display names like "Gemini 3.5 Flash (Low)"
        // (capitalized), so the keyword checks below are case-insensitive to route them
        // correctly alongside the lowercase canonical ids.
        if model.starts_with("opencode") || model.contains("/") {
            let parts: Vec<&str> = model.split('/').collect();
            if parts.len() >= 2 {
                return match parts[0].to_ascii_lowercase().as_str() {
                    "codex" => ProviderKind::Codex,
                    "claude" => ProviderKind::Claude,
                    "gemini" => ProviderKind::Gemini,
                    "kimi" => ProviderKind::Kimi,
                    _ => ProviderKind::Opencode,
                };
            }

            ProviderKind::Opencode
        } else {
            let lower = model.to_ascii_lowercase();
            if lower.contains("claude") {
                ProviderKind::Claude
            } else if lower.contains("gemini") {
                ProviderKind::Gemini
            } else if lower.contains("kimi") {
                ProviderKind::Kimi
            } else if lower.contains("gpt")
                || lower
                    .strip_prefix('o')
                    .and_then(|s| s.chars().next())
                    .is_some_and(|c| c.is_ascii_digit())
            {
                ProviderKind::Codex
            } else {
                ProviderKind::Opencode
            }
        }
    }

    pub fn read_last_message_file(path: &PathBuf) -> Option<String> {
        let content = std::fs::read_to_string(path).ok()?;
        let trimmed = content.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_resume_command_captures_last_message_file() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Codex,
            model: "gpt-4o-mini",
            session_id: Some("session-123"),
            role: "executor",
            pair_id: "pair-1",
            message: "hello world",
            reasoning_effort: None,
        })
        .unwrap();

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
    fn claude_command_uses_stream_json_and_resume_flags() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Claude,
            model: "sonnet",
            session_id: Some("claude-session"),
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: None,
        })
        .unwrap();

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
    fn gemini_mentor_args_use_plan_mode() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "Gemini 3.5 Flash (Low)",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "explain the current diff",
            reasoning_effort: None,
        })
        .unwrap();

        assert_eq!(command.executable, "agy");
        assert!(command.args.contains(&"--mode".to_string()));
        assert!(command.args.contains(&"plan".to_string()));
        assert!(!command
            .args
            .contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn gemini_executor_args_use_accept_edits_and_skip_permissions() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "Gemini 3.5 Flash (Low)",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "explain the current diff",
            reasoning_effort: None,
        })
        .unwrap();

        assert_eq!(command.executable, "agy");
        assert!(command.args.contains(&"--mode".to_string()));
        assert!(command.args.contains(&"accept-edits".to_string()));
        assert!(command
            .args
            .contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn gemini_agy_prepends_newline_for_leading_dash_prompt() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "Gemini 3.5 Flash (Low)",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "- Do the next step",
            reasoning_effort: None,
        })
        .unwrap();

        assert_eq!(
            command.args.last().expect("prompt is last"),
            "\n- Do the next step"
        );

        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "Gemini 3.5 Flash (Low)",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "Plan the refactor",
            reasoning_effort: None,
        })
        .unwrap();

        assert_eq!(
            command.args.last().expect("prompt is last"),
            "Plan the refactor"
        );
    }

    #[test]
    fn inference_recognizes_future_o_series_models() {
        // o4 and beyond should infer to Codex, matching the frontend /^o\d/ regex
        // and is_codex_model_id() predicate.
        assert_eq!(
            ProviderAdapter::infer_provider_kind("o4"),
            ProviderKind::Codex
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("o4-mini"),
            ProviderKind::Codex
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("o5"),
            ProviderKind::Codex
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("o9-preview"),
            ProviderKind::Codex
        );
        // Non-digit o-prefix should NOT match (e.g. "openai")
        assert_eq!(
            ProviderAdapter::infer_provider_kind("openai"),
            ProviderKind::Opencode
        );
    }

    #[test]
    fn inference_keeps_legacy_string_heuristics_for_snapshot_fallbacks() {
        assert_eq!(
            ProviderAdapter::infer_provider_kind("codex/gpt-4o-mini"),
            ProviderKind::Codex
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("claude-3-5-sonnet"),
            ProviderKind::Claude
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("gemini-2.5-pro"),
            ProviderKind::Gemini
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("Gemini 3.5 Flash (Low)"),
            ProviderKind::Gemini
        );
        // Kimi aliases contain their own slashes; only the leading qualifier routes.
        assert_eq!(
            ProviderAdapter::infer_provider_kind("kimi/kimi-code/k3"),
            ProviderKind::Kimi
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("kimi-k2.5"),
            ProviderKind::Kimi
        );
    }

    #[test]
    fn claude_command_omits_reasoning_effort_flag() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Claude,
            model: "sonnet",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("high"),
        })
        .unwrap();

        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
        assert!(!command.args.contains(&"high".to_string()));
    }

    #[test]
    fn gemini_command_omits_thinking_budget_flag() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Gemini,
            model: "gemini-2.5-pro",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("high"),
        })
        .unwrap();

        assert!(!command.args.contains(&"--thinking-budget".to_string()));
        assert!(!command.args.contains(&"32768".to_string()));
        assert!(!command.args.contains(&"high".to_string()));
    }

    #[test]
    fn codex_command_injects_reasoning_effort_via_config_override() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Codex,
            model: "o3",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("medium"),
        })
        .unwrap();

        assert!(command.args.contains(&"-c".to_string()));
        assert!(command
            .args
            .contains(&"model_reasoning_effort=medium".to_string()));
        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
    }

    #[test]
    fn opencode_command_omits_unsupported_reasoning_effort() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model: "example/model",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("high"),
        })
        .unwrap();

        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
        assert!(!command.args.contains(&"high".to_string()));
    }

    #[test]
    fn opencode_command_injects_supported_reasoning_variant() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model: "minimax/MiniMax-M3",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("adaptive"),
        })
        .unwrap();

        assert!(command.args.contains(&"--variant".to_string()));
        assert!(command.args.contains(&"adaptive".to_string()));
    }
}
