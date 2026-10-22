use super::Provider;
use crate::provider_adapter::{ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};

/// A token count off the wire. OpenCode's 2.x token schema is
/// `Schema.Finite` (`packages/schema/src/token-usage.ts`), so a fractional
/// count is legal JSON for it; `as_u64()` would drop the whole step.
/// Fractional counts round up — at usage-display scale the overcount is
/// invisible, and nothing downstream expects exactness.
fn token_count(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|count| count.max(0.0).ceil() as u64))
}
use serde_json::Value;
use std::path::Path;

/// OpenCode — multi-provider gateway. Model ids use `provider/model` format.
pub struct OpenCodeProvider;

#[derive(Clone, Copy)]
struct ReasoningVariant {
    effort: &'static str,
    cli_variant: &'static str,
}

const MINIMAX_M3_REASONING_VARIANTS: &[ReasoningVariant] = &[
    ReasoningVariant {
        effort: "adaptive",
        cli_variant: "thinking",
    },
    ReasoningVariant {
        effort: "disabled",
        cli_variant: "none",
    },
];

fn reasoning_variants_for_model(model_id: &str) -> Option<&'static [ReasoningVariant]> {
    let (source_provider, source_model) = model_id.split_once('/')?;
    let supported_provider = source_provider.eq_ignore_ascii_case("minimax")
        || source_provider.eq_ignore_ascii_case("minimax-cn")
        || source_provider.eq_ignore_ascii_case("minimax-coding-plan")
        || source_provider.eq_ignore_ascii_case("minimax-cn-coding-plan");
    (supported_provider && source_model.eq_ignore_ascii_case("MiniMax-M3"))
        .then_some(MINIMAX_M3_REASONING_VARIANTS)
}

/// Config file names OpenCode reads in a config directory.
const CONFIG_FILES: [&str; 2] = ["opencode.json", "opencode.jsonc"];
/// Subdirectories of a config directory holding markdown agent files; the
/// agent `plan` is `<dir>/plan.md`.
const AGENT_FILE_DIRS: [&str; 4] = ["agent", "agents", "mode", "modes"];

/// Fixed config locations OpenCode 2.x reads before the user's own config
/// (`packages/core/src/config/discovery.ts`: system directory + managed
/// preferences, verified against the v2.0.32 tag source). An MDM policy that
/// disables `plan` here must still drop `--agent plan`. Listed first in the
/// source order, so a later explicit value overrides them.
#[cfg(target_os = "macos")]
const SYSTEM_CONFIG_DIRS: [&str; 2] =
    ["/Library/Application Support/opencode", "/etc/opencode"];
#[cfg(target_os = "windows")]
const SYSTEM_CONFIG_DIRS: [&str; 1] = ["C:/ProgramData/opencode"];
#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
const SYSTEM_CONFIG_DIRS: [&str; 1] = ["/etc/opencode"];

/// The macOS managed-preferences plist MDM tooling can use to pin OpenCode
/// config (`ai.opencode.managed`, verified against v2.0.32 source). Its
/// payload is OpenCode's config JSON; parsing plist ourselves would need a
/// new dependency, so a file that mentions `plan` and one of the disable
/// keys fails open (see `plan_agent_disabled`).
#[cfg(target_os = "macos")]
fn managed_preferences_paths(home: &Path) -> Vec<std::path::PathBuf> {
    let user = home
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if user.is_empty() {
        return Vec::new();
    }
    vec![std::path::PathBuf::from(format!(
        "/Library/Managed Preferences/{user}/ai.opencode.managed.plist"
    ))]
}

/// Whether the user's OpenCode config disables the built-in `plan` agent:
/// `{"agent":{"plan":{"disable":true}}}` (1.x schema, still read by 2.x),
/// `{"agents":{"plan":{"disabled":true}}}` (2.x), the deprecated `mode.plan`,
/// or a `plan.md` agent file with `disable: true` front matter.
///
/// OpenCode 1.x answers `--agent plan` for a disabled agent by falling back to
/// the default one; 2.x fails every such run with `Agent not found: "plan"`.
///
/// Sources are read in OpenCode 2.x's precedence order and a later explicit
/// value wins: the global config dir(s), `$OPENCODE_CONFIG`, then (when
/// `project_dir` is known) `opencode.json[c]` and `.opencode/` from the
/// filesystem root down to `project_dir`, then `$OPENCODE_CONFIG_CONTENT`.
///
/// Fails open: a config file this parser can't read (OpenCode also accepts
/// `{env:...}` / `{file:...}` substitutions) counts as disabling the agent
/// when it mentions it. Dropping `--agent plan` only loses the plan agent's
/// edit guard (the mentor prompt still asks for read-only work); a wrong
/// `--agent plan` fails every mentor turn.
pub(crate) fn plan_agent_disabled(project_dir: Option<&Path>) -> bool {
    let mut disabled = None;
    let mut apply = |flag: Option<bool>| {
        if flag.is_some() {
            disabled = flag;
        }
    };

    for dir in SYSTEM_CONFIG_DIRS {
        apply(config_dir_plan_flag(Path::new(dir)));
    }
    #[cfg(target_os = "macos")]
    for path in managed_preferences_paths(&crate::provider_registry::homedir()) {
        apply(config_file_plan_flag(&path));
    }
    for dir in crate::config_paths::opencode_config_dirs() {
        apply(config_dir_plan_flag(&dir));
    }
    if let Some(file) = std::env::var_os("OPENCODE_CONFIG").filter(|value| !value.is_empty()) {
        apply(config_file_plan_flag(Path::new(&file)));
    }
    if let Some(project_dir) = project_dir {
        let from_root: Vec<&Path> = project_dir.ancestors().collect::<Vec<_>>();
        for dir in from_root.iter().rev() {
            for name in CONFIG_FILES {
                apply(config_file_plan_flag(&dir.join(name)));
            }
        }
        for dir in from_root.iter().rev() {
            apply(config_dir_plan_flag(&dir.join(".opencode")));
        }
    }
    if let Ok(content) = std::env::var("OPENCODE_CONFIG_CONTENT") {
        if !content.trim().is_empty() {
            apply(config_text_plan_flag(&content));
        }
    }

    disabled == Some(true)
}

/// The plan agent's `disable` flag set by a config directory: its config
/// files, then its markdown agent files.
fn config_dir_plan_flag(dir: &Path) -> Option<bool> {
    let mut flag = None;
    for name in CONFIG_FILES {
        flag = config_file_plan_flag(&dir.join(name)).or(flag);
    }
    for sub in AGENT_FILE_DIRS {
        flag = agent_file_plan_flag(&dir.join(sub).join("plan.md")).or(flag);
    }
    flag
}

fn config_file_plan_flag(path: &Path) -> Option<bool> {
    config_text_plan_flag(&std::fs::read_to_string(path).ok()?)
}

fn config_text_plan_flag(text: &str) -> Option<bool> {
    match serde_json::from_str::<Value>(&strip_jsonc(text)) {
        Ok(config) => plan_flag_in_config(&config),
        Err(_) => text.contains("\"plan\"").then_some(true),
    }
}

fn plan_flag_in_config(config: &Value) -> Option<bool> {
    let mut flag = None;
    for section in ["mode", "agent", "agents"] {
        let Some(plan) = config.get(section).and_then(|agents| agents.get("plan")) else {
            continue;
        };
        for key in ["disable", "disabled"] {
            match plan.get(key).and_then(Value::as_bool) {
                Some(true) => return Some(true),
                Some(false) => flag = Some(false),
                None => {}
            }
        }
    }
    flag
}

/// `disable` / `disabled` from a markdown agent file's front matter.
fn agent_file_plan_flag(path: &Path) -> Option<bool> {
    let text = std::fs::read_to_string(path).ok()?;
    let front_matter = front_matter(&text)?;
    let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(&front_matter) else {
        return front_matter.contains("disable").then_some(true);
    };
    let mut flag = None;
    for key in ["disable", "disabled"] {
        match yaml.get(key).and_then(serde_yaml::Value::as_bool) {
            Some(true) => return Some(true),
            Some(false) => flag = Some(false),
            None => {}
        }
    }
    flag
}

/// The YAML between a leading `---` line and the next `---` line.
fn front_matter(text: &str) -> Option<String> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut block = Vec::new();
    for line in lines {
        if line.trim_end() == "---" {
            return Some(block.join("\n"));
        }
        block.push(line);
    }
    None
}

/// JSONC → JSON: drops `//` and `/* */` comments and trailing commas outside
/// strings, as OpenCode's config parser accepts them.
fn strip_jsonc(text: &str) -> String {
    let mut without_comments = String::with_capacity(text.len());
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    let mut in_string = false;
    while let Some(ch) = chars.next() {
        if in_string {
            without_comments.push(ch);
            if ch == '\\' {
                if let Some(escaped) = chars.next() {
                    without_comments.push(escaped);
                }
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match (ch, chars.peek()) {
            ('"', _) => {
                in_string = true;
                without_comments.push(ch);
            }
            ('/', Some('/')) => {
                while chars.peek().is_some_and(|&next| next != '\n') {
                    chars.next();
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
                without_comments.push(' ');
            }
            _ => without_comments.push(ch),
        }
    }

    // A `,` whose next significant character closes the object/array.
    let chars: Vec<char> = without_comments.chars().collect();
    let mut json = String::with_capacity(without_comments.len());
    let mut in_string = false;
    let mut escaped = false;
    for (index, &ch) in chars.iter().enumerate() {
        if in_string {
            json.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == ','
            && matches!(
                chars[index + 1..].iter().find(|next| !next.is_whitespace()),
                Some('}') | Some(']')
            )
        {
            continue;
        }
        json.push(ch);
    }
    json
}

fn build_opencode_turn_command(
    request: &ProviderTurnRequest,
    variant_syntax: crate::provider_registry::OpencodeVariantSyntax,
    plan_agent_disabled: bool,
    session_exists: Option<bool>,
) -> ProviderTurnCommand {
    let model = request.model.to_string();
    let mut args = vec!["run".into(), "--model".into(), model];

    // Resolve the variant the selected effort maps to, if any.
    let variant = reasoning_variants_for_model(request.model)
        .zip(request.reasoning_effort)
        .and_then(|(variants, effort)| {
            variants
                .iter()
                .find(|variant| variant.effort == effort)
                .map(|variant| variant.cli_variant)
        });

    // OpenCode 1.x: emit `--variant <cli>` as a separate flag.
    // OpenCode 2.x: bake the variant into the model id (`provider/model#variant`).
    // Older installs without either form silently skip the variant.
    match (variant_syntax, variant) {
        (crate::provider_registry::OpencodeVariantSyntax::Flag, Some(cli_variant)) => {
            args.push("--variant".into());
            args.push(cli_variant.into());
        }
        (crate::provider_registry::OpencodeVariantSyntax::Suffix, Some(cli_variant)) => {
            // Re-stamp the model id with the variant suffix; the bare
            // `--model <id>` placeholder was inserted above.
            let model_idx = args.iter().position(|arg| arg == "--model").unwrap() + 1;
            args[model_idx] = format!("{}#{}", request.model, cli_variant);
        }
        _ => {}
    }

    // The mentor runs as OpenCode's built-in `plan` agent. It denies the
    // edit/write/patch tools and adds a read-only plan-mode reminder. `bash`
    // stays allowed (the agent inherits `"*": "allow"`), so shell writes are
    // blocked only by that reminder and the mentor prompt.
    //
    // This is safe for non-interactive `run`: `deny` rules never prompt, and
    // the plan agent adds no `ask` rules beyond the build agent's defaults.
    // Verified against v1.1.1–v1.18.32 and 2.0.14. Early 1.0.x releases had
    // a plan agent with `bash: {"*": "ask"}` and a `run` that answered
    // permission requests through an interactive terminal prompt, which could
    // stall a headless turn. No 1.0.x release supports variants (`--variant`
    // arrived in 1.1.x), so those installs keep the default agent. So does a
    // config that disables `plan`: 2.x would fail the run (see
    // `plan_agent_disabled`).
    if request.role == "mentor"
        && variant_syntax != crate::provider_registry::OpencodeVariantSyntax::Unsupported
        && !plan_agent_disabled
    {
        args.push("--agent".into());
        args.push("plan".into());
    }

    // The executor runs unattended: `--auto` approves whatever is not
    // explicitly denied. Without it a headless turn that hits one of the
    // build agent's built-in `ask` rules (external directories, `*.env*`
    // reads) is either failed (1.x and early 2.x) or, since 2.0.15, answered
    // with a rejection while the turn still exits 0 — the action silently
    // never ran. Deny rules keep applying under `--auto`, so a user's
    // explicit denies still bind. The flag exists in `run` on both majors
    // (verified against opencode-ai 1.18.32 and 2.0.14/2.0.32) but not on
    // pre-variant 1.0.x, whose `run` answered permission prompts on the
    // terminal; those installs keep the old behavior instead of an unknown
    // flag. The mentor never gets it: `plan`'s deny rules and ask posture
    // are its read-only guard.
    if request.role != "mentor"
        && variant_syntax != crate::provider_registry::OpencodeVariantSyntax::Unsupported
    {
        args.push("--auto".into());
    }

    // OpenCode 1.x fails a run whose `--session` id is unknown ("Error:
    // Session not found", verified against 1.18.32), but 2.x silently creates
    // a fresh empty session under that id (verified live on 2.0.14 and
    // 2.0.32; the 2.0.32 help spells it out: "Session ID to continue, or to
    // create if it does not exist") and every event still echoes the same
    // id, so a run whose snapshot references a deleted session would keep
    // "resuming" a context-free ghost. When the probe proved the id gone,
    // drop the flag: the turn starts a real new session and the spawner
    // persists its fresh id, so exactly one turn loses context. An
    // inconclusive probe (`None`) keeps the flag — 1.x keeps its loud
    // failure, 2.x keeps today's behavior.
    if let Some(sid) = request.session_id {
        if session_exists != Some(false) {
            args.push("--session".into());
            args.push(sid.into());
        }
    }
    args.push("--format".into());
    args.push("json".into());
    // opencode 2.x prints the `run` help (exit 0) for a message starting with
    // `-`, and `--` duplicates the message; a leading newline is safe.
    let prompt = if request.message.starts_with('-') {
        format!("\n{}", request.message)
    } else {
        request.message.to_string()
    };
    args.push(prompt);

    ProviderTurnCommand {
        executable: "opencode".into(),
        args,
        last_message_path: None,
    }
}

impl Provider for OpenCodeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Opencode
    }

    fn executable(&self) -> &str {
        "opencode"
    }

    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand {
        self.build_turn_command_in(request, None)
    }

    fn build_turn_command_in(
        &self,
        request: &ProviderTurnRequest,
        working_dir: Option<&Path>,
    ) -> ProviderTurnCommand {
        // Only a mentor turn asks for the plan agent. OpenCode loads
        // `opencode.json[c]` and `.opencode/` from its working directory, so the
        // same check has to see the pair's directory: a project that disables
        // the plan agent makes every `--agent plan` run fail with
        // `Agent not found: "plan"` (verified against opencode 2.0.14).
        let plan_disabled = request.role == "mentor" && plan_agent_disabled(working_dir);
        // Only a resumed turn can point at a session OpenCode lost; the probe
        // is skipped for first turns so they pay nothing.
        let session_exists = request.session_id.and_then(|session_id| {
            crate::provider_registry::opencode_session_exists(session_id, working_dir)
        });
        build_opencode_turn_command(
            request,
            crate::provider_registry::opencode_variant_syntax(),
            plan_disabled,
            session_exists,
        )
    }

    fn usage_scope(&self, event: &Value) -> super::UsageScope {
        let is_step_finish = |value: Option<&Value>| {
            matches!(
                value.and_then(|v| v.as_str()),
                Some("step_finish" | "step-finish")
            )
        };
        if is_step_finish(event.get("type"))
            || is_step_finish(event.get("part").and_then(|part| part.get("type")))
        {
            super::UsageScope::Step
        } else {
            super::UsageScope::Turn
        }
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        // `run --format json` emits one `step_finish` event per model step.
        // Its `part.tokens` holds `{input, output, reasoning, cache: {read,
        // write}}` for that step only (1.x and 2.x). The whole turn's usage
        // is the sum over its steps. Providers are stateless per event, so
        // the spawner has to do that summing; each step is reported as-is here.
        //
        // Caveat: 2.x may drop the final step's `step_finish`. Once the
        // session finishes, `run/noninteractive.ts` skips every event still
        // in flight, so whether the closing `reason: "stop"` step arrives is a
        // race (verified live on 2.0.14 and 2.0.24: single-step turns usually
        // print it, multi-step tool turns usually end on `text`). 1.x
        // (verified live on 1.18.32) always prints it. When it is dropped the
        // turn total misses the last step and the source stays Live.
        if let Some(part) = event.get("part") {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if part_type == "step-finish" || part_type == "step_finish" {
                if let Some(tokens) = part.get("tokens") {
                    let bucket = |key: &str| tokens.get(key).and_then(token_count);
                    let cache = |key: &str| {
                        tokens
                            .get("cache")
                            .and_then(|c| c.get(key))
                            .and_then(token_count)
                            .unwrap_or(0)
                    };

                    // OpenCode subtracts reasoning from `output`. Add it back
                    // so the count matches Claude and Codex, which include
                    // thinking in their output tokens.
                    let output_tokens = bucket("output")
                        .or_else(|| bucket("completionTokens"))
                        .or_else(|| bucket("completion_tokens"))
                        .map(|output| output + bucket("reasoning").unwrap_or(0));

                    // `input` counts only the uncached prompt tokens. Fold the
                    // cache buckets in so the count covers the whole prompt, as
                    // Claude, Gemini and Grok do.
                    let input_tokens = bucket("input")
                        .or_else(|| bucket("promptTokens"))
                        .or_else(|| bucket("prompt_tokens"));

                    if let Some(output) = output_tokens {
                        let input_val =
                            input_tokens.map(|uncached| uncached + cache("read") + cache("write"));
                        // A step is final when the model will not continue:
                        // `FinishReason` is
                        // `stop|length|tool-calls|content-filter|error|unknown`
                        // (2.x schema), and only `tool-calls` continues into
                        // the next tool round -> Live. An absent reason keeps
                        // the 1.x reading of final; an unrecognized one stays
                        // Live so a future reason can't freeze the total early.
                        let is_stop = part
                            .get("reason")
                            .and_then(|v| v.as_str())
                            .map(|reason| {
                                matches!(reason, "stop" | "length" | "content-filter" | "error")
                            })
                            .unwrap_or(true);
                        return Some(TurnTokenUsage {
                            output_tokens: output,
                            input_tokens: input_val,
                            last_updated_at: crate::util::now_millis(),
                            source: if is_stop {
                                TokenUsageSource::Final
                            } else {
                                TokenUsageSource::Live
                            },
                            provider: Some("opencode".to_string()),
                        });
                    }
                }
            }
        }

        // Fallback: try direct usage field (older format or different event)
        let usage = event.get("usage")?;

        let output_tokens = usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens"))
            .or_else(|| usage.get("completionTokens"))
            .or_else(|| usage.get("output"))
            .and_then(token_count)?;

        let input_tokens = usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .or_else(|| usage.get("promptTokens"))
            .or_else(|| usage.get("input"))
            .and_then(token_count);

        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let is_final = event_type == "result"
            || event_type == "complete"
            || event_type == "done"
            || event_type == "finish-step"
            || event_type == "finish"
            || event_type == "step_finish";

        Some(TurnTokenUsage {
            output_tokens,
            input_tokens,
            last_updated_at: crate::util::now_millis(),
            source: if is_final {
                TokenUsageSource::Final
            } else {
                TokenUsageSource::Live
            },
            provider: Some("opencode".to_string()),
        })
    }

    fn extract_error_detail(&self, event: &Value) -> Option<String> {
        // 2.x: {"type":"error","error":{"type":"provider.no-route","message":"…",
        //        "response":{"body":<raw upstream error>}}}
        // 1.x: {"type":"error","error":{"name":"…","data":{"message":"…"}}}
        if event.get("type").and_then(|v| v.as_str()) != Some("error") {
            return None;
        }
        let error = event.get("error")?;
        let message = error
            .get("message")
            .and_then(|v| v.as_str())
            .or_else(|| error.pointer("/data/message").and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty());
        // Since opencode 2.0.14+ the raw upstream error document rides in
        // `error.response.body` (string or object); on HTTP-level provider
        // failures the actionable detail lives there.
        let body_detail = error
            .pointer("/response/body")
            .and_then(|body| {
                body.as_str()
                    .or_else(|| body.get("message").and_then(|v| v.as_str()))
                    .or_else(|| body.pointer("/error/message").and_then(|v| v.as_str()))
            })
            .map(str::trim)
            .filter(|s| !s.is_empty() && Some(*s) != message);
        let message = match (message, body_detail) {
            (Some(message), Some(body)) => Some(format!("{message} — {body}")),
            (None, Some(body)) => Some(body.to_string()),
            (message, None) => message.map(str::to_string),
        };
        let kind = error
            .get("type")
            .or_else(|| error.get("name"))
            .and_then(|v| v.as_str());
        Some(match (kind, message) {
            (Some(kind), Some(message)) => format!("{kind}: {message}"),
            (None, Some(message)) => message,
            (Some(kind), None) => kind.to_string(),
            (None, None) => error.to_string(),
        })
    }

    fn clears_turn_error(&self, event: &Value) -> bool {
        // 2.x also emits `error` for transient transport failures it retries;
        // further output means the turn recovered.
        matches!(
            event.get("type").and_then(|v| v.as_str()),
            Some("text") | Some("step_finish") | Some("step-finish")
        )
    }

    fn detect(&self) -> DetectedProviderProfile {
        crate::provider_registry::ProviderRegistry::detect_opencode()
    }

    fn brand(&self) -> &str {
        "opencode"
    }

    fn provider_label(&self) -> &str {
        "OpenCode"
    }

    fn billing_kind(&self) -> &str {
        "byok"
    }

    fn billing_label(&self) -> &str {
        "Pay as you go"
    }

    fn access_label(&self, source_provider_label: &str) -> String {
        format!("{} API key", source_provider_label)
    }

    fn reasoning_effort_levels(&self, model_id: &str) -> Option<Vec<String>> {
        reasoning_variants_for_model(model_id).map(|variants| {
            variants
                .iter()
                .map(|variant| variant.effort.to_string())
                .collect()
        })
    }

    fn should_filter_unavailable_models(&self) -> bool {
        true
    }

    fn login_command(&self) -> Option<String> {
        Some("opencode auth login".into())
    }

    fn install_url(&self) -> Option<String> {
        Some("https://opencode.ai".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_registry::OpencodeVariantSyntax;

    /// A command built with no config disabling the plan agent and no
    /// session-existence probe result.
    fn build_command(
        request: &ProviderTurnRequest,
        syntax: OpencodeVariantSyntax,
    ) -> ProviderTurnCommand {
        build_opencode_turn_command(request, syntax, false, None)
    }

    fn base_request<'a>(
        model: &'a str,
        reasoning_effort: Option<&'a str>,
    ) -> ProviderTurnRequest<'a> {
        ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model,
            session_id: None,
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort,
        }
    }

    #[test]
    fn opencode_flash_preserves_discovered_routes_and_resumed_session() {
        for model in [
            "minimax/MiniMax-M3.1-Flash",
            "opencode/minimax-m3.1-flash-free",
            "openrouter/minimax/minimax-m3.1-flash:free",
        ] {
            for syntax in [OpencodeVariantSyntax::Flag, OpencodeVariantSyntax::Suffix] {
                let mut request = base_request(model, None);
                request.session_id = Some("ses_flash_history");
                let command = build_command(&request, syntax);
                let model_index = command
                    .args
                    .iter()
                    .position(|arg| arg == "--model")
                    .unwrap();
                assert_eq!(command.args[model_index + 1], model);
                assert!(!command.args.contains(&"--variant".to_string()));
                assert!(command.args.contains(&"ses_flash_history".to_string()));
                assert_eq!(command.args.last().unwrap(), "do the work");
                // Gateway effort variants are owned by the installed CLI. Do not
                // expose M3's disabled variant for adaptive-only Flash.
                assert_eq!(OpenCodeProvider.reasoning_effort_levels(model), None);
            }
        }
    }

    #[test]
    fn opencode_command_ignores_unsupported_reasoning_effort() {
        let command = build_command(
            &base_request("example/model", Some("high")),
            OpencodeVariantSyntax::Flag,
        );

        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
        assert!(!command.args.contains(&"high".to_string()));
        // The unsupported model id never receives a variant regardless of syntax.
        let model_idx = command
            .args
            .iter()
            .position(|arg| arg == "--model")
            .unwrap();
        assert_eq!(command.args[model_idx + 1], "example/model");
    }

    #[test]
    fn opencode_command_maps_reasoning_effort_to_cli_variant_for_opencode_1x() {
        // OpenCode 1.x emits --model <id> plus a separate --variant <cli> flag.
        for (effort, expected_variant) in [("adaptive", "thinking"), ("disabled", "none")] {
            let command = build_command(
                &base_request("minimax-cn/MiniMax-M3", Some(effort)),
                OpencodeVariantSyntax::Flag,
            );

            let model_idx = command
                .args
                .iter()
                .position(|arg| arg == "--model")
                .expect("--model flag should be present");
            assert_eq!(command.args[model_idx + 1], "minimax-cn/MiniMax-M3");

            let variant_index = command
                .args
                .iter()
                .position(|arg| arg == "--variant")
                .expect("1.x reasoning should use the OpenCode --variant flag");
            assert_eq!(command.args[variant_index + 1], expected_variant);
        }
    }

    #[test]
    fn opencode_command_bakes_variant_into_model_id_for_opencode_2x() {
        // OpenCode 2.x removed --variant; the variant must be appended to the
        // model id with `#variant`.
        for (effort, expected_variant) in [("adaptive", "thinking"), ("disabled", "none")] {
            let command = build_command(
                &base_request("minimax/MiniMax-M3", Some(effort)),
                OpencodeVariantSyntax::Suffix,
            );

            let model_idx = command
                .args
                .iter()
                .position(|arg| arg == "--model")
                .expect("--model flag should be present");
            assert_eq!(
                command.args[model_idx + 1],
                format!("minimax/MiniMax-M3#{}", expected_variant)
            );
            // --variant must NOT be present on 2.x.
            assert!(
                !command.args.contains(&"--variant".to_string()),
                "OpenCode 2.x does not accept --variant"
            );
        }
    }

    #[test]
    fn opencode_command_omits_variant_when_installed_cli_has_no_variant_support() {
        let command = build_command(
            &base_request("minimax/MiniMax-M3", Some("adaptive")),
            OpencodeVariantSyntax::Unsupported,
        );

        assert!(!command.args.contains(&"--variant".to_string()));
        assert!(!command.args.contains(&"thinking".to_string()));
        let model_idx = command
            .args
            .iter()
            .position(|arg| arg == "--model")
            .unwrap();
        assert_eq!(command.args[model_idx + 1], "minimax/MiniMax-M3");
    }

    #[test]
    fn opencode_command_resumes_session_and_emits_json_format() {
        let command = build_command(
            &ProviderTurnRequest {
                provider_kind: ProviderKind::Opencode,
                model: "minimax/MiniMax-M3",
                session_id: Some("session-xyz"),
                role: "executor",
                pair_id: "pair-1",
                message: "do the work",
                reasoning_effort: None,
            },
            OpencodeVariantSyntax::Suffix,
        );

        assert_eq!(command.executable, "opencode");
        let session_idx = command
            .args
            .iter()
            .position(|arg| arg == "--session")
            .expect("--session should be present when session_id is Some");
        assert_eq!(command.args[session_idx + 1], "session-xyz");
        let format_idx = command
            .args
            .iter()
            .position(|arg| arg == "--format")
            .expect("--format should be present");
        assert_eq!(command.args[format_idx + 1], "json");
        assert_eq!(command.args.last().unwrap(), "do the work");
    }

    #[test]
    fn opencode_executor_auto_approves_and_the_mentor_never_does() {
        // `--auto` keeps a headless executor turn from being failed (1.x,
        // early 2.x) or silently partial (2.0.15+) by the build agent's
        // built-in `ask` rules. It exists on both variant-capable majors.
        for syntax in [OpencodeVariantSyntax::Flag, OpencodeVariantSyntax::Suffix] {
            let executor = build_command(&base_request("minimax/MiniMax-M3", None), syntax);
            assert!(
                executor.args.contains(&"--auto".to_string()),
                "executor turns auto-approve on {syntax:?}"
            );
        }

        // Pre-variant 1.0.x `run` answered permission prompts on the
        // terminal and does not know the flag; unknown flags fail the turn.
        let legacy = build_command(
            &base_request("minimax/MiniMax-M3", None),
            OpencodeVariantSyntax::Unsupported,
        );
        assert!(!legacy.args.contains(&"--auto".to_string()));

        // The mentor's read-only posture *is* the plan agent's deny/ask
        // rules; auto-approving would punch through them.
        let mentor_request = ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model: "minimax/MiniMax-M3",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "review the diff",
            reasoning_effort: None,
        };
        let mentor = build_command(&mentor_request, OpencodeVariantSyntax::Suffix);
        assert!(!mentor.args.contains(&"--auto".to_string()));
    }

    #[test]
    fn opencode_resume_drops_a_session_the_cli_no_longer_has() {
        // 2.x silently recreates an unknown `--session` id as an empty
        // session (verified live on 2.0.14/2.0.32), so a proven-missing id
        // must not be sent; existing and inconclusive probes keep resuming.
        let request = ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model: "minimax/MiniMax-M3",
            session_id: Some("ses_gone"),
            role: "executor",
            pair_id: "pair-1",
            message: "do the work",
            reasoning_effort: None,
        };
        let gone = build_opencode_turn_command(
            &request,
            OpencodeVariantSyntax::Suffix,
            false,
            Some(false),
        );
        assert!(!gone.args.contains(&"--session".to_string()));
        assert!(!gone.args.contains(&"ses_gone".to_string()));

        for probe in [Some(true), None] {
            let command =
                build_opencode_turn_command(&request, OpencodeVariantSyntax::Suffix, false, probe);
            let session_idx = command
                .args
                .iter()
                .position(|arg| arg == "--session")
                .expect("--session kept unless the probe proved the id gone");
            assert_eq!(command.args[session_idx + 1], "ses_gone");
        }
    }

    #[test]
    fn opencode_step_usage_rounds_fractional_tokens_and_finishes_on_length() {
        // 2.x's token schema (`Schema.Finite`) permits fractional counts;
        // they must not drop the whole step. A `length` stop ends the turn
        // just like `stop`, so its usage is Final.
        let provider = OpenCodeProvider;
        let fractional = serde_json::json!({
            "type": "step_finish",
            "part": {
                "type": "step-finish",
                "reason": "length",
                "tokens": {
                    "input": 100.2,
                    "output": 50.6,
                    "reasoning": 10.0,
                    "cache": {"read": 5.5, "write": 0}
                }
            }
        });
        let usage = provider
            .extract_token_usage(&fractional)
            .expect("fractional step usage");
        assert_eq!(usage.input_tokens, Some(101 + 6 + 0));
        assert_eq!(usage.output_tokens, 51 + 10);
        assert_eq!(usage.source, TokenUsageSource::Final);
    }

    #[test]
    fn opencode_exposes_only_supported_model_reasoning_variants() {
        let provider = OpenCodeProvider;

        assert_eq!(
            provider.reasoning_effort_levels("minimax/MiniMax-M3"),
            Some(vec!["adaptive".to_string(), "disabled".to_string()])
        );
        assert_eq!(
            provider.reasoning_effort_levels("minimax-cn/MiniMax-M3"),
            Some(vec!["adaptive".to_string(), "disabled".to_string()])
        );
        assert_eq!(
            provider.reasoning_effort_levels("minimax-cn-coding-plan/MiniMax-M3"),
            Some(vec!["adaptive".to_string(), "disabled".to_string()])
        );
        assert_eq!(
            provider.reasoning_effort_levels("fireworks-ai/accounts/fireworks/models/minimax-m3"),
            None
        );
        assert_eq!(
            provider.reasoning_effort_levels("minimax/MiniMax-M2.7"),
            None
        );
    }

    #[test]
    fn opencode_guards_leading_dash_prompt() {
        let command = build_command(
            &ProviderTurnRequest {
                provider_kind: ProviderKind::Opencode,
                model: "opencode/mimo-v2.6-flash-free",
                session_id: None,
                role: "executor",
                pair_id: "pair-1",
                message: "- Do the next step",
                reasoning_effort: None,
            },
            crate::provider_registry::OpencodeVariantSyntax::Suffix,
        );
        assert_eq!(command.args.last().unwrap(), "\n- Do the next step");
        assert!(!command.args.contains(&"--".to_string()));
    }

    #[test]
    fn opencode_mentor_runs_as_the_read_only_plan_agent() {
        let mentor_request = ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model: "minimax/MiniMax-M3",
            session_id: Some("ses_1"),
            role: "mentor",
            pair_id: "pair-1",
            message: "review the diff",
            reasoning_effort: None,
        };
        for syntax in [OpencodeVariantSyntax::Flag, OpencodeVariantSyntax::Suffix] {
            let command = build_command(&mentor_request, syntax);
            let agent_idx = command
                .args
                .iter()
                .position(|arg| arg == "--agent")
                .expect("mentor should run as the plan agent");
            assert_eq!(command.args[agent_idx + 1], "plan");
            assert_eq!(command.args.last().unwrap(), "review the diff");
        }

        // OpenCode 1.0.x (no variant support) answered permission requests with
        // an interactive prompt, and early 1.0.x plan agents asked before most
        // bash commands, so these installs keep the default agent.
        let legacy = build_command(&mentor_request, OpencodeVariantSyntax::Unsupported);
        assert!(!legacy.args.contains(&"--agent".to_string()));

        let executor = build_command(
            &base_request("minimax/MiniMax-M3", None),
            OpencodeVariantSyntax::Suffix,
        );
        assert!(!executor.args.contains(&"--agent".to_string()));
    }

    #[test]
    fn opencode_mentor_skips_the_plan_agent_when_config_disables_it() {
        let mentor_request = ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model: "minimax/MiniMax-M3",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: None,
        };
        let command = build_opencode_turn_command(
            &mentor_request,
            OpencodeVariantSyntax::Suffix,
            true,
            None,
        );
        assert!(!command.args.contains(&"--agent".to_string()));
        assert_eq!(command.args.last().unwrap(), "plan the work");
    }

    #[test]
    fn strip_jsonc_drops_comments_and_trailing_commas_outside_strings() {
        let jsonc = "\u{feff}{\n  // the plan agent\n  \"agent\": { /* inline */ \"plan\": { \"disable\": true, }, },\n  \"url\": \"https://x.test/a//b\", \"note\": \"keep ,} and /* this */\",\n}\n";
        let value: Value = serde_json::from_str(&strip_jsonc(jsonc)).expect("valid JSON");
        assert_eq!(value["agent"]["plan"]["disable"], Value::Bool(true));
        assert_eq!(value["url"], "https://x.test/a//b");
        assert_eq!(value["note"], "keep ,} and /* this */");
    }

    #[test]
    fn plan_flag_reads_every_config_schema() {
        for text in [
            r#"{"agent":{"plan":{"disable":true}}}"#,
            r#"{"agents":{"plan":{"disabled":true}}}"#,
            r#"{"mode":{"plan":{"disable":true}}}"#,
            "{\n  // 2.x\n  \"agents\": {\"plan\": {\"disabled\": true,},},\n}",
        ] {
            assert_eq!(config_text_plan_flag(text), Some(true), "{text}");
        }
        assert_eq!(
            config_text_plan_flag(r#"{"agent":{"plan":{"disable":false,"model":"x/y"}}}"#),
            Some(false)
        );
        assert_eq!(
            config_text_plan_flag(r#"{"agent":{"build":{"disable":true}}}"#),
            None
        );
        assert_eq!(
            config_text_plan_flag(r#"{"$schema":"https://opencode.ai"}"#),
            None
        );
        // Unparseable (e.g. an `{env:...}` substitution): fail open only
        // when the plan agent is mentioned.
        assert_eq!(
            config_text_plan_flag(r#"{"agent":{"plan":{"disable":{env:NO_PLAN}}}}"#),
            Some(true)
        );
        assert_eq!(config_text_plan_flag(r#"{"model": {env:MODEL}}"#), None);
    }

    #[test]
    fn plan_flag_reads_markdown_agent_front_matter() {
        let dir = std::env::temp_dir().join(format!("the-pair-oc-md-{}", uuid::Uuid::new_v4()));
        let file = dir.join("agent/plan.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();

        std::fs::write(
            &file,
            "---\ndescription: Plan\ndisable: true\n---\nPrompt\n",
        )
        .unwrap();
        assert_eq!(agent_file_plan_flag(&file), Some(true));
        assert_eq!(config_dir_plan_flag(&dir), Some(true));

        std::fs::write(&file, "---\ndescription: Plan\n---\nPrompt\n").unwrap();
        assert_eq!(agent_file_plan_flag(&file), None);
        std::fs::write(&file, "No front matter, disable: true\n").unwrap();
        assert_eq!(agent_file_plan_flag(&file), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn managed_preferences_path_names_the_plist_for_the_home_user() {
        assert_eq!(
            managed_preferences_paths(Path::new("/Users/alex")),
            vec![std::path::PathBuf::from(
                "/Library/Managed Preferences/alex/ai.opencode.managed.plist"
            )]
        );
        // A home with no final component has no user to name.
        assert!(managed_preferences_paths(Path::new("/")).is_empty());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn plan_agent_disabled_follows_global_project_and_env_config() {
        const KEYS: [&str; 5] = [
            "HOME",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_CONTENT",
        ];
        let _guard = crate::test_env::lock_env();
        let saved: Vec<(&str, Option<std::ffi::OsString>)> = KEYS
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
        let root = std::env::temp_dir().join(format!("the-pair-oc-cfg-{}", uuid::Uuid::new_v4()));
        let global = root.join("xdg/opencode");
        let project = root.join("repo/.worktrees/pair-1");
        std::fs::create_dir_all(&global).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
        for key in [
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_CONTENT",
        ] {
            std::env::remove_var(key);
        }

        let mut results = Vec::new();
        results.push(("no config", plan_agent_disabled(Some(&project))));

        std::fs::write(
            global.join("opencode.jsonc"),
            "{\n  // no plan mode for me\n  \"agents\": {\"plan\": {\"disabled\": true,},},\n}\n",
        )
        .unwrap();
        results.push(("global jsonc", plan_agent_disabled(None)));

        // A project config closer to the pair re-enables it.
        std::fs::create_dir_all(root.join("repo/.opencode")).unwrap();
        std::fs::write(
            root.join("repo/.opencode/opencode.json"),
            r#"{"agent":{"plan":{"disable":false}}}"#,
        )
        .unwrap();
        results.push(("project re-enables", plan_agent_disabled(Some(&project))));

        std::env::set_var(
            "OPENCODE_CONFIG_CONTENT",
            r#"{"agent":{"plan":{"disable":true}}}"#,
        );
        results.push(("inline content", plan_agent_disabled(Some(&project))));

        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(
            results,
            vec![
                ("no config", false),
                ("global jsonc", true),
                ("project re-enables", false),
                ("inline content", true),
            ]
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn opencode_mentor_drops_plan_agent_when_the_pair_directory_disables_it() {
        // OpenCode loads config from its cwd, so a project that disables the
        // plan agent must also drop `--agent plan` here — otherwise every
        // mentor turn fails with `Agent not found: "plan"`.
        //
        // A stub `opencode` on PATH makes the variant-syntax probe
        // deterministic: `--agent plan` is only sent when the installed CLI
        // understands the `#variant` model suffix, and CI has no real binary.
        const KEYS: [&str; 6] = [
            "HOME",
            "PATH",
            "XDG_CONFIG_HOME",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_CONTENT",
        ];
        let _guard = crate::test_env::lock_env();
        let saved: Vec<(&str, Option<std::ffi::OsString>)> = KEYS
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
        let root = std::env::temp_dir().join(format!("the-pair-oc-dir-{}", uuid::Uuid::new_v4()));
        let project = root.join("repo");
        let bin_dir = root.join("bin");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&bin_dir).unwrap();
        write_stub_opencode(&bin_dir);
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg"));
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin_dir.display(),
                std::env::var_os("PATH").unwrap_or_default().to_string_lossy()
            ),
        );
        for key in [
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_CONTENT",
        ] {
            std::env::remove_var(key);
        }

        let provider = OpenCodeProvider;
        let request = ProviderTurnRequest {
            provider_kind: ProviderKind::Opencode,
            model: "minimax/MiniMax-M3",
            session_id: None,
            role: "mentor",
            pair_id: "pair-1",
            message: "plan the work",
            reasoning_effort: None,
        };
        let mut results = Vec::new();

        let enabled = provider.build_turn_command_in(&request, Some(&project));
        results.push(enabled.args.contains(&"--agent".to_string()));

        std::fs::write(
            project.join("opencode.json"),
            r#"{"agents":{"plan":{"disabled":true}}}"#,
        )
        .unwrap();
        let disabled = provider.build_turn_command_in(&request, Some(&project));
        results.push(disabled.args.contains(&"--agent".to_string()));
        assert_eq!(disabled.args.last().unwrap(), "plan the work");

        // Without a working directory the project config is out of scope, so
        // the plan agent stays in play (global dirs point at the temp root).
        results.push(
            provider
                .build_turn_command(&request)
                .args
                .contains(&"--agent".to_string()),
        );

        // A resumed turn probes `session list` (the stub knows `ses_live`
        // only): a session OpenCode lost is dropped so 2.x can't silently
        // recreate it empty, a live one is kept.
        let resumed = ProviderTurnRequest {
            session_id: Some("ses_gone"),
            ..request
        };
        let dropped = provider.build_turn_command_in(&resumed, Some(&project));
        results.push(!dropped.args.contains(&"--session".to_string()));
        let resumed = ProviderTurnRequest {
            session_id: Some("ses_live"),
            ..request
        };
        let kept = provider.build_turn_command_in(&resumed, Some(&project));
        let session_idx = kept
            .args
            .iter()
            .position(|arg| arg == "--session")
            .expect("live session should resume");
        results.push(kept.args[session_idx + 1] == "ses_live");

        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(results, vec![true, false, true, true, true]);
    }

    /// A stand-in `opencode` whose `run --help` advertises the 2.x
    /// `provider/model#variant` suffix, so `opencode_variant_syntax()` resolves
    /// to `Suffix` on machines without the real CLI (CI).
    #[cfg(not(target_os = "windows"))]
    fn write_stub_opencode(bin_dir: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        let path = bin_dir.join("opencode");
        std::fs::write(
            &path,
            "#!/bin/sh\n\
             if [ \"$1\" = \"run\" ] && [ \"$2\" = \"--help\" ]; then\n\
             \techo '  --model, -m string   Model to use in the format provider/model#variant'\n\
             \texit 0\n\
             fi\n\
             if [ \"$1\" = \"session\" ] && [ \"$2\" = \"list\" ]; then\n\
             \techo '[{\"id\":\"ses_live\",\"directory\":\"/repo\"}]'\n\
             \texit 0\n\
             fi\n\
             exit 1\n",
        )
        .expect("failed to write stub opencode");
        let mut perms = std::fs::metadata(&path)
            .expect("failed to read stub opencode metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("failed to make stub opencode executable");
    }

    #[test]
    fn opencode_step_usage_folds_cache_and_reasoning_buckets() {
        let provider = OpenCodeProvider;
        let step = serde_json::json!({
            "type": "step_finish",
            "part": {
                "type": "step-finish",
                "reason": "tool-calls",
                "cost": 0.01,
                "tokens": {
                    "total": 23120,
                    "input": 1200,
                    "output": 90,
                    "reasoning": 30,
                    "cache": {"read": 21504, "write": 296}
                }
            }
        });
        let usage = provider.extract_token_usage(&step).expect("step usage");
        assert_eq!(usage.input_tokens, Some(1200 + 21504 + 296));
        assert_eq!(usage.output_tokens, 90 + 30);
        assert_eq!(usage.source, TokenUsageSource::Live);

        // Events without cache or reasoning buckets keep their plain counts.
        let stop = serde_json::json!({
            "type": "step_finish",
            "part": {"type": "step-finish", "reason": "stop", "tokens": {"input": 671, "output": 8}}
        });
        let usage = provider.extract_token_usage(&stop).expect("stop usage");
        assert_eq!(usage.input_tokens, Some(671));
        assert_eq!(usage.output_tokens, 8);
        assert_eq!(usage.source, TokenUsageSource::Final);
    }

    #[test]
    fn opencode_error_events_surface_detail_and_recover() {
        let provider = OpenCodeProvider;
        let v2 = serde_json::json!({
            "type": "error",
            "sessionID": "ses_1",
            "error": {"type": "provider.no-route", "message": "Model unavailable: opencode/nope"}
        });
        assert_eq!(
            provider.extract_error_detail(&v2).as_deref(),
            Some("provider.no-route: Model unavailable: opencode/nope")
        );

        let v1 = serde_json::json!({
            "type": "error",
            "error": {"name": "ProviderAuthError", "data": {"message": "missing key"}}
        });
        assert_eq!(
            provider.extract_error_detail(&v1).as_deref(),
            Some("ProviderAuthError: missing key")
        );

        let text = serde_json::json!({"type": "text", "part": {"text": "done"}});
        assert!(provider.extract_error_detail(&text).is_none());
        assert!(provider.clears_turn_error(&text));
        let hyphenated = serde_json::json!({"type": "step-finish"});
        assert!(provider.clears_turn_error(&hyphenated));
        assert!(!provider.clears_turn_error(&v2));
    }

    #[test]
    fn opencode_error_events_append_the_upstream_response_body() {
        // Since 2.0.14+ an HTTP-level provider failure carries the raw
        // upstream error in `error.response.body`; when it adds detail beyond
        // the envelope message it is appended, and it stands in for a missing
        // message entirely.
        let provider = OpenCodeProvider;
        let with_body = serde_json::json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": "Request failed",
                "status": 429,
                "response": {"body": {"error": {"message": "rate limit exceeded for key …"}}}
            }
        });
        assert_eq!(
            provider.extract_error_detail(&with_body).as_deref(),
            Some("api_error: Request failed — rate limit exceeded for key …")
        );

        let body_only = serde_json::json!({
            "type": "error",
            "error": {"type": "api_error", "response": {"body": "upstream said no"}}
        });
        assert_eq!(
            provider.extract_error_detail(&body_only).as_deref(),
            Some("api_error: upstream said no")
        );

        // A body that merely repeats the message is not duplicated.
        let duplicate = serde_json::json!({
            "type": "error",
            "error": {"type": "api_error", "message": "same", "response": {"body": "same"}}
        });
        assert_eq!(
            provider.extract_error_detail(&duplicate).as_deref(),
            Some("api_error: same")
        );
    }
}
