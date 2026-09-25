use super::Provider;
use crate::provider_adapter::{ProviderTurnCommand, ProviderTurnRequest};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use crate::types::{TokenUsageSource, TurnTokenUsage};
use serde_json::Value;

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
        || source_provider.eq_ignore_ascii_case("minimax-cn");
    (supported_provider && source_model.eq_ignore_ascii_case("MiniMax-M3"))
        .then_some(MINIMAX_M3_REASONING_VARIANTS)
}

fn build_opencode_turn_command(
    request: &ProviderTurnRequest,
    variant_syntax: crate::provider_registry::OpencodeVariantSyntax,
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
        (
            crate::provider_registry::OpencodeVariantSyntax::Flag,
            Some(cli_variant),
        ) => {
            args.push("--variant".into());
            args.push(cli_variant.into());
        }
        (
            crate::provider_registry::OpencodeVariantSyntax::Suffix,
            Some(cli_variant),
        ) => {
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
    // arrived in 1.1.x), so those installs keep the default agent.
    if request.role == "mentor"
        && variant_syntax != crate::provider_registry::OpencodeVariantSyntax::Unsupported
    {
        args.push("--agent".into());
        args.push("plan".into());
    }

    if let Some(sid) = request.session_id {
        args.push("--session".into());
        args.push(sid.into());
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
        build_opencode_turn_command(
            request,
            crate::provider_registry::opencode_variant_syntax(),
        )
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        // `run --format json` emits one `step_finish` event per model step.
        // Its `part.tokens` holds `{input, output, reasoning, cache: {read,
        // write}}` for that step only (1.x and 2.x). The whole turn's usage
        // is the sum over its steps. Providers are stateless per event, so
        // the spawner has to do that summing; each step is reported as-is here.
        if let Some(part) = event.get("part") {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if part_type == "step-finish" || part_type == "step_finish" {
                if let Some(tokens) = part.get("tokens") {
                    let bucket = |key: &str| tokens.get(key).and_then(|v| v.as_u64());
                    let cache = |key: &str| {
                        tokens
                            .get("cache")
                            .and_then(|c| c.get(key))
                            .and_then(|v| v.as_u64())
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
                        // A step is final only when `reason == "stop"` (the model is done) or
                        // when `reason` is absent. `reason == "tool-calls"` is an intermediate
                        // step that continues into the next tool round -> Live.
                        let is_stop = part
                            .get("reason")
                            .and_then(|v| v.as_str())
                            .map(|reason| reason == "stop")
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
            .and_then(|v| v.as_u64())?;

        let input_tokens = usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .or_else(|| usage.get("promptTokens"))
            .or_else(|| usage.get("input"))
            .and_then(|v| v.as_u64());

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
        // 2.x: {"type":"error","error":{"type":"provider.no-route","message":"…"}}
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
        let kind = error
            .get("type")
            .or_else(|| error.get("name"))
            .and_then(|v| v.as_str());
        Some(match (kind, message) {
            (Some(kind), Some(message)) => format!("{kind}: {message}"),
            (None, Some(message)) => message.to_string(),
            (Some(kind), None) => kind.to_string(),
            (None, None) => error.to_string(),
        })
    }

    fn clears_turn_error(&self, event: &Value) -> bool {
        // 2.x also emits `error` for transient transport failures it retries;
        // further output means the turn recovered.
        matches!(
            event.get("type").and_then(|v| v.as_str()),
            Some("text") | Some("step_finish")
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
    fn opencode_command_ignores_unsupported_reasoning_effort() {
        let command = build_opencode_turn_command(
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
            let command = build_opencode_turn_command(
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
            let command = build_opencode_turn_command(
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
        let command = build_opencode_turn_command(
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
        let command = build_opencode_turn_command(
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
            None
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
        let command = build_opencode_turn_command(
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
            let command = build_opencode_turn_command(&mentor_request, syntax);
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
        let legacy =
            build_opencode_turn_command(&mentor_request, OpencodeVariantSyntax::Unsupported);
        assert!(!legacy.args.contains(&"--agent".to_string()));

        let executor = build_opencode_turn_command(
            &base_request("minimax/MiniMax-M3", None),
            OpencodeVariantSyntax::Suffix,
        );
        assert!(!executor.args.contains(&"--agent".to_string()));
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
        assert!(!provider.clears_turn_error(&v2));
    }
}
