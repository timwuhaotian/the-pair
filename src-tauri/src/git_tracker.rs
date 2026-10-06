use crate::types::{FileStatus, ModifiedFile, PairState};
use std::fs;
use std::io::Read;
use std::path::{Component, Path};
use std::process::{Command, Stdio};

/// Upper bound on entries kept from one `git status` poll, so an unignored
/// build directory can't flood the pair state (and every `pair:state` event).
const MAX_MODIFIED_FILES: usize = 5_000;
/// Untracked files are shown in full up to this many bytes.
const MAX_UNTRACKED_READ_BYTES: u64 = 512 * 1024;
const MAX_DIFF_LINES: usize = 500;
/// Exclude pathspecs passed to one `git status` poll. They only save git the
/// walk; `parse_porcelain_z` drops the same entries anyway.
const MAX_STATUS_EXCLUDES: usize = 100;

/// Dependency and build-output directories that tools recreate on demand
/// (`npm install`, `pip install`, `cargo build`, `pod install`, ...). An entry
/// with a `/` names a directory by its trailing path components.
///
/// Only *wholly untracked* instances count: git reports such a directory as a
/// single `?? dir/` entry with `--untracked-files=normal`, which guarantees
/// nothing inside it is tracked. A `build/` directory holding tracked sources
/// is left alone. Such directories are kept out of the modified-file list and
/// out of the auto-save stash of a deleted pair worktree, so one unignored
/// `node_modules` can neither flood the list nor bloat the repository.
pub(crate) const REGENERABLE_DIRS: &[&str] = &[
    "node_modules",
    "bower_components",
    ".pnpm-store",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    "target",
    "dist",
    "build",
    "coverage",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".gradle",
    "Pods",
    "DerivedData",
    ".dart_tool",
    "vendor/bundle",
    // Aider's repo-map cache, written into the repo root on every run
    // (aider-chat 0.86.2) with no flag to move it.
    ".aider.tags.cache.v4",
];

/// True when the repo-relative directory `dir` (`/`-separated, an optional
/// trailing `/` is ignored) is named like a regenerable directory.
pub(crate) fn is_regenerable_dir(dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    REGENERABLE_DIRS.iter().any(|name| {
        dir == *name
            || dir
                .strip_suffix(name)
                .is_some_and(|parent| parent.ends_with('/'))
    })
}

/// `path` equals `dir` or lies below it.
fn is_within(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The shallowest regenerable directory containing `path` (or equal to it,
/// for a `dir/` entry) that lies inside one of `untracked_dirs` — directories
/// git reported as wholly untracked, given without the trailing `/`.
pub(crate) fn regenerable_ancestor<'a>(
    path: &'a str,
    untracked_dirs: &[String],
) -> Option<&'a str> {
    let trimmed = path.trim_end_matches('/');
    let own_end = (path.len() != trimmed.len()).then_some(trimmed.len());
    trimmed
        .match_indices('/')
        .map(|(index, _)| index)
        .chain(own_end)
        .map(|end| &trimmed[..end])
        .find(|prefix| {
            is_regenerable_dir(prefix) && untracked_dirs.iter().any(|dir| is_within(prefix, dir))
        })
}

/// Pathspec that leaves `dir` (repo-relative) out of a git command, whatever
/// subdirectory it runs in and whatever characters the name contains.
pub(crate) fn exclude_pathspec(dir: &str) -> String {
    format!(":(top,exclude,literal){}", dir)
}

/// Wholly untracked directories (`?? dir/` records, trailing `/` removed) in
/// `git status --porcelain=v1 -z --untracked-files=normal` output.
fn untracked_dirs_in_porcelain_z(output: &[u8]) -> Vec<String> {
    let mut dirs = Vec::new();
    let mut records = output.split(|&byte| byte == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            continue;
        }
        if matches!(record[0], b'R' | b'C') || matches!(record[1], b'R' | b'C') {
            let _original_path = records.next();
            continue;
        }
        if record.starts_with(b"?? ") && record.ends_with(b"/") {
            let path = String::from_utf8_lossy(&record[3..]);
            dirs.push(path.trim_end_matches('/').to_string());
        }
    }
    dirs
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `git` with `--no-optional-locks`: these are read-only polls and must never
/// take `index.lock` away from an agent's concurrent `git add` / `git commit`.
fn git_command() -> Command {
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.arg("--no-optional-locks");
    command
}

/// Maps a porcelain v1 `XY` status pair onto the statuses the UI knows.
/// Unmerged entries count as modified (both-deleted as deleted), copies as
/// added, type changes as modified.
fn classify_status(x: u8, y: u8) -> Option<FileStatus> {
    match (x, y) {
        (b'?', b'?') => Some(FileStatus::Untracked),
        (b'!', b'!') => None,
        (b'D', b'D') => Some(FileStatus::D),
        (b'U', _) | (_, b'U') | (b'A', b'A') => Some(FileStatus::M),
        _ if x == b'R' || y == b'R' => Some(FileStatus::R),
        _ if x == b'C' || y == b'C' => Some(FileStatus::A),
        _ if x == b'A' || y == b'A' => Some(FileStatus::A),
        _ if x == b'D' || y == b'D' => Some(FileStatus::D),
        _ if matches!(x, b'M' | b'T') || matches!(y, b'M' | b'T') => Some(FileStatus::M),
        _ => None,
    }
}

/// Parses `git status --porcelain=v1 -z` output. Records are NUL-terminated
/// and paths are raw (no quoting), so spaces and non-ASCII names survive.
/// Renames and copies are followed by an extra NUL-terminated field holding
/// the original path; the live destination path is the one recorded.
///
/// Untracked entries inside a regenerable directory that lies in one of
/// `untracked_dirs` (see `regenerable_ancestor`) are dropped before the cap.
fn parse_porcelain_z(output: &[u8], untracked_dirs: &[String]) -> Vec<ModifiedFile> {
    let mut files = Vec::new();
    let mut records = output.split(|&byte| byte == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            continue;
        }
        let (x, y) = (record[0], record[1]);
        if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            let _original_path = records.next();
        }
        let Some(status) = classify_status(x, y) else {
            continue;
        };
        let path = String::from_utf8_lossy(&record[3..]).into_owned();
        if matches!(status, FileStatus::Untracked)
            && regenerable_ancestor(&path, untracked_dirs).is_some()
        {
            continue;
        }
        files.push(ModifiedFile {
            display_path: path.clone(),
            path,
            status,
        });
        if files.len() >= MAX_MODIFIED_FILES {
            break;
        }
    }
    files
}

/// Lexically validates a repo-relative path: no absolute paths, drive
/// prefixes or `..` components. Works for files that no longer exist.
fn validate_relative_path(file_path: &str) -> Result<&Path, String> {
    let path = Path::new(file_path);
    if file_path.is_empty() {
        return Err("File path is empty".to_string());
    }
    let only_normal = path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if !only_normal {
        return Err("File path escapes the workspace directory".to_string());
    }
    Ok(path)
}

fn truncate_lines(content: &str, max_lines: usize, force_marker: bool) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > max_lines {
        lines[..max_lines].join("\n") + "\n\n... (truncated)"
    } else if force_marker {
        format!("{}\n\n... (truncated)", content.trim_end())
    } else {
        content.to_string()
    }
}

/// `HEAD`, or the empty tree when the repository has no commits yet, so
/// staged files in a brand-new repo still diff.
pub(crate) fn diff_base(directory: &str) -> String {
    let has_head = git_command()
        .args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
        .current_dir(directory)
        .stdin(Stdio::null())
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if has_head {
        return "HEAD".to_string();
    }
    git_command()
        .args(["hash-object", "-t", "tree", "--stdin"])
        .current_dir(directory)
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| "HEAD".to_string())
}

fn read_untracked_file(
    canonical_directory: &Path,
    full_path: &Path,
    file_path: &str,
) -> Result<String, String> {
    let canonical = full_path
        .canonicalize()
        .map_err(|e| format!("Failed to resolve file path: {}", e))?;
    if !canonical.starts_with(canonical_directory) {
        return Err("File path escapes the workspace directory".to_string());
    }
    let metadata = fs::metadata(&canonical).map_err(|e| format!("Cannot read file: {}", e))?;
    if metadata.is_dir() {
        return Err("Untracked directory — cannot display diff".to_string());
    }
    // FIFOs, sockets and devices would block or never end.
    if !metadata.is_file() {
        return Err("Not a regular file — cannot display diff".to_string());
    }

    let mut bytes = Vec::new();
    fs::File::open(&canonical)
        .and_then(|file| file.take(MAX_UNTRACKED_READ_BYTES).read_to_end(&mut bytes))
        .map_err(|e| format!("Cannot read file: {}", e))?;
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return Err("Binary file — cannot display diff".to_string());
    }
    let content = String::from_utf8_lossy(&bytes);
    let truncated = truncate_lines(
        &content,
        MAX_DIFF_LINES,
        metadata.len() > MAX_UNTRACKED_READ_BYTES,
    );
    Ok(format!("--- /dev/null\n+++ b/{}\n{}", file_path, truncated))
}

pub struct GitTracker;

/// Outcome of committing a pair workspace's tracked changes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommitOutcome {
    pub sha: String,
    pub files_committed: usize,
}

/// Outcome of pushing a pair workspace's branch to its remote.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushOutcome {
    pub branch: String,
    pub remote: String,
    pub up_to_date: bool,
}

/// Upper bound on a commit message: long agent summaries stay intact, but an
/// unbounded `-m` argument could blow past OS arg limits.
const MAX_COMMIT_MESSAGE_CHARS: usize = 10_000;

/// Stages the files `collect_modified_files` reports (minus regenerable
/// untracked directories, which are excluded the same way the tracker polls)
/// and commits them with `message`. Returns the new short SHA.
pub fn commit_changes(directory: &str, message: &str) -> Result<CommitOutcome, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("Commit message is empty".to_string());
    }
    if message.chars().count() > MAX_COMMIT_MESSAGE_CHARS {
        return Err(format!(
            "Commit message exceeds {MAX_COMMIT_MESSAGE_CHARS} characters"
        ));
    }
    let files = GitTracker::collect_modified_files(directory)
        .ok_or_else(|| "Not a git repository".to_string())?;
    if files.is_empty() {
        return Err("Nothing to commit".to_string());
    }

    let collapsed = collapsed_status(directory)?;
    let untracked_dirs = untracked_dirs_in_porcelain_z(&collapsed);
    // No cap here, unlike the poll: `git add -A` has no second filter, so
    // every regenerable dir must be excluded or it gets committed.
    let excludes: Vec<String> = untracked_dirs
        .iter()
        .filter(|dir| is_regenerable_dir(dir))
        .map(|dir| exclude_pathspec(dir))
        .collect();

    run_git(directory, &["add", "-A"], &excludes, "git add failed")?;
    // The staged set is exactly the reported files: regenerable directories
    // were already filtered out of that list, so the excludes only keep git
    // from walking them.
    let files_committed = files.len();

    // Hooks stay enabled: user-side checks (lint, tests) must still run.
    run_git(
        directory,
        &["commit", "-m", message],
        &[],
        "git commit failed",
    )?;

    let sha = run_git(directory, &["rev-parse", "--short", "HEAD"], &[], "git rev-parse failed")?;
    Ok(CommitOutcome {
        sha: sha.trim().to_string(),
        files_committed,
    })
}

/// Pushes the workspace's current branch to `remote` (default `origin`),
/// setting upstream when the branch has none. Detached HEADs can't be pushed.
pub fn push_changes(directory: &str, remote: &str) -> Result<PushOutcome, String> {
    let remote = remote.trim();
    if remote.is_empty() {
        return Err("Remote name is empty".to_string());
    }
    let branch = run_git(directory, &["branch", "--show-current"], &[], "git branch failed")?;
    let branch = branch.trim().to_string();
    if branch.is_empty() {
        return Err("Cannot push a detached HEAD — check out a branch first".to_string());
    }
    let output = run_git(
        directory,
        &["push", "-u", remote, &branch],
        &[],
        "git push failed",
    )?;
    let combined = output.to_lowercase();
    Ok(PushOutcome {
        branch,
        remote: remote.to_string(),
        up_to_date: combined.contains("up to date") || combined.contains("up-to-date"),
    })
}

/// One `git status --porcelain=v1 -z --untracked-files=normal` poll, for the
/// wholly-untracked-directory list the commit staging excludes.
fn collapsed_status(directory: &str) -> Result<Vec<u8>, String> {
    git_command()
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=normal"])
        .current_dir(directory)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git status failed: {}", e))
        .and_then(|output| {
            if output.status.success() {
                Ok(output.stdout)
            } else {
                Err(format!(
                    "git status failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        })
}

/// Runs `git <args> [-- <pathspecs>]` in `directory`; on failure returns
/// `context: <stderr>`. Stdout and stderr are both returned on success
/// (callers trim) because progress reporting like `git push`'s
/// "Everything up-to-date" goes to stderr.
fn run_git(
    directory: &str,
    args: &[&str],
    pathspecs: &[String],
    context: &str,
) -> Result<String, String> {
    let mut command = git_command();
    command.args(args);
    if !pathspecs.is_empty() {
        command.arg("--");
        command.args(pathspecs);
    }
    let output = command
        .current_dir(directory)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{context}: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{context}: {}", stderr.trim()));
    }
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(combined)
}

impl GitTracker {
    /// Lists the working-tree changes of `directory`, or `None` when git isn't
    /// available there. Safe to call without holding any pair-state lock.
    pub fn collect_modified_files(directory: &str) -> Option<Vec<ModifiedFile>> {
        let status = |untracked: &str, pathspecs: &[String]| {
            let output = git_command()
                .args(["status", "--porcelain=v1", "-z", untracked])
                .args(if pathspecs.is_empty() {
                    &[][..]
                } else {
                    &["--"][..]
                })
                .args(pathspecs)
                .current_dir(directory)
                .stdin(Stdio::null())
                .output()
                .ok()?;
            output.status.success().then_some(output.stdout)
        };

        // `normal` lists a wholly untracked directory as one `dir/` entry and
        // never walks it. Without such entries it matches `all` exactly.
        let collapsed = status("--untracked-files=normal", &[])?;
        let untracked_dirs = untracked_dirs_in_porcelain_z(&collapsed);
        if untracked_dirs.is_empty() {
            return Some(parse_porcelain_z(&collapsed, &[]));
        }

        // `all` lists new files in new directories one by one. Top-level
        // regenerable directories are excluded so git doesn't walk them;
        // nested ones are filtered while parsing.
        let excludes: Vec<String> = untracked_dirs
            .iter()
            .filter(|dir| is_regenerable_dir(dir))
            .take(MAX_STATUS_EXCLUDES)
            .map(|dir| exclude_pathspec(dir))
            .collect();
        let expanded = status("--untracked-files=all", &excludes)?;
        Some(parse_porcelain_z(&expanded, &untracked_dirs))
    }

    pub fn update_state(state: &mut PairState) {
        match Self::collect_modified_files(&state.directory) {
            Some(files) => {
                state.git_tracking.available = true;
                state.modified_files = files;
            }
            None => {
                println!(
                    "[GitTracker] Git status command failed in directory: {}",
                    state.directory
                );
                state.git_tracking.available = false;
            }
        }
    }

    pub fn get_file_diff(directory: &str, file_path: &str, status: &str) -> Result<String, String> {
        let relative = validate_relative_path(file_path)?;
        let canonical_directory = Path::new(directory)
            .canonicalize()
            .map_err(|e| format!("Failed to resolve workspace directory: {}", e))?;
        let full_path = canonical_directory.join(relative);

        if status == "??" {
            return read_untracked_file(&canonical_directory, &full_path, file_path);
        }

        // Deleted files no longer exist, so only the nearest existing ancestor
        // can be resolved; it must not lead outside the workspace through a
        // symlinked directory. (git itself shows a symlink's target text, never
        // the file it points at, so the last component needs no check.)
        if let Some(existing_ancestor) = full_path
            .parent()
            .and_then(|parent| parent.ancestors().find(|p| p.exists()))
        {
            let resolved = existing_ancestor
                .canonicalize()
                .map_err(|e| format!("Failed to resolve file path: {}", e))?;
            if !resolved.starts_with(&canonical_directory) {
                return Err("File path escapes the workspace directory".to_string());
            }
        }

        // Tracked changes (including deletions) diff against HEAD, or the
        // empty tree in a repository without commits. `--literal-pathspecs`
        // stops names with `*`, `?` or `:(...)` from acting as patterns.
        let base = diff_base(directory);
        let output = git_command()
            .args([
                "--literal-pathspecs",
                "diff",
                "--no-color",
                "--no-ext-diff",
                &base,
                "--",
                file_path,
            ])
            .current_dir(directory)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("git diff failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git diff failed: {}", stderr.trim()));
        }

        let diff = String::from_utf8_lossy(&output.stdout);
        let truncated = truncate_lines(&diff, MAX_DIFF_LINES, false);

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
    use std::path::PathBuf;

    fn raw_records(records: &[&str]) -> Vec<u8> {
        let mut raw = Vec::new();
        for record in records {
            raw.extend_from_slice(record.as_bytes());
            raw.push(0);
        }
        raw
    }

    fn parse(records: &[&str]) -> Vec<ModifiedFile> {
        parse_porcelain_z(&raw_records(records), &[])
    }

    #[test]
    fn regenerable_dirs_match_by_trailing_components() {
        for dir in [
            "node_modules",
            "packages/app/node_modules/",
            ".venv",
            "crates/x/target",
            "vendor/bundle",
            "app/vendor/bundle",
        ] {
            assert!(is_regenerable_dir(dir), "{dir}");
        }
        for dir in [
            "src",
            "my_node_modules",
            "vendor",
            "bundle",
            "xvendor/bundle",
            "build.rs",
        ] {
            assert!(!is_regenerable_dir(dir), "{dir}");
        }
    }

    #[test]
    fn regenerable_ancestor_requires_a_wholly_untracked_directory() {
        let untracked = vec!["newpkg".to_string(), "node_modules".to_string()];
        assert_eq!(
            regenerable_ancestor("newpkg/node_modules/x/index.js", &untracked),
            Some("newpkg/node_modules")
        );
        assert_eq!(
            regenerable_ancestor("node_modules/x/index.js", &untracked),
            Some("node_modules")
        );
        assert_eq!(
            regenerable_ancestor("newpkg/lib/build/", &untracked),
            Some("newpkg/lib/build")
        );
        // A `build/` holding tracked files was not reported as untracked.
        assert_eq!(
            regenerable_ancestor("build/new-script.sh", &untracked),
            None
        );
        assert_eq!(regenerable_ancestor("newpkg/src/main.js", &untracked), None);
        // A file (not a directory) named like one is kept.
        assert_eq!(regenerable_ancestor("newpkg/build", &untracked), None);
    }

    #[test]
    fn untracked_dirs_are_read_from_collapsed_status_records() {
        let raw = raw_records(&[
            "R  new.rs",
            "old/dir/",
            "?? newpkg/",
            "?? file.txt",
            " M src/lib.rs",
            "?? node_modules/",
        ]);
        assert_eq!(
            untracked_dirs_in_porcelain_z(&raw),
            vec!["newpkg".to_string(), "node_modules".to_string()]
        );
    }

    #[test]
    fn parses_modified_untracked_added_and_deleted() {
        let files = parse(&[
            " M src/lib.rs",
            "?? new.txt",
            "A  added.rs",
            " D gone.rs",
            "D  staged-gone.rs",
        ]);
        let summary: Vec<(&str, &str)> = files
            .iter()
            .map(|f| {
                let status = match f.status {
                    FileStatus::M => "M",
                    FileStatus::A => "A",
                    FileStatus::D => "D",
                    FileStatus::R => "R",
                    FileStatus::Untracked => "??",
                };
                (f.path.as_str(), status)
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("src/lib.rs", "M"),
                ("new.txt", "??"),
                ("added.rs", "A"),
                ("gone.rs", "D"),
                ("staged-gone.rs", "D"),
            ]
        );
        assert_eq!(files[0].display_path, "src/lib.rs");
    }

    #[test]
    fn rename_and_copy_use_destination_path_and_consume_source_field() {
        // -z emits "XY new\0old\0"; the old path must not become its own entry.
        let files = parse(&[
            "R  new/name.rs",
            "old/name.rs",
            "C  copy.rs",
            "orig.rs",
            " M after.rs",
        ]);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "new/name.rs");
        assert!(matches!(files[0].status, FileStatus::R));
        assert_eq!(files[1].path, "copy.rs");
        assert!(matches!(files[1].status, FileStatus::A));
        assert_eq!(files[2].path, "after.rs");
    }

    #[test]
    fn maps_conflict_and_type_change_statuses() {
        let files = parse(&[
            "UU both.rs",
            "AA both-added.rs",
            "DD both-deleted.rs",
            "T  typechange",
            "MD modified-then-deleted.rs",
        ]);
        let statuses: Vec<&FileStatus> = files.iter().map(|f| &f.status).collect();
        assert!(matches!(statuses[0], FileStatus::M));
        assert!(matches!(statuses[1], FileStatus::M));
        assert!(matches!(statuses[2], FileStatus::D));
        assert!(matches!(statuses[3], FileStatus::M));
        assert!(matches!(statuses[4], FileStatus::D));
    }

    #[test]
    fn keeps_spaces_and_unicode_paths_verbatim() {
        let files = parse(&[
            "?? my file.txt",
            "?? 中文/文档.md",
            " M dir with space/a b.rs",
        ]);
        assert_eq!(files[0].path, "my file.txt");
        assert_eq!(files[1].path, "中文/文档.md");
        assert_eq!(files[2].path, "dir with space/a b.rs");
    }

    #[test]
    fn skips_records_too_short_to_hold_a_path() {
        assert!(parse(&[""]).is_empty());
        assert!(parse(&[" M "]).is_empty());
        assert!(parse_porcelain_z(b"", &[]).is_empty());
    }

    #[test]
    fn validate_relative_path_rejects_escapes() {
        assert!(validate_relative_path("src/main.rs").is_ok());
        assert!(validate_relative_path("./src/main.rs").is_ok());
        assert!(validate_relative_path("../outside").is_err());
        assert!(validate_relative_path("src/../../outside").is_err());
        assert!(validate_relative_path("/etc/passwd").is_err());
        assert!(validate_relative_path("").is_err());
    }

    struct TempRepo {
        root: PathBuf,
    }

    impl TempRepo {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "the-pair-git-tracker-{}-{}-{}",
                label,
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&root).unwrap();
            let temp = TempRepo {
                root: root.canonicalize().unwrap(),
            };
            temp.git(&["init", "-q"]);
            temp.git(&["config", "user.name", "Test"]);
            temp.git(&["config", "user.email", "test@example.com"]);
            temp.git(&["config", "commit.gpgsign", "false"]);
            temp
        }

        fn git(&self, args: &[&str]) {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn dir(&self) -> &str {
            self.root.to_str().unwrap()
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn collect_modified_files_handles_real_repository_edge_cases() {
        let temp = TempRepo::new("collect");
        fs::write(temp.root.join("keep.txt"), "keep\n").unwrap();
        fs::write(temp.root.join("old name.txt"), "rename me\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);

        temp.git(&["mv", "old name.txt", "new name.txt"]);
        fs::create_dir_all(temp.root.join("新目录").join("nested")).unwrap();
        fs::write(temp.root.join("新目录/nested/文件 一.md"), "hi\n").unwrap();
        fs::write(temp.root.join("新目录/second.txt"), "second\n").unwrap();
        fs::write(temp.root.join("keep.txt"), "changed\n").unwrap();

        let files = GitTracker::collect_modified_files(temp.dir()).expect("git status runs");
        let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        assert_eq!(
            paths,
            vec![
                "keep.txt",
                "new name.txt",
                "新目录/nested/文件 一.md",
                "新目录/second.txt"
            ]
        );
        let renamed = files.iter().find(|f| f.path == "new name.txt").unwrap();
        assert!(matches!(renamed.status, FileStatus::R));

        // Every reported path can be diffed.
        for file in &files {
            let status = if matches!(file.status, FileStatus::Untracked) {
                "??"
            } else {
                "M"
            };
            let diff = GitTracker::get_file_diff(temp.dir(), &file.path, status)
                .unwrap_or_else(|e| panic!("diff for {} failed: {}", file.path, e));
            assert!(!diff.is_empty());
        }
    }

    #[test]
    fn collect_modified_files_skips_untracked_dependency_directories() {
        let temp = TempRepo::new("regenerable");
        fs::create_dir_all(temp.root.join("build")).unwrap();
        fs::write(temp.root.join("build/tracked.sh"), "echo\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);

        for index in 0..(MAX_MODIFIED_FILES + 10) {
            let dir = temp.root.join(format!("node_modules/pkg{}", index % 50));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(format!("f{index}.js")), "x").unwrap();
        }
        fs::create_dir_all(temp.root.join("newpkg/.venv/lib")).unwrap();
        fs::write(temp.root.join("newpkg/.venv/lib/site.py"), "x").unwrap();
        fs::create_dir_all(temp.root.join("newpkg/src")).unwrap();
        fs::write(temp.root.join("newpkg/src/main.py"), "print()\n").unwrap();
        // `build/` holds tracked files, so new files in it are real work.
        fs::write(temp.root.join("build/tracked.sh"), "echo changed\n").unwrap();
        fs::write(temp.root.join("build/new.sh"), "echo new\n").unwrap();

        let files = GitTracker::collect_modified_files(temp.dir()).expect("git status runs");
        let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        assert_eq!(
            paths,
            vec!["build/new.sh", "build/tracked.sh", "newpkg/src/main.py"]
        );

        // Without untracked directories a single `normal` status is enough.
        fs::remove_dir_all(temp.root.join("node_modules")).unwrap();
        fs::remove_dir_all(temp.root.join("newpkg")).unwrap();
        let files = GitTracker::collect_modified_files(temp.dir()).expect("git status runs");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn get_file_diff_handles_deleted_files() {
        let temp = TempRepo::new("deleted");
        fs::write(temp.root.join("gone.txt"), "soon deleted\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);
        fs::remove_file(temp.root.join("gone.txt")).unwrap();

        let diff = GitTracker::get_file_diff(temp.dir(), "gone.txt", "D").expect("deleted diff");
        assert!(diff.contains("-soon deleted"), "{}", diff);

        // A deleted file inside a deleted directory, too.
        fs::create_dir_all(temp.root.join("dir")).unwrap();
        fs::write(temp.root.join("dir/inner.txt"), "inner\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "dir"]);
        fs::remove_dir_all(temp.root.join("dir")).unwrap();
        let diff = GitTracker::get_file_diff(temp.dir(), "dir/inner.txt", "D")
            .expect("nested deleted diff");
        assert!(diff.contains("-inner"), "{}", diff);
    }

    #[test]
    fn get_file_diff_works_before_the_first_commit() {
        let temp = TempRepo::new("unborn");
        fs::write(temp.root.join("staged.txt"), "first line\n").unwrap();
        temp.git(&["add", "staged.txt"]);

        let diff = GitTracker::get_file_diff(temp.dir(), "staged.txt", "A").expect("unborn diff");
        assert!(diff.contains("+first line"), "{}", diff);
    }

    #[test]
    fn get_file_diff_rejects_traversal_and_symlink_escapes() {
        let temp = TempRepo::new("escape");
        assert!(GitTracker::get_file_diff(temp.dir(), "../outside.txt", "M").is_err());
        assert!(GitTracker::get_file_diff(temp.dir(), "/etc/hosts", "??").is_err());

        #[cfg(unix)]
        {
            let outside = temp.root.with_extension("outside");
            fs::create_dir_all(&outside).unwrap();
            fs::write(outside.join("secret.txt"), "secret\n").unwrap();
            std::os::unix::fs::symlink(&outside, temp.root.join("link")).unwrap();
            let result = GitTracker::get_file_diff(temp.dir(), "link/secret.txt", "??");
            assert!(result.is_err());
            let result = GitTracker::get_file_diff(temp.dir(), "link/secret.txt", "M");
            assert!(result.is_err());
            let _ = fs::remove_dir_all(&outside);
        }
    }

    #[test]
    fn get_file_diff_caps_untracked_reads() {
        let temp = TempRepo::new("cap");
        let line = "x".repeat(99) + "\n";
        let big = line.repeat(((MAX_UNTRACKED_READ_BYTES as usize) / line.len()) * 3);
        fs::write(temp.root.join("big.txt"), &big).unwrap();

        let diff = GitTracker::get_file_diff(temp.dir(), "big.txt", "??").expect("big file");
        assert!(diff.ends_with("... (truncated)"));
        assert!(diff.len() < MAX_UNTRACKED_READ_BYTES as usize);

        fs::create_dir_all(temp.root.join("nested-repo")).unwrap();
        let err = GitTracker::get_file_diff(temp.dir(), "nested-repo", "??").expect_err("dir");
        assert!(err.contains("directory"), "{}", err);
    }

    #[test]
    fn outcome_structs_serialize_camel_case_for_ipc() {
        let commit =
            serde_json::to_value(CommitOutcome {
                sha: "abc1234".into(),
                files_committed: 2,
            })
            .unwrap();
        assert_eq!(commit["sha"], "abc1234");
        assert_eq!(commit["filesCommitted"], 2);
        assert!(commit.get("files_committed").is_none());

        let push = serde_json::to_value(PushOutcome {
            branch: "feature".into(),
            remote: "origin".into(),
            up_to_date: true,
        })
        .unwrap();
        assert_eq!(push["upToDate"], true);
        assert!(push.get("up_to_date").is_none());
    }

    #[test]
    fn commit_changes_stages_tracked_edits_and_new_files() {
        let temp = TempRepo::new("commit");
        fs::write(temp.root.join("keep.txt"), "keep\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);

        fs::write(temp.root.join("keep.txt"), "changed\n").unwrap();
        fs::write(temp.root.join("new.txt"), "new\n").unwrap();

        let outcome = commit_changes(temp.dir(), "the-pair: test commit").expect("commit");
        assert_eq!(outcome.sha.len(), 7);
        assert_eq!(outcome.files_committed, 2);
        assert!(
            GitTracker::collect_modified_files(temp.dir())
                .expect("status runs")
                .is_empty()
        );

        let log = Command::new("git")
            .args(["log", "-1", "--format=%s"])
            .current_dir(&temp.root)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&log.stdout).trim(),
            "the-pair: test commit"
        );
    }

    #[test]
    fn commit_changes_skips_untracked_regenerable_directories() {
        let temp = TempRepo::new("commit-skip");
        fs::write(temp.root.join("keep.txt"), "keep\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);

        fs::write(temp.root.join("keep.txt"), "changed\n").unwrap();
        fs::create_dir_all(temp.root.join("node_modules/pkg")).unwrap();
        fs::write(temp.root.join("node_modules/pkg/index.js"), "x").unwrap();

        let outcome = commit_changes(temp.dir(), "work").expect("commit");
        assert_eq!(outcome.files_committed, 1);

        let remaining = GitTracker::collect_modified_files(temp.dir()).expect("status runs");
        assert!(remaining.is_empty(), "{:?}", remaining);

        // The regenerable directory was never committed.
        let ls = Command::new("git")
            .args(["ls-tree", "-r", "--name-only", "HEAD"])
            .current_dir(&temp.root)
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&ls.stdout).into_owned();
        assert!(!tracked.contains("node_modules"), "{}", tracked);
    }

    #[test]
    fn commit_changes_excludes_every_regenerable_dir_beyond_the_status_cap() {
        let temp = TempRepo::new("commit-many-excludes");
        fs::write(temp.root.join("keep.txt"), "keep\n").unwrap();
        for index in 0..=(MAX_STATUS_EXCLUDES) {
            let pkg = temp.root.join(format!("pkg{index}"));
            fs::create_dir_all(&pkg).unwrap();
            fs::write(pkg.join("pkg.txt"), "x").unwrap();
        }
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);
        fs::write(temp.root.join("keep.txt"), "changed\n").unwrap();

        // One wholly-untracked regenerable directory per tracked package,
        // one more than the status-poll exclude cap.
        for index in 0..=(MAX_STATUS_EXCLUDES) {
            let dir = temp.root.join(format!("pkg{index}/node_modules"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("index.js"), "x").unwrap();
        }

        let outcome = commit_changes(temp.dir(), "work").expect("commit");
        assert_eq!(outcome.files_committed, 1);

        let ls = Command::new("git")
            .args(["ls-tree", "-r", "--name-only", "HEAD"])
            .current_dir(&temp.root)
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&ls.stdout).into_owned();
        assert!(!tracked.contains("node_modules"), "{}", tracked);
    }

    #[test]
    fn commit_changes_rejects_empty_message_and_clean_tree() {
        let temp = TempRepo::new("commit-err");
        fs::write(temp.root.join("keep.txt"), "keep\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);

        let err = commit_changes(temp.dir(), "   ").expect_err("blank message");
        assert!(err.contains("empty"), "{}", err);

        let err = commit_changes(temp.dir(), "work").expect_err("clean tree");
        assert!(err.contains("Nothing to commit"), "{}", err);

        let long = "x".repeat(MAX_COMMIT_MESSAGE_CHARS + 1);
        let err = commit_changes(temp.dir(), &long).expect_err("oversize message");
        assert!(err.contains("exceeds"), "{}", err);
    }

    #[test]
    fn push_changes_pushes_branch_and_reports_up_to_date() {
        let temp = TempRepo::new("push");
        let remote_dir = temp.root.with_extension("remote.git");
        let output = Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(&remote_dir)
            .output()
            .unwrap();
        assert!(output.status.success());
        temp.git(&["remote", "add", "origin", remote_dir.to_str().unwrap()]);
        fs::write(temp.root.join("keep.txt"), "keep\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);

        let branch = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&temp.root)
            .output()
            .unwrap();
        let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();

        let outcome = push_changes(temp.dir(), "origin").expect("push");
        assert_eq!(outcome.branch, branch);
        assert_eq!(outcome.remote, "origin");
        assert!(!outcome.up_to_date);

        let outcome = push_changes(temp.dir(), "origin").expect("second push");
        assert!(outcome.up_to_date);

        let _ = fs::remove_dir_all(&remote_dir);
    }

    #[test]
    fn push_changes_rejects_detached_head_and_unknown_remote() {
        let temp = TempRepo::new("push-err");
        fs::write(temp.root.join("keep.txt"), "keep\n").unwrap();
        temp.git(&["add", "."]);
        temp.git(&["commit", "-q", "--no-verify", "-m", "init"]);

        let err = push_changes(temp.dir(), "nope").expect_err("unknown remote");
        assert!(err.contains("git push failed"), "{}", err);

        temp.git(&["checkout", "-q", "--detach", "HEAD"]);
        let err = push_changes(temp.dir(), "origin").expect_err("detached HEAD");
        assert!(err.contains("detached HEAD"), "{}", err);

        let err = push_changes(temp.dir(), "  ").expect_err("blank remote");
        assert!(err.contains("empty"), "{}", err);
    }
}
