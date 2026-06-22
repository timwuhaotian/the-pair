use crate::types::{FileStatus, ModifiedFile, PairState};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Parse a single `git status --porcelain` line into a [`ModifiedFile`].
///
/// Returns `None` for lines too short to hold a 2-char status, a separator and
/// a path. Renames and copies are reported by git as `XY old -> new`; we record
/// the destination path so that `git diff HEAD -- <path>` resolves the live file
/// instead of the literal `old -> new` string.
fn parse_porcelain_line(line: &str) -> Option<ModifiedFile> {
    if line.len() <= 3 {
        return None;
    }
    let status_str = &line[0..2];
    let raw_path = &line[3..];

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

    // For rename/copy entries git emits "old -> new"; the destination is the
    // path that exists on disk and that diffing must target.
    let path = match raw_path.split_once(" -> ") {
        Some((_old, new)) => new,
        None => raw_path,
    };

    Some(ModifiedFile {
        path: path.to_string(),
        status,
        display_path: path.to_string(),
    })
}

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
                let files = stdout.lines().filter_map(parse_porcelain_line).collect();
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
        } else {
            // Tracked changes (including deletions) diff against HEAD.
            Command::new("git")
                .args(["diff", "HEAD", "--", file_path])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modified_file() {
        let file = parse_porcelain_line(" M src/lib.rs").expect("should parse");
        assert_eq!(file.path, "src/lib.rs");
        assert_eq!(file.display_path, "src/lib.rs");
        assert!(matches!(file.status, FileStatus::M));
    }

    #[test]
    fn parses_untracked_file() {
        let file = parse_porcelain_line("?? new.txt").expect("should parse");
        assert_eq!(file.path, "new.txt");
        assert!(matches!(file.status, FileStatus::Untracked));
    }

    #[test]
    fn parses_added_and_deleted() {
        let added = parse_porcelain_line("A  added.rs").expect("should parse");
        assert_eq!(added.path, "added.rs");
        assert!(matches!(added.status, FileStatus::A));

        let deleted = parse_porcelain_line(" D gone.rs").expect("should parse");
        assert_eq!(deleted.path, "gone.rs");
        assert!(matches!(deleted.status, FileStatus::D));
    }

    #[test]
    fn rename_uses_destination_path() {
        // Renames are emitted as "old -> new"; we must record the live destination
        // so `git diff HEAD -- <path>` resolves rather than failing on a phantom path.
        let file = parse_porcelain_line("R  old/name.rs -> new/name.rs").expect("should parse");
        assert_eq!(file.path, "new/name.rs");
        assert_eq!(file.display_path, "new/name.rs");
        assert!(matches!(file.status, FileStatus::R));
    }

    #[test]
    fn skips_lines_too_short_to_hold_a_path() {
        assert!(parse_porcelain_line("").is_none());
        assert!(parse_porcelain_line(" M ").is_none());
    }
}
