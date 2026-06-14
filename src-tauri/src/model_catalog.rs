use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    /// Stable identity used to merge the same model across routes (providers/plans).
    /// Two rows collapse onto one model picker entry iff this matches exactly.
    pub canonical_key: String,
    /// Display name with any baked-in effort suffix removed (e.g. "Gemini 3.5 Flash").
    pub canonical_display_name: String,
    /// Reasoning effort baked into this row's name by the provider (Antigravity), if any.
    pub effort_tag: Option<String>,
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
    crate::providers::provider_for_kind(provider).reasoning_effort_levels(model_id)
}

/// Split a trailing reasoning-effort suffix (e.g. "Gemini 3.5 Flash (Low)") from a
/// model name. Returns the base name plus the canonical effort tag when the suffix is a
/// recognized effort word. Only a small fixed vocabulary is matched so unrelated
/// parenthetical suffixes (e.g. "(Preview)") are left untouched — an under-merge bias.
fn split_effort_suffix(name: &str) -> (String, Option<String>) {
    let trimmed = name.trim();
    if trimmed.ends_with(')') {
        if let Some(open) = trimmed.rfind('(') {
            let inside = trimmed[open + 1..trimmed.len() - 1].trim().to_lowercase();
            let effort = match inside.as_str() {
                "low" => Some("low"),
                "medium" => Some("medium"),
                "high" | "thinking" => Some("high"),
                _ => None,
            };
            if let Some(effort) = effort {
                let base = trimmed[..open].trim().to_string();
                if !base.is_empty() {
                    return (base, Some(effort.to_string()));
                }
            }
        }
    }
    (trimmed.to_string(), None)
}

/// The brand a model belongs to, used as the high-order part of the canonical key.
/// Native providers map to their fixed brand; OpenCode rides on the resolved source
/// label so an OpenCode "openai/*" model keys to the same brand as native Codex.
fn brand_for_key(kind: ProviderKind, source_provider_label: &str) -> String {
    let provider = crate::providers::provider_for_kind(kind);
    let brand = provider.brand();
    if brand == "opencode" {
        // OpenCode rides on the resolved source label so an OpenCode "openai/*"
        // model keys to the same brand as native Codex.
        source_provider_label.to_lowercase()
    } else {
        brand.to_string()
    }
}

/// Normalize a model identity into a stable token string: drop the provider prefix,
/// split on non-alphanumerics, and drop a trailing date stamp (>= 6 digits). This makes
/// "anthropic/claude-sonnet-4-5" and "claude-sonnet-4-5-20250929" collapse, while keeping
/// genuinely different versions (4-5 vs 4-6) apart.
fn normalize_model_base(base_name: &str) -> String {
    let without_prefix = base_name.rsplit('/').next().unwrap_or(base_name);
    let lowered = without_prefix.to_lowercase();
    let mut tokens: Vec<&str> = lowered
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    if let Some(last) = tokens.last() {
        if last.len() >= 6 && last.chars().all(|c| c.is_ascii_digit()) {
            tokens.pop();
        }
    }
    tokens.join("-")
}

/// Build the canonical merge key: `brand::normalized-model`. Two routes collapse onto
/// the same model row iff this key matches exactly.
fn compute_canonical_key(
    kind: ProviderKind,
    source_provider_label: &str,
    id_base: &str,
) -> String {
    format!(
        "{}::{}",
        brand_for_key(kind, source_provider_label),
        normalize_model_base(id_base)
    )
}

pub struct ModelCatalog;

impl ModelCatalog {
    pub fn build_catalog(profiles: Vec<DetectedProviderProfile>) -> Vec<AvailableModel> {
        let mut catalog = Vec::new();

        for profile in profiles {
            let provider = crate::providers::provider_for_kind(profile.kind);
            let provider_label = provider.provider_label();

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

                // OpenCode mirrors the entire models.dev catalog plus any provider the
                // user has a key for. The unauthenticated long tail would bury the native
                // providers, so keep only OpenCode routes the user can actually run.
                if provider.should_filter_unavailable_models() && !available {
                    continue;
                }

                let billing_kind = provider.billing_kind();
                let billing_label = provider.billing_label();
                let access_label = provider.access_label(&source_provider_label);

                // Identity for cross-route merging. Antigravity bakes the reasoning effort
                // into the model name ("Gemini 3.5 Flash (Low)"); strip it so every effort
                // variant collapses onto one canonical model, and tag the effort so the UI
                // can offer it as a sub-control.
                let (id_base, effort_from_id) = split_effort_suffix(&model.model_id);
                let (display_base, effort_from_display) = split_effort_suffix(&model.display_name);
                let effort_tag = effort_from_display.or(effort_from_id);
                let canonical_key =
                    compute_canonical_key(profile.kind, &source_provider_label, &id_base);

                // Normalize display name (e.g. Claude beautifier).
                let normalized_display = provider.normalize_model_display_name(&model.display_name);

                let entry = AvailableModel {
                    provider: profile.kind,
                    model_id: model.model_id.clone(),
                    display_name: normalized_display,
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
                    canonical_key,
                    canonical_display_name: display_base,
                    effort_tag,
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
            login_command: None,
            install_url: None,
            detected_at: 0,
        }
    }

    #[test]
    fn reasoning_effort_levels_only_offered_for_codex_o_series() {
        // Claude Code and Gemini CLI have no reasoning-effort CLI flag, so the control
        // must be hidden (None) to avoid crashing the turn (Claude) or a silent no-op
        // (Gemini). Opencode delegates to the underlying model. Only Codex o-series
        // honors reasoning via `-c model_reasoning_effort=`.
        assert_eq!(
            reasoning_effort_levels_for(ProviderKind::Claude, "claude-sonnet-4-6"),
            None
        );
        assert_eq!(
            reasoning_effort_levels_for(ProviderKind::Gemini, "gemini-2.5-pro"),
            None
        );
        assert_eq!(
            reasoning_effort_levels_for(ProviderKind::Opencode, "openai/gpt-4o"),
            None
        );
        assert_eq!(
            reasoning_effort_levels_for(ProviderKind::Codex, "gpt-4o"),
            None
        );
        let levels =
            reasoning_effort_levels_for(ProviderKind::Codex, "o3").expect("o3 offers reasoning");
        assert_eq!(levels, vec!["low", "medium", "high"]);
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
    fn build_catalog_keeps_unavailable_native_models_visible_and_sorted_after_ready_models() {
        // Native providers keep their unavailable rows so the user sees *why* an installed
        // CLI is not yet usable; only OpenCode's unauthenticated long tail is filtered out.
        let catalog = ModelCatalog::build_catalog(vec![
            profile(
                ProviderKind::Codex,
                false,
                true,
                true,
                "subscription-backed",
                vec![model(
                    "gpt-5",
                    "GPT-5",
                    Some("openai"),
                    None,
                    "subscription-backed",
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
            .contains("Codex CLI is not installed"));
    }

    #[test]
    fn build_catalog_keeps_both_routes_with_shared_canonical_key() {
        // The same model reachable via a native CLI and via OpenCode is no longer dropped;
        // both survive as separate routes sharing one canonical key, so the UI can merge
        // them into a single model entry with a route sub-picker.
        let catalog = ModelCatalog::build_catalog(vec![
            profile(
                ProviderKind::Opencode,
                true,
                true,
                true,
                "pay-as-you-go",
                vec![model(
                    "anthropic/claude-sonnet-4-20250514",
                    "Claude Sonnet 4",
                    Some("anthropic"),
                    None,
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

        assert_eq!(catalog.len(), 2, "both routes survive (no brand dedup)");
        let claude = catalog
            .iter()
            .find(|m| m.provider == ProviderKind::Claude)
            .expect("native Claude route present");
        let opencode = catalog
            .iter()
            .find(|m| m.provider == ProviderKind::Opencode)
            .expect("OpenCode route present");
        assert_eq!(
            claude.canonical_key, opencode.canonical_key,
            "same model via different routes shares one canonical key"
        );
        assert_eq!(claude.canonical_key, "anthropic::claude-sonnet-4");
    }

    #[test]
    fn build_catalog_collapses_antigravity_effort_variants() {
        // Antigravity bakes effort into the model name; the Gemini rows must share a
        // canonical key and surface their effort as a tag with a clean display name.
        let catalog = ModelCatalog::build_catalog(vec![profile(
            ProviderKind::Gemini,
            true,
            true,
            true,
            "antigravity-backed",
            vec![
                model(
                    "Gemini 3.5 Flash (Low)",
                    "Gemini 3.5 Flash (Low)",
                    Some("google"),
                    Some("gemini"),
                    "antigravity-backed",
                    true,
                    true,
                ),
                model(
                    "Gemini 3.5 Flash (Medium)",
                    "Gemini 3.5 Flash (Medium)",
                    Some("google"),
                    Some("gemini"),
                    "antigravity-backed",
                    true,
                    true,
                ),
                model(
                    "Gemini 3.5 Flash (High)",
                    "Gemini 3.5 Flash (High)",
                    Some("google"),
                    Some("gemini"),
                    "antigravity-backed",
                    true,
                    true,
                ),
            ],
        )]);

        assert_eq!(catalog.len(), 3);
        let keys: std::collections::HashSet<&str> =
            catalog.iter().map(|m| m.canonical_key.as_str()).collect();
        assert_eq!(keys.len(), 1, "all effort variants share one canonical key");
        assert_eq!(catalog[0].canonical_key, "google::gemini-3-5-flash");
        for entry in &catalog {
            assert_eq!(entry.canonical_display_name, "Gemini 3.5 Flash");
        }
        let mut efforts: Vec<&str> = catalog
            .iter()
            .filter_map(|m| m.effort_tag.as_deref())
            .collect();
        efforts.sort_unstable();
        assert_eq!(efforts, vec!["high", "low", "medium"]);
    }

    #[test]
    fn build_catalog_drops_unauthenticated_opencode_long_tail() {
        // OpenCode mirrors all of models.dev; rows the user cannot run (provider not
        // authenticated) are filtered out so they don't bury the native providers.
        let catalog = ModelCatalog::build_catalog(vec![profile(
            ProviderKind::Opencode,
            true,
            false,
            true,
            "multi-provider",
            vec![model(
                "deepseek/deepseek-chat",
                "DeepSeek Chat",
                Some("deepseek"),
                None,
                "multi-provider",
                true,
                true,
            )],
        )]);

        assert!(
            catalog.is_empty(),
            "unauthenticated OpenCode models are filtered out"
        );
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
