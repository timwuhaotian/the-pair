//! Claude Code: `~/.claude/settings.json`.
//!
//! Shape:
//!
//! ```json
//! {
//!   "model": "claude-opus-4-6",
//!   "env": {
//!     "ANTHROPIC_API_KEY": "sk-ant-...",
//!     "ANTHROPIC_BASE_URL": "https://proxy.example.com"
//!   }
//! }
//! ```
//!
//! Claude Code signs in with a Claude subscription and keeps no key on disk, so
//! that case is skipped like every other agent's own account login: there is no
//! credential to move. The `env` block is only a fallback — a live shell
//! variable wins over it and stays a reference, so rotating the variable keeps
//! working.

use super::*;
use crate::accounts::registry::ProviderDraft;
use crate::accounts::types::{ModelRoute, ModelSource};

const CONFIG_RELATIVE: &str = ".claude/settings.json";

pub fn scan() -> ImportCandidate {
    match home_dir() {
        Some(home) => scan_from(&home),
        None => ImportCandidate {
            source_id: "claude".into(),
            source_name: "Claude Code".into(),
            config_path: None,
            drafts: Vec::new(),
            notes: vec!["No home directory, so no Claude Code config to read.".into()],
            error: None,
        },
    }
}

pub fn scan_from(root: &Path) -> ImportCandidate {
    let path = root.join(CONFIG_RELATIVE);
    let mut candidate = ImportCandidate {
        source_id: "claude".into(),
        source_name: "Claude Code".into(),
        config_path: Some(path.clone()),
        drafts: Vec::new(),
        notes: Vec::new(),
        error: None,
    };

    let Some(settings) = read_json(&path).unwrap_or_else(|error| {
        candidate.error = Some(error);
        None
    }) else {
        return candidate;
    };
    let env = settings.get("env").cloned().unwrap_or_default();

    // A subscription login has no key anywhere, which is the one case Claude
    // Code cannot be imported from.
    let secret = if live_env("ANTHROPIC_API_KEY").is_some() {
        Some(Secret::Env("ANTHROPIC_API_KEY".to_string()))
    } else {
        json_str(&env, "ANTHROPIC_API_KEY").map(Secret::ApiKey)
    };
    let Some(secret) = secret else {
        candidate.notes.push(
            "Claude Code signs in with a Claude subscription, which cannot be imported — \
             it needs an Anthropic API key."
                .into(),
        );
        return candidate;
    };
    let Some(account) = api_key_account("ANTHROPIC_API_KEY".into(), "claude", secret) else {
        return candidate;
    };

    let base_url = live_env("ANTHROPIC_BASE_URL")
        .or_else(|| json_str(&env, "ANTHROPIC_BASE_URL"))
        .unwrap_or_else(|| Protocol::Anthropic.default_base_url().to_string());
    let mut models = Vec::new();
    match live_env("ANTHROPIC_MODEL").or_else(|| json_str(&settings, "model")) {
        Some(id) => {
            // Claude Code accepts these aliases, the Messages API wants the full id.
            if matches!(id.as_str(), "opus" | "sonnet" | "haiku") {
                candidate.notes.push(format!(
                    "Claude Code's model `{id}` is an alias; \
                     set a full model id if the API refuses it."
                ));
            }
            models.push(model_route(&id));
        }
        None => candidate
            .notes
            .push("Claude Code's config names no model, so none was imported.".into()),
    }

    candidate.drafts.push(ProviderDraft {
        protocol: Protocol::Anthropic,
        base_url,
        vendor: None,
        display_name: Some("Anthropic (Claude Code)".into()),
        accounts: vec![account],
        models,
    });
    candidate
}

fn model_route(id: &str) -> ModelRoute {
    ModelRoute {
        id: id.to_string(),
        display_name: id.to_string(),
        context_window: None,
        max_output: None,
        efforts: Vec::new(),
        default_effort: None,
        price: None,
        source: ModelSource::Imported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ENV_NAMES: &[&str] = &["ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL"];

    /// Clears the variables the importer falls back to, and puts them back when
    /// the test ends — even when it fails.
    struct EnvBackup(Vec<(&'static str, Option<std::ffi::OsString>)>);

    impl EnvBackup {
        fn clear(names: &[&'static str]) -> Self {
            let saved = names
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect();
            for name in names {
                std::env::remove_var(name);
            }
            Self(saved)
        }
    }

    impl Drop for EnvBackup {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    /// The env lock plus a clean slate, so tests neither race each other nor see
    /// the developer's own shell.
    fn isolated_env() -> (std::sync::MutexGuard<'static, ()>, EnvBackup) {
        let guard = crate::test_env::lock_env();
        // The scan entry point fills the process env from the login shell.
        // Doing it here, under the lock, keeps that from landing halfway
        // through a test's assertions.
        crate::sources::env::load_login_shell_env();
        let backup = EnvBackup::clear(ENV_NAMES);
        (guard, backup)
    }

    fn write_settings(root: &Path, body: &str) {
        let path = root.join(CONFIG_RELATIVE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn temp_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("the-pair-claude-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn imports_key_endpoint_and_model() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_settings(
            &root,
            r#"{
  "model": "claude-opus-4-6",
  "env": {
    "ANTHROPIC_API_KEY": "sk-ant-file",
    "ANTHROPIC_BASE_URL": "https://proxy.example.com"
  }
}"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.protocol, crate::accounts::types::Protocol::Anthropic);
        assert_eq!(draft.base_url, "https://proxy.example.com");
        assert_eq!(
            draft.display_name.as_deref(),
            Some("Anthropic (Claude Code)")
        );
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(draft.accounts[0].source, "claude");
        assert_eq!(
            draft.accounts[0].secret,
            Secret::ApiKey("sk-ant-file".into())
        );
        assert_eq!(draft.models.len(), 1);
        assert_eq!(draft.models[0].id, "claude-opus-4-6");
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);
        assert!(candidate.error.is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_config_is_not_an_error() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_config_reports_an_error() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_settings(&root, "{ not json");
        let candidate = scan_from(&root);
        assert!(candidate.error.is_some());
        assert!(candidate.drafts.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn subscription_login_is_skipped_with_a_note() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_settings(&root, r#"{"model": "claude-opus-4-6"}"#);

        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("subscription") && candidate.notes[0].contains("API key"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn live_variables_beat_the_settings_env_block() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_settings(
            &root,
            r#"{
  "model": "claude-sonnet-4-5-20250929",
  "env": {
    "ANTHROPIC_API_KEY": "sk-ant-file",
    "ANTHROPIC_BASE_URL": "https://proxy.example.com"
  }
}"#,
        );
        std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-live");
        std::env::set_var("ANTHROPIC_BASE_URL", "https://live.example.com");

        let candidate = scan_from(&root);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(
            draft.accounts[0].kind,
            crate::accounts::types::AccountKind::Env
        );
        assert!(draft.accounts[0].secret.is_present());
        assert_eq!(
            draft.accounts[0].secret,
            Secret::Env("ANTHROPIC_API_KEY".into())
        );
        assert_eq!(draft.base_url, "https://live.example.com");
        assert_eq!(draft.models[0].id, "claude-sonnet-4-5-20250929");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_alias_model_is_attached_and_flagged() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_settings(
            &root,
            r#"{"model": "opus", "env": {"ANTHROPIC_API_KEY": "sk-ant-file"}}"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert_eq!(candidate.drafts[0].models[0].id, "opus");
        assert!(
            candidate.notes.iter().any(|note| note.contains("alias")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn without_a_model_the_endpoint_is_still_imported() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_settings(&root, r#"{"env": {"ANTHROPIC_API_KEY": "sk-ant-file"}}"#);

        let candidate = scan_from(&root);
        let draft = &candidate.drafts[0];
        assert!(draft.models.is_empty());
        // Claude Code's own endpoint, so the Anthropic preset still applies.
        assert_eq!(draft.base_url, "https://api.anthropic.com");
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("names no model")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_non_object_env_block_is_not_an_error() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_settings(&root, r#"{"env": "nonsense"}"#);

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert!(candidate.drafts.is_empty());
        assert!(
            candidate.notes[0].contains("subscription"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }
}
