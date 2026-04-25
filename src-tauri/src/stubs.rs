use crate::config_paths::opencode_config_path;
use crate::model_catalog::{AvailableModel, ModelCatalog};
use crate::provider_registry::{DetectedProviderProfile, ProviderRegistry};
use crate::util::is_mock_mode;
use std::fs;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MODEL_CACHE_TTL: Duration = Duration::from_secs(60);

static MODEL_CACHE: OnceLock<Mutex<Option<(Instant, Vec<AvailableModel>)>>> = OnceLock::new();
static PROVIDER_CACHE: OnceLock<Mutex<Option<(Instant, Vec<DetectedProviderProfile>)>>> =
    OnceLock::new();

fn detect_profiles() -> Vec<DetectedProviderProfile> {
    if is_mock_mode() {
        ProviderRegistry::detect_all_mock()
    } else {
        ProviderRegistry::detect_all()
    }
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
    let profiles = detect_profiles();
    let catalog = ModelCatalog::build_catalog(profiles);
    println!("[Tauri] config_get_models found {} models", catalog.len());
    *guard = Some((Instant::now(), catalog.clone()));
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
pub fn pair_retry_turn() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn pair_get_messages() -> Result<Vec<()>, String> {
    Ok(vec![])
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
