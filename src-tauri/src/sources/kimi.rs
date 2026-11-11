//! Kimi Code: `~/.kimi-code/config.toml`.
//!
//! Shape (verified against Kimi Code's own docs):
//!
//! ```toml
//! default_model = "k2"
//! [thinking]
//! enabled = true
//! effort = "high"
//! [providers.moonshot]
//! type = "openai"
//! base_url = "https://api.moonshot.cn/v1"
//! api_key = "sk-..."
//! [providers.kimi]
//! type = "kimi"
//! [providers.gateway.env]
//! GATEWAY_API_KEY = "sk-..."
//! GATEWAY_BASE_URL = "https://llm.internal.example/v1"
//! [models.k2]
//! provider = "moonshot"
//! model = "kimi-k2-0905-preview"
//! max_context_size = 256000
//! support_efforts = ["low", "high"]
//! ```
//!
//! `type = "kimi"` is not Kimi's own account login: it names Moonshot's
//! OpenAI-compatible interface, Kimi Platform API keys included, and defaults to
//! `https://api.moonshot.ai/v1`. A provider's credentials come from `api_key` /
//! `api_key_env`, else from its `[providers.<name>.env]` table's
//! `<PREFIX>_API_KEY` (with `<PREFIX>_BASE_URL` for the endpoint), consulted
//! only when the fields are absent. Providers with no credential anywhere are
//! skipped: there is no API to move an account login to. Gemini's own
//! protocols (`google-genai`, `vertexai`) have no OpenAI-compatible dialect in
//! The Pair, so they are skipped as well.

use super::*;
use crate::accounts::registry::ProviderDraft;
use crate::accounts::types::{ModelRoute, ModelSource};

const CONFIG_RELATIVE: &str = ".kimi-code/config.toml";
const CONFIG_FILE: &str = "config.toml";
/// Kimi's own data root, which `KIMI_CODE_HOME` moves wholesale.
const HOME_VAR: &str = "KIMI_CODE_HOME";
/// The endpoint `type = "kimi"` names: Moonshot's OpenAI-compatible interface.
const KIMI_BASE_URL: &str = "https://api.moonshot.ai/v1";
/// The types that speak Gemini's own API, which The Pair has no protocol for.
const GEMINI_TYPES: &[&str] = &["google-genai", "vertexai"];

pub fn scan() -> ImportCandidate {
    match home_dir() {
        Some(home) => scan_from(&home),
        None => ImportCandidate {
            source_id: "kimi".into(),
            source_name: "Kimi Code".into(),
            config_path: None,
            drafts: Vec::new(),
            notes: vec!["No home directory, so no Kimi Code config to read.".into()],
            error: None,
        },
    }
}

/// `~/.kimi-code/config.toml`, or the one under the dir `KIMI_CODE_HOME` names.
pub(super) fn config_path(home: &Path) -> PathBuf {
    match env_dir(HOME_VAR) {
        Some(dir) => dir.join(CONFIG_FILE),
        None => home.join(CONFIG_RELATIVE),
    }
}

pub fn scan_from(home: &Path) -> ImportCandidate {
    let path = config_path(home);
    let mut candidate = ImportCandidate {
        source_id: "kimi".into(),
        source_name: "Kimi Code".into(),
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

    // provider name -> draft, so models can be attached after both tables are read.
    let mut drafts: Vec<(String, ProviderDraft)> = Vec::new();
    let mut skipped = Vec::new();
    let mut unsupported = Vec::new();
    let mut attached = Vec::new();

    if let Some(toml::Value::Table(providers)) = config.get("providers") {
        for (name, provider) in providers {
            let Some(table) = provider.as_table() else {
                continue;
            };
            let kind = toml_str(table, "type").unwrap_or_default();
            if GEMINI_TYPES.contains(&kind.as_str()) {
                unsupported.push(name.clone());
                continue;
            }
            // The provider's own `[providers.<name>.env]` table is Kimi's third
            // credential source, read only when the fields say nothing. Its keys
            // are variable names and its values are what Kimi would set them to,
            // so a live variable stays a reference and only a dead one falls back
            // to the table's value.
            let env_table = table.get("env").and_then(toml::Value::as_table);
            let key_var = format!("{}_API_KEY", env_prefix(name));
            let base_url_var = format!("{}_BASE_URL", env_prefix(name));
            let table_value = |var: &str| env_table.and_then(|vars| toml_str(vars, var));
            let table_declares = |var: &str| env_table.is_some_and(|vars| vars.contains_key(var));
            let inline_key = toml_str(table, "api_key").or_else(|| table_value(&key_var));
            let env_key = toml_str(table, "api_key_env")
                .or_else(|| table_declares(&key_var).then(|| key_var.clone()));
            // An account login signs in without a key; there is nothing to import.
            if inline_key.is_none() && env_key.is_none() {
                skipped.push(name.clone());
                continue;
            }
            let base_url = toml_str(table, "base_url")
                .or_else(|| table_value(&base_url_var))
                .unwrap_or_else(|| {
                    if kind == "kimi" {
                        KIMI_BASE_URL.to_string()
                    } else {
                        protocol_from_type(&kind, "").default_base_url().to_string()
                    }
                });
            let protocol = protocol_from_type(&kind, &base_url);
            let accounts = secret_from_key_fields(inline_key, env_key)
                .and_then(|secret| api_key_account(name.clone(), "kimi", secret))
                .into_iter()
                .collect();

            drafts.push((
                name.clone(),
                ProviderDraft {
                    protocol,
                    base_url,
                    vendor: None,
                    display_name: Some(format!("{} (Kimi Code)", name)),
                    accounts,
                    models: Vec::new(),
                },
            ));
        }
    }

    if let Some(toml::Value::Table(models)) = config.get("models") {
        for (alias, model) in models {
            let Some(table) = model.as_table() else {
                continue;
            };
            let provider_name = toml_str(table, "provider").unwrap_or_default();
            let Some((_, draft)) = drafts.iter_mut().find(|(name, _)| *name == provider_name)
            else {
                continue;
            };
            attached.push(alias.clone());
            let model_id = toml_str(table, "model").unwrap_or_else(|| alias.clone());
            draft.models.push(ModelRoute {
                id: model_id.clone(),
                display_name: toml_str(table, "display_name").unwrap_or(model_id),
                context_window: toml_u64(table, "max_context_size"),
                max_output: toml_u64(table, "max_output_size"),
                efforts: toml_strings(table, "support_efforts"),
                default_effort: None,
                price: None,
                source: ModelSource::Imported,
            });
        }
    }

    if !skipped.is_empty() {
        candidate.notes.push(format!(
            "Skipped providers signed in through Kimi's account login (no API key): {}",
            skipped.join(", ")
        ));
    }
    if !unsupported.is_empty() {
        candidate.notes.push(format!(
            "The Pair has no Gemini-native protocol, so these providers were skipped: {}",
            unsupported.join(", ")
        ));
    }
    if let Some(default_model) = toml_str(&config, "default_model") {
        if !attached.iter().any(|alias| *alias == default_model) {
            candidate.notes.push(format!(
                "default_model `{}` was not imported.",
                default_model
            ));
        }
    }

    candidate.drafts = drafts.into_iter().map(|(_, draft)| draft).collect();
    candidate
}

/// The variable prefix Kimi derives from a provider's name: `moonshot` reads
/// `MOONSHOT_API_KEY` and `MOONSHOT_BASE_URL` out of its `env` table.
fn env_prefix(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_config(root: &Path, body: &str) {
        let path = root.join(CONFIG_RELATIVE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn imports_providers_and_their_models() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
default_model = "k2"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key = "sk-kimi-1"
[models.k2]
provider = "moonshot"
model = "kimi-k2-0905-preview"
max_context_size = 256000
support_efforts = ["low", "high"]
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.protocol, crate::accounts::types::Protocol::OpenAiChat);
        assert_eq!(draft.base_url, "https://api.moonshot.cn/v1");
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(draft.models.len(), 1);
        assert_eq!(draft.models[0].id, "kimi-k2-0905-preview");
        assert_eq!(draft.models[0].context_window, Some(256_000));
        assert_eq!(draft.models[0].efforts, vec!["low", "high"]);
        assert!(candidate.error.is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_kimi_type_is_moonshots_openai_compatible_interface() {
        // Kimi's own docs spell this type out with a Kimi Platform key, and it
        // is the first source in the import wizard.
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
default_model = "k2"
[providers.kimi]
type = "kimi"
api_key = "sk-kimi-1"
[models.k2]
provider = "kimi"
model = "kimi-k2-0905-preview"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.base_url, "https://api.moonshot.ai/v1");
        assert_eq!(draft.protocol, crate::accounts::types::Protocol::OpenAiChat);
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(draft.models.len(), 1);
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_env_table_supplies_the_key_and_the_endpoint() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[providers.my-gateway]
type = "openai"
[providers.my-gateway.env]
MY_GATEWAY_API_KEY = "sk-gateway"
MY_GATEWAY_BASE_URL = "https://llm.internal.example/v1"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("MY_GATEWAY_API_KEY");
        std::env::remove_var("MY_GATEWAY_API_KEY");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.base_url, "https://llm.internal.example/v1");
        assert_eq!(
            draft.accounts[0].secret,
            Secret::ApiKey("sk-gateway".into()),
            "the table's value is the key itself"
        );
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);

        match saved {
            Some(value) => std::env::set_var("MY_GATEWAY_API_KEY", value),
            None => std::env::remove_var("MY_GATEWAY_API_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_live_env_table_variable_is_kept_as_a_reference() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[providers.gateway]
type = "openai"
[providers.gateway.env]
GATEWAY_API_KEY = "sk-file"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("GATEWAY_API_KEY");
        std::env::set_var("GATEWAY_API_KEY", "sk-live");

        let candidate = scan_from(&root);
        assert_eq!(
            candidate.drafts[0].accounts[0].secret,
            Secret::Env("GATEWAY_API_KEY".into())
        );

        match saved {
            Some(value) => std::env::set_var("GATEWAY_API_KEY", value),
            None => std::env::remove_var("GATEWAY_API_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn gemini_native_types_are_skipped_with_a_note() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[providers.vertex]
type = "vertexai"
api_key = "sk-vertex"
[providers.genai]
type = "google-genai"
api_key = "sk-genai"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key = "sk-kimi-1"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].base_url, "https://api.moonshot.cn/v1");
        assert!(candidate.notes.iter().any(|note| note.contains("vertex")
            && note.contains("genai")
            && note.contains("Gemini-native")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn skips_account_login_providers_and_says_so() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[providers.kimi]
type = "kimi"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key = "sk-kimi-1"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].accounts.len(), 1);
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("kimi"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_default_model_whose_provider_was_skipped_is_noted() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
default_model = "gone"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key = "sk-kimi-1"
[models.gone]
provider = "vanished"
model = "kimi-k2-0905-preview"
[models.k2]
provider = "moonshot"
model = "kimi-k2-0905-preview"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("default_model `gone`")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_config_is_not_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_config_reports_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(&root, "this is not = = toml");
        let candidate = scan_from(&root);
        assert!(candidate.error.is_some());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn env_key_reference_is_kept_as_a_reference() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key_env = "THE_PAIR_TEST_KIMI_KEY"
"#,
        );
        let _guard = crate::test_env::lock_env();
        std::env::set_var("THE_PAIR_TEST_KIMI_KEY", "sk-live");

        let candidate = scan_from(&root);
        let account = &candidate.drafts[0].accounts[0];
        assert_eq!(account.kind, crate::accounts::types::AccountKind::Env);
        assert!(account.secret.is_present());
        std::env::remove_var("THE_PAIR_TEST_KIMI_KEY");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn models_for_unknown_providers_are_dropped() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        write_config(
            &root,
            r#"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key = "sk-kimi-1"
[models.orphan]
provider = "gone"
model = "whatever"
"#,
        );
        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts[0].models.len(), 0);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn kimi_code_home_moves_the_config() {
        let root = std::env::temp_dir().join(format!("the-pair-kimi-{}", uuid::Uuid::new_v4()));
        let home = root.join("home");
        let data = root.join("elsewhere");
        fs::create_dir_all(&data).unwrap();
        fs::write(
            data.join("config.toml"),
            r#"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key = "sk-kimi-1"
"#,
        )
        .unwrap();
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("KIMI_CODE_HOME");
        std::env::set_var("KIMI_CODE_HOME", &data);

        let candidate = scan_from(&home);
        assert_eq!(candidate.config_path, Some(data.join("config.toml")));
        assert_eq!(candidate.drafts.len(), 1);

        match saved {
            Some(value) => std::env::set_var("KIMI_CODE_HOME", value),
            None => std::env::remove_var("KIMI_CODE_HOME"),
        }
        fs::remove_dir_all(&root).ok();
    }
}
