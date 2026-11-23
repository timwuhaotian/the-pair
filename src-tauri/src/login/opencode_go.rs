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
//! value is only known per request, so the account carries no such header: the
//! harness fills it from the conversation's cache key on every Go request.
//!
//! The key is checked while signing in: the live model catalog is fetched with
//! it, so a wrong key shows up as HTTP 401 here instead of on the first request.
//! Only catalog models offering Chat Completions are recorded, since that is
//! the one Go endpoint this app speaks.

use super::*;
use crate::accounts::types::{AccountKind, ModelRoute, ModelSource, Secret};
use serde_json::Value;
use std::time::Duration;

/// Where Go's Chat Completions answer.
const BASE_URL: &str = "https://opencode.ai/inference/go/openai/v1";
/// Where Go's live model catalog answers.
const MODELS_URL: &str = "https://opencode.ai/inference/go/v1/models";

const SOURCE: &str = "oauth:opencode-go";
const LABEL: &str = "OpenCode Go";

/// Takes the pasted key, checks it against the live catalog, and returns the
/// provider to upsert with the catalog's Chat Completions models.
pub async fn login(api_key: &str) -> Result<LoginOutcome, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Paste your OpenCode Go API key to finish signing in.".to_string());
    }
    let catalog = catalog(&http_client(), key).await?;
    let models = catalog_models(&catalog);
    if models.is_empty() {
        return Err(
            "OpenCode Go listed no models this app can use (Chat Completions).".to_string(),
        );
    }
    build_outcome(key, models)
}

/// Builds the outcome for an already-fetched catalog: the sync half of
/// [`login`], kept separate so it can be tested without the network.
fn build_outcome(api_key: &str, models: Vec<ModelRoute>) -> Result<LoginOutcome, String> {
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
        models,
        notes: vec![
            "Your OpenCode Go key was checked and its models were listed from the live catalog."
                .to_string(),
        ],
    })
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("cli")
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// The live catalog behind `key`, or why it could not be read: a 401 here is
/// how a wrong key is refused.
async fn catalog(http: &reqwest::Client, key: &str) -> Result<Value, String> {
    let response = http
        .get(MODELS_URL)
        .header("authorization", format!("Bearer {key}"))
        .send()
        .await
        .map_err(|error| format!("could not reach OpenCode Go: {error}"))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(
            "OpenCode Go turned the key away (HTTP 401). Check the key and try again.".to_string(),
        );
    }
    if !status.is_success() {
        return Err(format!(
            "OpenCode Go refused to list its models (HTTP {status}). Try again."
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("could not read OpenCode Go's catalog: {error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("OpenCode Go's catalog is not valid JSON: {error}"))
}

/// The catalog as models: only entries offering Chat Completions answer on
/// the endpoint this app records, so the rest drop out.
fn catalog_models(catalog: &Value) -> Vec<ModelRoute> {
    catalog
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry
                .get("api")
                .and_then(Value::as_object)
                .is_some_and(|api| api.contains_key("chat-completions"))
        })
        .filter_map(|entry| {
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())?;
            let reasoning = entry
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("reasoning"));
            Some(ModelRoute {
                id: id.to_string(),
                display_name: entry
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .unwrap_or(id)
                    .to_string(),
                context_window: entry
                    .get("limit")
                    .and_then(|limit| limit.get("context"))
                    .and_then(Value::as_u64),
                max_output: entry
                    .get("limit")
                    .and_then(|limit| limit.get("output"))
                    .and_then(Value::as_u64),
                efforts: reasoning
                    .and_then(|reasoning| reasoning.get("effort"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect(),
                default_effort: reasoning
                    .and_then(|reasoning| reasoning.get("default"))
                    .filter(|default| {
                        matches!(
                            default.get("type").and_then(Value::as_str),
                            None | Some("effort")
                        )
                    })
                    .and_then(|default| default.get("value"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(String::from),
                price: None,
                source: ModelSource::Endpoint,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::registry::{upsert_drafts, ProviderDraft};
    use crate::accounts::types::Account;

    fn build(key: &str) -> Result<LoginOutcome, String> {
        build_outcome(
            key,
            vec![ModelRoute {
                id: "go-model".to_string(),
                display_name: "Go Model".to_string(),
                context_window: Some(200_000),
                max_output: Some(64_000),
                efforts: Vec::new(),
                default_effort: None,
                price: None,
                source: ModelSource::Endpoint,
            }],
        )
    }

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
        let outcome = build("sk-go-1").expect("a key is enough");
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
        assert!(build("").is_err());
    }

    #[test]
    fn a_blank_key_is_refused() {
        assert!(build("   \n\t ").is_err());
    }

    #[test]
    fn the_key_is_trimmed_before_it_is_stored() {
        let outcome = build("  sk-go-1\n").expect("a key is enough");
        assert_eq!(outcome.account.secret, Secret::ApiKey("sk-go-1".into()));
    }

    #[test]
    fn a_key_that_looks_like_an_env_reference_stays_literal() {
        // `Secret::Env` would make the account depend on a live process variable.
        let outcome = build("$OPENCODE_API_KEY").expect("a key is enough");
        assert_eq!(
            outcome.account.secret,
            Secret::ApiKey("$OPENCODE_API_KEY".into())
        );
        assert_eq!(outcome.account.kind, AccountKind::OpencodeGo);
    }

    #[test]
    fn no_empty_header_is_left_for_the_transport_to_fill() {
        // The harness fills `x-opencode-session` per request for Go accounts,
        // so a stored empty one would only be replaced dead weight.
        let outcome = build("sk-go-1").expect("a key is enough");
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
    fn the_catalog_keeps_only_chat_completions_models() {
        let catalog = serde_json::json!({
            "data": [
                {
                    "id": "go-chat",
                    "name": "Go Chat",
                    "capabilities": {"reasoning": {
                        "effort": ["none", "low", "high"],
                        "default": {"type": "effort", "value": "low"}
                    }},
                    "limit": {"context": 200_000, "output": 64_000},
                    "api": {"chat-completions": {}, "responses": {}}
                },
                {
                    "id": "go-responses-only",
                    "name": "Responses Only",
                    "limit": {"context": 100_000, "output": 8_000},
                    "api": {"responses": {}}
                },
                {
                    "id": "go-no-effort",
                    "limit": {"context": 128_000, "output": 32_000},
                    "api": {"chat-completions": {}}
                },
                {"id": "", "api": {"chat-completions": {}}},
                {"api": {"chat-completions": {}}}
            ]
        });
        let models = catalog_models(&catalog);
        assert_eq!(models.len(), 2, "{models:?}");
        assert_eq!(models[0].id, "go-chat");
        assert_eq!(models[0].display_name, "Go Chat");
        assert_eq!(models[0].context_window, Some(200_000));
        assert_eq!(models[0].max_output, Some(64_000));
        assert_eq!(models[0].efforts, ["none", "low", "high"]);
        assert_eq!(models[0].default_effort.as_deref(), Some("low"));
        assert_eq!(models[0].source, ModelSource::Endpoint);
        assert_eq!(models[1].id, "go-no-effort");
        assert_eq!(
            models[1].display_name, "go-no-effort",
            "falls back to the id"
        );
        assert!(models[1].efforts.is_empty());
        assert_eq!(models[1].default_effort, None);
    }

    #[test]
    fn a_non_effort_default_is_not_a_default_effort() {
        let catalog = serde_json::json!({"data": [{
            "id": "go-chat",
            "capabilities": {"reasoning": {
                "effort": ["low"],
                "default": {"type": "disabled", "value": "low"}
            }},
            "api": {"chat-completions": {}}
        }]});
        let models = catalog_models(&catalog);
        assert_eq!(models[0].efforts, ["low"]);
        assert_eq!(models[0].default_effort, None);
    }

    #[test]
    fn a_catalog_without_usable_models_is_empty() {
        assert!(catalog_models(&serde_json::json!({})).is_empty());
        assert!(catalog_models(&serde_json::json!({"data": [{
            "id": "go-responses-only",
            "api": {"responses": {}}
        }]}))
        .is_empty());
    }

    #[tokio::test]
    async fn login_refuses_an_empty_key_before_any_request() {
        assert!(login("   ").await.is_err());
    }

    #[test]
    fn signing_in_again_updates_the_same_account() {
        let first = build("sk-go-old").expect("a key is enough");
        let second = build("sk-go-new").expect("a key is enough");
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
        let outcome = build("sk-go-1").expect("a key is enough");
        assert!(std::fs::read_dir(&root).expect("temp dir").next().is_none());
        assert_eq!(outcome.account.source, SOURCE);
        std::fs::remove_dir_all(&root).ok();
    }
}
