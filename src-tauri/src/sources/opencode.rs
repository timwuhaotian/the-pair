//! opencode: `opencode.json` plus the keys `/connect` left in `auth.json`.
//!
//! Shape (verified against opencode's own config docs):
//!
//! ```json
//! {
//!   "model": "anthropic/claude-sonnet-4-5",
//!   "provider": {
//!     "anthropic": {
//!       "npm": "@ai-sdk/anthropic",
//!       "models": {
//!         "claude-sonnet-4-5": {
//!           "name": "Claude Sonnet 4.5",
//!           "limit": { "context": 200000, "output": 8192 },
//!           "reasoning": true
//!         }
//!       },
//!       "options": {
//!         "baseURL": "https://api.anthropic.com",
//!         "apiKey": "sk-ant-..."
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! The key may also live in `auth.json`, one entry per provider:
//!
//! ```json
//! { "anthropic": { "type": "api", "key": "sk-ant-..." } }
//! ```
//!
//! A config value may name a variable (`{env:NAME}`) or a file
//! (`{file:~/.secrets/openai-key}`) instead of holding the key: the first stays
//! a reference, the second is read, and one that cannot be read is declined
//! rather than offered as a ready account holding a placeholder.
//!
//! Providers opencode signs into with its own account record `"type": "oauth"`
//! there and carry no key to import, so they are skipped.

use super::*;
use crate::accounts::registry::ProviderDraft;
use crate::accounts::types::{ModelPrice, ModelRoute, ModelSource};

pub fn scan() -> ImportCandidate {
    scan_from(
        crate::config_paths::opencode_config_path().as_deref(),
        crate::config_paths::opencode_auth_path().as_deref(),
    )
}

pub fn scan_from(config_path: Option<&Path>, auth_path: Option<&Path>) -> ImportCandidate {
    let mut candidate = ImportCandidate {
        source_id: "opencode".into(),
        source_name: "opencode".into(),
        config_path: config_path.map(Path::to_path_buf),
        drafts: Vec::new(),
        notes: Vec::new(),
        error: None,
    };

    let config = match config_path {
        Some(path) => match read_json(path) {
            Ok(value) => value,
            Err(error) => {
                candidate.error = Some(error);
                return candidate;
            }
        },
        None => {
            candidate
                .notes
                .push("No home directory, so no opencode config to read.".into());
            return candidate;
        }
    };
    let Some(config) = config else {
        return candidate;
    };
    let auth = auth_path
        .map(|path| read_json(path))
        .unwrap_or(Ok(None))
        .unwrap_or_else(|error| {
            candidate.error = Some(error);
            None
        })
        .unwrap_or_default();

    let Some(providers) = config
        .get("provider")
        .and_then(serde_json::Value::as_object)
    else {
        return candidate;
    };

    // Provider name -> draft, so the configured default model can be checked
    // against what was actually imported.
    let mut drafts: Vec<(String, ProviderDraft)> = Vec::new();
    let mut signed_in = Vec::new();
    let mut keyless = Vec::new();
    let mut unreadable_keys = Vec::new();

    for (name, provider) in providers {
        let npm = json_str(provider, "npm");
        let base_url = provider
            .get("options")
            .and_then(|options| json_str(options, "baseURL"))
            .unwrap_or_else(|| {
                wire_protocol(npm.as_deref(), "")
                    .default_base_url()
                    .to_string()
            });
        let protocol = wire_protocol(npm.as_deref(), &base_url);
        let Some(secret) = credential(provider, &auth, name, &mut unreadable_keys) else {
            if is_oauth_signin(&auth, name) {
                signed_in.push(name.clone());
            } else {
                keyless.push(name.clone());
            }
            continue;
        };
        let mut models = Vec::new();
        push_models(provider, &mut models);

        drafts.push((
            name.clone(),
            ProviderDraft {
                protocol,
                base_url,
                vendor: None,
                display_name: Some(format!("{} (opencode)", name)),
                accounts: api_key_account(name.clone(), "opencode", secret)
                    .into_iter()
                    .collect(),
                models,
            },
        ));
    }

    if !signed_in.is_empty() {
        candidate.notes.push(format!(
            "Skipped providers opencode signs into with its own account (no API key to import): {}",
            signed_in.join(", ")
        ));
    }
    if !keyless.is_empty() {
        candidate.notes.push(format!(
            "Skipped providers with no API key in opencode's config or auth.json: {}",
            keyless.join(", ")
        ));
    }
    if !unreadable_keys.is_empty() {
        candidate.notes.push(format!(
            "Skipped providers whose key file could not be read, so their key is still in the \
             file opencode points at: {}",
            unreadable_keys.join(", ")
        ));
    }
    if let Some(configured) = json_str(&config, "model") {
        let (provider_name, model_id) = match configured.split_once('/') {
            Some((provider, model)) => (provider, model),
            None => ("", configured.as_str()),
        };
        let imported = drafts.iter().any(|(name, draft)| {
            (provider_name.is_empty() || name == provider_name)
                && draft.models.iter().any(|model| model.id == model_id)
        });
        if !imported {
            candidate.notes.push(format!(
                "opencode's default model `{}` was not imported.",
                configured
            ));
        }
    }

    candidate.drafts = drafts.into_iter().map(|(_, draft)| draft).collect();
    candidate
}

/// The key opencode keeps for a provider: the config's `options.apiKey`, else
/// the API key `/connect` stored in `auth.json`. An OAuth sign-in has no key.
///
/// A `{file:…}` reference is read (with `~` expanded) because opencode reads it
/// at request time; one that is missing, unreadable or blank is declined and
/// reported in `unreadable`, since the placeholder would only fail every
/// request while showing as ready.
fn credential(
    provider: &serde_json::Value,
    auth: &serde_json::Value,
    name: &str,
    unreadable: &mut Vec<String>,
) -> Option<Secret> {
    let api_key = provider
        .get("options")
        .and_then(|options| json_str(options, "apiKey"));
    if let Some(path) = api_key.as_deref().and_then(file_reference) {
        return match read_key_file(&path) {
            Some(key) => Some(Secret::ApiKey(key)),
            None => {
                unreadable.push(name.to_string());
                None
            }
        };
    }
    // A config value may name a variable instead of holding the key.
    let referenced = api_key.as_deref().and_then(env_reference);
    let inline = api_key.filter(|_| referenced.is_none());
    if let Some(secret) = secret_from_key_fields(inline, referenced) {
        return Some(secret);
    }
    let entry = auth.get(name)?;
    if entry.get("type").and_then(serde_json::Value::as_str) != Some("api") {
        return None;
    }
    json_str(entry, "key").map(Secret::ApiKey)
}

/// opencode's `{env:NAME}` placeholder in a config string.
fn env_reference(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix("{env:")?.strip_suffix('}')?;
    (!inner.is_empty()).then(|| inner.to_string())
}

/// opencode's `{file:PATH}` placeholder, with `~` expanded.
fn file_reference(raw: &str) -> Option<PathBuf> {
    let inner = raw.strip_prefix("{file:")?.strip_suffix('}')?;
    (!inner.is_empty()).then(|| expand_tilde(inner))
}

/// `~` and `~/…` as the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    let stripped = match path.strip_prefix("~/") {
        Some(rest) => Some(rest),
        None => (path == "~").then_some(""),
    };
    match stripped {
        Some(rest) => home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

/// The trimmed contents of a key file: `None` when it is missing, unreadable or
/// blank, all of which would otherwise become a key that cannot work.
fn read_key_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|key| !key.is_empty())
}

/// Whether `auth.json` records an account sign-in rather than a pasted key.
fn is_oauth_signin(auth: &serde_json::Value, name: &str) -> bool {
    auth.get(name)
        .and_then(|entry| entry.get("type"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind != "api")
}

/// opencode's `npm` package as a wire protocol. The AI SDK packages name the
/// protocol directly; anything else falls back to the endpoint's host.
fn wire_protocol(npm: Option<&str>, base_url: &str) -> Protocol {
    let kind = match npm {
        Some("@ai-sdk/anthropic") => "anthropic",
        Some("@ai-sdk/openai") | Some("@ai-sdk/openai-compatible") => "openai",
        _ => "",
    };
    protocol_from_type(kind, base_url)
}

/// `models` is a map of id to entry, or an array of entries carrying their id.
fn push_models(provider: &serde_json::Value, out: &mut Vec<ModelRoute>) {
    match provider.get("models") {
        Some(serde_json::Value::Object(map)) => {
            for (id, model) in map {
                push_model(out, id, model);
            }
        }
        Some(serde_json::Value::Array(items)) => {
            for item in items {
                let Some(id) = json_str(item, "id") else {
                    continue;
                };
                push_model(out, &id, item);
            }
        }
        _ => {}
    }
}

/// Keeps the first entry for an id, since the array form can repeat one.
fn push_model(out: &mut Vec<ModelRoute>, id: &str, model: &serde_json::Value) {
    let route = ModelRoute {
        id: id.to_string(),
        display_name: json_str(model, "name").unwrap_or_else(|| id.to_string()),
        context_window: model
            .get("limit")
            .and_then(|limit| json_u64(limit, "context")),
        max_output: model
            .get("limit")
            .and_then(|limit| json_u64(limit, "output")),
        efforts: Vec::new(),
        default_effort: model
            .get("options")
            .and_then(|options| json_str(options, "reasoningEffort")),
        price: model.get("cost").and_then(|cost| {
            let input = cost.get("input").and_then(serde_json::Value::as_f64)?;
            let output = cost.get("output").and_then(serde_json::Value::as_f64)?;
            Some(ModelPrice {
                input_per_million: input,
                output_per_million: output,
            })
        }),
        source: ModelSource::Imported,
    };
    if out.iter().any(|existing| existing.id == route.id) {
        return;
    }
    out.push(route);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// A config dir holding `opencode.json` and a data dir holding `auth.json`.
    fn paths(root: &Path) -> (PathBuf, PathBuf) {
        (
            root.join("config/opencode/opencode.json"),
            root.join("data/opencode/auth.json"),
        )
    }

    #[test]
    fn imports_providers_models_and_keys() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(
            &config_path,
            r#"{
              "model": "anthropic/claude-sonnet-4-5",
              "provider": {
                "anthropic": {
                  "npm": "@ai-sdk/anthropic",
                  "models": {
                    "claude-sonnet-4-5": {
                      "name": "Claude Sonnet 4.5",
                      "limit": { "context": 200000, "output": 8192 },
                      "cost": { "input": 3, "output": 15 }
                    }
                  },
                  "options": { "baseURL": "https://api.anthropic.com" }
                }
              }
            }"#,
        );
        write(
            &auth_path,
            r#"{ "anthropic": { "type": "api", "key": "sk-ant-auth" } }"#,
        );

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        assert!(candidate.error.is_none());
        assert_eq!(candidate.source_id, "opencode");
        assert_eq!(candidate.config_path, Some(config_path));
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.protocol, Protocol::Anthropic);
        assert_eq!(draft.base_url, "https://api.anthropic.com");
        assert_eq!(draft.display_name.as_deref(), Some("anthropic (opencode)"));
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(draft.accounts[0].source, "opencode");
        assert_eq!(
            draft.accounts[0].secret,
            Secret::ApiKey("sk-ant-auth".into())
        );
        assert_eq!(draft.models.len(), 1);
        assert_eq!(draft.models[0].id, "claude-sonnet-4-5");
        assert_eq!(draft.models[0].display_name, "Claude Sonnet 4.5");
        assert_eq!(draft.models[0].context_window, Some(200_000));
        assert_eq!(draft.models[0].max_output, Some(8192));
        assert_eq!(
            draft.models[0].price.as_ref().unwrap().input_per_million,
            3.0
        );
        assert_eq!(draft.models[0].source, ModelSource::Imported);
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_config_key_wins_over_the_one_in_auth_json() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(
            &config_path,
            r#"{
              "provider": {
                "openai": {
                  "npm": "@ai-sdk/openai",
                  "models": { "gpt-5": {} },
                  "options": { "baseURL": "https://api.openai.com/v1", "apiKey": "sk-inline" }
                }
              }
            }"#,
        );
        write(
            &auth_path,
            r#"{ "openai": { "type": "api", "key": "sk-auth" } }"#,
        );

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        assert_eq!(candidate.drafts.len(), 1);
        // `@ai-sdk/openai` at api.openai.com is the Responses API, the same
        // endpoint the env source maps to Responses.
        assert_eq!(candidate.drafts[0].protocol, Protocol::Responses);
        assert_eq!(
            candidate.drafts[0].accounts[0].secret,
            Secret::ApiKey("sk-inline".into())
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_file_reference_is_read_and_an_unreadable_one_is_declined() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        let secrets = root.join("secrets");
        fs::create_dir_all(&secrets).unwrap();
        fs::write(secrets.join("openai-key"), "sk-from-file\n").unwrap();
        write(
            &config_path,
            &format!(
                r#"{{
                  "provider": {{
                    "openai": {{
                      "npm": "@ai-sdk/openai",
                      "models": {{ "gpt-5": {{}} }},
                      "options": {{
                        "baseURL": "https://api.openai.com/v1",
                        "apiKey": "{{file:{}}}"
                      }}
                    }},
                    "missing": {{
                      "npm": "@ai-sdk/openai-compatible",
                      "models": {{ "m": {{}} }},
                      "options": {{
                        "baseURL": "https://gone.internal.example/v1",
                        "apiKey": "{{file:{}/not-there}}"
                      }}
                    }}
                  }}
                }}"#,
                secrets.join("openai-key").display(),
                secrets.display()
            ),
        );

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(
            candidate.drafts[0].accounts[0].secret,
            Secret::ApiKey("sk-from-file".into()),
            "the file's contents are the key"
        );
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("missing") && note.contains("key file")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_config_is_not_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        assert!(candidate.notes.is_empty());

        let without_home = scan_from(None, None);
        assert!(without_home.drafts.is_empty());
        assert!(without_home.error.is_none());
        assert!(without_home.notes.iter().any(|note| note.contains("home")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_config_reports_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(&config_path, "{ not json");

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        assert!(candidate.drafts.is_empty());
        assert!(candidate
            .error
            .as_deref()
            .is_some_and(|error| error.contains("opencode.json")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn skips_account_login_providers_and_says_so() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(
            &config_path,
            r#"{
              "provider": {
                "opencode": { "npm": "@opencode-ai/sdk", "models": { "big-pickle": {} } },
                "moonshot": {
                  "npm": "@ai-sdk/openai-compatible",
                  "models": { "kimi-k2": {} },
                  "options": { "baseURL": "https://api.moonshot.cn/v1", "apiKey": "sk-moonshot" }
                }
              }
            }"#,
        );
        write(
            &auth_path,
            r#"{ "opencode": { "type": "oauth", "access": "tok", "refresh": "ref" } }"#,
        );

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].base_url, "https://api.moonshot.cn/v1");
        assert_eq!(candidate.drafts[0].accounts.len(), 1);
        assert_eq!(candidate.notes.len(), 1, "{:?}", candidate.notes);
        assert!(
            candidate.notes[0].contains("opencode"),
            "{}",
            candidate.notes[0]
        );
        assert!(
            candidate.notes[0].contains("own account"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn keyless_providers_are_listed_separately() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(
            &config_path,
            r#"{
              "provider": {
                "ollama": {
                  "npm": "@ai-sdk/openai-compatible",
                  "models": { "qwen3": {} },
                  "options": { "baseURL": "http://localhost:11434/v1" }
                }
              }
            }"#,
        );

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("ollama"),
            "{}",
            candidate.notes[0]
        );
        assert!(
            candidate.notes[0].contains("no API key"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_env_placeholder_becomes_an_env_account() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(
            &config_path,
            r#"{
              "provider": {
                "openrouter": {
                  "npm": "@openrouter/ai-sdk-provider",
                  "models": { "gpt-5": {} },
                  "options": {
                    "baseURL": "https://openrouter.ai/api/v1",
                    "apiKey": "{env:THE_PAIR_TEST_OPENROUTER_KEY}"
                  }
                }
              }
            }"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_OPENROUTER_KEY");

        std::env::set_var("THE_PAIR_TEST_OPENROUTER_KEY", "sk-live");
        let live = scan_from(Some(&config_path), Some(&auth_path));
        let account = &live.drafts[0].accounts[0];
        assert_eq!(account.kind, crate::accounts::types::AccountKind::Env);
        assert_eq!(
            account.secret,
            Secret::Env("THE_PAIR_TEST_OPENROUTER_KEY".into())
        );

        std::env::remove_var("THE_PAIR_TEST_OPENROUTER_KEY");
        let unset = scan_from(Some(&config_path), Some(&auth_path));
        assert!(
            unset.drafts.is_empty(),
            "an unset variable is no credential"
        );
        assert!(unset.notes[0].contains("openrouter"), "{}", unset.notes[0]);

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_OPENROUTER_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_OPENROUTER_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn array_models_are_imported_and_duplicates_dropped() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(
            &config_path,
            r#"{
              "provider": {
                "gateway": {
                  "npm": "@ai-sdk/openai-compatible",
                  "models": [
                    { "id": "internal-1", "name": "Internal 1" },
                    { "id": "internal-1", "name": "Internal 1 again" },
                    { "id": "internal-2", "limit": { "context": 128000 } }
                  ],
                  "options": { "baseURL": "https://llm.internal.example/v1", "apiKey": "sk-gw" }
                }
              }
            }"#,
        );

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        let models = &candidate.drafts[0].models;
        assert_eq!(models.len(), 2, "the repeated id must collapse");
        assert_eq!(models[0].display_name, "Internal 1");
        assert_eq!(models[1].context_window, Some(128_000));
        assert_eq!(models[1].display_name, "internal-2");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unimported_default_model_is_reported() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(
            &config_path,
            r#"{
              "model": "opencode/big-pickle",
              "provider": {
                "moonshot": {
                  "npm": "@ai-sdk/openai-compatible",
                  "models": { "kimi-k2": {} },
                  "options": { "baseURL": "https://api.moonshot.cn/v1", "apiKey": "sk-moonshot" }
                }
              }
            }"#,
        );
        write(&auth_path, r#"{ "opencode": { "type": "oauth" } }"#);

        let candidate = scan_from(Some(&config_path), Some(&auth_path));
        assert_eq!(candidate.drafts.len(), 1);
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("opencode/big-pickle")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_follows_the_opencode_env_overrides() {
        let root = std::env::temp_dir().join(format!("the-pair-opencode-{}", uuid::Uuid::new_v4()));
        let (config_path, auth_path) = paths(&root);
        write(
            &config_path,
            r#"{
              "provider": {
                "anthropic": {
                  "npm": "@ai-sdk/anthropic",
                  "models": { "claude-sonnet-4-5": {} }
                }
              }
            }"#,
        );
        write(
            &auth_path,
            r#"{ "anthropic": { "type": "api", "key": "sk-env" } }"#,
        );
        let _guard = crate::test_env::lock_env();
        let keys = ["OPENCODE_CONFIG_DIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME"];
        let saved: Vec<_> = keys
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
        std::env::set_var("OPENCODE_CONFIG_DIR", config_path.parent().unwrap());
        std::env::set_var(
            "XDG_DATA_HOME",
            auth_path.parent().unwrap().parent().unwrap(),
        );

        let candidate = scan();
        assert_eq!(candidate.config_path, Some(config_path.clone()));
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(
            candidate.drafts[0].accounts[0].secret,
            Secret::ApiKey("sk-env".into())
        );

        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        fs::remove_dir_all(&root).ok();
    }
}
