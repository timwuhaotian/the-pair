//! pi: `~/.pi/agent/models.json`, with `models-store.json` and `settings.json`
//! beside it.
//!
//! Shape (verified against pi's own config):
//!
//! ```json
//! {
//!   "providers": {
//!     "moonshot": {
//!       "api": "openai",
//!       "baseUrl": "https://api.moonshot.cn/v1",
//!       "apiKey": "sk-...",
//!       "models": [
//!         { "id": "kimi-k2-0905-preview", "contextWindow": 256000, "maxTokens": 8192 }
//!       ]
//!     }
//!   }
//! }
//! ```
//!
//! `api` is pi's own spelling of the wire protocol (`anthropic-messages`,
//! `openai-responses`, else openai chat) and a model may name its own. Models a
//! provider does not list inline come from `models-store.json`, keyed by
//! provider name; `settings.json` carries the default provider/model pair and
//! the thinking level; `auth.json` holds the keys `/login` fetched, one entry
//! per provider. An `apiKey` may interpolate `$NAME` / `${NAME}` from the
//! environment, and a leading `!command` runs at request time — there is no key
//! to copy for either shape beyond what the environment holds. Providers pi
//! reaches through its own account login carry no key anywhere and are skipped:
//! there is no API to move those credentials to.

use super::*;
use crate::accounts::registry::ProviderDraft;
use crate::accounts::types::{HeaderPair, ModelRoute, ModelSource};

const AGENT_RELATIVE: &str = ".pi/agent";
const CONFIG_RELATIVE: &str = "models.json";
/// Where `/login` keeps the keys it fetched.
pub(super) const AUTH_FILE: &str = "auth.json";
/// pi's own agent dir, which `PI_CODING_AGENT_DIR` moves wholesale.
const AGENT_DIR_VAR: &str = "PI_CODING_AGENT_DIR";

pub fn scan() -> ImportCandidate {
    match home_dir() {
        Some(home) => scan_from(&home),
        None => ImportCandidate {
            source_id: "pi".into(),
            source_name: "pi".into(),
            config_path: None,
            drafts: Vec::new(),
            notes: vec!["No home directory, so no pi config to read.".into()],
            error: None,
        },
    }
}

/// The dir pi's agent config lives in: `~/.pi/agent`, or the one
/// `PI_CODING_AGENT_DIR` names.
pub(super) fn agent_dir(home: &Path) -> PathBuf {
    env_dir(AGENT_DIR_VAR).unwrap_or_else(|| home.join(AGENT_RELATIVE))
}

/// `~/.pi/agent/models.json`, under the agent dir `PI_CODING_AGENT_DIR` names.
pub(super) fn config_path(home: &Path) -> PathBuf {
    agent_dir(home).join(CONFIG_RELATIVE)
}

pub fn scan_from(home: &Path) -> ImportCandidate {
    let dir = agent_dir(home);
    let path = dir.join(CONFIG_RELATIVE);
    let mut candidate = ImportCandidate {
        source_id: "pi".into(),
        source_name: "pi".into(),
        config_path: Some(path.clone()),
        drafts: Vec::new(),
        notes: Vec::new(),
        error: None,
    };

    let config = match read_json(&path) {
        Ok(Some(config)) => config,
        Ok(None) => return candidate,
        Err(error) => {
            candidate.error = Some(error);
            return candidate;
        }
    };
    let catalog = read_optional_json(&dir.join("models-store.json"), &mut candidate);
    let settings = read_optional_json(&dir.join("settings.json"), &mut candidate);
    let auth = read_optional_json(&dir.join(AUTH_FILE), &mut candidate);
    if candidate.error.is_some() {
        return candidate;
    }

    let configured = json_str(&settings, "defaultProvider")
        .zip(json_str(&settings, "defaultModel"))
        .map(|(provider, model)| format!("{provider}/{model}"));
    let effort = json_str(&settings, "defaultThinkingLevel").filter(|level| level != "auto");
    let (drafts, notes) = scan_pi_shape(
        "pi",
        &config,
        &catalog,
        configured.as_deref(),
        effort.as_deref(),
        KeyStyle::Pi,
        &auth,
    );
    candidate.drafts = drafts;
    candidate.notes = notes;
    candidate
}

/// A companion file that may legitimately be absent: a missing one reads as
/// empty, an unreadable or invalid one fails the scan.
pub(super) fn read_optional_json(
    path: &Path,
    candidate: &mut ImportCandidate,
) -> serde_json::Value {
    match read_json(path) {
        Ok(value) => value.unwrap_or_default(),
        Err(error) => {
            candidate.error = Some(error);
            serde_json::Value::Null
        }
    }
}

/// How a provider entry's `apiKey` reads. pi's is the key itself, with `$NAME` /
/// `${NAME}` interpolation and a leading `!command` that runs at request time;
/// omp's names an environment variable when one is set.
pub(super) enum KeyStyle {
    Pi,
    Omp,
}

/// Maps a pi-shaped provider tree — pi's `models.json`, omp's YAML read the same
/// way — onto provider drafts: one per provider with a key, each carrying the
/// models listed for it. Returns the drafts and the notes worth showing.
pub(super) fn scan_pi_shape(
    source: &str,
    config: &serde_json::Value,
    catalog: &serde_json::Value,
    configured: Option<&str>,
    effort: Option<&str>,
    style: KeyStyle,
    auth: &serde_json::Value,
) -> (Vec<ProviderDraft>, Vec<String>) {
    let mut notes = Vec::new();
    let Some(providers) = config
        .get("providers")
        .and_then(serde_json::Value::as_object)
    else {
        notes.push(format!("{source}'s config lists no providers."));
        return (Vec::new(), notes);
    };

    let mut drafts: Vec<ProviderDraft> = Vec::new();
    let mut skipped = Vec::new();
    let mut unsupported = Vec::new();
    let mut unresolved_headers = Vec::new();
    let mut default_imported = configured.is_none();

    for (name, provider) in providers {
        let Some(secret) = provider_secret(provider, &style, auth, name) else {
            skipped.push(name.clone());
            continue;
        };
        let listed = pi_models(
            provider,
            catalog.get(name).unwrap_or(&serde_json::Value::Null),
        );
        let api = json_str(provider, "api")
            .or_else(|| listed.first().and_then(|model| model.api.clone()));
        let raw_base_url = json_str(provider, "baseUrl")
            .or_else(|| listed.first().and_then(|model| model.base_url.clone()));
        let Some(protocol) =
            protocol_for_api(api.as_deref(), raw_base_url.as_deref().unwrap_or_default())
        else {
            unsupported.push(name.clone());
            continue;
        };
        let base_url = raw_base_url.unwrap_or_else(|| protocol.default_base_url().to_string());

        let mut models = Vec::new();
        for model in &listed {
            let duplicate = models
                .iter()
                .any(|existing: &ModelRoute| existing.id.eq_ignore_ascii_case(&model.id));
            if duplicate {
                continue;
            }
            let is_default = configured == Some(format!("{name}/{}", model.id).as_str());
            default_imported |= is_default;
            let default_effort = match (is_default, effort) {
                (true, Some(effort)) => Some(effort.to_string()),
                _ => None,
            };
            models.push(ModelRoute {
                id: model.id.clone(),
                display_name: model.id.clone(),
                context_window: model.context,
                max_output: model.output,
                efforts: Vec::new(),
                default_effort,
                price: None,
                source: ModelSource::Imported,
            });
        }

        let (headers, dropped) = header_pairs(name, provider);
        unresolved_headers.extend(dropped);
        let accounts = api_key_account(name.clone(), source, secret)
            .map(|account| AccountDraft { headers, ..account })
            .into_iter()
            .collect();
        drafts.push(ProviderDraft {
            protocol,
            base_url,
            vendor: None,
            display_name: Some(format!("{name} ({source})")),
            accounts,
            models,
        });
    }

    if !skipped.is_empty() {
        notes.push(format!(
            "Skipped providers with no API key to import ({source} account login): {}",
            skipped.join(", ")
        ));
    }
    if !unsupported.is_empty() {
        notes.push(format!(
            "The Pair has no Gemini-native protocol, so these providers were skipped: {}",
            unsupported.join(", ")
        ));
    }
    if !unresolved_headers.is_empty() {
        notes.push(format!(
            "Dropped headers naming variables that aren't set, so those requests may go out \
             unauthenticated: {}",
            unresolved_headers.join(", ")
        ));
    }
    if let Some(alias) = configured.filter(|_| !default_imported) {
        notes.push(format!(
            "{source}'s default model `{alias}` was not imported."
        ));
    }
    (drafts, notes)
}

/// The secret a provider entry offers, or `None` when there is nothing The Pair
/// can copy across: the entry's `apiKey`, else `apiKeyEnv`, else the key
/// `/login` left in `auth.json`.
fn provider_secret(
    provider: &serde_json::Value,
    style: &KeyStyle,
    auth: &serde_json::Value,
    name: &str,
) -> Option<Secret> {
    if let Some(raw) = json_str(provider, "apiKey") {
        // Commands in models.json run at request time; there is no key to copy.
        if raw.starts_with('!') {
            return None;
        }
        return match style {
            // pi interpolates `$NAME` / `${NAME}` from the environment. A name
            // filling the whole value stays a reference, so rotating it keeps
            // working; anything else is expanded into the key itself.
            KeyStyle::Pi => env_reference(&raw)
                .map(Secret::Env)
                .or_else(|| expand_env_refs(&raw).map(Secret::ApiKey)),
            // omp spells a variable's name on its own. An unset one is still a
            // reference: sending the name as a bearer token would only fail.
            KeyStyle::Omp if is_env_name(&raw) => Some(Secret::Env(raw)),
            KeyStyle::Omp => Some(Secret::ApiKey(raw)),
        };
    }
    if let Some(variable) = json_str(provider, "apiKeyEnv") {
        return Some(Secret::Env(variable));
    }
    auth_key(auth, name)
}

/// The key `/login` stored in `auth.json` for a provider. An account sign-in
/// carries no key, and a command-backed one runs at request time.
fn auth_key(auth: &serde_json::Value, name: &str) -> Option<Secret> {
    let entry = auth.get(name)?;
    if entry
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| !matches!(kind, "api_key" | "api"))
    {
        return None;
    }
    let key = json_str(entry, "key")?;
    (!key.starts_with('!')).then_some(Secret::ApiKey(key))
}

/// A `$NAME` / `${NAME}` that fills a whole value.
fn env_reference(raw: &str) -> Option<String> {
    let name = raw
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .or_else(|| raw.strip_prefix('$'))?;
    let is_name = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    is_name.then(|| name.to_string())
}

/// pi's `$NAME` / `${NAME}` interpolation: every name is replaced with its live
/// value. `None` when one names a variable that is not set, which pi would send
/// as the placeholder.
fn expand_env_refs(raw: &str) -> Option<String> {
    if !raw.contains('$') {
        return Some(raw.to_string());
    }
    let mut expanded = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(index) = rest.find('$') {
        expanded.push_str(&rest[..index]);
        rest = &rest[index..];
        let (name, tail) = match rest.strip_prefix("${") {
            Some(after) => match after.split_once('}') {
                Some((name, tail)) => (name, tail),
                None => return None,
            },
            None => {
                // The name runs to the first character that cannot be part of
                // one; those characters are all ASCII, so bytes and chars agree.
                let len = rest[1..]
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                    .count();
                if len == 0 {
                    return None;
                }
                (&rest[1..1 + len], &rest[1 + len..])
            }
        };
        expanded.push_str(&live_env(name)?);
        rest = tail;
    }
    expanded.push_str(rest);
    Some(expanded)
}

/// An all-caps name like `GATEWAY_API_KEY`: omp spells a variable's name on its
/// own, and a literal key never looks like that.
fn is_env_name(raw: &str) -> bool {
    !raw.is_empty()
        && raw
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

/// One model as pi lists it, in a provider's own `models` or the
/// `models-store.json` catalog.
struct PiModel {
    id: String,
    /// The API the model speaks, when it says so; else the provider's.
    api: Option<String>,
    base_url: Option<String>,
    context: Option<u64>,
    output: Option<u64>,
}

/// The provider's own models, or its catalog entry's when it lists none.
fn pi_models(provider: &serde_json::Value, catalog: &serde_json::Value) -> Vec<PiModel> {
    let inline = provider
        .get("models")
        .and_then(serde_json::Value::as_array)
        .filter(|models| !models.is_empty());
    let listed: Vec<&serde_json::Value> = match inline {
        Some(models) => models.iter().collect(),
        None => catalog
            .get("models")
            .and_then(serde_json::Value::as_array)
            .map(|models| models.iter().collect())
            .unwrap_or_default(),
    };
    listed
        .iter()
        .filter_map(|model| {
            Some(PiModel {
                id: json_str(model, "id")?,
                api: json_str(model, "api"),
                base_url: json_str(model, "baseUrl"),
                context: json_u64(model, "contextWindow"),
                output: json_u64(model, "maxTokens"),
            })
        })
        .collect()
}

/// The protocol pi's `api` names for a provider or model; anything else falls
/// back to the shared host heuristic. `None` for an API The Pair cannot speak.
fn protocol_for_api(api: Option<&str>, base_url: &str) -> Option<Protocol> {
    let kind = match api {
        Some("openai-responses" | "openai-codex-responses" | "azure-openai-responses") => {
            "responses"
        }
        Some("anthropic-messages") => "anthropic",
        // Gemini's own API has no OpenAI-compatible dialect in The Pair.
        Some("google-generative-ai") => return None,
        _ => "openai",
    };
    Some(protocol_from_type(kind, base_url))
}

/// A provider's `headers`, as custom headers on its account, plus the ones that
/// name a variable that is not set. Sending such a placeholder would fail the
/// request, so the header is dropped and reported instead.
fn header_pairs(name: &str, provider: &serde_json::Value) -> (Vec<HeaderPair>, Vec<String>) {
    let Some(headers) = provider
        .get("headers")
        .and_then(serde_json::Value::as_object)
    else {
        return (Vec::new(), Vec::new());
    };
    let mut pairs = Vec::new();
    let mut dropped = Vec::new();
    for (header, value) in headers {
        let Some(value) = value.as_str() else {
            continue;
        };
        match expand_env_refs(value) {
            Some(resolved) => pairs.push(HeaderPair {
                name: header.clone(),
                value: resolved,
            }),
            None => dropped.push(format!("{name}'s {header}")),
        }
    }
    (pairs, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::types::AccountKind;
    use std::fs;

    /// Scans `home` with the environment held still.
    ///
    /// `pi_coding_agent_dir_moves_the_config` points `PI_CODING_AGENT_DIR` at
    /// its own directory while it runs, and every scan reads that variable: a
    /// test scanning at the same moment without the lock reads that test's
    /// config instead of its own.
    fn scan_home(home: &Path) -> ImportCandidate {
        let _guard = crate::test_env::lock_env();
        scan_from(home)
    }

    fn write_files(root: &Path, files: &[(&str, &str)]) {
        for (relative, body) in files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
    }

    #[test]
    fn imports_providers_models_and_the_default_effort() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".pi/agent/models.json",
                    r#"{
                      "providers": {
                        "moonshot": {
                          "api": "openai",
                          "baseUrl": "https://api.moonshot.cn/v1",
                          "apiKey": "sk-pi-1",
                          "models": [
                            { "id": "kimi-k2-0905-preview", "contextWindow": 256000, "maxTokens": 8192 }
                          ]
                        }
                      }
                    }"#,
                ),
                (
                    ".pi/agent/settings.json",
                    r#"{
                      "defaultProvider": "moonshot",
                      "defaultModel": "kimi-k2-0905-preview",
                      "defaultThinkingLevel": "high"
                    }"#,
                ),
            ],
        );

        let candidate = scan_home(&root);
        assert!(candidate.error.is_none());
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.protocol, Protocol::OpenAiChat);
        assert_eq!(draft.base_url, "https://api.moonshot.cn/v1");
        assert_eq!(draft.display_name.as_deref(), Some("moonshot (pi)"));
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(draft.accounts[0].source, "pi");
        assert_eq!(draft.accounts[0].secret, Secret::ApiKey("sk-pi-1".into()));
        assert_eq!(draft.models.len(), 1);
        let model = &draft.models[0];
        assert_eq!(model.id, "kimi-k2-0905-preview");
        assert_eq!(model.display_name, "kimi-k2-0905-preview");
        assert_eq!(model.context_window, Some(256_000));
        assert_eq!(model.max_output, Some(8192));
        assert_eq!(model.default_effort.as_deref(), Some("high"));
        assert_eq!(model.source, ModelSource::Imported);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn anthropic_and_responses_apis_pick_their_protocol() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                      "providers": {
                        "anthropic": {
                          "api": "anthropic-messages",
                          "baseUrl": "https://api.anthropic.com",
                          "apiKey": "sk-ant-1"
                        },
                        "openai": {
                          "api": "openai-responses",
                          "apiKey": "sk-oai-1",
                          "models": [{ "id": "gpt-5-codex", "api": "openai-responses" }]
                        }
                      }
                    }"#,
            )],
        );

        let candidate = scan_home(&root);
        assert!(candidate.error.is_none());
        let anthropic = candidate
            .drafts
            .iter()
            .find(|draft| draft.base_url == "https://api.anthropic.com")
            .unwrap();
        assert_eq!(anthropic.protocol, Protocol::Anthropic);
        let openai = candidate
            .drafts
            .iter()
            .find(|draft| draft.accounts.len() == 1 && draft.protocol == Protocol::Responses)
            .unwrap();
        assert_eq!(openai.base_url, "https://api.openai.com/v1");
        assert_eq!(openai.models.len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_variable_in_an_api_key_is_kept_as_a_reference() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                  "providers": {
                    "gateway": {
                      "api": "openai",
                      "baseUrl": "https://llm.internal.example/v1",
                      "apiKey": "$GATEWAY_API_KEY"
                    },
                    "braced": {
                      "api": "openai",
                      "baseUrl": "https://braced.internal.example/v1",
                      "apiKey": "${GATEWAY_API_KEY}"
                    },
                    "vaulted": {
                      "api": "openai",
                      "baseUrl": "https://vault.internal.example/v1",
                      "apiKey": "!security find-generic-password -w"
                    }
                  }
                }"#,
            )],
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("GATEWAY_API_KEY");
        std::env::set_var("GATEWAY_API_KEY", "sk-live");

        let candidate = scan_from(&root);
        let gateway = candidate
            .drafts
            .iter()
            .find(|draft| draft.base_url.contains("llm.internal"))
            .unwrap();
        assert_eq!(
            gateway.accounts[0].secret,
            Secret::Env("GATEWAY_API_KEY".into()),
            "the placeholder must not become the key"
        );
        let braced = candidate
            .drafts
            .iter()
            .find(|draft| draft.base_url.contains("braced.internal"))
            .unwrap();
        assert_eq!(
            braced.accounts[0].secret,
            Secret::Env("GATEWAY_API_KEY".into())
        );
        assert!(
            candidate.notes.iter().any(|note| note.contains("vaulted")),
            "{:?}",
            candidate.notes
        );

        // An unset variable leaves the account needing a key rather than
        // sending the name as one.
        std::env::remove_var("GATEWAY_API_KEY");
        let unset = scan_from(&root);
        let gateway = unset
            .drafts
            .iter()
            .find(|draft| draft.base_url.contains("llm.internal"))
            .unwrap();
        assert_eq!(gateway.accounts[0].kind, AccountKind::Env);
        assert!(!gateway.accounts[0].secret.is_present());

        match saved {
            Some(value) => std::env::set_var("GATEWAY_API_KEY", value),
            None => std::env::remove_var("GATEWAY_API_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_interpolated_key_is_expanded_and_an_unset_one_skips_the_provider() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                  "providers": {
                    "prefix": {
                      "api": "openai",
                      "baseUrl": "https://llm.internal.example/v1",
                      "apiKey": "sk-$KEY_SUFFIX"
                    }
                  }
                }"#,
            )],
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("KEY_SUFFIX");
        std::env::set_var("KEY_SUFFIX", "live");

        let candidate = scan_from(&root);
        assert_eq!(
            candidate.drafts[0].accounts[0].secret,
            Secret::ApiKey("sk-live".into())
        );

        std::env::remove_var("KEY_SUFFIX");
        let unset = scan_from(&root);
        assert!(unset.drafts.is_empty(), "the placeholder is not a key");
        assert!(unset.notes[0].contains("prefix"), "{}", unset.notes[0]);

        match saved {
            Some(value) => std::env::set_var("KEY_SUFFIX", value),
            None => std::env::remove_var("KEY_SUFFIX"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_header_naming_a_variable_is_expanded_or_dropped() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                  "providers": {
                    "gateway": {
                      "api": "openai",
                      "baseUrl": "https://llm.internal.example/v1",
                      "apiKey": "sk-pi-1",
                      "headers": {
                        "X-Gateway-Key": "$GATEWAY_API_KEY",
                        "X-Tenant": "acme"
                      }
                    }
                  }
                }"#,
            )],
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("GATEWAY_API_KEY");
        std::env::set_var("GATEWAY_API_KEY", "gateway-token");

        let candidate = scan_from(&root);
        assert_eq!(
            candidate.drafts[0].accounts[0].headers,
            vec![
                HeaderPair {
                    name: "X-Gateway-Key".into(),
                    value: "gateway-token".into()
                },
                HeaderPair {
                    name: "X-Tenant".into(),
                    value: "acme".into()
                },
            ]
        );
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);

        std::env::remove_var("GATEWAY_API_KEY");
        let unset = scan_from(&root);
        assert_eq!(
            unset.drafts[0].accounts[0].headers,
            vec![HeaderPair {
                name: "X-Tenant".into(),
                value: "acme".into()
            }]
        );
        assert!(
            unset
                .notes
                .iter()
                .any(|note| note.contains("X-Gateway-Key")),
            "{:?}",
            unset.notes
        );

        match saved {
            Some(value) => std::env::set_var("GATEWAY_API_KEY", value),
            None => std::env::remove_var("GATEWAY_API_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn auth_json_supplies_the_key_login_fetched() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".pi/agent/models.json",
                    r#"{
                      "providers": {
                        "anthropic": { "api": "anthropic-messages", "baseUrl": "https://api.anthropic.com" },
                        "signed-in": { "api": "anthropic-messages", "baseUrl": "https://api.anthropic.com" },
                        "vaulted": { "api": "openai", "baseUrl": "https://llm.internal.example/v1" }
                      }
                    }"#,
                ),
                (
                    ".pi/agent/auth.json",
                    r#"{
                      "anthropic": { "type": "api_key", "key": "sk-auth" },
                      "signed-in": { "type": "oauth", "access": "tok" },
                      "vaulted": { "type": "api_key", "key": "!security find-generic-password -w" }
                    }"#,
                ),
            ],
        );

        let candidate = scan_home(&root);
        assert!(candidate.error.is_none());
        assert_eq!(candidate.drafts.len(), 1, "only the api_key entry imports");
        assert_eq!(
            candidate.drafts[0].accounts[0].secret,
            Secret::ApiKey("sk-auth".into())
        );
        assert_eq!(
            candidate.drafts[0].display_name.as_deref(),
            Some("anthropic (pi)")
        );
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("signed-in") && note.contains("vaulted")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_gemini_native_api_is_skipped_with_a_note() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                  "providers": {
                    "google": {
                      "api": "google-generative-ai",
                      "apiKey": "sk-google",
                      "models": [{ "id": "gemini-2.5-pro" }]
                    },
                    "moonshot": {
                      "api": "openai",
                      "baseUrl": "https://api.moonshot.cn/v1",
                      "apiKey": "sk-pi-1"
                    }
                  }
                }"#,
            )],
        );

        let candidate = scan_home(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].base_url, "https://api.moonshot.cn/v1");
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("google") && note.contains("Gemini-native")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_config_is_not_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        let candidate = scan_home(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.notes.is_empty());
        assert!(candidate.error.is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_config_reports_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(&root, &[(".pi/agent/models.json", "not json at all")]);
        let candidate = scan_home(&root);
        assert!(candidate.error.is_some());
        assert!(candidate.drafts.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_catalog_reports_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (".pi/agent/models.json", r#"{"providers": {}}"#),
                (".pi/agent/models-store.json", "{oops"),
            ],
        );
        let candidate = scan_home(&root);
        assert!(candidate.error.is_some());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_auth_json_reports_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (".pi/agent/models.json", r#"{"providers": {}}"#),
                (".pi/agent/auth.json", "{oops"),
            ],
        );
        let candidate = scan_home(&root);
        assert!(candidate.error.is_some());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn skips_providers_without_a_key_and_says_so() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                  "providers": {
                    "moonshot": {
                      "api": "openai",
                      "baseUrl": "https://api.moonshot.cn/v1",
                      "apiKey": "sk-pi-1"
                    },
                    "anthropic-account": {
                      "api": "anthropic-messages",
                      "baseUrl": "https://api.anthropic.com"
                    }
                  }
                }"#,
            )],
        );

        let candidate = scan_home(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].base_url, "https://api.moonshot.cn/v1");
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("anthropic-account"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn env_key_reference_becomes_an_env_account() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                  "providers": {
                    "moonshot": {
                      "api": "openai",
                      "baseUrl": "https://api.moonshot.cn/v1",
                      "apiKeyEnv": "THE_PAIR_TEST_PI_KEY"
                    }
                  }
                }"#,
            )],
        );
        let _guard = crate::test_env::lock_env();
        std::env::set_var("THE_PAIR_TEST_PI_KEY", "sk-live");

        let candidate = scan_from(&root);
        let account = &candidate.drafts[0].accounts[0];
        assert_eq!(account.kind, AccountKind::Env);
        assert_eq!(account.secret, Secret::Env("THE_PAIR_TEST_PI_KEY".into()));
        assert!(account.secret.is_present());

        std::env::remove_var("THE_PAIR_TEST_PI_KEY");
        let candidate = scan_from(&root);
        let account = &candidate.drafts[0].accounts[0];
        assert_eq!(account.secret, Secret::Env("THE_PAIR_TEST_PI_KEY".into()));
        assert!(!account.secret.is_present());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn catalog_supplies_models_when_the_provider_lists_none() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".pi/agent/models.json",
                    r#"{
                      "providers": {
                        "moonshot": {
                          "apiKey": "sk-pi-1"
                        }
                      }
                    }"#,
                ),
                (
                    ".pi/agent/models-store.json",
                    r#"{
                      "moonshot": {
                        "models": [
                          {
                            "id": "kimi-k2-0905-preview",
                            "api": "anthropic-messages",
                            "baseUrl": "https://api.moonshot.cn/v1",
                            "contextWindow": 128000,
                            "maxTokens": 4096
                          }
                        ]
                      }
                    }"#,
                ),
            ],
        );

        let candidate = scan_home(&root);
        assert!(candidate.error.is_none());
        let draft = &candidate.drafts[0];
        assert_eq!(draft.protocol, Protocol::Anthropic);
        assert_eq!(draft.base_url, "https://api.moonshot.cn/v1");
        assert_eq!(draft.models.len(), 1);
        assert_eq!(draft.models[0].id, "kimi-k2-0905-preview");
        assert_eq!(draft.models[0].context_window, Some(128_000));
        assert_eq!(draft.models[0].max_output, Some(4096));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn duplicate_model_ids_are_deduped() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                  "providers": {
                    "moonshot": {
                      "api": "openai",
                      "baseUrl": "https://api.moonshot.cn/v1",
                      "apiKey": "sk-pi-1",
                      "models": [
                        { "id": "kimi-k2-0905-preview" },
                        { "id": "KIMI-K2-0905-PREVIEW" }
                      ]
                    }
                  }
                }"#,
            )],
        );

        let candidate = scan_home(&root);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_default_model_that_was_not_imported_is_noted() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".pi/agent/models.json",
                    r#"{
                      "providers": {
                        "moonshot": {
                          "api": "openai",
                          "baseUrl": "https://api.moonshot.cn/v1",
                          "apiKey": "sk-pi-1",
                          "models": [{ "id": "kimi-k2-0905-preview" }]
                        }
                      }
                    }"#,
                ),
                (
                    ".pi/agent/settings.json",
                    r#"{
                      "defaultProvider": "gone",
                      "defaultModel": "missing",
                      "defaultThinkingLevel": "auto"
                    }"#,
                ),
            ],
        );

        let candidate = scan_home(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert_eq!(candidate.drafts[0].models[0].default_effort, None);
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("gone/missing"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn custom_headers_ride_along_with_the_account() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".pi/agent/models.json",
                r#"{
                  "providers": {
                    "gateway": {
                      "api": "openai",
                      "baseUrl": "https://llm.internal.example/v1",
                      "apiKey": "sk-pi-1",
                      "headers": { "x-tenant": "acme", "x-non-string": 3 }
                    }
                  }
                }"#,
            )],
        );

        let candidate = scan_home(&root);
        let headers = &candidate.drafts[0].accounts[0].headers;
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].name, "x-tenant");
        assert_eq!(headers[0].value, "acme");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn pi_coding_agent_dir_moves_the_config() {
        let root = std::env::temp_dir().join(format!("the-pair-pi-{}", uuid::Uuid::new_v4()));
        let home = root.join("home");
        let agent = root.join("elsewhere");
        fs::create_dir_all(&agent).unwrap();
        fs::write(
            agent.join("models.json"),
            r#"{
              "providers": {
                "moonshot": {
                  "api": "openai",
                  "baseUrl": "https://api.moonshot.cn/v1",
                  "apiKey": "sk-pi-1"
                }
              }
            }"#,
        )
        .unwrap();
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("PI_CODING_AGENT_DIR");
        std::env::set_var("PI_CODING_AGENT_DIR", &agent);

        let candidate = scan_from(&home);
        assert_eq!(candidate.config_path, Some(agent.join("models.json")));
        assert_eq!(candidate.drafts.len(), 1);

        match saved {
            Some(value) => std::env::set_var("PI_CODING_AGENT_DIR", value),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
        fs::remove_dir_all(&root).ok();
    }
}
