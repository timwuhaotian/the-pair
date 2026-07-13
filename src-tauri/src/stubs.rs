use crate::config_paths::opencode_config_path;
use crate::model_catalog::{AvailableModel, ModelCatalog};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind, ProviderRegistry};
use crate::util::is_mock_mode;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Manager;

const MODEL_CACHE_TTL: Duration = Duration::from_secs(60);
const MODEL_CACHE_FILE_NAME: &str = "model-cache.json";

type TimedCache<T> = OnceLock<Mutex<Option<(Instant, Vec<T>)>>>;

static MODEL_CACHE: TimedCache<AvailableModel> = OnceLock::new();
static PROVIDER_CACHE: TimedCache<DetectedProviderProfile> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelCacheRecord {
    saved_at: u64,
    models: Vec<AvailableModel>,
}

fn detect_profiles() -> Vec<DetectedProviderProfile> {
    if is_mock_mode() {
        ProviderRegistry::detect_all_mock()
    } else {
        ProviderRegistry::detect_all()
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn model_cache_path_in_dir(dir: &Path) -> PathBuf {
    dir.join(MODEL_CACHE_FILE_NAME)
}

fn model_cache_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(model_cache_path_in_dir(
        &app.path()
            .app_data_dir()
            .map_err(|e| format!("Failed to resolve app data dir: {}", e))?,
    ))
}

fn read_model_cache_in_dir(dir: &Path) -> Result<Option<ModelCacheRecord>, String> {
    let path = model_cache_path_in_dir(dir);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    match serde_json::from_str::<ModelCacheRecord>(&raw) {
        Ok(record) => Ok(Some(record)),
        Err(error) => {
            println!("[Tauri] Ignoring corrupt model cache {:?}: {}", path, error);
            Ok(None)
        }
    }
}

fn write_model_cache_in_dir(
    dir: &Path,
    models: &[AvailableModel],
) -> Result<ModelCacheRecord, String> {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create model cache dir: {}", e))?;
    let record = ModelCacheRecord {
        saved_at: now_millis(),
        models: models.to_vec(),
    };
    let path = model_cache_path_in_dir(dir);
    let tmp_path = path.with_extension("tmp");
    let payload = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    fs::write(&tmp_path, payload).map_err(|e| format!("Failed to write model cache: {}", e))?;
    fs::rename(&tmp_path, &path).map_err(|e| format!("Failed to move model cache: {}", e))?;
    Ok(record)
}

fn set_model_memory_cache(models: Vec<AvailableModel>) {
    let cache = MODEL_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap();
    *guard = Some((Instant::now(), models));
}

fn cached_models_from_memory() -> Option<Vec<AvailableModel>> {
    let cache = MODEL_CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache.lock().unwrap();
    guard.as_ref().map(|(_, models)| models.clone())
}

fn detect_model_catalog() -> Vec<AvailableModel> {
    let profiles = detect_profiles();
    ModelCatalog::build_catalog(profiles)
}

#[tauri::command]
pub fn config_read() -> Result<Option<serde_json::Value>, String> {
    let path = opencode_config_path().ok_or("Could not determine home directory")?;
    println!("[Tauri] Reading config from: {:?}", path);
    if !path.exists() {
        println!("[Tauri] Config file does not exist at {:?}", path);
        return Ok(None);
    }

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        println!("[Tauri] Config parse error: {}", e);
        e.to_string()
    })?;
    println!("[Tauri] Config loaded successfully");
    Ok(Some(config))
}

#[tauri::command]
pub fn config_get_models() -> Result<Vec<AvailableModel>, String> {
    let cache = MODEL_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap();
    if let Some((ts, ref models)) = *guard {
        if ts.elapsed() < MODEL_CACHE_TTL {
            return Ok(models.clone());
        }
    }
    let catalog = detect_model_catalog();
    println!("[Tauri] config_get_models found {} models", catalog.len());
    *guard = Some((Instant::now(), catalog.clone()));
    Ok(catalog)
}

#[tauri::command]
pub fn config_get_cached_models(app: tauri::AppHandle) -> Result<Vec<AvailableModel>, String> {
    if let Some(models) = cached_models_from_memory() {
        return Ok(models);
    }

    let path = model_cache_path(&app)?;
    let Some(dir) = path.parent() else {
        return Ok(Vec::new());
    };
    let Some(record) = read_model_cache_in_dir(dir)? else {
        return Ok(Vec::new());
    };

    set_model_memory_cache(record.models.clone());
    Ok(record.models)
}

#[tauri::command]
pub fn config_refresh_models(app: tauri::AppHandle) -> Result<Vec<AvailableModel>, String> {
    let catalog = detect_model_catalog();
    let path = model_cache_path(&app)?;
    if let Some(dir) = path.parent() {
        let _record = write_model_cache_in_dir(dir, &catalog)?;
    }
    set_model_memory_cache(catalog.clone());
    println!(
        "[Tauri] config_refresh_models found {} models",
        catalog.len()
    );
    Ok(catalog)
}

#[tauri::command]
pub fn config_get_providers() -> Result<Vec<DetectedProviderProfile>, String> {
    let cache = PROVIDER_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap();
    if let Some((ts, ref profiles)) = *guard {
        if ts.elapsed() < MODEL_CACHE_TTL {
            return Ok(profiles.clone());
        }
    }
    let profiles = detect_profiles();
    *guard = Some((Instant::now(), profiles.clone()));
    Ok(profiles)
}

#[tauri::command]
pub fn pair_get_messages(
    broker: tauri::State<std::sync::Mutex<MessageBroker>>,
    pair_id: String,
) -> Result<Vec<crate::types::Message>, String> {
    let broker = broker.lock().unwrap();
    let state = broker
        .get_state(&pair_id)
        .ok_or_else(|| format!("No broker state found for pair {}", pair_id))?;
    Ok(state.messages)
}

use crate::message_broker::MessageBroker;
use crate::types::PairState;

#[tauri::command]
pub fn pair_get_state(
    broker: tauri::State<std::sync::Mutex<MessageBroker>>,
    pair_id: String,
) -> Result<Option<PairState>, String> {
    let broker = broker.lock().unwrap();
    Ok(broker.get_state(&pair_id))
}

#[tauri::command]
pub fn config_open_file() -> Result<(), String> {
    let path = opencode_config_path().ok_or("Could not determine home directory")?;

    // Create default config if file doesn't exist
    if !path.exists() {
        let parent = path
            .parent()
            .ok_or("Could not determine config directory")?;
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;

        // Minimal default config - OpenCode CLI detects free models automatically
        // This file is only needed for custom BYOK providers
        let default_config = serde_json::json!({
            "$schema": "https://opencode.dev/schema/config.json"
        });

        let content = serde_json::to_string_pretty(&default_config)
            .map_err(|e| format!("Failed to serialize default config: {}", e))?;
        std::fs::write(&path, content)
            .map_err(|e| format!("Failed to write config file: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Launch a provider's login command in the system's native terminal.
/// Fire-and-forget — the terminal stays open for the user to complete the login flow.
///
/// The login command is resolved **server-side** from the `ProviderKind` enum —
/// the frontend never sends a free-form command string. This prevents command
/// injection via a crafted `login_command` parameter.
#[tauri::command]
pub fn provider_launch_login(provider_kind: ProviderKind) -> Result<(), String> {
    use crate::providers::provider_for_kind;

    let provider = provider_for_kind(provider_kind)
        .ok_or_else(|| format!("No login command for {:?}", provider_kind))?;
    let login_command = provider
        .login_command()
        .ok_or_else(|| format!("Provider {:?} has no login command", provider_kind))?;

    #[cfg(target_os = "macos")]
    {
        // AppleScript opens Terminal.app and runs the command.
        let script = format!(
            "tell application \"Terminal\" to do script \"{}\"",
            login_command.replace('\\', "\\\\").replace('"', "\\\"")
        );
        std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()
            .map_err(|e| format!("Failed to open Terminal: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        // Try common terminal emulators. We invoke `sh -c "<cmd>"` so multi-word
        // commands (e.g. "claude login") work correctly. After the login command
        // exits, we `exec sh` to keep the terminal window open for the user.
        let shell_cmd = format!("{}; exec sh", login_command);
        let launched = [
            // gnome-terminal: `gnome-terminal -- sh -c '<cmd>; exec sh'`
            ("gnome-terminal", &["--", "sh", "-c"][..]),
            // konsole: `konsole -e sh -c '<cmd>; exec sh'`
            ("konsole", &["-e", "sh", "-c"][..]),
            // x-terminal-emulator (Debian/Ubuntu wrapper)
            ("x-terminal-emulator", &["-e", "sh", "-c"][..]),
            // xterm fallback
            ("xterm", &["-e", "sh", "-c"][..]),
        ]
        .iter()
        .any(|(term, prefix)| {
            std::process::Command::new(term)
                .args(*prefix)
                .arg(&shell_cmd)
                .spawn()
                .is_ok()
        });
        if !launched {
            return Err("Could not find a terminal emulator. Run this command manually:".into());
        }
    }

    #[cfg(target_os = "windows")]
    {
        // `start ""` — the empty string is consumed as the window title,
        // preventing `start` from misinterpreting quoted commands.
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg("cmd")
            .arg("/k")
            .arg(&login_command)
            .spawn()
            .map_err(|e| format!("Failed to open terminal: {}", e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_registry::ProviderKind;
    use uuid::Uuid;

    fn temp_cache_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("the-pair-model-cache-test-{}", Uuid::new_v4()))
    }

    fn test_model(model_id: &str) -> AvailableModel {
        AvailableModel {
            provider: ProviderKind::Codex,
            model_id: model_id.to_string(),
            display_name: model_id.to_string(),
            available: true,
            provider_label: "Codex".to_string(),
            source_provider: Some("openai".to_string()),
            source_provider_label: "OpenAI".to_string(),
            billing_kind: "plan".to_string(),
            billing_label: "Included with plan".to_string(),
            access_label: "ChatGPT plan".to_string(),
            plan_label: Some("subscription-backed".to_string()),
            availability_status: "ready".to_string(),
            availability_reason: None,
            supports_pair_execution: true,
            recommended_roles: vec!["mentor".to_string(), "executor".to_string()],
            reasoning_effort_levels: None,
            canonical_key: format!("openai::{}", model_id),
            canonical_display_name: model_id.to_string(),
            effort_tag: None,
        }
    }

    #[test]
    fn model_cache_round_trips_models_from_disk() {
        let dir = temp_cache_dir();
        let models = vec![test_model("gpt-test-startup")];

        write_model_cache_in_dir(&dir, &models).expect("cache write should succeed");
        let cached = read_model_cache_in_dir(&dir)
            .expect("cache read should not fail")
            .expect("cache should exist");

        assert_eq!(cached.models.len(), 1);
        assert_eq!(cached.models[0].model_id, "gpt-test-startup");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_model_cache_returns_none() {
        let dir = temp_cache_dir();
        std::fs::create_dir_all(&dir).expect("cache dir should be created");
        std::fs::write(model_cache_path_in_dir(&dir), "{not-json").expect("cache should be seeded");

        let cached = read_model_cache_in_dir(&dir).expect("corrupt cache should be non-fatal");

        assert!(cached.is_none());

        let _ = std::fs::remove_dir_all(dir);
    }
}
