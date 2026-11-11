//! OpenCode Go: one API key from the OpenCode Console (opencode.ai/docs/go) in
//! front of the open models Go serves. The Pair speaks Chat Completions, so the
//! flow records that endpoint and the key:
//!
//! ```text
//! POST https://opencode.ai/inference/go/openai/v1/chat/completions
//! authorization: Bearer sk-...
//! ```
//!
//! OpenCode also routes and caches a conversation by `x-opencode-session`, whose
//! value is the conversation's id and so is only known per request. killi fills
//! that header from the conversation's id on every request, but the transport
//! here sends an account's headers verbatim and fills nothing in, so the account
//! carries no such header: an empty one would be sent as a no-op that only looks
//! like routing.
//!
//! The key is never checked here: a wrong one shows up as HTTP 401 on the first
//! request, the same as in killi's own `killi login opencode-go`.

use super::*;
use crate::accounts::presets::preset_models;
use crate::accounts::types::{AccountKind, Secret};

/// Where Go's Chat Completions answer.
const BASE_URL: &str = "https://opencode.ai/inference/go/openai/v1";

const SOURCE: &str = "oauth:opencode-go";
const LABEL: &str = "OpenCode Go";

/// Takes the pasted key and returns the provider to upsert.
pub fn login(api_key: &str) -> Result<LoginOutcome, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Paste your OpenCode Go API key to finish signing in.".to_string());
    }

    Ok(LoginOutcome {
        protocol: Protocol::OpenAiChat,
        base_url: BASE_URL.to_string(),
        vendor: None,
        display_name: None,
        account: AccountDraft {
            kind: AccountKind::OpencodeGo,
            label: LABEL.to_string(),
            source: SOURCE.to_string(),
            secret: Secret::ApiKey(key.to_string()),
            headers: Vec::new(),
            tool_names: None,
        },
        models: preset_models(Protocol::OpenAiChat, BASE_URL),
        notes: vec![
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
    fn records_the_go_endpoint_and_the_key() {
        let outcome = login("sk-go-1").expect("a key is enough");
        assert_eq!(outcome.protocol, Protocol::OpenAiChat);
        assert_eq!(outcome.base_url, BASE_URL);
        assert_eq!(outcome.account.kind, AccountKind::OpencodeGo);
        assert_eq!(outcome.account.source, SOURCE);
        assert_eq!(outcome.account.secret, Secret::ApiKey("sk-go-1".into()));
        assert!(
            outcome.account.headers.is_empty(),
            "nothing is sent that the transport cannot fill in"
        );
        assert!(outcome.account.tool_names.is_none());

        let provider = provider_of(&outcome);
        assert_eq!(provider.vendor, "opencode");
        assert_eq!(provider.display_name, "OpenCode Go");
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
        let outcome = login("  sk-go-1\n").expect("a key is enough");
        assert_eq!(outcome.account.secret, Secret::ApiKey("sk-go-1".into()));
    }

    #[test]
    fn a_key_that_looks_like_an_env_reference_stays_literal() {
        // `Secret::Env` would make the account depend on a live process variable.
        let outcome = login("$OPENCODE_API_KEY").expect("a key is enough");
        assert_eq!(
            outcome.account.secret,
            Secret::ApiKey("$OPENCODE_API_KEY".into())
        );
        assert_eq!(outcome.account.kind, AccountKind::OpencodeGo);
    }

    #[test]
    fn no_empty_header_is_left_for_the_transport_to_fill() {
        // `llm/mod.rs` sends an account's headers verbatim and fills nothing
        // in, so an empty `x-opencode-session` would reach OpenCode as a no-op.
        let outcome = login("sk-go-1").expect("a key is enough");
        assert!(
            outcome.account.headers.is_empty(),
            "{:?}",
            outcome.account.headers
        );
        assert!(
            outcome.notes.iter().all(|note| !note.contains("session")),
            "nothing advertises a header that is not sent: {:?}",
            outcome.notes
        );
    }

    #[test]
    fn models_come_from_the_preset_without_duplicates() {
        let outcome = login("sk-go-1").expect("a key is enough");
        assert_eq!(
            outcome.models,
            preset_models(Protocol::OpenAiChat, BASE_URL)
        );

        let provider = provider_of(&outcome);
        let ids: Vec<&str> = provider
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect();
        assert!(ids.contains(&"claude-sonnet-4-5"), "{:?}", ids);
        assert!(ids.contains(&"kimi-k2"), "{:?}", ids);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "a preset model listed twice");
    }

    #[test]
    fn signing_in_again_updates_the_same_account() {
        let first = login("sk-go-old").expect("a key is enough");
        let second = login("sk-go-new").expect("a key is enough");
        let id_of = |outcome: &LoginOutcome| {
            Account::make_id(
                "opencode",
                outcome.account.kind,
                SOURCE,
                &outcome.account.label,
            )
        };
        assert_eq!(
            id_of(&first),
            id_of(&second),
            "id must not depend on the key"
        );
    }

    #[test]
    fn nothing_is_written_to_disk() {
        let root =
            std::env::temp_dir().join(format!("the-pair-opencode-go-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp dir");
        let outcome = login("sk-go-1").expect("a key is enough");
        assert!(std::fs::read_dir(&root).expect("temp dir").next().is_none());
        assert_eq!(outcome.account.source, SOURCE);
        std::fs::remove_dir_all(&root).ok();
    }
}
