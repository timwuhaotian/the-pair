//! Merging discovered providers and accounts into the registry.
//!
//! Scans are idempotent: the same source scanned twice must not duplicate
//! accounts, and a key rotated in the source file must update the existing
//! account instead of orphaning it. That is why an account is matched on
//! `(source, label)` — the credential's identity in every config — rather than
//! on anything derived from the secret itself.

use super::presets::{preset_for, preset_models};
use super::types::{
    normalize_base_url, provider_id, Account, AccountKind, AccountStatus, HeaderPair, ModelRoute,
    ModelSource, Protocol, Provider, Secret,
};
use std::collections::BTreeMap;

/// An account a scan wants to create or refresh.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountDraft {
    pub kind: AccountKind,
    pub label: String,
    pub source: String,
    pub secret: Secret,
    pub headers: Vec<HeaderPair>,
    pub tool_names: Option<BTreeMap<String, String>>,
}

/// A provider a scan wants to create or refresh.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderDraft {
    pub protocol: Protocol,
    pub base_url: String,
    /// Overrides the preset vendor; otherwise inferred from the host.
    pub vendor: Option<String>,
    pub display_name: Option<String>,
    pub accounts: Vec<AccountDraft>,
    pub models: Vec<ModelRoute>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpsertReport {
    pub providers_added: usize,
    pub accounts_added: usize,
    pub accounts_updated: usize,
    pub models_added: usize,
}

/// Applies drafts to `providers` in place.
pub fn upsert_drafts(providers: &mut Vec<Provider>, drafts: &[ProviderDraft]) -> UpsertReport {
    let mut report = UpsertReport::default();
    for draft in drafts {
        let base_url = normalize_base_url(&draft.base_url);
        if base_url.is_empty() {
            continue;
        }
        let id = provider_id(draft.protocol, &base_url);
        let provider_position = providers.iter().position(|provider| provider.id == id);
        let index = match provider_position {
            Some(index) => index,
            None => {
                providers.push(new_provider(draft, &base_url, &id));
                report.providers_added += 1;
                providers.len() - 1
            }
        };
        let provider = &mut providers[index];

        for account in &draft.accounts {
            // `kind` used to be part of the id, but the importers decide it from
            // whether an env variable is live, so the same credential flipped
            // between `env` and `apiKey` from scan to scan and left a stale
            // account behind. `(source, label)` identifies the credential; the
            // id it already has is kept, because pair selections store it.
            let position = provider.accounts.iter().position(|existing| {
                existing.source == account.source && existing.label == account.label
            });
            match position {
                Some(position) => {
                    let existing = &mut provider.accounts[position];
                    existing.kind = account.kind;
                    let same_secret = existing.secret == account.secret;
                    existing.secret = account.secret.clone();
                    existing.headers = account.headers.clone();
                    existing.tool_names = account.tool_names.clone();
                    // A scan never downgrades a live OAuth account to "expired":
                    // only a failed refresh does that. Re-reporting the very
                    // same token therefore leaves the status alone, while a new
                    // token — the sign-in flow just ran — is re-evaluated, or an
                    // account stuck in `AuthExpired` could never recover. Every
                    // other kind is re-evaluated too: an env reference gains and
                    // loses liveness with the process, not with the scan.
                    if !(same_secret && matches!(account.secret, Secret::OAuth(_))) {
                        existing.status = status_for(&account.secret, account.kind);
                    }
                    report.accounts_updated += 1;
                }
                None => {
                    let account_id = Account::make_id(
                        &provider.id,
                        account.kind,
                        &account.source,
                        &account.label,
                    );
                    provider.accounts.push(Account {
                        id: account_id,
                        kind: account.kind,
                        label: account.label.clone(),
                        source: account.source.clone(),
                        status: status_for(&account.secret, account.kind),
                        headers: account.headers.clone(),
                        tool_names: account.tool_names.clone(),
                        secret: account.secret.clone(),
                    });
                    report.accounts_added += 1;
                }
            }
        }

        for model in &draft.models {
            if merge_model(&mut provider.models, model) {
                report.models_added += 1;
            }
        }
    }
    report
}

fn new_provider(draft: &ProviderDraft, base_url: &str, id: &str) -> Provider {
    let preset = preset_for(draft.protocol, base_url);
    let vendor = draft
        .vendor
        .clone()
        .or_else(|| preset.map(|preset| preset.vendor.to_string()))
        .unwrap_or_else(|| infer_vendor(base_url));
    let display_name = draft
        .display_name
        .clone()
        .or_else(|| preset.map(|preset| preset.display_name.to_string()))
        .unwrap_or_else(|| prettify_vendor(&vendor));

    let mut models = preset_models(draft.protocol, base_url);
    for model in &draft.models {
        merge_model(&mut models, model);
    }

    Provider {
        id: id.to_string(),
        protocol: draft.protocol,
        base_url: base_url.to_string(),
        vendor,
        display_name,
        accounts: Vec::new(),
        models,
    }
}

/// Adds `incoming` unless an equal-or-better entry already exists. Higher
/// precedence sources win; a lower one may still fill in missing metadata.
fn merge_model(existing: &mut Vec<ModelRoute>, incoming: &ModelRoute) -> bool {
    let Some(position) = existing
        .iter()
        .position(|model| model.id.eq_ignore_ascii_case(&incoming.id))
    else {
        existing.push(incoming.clone());
        return true;
    };

    let current = &mut existing[position];
    if source_rank(incoming.source) > source_rank(current.source) {
        let mut merged = incoming.clone();
        fill_missing(&mut merged, current);
        *current = merged;
    } else {
        fill_missing(current, incoming);
    }
    false
}

fn source_rank(source: ModelSource) -> u8 {
    match source {
        ModelSource::Preset => 0,
        ModelSource::Imported => 1,
        ModelSource::Endpoint => 2,
        ModelSource::Manual => 3,
    }
}

fn fill_missing(target: &mut ModelRoute, donor: &ModelRoute) {
    if target.context_window.is_none() {
        target.context_window = donor.context_window;
    }
    if target.max_output.is_none() {
        target.max_output = donor.max_output;
    }
    if target.efforts.is_empty() {
        target.efforts = donor.efforts.clone();
    }
    if target.default_effort.is_none() {
        target.default_effort = donor.default_effort.clone();
    }
    if target.price.is_none() {
        target.price = donor.price.clone();
    }
}

fn status_for(secret: &Secret, kind: AccountKind) -> AccountStatus {
    match kind {
        AccountKind::ChatGptOAuth | AccountKind::CommandCodeOAuth => match secret {
            // A token past its expiry is not usable, however long it looks: the
            // first turn after it would fail with "sign in again", so the
            // credential must not be offered as ready.
            Secret::OAuth(token)
                if token.refresh_token.is_some() || !token.access_token.is_empty() =>
            {
                if token.is_expired(crate::util::now_millis()) {
                    AccountStatus::AuthExpired
                } else {
                    AccountStatus::Ready
                }
            }
            _ => AccountStatus::AuthExpired,
        },
        _ => {
            if secret.is_present() {
                AccountStatus::Ready
            } else {
                AccountStatus::NeedsKey
            }
        }
    }
}

/// `https://api.moonshot.cn/v1` → `moonshot.cn`.
pub fn infer_vendor(base_url: &str) -> String {
    let normalized = normalize_base_url(base_url).to_lowercase();
    let host = normalized
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&normalized);
    let host = host.split('/').next().unwrap_or(host);
    let host = host.strip_prefix("api.").unwrap_or(host);
    if host.is_empty() {
        "custom".to_string()
    } else {
        host.to_string()
    }
}

/// `moonshot.cn` → `Moonshot.cn`, `llm.internal.example` → `Llm.internal.example`.
///
/// Only the first label is capitalized: the rest of a host keeps its casing, so
/// a TLD never turns into `Cn`.
pub fn prettify_vendor(vendor: &str) -> String {
    let mut parts = vendor.split('.');
    let Some(first) = parts.next() else {
        return String::new();
    };
    let mut out = String::with_capacity(vendor.len());
    let mut chars = first.chars();
    if let Some(head) = chars.next() {
        out.push(head.to_ascii_uppercase());
        out.push_str(chars.as_str());
    }
    for part in parts {
        out.push('.');
        out.push_str(part);
    }
    out
}

/// Replaces an account's secret in place (manual edit, OAuth refresh).
///
/// Pasting a key onto an OAuth account retires the token, so the account becomes
/// a key account: leaving the old kind would keep reporting it `authExpired`
/// while the key itself works.
pub fn replace_secret(
    providers: &mut [Provider],
    provider_id: &str,
    account_id: &str,
    secret: Secret,
) -> bool {
    let Some(provider) = providers
        .iter_mut()
        .find(|provider| provider.id == provider_id)
    else {
        return false;
    };
    let Some(account) = provider
        .accounts
        .iter_mut()
        .find(|account| account.id == account_id)
    else {
        return false;
    };
    account.kind = match &secret {
        Secret::Env(_) => AccountKind::Env,
        Secret::ApiKey(_) => AccountKind::ApiKey,
        Secret::OAuth(_) | Secret::None => account.kind,
    };
    account.status = status_for(&secret, account.kind);
    account.secret = secret;
    true
}

/// Adds a hand-written provider, or extends the one already at that endpoint.
///
/// Returns the provider and account ids the caller should report back.
pub fn upsert_manual(
    providers: &mut Vec<Provider>,
    protocol: Protocol,
    base_url: &str,
    label: &str,
    secret: Secret,
    models: Vec<ModelRoute>,
) -> Result<(String, String), String> {
    let normalized = normalize_base_url(base_url);
    if normalized.is_empty() {
        return Err(format!("`{}` does not name an endpoint", base_url.trim()));
    }
    let draft = ProviderDraft {
        protocol,
        base_url: base_url.to_string(),
        vendor: None,
        display_name: None,
        accounts: vec![AccountDraft {
            kind: match &secret {
                Secret::Env(_) => AccountKind::Env,
                _ => AccountKind::ApiKey,
            },
            label: label.to_string(),
            source: "manual".to_string(),
            secret,
            headers: Vec::new(),
            tool_names: None,
        }],
        models,
    };
    upsert_drafts(providers, std::slice::from_ref(&draft));
    let id = provider_id(protocol, &normalized);
    let provider = providers
        .iter()
        .find(|provider| provider.id == id)
        .ok_or_else(|| format!("No provider for {}", normalized))?;
    let account = provider
        .accounts
        .iter()
        .find(|account| account.source == "manual" && account.label == label)
        .ok_or_else(|| format!("No manual account `{}` on {}", label, id))?;
    Ok((provider.id.clone(), account.id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::types::ModelPrice;

    fn draft(protocol: Protocol, base_url: &str, source: &str, key: &str) -> ProviderDraft {
        ProviderDraft {
            protocol,
            base_url: base_url.to_string(),
            vendor: None,
            display_name: None,
            accounts: vec![AccountDraft {
                kind: AccountKind::ApiKey,
                label: format!("{} key", source),
                source: source.to_string(),
                secret: Secret::ApiKey(key.to_string()),
                headers: Vec::new(),
                tool_names: None,
            }],
            models: Vec::new(),
        }
    }

    fn model(id: &str, source: ModelSource) -> ModelRoute {
        ModelRoute {
            id: id.to_string(),
            display_name: id.to_string(),
            context_window: None,
            max_output: None,
            efforts: Vec::new(),
            default_effort: None,
            price: None,
            source,
        }
    }

    fn oauth_secret(expires_at: u64) -> Secret {
        Secret::OAuth(Box::new(crate::accounts::types::OAuthToken {
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            expires_at,
            token_type: "Bearer".into(),
            scope: None,
            account_label: Some("me@example.com".into()),
        }))
    }

    /// The ChatGPT sign-in flow's draft: one OAuth account on the Responses
    /// endpoint.
    fn oauth_draft(expires_at: u64) -> ProviderDraft {
        ProviderDraft {
            accounts: vec![AccountDraft {
                kind: AccountKind::ChatGptOAuth,
                label: "chatgpt".into(),
                source: "oauth:chatgpt".into(),
                secret: oauth_secret(expires_at),
                headers: Vec::new(),
                tool_names: None,
            }],
            ..draft(
                Protocol::Responses,
                "https://api.openai.com/v1",
                "oauth:chatgpt",
                "",
            )
        }
    }

    fn manual(
        providers: &mut Vec<Provider>,
        protocol: Protocol,
        base_url: &str,
        secret: Secret,
    ) -> Result<(String, String), String> {
        upsert_manual(providers, protocol, base_url, "work", secret, vec![])
    }

    #[test]
    fn same_endpoint_from_two_agents_merges_into_one_provider() {
        let mut providers = Vec::new();
        upsert_drafts(
            &mut providers,
            &[
                draft(
                    Protocol::OpenAiChat,
                    "https://api.moonshot.cn/v1",
                    "kimi",
                    "key-one",
                ),
                draft(
                    Protocol::OpenAiChat,
                    "https://api.moonshot.cn/v1/",
                    "opencode",
                    "key-two",
                ),
            ],
        );
        assert_eq!(providers.len(), 1, "endpoint must be deduplicated");
        assert_eq!(providers[0].accounts.len(), 2);
        assert_eq!(providers[0].vendor, "moonshot");
        assert_eq!(providers[0].display_name, "Moonshot (Kimi)");
    }

    #[test]
    fn different_protocols_stay_separate_providers() {
        let mut providers = Vec::new();
        upsert_drafts(
            &mut providers,
            &[
                draft(
                    Protocol::Anthropic,
                    "https://api.anthropic.com",
                    "claude",
                    "a",
                ),
                draft(
                    Protocol::OpenAiChat,
                    "https://api.anthropic.com",
                    "claude",
                    "a",
                ),
            ],
        );
        assert_eq!(providers.len(), 2);
    }

    #[test]
    fn rescanning_is_idempotent_and_rotates_the_key() {
        let mut providers = Vec::new();
        upsert_drafts(
            &mut providers,
            &[draft(
                Protocol::Anthropic,
                "https://api.anthropic.com",
                "claude",
                "old",
            )],
        );
        let first = providers[0].accounts[0].clone();

        let report = upsert_drafts(
            &mut providers,
            &[draft(
                Protocol::Anthropic,
                "https://api.anthropic.com",
                "claude",
                "new",
            )],
        );
        assert_eq!(report.accounts_added, 0);
        assert_eq!(report.accounts_updated, 1);
        assert_eq!(providers[0].accounts.len(), 1);
        assert_eq!(providers[0].accounts[0].id, first.id, "id must stay stable");
        assert_eq!(
            providers[0].accounts[0].secret,
            Secret::ApiKey("new".into()),
            "rotated key must replace the old one"
        );
    }

    #[test]
    fn empty_base_url_is_skipped() {
        let mut providers = Vec::new();
        let report = upsert_drafts(
            &mut providers,
            &[draft(Protocol::OpenAiChat, "   ", "kimi", "k")],
        );
        assert_eq!(report, UpsertReport::default());
        assert!(providers.is_empty());
    }

    #[test]
    fn preset_models_arrive_with_a_new_provider() {
        let mut providers = Vec::new();
        upsert_drafts(
            &mut providers,
            &[draft(
                Protocol::Anthropic,
                "https://api.anthropic.com",
                "claude",
                "k",
            )],
        );
        let ids: Vec<&str> = providers[0].models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"claude-sonnet-4-5-20250929"));
    }

    #[test]
    fn imported_model_beats_the_preset_and_keeps_its_price() {
        let mut providers = Vec::new();
        let mut with_model = draft(
            Protocol::Anthropic,
            "https://api.anthropic.com",
            "claude",
            "k",
        );
        with_model.models = vec![ModelRoute {
            price: Some(ModelPrice {
                input_per_million: 1.0,
                output_per_million: 2.0,
            }),
            ..model("claude-sonnet-4-5-20250929", ModelSource::Imported)
        }];
        upsert_drafts(&mut providers, &[with_model]);

        let merged = providers[0]
            .models
            .iter()
            .find(|model| model.id == "claude-sonnet-4-5-20250929")
            .unwrap();
        assert_eq!(merged.source, ModelSource::Imported);
        assert_eq!(merged.price.as_ref().unwrap().input_per_million, 1.0);
        // The preset filled in what the import left out.
        assert_eq!(merged.context_window, Some(200_000));
    }

    #[test]
    fn manual_model_beats_an_imported_one() {
        let mut providers = Vec::new();
        upsert_drafts(
            &mut providers,
            &[draft(
                Protocol::OpenAiChat,
                "https://api.x.ai/v1",
                "grok",
                "k",
            )],
        );
        let before = providers[0].models.len();
        let mut with_model = draft(Protocol::OpenAiChat, "https://api.x.ai/v1", "manual", "k");
        with_model.models = vec![model("grok-code-fast-1", ModelSource::Manual)];
        upsert_drafts(&mut providers, &[with_model]);

        assert_eq!(providers[0].models.len(), before, "no duplicate entry");
        let merged = providers[0]
            .models
            .iter()
            .find(|model| model.id == "grok-code-fast-1")
            .unwrap();
        assert_eq!(merged.source, ModelSource::Manual);
    }

    #[test]
    fn env_account_without_a_variable_needs_a_key() {
        let mut providers = Vec::new();
        let _guard = crate::test_env::lock_env();
        std::env::remove_var("THE_PAIR_MISSING_KEY");
        upsert_drafts(
            &mut providers,
            &[ProviderDraft {
                accounts: vec![AccountDraft {
                    kind: AccountKind::Env,
                    label: "env".into(),
                    source: "env".into(),
                    secret: Secret::Env("THE_PAIR_MISSING_KEY".into()),
                    headers: Vec::new(),
                    tool_names: None,
                }],
                ..draft(Protocol::Anthropic, "https://api.anthropic.com", "env", "")
            }],
        );
        assert_eq!(providers[0].accounts[0].status, AccountStatus::NeedsKey);
    }

    #[test]
    fn oauth_accounts_are_ready_with_a_token() {
        let mut providers = Vec::new();
        upsert_drafts(&mut providers, &[oauth_draft(u64::MAX)]);
        assert_eq!(providers[0].accounts[0].status, AccountStatus::Ready);
        assert_eq!(providers[0].ready_account_count(), 1);
    }

    #[test]
    fn an_expired_oauth_token_is_not_ready() {
        let mut providers = Vec::new();
        let now = crate::util::now_millis();
        upsert_drafts(&mut providers, &[oauth_draft(now - 1_000)]);
        assert_eq!(providers[0].accounts[0].status, AccountStatus::AuthExpired);
        assert_eq!(providers[0].ready_account_count(), 0);

        // The same token with time still on it is selectable again.
        upsert_drafts(&mut providers, &[oauth_draft(now + 1_000)]);
        assert_eq!(providers[0].accounts[0].status, AccountStatus::Ready);
    }

    #[test]
    fn a_refreshed_token_lifts_auth_expired_but_a_rescan_does_not() {
        let mut providers = Vec::new();
        let now = crate::util::now_millis();
        upsert_drafts(&mut providers, &[oauth_draft(now + 3_600_000)]);
        assert_eq!(providers[0].accounts[0].status, AccountStatus::Ready);

        // One 401 marks the account expired; the harness does that in place.
        providers[0].accounts[0].status = AccountStatus::AuthExpired;
        // Re-reporting the very same token is a rescan, not a recovery.
        upsert_drafts(&mut providers, &[oauth_draft(now + 3_600_000)]);
        assert_eq!(
            providers[0].accounts[0].status,
            AccountStatus::AuthExpired,
            "a scan must not resurrect an account the provider just refused"
        );

        // Signing in again is a new token, so the status is recomputed from it.
        upsert_drafts(&mut providers, &[oauth_draft(now + 7_200_000)]);
        assert_eq!(
            providers[0].accounts[0].status,
            AccountStatus::Ready,
            "a refreshed token must clear authExpired"
        );
        assert_eq!(providers[0].accounts.len(), 1);
    }

    #[test]
    fn a_credential_that_flips_between_env_and_key_keeps_one_account() {
        let mut providers = Vec::new();
        let _guard = crate::test_env::lock_env();
        std::env::remove_var("THE_PAIR_FLIPPING_KEY");
        let draft_for = |kind: AccountKind, secret: Secret| ProviderDraft {
            accounts: vec![AccountDraft {
                kind,
                label: "ANTHROPIC_API_KEY".into(),
                source: "claude".into(),
                secret,
                headers: Vec::new(),
                tool_names: None,
            }],
            ..draft(
                Protocol::Anthropic,
                "https://api.anthropic.com",
                "claude",
                "",
            )
        };

        // The importers record an env reference only while the variable is live.
        upsert_drafts(
            &mut providers,
            &[draft_for(
                AccountKind::Env,
                Secret::Env("THE_PAIR_FLIPPING_KEY".into()),
            )],
        );
        let first_id = providers[0].accounts[0].id.clone();
        assert_eq!(providers[0].accounts[0].status, AccountStatus::NeedsKey);

        upsert_drafts(
            &mut providers,
            &[draft_for(
                AccountKind::ApiKey,
                Secret::ApiKey("sk-copied-from-the-file".into()),
            )],
        );
        assert_eq!(
            providers[0].accounts.len(),
            1,
            "one credential must not become two accounts"
        );
        assert_eq!(providers[0].accounts[0].id, first_id, "id must stay stable");
        assert_eq!(providers[0].accounts[0].kind, AccountKind::ApiKey);
        assert_eq!(providers[0].accounts[0].status, AccountStatus::Ready);
    }

    #[test]
    fn unknown_vendor_is_inferred_from_the_host() {
        assert_eq!(infer_vendor("https://api.moonshot.cn/v1"), "moonshot.cn");
        assert_eq!(
            infer_vendor("https://llm.internal.example/v1"),
            "llm.internal.example"
        );
        assert_eq!(infer_vendor(""), "custom");
        assert_eq!(prettify_vendor("moonshot.cn"), "Moonshot.cn");
    }

    #[test]
    fn manual_provider_can_be_added_and_extended() {
        let mut providers = Vec::new();
        let (provider_id, account_id) = upsert_manual(
            &mut providers,
            Protocol::OpenAiChat,
            "https://llm.internal.example/v1",
            "work",
            Secret::ApiKey("k".into()),
            vec![model("internal-1", ModelSource::Manual)],
        )
        .unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].display_name, "Llm.internal.example");
        assert_eq!(providers[0].models.len(), 1);

        let (same_provider, same_account) = upsert_manual(
            &mut providers,
            Protocol::OpenAiChat,
            "https://llm.internal.example/v1",
            "work",
            Secret::ApiKey("rotated".into()),
            vec![],
        )
        .unwrap();
        assert_eq!(same_provider, provider_id);
        assert_eq!(same_account, account_id);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].accounts.len(), 1);
        assert_eq!(
            providers[0].accounts[0].secret,
            Secret::ApiKey("rotated".into())
        );
    }

    #[test]
    fn a_base_url_that_normalizes_to_nothing_is_rejected() {
        let mut providers = Vec::new();
        for base_url in ["   ", "/", "///"] {
            assert!(
                manual(
                    &mut providers,
                    Protocol::Anthropic,
                    base_url,
                    Secret::ApiKey("k".into())
                )
                .is_err(),
                "`{}` must not mint a provider",
                base_url
            );
        }
        assert!(providers.is_empty(), "nothing may be written");
    }

    #[test]
    fn replace_secret_targets_one_account() {
        let mut providers = Vec::new();
        let (provider_id, account_id) = manual(
            &mut providers,
            Protocol::Anthropic,
            "https://api.anthropic.com",
            Secret::ApiKey("k".into()),
        )
        .unwrap();
        assert!(replace_secret(
            &mut providers,
            &provider_id,
            &account_id,
            Secret::ApiKey("rotated".into())
        ));
        assert_eq!(
            providers[0].accounts[0].secret,
            Secret::ApiKey("rotated".into())
        );
        assert!(!replace_secret(
            &mut providers,
            "nope",
            &account_id,
            Secret::None
        ));
    }

    #[test]
    fn pasting_a_key_onto_an_oauth_account_makes_it_ready() {
        let mut providers = Vec::new();
        upsert_drafts(
            &mut providers,
            &[oauth_draft(crate::util::now_millis() - 1_000)],
        );
        let provider_id = providers[0].id.clone();
        let account_id = providers[0].accounts[0].id.clone();
        assert_eq!(providers[0].accounts[0].status, AccountStatus::AuthExpired);

        // The user resolved the sign-in out of band and pasted the key it gave
        // them: the key works, so the account must be selectable.
        assert!(replace_secret(
            &mut providers,
            &provider_id,
            &account_id,
            Secret::ApiKey("sk-pasted".into())
        ));
        assert_eq!(providers[0].accounts[0].kind, AccountKind::ApiKey);
        assert_eq!(providers[0].accounts[0].status, AccountStatus::Ready);
        assert_eq!(providers[0].accounts[0].id, account_id);

        // An env reference is tracked the same way.
        assert!(replace_secret(
            &mut providers,
            &provider_id,
            &account_id,
            Secret::Env("SOME_KEY".into())
        ));
        assert_eq!(providers[0].accounts[0].kind, AccountKind::Env);
    }
}
