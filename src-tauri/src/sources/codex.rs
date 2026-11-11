//! Codex CLI: `~/.codex/config.toml`.
//!
//! Shape (verified against Codex's own config docs):
//!
//! ```toml
//! model = "gpt-5-codex"
//! model_provider = "gateway"
//! model_context_window = 272000
//! model_reasoning_effort = "high"
//!
//! [model_providers.gateway]
//! name = "Internal gateway"
//! base_url = "https://llm.internal.example/v1"
//! env_key = "INTERNAL_KEY"
//! wire_api = "responses"
//! http_headers = { "X-Tenant" = "acme" }
//! env_http_headers = { "X-Governance-Token" = "CODER_API_KEY" }
//! query_params = { "tenant" = "acme" }
//! ```
//!
//! `wire_api` is read for the note it deserves and nothing else: upstream
//! `WireApi` has exactly one variant left, `Responses`, with `#[serde(default)]`,
//! and `"chat"` is a hard deserialize error — so every real entry is a
//! Responses endpoint, with or without the field. `env_http_headers` names
//! variables, so each is resolved before it is stored.
//!
//! `~/.codex/auth.json` is deliberately left alone: it holds the ChatGPT
//! account sign-in, which The Pair performs through its own OAuth flow instead
//! of importing. A provider that names no `env_key` (Codex's built-in `openai`)
//! is therefore skipped — its credentials are not an API key — as is one that
//! authenticates with `experimental_bearer_token` or a command-backed `auth`,
//! neither of which The Pair has a record for.

use super::*;
use crate::accounts::registry::{AccountDraft, ProviderDraft};
use crate::accounts::types::{AccountKind, HeaderPair, ModelRoute, ModelSource};

const CONFIG_RELATIVE: &str = ".codex/config.toml";

pub fn scan() -> ImportCandidate {
    match home_dir() {
        Some(home) => scan_from(&home),
        None => ImportCandidate {
            source_id: "codex".into(),
            source_name: "Codex".into(),
            config_path: None,
            drafts: Vec::new(),
            notes: vec!["No home directory, so no Codex config to read.".into()],
            error: None,
        },
    }
}

pub fn scan_from(root: &Path) -> ImportCandidate {
    let path = root.join(CONFIG_RELATIVE);
    let mut candidate = ImportCandidate {
        source_id: "codex".into(),
        source_name: "Codex".into(),
        config_path: Some(path.clone()),
        drafts: Vec::new(),
        notes: Vec::new(),
        error: None,
    };

    let Some(config) = read_toml(&path).unwrap_or_else(|error| {
        candidate.error = Some(error);
        None
    }) else {
        return candidate;
    };

    let mut drafts: Vec<(String, ProviderDraft)> = Vec::new();
    let mut skipped = Vec::new();
    let mut unset_keys = Vec::new();
    let mut dropped_params = Vec::new();
    let mut unset_env_headers = Vec::new();
    let mut unsupported_auth = Vec::new();
    let mut unknown_wire_api = Vec::new();

    if let Some(toml::Value::Table(all)) = config.get("model_providers") {
        for (name, provider) in all {
            let Some(table) = provider.as_table() else {
                continue;
            };
            // Neither spelling has a credential The Pair could send, so the
            // provider is reported rather than imported half-configured.
            let unsupported = if toml_str(table, "experimental_bearer_token").is_some() {
                Some("experimental_bearer_token")
            } else if table
                .get("auth")
                .and_then(toml::Value::as_table)
                .is_some_and(|auth| auth.contains_key("command"))
            {
                Some("a command-backed `auth`")
            } else {
                None
            };
            if let Some(reason) = unsupported {
                unsupported_auth.push(format!("{} ({})", name, reason));
                continue;
            }
            let base_url = toml_str(table, "base_url");
            let env_key = toml_str(table, "env_key");
            let (Some(base_url), Some(env_key)) = (base_url, env_key) else {
                skipped.push(name.clone());
                continue;
            };
            if live_env(&env_key).is_none() {
                unset_keys.push(format!("{} (${})", name, env_key));
            }
            if let Some(toml::Value::Table(params)) = table.get("query_params") {
                if !params.is_empty() {
                    dropped_params.push(name.clone());
                }
            }
            // `WireApi` has one variant left, `Responses`, and it is the
            // default: `"chat"` is a hard deserialize error upstream, so it
            // cannot be what a working config says.
            let protocol = match toml_str(table, "wire_api").as_deref() {
                Some("responses") | Some("chat") | None => Protocol::Responses,
                Some(other) => {
                    unknown_wire_api.push(format!("{} (`{}`)", name, other));
                    Protocol::Responses
                }
            };
            let label = toml_str(table, "name").unwrap_or_else(|| name.clone());
            drafts.push((
                name.clone(),
                ProviderDraft {
                    protocol,
                    base_url,
                    vendor: None,
                    display_name: Some(format!("{} (Codex)", label)),
                    accounts: vec![AccountDraft {
                        kind: AccountKind::Env,
                        label: name.clone(),
                        source: "codex".to_string(),
                        secret: Secret::Env(env_key),
                        // Headers are copied verbatim: Codex sends them as
                        // written, once its variables are resolved.
                        headers: headers_of(table, &mut unset_env_headers),
                        tool_names: None,
                    }],
                    models: Vec::new(),
                },
            ));
        }
    }

    if !skipped.is_empty() {
        candidate.notes.push(format!(
            "Skipped providers without an endpoint or a key variable \
             (Codex's own account login isn't importable): {}",
            skipped.join(", ")
        ));
    }
    if !unsupported_auth.is_empty() {
        candidate.notes.push(format!(
            "Skipped providers whose authentication The Pair cannot import: {}",
            unsupported_auth.join(", ")
        ));
    }
    if !unset_keys.is_empty() {
        candidate.notes.push(format!(
            "These providers read a key variable that isn't set in this shell, so they need one: \
             {}",
            unset_keys.join(", ")
        ));
    }
    if !unset_env_headers.is_empty() {
        candidate.notes.push(format!(
            "These providers' env_http_headers name variables that aren't set, so those headers \
             were dropped and their requests may go out unauthenticated: {}",
            unset_env_headers.join(", ")
        ));
    }
    if !dropped_params.is_empty() {
        candidate.notes.push(format!(
            "Codex's query_params have nowhere to go in The Pair's provider records \
             and were dropped for: {}",
            dropped_params.join(", ")
        ));
    }
    if !unknown_wire_api.is_empty() {
        candidate.notes.push(format!(
            "These providers name a wire_api The Pair does not know; they were imported as \
             Responses, the only wire Codex still has: {}",
            unknown_wire_api.join(", ")
        ));
    }

    attach_configured_model(&config, &mut drafts, &mut candidate.notes);
    candidate.drafts = drafts.into_iter().map(|(_, draft)| draft).collect();
    candidate
}

/// Attaches Codex's configured model to the provider it names, falling back to
/// the first imported provider when that one was skipped.
fn attach_configured_model(
    config: &toml::Table,
    drafts: &mut Vec<(String, ProviderDraft)>,
    notes: &mut Vec<String>,
) {
    let Some(model_id) = toml_str(config, "model") else {
        return;
    };
    let selected = toml_str(config, "model_provider");
    let provider_name = match selected
        .as_deref()
        .filter(|name| drafts.iter().any(|(imported, _)| imported == name))
    {
        Some(name) => name.to_string(),
        None => match drafts.first() {
            Some((name, _)) => {
                if let Some(missed) = selected.as_deref() {
                    notes.push(format!(
                        "Codex's model provider `{}` wasn't imported (its own account login \
                     isn't), so the first imported provider carries the model.",
                        missed
                    ));
                }
                name.clone()
            }
            None => return,
        },
    };

    let context_window = toml_u64(config, "model_context_window");
    let default_effort = toml_str(config, "model_reasoning_effort");
    for (name, draft) in drafts.iter_mut() {
        if *name != provider_name {
            continue;
        }
        draft.models.push(ModelRoute {
            id: model_id.clone(),
            display_name: model_id.clone(),
            context_window,
            max_output: None,
            efforts: Vec::new(),
            default_effort: default_effort.clone(),
            price: None,
            source: ModelSource::Imported,
        });
    }
}

/// `http_headers` as account headers, with `env_http_headers` resolved on top:
/// Codex reads each of those values as a variable name and sends what it holds.
/// Non-string values are dropped, and a variable that is not set is reported in
/// `unset` rather than sent as its own name.
fn headers_of(table: &toml::Table, unset: &mut Vec<String>) -> Vec<HeaderPair> {
    let mut headers: Vec<HeaderPair> = Vec::new();
    if let Some(toml::Value::Table(static_headers)) = table.get("http_headers") {
        headers.extend(static_headers.iter().filter_map(|(name, value)| {
            Some(HeaderPair {
                name: name.clone(),
                value: value.as_str()?.to_string(),
            })
        }));
    }
    if let Some(toml::Value::Table(env_headers)) = table.get("env_http_headers") {
        headers.extend(env_headers.iter().filter_map(|(name, value)| {
            let variable = value.as_str()?;
            match live_env(variable) {
                Some(resolved) => Some(HeaderPair {
                    name: name.clone(),
                    value: resolved,
                }),
                None => {
                    unset.push(format!("{} (${})", name, variable));
                    None
                }
            }
        }));
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::types::AccountKind;
    use std::fs;

    fn write_config(root: &Path, body: &str) {
        let path = root.join(CONFIG_RELATIVE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn imports_custom_providers_and_the_configured_model() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
model = "internal-fast"
model_provider = "gateway"
model_context_window = 128000
model_reasoning_effort = "high"

[model_providers.gateway]
name = "Internal gateway"
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
wire_api = "chat"
http_headers = { "X-Tenant" = "acme" }
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");

        let candidate = scan_from(&root);
        assert_eq!(candidate.source_id, "codex");
        assert_eq!(candidate.config_path, Some(root.join(CONFIG_RELATIVE)));
        assert!(candidate.error.is_none());
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        // `wire_api = "chat"` is a hard deserialize error upstream: every real
        // entry is a Responses endpoint.
        assert_eq!(draft.protocol, Protocol::Responses);
        assert_eq!(draft.base_url, "https://llm.internal.example/v1");
        assert_eq!(
            draft.display_name.as_deref(),
            Some("Internal gateway (Codex)")
        );
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(draft.accounts[0].source, "codex");
        assert_eq!(draft.accounts[0].kind, AccountKind::Env);
        assert_eq!(
            draft.accounts[0].secret,
            Secret::Env("THE_PAIR_TEST_CODEX_KEY".into())
        );
        assert_eq!(
            draft.accounts[0].headers,
            vec![HeaderPair {
                name: "X-Tenant".into(),
                value: "acme".into()
            }]
        );
        assert_eq!(draft.models.len(), 1);
        assert_eq!(draft.models[0].id, "internal-fast");
        assert_eq!(draft.models[0].context_window, Some(128_000));
        assert_eq!(draft.models[0].default_effort.as_deref(), Some("high"));
        assert_eq!(draft.models[0].source, ModelSource::Imported);
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_provider_without_wire_api_is_a_responses_endpoint() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
model = "gpt-5-codex"
model_provider = "gateway"
[model_providers.gateway]
name = "Internal gateway"
base_url = "https://api.openai.com/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        // The same endpoint the env source maps to Responses, so the two
        // dedupe into one provider instead of two.
        assert_eq!(candidate.drafts[0].protocol, Protocol::Responses);
        assert_eq!(candidate.drafts[0].base_url, "https://api.openai.com/v1");
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unknown_wire_api_is_noted_and_read_as_responses() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[model_providers.gateway]
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
wire_api = "completions"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].protocol, Protocol::Responses);
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("completions") && note.contains("wire_api")),
            "{:?}",
            candidate.notes
        );

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn env_http_headers_are_resolved_before_they_are_stored() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[model_providers.gateway]
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
http_headers = { "X-Tenant" = "acme" }
env_http_headers = { "X-Coder-AI-Governance-Token" = "THE_PAIR_TEST_CODEX_TOKEN" }
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved_key = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        let saved_token = std::env::var_os("THE_PAIR_TEST_CODEX_TOKEN");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");
        std::env::set_var("THE_PAIR_TEST_CODEX_TOKEN", "governance-token");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(
            candidate.drafts[0].accounts[0].headers,
            vec![
                HeaderPair {
                    name: "X-Tenant".into(),
                    value: "acme".into()
                },
                HeaderPair {
                    name: "X-Coder-AI-Governance-Token".into(),
                    value: "governance-token".into()
                },
            ]
        );
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);

        // An unset variable is reported instead of sent as its own name.
        std::env::remove_var("THE_PAIR_TEST_CODEX_TOKEN");
        let unset = scan_from(&root);
        assert_eq!(unset.drafts[0].accounts[0].headers.len(), 1);
        assert!(
            unset
                .notes
                .iter()
                .any(|note| note.contains("THE_PAIR_TEST_CODEX_TOKEN")),
            "{:?}",
            unset.notes
        );

        match saved_key {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        match saved_token {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_TOKEN", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_TOKEN"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn authentication_the_pair_cannot_import_is_noted() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[model_providers.bearer]
base_url = "https://llm.internal.example/v1"
experimental_bearer_token = "tok-1"
[model_providers.vaulted]
base_url = "https://vault.internal.example/v1"
[model_providers.vaulted.auth]
command = ["/usr/bin/security", "find-generic-password", "-w"]
[model_providers.gateway]
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(
            candidate.drafts[0].display_name.as_deref(),
            Some("gateway (Codex)")
        );
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("bearer") && note.contains("experimental_bearer_token")),
            "{:?}",
            candidate.notes
        );
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("vaulted") && note.contains("command-backed")),
            "{:?}",
            candidate.notes
        );

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_responses_wire_api_maps_to_the_responses_protocol() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
model = "gpt-5-codex"
model_provider = "responses-gw"
[model_providers.responses-gw]
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
wire_api = "responses"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].protocol, Protocol::Responses);
        assert_eq!(
            candidate.drafts[0].display_name.as_deref(),
            Some("responses-gw (Codex)")
        );

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_config_is_not_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        assert!(candidate.notes.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_config_reports_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(&root, "this is not = = toml");
        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(
            candidate
                .error
                .as_deref()
                .is_some_and(|error| error.contains("config.toml")),
            "{:?}",
            candidate.error
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn skips_providers_without_an_endpoint_or_a_key_variable() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
model = "gpt-5-codex"
model_provider = "openai"
[model_providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
[model_providers.keyless]
base_url = "https://llm.internal.example/v1"
[model_providers.endpointless]
env_key = "THE_PAIR_TEST_CODEX_KEY"
"#,
        );

        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        assert_eq!(candidate.notes.len(), 1, "{:?}", candidate.notes);
        assert!(
            candidate.notes[0].contains("openai"),
            "{}",
            candidate.notes[0]
        );
        assert!(
            candidate.notes[0].contains("keyless"),
            "{}",
            candidate.notes[0]
        );
        assert!(
            candidate.notes[0].contains("endpointless"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unset_key_variable_is_kept_and_reported() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
model = "internal-fast"
model_provider = "gateway"
[model_providers.gateway]
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::remove_var("THE_PAIR_TEST_CODEX_KEY");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1, "the reference must survive");
        let account = &candidate.drafts[0].accounts[0];
        assert_eq!(account.kind, AccountKind::Env);
        assert_eq!(
            account.secret,
            Secret::Env("THE_PAIR_TEST_CODEX_KEY".into())
        );
        assert_eq!(candidate.notes.len(), 1, "{:?}", candidate.notes);
        assert!(
            candidate.notes[0].contains("THE_PAIR_TEST_CODEX_KEY"),
            "{}",
            candidate.notes[0]
        );

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_model_falls_back_when_its_provider_was_not_imported() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
model = "gpt-5-codex"
model_provider = "openai"
[model_providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
[model_providers.gateway]
name = "Internal gateway"
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert_eq!(candidate.drafts[0].models[0].id, "gpt-5-codex");
        assert!(
            candidate.notes.iter().any(|note| note.contains("openai")),
            "{:?}",
            candidate.notes
        );

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn query_params_are_dropped_with_a_note() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
model = "internal-fast"
[model_providers.gateway]
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
query_params = { "tenant" = "acme" }
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("query_params")),
            "{:?}",
            candidate.notes
        );

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_chatgpt_auth_file_is_never_read() {
        let root = std::env::temp_dir().join(format!("the-pair-codex-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
model = "gpt-5-codex"
[model_providers.gateway]
base_url = "https://llm.internal.example/v1"
env_key = "THE_PAIR_TEST_CODEX_KEY"
"#,
        );
        // A real ChatGPT sign-in sits next to the config; it must stay untouched.
        let auth_path = root.join(".codex/auth.json");
        fs::create_dir_all(auth_path.parent().unwrap()).unwrap();
        fs::write(
            &auth_path,
            r#"{
              "tokens": { "access_token": "chatgpt-access", "refresh_token": "chatgpt-refresh" }
            }"#,
        )
        .unwrap();
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_CODEX_KEY");
        std::env::set_var("THE_PAIR_TEST_CODEX_KEY", "sk-gateway");

        let candidate = scan_from(&root);
        assert_eq!(candidate.config_path, Some(root.join(CONFIG_RELATIVE)));
        let accounts: Vec<_> = candidate
            .drafts
            .iter()
            .flat_map(|draft| &draft.accounts)
            .collect();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].source, "codex");
        assert!(
            matches!(accounts[0].secret, Secret::Env(_)),
            "the ChatGPT token must not become an account: {:?}",
            accounts[0].secret
        );

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_CODEX_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_CODEX_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }
}
