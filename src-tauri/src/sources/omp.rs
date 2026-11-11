//! omp: `~/.omp/agent/models.yml`, with `config.yml` and `models-store.json`
//! beside it.
//!
//! omp writes pi's model shape as YAML, so the tree reads the same way and
//! shares pi's mapping; a leftover pi-era `models.json`/`settings.json` stands in
//! when the YAML isn't there.
//!
//! ```yaml
//! # ~/.omp/agent/models.yml
//! providers:
//!   moonshot:
//!     api: openai
//!     baseUrl: https://api.moonshot.cn/v1
//!     apiKey: sk-...
//!     models:
//!       - id: kimi-k2-0905-preview
//!         contextWindow: 256000
//! ```
//!
//! ```yaml
//! # ~/.omp/agent/config.yml
//! modelRoles:
//!   default: moonshot/kimi-k2-0905-preview:high
//! ```
//!
//! Only the providers the models file itself defines come over: the ones omp
//! builds in — reached with an environment variable or its own account login —
//! carry no endpoint or key in the file. An `apiKey` that names a variable is
//! kept as a reference, whether or not it is set; one that runs a shell command
//! (a leading `!`) is skipped with the rest. `auth.json` beside the models file
//! holds the keys a login fetched, as it does for pi.

use super::pi::{read_optional_json, scan_pi_shape, KeyStyle, AUTH_FILE};
use super::*;
use std::path::PathBuf;

const AGENT_RELATIVE: &str = ".omp/agent";

pub fn scan() -> ImportCandidate {
    match home_dir() {
        Some(home) => scan_from(&home),
        None => ImportCandidate {
            source_id: "omp".into(),
            source_name: "omp".into(),
            config_path: None,
            drafts: Vec::new(),
            notes: vec!["No home directory, so no omp config to read.".into()],
            error: None,
        },
    }
}

pub fn scan_from(root: &Path) -> ImportCandidate {
    let dir = root.join(AGENT_RELATIVE);
    let mut candidate = ImportCandidate {
        source_id: "omp".into(),
        source_name: "omp".into(),
        config_path: None,
        drafts: Vec::new(),
        notes: Vec::new(),
        error: None,
    };

    let Some((config, path)) = read_models(&dir).unwrap_or_else(|error| {
        candidate.error = Some(error);
        None
    }) else {
        return candidate;
    };
    candidate.config_path = Some(path);

    let catalog = read_optional_json(&dir.join("models-store.json"), &mut candidate);
    let auth = read_optional_json(&dir.join(AUTH_FILE), &mut candidate);
    let settings = match read_settings(&dir) {
        Ok(settings) => settings.unwrap_or_default(),
        Err(error) => {
            candidate.error = Some(error);
            return candidate;
        }
    };
    if candidate.error.is_some() {
        return candidate;
    }

    let (configured, effort) = omp_default(&settings);
    let (drafts, notes) = scan_pi_shape(
        "omp",
        &config,
        &catalog,
        configured.as_deref(),
        effort.as_deref(),
        KeyStyle::Omp,
        &auth,
    );
    candidate.drafts = drafts;
    candidate.notes = notes;
    candidate
}

/// omp's models file: `models.yml`, its `.yaml` sibling, else the pi-era
/// `models.json`. Returns the tree and the file it came from.
fn read_models(dir: &Path) -> Result<Option<(serde_json::Value, PathBuf)>, String> {
    for name in ["models.yml", "models.yaml"] {
        let path = dir.join(name);
        if path.exists() {
            return read_yaml(&path).map(|value| value.map(|value| (value, path)));
        }
    }
    let path = dir.join("models.json");
    if path.exists() {
        return read_json(&path).map(|value| value.map(|value| (value, path)));
    }
    Ok(None)
}

/// omp's settings: `config.yml`, its `.yaml` sibling, else the pi-era
/// `settings.json`.
fn read_settings(dir: &Path) -> Result<Option<serde_json::Value>, String> {
    for name in ["config.yml", "config.yaml"] {
        let path = dir.join(name);
        if path.exists() {
            return read_yaml(&path);
        }
    }
    let path = dir.join("settings.json");
    if path.exists() {
        return read_json(&path);
    }
    Ok(None)
}

/// omp's configured default model and thinking level: its `modelRoles.default`
/// (the first of a comma list, with any `:effort` off its end), else the pi-era
/// settings it migrates from. A role alias (`@smol`) or a glob names no one
/// model, so pressing on falls back.
fn omp_default(settings: &serde_json::Value) -> (Option<String>, Option<String>) {
    let role = settings
        .get("modelRoles")
        .and_then(|roles| json_str(roles, "default"));
    if let Some(role) = role.filter(|value| !value.starts_with('@') && !value.starts_with('*')) {
        let first = role.split(',').next().unwrap_or_default().trim();
        let (alias, effort) = match first.rsplit_once(':') {
            Some((alias, effort)) if !effort.is_empty() && effort != "auto" => {
                (alias, Some(effort.to_string()))
            }
            _ => (first, None),
        };
        return ((!alias.is_empty()).then(|| alias.to_string()), effort);
    }
    let configured = json_str(settings, "defaultProvider")
        .zip(json_str(settings, "defaultModel"))
        .map(|(provider, model)| format!("{provider}/{model}"));
    let effort = json_str(settings, "defaultThinkingLevel").filter(|level| level != "auto");
    (configured, effort)
}

/// Reads a YAML config into the same value tree `read_json` returns, so omp's
/// YAML and a leftover pi JSON read the same way downstream.
fn read_yaml(path: &Path) -> Result<Option<serde_json::Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {}", path.display(), error)),
    };
    serde_yaml::from_str::<serde_yaml::Value>(&text)
        .map(|value| Some(yaml_to_json(value)))
        .map_err(|error| format!("invalid YAML in {}: {}", path.display(), error))
}

/// serde_yaml's tree as serde_json's. A tagged scalar keeps its tag, so omp's
/// `!command` keys stay text for the importer to decline rather than silently
/// becoming the command it names.
fn yaml_to_json(value: serde_yaml::Value) -> serde_json::Value {
    match value {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(flag) => serde_json::Value::Bool(flag),
        serde_yaml::Value::Number(number) => {
            if let Some(number) = number.as_u64() {
                serde_json::Value::from(number)
            } else if let Some(number) = number.as_i64() {
                serde_json::Value::from(number)
            } else if let Some(number) = number.as_f64() {
                serde_json::Number::from_f64(number)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(text) => serde_json::Value::String(text),
        serde_yaml::Value::Sequence(items) => items.into_iter().map(yaml_to_json).collect(),
        serde_yaml::Value::Mapping(map) => map
            .into_iter()
            .map(|(key, value)| (yaml_key(key), yaml_to_json(value)))
            .collect(),
        serde_yaml::Value::Tagged(tagged) => {
            let tagged = *tagged;
            match yaml_to_json(tagged.value) {
                serde_json::Value::String(text) if !text.is_empty() => {
                    serde_json::Value::String(format!("{} {}", tagged.tag, text))
                }
                value => value,
            }
        }
    }
}

/// A YAML mapping key as a string; only string keys are looked up downstream.
fn yaml_key(key: serde_yaml::Value) -> String {
    match yaml_to_json(key) {
        serde_json::Value::String(text) => text,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::types::{AccountKind, ModelSource};
    use std::fs;

    fn write_files(root: &Path, files: &[(&str, &str)]) {
        for (relative, body) in files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
    }

    #[test]
    fn imports_yaml_providers_models_and_the_role_default() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".omp/agent/models.yml",
                    r#"
providers:
  moonshot:
    api: openai
    baseUrl: https://api.moonshot.cn/v1
    apiKey: sk-omp-1
    models:
      - id: kimi-k2-0905-preview
        contextWindow: 256000
        maxTokens: 8192
"#,
                ),
                (
                    ".omp/agent/config.yml",
                    r#"
modelRoles:
  default: moonshot/kimi-k2-0905-preview:high
"#,
                ),
            ],
        );

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert_eq!(
            candidate.config_path,
            Some(root.join(".omp/agent/models.yml"))
        );
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.protocol, Protocol::OpenAiChat);
        assert_eq!(draft.base_url, "https://api.moonshot.cn/v1");
        assert_eq!(draft.display_name.as_deref(), Some("moonshot (omp)"));
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(draft.accounts[0].source, "omp");
        assert_eq!(draft.accounts[0].secret, Secret::ApiKey("sk-omp-1".into()));
        assert_eq!(draft.models.len(), 1);
        assert_eq!(draft.models[0].id, "kimi-k2-0905-preview");
        assert_eq!(draft.models[0].context_window, Some(256_000));
        assert_eq!(draft.models[0].max_output, Some(8192));
        assert_eq!(draft.models[0].default_effort.as_deref(), Some("high"));
        assert_eq!(draft.models[0].source, ModelSource::Imported);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_live_env_var_name_becomes_an_env_account() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".omp/agent/models.yml",
                r#"
providers:
  moonshot:
    api: openai
    baseUrl: https://api.moonshot.cn/v1
    apiKey: THE_PAIR_TEST_OMP_KEY
    models:
      - id: kimi-k2-0905-preview
"#,
            )],
        );
        let _guard = crate::test_env::lock_env();
        std::env::set_var("THE_PAIR_TEST_OMP_KEY", "sk-live");

        let candidate = scan_from(&root);
        let account = &candidate.drafts[0].accounts[0];
        assert_eq!(account.kind, AccountKind::Env);
        assert_eq!(account.secret, Secret::Env("THE_PAIR_TEST_OMP_KEY".into()));
        assert!(account.secret.is_present());

        // Without the variable the name is still a reference: the account
        // shows as needing a key instead of sending the name as a bearer token.
        std::env::remove_var("THE_PAIR_TEST_OMP_KEY");
        let candidate = scan_from(&root);
        let account = &candidate.drafts[0].accounts[0];
        assert_eq!(account.kind, AccountKind::Env);
        assert_eq!(account.secret, Secret::Env("THE_PAIR_TEST_OMP_KEY".into()));
        assert!(!account.secret.is_present());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn command_api_keys_are_skipped() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[(
                ".omp/agent/models.yml",
                r#"
providers:
  vaulted:
    api: openai
    baseUrl: https://api.moonshot.cn/v1
    apiKey: !op read op://vault/kimi
"#,
            )],
        );

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert!(candidate.drafts.is_empty());
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("vaulted"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn auth_json_supplies_the_key_a_login_fetched() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".omp/agent/models.yml",
                    "\nproviders:\n  moonshot:\n    api: openai\n    baseUrl: https://api.moonshot.cn/v1\n",
                ),
                (
                    ".omp/agent/auth.json",
                    r#"{ "moonshot": { "type": "api_key", "key": "sk-auth" } }"#,
                ),
            ],
        );

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(
            candidate.drafts[0].accounts[0].secret,
            Secret::ApiKey("sk-auth".into())
        );
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn falls_back_to_pi_shaped_json_files() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".omp/agent/models.json",
                    r#"{
                      "providers": {
                        "moonshot": {
                          "api": "openai",
                          "baseUrl": "https://api.moonshot.cn/v1",
                          "apiKey": "sk-omp-1",
                          "models": [{ "id": "kimi-k2-0905-preview" }]
                        }
                      }
                    }"#,
                ),
                (
                    ".omp/agent/settings.json",
                    r#"{
                      "defaultProvider": "moonshot",
                      "defaultModel": "kimi-k2-0905-preview",
                      "defaultThinkingLevel": "medium"
                    }"#,
                ),
            ],
        );

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert_eq!(
            candidate.config_path,
            Some(root.join(".omp/agent/models.json"))
        );
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert_eq!(
            candidate.drafts[0].models[0].default_effort.as_deref(),
            Some("medium")
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn models_yaml_stands_in_for_models_yml() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".omp/agent/models.yaml",
                    r#"
providers:
  moonshot:
    api: openai
    baseUrl: https://api.moonshot.cn/v1
    apiKey: sk-omp-1
"#,
                ),
                (
                    ".omp/agent/config.yaml",
                    r#"
modelRoles:
  default: "@smol"
"#,
                ),
            ],
        );

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert_eq!(
            candidate.config_path,
            Some(root.join(".omp/agent/models.yaml"))
        );
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].models.len(), 0);
        // A role alias names no one model, so nothing is marked as the default.
        assert!(candidate.notes.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_config_is_not_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        let candidate = scan_from(&root);
        assert!(candidate.config_path.is_none());
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_yaml_reports_an_error() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(&root, &[(".omp/agent/models.yml", "providers: [oops")]);
        let candidate = scan_from(&root);
        assert!(candidate.error.is_some());
        assert!(candidate.drafts.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn catalog_supplies_models_when_the_provider_lists_none() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(
            &root,
            &[
                (
                    ".omp/agent/models.yml",
                    r#"
providers:
  moonshot:
    api: openai
    baseUrl: https://api.moonshot.cn/v1
    apiKey: sk-omp-1
"#,
                ),
                (
                    ".omp/agent/models-store.json",
                    r#"{
                      "moonshot": {
                        "models": [
                          { "id": "kimi-k2-0905-preview", "contextWindow": 128000 }
                        ]
                      }
                    }"#,
                ),
            ],
        );

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert_eq!(candidate.drafts[0].models[0].id, "kimi-k2-0905-preview");
        assert_eq!(candidate.drafts[0].models[0].context_window, Some(128_000));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_config_without_providers_is_noted() {
        let root = std::env::temp_dir().join(format!("the-pair-omp-{}", uuid::Uuid::new_v4()));
        write_files(&root, &[(".omp/agent/models.yml", "unrelated: true\n")]);

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert!(candidate.drafts.is_empty());
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("no providers"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }
}
