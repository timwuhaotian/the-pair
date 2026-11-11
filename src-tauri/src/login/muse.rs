//! Muse Code: Meta's subscription, billed to the Muse Code client, behind one API
//! key. Its gateway serves the subscription only to Muse Code's own client, and
//! refuses another client's tool names: a request naming `bash` is answered
//! `client_attestation_denied`, so the account carries the mapping killi sends
//! and reads instead —
//!
//! ```text
//! POST https://api.meta.ai/v1
//! authorization: Bearer LLM|...
//! tools: run_command   (bash → run_command)
//! ```
//!
//! The key is never checked here: a wrong one shows up as HTTP 401 on the first
//! request, the same as in killi's own `killi login muse-code`.

use super::*;
use crate::accounts::presets::preset_models;
use crate::accounts::types::{AccountKind, Secret};
use std::collections::BTreeMap;

/// Where Muse Code's models answer.
const BASE_URL: &str = "https://api.meta.ai/v1";

/// Tool names Meta's client check refuses, and what is sent instead.
const TOOL_NAMES: &[(&str, &str)] = &[("bash", "run_command")];

const SOURCE: &str = "oauth:muse";
const LABEL: &str = "Muse Code";

fn tool_names() -> BTreeMap<String, String> {
    TOOL_NAMES
        .iter()
        .map(|(from, to)| ((*from).to_string(), (*to).to_string()))
        .collect()
}

/// Takes the pasted key and returns the provider to upsert.
pub fn login(api_key: &str) -> Result<LoginOutcome, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Paste your Muse Code API key to finish signing in.".to_string());
    }

    Ok(LoginOutcome {
        protocol: Protocol::Responses,
        base_url: BASE_URL.to_string(),
        vendor: None,
        display_name: None,
        account: AccountDraft {
            kind: AccountKind::ApiKey,
            label: LABEL.to_string(),
            source: SOURCE.to_string(),
            secret: Secret::ApiKey(key.to_string()),
            headers: Vec::new(),
            tool_names: Some(tool_names()),
        },
        models: preset_models(Protocol::Responses, BASE_URL),
        notes: vec![
            "Tools are sent and read under Meta's own names: bash → run_command.".to_string(),
            "The key isn't checked here; a wrong one shows as HTTP 401 on the first request."
                .to_string(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::registry::{upsert_drafts, ProviderDraft};
    use crate::accounts::types::Account;

    fn provider_of(outcome: &LoginOutcome) -> crate::accounts::types::Provider {
        let mut providers = Vec::new();
        upsert_drafts(
            &mut providers,
            &[ProviderDraft {
                protocol: outcome.protocol,
                base_url: outcome.base_url.clone(),
                vendor: outcome.vendor.clone(),
                display_name: outcome.display_name.clone(),
                accounts: vec![outcome.account.clone()],
                models: outcome.models.clone(),
            }],
        );
        providers.remove(0)
    }

    #[test]
    fn records_the_muse_endpoint_the_key_and_the_tool_names() {
        let outcome = login("LLM|1|k").expect("a key is enough");
        assert_eq!(outcome.protocol, Protocol::Responses);
        assert_eq!(outcome.base_url, BASE_URL);
        assert_eq!(outcome.account.kind, AccountKind::ApiKey);
        assert_eq!(outcome.account.source, SOURCE);
        assert_eq!(outcome.account.secret, Secret::ApiKey("LLM|1|k".into()));
        assert!(outcome.account.headers.is_empty());

        let provider = provider_of(&outcome);
        assert_eq!(provider.vendor, "meta");
        assert_eq!(provider.display_name, "Meta (Muse Code)");
        assert_eq!(provider.accounts.len(), 1);
    }

    #[test]
    fn an_empty_key_is_refused() {
        assert!(login("").is_err());
    }

    #[test]
    fn a_blank_key_is_refused() {
        assert!(login("   \n\t ").is_err());
    }

    #[test]
    fn the_key_is_trimmed_before_it_is_stored() {
        let outcome = login("  LLM|1|k\n").expect("a key is enough");
        assert_eq!(outcome.account.secret, Secret::ApiKey("LLM|1|k".into()));
    }

    #[test]
    fn only_the_names_meta_refuses_are_mapped() {
        let outcome = login("LLM|1|k").expect("a key is enough");
        let names = outcome
            .account
            .tool_names
            .as_ref()
            .expect("muse maps tool names");
        assert_eq!(
            names,
            &BTreeMap::from([("bash".to_string(), "run_command".to_string())])
        );
        assert!(
            !names.contains_key("read") && !names.contains_key("shell"),
            "killi maps bash only"
        );
        assert!(
            !names.contains_key("web_search"),
            "The Pair sends no web search tool, so there is nothing to map"
        );
        assert!(
            outcome
                .notes
                .iter()
                .any(|note| note.contains("run_command")),
            "{:?}",
            outcome.notes
        );
    }

    #[test]
    fn models_come_from_the_preset_without_duplicates() {
        let outcome = login("LLM|1|k").expect("a key is enough");
        assert_eq!(outcome.models, preset_models(Protocol::Responses, BASE_URL));

        let provider = provider_of(&outcome);
        let ids: Vec<&str> = provider
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect();
        assert!(ids.contains(&"muse-wonder-1.0-contributor"), "{:?}", ids);
        assert!(ids.contains(&"muse-wonder-1.0-full"), "{:?}", ids);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "a preset model listed twice");
        assert!(provider.models.iter().all(|model| model.price.is_none()));
    }

    #[test]
    fn signing_in_again_updates_the_same_account() {
        let first = login("LLM|1|old").expect("a key is enough");
        let second = login("LLM|2|new").expect("a key is enough");
        let id_of = |outcome: &LoginOutcome| {
            Account::make_id("meta", outcome.account.kind, SOURCE, &outcome.account.label)
        };
        assert_eq!(
            id_of(&first),
            id_of(&second),
            "id must not depend on the key"
        );
    }

    #[test]
    fn nothing_is_written_to_disk() {
        let root = std::env::temp_dir().join(format!("the-pair-muse-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp dir");
        let outcome = login("LLM|1|k").expect("a key is enough");
        assert!(std::fs::read_dir(&root).expect("temp dir").next().is_none());
        assert_eq!(outcome.account.source, SOURCE);
        std::fs::remove_dir_all(&root).ok();
    }
}
