use crate::provider_registry::ProviderKind;
use std::path::PathBuf;

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

pub struct ProviderAdapter;

impl ProviderAdapter {
    pub fn runtime_spec(kind: ProviderKind) -> ProviderRuntimeSpec {
        match kind {
            ProviderKind::Opencode => ProviderRuntimeSpec {
                executable: "opencode".into(),
                input_transport: InputTransport::Stdio,
                output_transport: OutputTransport::JsonEvents,
                session_strategy: SessionStrategy::NewFirst,
                permission_strategy: PermissionStrategy::Auto,
                cwd_strategy: CwdStrategy::Worktree,
            },
            ProviderKind::Codex => ProviderRuntimeSpec {
                executable: "codex".into(),
                input_transport: InputTransport::SessionJson,
                output_transport: OutputTransport::SessionJson,
                session_strategy: SessionStrategy::ResumeExisting,
                permission_strategy: PermissionStrategy::Auto,
                cwd_strategy: CwdStrategy::Worktree,
            },
            ProviderKind::Claude => ProviderRuntimeSpec {
                executable: "claude".into(),
                input_transport: InputTransport::Stdio,
                output_transport: OutputTransport::JsonEvents,
                session_strategy: SessionStrategy::ResumeExisting,
                permission_strategy: PermissionStrategy::PreApproved,
                cwd_strategy: CwdStrategy::Worktree,
            },
            ProviderKind::Gemini => {
                let (executable, is_antigravity) =
                    crate::provider_registry::resolve_gemini_executable();
                ProviderRuntimeSpec {
                    executable,
                    input_transport: InputTransport::Stdio,
                    // Antigravity (`agy --print`) emits the final response as plain text,
                    // so it is Stdio, not JsonEvents. It also runs with
                    // --dangerously-skip-permissions (no approval prompts), so the
                    // permission strategy is PreApproved, not ManualConfirm. Legacy
                    // `gemini --output-format stream-json` keeps the original values.
                    output_transport: if is_antigravity {
                        OutputTransport::Stdio
                    } else {
                        OutputTransport::JsonEvents
                    },
                    session_strategy: SessionStrategy::NewFirst,
                    permission_strategy: if is_antigravity {
                        PermissionStrategy::PreApproved
                    } else {
                        PermissionStrategy::ManualConfirm
                    },
                    cwd_strategy: CwdStrategy::Worktree,
                }
            }
        }
    }

    pub fn build_turn_command(request: ProviderTurnRequest<'_>) -> ProviderTurnCommand {
        match request.provider_kind {
            ProviderKind::Opencode => {
                let mut args = vec!["run".into(), "--model".into(), request.model.into()];
                if let Some(sid) = request.session_id {
                    args.push("--session".into());
                    args.push(sid.into());
                }
                args.push("--format".into());
                args.push("json".into());
                args.push(request.message.into());

                ProviderTurnCommand {
                    executable: "opencode".into(),
                    args,
                    last_message_path: None,
                }
            }
            ProviderKind::Codex => {
                let mut args = vec!["exec".into()];
                if let Some(sid) = request.session_id {
                    args.push("resume".into());
                    args.push(sid.into());
                }
                args.push("--model".into());
                args.push(request.model.into());
                // Sandbox is explicit per role: mentor is read-only (the CLI default),
                // executor needs workspace-write to apply edits in the worktree. Without
                // this, `codex exec` defaults to read-only and the executor silently
                // can't modify files (only ~/.codex/config.toml overrides would save it).
                args.push("--sandbox".into());
                if request.role == "mentor" {
                    args.push("read-only".into());
                } else {
                    args.push("workspace-write".into());
                }
                // `codex exec` removed the `--reasoning-effort` flag (clap now rejects it
                // with "unexpected argument"). Reasoning is configured via the
                // `model_reasoning_effort` key, injected through `-c` so it overrides
                // ~/.codex/config.toml for this run. Valid values: minimal|low|medium|high.
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
            ProviderKind::Claude => {
                let mut args = vec![
                    "-p".into(),
                    "--verbose".into(),
                    "--model".into(),
                    request.model.into(),
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
                // effort (only `--max-budget-usd` for cost). Injecting `--reasoning-effort`
                // hard-crashes the turn with `error: unknown option '--reasoning-effort'`.
                // The stored `reasoning_effort` value is intentionally ignored here, and the
                // control is hidden in model_catalog.rs so users can't select a no-op value.
                args.push(request.message.into());

                ProviderTurnCommand {
                    executable: "claude".into(),
                    args,
                    last_message_path: None,
                }
            }
            ProviderKind::Gemini => {
                let (executable, is_antigravity) =
                    crate::provider_registry::resolve_gemini_executable();
                ProviderTurnCommand {
                    executable,
                    args: build_gemini_args(request.model, request.message, is_antigravity),
                    last_message_path: None,
                }
            }
        }
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
            } else if lower.contains("gpt") || lower.starts_with("o1") || lower.starts_with("o3") {
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

/// Build the Gemini-provider CLI args for either backend:
/// - **Antigravity (`agy`)** — successor to the sunsetting Gemini CLI. Has no
///   stream-json mode; `--print` emits the final response as plain text.
///   `--print-timeout` bounds a stuck turn (agy blocks until the response completes);
///   `--dangerously-skip-permissions` avoids approval-prompt hangs in headless mode
///   (the worktree provides isolation). Model ids are the display names from `agy
///   models` (e.g. "Gemini 3.5 Flash (Low)"). Reasoning level is baked into the model
///   name, so no separate flag is needed.
/// - **Legacy `gemini`** — keeps `--output-format stream-json` for structured parsing.
pub fn build_gemini_args(model: &str, message: &str, is_antigravity: bool) -> Vec<String> {
    if is_antigravity {
        // agy uses Go's flag package, which treats an argv element starting with "-"
        // as a flag ("flag provided but not defined") — and agy does NOT honor a "--"
        // separator (it treats "--" itself as the prompt). A task spec or handoff prompt
        // can legitimately start with a "- " markdown bullet, so we prepend a newline to
        // keep the first byte as '\n'; the package then sees a positional, and the
        // newline is semantically invisible to the model.
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
    fn claude_command_uses_stream_json_and_resume_flags() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
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
        // agy has no stream-json; --print emits plain text. Model ids are display names.
        let args = build_gemini_args("Gemini 3.5 Flash (Low)", "explain the current diff", true);
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
        // A prompt starting with "-" would be misparsed as a flag by Go's flag package
        // (agy does not honor "--"), so a leading newline is prepended to keep it a
        // positional. Verified against agy 1.0.8.
        let args = build_gemini_args("Gemini 3.5 Flash (Low)", "- Do the next step", true);
        assert_eq!(
            args.last().expect("prompt is last"),
            "\n- Do the next step"
        );

        // Non-dash prompts are passed through untouched.
        let args = build_gemini_args("Gemini 3.5 Flash (Low)", "Plan the refactor", true);
        assert_eq!(
            args.last().expect("prompt is last"),
            "Plan the refactor"
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
        // Antigravity model ids are capitalized display names and must still route to
        // the Gemini provider (which now spawns agy).
        assert_eq!(
            ProviderAdapter::infer_provider_kind("Gemini 3.5 Flash (Low)"),
            ProviderKind::Gemini
        );
    }

    #[test]
    fn claude_command_omits_reasoning_effort_flag() {
        // Claude Code (2.1.x) has no reasoning-effort CLI flag; injecting it crashes
        // the turn with `error: unknown option`. The stored value must be ignored.
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Claude,
            model: "sonnet",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("high"),
        });

        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
        // The effort value must not leak in as a bare positional (it would become the prompt).
        assert!(!command.args.contains(&"high".to_string()));
    }

    #[test]
    fn gemini_command_omits_thinking_budget_flag() {
        // Gemini CLI (0.46.x) has no --thinking-budget flag; it is silently swallowed.
        // The stored effort value must not leak in as a bare positional (prompt shift).
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
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

    #[test]
    fn codex_command_injects_reasoning_effort_via_config_override() {
        // codex exec removed --reasoning-effort (clap rejects it); reasoning is set via
        // `-c model_reasoning_effort=<value>`, which overrides ~/.codex/config.toml.
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Codex,
            model: "o3",
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: Some("medium"),
        });

        assert!(command.args.contains(&"-c".to_string()));
        assert!(
            command
                .args
                .contains(&"model_reasoning_effort=medium".to_string())
        );
        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
    }

    #[test]
    fn opencode_command_does_not_add_reasoning_effort() {
        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model: "openai/gpt-4o-mini",
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
