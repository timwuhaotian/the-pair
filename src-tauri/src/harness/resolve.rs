//! Mapping a pair's stored model choice onto a registry selection.
//!
//! Pairs created before the registry existed name their models the way the
//! provider CLIs did (`claude:opus`, `grok:fast`). This resolves such a string
//! onto a (provider, account, model) triple the harness can talk to, so an
//! existing pair moves onto the direct connection as soon as the machine has a
//! credential for its endpoint.

use super::ModelSelection;
use crate::accounts::presets::{preset_for, PRESETS};
use crate::accounts::store::load as load_providers;
use crate::accounts::types::{normalize_base_url, AccountStatus, Provider};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

/// The registry selection a model id resolves onto by itself, with no pick to
/// go by: what [`selection_for_role`] falls back to.
#[cfg(test)]
fn selection_for_model(app_dir: &Path, model: &str) -> Option<ModelSelection> {
    let wanted = model_id(model);
    if wanted.is_empty() {
        return None;
    }
    resolve_among(&load_providers(app_dir), &wanted)
}

/// The selection for one role's model: the endpoint the human picked for it,
/// when that endpoint can still serve it, otherwise the one the id resolves
/// onto.
///
/// Resolution is deliberately conservative: an account with no credential is
/// never chosen, and a model the endpoint does not list is only used when the
/// endpoint has nothing better to say about itself.
///
/// A model id alone does not say which endpoint it is on: a ChatGPT plan and an
/// OpenAI API key both list `gpt-5-codex`, and inferring from the id would bill
/// whichever comes first in the registry. The picker reports the endpoint a
/// model was picked under ([`prefer_route`]), and that is what decides here.
pub fn selection_for_role(app_dir: &Path, role: &str, model: &str) -> Option<ModelSelection> {
    let wanted = model_id(model);
    if wanted.is_empty() {
        return None;
    }
    let providers = load_providers(app_dir);
    preferred_among(app_dir, &providers, role, &wanted)
        .or_else(|| resolve_among(&providers, &wanted))
}

/// The selection the human's own pick names for a role's model, and nothing
/// else: `None` when no endpoint was picked for it, or the one that was can no
/// longer serve it.
pub fn preferred_selection(app_dir: &Path, role: &str, model: &str) -> Option<ModelSelection> {
    let wanted = model_id(model);
    if wanted.is_empty() {
        return None;
    }
    preferred_among(app_dir, &load_providers(app_dir), role, &wanted)
}

/// The selection a role carries into its next turn: the one it already has,
/// while that still names this model on an endpoint with a ready account,
/// otherwise a fresh [`selection_for_role`].
///
/// A conversation belongs to the endpoint it was had with, so a pair must not
/// drift onto another one because the registry was re-scanned into a different
/// order between two handoffs — that would bill another account and start the
/// role's conversation over.
pub fn keep_or_resolve(
    app_dir: &Path,
    role: &str,
    model: &str,
    current: Option<&ModelSelection>,
) -> Option<ModelSelection> {
    let wanted = model_id(model);
    if let Some(current) = current {
        if current.model.eq_ignore_ascii_case(&wanted)
            && still_ready(&load_providers(app_dir), current)
        {
            return Some(current.clone());
        }
    }
    selection_for_role(app_dir, role, model)
}

/// Whether the endpoint and the account a selection names are still in the
/// registry with a usable credential.
fn still_ready(providers: &[Provider], selection: &ModelSelection) -> bool {
    providers
        .iter()
        .find(|provider| provider.id == selection.provider_id)
        .and_then(|provider| {
            provider
                .accounts
                .iter()
                .find(|account| account.id == selection.account_id)
        })
        .is_some_and(|account| account.status == AccountStatus::Ready)
}

fn resolve_among(providers: &[Provider], wanted: &str) -> Option<ModelSelection> {
    let vendor = vendor_hint(wanted);

    // 1. The endpoint this model id is known to live behind. The hint only
    //    orders the search: a `gpt-*` id behind an Anthropic endpoint is a
    //    mismatch, so the vendor's own endpoints — and the presets that host it —
    //    are tried before anything else.
    if let Some(vendor) = vendor {
        if let Some(selection) = pick(providers, wanted, |provider| {
            provider.vendor == vendor || host_matches_preset(provider, vendor)
        }) {
            return Some(selection);
        }
    }

    // 2. Any endpoint that lists exactly this model. An endpoint that lists
    //    nothing at all cannot contradict the id, so it may serve it. A gateway
    //    or a proxy — `ANTHROPIC_BASE_URL` from the claude importer, OpenRouter,
    //    a hand-added endpoint — is registered under its own host as the vendor,
    //    so the hint cannot describe it; what it lists is the only evidence
    //    there is, and an endpoint that names the id wins.
    pick(providers, wanted, |_| true)
}

// ---------------------------------------------------------------------------
// The endpoint a model was picked under
// ---------------------------------------------------------------------------

/// Where the picked endpoints are kept, next to the credential store. It holds
/// ids only — no secret — so it is a plain file.
const ROUTES_FILE: &str = "model_routes.json";

/// `role` → model id → the provider id it was last picked under.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PickedRoutes {
    #[serde(default)]
    mentor: BTreeMap<String, String>,
    #[serde(default)]
    executor: BTreeMap<String, String>,
}

impl PickedRoutes {
    fn of(&self, role: &str) -> Option<&BTreeMap<String, String>> {
        match role {
            "mentor" => Some(&self.mentor),
            "executor" => Some(&self.executor),
            _ => None,
        }
    }
}

/// Serializes the read-modify-write of the routes file.
static ROUTES_LOCK: Mutex<()> = Mutex::new(());

fn read_routes(app_dir: &Path) -> PickedRoutes {
    std::fs::read_to_string(app_dir.join(ROUTES_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Records the endpoint the human picked `model` under for `role`, so the
/// pair's turns talk to that endpoint and not to whichever lists the id first.
pub fn prefer_route(
    app_dir: &Path,
    role: &str,
    model: &str,
    provider_id: &str,
) -> Result<(), String> {
    let wanted = model_id(model);
    let provider_id = provider_id.trim();
    if wanted.is_empty() || provider_id.is_empty() {
        return Err("A picked route needs a model and an endpoint.".to_string());
    }
    let _guard = ROUTES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut routes = read_routes(app_dir);
    let picked = match role {
        "mentor" => &mut routes.mentor,
        "executor" => &mut routes.executor,
        other => return Err(format!("Unknown role `{}`.", other)),
    };
    if picked.get(&wanted).map(String::as_str) == Some(provider_id) {
        return Ok(());
    }
    picked.insert(wanted, provider_id.to_string());
    let text = serde_json::to_string_pretty(&routes)
        .map_err(|error| format!("Failed to serialize the picked routes: {}", error))?;
    crate::accounts::store::write_private(&app_dir.join(ROUTES_FILE), text.as_bytes())
}

/// The picked endpoint's selection for `wanted`, when it has a ready account and
/// either lists the model or lists nothing at all.
fn preferred_among(
    app_dir: &Path,
    providers: &[Provider],
    role: &str,
    wanted: &str,
) -> Option<ModelSelection> {
    let routes = read_routes(app_dir);
    let preferred = routes.of(role)?.get(wanted)?;
    pick(providers, wanted, |provider| &provider.id == preferred)
}

/// Prefixes a pair's stored model id may carry. A pair used to hold one id per
/// agent CLI, so these have to be recognised to read legacy snapshots back.
///
/// Gateway path segments (`anthropic/`, `openai/`, `openrouter/`) are
/// deliberately absent: they are how a gateway names a model in its own catalog,
/// and stripping one would resolve the model onto the wrong endpoint. This set
/// mirrors `QUALIFIER_TOKENS` in the renderer's `providerResolution.ts`.
const QUALIFIERS: &[&str] = &[
    "aider", "claude", "codex", "gemini", "grok", "kimi", "kiro", "muse", "opencode", "pi",
];

/// `claude:opus-4-1` and `claude/opus-4-1` -> `opus-4-1`. Anything else — a
/// gateway route such as `anthropic/claude-3-5-sonnet`, say — is left whole,
/// because the path is part of what the id names.
fn model_id(raw: &str) -> String {
    let trimmed = raw.trim();
    let (prefix, bare) = match trimmed.split_once(':').or_else(|| trimmed.split_once('/')) {
        Some(parts) => parts,
        None => return trimmed.to_ascii_lowercase(),
    };
    let known = QUALIFIERS
        .iter()
        .any(|qualifier| prefix.eq_ignore_ascii_case(qualifier));
    let looks_like_id = !bare.is_empty()
        && bare
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '+' | '-'));
    match known && looks_like_id {
        true => bare.trim().to_ascii_lowercase(),
        false => trimmed.to_ascii_lowercase(),
    }
}

/// Which vendor a model id is known to live behind, when the id says so.
///
/// Only the two vendors whose endpoints are unambiguous by protocol: a
/// `claude-*` id is Anthropic's Messages API, a `gpt-*`/`codex-*`/`o*` id is
/// OpenAI's Responses API. Everything else is found by the endpoints that list
/// it, which is how a gateway or a proxy shows up correctly.
///
/// The hint orders the search rather than filtering it: an endpoint that lists
/// the id is trusted over it, because a gateway serving a `claude-*` id is not
/// a mismatch.
fn vendor_hint(model: &str) -> Option<&'static str> {
    let protocol = if model.starts_with("claude") || model.contains("anthropic") {
        crate::accounts::types::Protocol::Anthropic
    } else if model.starts_with("gpt")
        || model.starts_with("codex")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        crate::accounts::types::Protocol::Responses
    } else {
        return None;
    };
    preset_for(protocol, protocol.default_base_url()).map(|preset| preset.vendor)
}

/// Whether a provider's endpoint belongs to a preset vendor.
fn host_matches_preset(provider: &Provider, vendor: &str) -> bool {
    PRESETS.iter().any(|preset| {
        preset.vendor == vendor
            && preset
                .hosts
                .iter()
                .any(|host| host_of(&provider.base_url) == *host)
    })
}

fn host_of(base_url: &str) -> String {
    let normalized = normalize_base_url(base_url).to_lowercase();
    let without_scheme = normalized
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&normalized);
    without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split(':')
        .next()
        .unwrap_or(without_scheme)
        .to_string()
}

/// The provider matching `accepts` that can serve `wanted`.
///
/// An endpoint that lists the model wins, and keeps its own spelling of the id.
/// An endpoint that lists models but not this one is skipped: the pair's id is
/// an alias the endpoint never confirmed, and sending it would fail. An endpoint
/// that lists nothing at all cannot contradict the id, so it may serve it.
fn pick(
    providers: &[Provider],
    wanted: &str,
    accepts: impl Fn(&Provider) -> bool,
) -> Option<ModelSelection> {
    let ready = || {
        providers
            .iter()
            .filter(|provider| accepts(provider))
            .filter_map(|provider| {
                provider
                    .accounts
                    .iter()
                    .find(|account| account.status == AccountStatus::Ready)
                    .map(|account| (provider, account))
            })
    };

    for (provider, account) in ready() {
        if let Some(route) = provider
            .models
            .iter()
            .find(|route| route.id.eq_ignore_ascii_case(wanted))
        {
            return Some(ModelSelection {
                provider_id: provider.id.clone(),
                account_id: account.id.clone(),
                model: route.id.clone(),
                effort: route.default_effort.clone(),
                backup: None,
            });
        }
    }

    // The first endpoint with nothing to say about itself takes the id as given.
    let (provider, account) = ready().find(|(provider, _)| provider.models.is_empty())?;
    Some(ModelSelection {
        provider_id: provider.id.clone(),
        account_id: account.id.clone(),
        model: wanted.to_string(),
        effort: None,
        backup: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::store::save as save_providers;
    use crate::accounts::types::{Account, AccountKind, ModelRoute, ModelSource, Protocol, Secret};

    fn app_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("the-pair-resolve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn provider(vendor: &str, base_url: &str, models: &[&str]) -> Provider {
        provider_with_id(vendor, &format!("{}-1", vendor), base_url, models)
    }

    fn provider_with_id(vendor: &str, id: &str, base_url: &str, models: &[&str]) -> Provider {
        Provider {
            id: id.into(),
            protocol: Protocol::Anthropic,
            base_url: base_url.into(),
            vendor: vendor.into(),
            display_name: vendor.into(),
            accounts: vec![Account {
                id: format!("{}-a", vendor),
                kind: AccountKind::ApiKey,
                label: vendor.into(),
                source: "manual".into(),
                status: AccountStatus::Ready,
                headers: vec![],
                tool_names: None,
                secret: Secret::ApiKey("k".into()),
            }],
            models: models
                .iter()
                .map(|id| ModelRoute {
                    id: (*id).to_string(),
                    display_name: (*id).to_string(),
                    context_window: None,
                    max_output: None,
                    efforts: vec![],
                    default_effort: None,
                    price: None,
                    source: ModelSource::Manual,
                })
                .collect(),
        }
    }

    #[test]
    fn a_slash_qualified_id_resolves_like_a_colon_qualified_one() {
        let dir = app_dir();
        save_providers(
            &dir,
            &[provider(
                "anthropic",
                "https://api.anthropic.com",
                &["opus-4-1"],
            )],
        )
        .unwrap();

        // A known qualifier is dropped either side of the separator.
        assert_eq!(
            selection_for_model(&dir, "claude/opus-4-1").unwrap().model,
            "opus-4-1"
        );
        // A gateway route is a path, not a qualifier: it is kept whole, so the
        // endpoint that lists it can still be found by the exact id.
        assert_eq!(
            model_id("anthropic/claude-3-5-sonnet"),
            "anthropic/claude-3-5-sonnet"
        );
        assert_eq!(model_id("minimax/MiniMax-M3"), "minimax/minimax-m3");
        assert_eq!(model_id("gpt-5.1"), "gpt-5.1");
        assert_eq!(model_id("claude:"), "claude:");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_prefixed_model_id_resolves_on_its_vendor() {
        let dir = app_dir();
        save_providers(
            &dir,
            &[provider(
                "anthropic",
                "https://api.anthropic.com",
                &["opus-4-1"],
            )],
        )
        .unwrap();

        let selection = selection_for_model(&dir, "claude:opus-4-1").unwrap();
        assert_eq!(selection.model, "opus-4-1");
        assert_eq!(selection.provider_id, "anthropic-1");
        assert_eq!(selection.account_id, "anthropic-a");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_route_default_effort_rides_along() {
        let dir = app_dir();
        let mut provider = provider("anthropic", "https://api.anthropic.com", &["sonnet-4-5"]);
        provider.models[0].default_effort = Some("high".into());
        save_providers(&dir, &[provider]).unwrap();

        let selection = selection_for_model(&dir, "sonnet-4-5").unwrap();
        assert_eq!(selection.effort.as_deref(), Some("high"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_model_only_another_endpoint_lists_still_resolves() {
        let dir = app_dir();
        save_providers(
            &dir,
            &[
                provider("xai", "https://api.x.ai/v1", &["grok-code-fast-1"]),
                provider("moonshot", "https://api.moonshot.cn/v1", &[]),
            ],
        )
        .unwrap();

        // No vendor hint for Grok, so the model id has to find it.
        let selection = selection_for_model(&dir, "grok:grok-code-fast-1").unwrap();
        assert_eq!(selection.model, "grok-code-fast-1");
        assert_eq!(selection.provider_id, "xai-1");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Two endpoints of one vendor that both list the model: a subscription
    /// plan and an API key, say.
    fn plan_and_key(dir: &std::path::Path) {
        let mut plan = provider_with_id(
            "openai",
            "plan-1",
            "https://api.openai.com/v1",
            &["gpt-5-codex"],
        );
        plan.accounts[0].id = "plan-a".into();
        let mut key = provider_with_id(
            "openai",
            "key-1",
            "https://api.openai.com/v1",
            &["gpt-5-codex"],
        );
        key.accounts[0].id = "key-a".into();
        // The API key comes first, so the id alone resolves onto it.
        save_providers(dir, &[key, plan]).unwrap();
    }

    #[test]
    fn the_endpoint_a_model_was_picked_under_decides() {
        let dir = app_dir();
        plan_and_key(&dir);
        assert_eq!(
            selection_for_role(&dir, "mentor", "gpt-5-codex")
                .unwrap()
                .provider_id,
            "key-1",
            "nothing picked: the first endpoint that lists it"
        );
        assert!(preferred_selection(&dir, "mentor", "gpt-5-codex").is_none());

        prefer_route(&dir, "mentor", "GPT-5-Codex", "plan-1").unwrap();

        let picked = selection_for_role(&dir, "mentor", "gpt-5-codex").unwrap();
        assert_eq!(picked.provider_id, "plan-1");
        assert_eq!(picked.account_id, "plan-a");
        assert_eq!(
            preferred_selection(&dir, "mentor", "codex:gpt-5-codex")
                .unwrap()
                .provider_id,
            "plan-1",
            "a legacy spelling of the id names the same pick"
        );
        // The pick is the role's own: the executor still resolves by the id.
        assert_eq!(
            selection_for_role(&dir, "executor", "gpt-5-codex")
                .unwrap()
                .provider_id,
            "key-1"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_picked_endpoint_that_cannot_serve_the_model_is_passed_over() {
        let dir = app_dir();
        plan_and_key(&dir);
        prefer_route(&dir, "mentor", "gpt-5-codex", "plan-1").unwrap();

        // The plan's sign-in expired: the pick no longer resolves, so the id
        // falls back to an endpoint that can run it.
        let mut providers = load_providers(&dir);
        providers[1].accounts[0].status = AccountStatus::AuthExpired;
        save_providers(&dir, &providers).unwrap();
        assert!(preferred_selection(&dir, "mentor", "gpt-5-codex").is_none());
        assert_eq!(
            selection_for_role(&dir, "mentor", "gpt-5-codex")
                .unwrap()
                .provider_id,
            "key-1"
        );

        // An endpoint that is gone altogether, a role that does not exist and
        // an empty pick are all refused or ignored, never a panic.
        prefer_route(&dir, "mentor", "gpt-5-codex", "gone-9").unwrap();
        assert_eq!(
            selection_for_role(&dir, "mentor", "gpt-5-codex")
                .unwrap()
                .provider_id,
            "key-1"
        );
        assert!(prefer_route(&dir, "reviewer", "gpt-5-codex", "plan-1").is_err());
        assert!(prefer_route(&dir, "mentor", "", "plan-1").is_err());
        assert!(prefer_route(&dir, "mentor", "gpt-5-codex", " ").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_role_keeps_its_endpoint_while_it_can_still_run_the_model() {
        let dir = app_dir();
        plan_and_key(&dir);
        let on_plan = ModelSelection {
            provider_id: "plan-1".into(),
            account_id: "plan-a".into(),
            model: "gpt-5-codex".into(),
            effort: None,
            backup: None,
        };

        // The id alone would resolve onto the API key, but the role is already
        // talking to the plan: a handoff must not move it.
        assert_eq!(
            keep_or_resolve(&dir, "mentor", "gpt-5-codex", Some(&on_plan)),
            Some(on_plan.clone())
        );
        // Another model is another selection.
        assert!(keep_or_resolve(&dir, "mentor", "gpt-4o", Some(&on_plan)).is_none());
        // Nothing to keep resolves afresh.
        assert_eq!(
            keep_or_resolve(&dir, "mentor", "gpt-5-codex", None)
                .unwrap()
                .provider_id,
            "key-1"
        );

        // Once the plan cannot be used, the role moves to an endpoint that can.
        let mut providers = load_providers(&dir);
        providers[1].accounts[0].status = AccountStatus::AuthExpired;
        save_providers(&dir, &providers).unwrap();
        assert_eq!(
            keep_or_resolve(&dir, "mentor", "gpt-5-codex", Some(&on_plan))
                .unwrap()
                .provider_id,
            "key-1"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_endpoint_without_a_credential_is_never_chosen() {
        let dir = app_dir();
        let mut provider = provider("anthropic", "https://api.anthropic.com", &["opus-4-1"]);
        provider.accounts[0].status = AccountStatus::NeedsKey;
        save_providers(&dir, &[provider]).unwrap();

        assert!(selection_for_model(&dir, "claude:opus-4-1").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_vendor_mismatch_falls_back_to_the_model_id() {
        let dir = app_dir();
        // The endpoint is Anthropic-shaped but registered under another vendor,
        // the way a proxy or a gateway shows up.
        save_providers(
            &dir,
            &[provider(
                "gateway",
                "https://api.anthropic.com",
                &["opus-4-1"],
            )],
        )
        .unwrap();

        let selection = selection_for_model(&dir, "claude:opus-4-1").unwrap();
        assert_eq!(selection.provider_id, "gateway-1");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_gateway_that_lists_the_model_serves_it() {
        let dir = app_dir();
        // A proxy or a gateway — `ANTHROPIC_BASE_URL` from the claude importer,
        // OpenRouter, a hand-added endpoint — is registered under its own host as
        // the vendor, so no vendor hint can name it. What it lists is the only
        // evidence there is, and it lists this id.
        save_providers(
            &dir,
            &[provider_with_id(
                "llm.internal.example",
                "gateway-1",
                "https://llm.internal.example/v1",
                &["claude-sonnet-4-5"],
            )],
        )
        .unwrap();

        let selection = selection_for_model(&dir, "claude-sonnet-4-5").unwrap();
        assert_eq!(selection.provider_id, "gateway-1");
        assert_eq!(selection.account_id, "llm.internal.example-a");
        assert_eq!(selection.model, "claude-sonnet-4-5");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_vendor_hint_orders_the_candidates_without_excluding_any() {
        let dir = app_dir();
        // Both endpoints list the model. The vendor's own endpoint is the honest
        // answer and is tried first; the gateway that lists it too is what the
        // hint used to hide, and it is now the fallback rather than nothing.
        save_providers(
            &dir,
            &[
                provider(
                    "gateway",
                    "https://llm.internal.example/v1",
                    &["claude-sonnet-4-5"],
                ),
                provider(
                    "anthropic",
                    "https://api.anthropic.com",
                    &["claude-sonnet-4-5"],
                ),
            ],
        )
        .unwrap();

        assert_eq!(
            selection_for_model(&dir, "claude-sonnet-4-5")
                .unwrap()
                .provider_id,
            "anthropic-1"
        );

        // Without the vendor's own endpoint the gateway answers, and a `gpt-*`
        // id is served the same way.
        save_providers(
            &dir,
            &[provider_with_id(
                "llm.internal.example",
                "gateway-1",
                "https://llm.internal.example/v1",
                &["gpt-5.1"],
            )],
        )
        .unwrap();
        assert_eq!(
            selection_for_model(&dir, "gpt-5.1").unwrap().provider_id,
            "gateway-1"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_empty_model_or_empty_registry_resolves_to_nothing() {
        let dir = app_dir();
        assert!(selection_for_model(&dir, "claude:").is_none());
        assert!(selection_for_model(&dir, "").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_endpoint_that_lists_other_models_is_skipped() {
        let dir = app_dir();
        // The endpoint is right but has never heard of this id, so a pair asking
        // for it stays on its CLI rather than sending an id that cannot work.
        save_providers(
            &dir,
            &[provider(
                "anthropic",
                "https://api.anthropic.com",
                &["sonnet-4-5"],
            )],
        )
        .unwrap();
        assert!(selection_for_model(&dir, "claude:opus-4-1").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_endpoint_with_no_model_list_serves_the_id_as_given() {
        let dir = app_dir();
        save_providers(&dir, &[provider("first", "https://one.example/v1", &[])]).unwrap();

        let selection = selection_for_model(&dir, "some-model").unwrap();
        assert_eq!(selection.provider_id, "first-1");
        assert_eq!(selection.model, "some-model");
        std::fs::remove_dir_all(&dir).ok();
    }
}
