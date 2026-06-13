use crate::config_paths::{opencode_auth_path, opencode_config_path};
use crate::path_env::fallback_path_dirs;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CLI_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Opencode,
    Codex,
    Claude,
    Gemini,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DetectedModelOption {
    pub model_id: String,
    pub display_name: String,
    pub source_provider: Option<String>,
    pub family: Option<String>,
    pub subscription_label: String,
    pub supports_pair_execution: bool,
    pub runnable: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProviderProfile {
    pub kind: ProviderKind,
    pub installed: bool,
    pub authenticated: bool,
    pub runnable: bool,
    pub subscription_label: String,
    pub current_models: Vec<DetectedModelOption>,
    pub detected_at: u64,
}

pub(crate) fn homedir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let home = std::env::var("USERPROFILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("APPDATA").ok().and_then(|value| {
                std::path::Path::new(&value)
                    .parent()
                    .map(|path| path.to_string_lossy().into_owned())
            })
        })
        .or_else(|| {
            std::env::var("LOCALAPPDATA").ok().and_then(|value| {
                std::path::Path::new(&value)
                    .parent()
                    .map(|path| path.to_string_lossy().into_owned())
            })
        })
        .unwrap_or_default();
    #[cfg(not(target_os = "windows"))]
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
}

pub(crate) fn cli_environment_overrides(home: &std::path::Path) -> Vec<(OsString, OsString)> {
    let mut overrides = Vec::new();

    if !home.as_os_str().is_empty() {
        overrides.push((OsString::from("HOME"), home.as_os_str().to_owned()));
        overrides.push((OsString::from("USERPROFILE"), home.as_os_str().to_owned()));

        #[cfg(target_os = "windows")]
        {
            if std::env::var_os("APPDATA").is_none() {
                overrides.push((
                    OsString::from("APPDATA"),
                    home.join("AppData/Roaming").into_os_string(),
                ));
            }
            if std::env::var_os("LOCALAPPDATA").is_none() {
                overrides.push((
                    OsString::from("LOCALAPPDATA"),
                    home.join("AppData/Local").into_os_string(),
                ));
            }
        }
    }

    if let Some(appdata) = std::env::var_os("APPDATA") {
        overrides.push((OsString::from("APPDATA"), appdata));
    }

    if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
        overrides.push((OsString::from("LOCALAPPDATA"), local_appdata));
    }

    let fallback_dirs = fallback_path_dirs(
        Some(home.to_path_buf()),
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        cfg!(target_os = "windows"),
    );
    let base_path = std::env::var_os("PATH").unwrap_or_default();
    let mut path_entries: Vec<PathBuf> = std::env::split_paths(&base_path).collect();
    for dir in fallback_dirs {
        if !path_entries.iter().any(|entry| entry == &dir) {
            path_entries.push(dir);
        }
    }
    if let Ok(path) = std::env::join_paths(path_entries) {
        overrides.push((OsString::from("PATH"), path));
    }

    overrides
}

fn prepare_cli_command(command: &mut Command, home: &std::path::Path) {
    for (key, value) in cli_environment_overrides(home) {
        command.env(key, value);
    }
}

fn capture_command_output_with_timeout(
    command_path: &Path,
    args: &[&str],
    home: &std::path::Path,
    timeout: Duration,
) -> Option<String> {
    let mut command = Command::new(command_path);
    command.args(args);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());
    prepare_cli_command(&mut command, home);

    let mut child = command.spawn().ok()?;
    let started = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                if !status.success() {
                    return None;
                }
                return String::from_utf8(output.stdout).ok();
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub fn which_binary(name: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH").and_then(|value| {
        std::env::split_paths(&value).find_map(|dir| resolve_binary_in_dir(&dir, name))
    }) {
        return Some(path);
    }

    if let Some(path) = resolve_binary_at_known_locations(name, &homedir()) {
        return Some(path);
    }

    #[cfg(target_os = "windows")]
    let cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let cmd = "which";

    // Try `which`/`where` first — the first line of output is the resolved path.
    if let Ok(output) = Command::new(cmd).arg(name).output() {
        if output.status.success() {
            if let Some(path) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty())
                .map(PathBuf::from)
            {
                return Some(path);
            }
        }
    }

    None
}

fn which_binary_exists(name: &str) -> bool {
    which_binary(name).is_some()
}

fn safe_read_json<T: DeserializeOwned>(path: impl AsRef<std::path::Path>) -> Option<T> {
    let path = path.as_ref();
    if !path.exists() {
        return None;
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

fn detected_at_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn resolve_binary_at_known_locations(name: &str, home: &std::path::Path) -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let fallback_dirs = fallback_path_dirs(
        Some(home.to_path_buf()),
        appdata.clone(),
        local_appdata.clone(),
        false,
    );
    for dir in &fallback_dirs {
        if let Some(path) = resolve_binary_in_dir(dir, name) {
            return Some(path);
        }
    }

    let windows_fallback_dirs =
        fallback_path_dirs(Some(home.to_path_buf()), appdata, local_appdata, true);
    for dir in &windows_fallback_dirs {
        if let Some(path) = resolve_binary_in_dir(dir, name) {
            return Some(path);
        }
    }

    None
}

#[allow(dead_code)]
fn binary_exists_at_known_locations(name: &str, home: &std::path::Path) -> bool {
    resolve_binary_at_known_locations(name, home).is_some()
}

fn resolve_binary_in_dir(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
    let candidate = dir.join(name);
    if candidate.exists() {
        return Some(candidate);
    }

    for suffix in [".cmd", ".exe", ".bat"] {
        let full = dir.join(format!("{name}{suffix}"));
        if full.exists() {
            return Some(full);
        }
    }

    None
}

fn push_unique_model_id(model_ids: &mut Vec<String>, model_id: &str) {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return;
    }

    if !model_ids.iter().any(|existing| existing == trimmed) {
        model_ids.push(trimmed.to_string());
    }
}

fn extract_quoted_segments(line: &str) -> Vec<String> {
    line.split('"')
        .enumerate()
        .filter_map(|(index, segment)| (index % 2 == 1).then_some(segment.trim().to_string()))
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn extract_single_quoted_segments(line: &str) -> Vec<String> {
    line.split('\'')
        .enumerate()
        .filter_map(|(index, segment)| (index % 2 == 1).then_some(segment.trim().to_string()))
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn collect_json_string_values(
    value: &serde_json::Value,
    interesting_keys: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map {
                if interesting_keys.iter().any(|candidate| candidate == key) {
                    if let Some(string_value) = nested.as_str() {
                        if predicate(string_value) {
                            push_unique_model_id(model_ids, string_value);
                        }
                    }
                }
                collect_json_string_values(nested, interesting_keys, predicate, model_ids);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_string_values(item, interesting_keys, predicate, model_ids);
            }
        }
        _ => {}
    }
}

fn collect_model_ids_from_json_file(
    path: &std::path::Path,
    interesting_keys: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    if let Some(value) = safe_read_json::<serde_json::Value>(path) {
        collect_json_string_values(&value, interesting_keys, predicate, model_ids);
    }
}

fn collect_model_ids_from_jsonl_file(
    path: &std::path::Path,
    interesting_keys: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            collect_json_string_values(&value, interesting_keys, predicate, model_ids);
        }
    }
}

fn collect_recent_files(
    root: &std::path::Path,
    extensions: &[&str],
    max_depth: usize,
    limit: usize,
) -> Vec<PathBuf> {
    fn visit_dir(
        dir: &std::path::Path,
        extensions: &[&str],
        max_depth: usize,
        depth: usize,
        files: &mut Vec<(std::time::SystemTime, PathBuf)>,
    ) {
        if depth > max_depth {
            return;
        }

        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_dir() {
                visit_dir(&path, extensions, max_depth, depth + 1, files);
                continue;
            }

            let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
                continue;
            };

            if !extensions.iter().any(|candidate| candidate == &extension) {
                continue;
            }

            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            files.push((modified, path));
        }
    }

    let mut files = Vec::new();
    visit_dir(root, extensions, max_depth, 0, &mut files);
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files
        .into_iter()
        .take(limit)
        .map(|(_, path)| path)
        .collect()
}

fn collect_model_ids_from_recent_files(
    root: &std::path::Path,
    extensions: &[&str],
    interesting_keys: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    max_depth: usize,
    limit: usize,
    model_ids: &mut Vec<String>,
) {
    for path in collect_recent_files(root, extensions, max_depth, limit) {
        match path.extension().and_then(|value| value.to_str()) {
            Some("json") => {
                collect_model_ids_from_json_file(&path, interesting_keys, predicate, model_ids)
            }
            Some("jsonl") => {
                collect_model_ids_from_jsonl_file(&path, interesting_keys, predicate, model_ids)
            }
            _ => {}
        }
    }
}

fn collect_model_ids_from_toml_text(
    content: &str,
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("model =") {
            if let Some(model_id) = line.split('"').nth(1) {
                if predicate(model_id) {
                    push_unique_model_id(model_ids, model_id);
                }
            }
            continue;
        }

        for model_id in extract_quoted_segments(line) {
            if predicate(&model_id) {
                push_unique_model_id(model_ids, &model_id);
            }
        }
    }
}

fn collect_model_ids_from_help_line(
    help_text: &str,
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    for line in help_text.lines() {
        if !line.contains("--model") {
            continue;
        }

        for candidate in extract_single_quoted_segments(line)
            .into_iter()
            .chain(extract_quoted_segments(line))
        {
            if !candidate.contains(char::is_whitespace) && predicate(&candidate) {
                push_unique_model_id(model_ids, &candidate);
            }
        }
    }
}

fn is_plausible_model_id(value: &str) -> bool {
    if value.len() < 4 {
        return false;
    }
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c.is_alphanumeric())
        && chars.all(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '/')
}

fn beautify_claude_display_name(model_id: &str) -> String {
    let lower = model_id.to_lowercase();
    if !lower.starts_with("claude-") {
        return model_id.to_string();
    }

    let mut parts: Vec<String> = lower.split('-').map(|s| s.to_string()).collect();

    // Remove date suffix (8-digit segment at end)
    if let Some(last) = parts.last() {
        if last.len() == 8 && last.chars().all(|c| c.is_ascii_digit()) {
            parts.pop();
        }
    }

    // Handle old format: claude-{major}-{minor}-{name} (e.g., claude-3-5-sonnet)
    if parts.len() >= 4
        && parts[1].len() == 1
        && parts[2].len() == 1
        && parts[1].chars().all(|c| c.is_ascii_digit())
        && parts[2].chars().all(|c| c.is_ascii_digit())
    {
        let minor = parts.remove(2);
        let major = parts.remove(1);
        parts.insert(1, format!("{}.{}", major, minor));
    }

    // Merge trailing version segments (e.g., "4" + "5" → "4.5")
    if parts.len() >= 2 {
        let last = parts[parts.len() - 1].clone();
        let prev = parts[parts.len() - 2].clone();
        if last.len() <= 2
            && prev.len() <= 2
            && last.chars().all(|c| c.is_ascii_digit())
            && prev.chars().all(|c| c.is_ascii_digit())
        {
            parts.pop();
            parts.pop();
            parts.push(format!("{}.{}", prev, last));
        }
    }

    // Capitalize each word
    parts
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_detected_models(
    model_ids: Vec<String>,
    source_provider: &str,
    subscription_label: &str,
) -> Vec<DetectedModelOption> {
    model_ids
        .into_iter()
        .filter(|id| is_plausible_model_id(id))
        .map(|model_id| DetectedModelOption {
            display_name: model_id.clone(),
            model_id,
            source_provider: Some(source_provider.to_string()),
            family: None,
            subscription_label: subscription_label.to_string(),
            supports_pair_execution: true,
            runnable: true,
        })
        .collect()
}

fn is_codex_model_id(value: &str) -> bool {
    value.starts_with("gpt-")
        || value.starts_with("codex-")
        || value
            .strip_prefix('o')
            .and_then(|suffix| suffix.chars().next())
            .map(|ch| ch.is_ascii_digit())
            .unwrap_or(false)
}

fn is_gemini_model_id(value: &str) -> bool {
    value.starts_with("gemini-")
}

fn is_claude_model_id(value: &str) -> bool {
    value.starts_with("claude-")
}

fn discover_codex_model_ids(home: &std::path::Path) -> Vec<String> {
    let predicate = |value: &str| is_codex_model_id(value);
    let mut model_ids = Vec::new();
    let config_path = home.join(".codex/config.toml");
    let models_cache_path = home.join(".codex/models_cache.json");

    collect_model_ids_from_json_file(&models_cache_path, &["slug"], &predicate, &mut model_ids);

    if let Ok(content) = fs::read_to_string(config_path) {
        collect_model_ids_from_toml_text(&content, &predicate, &mut model_ids);
    }

    collect_model_ids_from_recent_files(
        &home.join(".codex"),
        &["json", "jsonl"],
        &["model", "slug"],
        &predicate,
        2,
        20,
        &mut model_ids,
    );

    model_ids
}

fn discover_gemini_model_ids(home: &std::path::Path) -> Vec<String> {
    let predicate = |value: &str| is_gemini_model_id(value);
    let mut model_ids = Vec::new();
    let settings_path = home.join(".gemini/settings.json");

    collect_model_ids_from_json_file(
        &settings_path,
        &["model", "name"],
        &predicate,
        &mut model_ids,
    );
    collect_model_ids_from_recent_files(
        &home.join(".gemini/tmp"),
        &["json", "jsonl"],
        &["model", "name"],
        &predicate,
        4,
        25,
        &mut model_ids,
    );

    model_ids
}

fn discover_claude_model_ids(home: &std::path::Path, command_path: Option<&Path>) -> Vec<String> {
    let mut model_ids = Vec::new();

    if let Some(command_path) = command_path {
        if let Some(help_text) = capture_claude_help_text(home, command_path) {
            let help_predicate =
                |value: &str| is_plausible_model_id(value) && !value.starts_with("--");
            collect_model_ids_from_help_line(&help_text, &help_predicate, &mut model_ids);
        }
    }

    let history_predicate = |value: &str| is_claude_model_id(value);
    collect_model_ids_from_recent_files(
        &home.join(".claude/projects"),
        &["json", "jsonl"],
        &["model"],
        &history_predicate,
        3,
        25,
        &mut model_ids,
    );

    model_ids
}

fn capture_claude_help_text(home: &std::path::Path, command_path: &Path) -> Option<String> {
    capture_command_output_with_timeout(command_path, &["--help"], home, CLI_PROBE_TIMEOUT)
}

fn claude_credentials_paths(home: &std::path::Path) -> Vec<PathBuf> {
    let mut paths = vec![home.join(".claude/.credentials.json")];

    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        paths.push(appdata.join("Claude/.credentials.json"));
    }

    if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        paths.push(local_appdata.join("Claude/.credentials.json"));
    }

    paths
}

fn has_claude_credentials(home: &std::path::Path) -> bool {
    claude_credentials_paths(home)
        .into_iter()
        .any(|path| path.exists() && safe_read_json::<serde_json::Value>(&path).is_some())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenCodeConfig {
    pub provider: Option<HashMap<String, ProviderConfig>>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    pub options: Option<ProviderOptions>,
    pub models: Option<HashMap<String, ModelConfig>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderOptions {
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "baseURL")]
    pub base_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelConfig {
    pub name: Option<String>,
}

pub struct ProviderRegistry;

impl ProviderRegistry {
    pub fn detect_all() -> Vec<DetectedProviderProfile> {
        let opencode = thread::spawn(Self::detect_opencode);
        let codex = thread::spawn(Self::detect_codex);
        let claude = thread::spawn(Self::detect_claude);
        let gemini = thread::spawn(Self::detect_gemini);

        vec![
            opencode
                .join()
                .expect("OpenCode detection should not panic"),
            codex.join().expect("Codex detection should not panic"),
            claude.join().expect("Claude detection should not panic"),
            gemini.join().expect("Gemini detection should not panic"),
        ]
    }

    pub fn detect_all_mock() -> Vec<DetectedProviderProfile> {
        vec![
            DetectedProviderProfile {
                kind: ProviderKind::Opencode,
                installed: true,
                authenticated: true,
                runnable: true,
                subscription_label: "mock".to_string(),
                current_models: vec![DetectedModelOption {
                    model_id: "opencode/glm-5-turbo".to_string(),
                    display_name: "GLM-5 Turbo (Mock)".to_string(),
                    source_provider: Some("opencode".to_string()),
                    family: None,
                    subscription_label: "mock".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                }],
                detected_at: detected_at_now(),
            },
            DetectedProviderProfile {
                kind: ProviderKind::Claude,
                installed: true,
                authenticated: true,
                runnable: true,
                subscription_label: "mock".to_string(),
                current_models: vec![DetectedModelOption {
                    model_id: "claude-sonnet-4-20250514".to_string(),
                    display_name: "Claude Sonnet 4 (Mock)".to_string(),
                    source_provider: Some("claude".to_string()),
                    family: None,
                    subscription_label: "mock".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                }],
                detected_at: detected_at_now(),
            },
        ]
    }

    pub fn detect_opencode() -> DetectedProviderProfile {
        let opencode_path = which_binary("opencode");
        let installed = opencode_path.is_some();

        let mut models = Vec::new();
        let mut authenticated = false;

        // 1. Detect from ~/.config/opencode/opencode.json (user custom models)
        let config_path = opencode_config_path()
            .unwrap_or_else(|| homedir().join(".config/opencode/opencode.json"));
        if config_path.exists() {
            authenticated = true;
            if let Some(config) = safe_read_json::<OpenCodeConfig>(config_path) {
                if let Some(providers) = config.provider {
                    for (provider_id, provider_data) in providers {
                        if let Some(model_list) = provider_data.models {
                            for (model_id, model_config) in model_list {
                                let display_name =
                                    model_config.name.unwrap_or_else(|| model_id.clone());
                                models.push(DetectedModelOption {
                                    model_id: format!("{}/{}", provider_id, model_id),
                                    display_name,
                                    source_provider: Some(provider_id.clone()),
                                    family: None,
                                    subscription_label: "custom-provider".into(),
                                    supports_pair_execution: true,
                                    runnable: true,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 2. Detect from ~/.local/share/opencode/auth.json (internal providers via /connect)
        let auth_path = opencode_auth_path()
            .unwrap_or_else(|| homedir().join(".local/share/opencode/auth.json"));
        let mut internal_providers = Vec::new();
        if auth_path.exists() {
            authenticated = true;
            if let Some(auth_data) = safe_read_json::<serde_json::Value>(auth_path) {
                if let Some(obj) = auth_data.as_object() {
                    for provider_id in obj.keys() {
                        internal_providers.push(provider_id.clone());
                    }
                }
            }
        }

        // 3. Detect from 'opencode models' command output
        if installed {
            let bin_path = opencode_path.expect("opencode path should be resolved");
            if let Some(content) = capture_command_output_with_timeout(
                &bin_path,
                &["models"],
                &homedir(),
                CLI_PROBE_TIMEOUT,
            ) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    // Check if we already added this model from config
                    if models.iter().any(|m| m.model_id == line) {
                        continue;
                    }

                    let parts: Vec<&str> = line.split('/').collect();
                    if parts.len() >= 2 {
                        let provider_id = parts[0];
                        let model_name = parts[1..].join("/");

                        // Check if this model belongs to an authenticated provider (either internal or custom).
                        // The "opencode" provider_id represents zen-backed models that are available
                        // whenever opencode is installed — they don't appear in auth.json.
                        let is_authenticated = provider_id == "opencode"
                            || internal_providers.contains(&provider_id.to_string())
                            || models
                                .iter()
                                .any(|m| m.source_provider.as_deref() == Some(provider_id));

                        if is_authenticated {
                            // Derive family from model name for OpenCode models
                            // e.g., "minimax-m2.5" -> "minimax", "claude-3-5-sonnet" -> "claude"
                            let family = if provider_id == "opencode" {
                                model_name.split('-').next().map(|s| s.to_string())
                            } else {
                                None
                            };

                            models.push(DetectedModelOption {
                                model_id: line.to_string(),
                                display_name: model_name,
                                source_provider: Some(provider_id.to_string()),
                                family,
                                subscription_label: if provider_id == "opencode" {
                                    "zen-backed".into()
                                } else {
                                    "internal-provider".into()
                                },
                                supports_pair_execution: true,
                                runnable: true,
                            });
                        }
                    }
                }
            }
        }

        DetectedProviderProfile {
            kind: ProviderKind::Opencode,
            installed,
            authenticated,
            runnable: installed,
            subscription_label: "multi-provider".into(),
            current_models: models,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_codex() -> DetectedProviderProfile {
        let installed = which_binary_exists("codex");

        let homedir = homedir();
        let auth_path = homedir.join(".codex/auth.json");
        let config_path = homedir.join(".codex/config.toml");

        let authenticated = auth_path.exists();
        let subscription_label = "subscription-backed".to_string();
        let models = if installed {
            build_detected_models(
                discover_codex_model_ids(&homedir),
                "openai",
                &subscription_label,
            )
        } else {
            Vec::new()
        };

        DetectedProviderProfile {
            kind: ProviderKind::Codex,
            installed,
            authenticated,
            runnable: installed && (authenticated || config_path.exists()),
            subscription_label,
            current_models: models,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_claude() -> DetectedProviderProfile {
        let claude_path = which_binary("claude");
        let installed = claude_path.is_some();

        let homedir = homedir();
        let mut authenticated = false;
        let mut subscription_label = "api-backed".to_string();
        let mut model_ids = Vec::new();

        if installed {
            let bin_path = claude_path.expect("claude path should be resolved");
            if let Some(status_str) = capture_command_output_with_timeout(
                &bin_path,
                &["auth", "status"],
                &homedir,
                CLI_PROBE_TIMEOUT,
            ) {
                if let Ok(status) = serde_json::from_str::<serde_json::Value>(&status_str) {
                    if status
                        .get("loggedIn")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        authenticated = true;
                        subscription_label = "subscription-backed".to_string();
                    }
                }
            }

            if !authenticated && has_claude_credentials(&homedir) {
                authenticated = true;
                subscription_label = "subscription-backed".to_string();
            }

            // Also check for ANTHROPIC_API_KEY env var as fallback auth
            if !authenticated && std::env::var("ANTHROPIC_API_KEY").is_ok() {
                authenticated = true;
            }

            model_ids = discover_claude_model_ids(&homedir, Some(&bin_path));
        }

        let mut claude_models = build_detected_models(model_ids, "anthropic", &subscription_label);
        for model in &mut claude_models {
            model.display_name = beautify_claude_display_name(&model.model_id);
        }

        DetectedProviderProfile {
            kind: ProviderKind::Claude,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label,
            current_models: claude_models,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_gemini() -> DetectedProviderProfile {
        // Prefer Antigravity CLI (`agy`) — Google's successor to the Gemini CLI, which
        // stops serving requests on 2026-06-18. Fall back to the legacy `gemini` binary.
        if let Some(agy_bin) = which_binary("agy") {
            let models = discover_antigravity_model_ids(&agy_bin);
            let authenticated = !models.is_empty();
            return DetectedProviderProfile {
                kind: ProviderKind::Gemini,
                installed: true,
                authenticated,
                runnable: authenticated,
                subscription_label: "antigravity-backed".into(),
                current_models: models,
                detected_at: detected_at_now(),
            };
        }

        let installed = which_binary_exists("gemini");

        let homedir = homedir();
        let settings_path = homedir.join(".gemini/settings.json");
        let mut authenticated = false;

        if settings_path.exists() {
            authenticated = true;
        }

        let models = if installed {
            build_detected_models(
                discover_gemini_model_ids(&homedir),
                "google",
                "subscription-backed",
            )
        } else {
            Vec::new()
        };

        DetectedProviderProfile {
            kind: ProviderKind::Gemini,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label: "subscription-backed".into(),
            current_models: models,
            detected_at: detected_at_now(),
        }
    }
}

/// Resolve which executable backs the "Gemini" provider. Prefers Antigravity CLI
/// (`agy`, the successor to the sunsetting Gemini CLI) when installed, falling back
/// to `gemini`. Returns `(executable_name, is_antigravity)`.
pub fn resolve_gemini_executable() -> (String, bool) {
    if which_binary("agy").is_some() {
        return ("agy".to_string(), true);
    }
    ("gemini".to_string(), false)
}

/// Discover models from Antigravity CLI via `agy models`. The CLI prints one display
/// name per line (e.g. "Gemini 3.5 Flash (Low)") and those names double as the
/// `--model` values. We surface only the Gemini-family models here: agy also offers
/// Claude/GPT models, but those are owned by the native Claude/Codex providers and
/// would misroute if inferred from the display name.
fn discover_antigravity_model_ids(agy_bin: &Path) -> Vec<DetectedModelOption> {
    // agy models may hit the network on first run; give it more headroom than the
    // default 3s CLI probe.
    let output = capture_command_output_with_timeout(
        agy_bin,
        &["models"],
        &homedir(),
        Duration::from_secs(8),
    )
    .unwrap_or_default();

    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line.to_ascii_lowercase().starts_with("gemini "))
        .map(|name| DetectedModelOption {
            model_id: name.to_string(),
            display_name: name.to_string(),
            source_provider: Some("google".to_string()),
            family: Some("gemini".to_string()),
            subscription_label: "antigravity-backed".to_string(),
            supports_pair_execution: true,
            runnable: true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    fn write_executable_script(dir: &Path, name: &str, contents: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(name);
        fs::write(&path, contents).expect("failed to write test script");
        let mut perms = fs::metadata(&path)
            .expect("failed to read script metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("failed to mark test script executable");
        path
    }

    #[test]
    fn detect_opencode_does_not_invent_models_without_local_sources() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let opencode_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        fs::create_dir_all(&opencode_dir).expect("failed to create temp opencode dir");

        write_executable_script(
            &opencode_dir,
            "opencode",
            r#"#!/bin/sh
if [ "$1" = "models" ]; then
  exit 0
fi
exit 0
"#,
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", opencode_dir.display(), existing.to_string_lossy())
        } else {
            opencode_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);

        let profile = ProviderRegistry::detect_opencode();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert!(profile.installed);
        assert!(
            profile.current_models.is_empty(),
            "OpenCode should not invent fallback models when no local sources are present"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_claude_lists_models_even_when_auth_status_is_logged_out() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let bin_dir = temp_root.join("bin");
        fs::create_dir_all(&bin_dir).expect("failed to create temp bin dir");

        write_executable_script(
            &bin_dir,
            "claude",
            r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  printf '%s\n' '{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}'
elif [ "$1" = "--help" ]; then
  printf '%s\n' "  --model <model>  Use aliases like 'sonnet' and full names like 'claude-sonnet-4-6'"
else
  exit 0
fi
"#,
        );

        let original_path = std::env::var_os("PATH");
        let original_api_key = std::env::var_os("ANTHROPIC_API_KEY");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", bin_dir.display(), existing.to_string_lossy())
        } else {
            bin_dir.display().to_string()
        };

        std::env::set_var("PATH", new_path);
        std::env::remove_var("ANTHROPIC_API_KEY");

        let profile = ProviderRegistry::detect_claude();

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        if let Some(value) = original_api_key {
            std::env::set_var("ANTHROPIC_API_KEY", value);
        }

        assert!(profile.installed);
        assert!(!profile.authenticated);
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "claude-sonnet-4-6"),
            "logged-out Claude Code should still expose help-discovered models"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_codex_reads_models_from_config_toml() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let codex_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let codex_config_dir = temp_home.join(".codex");
        fs::create_dir_all(&codex_dir).expect("failed to create temp codex dir");
        fs::create_dir_all(&codex_config_dir).expect("failed to create temp codex config dir");

        write_executable_script(
            &codex_dir,
            "codex",
            r#"#!/bin/sh
exit 0
"#,
        );

        fs::write(
            codex_config_dir.join("config.toml"),
            r#"
model = "gpt-5.4"

[profiles.fast]
model = "gpt-5.4-mini"

[notice.model_migrations]
"gpt-5.1-codex-mini" = "gpt-5.4"
"#,
        )
        .expect("failed to write codex config");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", codex_dir.display(), existing.to_string_lossy())
        } else {
            codex_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);

        let profile = ProviderRegistry::detect_codex();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert!(profile.installed);
        assert!(profile.runnable);
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gpt-5.4-mini"),
            "Codex should expose profile models declared in config.toml"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gpt-5.4"),
            "Codex should expose the primary configured model"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gpt-5.1-codex-mini"),
            "Codex should preserve migration-linked model ids seen in config"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_codex_reads_models_from_local_cache() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let codex_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let codex_config_dir = temp_home.join(".codex");
        fs::create_dir_all(&codex_dir).expect("failed to create temp codex dir");
        fs::create_dir_all(&codex_config_dir).expect("failed to create temp codex config dir");

        write_executable_script(
            &codex_dir,
            "codex",
            r#"#!/bin/sh
exit 0
"#,
        );

        fs::write(codex_config_dir.join("config.toml"), r#"model = "gpt-5.4""#)
            .expect("failed to write codex config");

        fs::write(
            codex_config_dir.join("models_cache.json"),
            r#"{
  "models": [
    { "slug": "gpt-9-coder-preview" },
    { "slug": "codex-ultra-latest" }
  ]
}"#,
        )
        .expect("failed to write codex models cache");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", codex_dir.display(), existing.to_string_lossy())
        } else {
            codex_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);

        let profile = ProviderRegistry::detect_codex();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gpt-9-coder-preview"),
            "Codex should discover model slugs from the local models cache"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "codex-ultra-latest"),
            "Codex should preserve additional cache-backed model ids"
        );
    }

    #[cfg(unix)]
    #[test]
    fn binary_exists_at_known_locations_finds_gemini_in_nvm_layout() {
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let gemini_dir = temp_root.join(".nvm/versions/node/v24.14.0/bin");
        fs::create_dir_all(&gemini_dir).expect("failed to create temp gemini dir");

        write_executable_script(
            &gemini_dir,
            "gemini",
            r#"#!/bin/sh
exit 0
"#,
        );

        assert!(
            binary_exists_at_known_locations("gemini", &temp_root),
            "gemini should be discoverable in a common NVM layout even when PATH is empty"
        );
    }

    #[cfg(unix)]
    #[test]
    fn which_binary_returns_resolved_path_in_nvm_layout() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let claude_dir = temp_root.join(".nvm/versions/node/v24.14.0/bin");
        fs::create_dir_all(&claude_dir).expect("failed to create temp claude dir");

        write_executable_script(
            &claude_dir,
            "opencode",
            r#"#!/bin/sh
exit 0
"#,
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");

        std::env::set_var("HOME", &temp_root);
        std::env::set_var("PATH", "/usr/bin:/bin");

        let resolved = which_binary("opencode");

        if let Some(value) = original_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(value) = original_path {
            std::env::set_var("PATH", value);
        } else {
            std::env::remove_var("PATH");
        }

        assert_eq!(
            resolved,
            Some(claude_dir.join("opencode")),
            "which_binary should return resolved binary path, not bool"
        );
    }

    #[test]
    fn binary_exists_at_known_locations_finds_windows_global_npm_bins() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let roaming_npm_dir = temp_root.join("AppData/Roaming/npm");
        fs::create_dir_all(&roaming_npm_dir).expect("failed to create roaming npm dir");
        fs::write(roaming_npm_dir.join("claude.cmd"), "@echo off\r\n").expect("failed to seed cmd");

        assert!(
            binary_exists_at_known_locations("claude", &temp_root),
            "claude should be discoverable in the standard Windows global npm directory"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn binary_path_in_dir_prefers_windows_cmd_launcher() {
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let bin_dir = temp_root.join("windows-bin");
        fs::create_dir_all(&bin_dir).expect("failed to create temp windows bin dir");
        fs::write(bin_dir.join("claude"), "sh wrapper").expect("failed to seed bare shim");
        fs::write(bin_dir.join("claude.cmd"), "@echo off\r\n").expect("failed to seed cmd shim");

        assert_eq!(
            binary_path_in_dir(&bin_dir, "claude"),
            Some(bin_dir.join("claude.cmd"))
        );
    }

    #[test]
    fn binary_exists_at_known_locations_uses_custom_appdata_env_paths() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let roaming = temp_root.join("enterprise/roaming");
        let local = temp_root.join("enterprise/local");
        let original_appdata = std::env::var_os("APPDATA");
        let original_local_appdata = std::env::var_os("LOCALAPPDATA");
        fs::create_dir_all(roaming.join("npm")).expect("failed to create roaming npm dir");
        fs::create_dir_all(local.join("npm")).expect("failed to create local npm dir");
        fs::write(roaming.join("npm/opencode.cmd"), "@echo off\r\n")
            .expect("failed to seed roaming binary");

        std::env::set_var("APPDATA", &roaming);
        std::env::set_var("LOCALAPPDATA", &local);

        let found = binary_exists_at_known_locations("opencode", &temp_root.join("home"));

        if let Some(value) = original_appdata {
            std::env::set_var("APPDATA", value);
        } else {
            std::env::remove_var("APPDATA");
        }

        if let Some(value) = original_local_appdata {
            std::env::set_var("LOCALAPPDATA", value);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }

        assert!(
            found,
            "Windows fallback lookup should honor APPDATA/LOCALAPPDATA env values"
        );
    }

    #[test]
    fn cli_environment_overrides_preserve_custom_appdata_values() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let roaming = temp_home.join("enterprise/Roaming");
        let local = temp_home.join("enterprise/Local");
        let original_appdata = std::env::var_os("APPDATA");
        let original_local_appdata = std::env::var_os("LOCALAPPDATA");

        std::env::set_var("APPDATA", &roaming);
        std::env::set_var("LOCALAPPDATA", &local);

        let overrides: HashMap<_, _> = cli_environment_overrides(&temp_home).into_iter().collect();

        if let Some(value) = original_appdata {
            std::env::set_var("APPDATA", value);
        } else {
            std::env::remove_var("APPDATA");
        }

        if let Some(value) = original_local_appdata {
            std::env::set_var("LOCALAPPDATA", value);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }

        assert_eq!(
            overrides.get(&OsString::from("HOME")),
            Some(&temp_home.as_os_str().to_owned())
        );
        assert_eq!(
            overrides.get(&OsString::from("USERPROFILE")),
            Some(&temp_home.as_os_str().to_owned())
        );
        assert_eq!(
            overrides.get(&OsString::from("APPDATA")),
            Some(&roaming.as_os_str().to_owned())
        );
        assert_eq!(
            overrides.get(&OsString::from("LOCALAPPDATA")),
            Some(&local.as_os_str().to_owned())
        );

        let path_override = overrides.get(&OsString::from("PATH"));
        assert!(
            path_override.is_some(),
            "PATH should be propagated to CLI child env"
        );
        #[cfg(unix)]
        {
            assert!(
                std::env::split_paths(path_override.unwrap()).any(|path| {
                    path == temp_home.join(".local/bin")
                        || path == temp_home.join("go/bin")
                        || path == temp_home.join(".npm-global/bin")
                        || path == temp_home.join(".volta/bin")
                }),
                "PATH should include common Unix fallback directories"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn detect_gemini_finds_models_from_nvm_style_installation() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let gemini_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let settings_dir = temp_home.join(".gemini");
        fs::create_dir_all(&gemini_dir).expect("failed to create temp gemini dir");
        fs::create_dir_all(&settings_dir).expect("failed to create temp settings dir");

        write_executable_script(
            &gemini_dir,
            "gemini",
            r#"#!/bin/sh
exit 0
"#,
        );

        fs::write(
            settings_dir.join("settings.json"),
            r#"{"model":{"name":"gemini-3.1-pro-preview"}}"#,
        )
        .expect("failed to write settings file");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", "/usr/bin:/bin");

        let profile = ProviderRegistry::detect_gemini();

        if let Some(value) = original_home {
            std::env::set_var("HOME", value);
        }

        if let Some(value) = original_path {
            std::env::set_var("PATH", value);
        }

        assert!(profile.installed);
        assert!(profile.authenticated);
        assert!(
            !profile.current_models.is_empty(),
            "Gemini CLI should surface native models when installed in an NVM layout"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gemini-3.1-pro-preview"),
            "gemini should expose the configured model from settings"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_gemini_reads_models_from_recent_local_history() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let gemini_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let settings_dir = temp_home.join(".gemini");
        let chats_dir = settings_dir.join("tmp/sample/chats");
        fs::create_dir_all(&gemini_dir).expect("failed to create temp gemini dir");
        fs::create_dir_all(&chats_dir).expect("failed to create temp chat dir");

        write_executable_script(
            &gemini_dir,
            "gemini",
            r#"#!/bin/sh
exit 0
"#,
        );

        fs::write(
            settings_dir.join("settings.json"),
            r#"{"model":{"name":"gemini-3.1-pro-preview"}}"#,
        )
        .expect("failed to write settings file");

        fs::write(
            chats_dir.join("session.json"),
            r#"{
  "messages": [
    { "model": "gemini-9-pro-experimental" },
    { "model": "gemini-9-flash-experimental" }
  ]
}"#,
        )
        .expect("failed to write recent gemini history");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", "/usr/bin:/bin");

        let profile = ProviderRegistry::detect_gemini();

        if let Some(value) = original_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(value) = original_path {
            std::env::set_var("PATH", value);
        } else {
            std::env::remove_var("PATH");
        }

        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gemini-9-pro-experimental"),
            "Gemini should discover model ids from recent local session history"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gemini-9-flash-experimental"),
            "Gemini should surface multiple history-backed model ids"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_claude_reads_models_from_help_and_recent_history() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let claude_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let claude_projects_dir = temp_home.join(".claude/projects/example");
        fs::create_dir_all(&claude_dir).expect("failed to create temp claude dir");
        fs::create_dir_all(&claude_projects_dir).expect("failed to create temp claude project dir");

        write_executable_script(
            &claude_dir,
            "claude",
            r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  printf '%s\n' '{"loggedIn":true,"authMethod":"oauth"}'
elif [ "$1" = "--help" ]; then
  printf '%s\n' "  --model <model>  Use aliases like 'sonnet' and full names like 'claude-sonnet-9-9'"
else
  exit 0
fi
"#,
        );

        fs::write(
            claude_projects_dir.join("session.jsonl"),
            "{\"message\":{\"model\":\"claude-opus-9-1\"}}\n",
        )
        .expect("failed to write recent claude history");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_api_key = std::env::var_os("ANTHROPIC_API_KEY");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", claude_dir.display(), existing.to_string_lossy())
        } else {
            claude_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);
        std::env::remove_var("ANTHROPIC_API_KEY");

        let profile = ProviderRegistry::detect_claude();

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(value) = original_api_key {
            std::env::set_var("ANTHROPIC_API_KEY", value);
        }

        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "claude-sonnet-9-9"),
            "Claude should discover full model names from CLI help output"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "claude-opus-9-1"),
            "Claude should discover full model names from recent local history"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_claude_uses_credentials_file_when_auth_status_fails() {
        let _guard = ENV_LOCK.lock().expect("env lock should be available");
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let claude_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let credentials_dir = temp_home.join(".claude");
        fs::create_dir_all(&claude_dir).expect("failed to create temp claude dir");
        fs::create_dir_all(&credentials_dir).expect("failed to create temp credentials dir");

        write_executable_script(
            &claude_dir,
            "claude",
            r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  exit 1
elif [ "$1" = "--help" ]; then
  printf '%s\n' "  --model <model>  Use aliases like 'sonnet' and full names like 'claude-sonnet-4-7'"
else
  exit 0
fi
"#,
        );

        fs::write(
            credentials_dir.join(".credentials.json"),
            r#"{"accessToken":"test-token"}"#,
        )
        .expect("failed to write claude credentials");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_api_key = std::env::var_os("ANTHROPIC_API_KEY");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", claude_dir.display(), existing.to_string_lossy())
        } else {
            claude_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);
        std::env::remove_var("ANTHROPIC_API_KEY");

        let profile = ProviderRegistry::detect_claude();

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(value) = original_api_key {
            std::env::set_var("ANTHROPIC_API_KEY", value);
        }

        assert!(profile.installed);
        assert!(profile.authenticated);
        assert!(profile.runnable);
        assert_eq!(profile.subscription_label, "subscription-backed");
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "claude-sonnet-4-7"),
            "Claude should still discover models when auth falls back to local credentials"
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_output_timeout_returns_none_for_slow_command() {
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_home).expect("failed to create temp home");
        let script = write_executable_script(
            &temp_home,
            "slow-provider",
            r#"#!/bin/sh
sleep 2
printf '%s\n' 'eventual output'
"#,
        );

        let started = Instant::now();
        let output = capture_command_output_with_timeout(
            &script,
            &["--help"],
            &temp_home,
            Duration::from_millis(100),
        );

        assert!(output.is_none());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "slow provider command should be bounded by timeout"
        );

        let _ = fs::remove_dir_all(temp_home);
    }
}
