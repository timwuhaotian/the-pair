use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::Manager;

/// The @-mention list is for humans to pick from; past this many entries the
/// scan stops instead of walking (and serializing) an entire monorepo.
const MAX_SCAN_ENTRIES: usize = 10_000;
/// Directory nesting depth the scan descends to.
const MAX_SCAN_DEPTH: usize = 16;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    ".worktrees",
    "target",
    "coverage",
    "bower_components",
    "Pods",
    "DerivedData",
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

/// Recursively lists `directory` relative to `base_dir`. Symlinks are listed
/// but never followed (a symlinked directory could loop or leave the
/// workspace), heavy build/dependency directories are skipped, and the walk
/// is bounded by `MAX_SCAN_DEPTH` and `MAX_SCAN_ENTRIES`.
pub fn scan_directory(directory: &Path, base_dir: &Path) -> Result<Vec<FileEntry>, String> {
    let mut results = Vec::new();

    if !directory.exists() {
        return Ok(results);
    }

    let entries =
        fs::read_dir(directory).map_err(|e| format!("Failed to read directory: {}", e))?;
    scan_entries(entries, base_dir, 0, &mut results);

    Ok(results)
}

fn scan_entries(entries: fs::ReadDir, base_dir: &Path, depth: usize, results: &mut Vec<FileEntry>) {
    for entry in entries {
        if results.len() >= MAX_SCAN_ENTRIES {
            return;
        }
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

        // `DirEntry::file_type` does not follow symlinks.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            if should_exclude_dir(&file_name) {
                continue;
            }

            results.push(FileEntry {
                path: relative_path,
                file_type: "directory".to_string(),
            });

            if depth + 1 < MAX_SCAN_DEPTH {
                // An unreadable subdirectory shouldn't fail the whole scan.
                if let Ok(sub_entries) = fs::read_dir(&full_path) {
                    scan_entries(sub_entries, base_dir, depth + 1, results);
                }
            }
        } else if file_type.is_file() {
            results.push(FileEntry {
                path: relative_path,
                file_type: "file".to_string(),
            });
        } else if file_type.is_symlink() {
            match fs::metadata(&full_path) {
                Ok(target) if target.is_file() => results.push(FileEntry {
                    path: relative_path,
                    file_type: "file".to_string(),
                }),
                Ok(target) if target.is_dir() && !should_exclude_dir(&file_name) => {
                    results.push(FileEntry {
                        path: relative_path,
                        file_type: "directory".to_string(),
                    })
                }
                _ => {}
            }
        }
    }
}

/// Lists files with `git ls-files` (tracked plus untracked, honoring
/// `.gitignore` and the other exclude sources) when `directory` is inside a
/// git work tree. Returns `None` when git can't answer, so the caller falls
/// back to walking the filesystem.
fn list_with_git(directory: &Path) -> Option<Vec<FileEntry>> {
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .args([
            "--no-optional-locks",
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;

    let mut reader = BufReader::new(stdout);
    let mut results = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut record = Vec::new();
    let mut truncated = false;

    loop {
        record.clear();
        match reader.read_until(0, &mut record) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        if record.last() == Some(&0) {
            record.pop();
        }
        let Ok(raw_path) = std::str::from_utf8(&record) else {
            continue;
        };
        // Untracked nested repositories are reported as `dir/`.
        let is_dir_entry = raw_path.ends_with('/');
        let path = raw_path.trim_end_matches('/');
        if path.is_empty() {
            continue;
        }

        let components: Vec<&str> = path.split('/').collect();
        let (parents, name) = components.split_at(components.len() - 1);
        let name = name[0];
        if parents.len() >= MAX_SCAN_DEPTH || parents.iter().any(|dir| should_exclude_dir(dir)) {
            continue;
        }
        let excluded = if is_dir_entry {
            should_exclude_dir(name)
        } else {
            should_exclude_file(name)
        };
        if excluded {
            continue;
        }

        let mut prefix = String::new();
        for dir in parents {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(dir);
            if seen.insert(format!("{}/", prefix)) {
                results.push(FileEntry {
                    path: prefix.clone(),
                    file_type: "directory".to_string(),
                });
            }
        }

        let key = if is_dir_entry {
            format!("{}/", path)
        } else {
            path.to_string()
        };
        if seen.insert(key) {
            results.push(FileEntry {
                path: path.to_string(),
                file_type: if is_dir_entry { "directory" } else { "file" }.to_string(),
            });
        }

        if results.len() >= MAX_SCAN_ENTRIES {
            truncated = true;
            break;
        }
    }

    if truncated {
        let _ = child.kill();
    }
    let status = child.wait().ok()?;
    if !truncated && !status.success() {
        return None;
    }
    Some(results)
}

/// Blocking: prefers the git-aware listing, falls back to a bounded walk.
fn list_files_blocking(directory: &Path) -> Result<Vec<FileEntry>, String> {
    match list_with_git(directory) {
        Some(files) if !files.is_empty() => Ok(files),
        _ => scan_directory(directory, directory),
    }
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

    let dir_path = PathBuf::from(&directory);
    if !dir_path.exists() {
        return Err(format!("Directory does not exist: {}", directory));
    }

    tauri::async_runtime::spawn_blocking(move || list_files_blocking(&dir_path))
        .await
        .map_err(|e| format!("Failed to list files: {}", e))?
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

    // A FIFO or device inside the workspace would block the read forever.
    if !metadata.is_file() {
        return Err("Not a regular file".to_string());
    }

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
    use super::{
        list_with_git, resolve_workspace_file_path, scan_directory, FileEntry, FileListOptions,
        FileReadOptions, MAX_SCAN_DEPTH,
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    fn unique_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "the-pair-file-cache-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn paths(entries: &[FileEntry]) -> Vec<String> {
        let mut paths: Vec<String> = entries
            .iter()
            .map(|e| format!("{}:{}", e.file_type, e.path.replace('\\', "/")))
            .collect();
        paths.sort();
        paths
    }

    #[cfg(unix)]
    #[test]
    fn scan_directory_does_not_follow_directory_symlinks() {
        let root = unique_dir("symlinks");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::os::unix::fs::symlink(&root, root.join("loop-a")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("loop-b")).unwrap();
        std::os::unix::fs::symlink(root.join("src/main.rs"), root.join("alias.rs")).unwrap();

        let entries = scan_directory(&root, &root).unwrap();
        assert_eq!(
            paths(&entries),
            vec![
                "directory:loop-a",
                "directory:loop-b",
                "directory:src",
                "file:alias.rs",
                "file:src/main.rs",
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn scan_directory_skips_build_output_and_caps_depth() {
        let root = unique_dir("heavy");
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/app"), "bin").unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(root.join("node_modules/pkg/index.js"), "").unwrap();

        let mut deep = root.clone();
        for level in 0..(MAX_SCAN_DEPTH + 4) {
            deep = deep.join(format!("d{}", level));
        }
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("too-deep.txt"), "").unwrap();

        let entries = scan_directory(&root, &root).unwrap();
        assert!(entries.iter().all(|e| !e.path.contains("target")));
        assert!(entries.iter().all(|e| !e.path.contains("node_modules")));
        assert!(entries.iter().all(|e| !e.path.ends_with("too-deep.txt")));
        assert_eq!(entries.len(), MAX_SCAN_DEPTH);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn list_with_git_respects_gitignore() {
        let root = unique_dir("git");
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .output()
                .unwrap();
            assert!(status.status.success());
        };
        git(&["init", "-q"]);
        fs::write(root.join(".gitignore"), "generated/\n").unwrap();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/nested/lib.rs"), "").unwrap();
        fs::create_dir_all(root.join("generated")).unwrap();
        fs::write(root.join("generated/out.txt"), "").unwrap();
        fs::write(root.join("notes with space.md"), "").unwrap();
        git(&["add", "src"]);

        let entries = list_with_git(&root).expect("git listing");
        assert_eq!(
            paths(&entries),
            vec![
                "directory:src",
                "directory:src/nested",
                "file:.gitignore",
                "file:notes with space.md",
                "file:src/nested/lib.rs",
            ]
        );

        let outside = unique_dir("not-a-repo");
        assert!(list_with_git(&outside).is_none());
        fs::remove_dir_all(&outside).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

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
