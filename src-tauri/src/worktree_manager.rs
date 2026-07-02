use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    #[serde(rename = "isLocal")]
    pub is_local: bool,
    #[serde(rename = "isRemote")]
    pub is_remote: bool,
    #[serde(rename = "lastCommitSha")]
    pub last_commit_sha: Option<String>,
    #[serde(rename = "lastCommitMessage")]
    pub last_commit_message: Option<String>,
    #[serde(rename = "lastCommitDate")]
    pub last_commit_date: Option<u64>,
    #[serde(rename = "isCheckedOutLocally")]
    pub is_checked_out_locally: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoState {
    #[serde(rename = "isGitRepo")]
    pub is_git_repo: bool,
    #[serde(rename = "isDirty")]
    pub is_dirty: bool,
    #[serde(rename = "currentBranch")]
    pub current_branch: Option<String>,
    pub branches: Vec<BranchInfo>,
}

fn run_git_command(directory: &str, args: &[&str]) -> Result<String, String> {
    println!(
        "[worktree_manager] run_git_command: dir={}, args={:?}",
        directory, args
    );
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output();

    match output {
        Ok(o) => {
            println!(
                "[worktree_manager] git command success: {}",
                o.status.success()
            );
            if o.status.success() {
                let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                println!("[worktree_manager] stdout: {}", stdout);
                Ok(stdout)
            } else {
                let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                println!("[worktree_manager] stderr: {}", stderr);
                Err(stderr)
            }
        }
        Err(e) => {
            println!("[worktree_manager] git command failed to execute: {}", e);
            Err(format!("Failed to run git: {}", e))
        }
    }
}

pub fn check_is_git_repo(directory: &str) -> bool {
    let result = run_git_command(directory, &["rev-parse", "--is-inside-work-tree"]);
    println!("[worktree_manager] check_is_git_repo result: {:?}", result);
    result.is_ok()
}

pub fn check_is_dirty(directory: &str) -> bool {
    let output = run_git_command(directory, &["status", "--porcelain"]);
    match output {
        Ok(s) => !s.is_empty(),
        Err(_) => false,
    }
}

pub fn get_current_branch(directory: &str) -> Option<String> {
    run_git_command(directory, &["branch", "--show-current"]).ok()
}

pub fn list_branches(directory: &str) -> Result<Vec<BranchInfo>, String> {
    let local_output = run_git_command(
        directory,
        &[
            "branch",
            "--format=%(refname:short)|%(objectname:short)|%(subject)|%(committerdate:unix)",
        ],
    );

    let remote_output = run_git_command(
        directory,
        &[
            "branch",
            "-r",
            "--format=%(refname:short)|%(objectname:short)|%(subject)|%(committerdate:unix)",
        ],
    );

    let local_branch_names: Vec<String> = match &local_output {
        Ok(s) => s
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.split('|').next().unwrap_or("").trim().to_string())
            .collect(),
        Err(_) => vec![],
    };

    let mut branches: Vec<BranchInfo> = Vec::new();

    let current_branch = get_current_branch(directory);

    if let Ok(s) = local_output {
        for line in s.lines() {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            if parts.is_empty() {
                continue;
            }
            let name = parts[0].trim().to_string();
            let sha = parts.get(1).map(|s| s.trim().to_string());
            let message = parts.get(2).map(|s| s.trim().to_string());
            let date = parts.get(3).and_then(|s| s.trim().parse::<u64>().ok());
            let is_checked_out = current_branch.as_deref() == Some(name.as_str());

            branches.push(BranchInfo {
                name,
                is_local: true,
                is_remote: false,
                last_commit_sha: sha,
                last_commit_message: message,
                last_commit_date: date,
                is_checked_out_locally: is_checked_out,
            });
        }
    }

    if let Ok(s) = remote_output {
        for line in s.lines() {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            if parts.is_empty() {
                continue;
            }
            let name = parts[0].trim().to_string();
            if name.ends_with("/HEAD") {
                continue;
            }

            let local_name = if name.contains('/') {
                name.split('/').skip(1).collect::<Vec<&str>>().join("/")
            } else {
                name.to_string()
            };
            let is_checked_out_locally = local_branch_names.contains(&local_name);

            let sha = parts.get(1).map(|s| s.trim().to_string());
            let message = parts.get(2).map(|s| s.trim().to_string());
            let date = parts.get(3).and_then(|s| s.trim().parse::<u64>().ok());

            branches.push(BranchInfo {
                name,
                is_local: false,
                is_remote: true,
                last_commit_sha: sha,
                last_commit_message: message,
                last_commit_date: date,
                is_checked_out_locally,
            });
        }
    }

    Ok(branches)
}

pub fn check_repo_state(directory: &str) -> RepoState {
    println!(
        "[worktree_manager] check_repo_state called for: {}",
        directory
    );
    let is_git_repo = check_is_git_repo(directory);
    println!("[worktree_manager] is_git_repo: {}", is_git_repo);
    if !is_git_repo {
        println!("[worktree_manager] Not a git repo, returning empty state");
        return RepoState {
            is_git_repo: false,
            is_dirty: false,
            current_branch: None,
            branches: vec![],
        };
    }

    let is_dirty = check_is_dirty(directory);
    let current_branch = get_current_branch(directory);
    let branches = list_branches(directory).unwrap_or_default();
    println!(
        "[worktree_manager] is_dirty: {}, current_branch: {:?}, branches: {}",
        is_dirty,
        current_branch,
        branches.len()
    );
    RepoState {
        is_git_repo: true,
        is_dirty,
        current_branch,
        branches,
    }
}

pub fn create_worktree(
    repo_path: &str,
    branch: &str,
    worktree_path: &str,
) -> Result<String, String> {
    let worktrees_dir = Path::new(repo_path).join(".worktrees");
    if !worktrees_dir.exists() {
        std::fs::create_dir_all(&worktrees_dir)
            .map_err(|e| format!("Failed to create .worktrees directory: {}", e))?;
    }

    let full_worktree_path = Path::new(repo_path).join(worktree_path);

    let path_str = full_worktree_path.to_str().ok_or_else(|| {
        format!(
            "Worktree path contains non-UTF-8 characters: {:?}",
            full_worktree_path
        )
    })?;

    let result = run_git_command(
        repo_path,
        &["worktree", "add", "--detach", path_str, branch],
    )
    .or_else(|_| run_git_command(repo_path, &["worktree", "add", path_str, branch]));

    match result {
        Ok(_) => Ok(full_worktree_path.to_string_lossy().to_string()),
        Err(e) => Err(format!("Failed to create worktree: {}", e)),
    }
}

pub fn delete_worktree(worktree_path: &str) -> Result<(), String> {
    let parent_repo = find_repo_root(worktree_path);

    let run_from = parent_repo.as_deref().unwrap_or(worktree_path);

    let result = run_git_command(run_from, &["worktree", "remove", "--force", worktree_path]);

    let dir_still_exists = Path::new(worktree_path).exists();

    if dir_still_exists {
        if let Err(fs_err) = std::fs::remove_dir_all(worktree_path) {
            if result.is_err() {
                if let Err(git_err) = &result {
                    return Err(format!(
                        "Git worktree remove failed ({}) and directory cleanup also failed ({})",
                        git_err, fs_err
                    ));
                }
            }
            return Err(format!("Failed to remove worktree directory: {}", fs_err));
        }
    }

    if let Some(repo) = &parent_repo {
        let _ = run_git_command(repo, &["worktree", "prune"]);
    }

    Ok(())
}

fn find_repo_root(worktree_path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(worktree_path)
        .output()
        .ok();

    match output {
        Some(o) if o.status.success() => {
            let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if path.is_empty() {
                None
            } else {
                Some(path)
            }
        }
        _ => {
            let mut candidate = Path::new(worktree_path);
            for _ in 0..32 {
                if let Some(parent) = candidate.parent() {
                    if parent.join(".git").exists() || parent.join(".git").is_file() {
                        return Some(parent.to_string_lossy().to_string());
                    }
                    candidate = parent;
                } else {
                    break;
                }
            }
            None
        }
    }
}

pub fn ensure_local_tracking_branch(
    repo_path: &str,
    remote_branch: &str,
) -> Result<String, String> {
    let (local_name, remote_ref) = if remote_branch.contains('/') {
        let mut parts = remote_branch.splitn(2, '/');
        let remote = parts.next().unwrap_or("origin");
        let rest = parts.next().unwrap_or(remote_branch);
        if rest.is_empty() {
            (
                remote_branch.to_string(),
                format!("{}/{}", remote, remote_branch),
            )
        } else {
            (rest.to_string(), remote_branch.to_string())
        }
    } else {
        let fallback = format!("origin/{}", remote_branch);
        (remote_branch.to_string(), fallback)
    };

    let check_result = run_git_command(repo_path, &["branch", "--list", &local_name]);
    if let Ok(s) = check_result {
        if !s.is_empty() {
            return Ok(local_name.to_string());
        }
    }

    let _ = run_git_command(repo_path, &["fetch", "--all"]);

    let result = run_git_command(repo_path, &["branch", "--track", &local_name, &remote_ref]);

    match result {
        Ok(_) => Ok(local_name.to_string()),
        Err(e) => Err(format!("Failed to create local tracking branch: {}", e)),
    }
}

/// Atomic write for plain-text files: write to a temp sibling then rename.
/// Mirrors the `write_json_atomic` pattern from `session_snapshot.rs`.
fn write_text_atomic(path: &Path, content: &str) -> Result<(), String> {
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, content).map_err(|e| format!("Failed to write file: {}", e))?;
    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("Failed to move file into place: {}", e))
}

pub fn ensure_gitignore_worktrees(repo_path: &str) -> Result<bool, String> {
    let exclude_path = Path::new(repo_path)
        .join(".git")
        .join("info")
        .join("exclude");
    let gitignore_path = Path::new(repo_path).join(".gitignore");

    let entry = ".worktrees/";

    if exclude_path.exists() {
        let content = std::fs::read_to_string(&exclude_path)
            .map_err(|e| format!("Failed to read .git/info/exclude: {}", e))?;
        if content.contains(entry) {
            return Ok(false);
        }
        let new_content = if content.ends_with('\n') {
            format!("{}{}\n", content, entry)
        } else {
            format!("{}\n{}\n", content, entry)
        };
        write_text_atomic(&exclude_path, &new_content)
            .map_err(|e| format!("Failed to update .git/info/exclude: {}", e))?;
        return Ok(true);
    }

    if let Some(parent) = exclude_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create .git/info directory: {}", e))?;
    }
    write_text_atomic(&exclude_path, &format!("{}\n", entry))
        .map_err(|e| format!("Failed to write .git/info/exclude: {}", e))?;

    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)
            .map_err(|e| format!("Failed to read .gitignore: {}", e))?;
        if content.contains(entry) {
            return Ok(true);
        }
    }

    Ok(true)
}
