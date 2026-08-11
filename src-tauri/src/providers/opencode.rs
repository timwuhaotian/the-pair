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
    supports_variant_flag: bool,
) -> ProviderTurnCommand {
    let mut args = vec!["run".into(), "--model".into(), request.model.into()];
    if supports_variant_flag {
        if let (Some(variants), Some(effort)) = (
            reasoning_variants_for_model(request.model),
            request.reasoning_effort,
        ) {
            if let Some(variant) = variants.iter().find(|variant| variant.effort == effort) {
                args.push("--variant".into());
                args.push(variant.cli_variant.into());
            }
        }
    }
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
            crate::provider_registry::opencode_supports_variant_flag(),
        )
    }

    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage> {
        // OpenCode emits message.updated events with part.type = "step-finish"
        // The tokens are at event.part.tokens.{input, output, total}
        if let Some(part) = event.get("part") {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if part_type == "step-finish" || part_type == "step_finish" {
                if let Some(tokens) = part.get("tokens") {
                    let output_tokens = tokens
                        .get("output")
                        .or_else(|| tokens.get("completionTokens"))
                        .or_else(|| tokens.get("completion_tokens"))
                        .and_then(|v| v.as_u64());

                    let input_tokens = tokens
                        .get("input")
                        .or_else(|| tokens.get("promptTokens"))
                        .or_else(|| tokens.get("prompt_tokens"));

                    if let Some(output) = output_tokens {
                        let input_val = input_tokens.and_then(|v| v.as_u64());
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

    #[test]
    fn opencode_command_ignores_unsupported_reasoning_effort() {
        let command = build_opencode_turn_command(
            &ProviderTurnRequest {
                provider_kind: ProviderKind::Opencode,
                model: "example/model",
                session_id: None,
                role: "executor",
                pair_id: "pair-1",
                message: "do the work",
                reasoning_effort: Some("high"),
            },
            true,
        );

        assert!(!command.args.contains(&"--reasoning-effort".to_string()));
        assert!(!command.args.contains(&"high".to_string()));
    }

    #[test]
    fn opencode_command_maps_reasoning_effort_to_cli_variant() {
        for (effort, expected_variant) in [("adaptive", "thinking"), ("disabled", "none")] {
            let command = build_opencode_turn_command(
                &ProviderTurnRequest {
                    provider_kind: ProviderKind::Opencode,
                    model: "minimax-cn/MiniMax-M3",
                    session_id: None,
                    role: "executor",
                    pair_id: "pair-1",
                    message: "do the work",
                    reasoning_effort: Some(effort),
                },
                true,
            );

            let variant_index = command
                .args
                .iter()
                .position(|arg| arg == "--variant")
                .expect("supported reasoning should use the OpenCode variant flag");
            assert_eq!(command.args[variant_index + 1], expected_variant);
        }
    }

    #[test]
    fn opencode_command_omits_variant_for_older_cli() {
        let command = build_opencode_turn_command(
            &ProviderTurnRequest {
                provider_kind: ProviderKind::Opencode,
                model: "minimax/MiniMax-M3",
                session_id: None,
                role: "executor",
                pair_id: "pair-1",
                message: "do the work",
                reasoning_effort: Some("adaptive"),
            },
            false,
        );

        assert!(!command.args.contains(&"--variant".to_string()));
        assert!(!command.args.contains(&"thinking".to_string()));
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
}
