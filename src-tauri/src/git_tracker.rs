use crate::types::{FileStatus, ModifiedFile, PairState};
use std::fs;
use std::path::Path;
use std::process::Command;

pub struct GitTracker;

impl GitTracker {
    pub fn update_state(state: &mut PairState) {
        let output = Command::new("git")
            .arg("status")
            .arg("--porcelain")
            .current_dir(&state.directory)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                state.git_tracking.available = true;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut files = Vec::new();
                for line in stdout.lines() {
                    if line.len() > 3 {
                        let status_str = &line[0..2];
                        let path = &line[3..];

                        let status = if status_str.starts_with('?') {
                            FileStatus::Untracked
                        } else if status_str.contains('M') {
                            FileStatus::M
                        } else if status_str.contains('A') {
                            FileStatus::A
                        } else if status_str.contains('D') {
                            FileStatus::D
                        } else if status_str.contains('R') {
                            FileStatus::R
                        } else {
                            FileStatus::Untracked
                        };

                        files.push(ModifiedFile {
                            path: path.to_string(),
                            status,
                            display_path: path.to_string(),
                        });
                    }
                }
                state.modified_files = files;
            } else {
                println!(
                    "[GitTracker] Git status command failed in directory: {}",
                    state.directory
                );
                state.git_tracking.available = false;
            }
        } else {
            state.git_tracking.available = false;
        }
    }

    pub fn get_file_diff(directory: &str, file_path: &str, status: &str) -> Result<String, String> {
        let max_lines = 500;
        let full_path = Path::new(directory).join(file_path);

        if full_path.exists() {
            if let Ok(content) = fs::read(&full_path) {
                if content.iter().take(8000).any(|&b| b == 0) {
                    return Err("Binary file — cannot display diff".to_string());
                }
            }
        }

        let output = if status == "??" {
            match fs::read_to_string(&full_path) {
                Ok(content) => {
                    let lines: Vec<&str> = content.lines().collect();
                    let truncated = if lines.len() > max_lines {
                        lines[..max_lines].join("\n") + "\n\n... (truncated)"
                    } else {
                        content
                    };
                    return Ok(format!("--- /dev/null\n+++ b/{}\n{}", file_path, truncated));
                }
                Err(e) => return Err(format!("Cannot read file: {}", e)),
            }
        } else if status.contains('D') {
            Command::new("git")
                .args(["diff", "HEAD", "--", file_path])
                .current_dir(directory)
                .output()
                .map_err(|e| format!("git diff failed: {}", e))?
        } else {
            Command::new("git")
                .args(["diff", "--", file_path])
                .current_dir(directory)
                .output()
                .map_err(|e| format!("git diff failed: {}", e))?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git diff failed: {}", stderr.trim()));
        }

        let diff = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = diff.lines().collect();
        let truncated = if lines.len() > max_lines {
            lines[..max_lines].join("\n") + "\n\n... (truncated)"
        } else {
            diff.to_string()
        };

        if truncated.trim().is_empty() {
            Ok("No changes to display".to_string())
        } else {
            Ok(truncated)
        }
    }
}
