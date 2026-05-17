use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub file_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileListOptions {
    pub pair_id: Option<String>,
    pub directory: Option<String>,
}

const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "__pycache__",
    ".venv",
    "venv",
    ".cache",
    ".parcel-cache",
    ".turbo",
    ".pair",
    ".opencode",
];

const EXCLUDED_FILES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "*.log",
];

pub fn should_exclude_dir(name: &str) -> bool {
    EXCLUDED_DIRS.contains(&name) || name.starts_with('.')
}

pub fn should_exclude_file(name: &str) -> bool {
    EXCLUDED_FILES.contains(&name) || name.ends_with(".log")
}

pub fn scan_directory(directory: &Path, base_dir: &Path) -> Result<Vec<FileEntry>, String> {
    let mut results = Vec::new();

    if !directory.exists() {
        return Ok(results);
    }

    let entries =
        fs::read_dir(directory).map_err(|e| format!("Failed to read directory: {}", e))?;

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let file_name = match entry.file_name().to_str() {
            Some(name) => name.to_string(),
            None => continue,
        };

        if should_exclude_file(&file_name) {
            continue;
        }

        let full_path = entry.path();
        let relative_path = match full_path.strip_prefix(base_dir) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        if full_path.is_dir() {
            if should_exclude_dir(&file_name) {
                continue;
            }

            results.push(FileEntry {
                path: relative_path.clone(),
                file_type: "directory".to_string(),
            });

            let sub_files = scan_directory(&full_path, base_dir)?;
            results.extend(sub_files);
        } else if full_path.is_file() {
            results.push(FileEntry {
                path: relative_path,
                file_type: "file".to_string(),
            });
        }
    }

    Ok(results)
}

#[tauri::command]
pub async fn file_list_files(
    app: tauri::AppHandle,
    options: FileListOptions,
) -> Result<Vec<FileEntry>, String> {
    let directory = if let Some(pair_id) = options.pair_id {
        let pair_manager = app.state::<std::sync::Mutex<crate::pair_manager::PairManager>>();
        let manager = pair_manager.lock().map_err(|e| e.to_string())?;
        let pairs = manager.list_pairs();
        pairs
            .iter()
            .find(|p| p.pair_id == pair_id)
            .map(|p| p.directory.clone())
            .ok_or_else(|| format!("Pair {} not found", pair_id))?
    } else if let Some(dir) = options.directory {
        dir
    } else {
        return Err("Either pair_id or directory must be provided".to_string());
    };

    let dir_path = Path::new(&directory);
    if !dir_path.exists() {
        return Err(format!("Directory does not exist: {}", directory));
    }

    scan_directory(dir_path, dir_path)
}

#[tauri::command]
pub async fn file_parse_mentions(
    _app: tauri::AppHandle,
    _pair_id: String,
    spec: String,
) -> Result<String, String> {
    Ok(spec)
}

const MAX_FILE_SIZE: u64 = 100 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReadOptions {
    pub pair_id: Option<String>,
    pub directory: Option<String>,
    pub file_path: String,
}

fn resolve_directory(
    app: &tauri::AppHandle,
    pair_id: Option<String>,
    directory: Option<String>,
) -> Result<String, String> {
    if let Some(pair_id) = pair_id {
        let pair_manager = app.state::<std::sync::Mutex<crate::pair_manager::PairManager>>();
        let manager = pair_manager.lock().map_err(|e| e.to_string())?;
        let pairs = manager.list_pairs();
        pairs
            .iter()
            .find(|p| p.pair_id == pair_id)
            .map(|p| p.directory.clone())
            .ok_or_else(|| format!("Pair {} not found", pair_id))
    } else if let Some(dir) = directory {
        Ok(dir)
    } else {
        Err("Either pair_id or directory must be provided".to_string())
    }
}

fn resolve_workspace_file_path(directory: &Path, file_path: &Path) -> Result<PathBuf, String> {
    let canonical_directory = directory
        .canonicalize()
        .map_err(|e| format!("Failed to resolve workspace directory: {}", e))?;
    let candidate = canonical_directory.join(file_path);
    let canonical_candidate = candidate
        .canonicalize()
        .map_err(|e| format!("Failed to resolve file path: {}", e))?;

    if !canonical_candidate.starts_with(&canonical_directory) {
        return Err("File path escapes the workspace directory".to_string());
    }

    Ok(canonical_candidate)
}

#[tauri::command]
pub async fn file_read_content(
    app: tauri::AppHandle,
    options: FileReadOptions,
) -> Result<String, String> {
    let directory = resolve_directory(&app, options.pair_id, options.directory)?;

    let full_path =
        resolve_workspace_file_path(Path::new(&directory), Path::new(&options.file_path))?;
    let metadata =
        fs::metadata(&full_path).map_err(|e| format!("Failed to read file metadata: {}", e))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "File too large ({} bytes). Maximum size is {} bytes.",
            metadata.len(),
            MAX_FILE_SIZE
        ));
    }

    let content =
        fs::read_to_string(&full_path).map_err(|e| format!("Failed to read file: {}", e))?;

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::{resolve_workspace_file_path, FileListOptions, FileReadOptions};
    use std::fs;
    use std::path::Path;

    #[test]
    fn file_list_options_deserializes_camelcase_keys() {
        let json = r#"{"pairId":"abc","directory":"/tmp/proj"}"#;
        let parsed: FileListOptions = serde_json::from_str(json).expect("camelCase parse");
        assert_eq!(parsed.pair_id.as_deref(), Some("abc"));
        assert_eq!(parsed.directory.as_deref(), Some("/tmp/proj"));
    }

    #[test]
    fn file_read_options_deserializes_camelcase_keys() {
        let json = r#"{"pairId":"abc","filePath":"src/main.rs"}"#;
        let parsed: FileReadOptions = serde_json::from_str(json).expect("camelCase parse");
        assert_eq!(parsed.pair_id.as_deref(), Some("abc"));
        assert_eq!(parsed.directory, None);
        assert_eq!(parsed.file_path, "src/main.rs");
    }

    #[test]
    fn resolve_workspace_file_path_keeps_files_inside_the_workspace() {
        let root =
            std::env::temp_dir().join(format!("the-pair-file-cache-test-{}", std::process::id()));
        let nested_dir = root.join("src");
        let file_path = nested_dir.join("main.rs");

        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(&file_path, "fn main() {}").unwrap();

        let resolved = resolve_workspace_file_path(Path::new(&root), Path::new("src/main.rs"))
            .expect("path should resolve inside the workspace");
        let canonical_root = root.canonicalize().unwrap();

        assert!(resolved.starts_with(&canonical_root));

        fs::remove_file(&file_path).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resolve_workspace_file_path_rejects_escape_attempts() {
        let root = std::env::temp_dir().join(format!(
            "the-pair-file-cache-test-{}-escape",
            std::process::id()
        ));
        let nested_dir = root.join("src");
        let outside_file = std::env::temp_dir().join(format!(
            "outside-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(&outside_file, "outside").unwrap();

        let result = resolve_workspace_file_path(Path::new(&root), Path::new("../outside.txt"));
        assert!(result.is_err());

        let _ = fs::remove_file(&outside_file);
        fs::remove_dir_all(&root).unwrap();
    }
}
