use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AvailableModel {
    pub provider: ProviderKind,
    pub model_id: String,
    pub display_name: String,
    pub available: bool,
    pub provider_label: String,
    pub source_provider: Option<String>,
    pub source_provider_label: String,
    pub billing_kind: String,
    pub billing_label: String,
    pub access_label: String,
    pub plan_label: Option<String>,
    pub availability_status: String,
    pub availability_reason: Option<String>,
    pub supports_pair_execution: bool,
    pub recommended_roles: Vec<String>,
    pub reasoning_effort_levels: Option<Vec<String>>,
}

/// Normalize a provider slug or family name into its canonical display label.
/// Uses the known brand map first, then falls back to title-casing unknown values.
fn normalize_provider_label(slug: &str) -> String {
    match slug.to_lowercase().as_str() {
        "openai" | "gpt" => "OpenAI".to_string(),
        "anthropic" | "claude" => "Anthropic".to_string(),
        "google" | "gemini" => "Google".to_string(),
        "meta" | "llama" => "Meta".to_string(),
        "mistral" => "Mistral".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "minimax" => "MiniMax".to_string(),
        "opencode" => "OpenCode".to_string(),
        _ => {
            let mut chars = slug.chars();
            match chars.next() {
                None => "Unknown".to_string(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        }
    }
}

fn reasoning_effort_levels_for(provider: ProviderKind, model_id: &str) -> Option<Vec<String>> {
    match provider {
        ProviderKind::Claude => Some(vec!["low".into(), "medium".into(), "high".into()]),
        ProviderKind::Codex => {
            if model_id.starts_with("o3")
                || model_id.starts_with("o4")
                || model_id.starts_with("o1")
            {
                Some(vec!["low".into(), "medium".into(), "high".into()])
            } else {
                None
            }
        }
        ProviderKind::Gemini => {
            if model_id.contains("2.5") || model_id.contains("2.0-flash") {
                Some(vec![
                    "none".into(),
                    "low".into(),
                    "medium".into(),
                    "high".into(),
                ])
            } else {
                None
            }
        }
        ProviderKind::Opencode => None,
    }
}

pub struct ModelCatalog;

impl ModelCatalog {
    pub fn build_catalog(profiles: Vec<DetectedProviderProfile>) -> Vec<AvailableModel> {
        let mut catalog = Vec::new();
        let native_source_labels: HashSet<String> = profiles
            .iter()
            .filter(|profile| {
                !profile.current_models.is_empty()
                    && !matches!(profile.kind, ProviderKind::Opencode)
            })
            .map(|profile| match profile.kind {
                ProviderKind::Codex => "OpenAI".to_string(),
                ProviderKind::Claude => "Anthropic".to_string(),
                ProviderKind::Gemini => "Google".to_string(),
                ProviderKind::Opencode => unreachable!(),
            })
            .collect();

        for profile in profiles {
            let provider_label = match profile.kind {
                ProviderKind::Opencode => "OpenCode",
                ProviderKind::Codex => "Codex",
                ProviderKind::Claude => "Claude Code",
                ProviderKind::Gemini => "Gemini CLI",
            };

            for model in profile.current_models {
                // Use family field for display if available (for OpenCode models),
                // otherwise fall back to source_provider. Both use the canonical label map.
                let source_provider_label = if let Some(ref family) = model.family {
                    normalize_provider_label(family)
                } else if let Some(ref provider) = model.source_provider {
                    normalize_provider_label(provider)
                } else {
                    "Configured Provider".to_string()
                };

                if matches!(profile.kind, ProviderKind::Opencode)
                    && native_source_labels.contains(&source_provider_label)
                {
                    continue;
                }

                let (status, reason, available) = if !profile.installed {
                    (
                        "cli-missing".to_string(),
                        Some(format!("{} CLI is not installed", provider_label)),
                        false,
                    )
                } else if !profile.authenticated {
                    (
                        "auth-missing".to_string(),
                        Some(format!("{} is not signed in", provider_label)),
                        false,
                    )
                } else if !profile.runnable || !model.runnable || !model.supports_pair_execution {
                    (
                        "runtime-unsupported".to_string(),
                        Some(format!(
                            "{} is detected, but pair execution is not yet supported",
                            provider_label
                        )),
                        false,
                    )
                } else {
                    ("ready".to_string(), None, true)
                };

                let billing_kind = match profile.kind {
                    ProviderKind::Opencode => "byok",
                    _ => "plan",
                };

                let billing_label = match profile.kind {
                    ProviderKind::Opencode => "Pay as you go",
                    _ => "Included with plan",
                };

                let access_label = match profile.kind {
                    ProviderKind::Opencode => format!("{} API key", source_provider_label),
                    ProviderKind::Codex => "ChatGPT plan".into(),
                    ProviderKind::Claude => "Claude Code login".into(),
                    ProviderKind::Gemini => "Google account".into(),
                };

                let entry = AvailableModel {
                    provider: profile.kind,
                    model_id: model.model_id.clone(),
                    display_name: model.display_name,
                    available,
                    provider_label: provider_label.to_string(),
                    source_provider: model.source_provider,
                    source_provider_label: source_provider_label.to_string(),
                    billing_kind: billing_kind.to_string(),
                    billing_label: billing_label.to_string(),
                    access_label,
                    plan_label: Some(profile.subscription_label.clone()),
                    availability_status: status,
                    availability_reason: reason,
                    supports_pair_execution: model.supports_pair_execution,
                    recommended_roles: vec!["mentor".into(), "executor".into()],
                    reasoning_effort_levels: reasoning_effort_levels_for(
                        profile.kind,
                        &model.model_id,
                    ),
                };
                catalog.push(entry);
            }
        }

        catalog.sort_by(|a, b| {
            if a.available != b.available {
                return b.available.cmp(&a.available);
            }
            a.provider_label.cmp(&b.provider_label)
        });

        catalog
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_registry::{DetectedModelOption, DetectedProviderProfile, ProviderKind};

    fn model(
        model_id: &str,
        display_name: &str,
        source_provider: Option<&str>,
        family: Option<&str>,
        subscription_label: &str,
        supports_pair_execution: bool,
        runnable: bool,
    ) -> DetectedModelOption {
        DetectedModelOption {
            model_id: model_id.to_string(),
            display_name: display_name.to_string(),
            source_provider: source_provider.map(|value| value.to_string()),
            family: family.map(|value| value.to_string()),
            subscription_label: subscription_label.to_string(),
            supports_pair_execution,
            runnable,
        }
    }

    fn profile(
        kind: ProviderKind,
        installed: bool,
        authenticated: bool,
        runnable: bool,
        subscription_label: &str,
        current_models: Vec<DetectedModelOption>,
    ) -> DetectedProviderProfile {
        DetectedProviderProfile {
            kind,
            installed,
            authenticated,
            runnable,
            subscription_label: subscription_label.to_string(),
            current_models,
            detected_at: 0,
        }
    }

    #[test]
    fn build_catalog_marks_supported_opencode_models_ready() {
        let catalog = ModelCatalog::build_catalog(vec![profile(
            ProviderKind::Opencode,
            true,
            true,
            true,
            "provider-backed",
            vec![model(
                "openai/gpt-4o-mini",
                "GPT-4o Mini",
                Some("openai"),
                None,
                "provider-backed",
                true,
                true,
            )],
        )]);

        assert_eq!(catalog.len(), 1);
        let model = &catalog[0];
        assert!(model.available);
        assert_eq!(model.provider_label, "OpenCode");
        assert_eq!(model.billing_kind, "byok");
        assert_eq!(model.billing_label, "Pay as you go");
        assert_eq!(model.access_label, "OpenAI API key");
        assert_eq!(model.availability_status, "ready");
        assert_eq!(model.recommended_roles, vec!["mentor", "executor"]);
        assert!(model.reasoning_effort_levels.is_none());
    }

    #[test]
    fn build_catalog_keeps_unavailable_models_visible_and_sorted_after_ready_models() {
        let catalog = ModelCatalog::build_catalog(vec![
            profile(
                ProviderKind::Opencode,
                false,
                true,
                true,
                "provider-backed",
                vec![model(
                    "openai/gpt-4o-mini",
                    "GPT-4o Mini",
                    Some("openai"),
                    None,
                    "provider-backed",
                    true,
                    true,
                )],
            ),
            profile(
                ProviderKind::Claude,
                true,
                true,
                true,
                "pro",
                vec![model(
                    "claude-3-5-sonnet",
                    "Claude 3.5 Sonnet",
                    Some("anthropic"),
                    None,
                    "pro",
                    true,
                    true,
                )],
            ),
        ]);

        assert_eq!(catalog.len(), 2);
        assert!(catalog[0].available, "ready models should sort first");
        assert_eq!(catalog[0].provider_label, "Claude Code");
        assert!(!catalog[1].available);
        assert_eq!(catalog[1].availability_status, "cli-missing");
        assert!(catalog[1]
            .availability_reason
            .as_deref()
            .unwrap_or_default()
            .contains("OpenCode CLI is not installed"));
    }

    #[test]
    fn build_catalog_prefers_native_provider_over_opencode_duplicate() {
        let catalog = ModelCatalog::build_catalog(vec![
            profile(
                ProviderKind::Opencode,
                true,
                true,
                true,
                "pay-as-you-go",
                vec![model(
                    "opencode/claude-sonnet-4-20250514",
                    "Claude Sonnet 4",
                    Some("anthropic"),
                    Some("claude"),
                    "pay-as-you-go",
                    true,
                    true,
                )],
            ),
            profile(
                ProviderKind::Claude,
                true,
                true,
                true,
                "subscription-backed",
                vec![model(
                    "claude-sonnet-4-20250514",
                    "Claude Sonnet 4",
                    Some("anthropic"),
                    None,
                    "subscription-backed",
                    true,
                    true,
                )],
            ),
        ]);

        assert_eq!(
            catalog.len(),
            1,
            "native Claude rows should replace OpenCode duplicates"
        );
        assert_eq!(catalog[0].provider, ProviderKind::Claude);
        assert_eq!(catalog[0].provider_label, "Claude Code");
        assert_eq!(catalog[0].source_provider_label, "Anthropic");
        assert_eq!(catalog[0].model_id, "claude-sonnet-4-20250514");
    }

    #[test]
    fn build_catalog_displays_minimax_models_with_correct_source_provider_label() {
        let catalog = ModelCatalog::build_catalog(vec![profile(
            ProviderKind::Opencode,
            true,
            true,
            true,
            "pay-as-you-go",
            vec![
                model(
                    "opencode/minimax-m2.5",
                    "MiniMax M2.5",
                    Some("opencode"),
                    Some("minimax"),
                    "pay-as-you-go",
                    true,
                    true,
                ),
                model(
                    "opencode/claude-3-5-sonnet",
                    "Claude 3.5 Sonnet",
                    Some("opencode"),
                    Some("claude"),
                    "pay-as-you-go",
                    true,
                    true,
                ),
            ],
        )]);

        assert_eq!(catalog.len(), 2);

        let minimax_model = catalog
            .iter()
            .find(|m| m.model_id == "opencode/minimax-m2.5")
            .expect("Minimax model should be in catalog");
        assert!(minimax_model.available);
        assert_eq!(minimax_model.provider_label, "OpenCode");
        assert_eq!(
            minimax_model.source_provider_label, "MiniMax",
            "Minimax models should display 'MiniMax' with correct brand casing"
        );
        assert_eq!(minimax_model.display_name, "MiniMax M2.5");
        assert_eq!(minimax_model.availability_status, "ready");
        assert_eq!(
            minimax_model.access_label, "MiniMax API key",
            "access_label should use the normalized brand name"
        );

        let claude_model = catalog
            .iter()
            .find(|m| m.model_id == "opencode/claude-3-5-sonnet")
            .expect("Claude model should be in catalog");
        assert!(claude_model.available);
        assert_eq!(claude_model.provider_label, "OpenCode");
        assert_eq!(
            claude_model.source_provider_label, "Anthropic",
            "Claude family should normalize to Anthropic (Claude is Anthropic's model family)"
        );
    }

    #[test]
    fn build_catalog_normalizes_brand_casing_for_family_and_source_provider() {
        let catalog = ModelCatalog::build_catalog(vec![profile(
            ProviderKind::Opencode,
            true,
            true,
            true,
            "pay-as-you-go",
            vec![
                // Test family field with various brand casings
                model(
                    "opencode/gpt-4o",
                    "GPT-4o",
                    Some("opencode"),
                    Some("gpt"), // family: gpt -> should normalize to OpenAI (runtime shape)
                    "pay-as-you-go",
                    true,
                    true,
                ),
                model(
                    "opencode/deepseek-chat",
                    "DeepSeek Chat",
                    Some("opencode"),
                    Some("deepseek"), // family: deepseek -> should normalize to DeepSeek
                    "pay-as-you-go",
                    true,
                    true,
                ),
                model(
                    "opencode/gemini-2.5-pro",
                    "Gemini 2.5 Pro",
                    Some("opencode"),
                    Some("gemini"), // family: gemini -> should normalize to Google
                    "pay-as-you-go",
                    true,
                    true,
                ),
                model(
                    "opencode/unknown-model",
                    "Unknown Model",
                    Some("opencode"),
                    Some("unknownvendor"), // family: unknown -> title-cased fallback
                    "pay-as-you-go",
                    true,
                    true,
                ),
            ],
        )]);

        let gpt_model = catalog
            .iter()
            .find(|m| m.model_id == "opencode/gpt-4o")
            .expect("GPT model should be in catalog");
        assert_eq!(
            gpt_model.source_provider_label, "OpenAI",
            "gpt family should normalize to OpenAI (not GPT)"
        );
        assert_eq!(gpt_model.access_label, "OpenAI API key");

        let deepseek_model = catalog
            .iter()
            .find(|m| m.model_id == "opencode/deepseek-chat")
            .expect("DeepSeek model should be in catalog");
        assert_eq!(
            deepseek_model.source_provider_label, "DeepSeek",
            "deepseek family should normalize to DeepSeek (not Deepseek)"
        );

        let gemini_model = catalog
            .iter()
            .find(|m| m.model_id == "opencode/gemini-2.5-pro")
            .expect("Gemini model should be in catalog");
        assert_eq!(
            gemini_model.source_provider_label, "Google",
            "gemini family should normalize to Google"
        );

        let unknown_model = catalog
            .iter()
            .find(|m| m.model_id == "opencode/unknown-model")
            .expect("Unknown model should be in catalog");
        assert_eq!(
            unknown_model.source_provider_label, "Unknownvendor",
            "unknown family should fall back to title-casing"
        );
    }
}
