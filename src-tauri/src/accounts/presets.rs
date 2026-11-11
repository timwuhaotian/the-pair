//! Static vendor presets: display names, model metadata and pricing.
//!
//! A preset is matched by the host of a provider's `base_url`, so an endpoint
//! imported from kimi and one pasted by hand land on the same metadata. Prices
//! are USD per million tokens; models with no published price stay `None` and
//! the UI shows a dash instead of inventing a number.

use super::types::{ModelPrice, ModelRoute, ModelSource, Protocol};
use crate::accounts::types::normalize_base_url;

pub struct ModelSeed {
    pub id: &'static str,
    pub display_name: &'static str,
    pub context_window: u64,
    pub max_output: u64,
    pub efforts: &'static [&'static str],
    pub default_effort: Option<&'static str>,
    /// USD per million input tokens.
    pub input_per_million: Option<f64>,
    /// USD per million output tokens.
    pub output_per_million: Option<f64>,
}

pub struct Preset {
    pub vendor: &'static str,
    pub display_name: &'static str,
    /// Host substrings identifying the endpoint, longest first.
    pub hosts: &'static [&'static str],
    pub protocol: Protocol,
    pub models: &'static [ModelSeed],
}

const NO_EFFORTS: &[&str] = &[];

pub const PRESETS: &[Preset] = &[
    Preset {
        vendor: "anthropic",
        display_name: "Anthropic",
        hosts: &["api.anthropic.com"],
        protocol: Protocol::Anthropic,
        models: &[
            ModelSeed {
                id: "claude-opus-4-1-20250805",
                display_name: "Claude Opus 4.1",
                context_window: 200_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(15.0),
                output_per_million: Some(75.0),
            },
            ModelSeed {
                id: "claude-sonnet-4-5-20250929",
                display_name: "Claude Sonnet 4.5",
                context_window: 200_000,
                max_output: 64_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(3.0),
                output_per_million: Some(15.0),
            },
            ModelSeed {
                id: "claude-haiku-4-5-20251001",
                display_name: "Claude Haiku 4.5",
                context_window: 200_000,
                max_output: 64_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(1.0),
                output_per_million: Some(5.0),
            },
        ],
    },
    Preset {
        vendor: "openai",
        display_name: "OpenAI",
        hosts: &["api.openai.com"],
        protocol: Protocol::Responses,
        models: &[
            ModelSeed {
                id: "gpt-5.1-codex-max",
                display_name: "GPT-5.1 Codex Max",
                context_window: 400_000,
                max_output: 128_000,
                efforts: &["low", "medium", "high", "xhigh"],
                default_effort: Some("medium"),
                input_per_million: Some(1.25),
                output_per_million: Some(10.0),
            },
            ModelSeed {
                id: "gpt-5.1",
                display_name: "GPT-5.1",
                context_window: 400_000,
                max_output: 128_000,
                efforts: &["low", "medium", "high"],
                default_effort: Some("medium"),
                input_per_million: Some(1.25),
                output_per_million: Some(10.0),
            },
            ModelSeed {
                id: "o4-mini",
                display_name: "o4-mini",
                context_window: 200_000,
                max_output: 100_000,
                efforts: &["low", "medium", "high"],
                default_effort: Some("medium"),
                input_per_million: Some(1.1),
                output_per_million: Some(4.4),
            },
        ],
    },
    Preset {
        vendor: "xai",
        display_name: "xAI",
        hosts: &["api.x.ai"],
        protocol: Protocol::OpenAiChat,
        models: &[
            ModelSeed {
                id: "grok-code-fast-1",
                display_name: "Grok Code Fast",
                context_window: 256_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(0.2),
                output_per_million: Some(1.5),
            },
            ModelSeed {
                id: "grok-4-fast",
                display_name: "Grok 4 Fast",
                context_window: 2_000_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(0.2),
                output_per_million: Some(0.5),
            },
        ],
    },
    Preset {
        vendor: "moonshot",
        display_name: "Moonshot (Kimi)",
        hosts: &["api.moonshot.cn", "api.moonshot.ai"],
        protocol: Protocol::OpenAiChat,
        models: &[
            ModelSeed {
                id: "kimi-k2-0905-preview",
                display_name: "Kimi K2",
                context_window: 256_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(0.6),
                output_per_million: Some(2.5),
            },
            ModelSeed {
                id: "kimi-k2-turbo-preview",
                display_name: "Kimi K2 Turbo",
                context_window: 256_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(2.4),
                output_per_million: Some(10.0),
            },
        ],
    },
    Preset {
        vendor: "meta",
        display_name: "Meta (Muse Code)",
        hosts: &["api.meta.ai"],
        protocol: Protocol::Responses,
        models: &[
            ModelSeed {
                id: "muse-wonder-1.0-contributor",
                display_name: "Muse Wonder 1.0 Contributor",
                context_window: 200_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: None,
                output_per_million: None,
            },
            ModelSeed {
                id: "muse-wonder-1.0-full",
                display_name: "Muse Wonder 1.0 Full",
                context_window: 200_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: None,
                output_per_million: None,
            },
        ],
    },
    Preset {
        vendor: "commandcode",
        display_name: "Command Code",
        hosts: &["api.commandcode.ai"],
        protocol: Protocol::CommandCode,
        models: &[ModelSeed {
            id: "command-code-auto",
            display_name: "Command Code Auto",
            context_window: 200_000,
            max_output: 32_000,
            efforts: &["low", "medium", "high"],
            default_effort: Some("medium"),
            input_per_million: None,
            output_per_million: None,
        }],
    },
    Preset {
        vendor: "opencode",
        display_name: "OpenCode Go",
        hosts: &["opencode.ai"],
        protocol: Protocol::OpenAiChat,
        models: &[
            ModelSeed {
                id: "claude-sonnet-4-5",
                display_name: "Claude Sonnet 4.5 (Go)",
                context_window: 200_000,
                max_output: 64_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: None,
                output_per_million: None,
            },
            ModelSeed {
                id: "kimi-k2",
                display_name: "Kimi K2 (Go)",
                context_window: 256_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: None,
                output_per_million: None,
            },
        ],
    },
    Preset {
        vendor: "google",
        display_name: "Google (Gemini)",
        hosts: &["generativelanguage.googleapis.com"],
        protocol: Protocol::OpenAiChat,
        models: &[
            ModelSeed {
                id: "gemini-2.5-pro",
                display_name: "Gemini 2.5 Pro",
                context_window: 1_048_576,
                max_output: 65_536,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(1.25),
                output_per_million: Some(10.0),
            },
            ModelSeed {
                id: "gemini-2.5-flash",
                display_name: "Gemini 2.5 Flash",
                context_window: 1_048_576,
                max_output: 65_536,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(0.3),
                output_per_million: Some(2.5),
            },
        ],
    },
    Preset {
        vendor: "openrouter",
        display_name: "OpenRouter",
        hosts: &["openrouter.ai"],
        protocol: Protocol::OpenAiChat,
        models: &[
            ModelSeed {
                id: "anthropic/claude-sonnet-4.5",
                display_name: "Claude Sonnet 4.5",
                context_window: 200_000,
                max_output: 64_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(3.0),
                output_per_million: Some(15.0),
            },
            ModelSeed {
                id: "x-ai/grok-code-fast-1",
                display_name: "Grok Code Fast",
                context_window: 256_000,
                max_output: 32_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(0.2),
                output_per_million: Some(1.5),
            },
        ],
    },
    Preset {
        vendor: "deepseek",
        display_name: "DeepSeek",
        hosts: &["api.deepseek.com"],
        protocol: Protocol::OpenAiChat,
        models: &[
            ModelSeed {
                id: "deepseek-chat",
                display_name: "DeepSeek Chat",
                context_window: 128_000,
                max_output: 8_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(0.28),
                output_per_million: Some(0.42),
            },
            ModelSeed {
                id: "deepseek-reasoner",
                display_name: "DeepSeek Reasoner",
                context_window: 128_000,
                max_output: 64_000,
                efforts: NO_EFFORTS,
                default_effort: None,
                input_per_million: Some(0.28),
                output_per_million: Some(0.42),
            },
        ],
    },
    Preset {
        vendor: "zai",
        display_name: "Z.ai (GLM)",
        hosts: &["api.z.ai", "open.bigmodel.cn"],
        protocol: Protocol::OpenAiChat,
        models: &[ModelSeed {
            id: "glm-4.6",
            display_name: "GLM-4.6",
            context_window: 200_000,
            max_output: 32_000,
            efforts: NO_EFFORTS,
            default_effort: None,
            input_per_million: Some(0.6),
            output_per_million: Some(2.2),
        }],
    },
];

/// The preset whose host matches `base_url`, if any.
pub fn preset_for(protocol: Protocol, base_url: &str) -> Option<&'static Preset> {
    let normalized = normalize_base_url(base_url).to_lowercase();
    let host = normalized
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&normalized);
    let host = host.split('/').next().unwrap_or(host);

    // Exact protocol matches win, then any protocol: a Responses base URL may
    // still be an OpenAI-compatible gateway that only speaks chat completions.
    let mut fallback = None;
    for preset in PRESETS {
        if !preset
            .hosts
            .iter()
            .any(|candidate| host == *candidate || host.ends_with(&format!(".{}", candidate)))
        {
            continue;
        }
        if preset.protocol == protocol {
            return Some(preset);
        }
        if fallback.is_none() {
            fallback = Some(preset);
        }
    }
    fallback
}

/// Preset models as routes, for a provider that has none of its own yet.
pub fn preset_models(protocol: Protocol, base_url: &str) -> Vec<ModelRoute> {
    let Some(preset) = preset_for(protocol, base_url) else {
        return Vec::new();
    };
    preset
        .models
        .iter()
        .map(|seed| ModelRoute {
            id: seed.id.to_string(),
            display_name: seed.display_name.to_string(),
            context_window: Some(seed.context_window),
            max_output: Some(seed.max_output),
            efforts: seed
                .efforts
                .iter()
                .map(|effort| effort.to_string())
                .collect(),
            default_effort: seed.default_effort.map(str::to_string),
            price: seed.input_per_million.zip(seed.output_per_million).map(
                |(input_per_million, output_per_million)| ModelPrice {
                    input_per_million,
                    output_per_million,
                },
            ),
            source: ModelSource::Preset,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_a_known_host() {
        let preset = preset_for(Protocol::Anthropic, "https://api.anthropic.com").unwrap();
        assert_eq!(preset.vendor, "anthropic");
        assert_eq!(preset.protocol, Protocol::Anthropic);
    }

    #[test]
    fn ignores_trailing_slashes_and_paths() {
        assert!(preset_for(Protocol::OpenAiChat, "https://api.x.ai/v1/").is_some());
        assert!(preset_for(Protocol::Anthropic, "https://api.anthropic.com/v1").is_some());
    }

    #[test]
    fn unknown_hosts_have_no_preset() {
        assert!(preset_for(Protocol::OpenAiChat, "https://llm.internal.example/v1").is_none());
    }

    #[test]
    fn protocol_mismatch_falls_back_to_the_host_match() {
        // A chat-completions gateway in front of Anthropic still gets Anthropic
        // metadata, so the UI can name the vendor.
        let preset = preset_for(Protocol::OpenAiChat, "https://api.anthropic.com").unwrap();
        assert_eq!(preset.vendor, "anthropic");
    }

    #[test]
    fn subdomains_match() {
        let preset = preset_for(Protocol::OpenAiChat, "https://gateway.api.x.ai/v1").unwrap();
        assert_eq!(preset.vendor, "xai");
    }

    #[test]
    fn preset_models_carry_pricing_and_efforts() {
        let models = preset_models(Protocol::Responses, "https://api.openai.com/v1");
        let codex = models
            .iter()
            .find(|model| model.id == "gpt-5.1-codex-max")
            .expect("codex preset model");
        assert_eq!(codex.efforts, vec!["low", "medium", "high", "xhigh"]);
        assert_eq!(codex.default_effort.as_deref(), Some("medium"));
        assert!(codex.price.is_some());
        assert_eq!(codex.source, ModelSource::Preset);
    }

    #[test]
    fn subscription_models_have_no_price() {
        let models = preset_models(Protocol::CommandCode, "https://api.commandcode.ai");
        assert!(models.iter().all(|model| model.price.is_none()));
    }

    #[test]
    fn unknown_endpoint_yields_no_models() {
        assert!(preset_models(Protocol::OpenAiChat, "https://llm.internal.example/v1").is_empty());
    }
}
