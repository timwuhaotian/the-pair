//! Gemini CLI: `~/.gemini/settings.json` plus `~/.gemini/.env`.
//!
//! Shape:
//!
//! ```json
//! {
//!   "model": { "name": "gemini-2.5-pro" },
//!   "mcpServers": { "files": { "command": "files-mcp" } }
//! }
//! ```
//!
//! ```env
//! GOOGLE_GEMINI_BASE_URL=https://gw.example.com/v1
//! GEMINI_API_KEY=sk-...
//! ```
//!
//! The registry speaks the OpenAI-compatible dialect of the Gemini API, so a
//! draft is only produced when a key is available and an endpoint can be
//! pinned: `GOOGLE_GEMINI_BASE_URL` when a gateway is configured, Google's own
//! OpenAI-compatible endpoint otherwise. A Google account login carries no key
//! and is skipped, like every other agent's own account login.

use super::*;
use crate::accounts::registry::ProviderDraft;
use crate::accounts::types::{ModelRoute, ModelSource};

const CONFIG_RELATIVE: &str = ".gemini/settings.json";
const DOTENV_RELATIVE: &str = ".gemini/.env";
/// Google's OpenAI-compatible endpoint, used when no gateway is configured.
const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

pub fn scan() -> ImportCandidate {
    match home_dir() {
        Some(home) => scan_from(&home),
        None => ImportCandidate {
            source_id: "gemini".into(),
            source_name: "Gemini CLI".into(),
            config_path: None,
            drafts: Vec::new(),
            notes: vec!["No home directory, so no Gemini CLI config to read.".into()],
            error: None,
        },
    }
}

pub fn scan_from(root: &Path) -> ImportCandidate {
    let path = root.join(CONFIG_RELATIVE);
    let mut candidate = ImportCandidate {
        source_id: "gemini".into(),
        source_name: "Gemini CLI".into(),
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
    let file_env = match dotenv(&root.join(DOTENV_RELATIVE)) {
        Ok(pairs) => pairs,
        Err(error) => {
            candidate.error = Some(error);
            return candidate;
        }
    };

    // A Google login leaves no key in either file, which is the one case the
    // Gemini CLI cannot be imported from.
    let Some((secret, label)) = gemini_key(&file_env) else {
        candidate.notes.push(
            "Gemini CLI signs in with a Google account, which cannot be imported — \
             it needs a Gemini API key (GEMINI_API_KEY or GOOGLE_API_KEY)."
                .into(),
        );
        return candidate;
    };
    let Some(account) = api_key_account(label, "gemini", secret) else {
        return candidate;
    };

    let base_url = live_env("GOOGLE_GEMINI_BASE_URL")
        .or_else(|| dotenv_value(&file_env, "GOOGLE_GEMINI_BASE_URL"))
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let mut models = Vec::new();
    match settings_model(&settings)
        .or_else(|| live_env("GEMINI_MODEL"))
        .or_else(|| dotenv_value(&file_env, "GEMINI_MODEL"))
    {
        Some(id) => models.push(model_route(&id)),
        None => candidate
            .notes
            .push("Gemini CLI's config names no model, so none was imported.".into()),
    }
    if let Some(servers) = settings
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .filter(|servers| !servers.is_empty())
    {
        candidate.notes.push(format!(
            "Gemini CLI's {} MCP server{} were not imported; The Pair has no MCP registry yet.",
            servers.len(),
            if servers.len() == 1 { "" } else { "s" }
        ));
    }

    candidate.drafts.push(ProviderDraft {
        protocol: Protocol::OpenAiChat,
        base_url,
        vendor: None,
        display_name: Some("Gemini (Gemini CLI)".into()),
        accounts: vec![account],
        models,
    });
    candidate
}

/// Gemini's key as killi keeps it: a live variable stays a reference, one only
/// `.env` sets is copied inline. `GEMINI_API_KEY` first, then the Vertex one.
fn gemini_key(file_env: &[(String, String)]) -> Option<(Secret, String)> {
    for name in ["GEMINI_API_KEY", "GOOGLE_API_KEY"] {
        if live_env(name).is_some() {
            return Some((Secret::Env(name.to_string()), name.to_string()));
        }
    }
    ["GEMINI_API_KEY", "GOOGLE_API_KEY"]
        .into_iter()
        .find_map(|name| {
            dotenv_value(file_env, name).map(|key| (Secret::ApiKey(key), name.to_string()))
        })
}

/// The `KEY=value` pairs of a `.env` file; a missing file reads as empty, and
/// an unreadable one fails the scan — a wrong owner or mode would otherwise be
/// reported as a Google account login.
fn dotenv(path: &Path) -> Result<Vec<(String, String)>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("could not read {}: {}", path.display(), error)),
    };
    Ok(parse_dotenv(&text))
}

/// The last assignment of `name` in a parsed `.env`, dotenv override semantics.
fn dotenv_value(pairs: &[(String, String)], name: &str) -> Option<String> {
    pairs
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
}

/// Gemini CLI spells the model either as a plain string or as `{"name": …}`.
fn settings_model(settings: &serde_json::Value) -> Option<String> {
    match settings.get("model") {
        Some(serde_json::Value::String(id)) => Some(id.clone()).filter(|id| !id.is_empty()),
        Some(value) => json_str(value, "name"),
        None => None,
    }
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

    const ENV_NAMES: &[&str] = &[
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "GOOGLE_GEMINI_BASE_URL",
        "GEMINI_MODEL",
    ];

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

    fn write_config(root: &Path, settings: &str, dotenv: &str) {
        let path = root.join(CONFIG_RELATIVE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, settings).unwrap();
        if !dotenv.is_empty() {
            fs::write(root.join(DOTENV_RELATIVE), dotenv).unwrap();
        }
    }

    fn temp_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("the-pair-gemini-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn imports_key_endpoint_and_model() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_config(
            &root,
            r#"{"model": {"name": "gemini-2.5-pro"},
                "mcpServers": {"files": {"command": "files-mcp"}}}"#,
            "# gemini's environment\nexport GOOGLE_GEMINI_BASE_URL=\"https://gw.example.com/v1\"\n\
             GOOGLE_API_KEY='sk-file'\n",
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.protocol, crate::accounts::types::Protocol::OpenAiChat);
        assert_eq!(draft.base_url, "https://gw.example.com/v1");
        assert_eq!(draft.display_name.as_deref(), Some("Gemini (Gemini CLI)"));
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(draft.accounts[0].source, "gemini");
        assert_eq!(draft.accounts[0].secret, Secret::ApiKey("sk-file".into()));
        assert_eq!(draft.models.len(), 1);
        assert_eq!(draft.models[0].id, "gemini-2.5-pro");
        assert!(
            candidate.notes.iter().any(|note| note.contains("MCP")),
            "{:?}",
            candidate.notes
        );
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
        write_config(&root, "{ not json", "GEMINI_API_KEY=sk-file\n");
        let candidate = scan_from(&root);
        assert!(candidate.error.is_some());
        assert!(candidate.drafts.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unreadable_dotenv_fails_the_scan() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_config(
            &root,
            r#"{"model": "gemini-2.5-pro"}"#,
            "GEMINI_API_KEY=sk-file\n",
        );
        // A directory where the file belongs reads as an I/O error, the same
        // shape a wrong owner or mode produces.
        let path = root.join(DOTENV_RELATIVE);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();

        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(
            candidate
                .error
                .as_deref()
                .is_some_and(|error| error.contains(".env")),
            "{:?}",
            candidate.error
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn google_login_is_skipped_with_a_note() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_config(&root, r#"{"model": "gemini-2.5-pro"}"#, "");

        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("Google account") && candidate.notes[0].contains("API key"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn live_variables_beat_the_dotenv_file() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_config(
            &root,
            r#"{"model": "gemini-2.5-flash"}"#,
            "GOOGLE_GEMINI_BASE_URL=https://gw.example.com/v1\nGEMINI_API_KEY=sk-file\n\
             GEMINI_MODEL=gemini-file\n",
        );
        std::env::set_var("GEMINI_API_KEY", "sk-live");
        std::env::set_var("GOOGLE_GEMINI_BASE_URL", "https://live.example.com/v1");

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
            Secret::Env("GEMINI_API_KEY".into())
        );
        assert_eq!(draft.base_url, "https://live.example.com/v1");
        // `GEMINI_MODEL` is unset, so the settings' model still wins.
        assert_eq!(draft.models[0].id, "gemini-2.5-flash");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn without_an_endpoint_override_google_is_used() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_config(
            &root,
            r#"{"model": "gemini-2.5-pro"}"#,
            "GOOGLE_API_KEY=sk-file\n",
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].base_url, DEFAULT_BASE_URL);
        assert_eq!(candidate.drafts[0].models[0].id, "gemini-2.5-pro");
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_model_falls_back_to_the_environment() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_config(&root, "{}", "GEMINI_API_KEY=sk-file\n");
        std::env::set_var("GEMINI_MODEL", "gemini-3-pro");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert_eq!(candidate.drafts[0].models[0].id, "gemini-3-pro");
        assert!(
            !candidate
                .notes
                .iter()
                .any(|note| note.contains("names no model")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_dotenv_model_fills_in_when_nothing_else_names_one() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_config(
            &root,
            r#"{"model": ""}"#,
            "GEMINI_API_KEY=sk-file\nGEMINI_MODEL=gemini-2.5-flash\nGEMINI_MODEL=gemini-3-pro\n",
        );

        let candidate = scan_from(&root);
        // The last assignment in the file wins, like dotenv.
        assert_eq!(candidate.drafts[0].models[0].id, "gemini-3-pro");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_model_object_without_a_name_imports_no_model() {
        let root = temp_root();
        let (_guard, _env) = isolated_env();
        write_config(&root, r#"{"model": {}}"#, "GEMINI_API_KEY=sk-file\n");

        let candidate = scan_from(&root);
        assert!(candidate.drafts[0].models.is_empty());
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
}
