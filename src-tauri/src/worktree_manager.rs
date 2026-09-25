use crate::git_tracker::{exclude_pathspec, is_regenerable_dir, regenerable_ancestor};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Namespace for the branch each pair worktree is created on
/// (`the-pair/<worktree-dir-name>`), so executor commits are never orphaned.
const PAIR_BRANCH_PREFIX: &str = "the-pair/";
/// Namespace for branches that keep otherwise-unreachable commits alive when a
/// detached worktree (created by older versions) is deleted.
const RESCUE_BRANCH_PREFIX: &str = "the-pair/rescued-";
/// Identity used for the auto-save stash when the user has none configured,
/// so a missing `user.email` can't block deleting a pair.
const FALLBACK_IDENTITY: [&str; 4] = [
    "-c",
    "user.name=The Pair",
    "-c",
    "user.email=the-pair@localhost",
];
/// `for-each-ref` format: NUL-separated fields with the free-form subject last,
/// so a `|` (or anything else) in a commit subject can't shift the other fields.
const REF_FORMAT: &str =
    "--format=%(refname)%00%(symref)%00%(objectname:short)%00%(committerdate:unix)%00%(subject)";
/// Exclude pathspecs one stash command may carry (command-line length).
const MAX_STASH_EXCLUDES: usize = 256;

/// Past these, auto-saving untracked files into the stash would take minutes
/// and grow `.git` for good, so deleting the pair is refused instead.
#[derive(Clone, Copy)]
struct StashLimits {
    files: usize,
    bytes: u64,
}

const STASH_LIMITS: StashLimits = StashLimits {
    files: 20_000,
    bytes: 200 * 1024 * 1024,
};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

fn git_command() -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Runs git and returns its raw stdout (for `-z` output, where trimming would
/// eat a record's leading status column).
fn run_git_bytes(directory: impl AsRef<Path>, args: &[&str]) -> Result<Vec<u8>, String> {
    let directory = directory.as_ref();
    println!(
        "[worktree_manager] run_git_command: dir={}, args={:?}",
        directory.display(),
        args
    );
    let output = git_command()
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::null())
        .output();

    match output {
        Ok(o) => {
            if o.status.success() {
                Ok(o.stdout)
            } else {
                let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                println!("[worktree_manager] git command failed: {}", stderr);
                Err(stderr)
            }
        }
        Err(e) => {
            println!("[worktree_manager] git command failed to execute: {}", e);
            Err(format!("Failed to run git: {}", e))
        }
    }
}

fn run_git_command(directory: impl AsRef<Path>, args: &[&str]) -> Result<String, String> {
    run_git_bytes(directory, args).map(|stdout| String::from_utf8_lossy(&stdout).trim().to_string())
}

pub fn check_is_git_repo(directory: &str) -> bool {
    // Inside a `.git` directory this prints "false" with exit code 0.
    let result = run_git_command(directory, &["rev-parse", "--is-inside-work-tree"]);
    println!("[worktree_manager] check_is_git_repo result: {:?}", result);
    matches!(result.as_deref(), Ok("true"))
}

pub fn check_is_dirty(directory: &str) -> bool {
    // `--no-optional-locks` keeps this read-only poll from taking `index.lock`
    // out from under an agent's concurrent `git add` / `git commit`.
    let output = run_git_command(directory, &["--no-optional-locks", "status", "--porcelain"]);
    match output {
        Ok(s) => !s.is_empty(),
        Err(_) => false,
    }
}

pub fn get_current_branch(directory: &str) -> Option<String> {
    // Detached HEAD prints nothing; report that as "no current branch".
    run_git_command(directory, &["branch", "--show-current"])
        .ok()
        .filter(|name| !name.is_empty())
}

struct RefRecord {
    refname: String,
    symref: String,
    sha: Option<String>,
    date: Option<u64>,
    subject: Option<String>,
}

fn parse_ref_records(output: &str) -> Vec<RefRecord> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\0');
            let refname = fields.next()?.trim().to_string();
            if refname.is_empty() {
                return None;
            }
            let symref = fields.next().unwrap_or("").trim().to_string();
            let sha = fields
                .next()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let date = fields.next().and_then(|s| s.trim().parse::<u64>().ok());
            let subject = fields.next().map(|s| s.trim().to_string());
            Some(RefRecord {
                refname,
                symref,
                sha,
                date,
                subject,
            })
        })
        .collect()
}

fn list_remotes(directory: &str) -> Vec<String> {
    run_git_command(directory, &["remote"])
        .map(|output| {
            output
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Splits `origin/feature/x` into (`origin`, `feature/x`), preferring the
/// longest configured remote name so remotes containing `/` resolve correctly.
fn split_remote_ref<'a>(name: &'a str, remotes: &[String]) -> Option<(&'a str, &'a str)> {
    let configured = remotes
        .iter()
        .filter(|remote| {
            name.len() > remote.len() + 1
                && name.starts_with(remote.as_str())
                && name.as_bytes()[remote.len()] == b'/'
        })
        .max_by_key(|remote| remote.len());
    let (remote, branch) = match configured {
        Some(remote) => (&name[..remote.len()], &name[remote.len() + 1..]),
        None => name.split_once('/')?,
    };
    if remote.is_empty() || branch.is_empty() {
        None
    } else {
        Some((remote, branch))
    }
}

pub fn list_branches(directory: &str) -> Result<Vec<BranchInfo>, String> {
    let local_refs = run_git_command(directory, &["for-each-ref", REF_FORMAT, "refs/heads"])
        .map(|output| parse_ref_records(&output))
        .unwrap_or_default();
    let remote_refs = run_git_command(directory, &["for-each-ref", REF_FORMAT, "refs/remotes"])
        .map(|output| parse_ref_records(&output))
        .unwrap_or_default();

    let local_branch_names: Vec<&str> = local_refs
        .iter()
        .filter_map(|record| record.refname.strip_prefix("refs/heads/"))
        .collect();

    let mut branches: Vec<BranchInfo> = Vec::new();

    let current_branch = get_current_branch(directory);

    for record in &local_refs {
        let Some(name) = record.refname.strip_prefix("refs/heads/") else {
            continue;
        };
        // A name starting with '-' would be parsed as an option downstream.
        if name.starts_with('-') {
            continue;
        }
        branches.push(BranchInfo {
            name: name.to_string(),
            is_local: true,
            is_remote: false,
            last_commit_sha: record.sha.clone(),
            last_commit_message: record.subject.clone(),
            last_commit_date: record.date,
            is_checked_out_locally: current_branch.as_deref() == Some(name),
        });
    }

    let remotes = if remote_refs.is_empty() {
        Vec::new()
    } else {
        list_remotes(directory)
    };

    for record in &remote_refs {
        // `refs/remotes/origin/HEAD` is a symbolic ref whose short name is just
        // `origin`; it is not a branch anyone can pick.
        if !record.symref.is_empty() || record.refname.ends_with("/HEAD") {
            continue;
        }
        let Some(name) = record.refname.strip_prefix("refs/remotes/") else {
            continue;
        };
        let Some((_remote, branch)) = split_remote_ref(name, &remotes) else {
            continue;
        };
        // Remote refs may start with '-' (e.g. `origin/-M`), which can never be a
        // local branch name and would be parsed as an option by `git branch`.
        if branch.starts_with('-') {
            continue;
        }

        branches.push(BranchInfo {
            name: name.to_string(),
            is_local: false,
            is_remote: true,
            last_commit_sha: record.sha.clone(),
            last_commit_message: record.subject.clone(),
            last_commit_date: record.date,
            is_checked_out_locally: local_branch_names.contains(&branch),
        });
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

/// Rejects empty names, names that would be parsed as options, and anything
/// `git check-ref-format --branch` refuses.
fn validate_branch_name(directory: &str, name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.starts_with('-') {
        return Err(format!("Invalid branch name: '{}'", name));
    }
    run_git_command(directory, &["check-ref-format", "--branch", name])
        .map(|_| ())
        .map_err(|_| format!("Invalid branch name: '{}'", name))
}

fn ref_exists(directory: impl AsRef<Path>, full_ref: &str) -> bool {
    run_git_command(directory, &["show-ref", "--verify", "--quiet", full_ref]).is_ok()
}

/// Resolves the user's selection to a fully-qualified ref: a local branch, or
/// a remote-tracking branch such as `origin/feature`.
fn resolve_start_point(repo_path: &str, branch: &str) -> Result<String, String> {
    if branch.trim().is_empty() || branch.starts_with('-') {
        return Err(format!("Invalid branch name: '{}'", branch));
    }
    let local = format!("refs/heads/{}", branch);
    if ref_exists(repo_path, &local) {
        return Ok(local);
    }
    let remote = format!("refs/remotes/{}", branch);
    if ref_exists(repo_path, &remote) {
        return Ok(remote);
    }
    Err(format!(
        "Branch '{}' was not found in {}",
        branch, repo_path
    ))
}

fn pair_branch_name(worktree_dir_name: &str) -> String {
    format!("{}{}", PAIR_BRANCH_PREFIX, worktree_dir_name)
}

/// Creates `<repo_path>/<worktree_path>` as a linked worktree on a NEW branch
/// `the-pair/<worktree-dir-name>` started from `branch` (a local branch or a
/// remote-tracking branch). Working on a named branch, instead of a detached
/// HEAD, keeps every executor commit reachable after the worktree is removed.
pub fn create_worktree(
    repo_path: &str,
    branch: &str,
    worktree_path: &str,
) -> Result<String, String> {
    let start_point = resolve_start_point(repo_path, branch)?;

    let full_worktree_path = Path::new(repo_path).join(worktree_path);

    let path_str = full_worktree_path.to_str().ok_or_else(|| {
        format!(
            "Worktree path contains non-UTF-8 characters: {:?}",
            full_worktree_path
        )
    })?;
    let dir_name = full_worktree_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("Invalid worktree path: {}", path_str))?;

    let pair_branch = pair_branch_name(dir_name);
    validate_branch_name(repo_path, &pair_branch)?;
    let pair_ref = format!("refs/heads/{}", pair_branch);
    if ref_exists(repo_path, &pair_ref) {
        return Err(format!(
            "Failed to create worktree: branch '{}' already exists",
            pair_branch
        ));
    }

    let worktrees_dir = Path::new(repo_path).join(".worktrees");
    if !worktrees_dir.exists() {
        std::fs::create_dir_all(&worktrees_dir)
            .map_err(|e| format!("Failed to create .worktrees directory: {}", e))?;
    }

    // `--no-track`: the pair branch must not inherit the selected branch as its
    // upstream. `--` ends option parsing before the path and start point.
    let result = run_git_command(
        repo_path,
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            &pair_branch,
            "--",
            path_str,
            &start_point,
        ],
    );

    match result {
        Ok(_) => Ok(full_worktree_path.to_string_lossy().to_string()),
        Err(e) => {
            // `worktree add -b` creates the branch before checking out; don't
            // leave it behind when the checkout fails. It didn't exist before
            // (checked above), so deleting it can't lose anything.
            if ref_exists(repo_path, &pair_ref) {
                let _ = run_git_command(repo_path, &["branch", "-D", "--", &pair_branch]);
            }
            Err(format!("Failed to create worktree: {}", e))
        }
    }
}

/// Deletes a pair worktree without losing work (contract C1):
///
/// - Directory already gone: the worktree's own stale entry is removed from
///   the main repository (after rescuing a detached HEAD commit) and `Ok(())`
///   returned. Other worktrees are never pruned: one on an unmounted volume
///   would lose its registration.
/// - Otherwise, before anything is removed, a detached HEAD whose commits are
///   reachable from no branch/tag gets a `the-pair/rescued-<short-sha>` branch,
///   and uncommitted changes (tracked and untracked, not ignored) are saved with
///   `git stash push --include-untracked -m "the-pair: auto-saved from pair worktree <dir>"`.
///   Wholly untracked dependency/build directories (`node_modules`, `.venv`,
///   `target`, ... see `git_tracker::REGENERABLE_DIRS`) are left out of the
///   stash and deleted: they are recreated by the tools that made them.
///   The worktree is then removed.
/// - What the stash can't hold (a nested repository, submodule changes, an
///   untracked set too large to hash) is refused up front: nothing is stashed
///   and nothing removed. If the stash still misses something, it is popped
///   back before `Err` is returned.
/// - A `pair-<uuid>` directory under `.worktrees/` that git no longer lists
///   is what a part-failed removal leaves behind (git drops its entry even when
///   a locked file stops the directory removal). Its work was preserved by
///   that first attempt, so the rest of it is removed.
/// - Anything else that can't be preserved is refused with `Err`.
///
/// The pair's own `the-pair/<dir>` branch is kept whenever it holds commits no
/// other branch/tag has; it is only deleted when it adds nothing.
pub fn delete_worktree(worktree_path: &str) -> Result<(), String> {
    let path = Path::new(worktree_path);
    let dir_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| worktree_path.to_string());
    let common_dir = find_common_git_dir(path);

    if !path.exists() {
        if let Some(common_dir) = &common_dir {
            if let Some(entry) = forget_missing_worktree(common_dir, path) {
                if let (Some(branch), Some(head)) = (&entry.branch, &entry.head) {
                    delete_pair_branch_if_redundant(common_dir, branch, head, &dir_name);
                }
            }
        }
        return Ok(());
    }

    let layout = match worktree_layout(path) {
        Some(layout)
            if same_path(&layout.toplevel, path)
                && !same_path(&layout.git_dir, &layout.common_dir) =>
        {
            layout
        }
        _ => {
            // Not the root of a linked worktree, so git can't preserve its
            // contents. Only an empty directory or the leftover of a removal
            // that already preserved everything is safe to drop.
            if dir_is_empty(path) {
                std::fs::remove_dir(path)
                    .map_err(|e| format!("Failed to remove empty worktree directory: {}", e))?;
                if let Some(common_dir) = &common_dir {
                    forget_missing_worktree(common_dir, path);
                }
                return Ok(());
            }
            if let Some(common_dir) = leftover_pair_worktree(path) {
                std::fs::remove_dir_all(path).map_err(|e| {
                    format!(
                        "Could not remove what is left of {} (its work was already saved): {}",
                        worktree_path, e
                    )
                })?;
                let pair_ref = format!("refs/heads/{}", pair_branch_name(&dir_name));
                if let Ok(head) = run_git_command(
                    &common_dir,
                    &["rev-parse", "--verify", "--quiet", &pair_ref],
                ) {
                    delete_pair_branch_if_redundant(&common_dir, &pair_ref, &head, &dir_name);
                }
                return Ok(());
            }
            return Err(format!(
                "Refusing to delete {}: it is not a linked git worktree, so its contents can't be preserved. Remove it manually if it is no longer needed.",
                worktree_path
            ));
        }
    };

    let preserved = preserve_worktree(path, &dir_name)?;
    remove_linked_worktree(&layout.common_dir, path, &preserved.excludes)?;
    // Only needed when git refused and the directory was removed directly.
    forget_missing_worktree(&layout.common_dir, path);

    if let (Some(branch), Some(head)) = (&preserved.branch_ref, &preserved.head) {
        delete_pair_branch_if_redundant(&layout.common_dir, branch, head, &dir_name);
    }

    Ok(())
}

/// For a directory that is no longer a git worktree: `Some(common git dir)`
/// when it is a pair worktree (`.worktrees/pair-<uuid>`) whose registration
/// git already removed, i.e. what a part-failed `git worktree remove` leaves.
fn leftover_pair_worktree(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let pair_id = name.strip_prefix("pair-")?;
    uuid::Uuid::parse_str(pair_id).ok()?;
    let parent = path.parent()?;
    if parent.file_name()? != ".worktrees" {
        return None;
    }

    let common_dir = find_common_git_dir(parent)?;
    let entries = list_worktree_entries(&common_dir)?;
    // Still registered, here or under an older path (a moved repository).
    let registered = entries.iter().any(|entry| {
        same_path(&entry.path, path) || entry.path.file_name().is_some_and(|n| n == name)
    });
    let admin_dir = common_dir.join("worktrees");
    if registered || admin_dir.join(name).exists() {
        return None;
    }

    // A `.git` link that survived must point at this repository's (now
    // removed) entry; a `.git` directory is a repository of its own.
    let dot_git = path.join(".git");
    match std::fs::symlink_metadata(&dot_git) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(common_dir),
        Ok(metadata) if metadata.is_file() => {
            let content = std::fs::read_to_string(&dot_git).ok()?;
            let target = PathBuf::from(content.trim().strip_prefix("gitdir:")?.trim());
            let target = if target.is_relative() {
                path.join(target)
            } else {
                target
            };
            let points_here = !target.exists()
                && target
                    .parent()
                    .is_some_and(|parent| same_path(parent, &admin_dir));
            points_here.then_some(common_dir)
        }
        _ => None,
    }
}

/// Runs `git rev-parse <args>` in `directory` and returns its output lines as
/// paths, resolving relative ones against `directory`. (`--path-format=absolute`
/// is avoided on purpose: git < 2.31 echoes it back instead of honoring it.)
fn rev_parse_paths(directory: &Path, args: &[&str], expected: usize) -> Option<Vec<PathBuf>> {
    let mut command = vec!["rev-parse"];
    command.extend_from_slice(args);
    let output = run_git_command(directory, &command).ok()?;
    let paths: Vec<PathBuf> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let path = PathBuf::from(line);
            if path.is_relative() {
                directory.join(path)
            } else {
                path
            }
        })
        .collect();
    (paths.len() == expected).then_some(paths)
}

/// Path of the repository's common git dir (the main repo's `.git`, shared by
/// all linked worktrees). A path that no longer exists is resolved through its
/// nearest existing ancestor.
fn find_common_git_dir(path: &Path) -> Option<PathBuf> {
    let probe = path.ancestors().find(|candidate| candidate.is_dir())?;
    rev_parse_paths(probe, &["--git-common-dir"], 1)?.pop()
}

struct WorktreeLayout {
    git_dir: PathBuf,
    common_dir: PathBuf,
    toplevel: PathBuf,
}

fn worktree_layout(path: &Path) -> Option<WorktreeLayout> {
    let mut paths = rev_parse_paths(
        path,
        &["--git-dir", "--git-common-dir", "--show-toplevel"],
        3,
    )?
    .into_iter();
    Some(WorktreeLayout {
        git_dir: paths.next()?,
        common_dir: paths.next()?,
        toplevel: paths.next()?,
    })
}

fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(parent) = parent.canonicalize() {
            return parent.join(name);
        }
    }
    path.to_path_buf()
}

fn same_path(a: &Path, b: &Path) -> bool {
    normalize_path(a) == normalize_path(b)
}

fn dir_is_empty(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

struct PreservedWorktree {
    head: Option<String>,
    branch_ref: Option<String>,
    /// Exclude pathspecs for the regenerable directories left out of the stash.
    excludes: Vec<String>,
}

fn preserve_worktree(worktree: &Path, dir_name: &str) -> Result<PreservedWorktree, String> {
    let head = run_git_command(
        worktree,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
    )
    .ok()
    .filter(|sha| !sha.is_empty());
    let branch_ref = run_git_command(worktree, &["symbolic-ref", "--quiet", "HEAD"])
        .ok()
        .filter(|name| !name.is_empty());

    // A detached HEAD (worktrees made by older versions) keeps its commits
    // reachable only through the worktree itself.
    if branch_ref.is_none() {
        if let Some(sha) = &head {
            rescue_commit_if_unreachable(worktree, sha)?;
        }
    }

    let status = read_worktree_status(worktree)?;
    let mut excludes = Vec::new();
    if !status.clean {
        // Submodule changes can't go into the stash (and a submodule's own
        // repository is deleted with the worktree). Refuse before stashing.
        if let Some(submodule) = status.changed_submodules.first() {
            return Err(format!(
                "Could not delete {}: the submodule '{}' has changes (or commits) that git stash can't save. Commit and push them inside the submodule, or discard them, then delete the pair again (nothing was deleted or stashed).",
                worktree.display(),
                submodule
            ));
        }
        excludes = plan_untracked_stash(worktree, &status.untracked_dirs, STASH_LIMITS)?;
        let pending = changed_paths(worktree, &excludes)?;
        if !pending.is_empty() {
            stash_worktree_changes(worktree, dir_name, &excludes, &pending)?;
        }
    }

    Ok(PreservedWorktree {
        head,
        branch_ref,
        excludes,
    })
}

/// What `git status --porcelain=v2` reports that matters before stashing.
#[derive(Debug, Default)]
struct WorktreeStatus {
    clean: bool,
    /// Wholly untracked directories (`? dir/`), without the trailing `/`.
    untracked_dirs: Vec<String>,
    changed_submodules: Vec<String>,
}

fn parse_worktree_status_v2(output: &[u8]) -> WorktreeStatus {
    let mut status = WorktreeStatus {
        clean: true,
        ..WorktreeStatus::default()
    };
    let mut records = output.split(|&byte| byte == 0);
    while let Some(record) = records.next() {
        let record = String::from_utf8_lossy(record);
        // `1 XY sub mH mI mW hH hI path`, `2 ... Xscore path\0orig`,
        // `u XY sub m1 m2 m3 mW h1 h2 h3 path`, `? path`.
        let (fields, has_orig) = match record.as_bytes().first() {
            Some(b'1') => (9, false),
            Some(b'2') => (10, true),
            Some(b'u') => (11, false),
            Some(b'?') => {
                status.clean = false;
                if let Some(dir) = record.strip_prefix("? ").and_then(|p| p.strip_suffix('/')) {
                    status.untracked_dirs.push(dir.to_string());
                }
                continue;
            }
            _ => continue,
        };
        if has_orig {
            let _original_path = records.next();
        }
        status.clean = false;
        let parts: Vec<&str> = record.splitn(fields, ' ').collect();
        if parts.len() == fields && parts[2].starts_with('S') {
            status
                .changed_submodules
                .push(parts[fields - 1].to_string());
        }
    }
    status
}

fn read_worktree_status(worktree: &Path) -> Result<WorktreeStatus, String> {
    run_git_bytes(
        worktree,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=none",
        ],
    )
    .map(|output| parse_worktree_status_v2(&output))
    .map_err(|e| {
        format!(
            "Could not read the status of {} (nothing was deleted): {}",
            worktree.display(),
            e
        )
    })
}

/// One entry of `git status --porcelain=v1 -z --untracked-files=all`.
#[derive(Debug)]
struct ChangedPath {
    path: String,
    untracked: bool,
}

/// Every change the auto-save has to capture, untracked files one by one.
fn changed_paths(worktree: &Path, excludes: &[String]) -> Result<Vec<ChangedPath>, String> {
    let mut args = vec![
        "--no-optional-locks",
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=none",
    ];
    if !excludes.is_empty() {
        args.push("--");
        args.extend(excludes.iter().map(String::as_str));
    }
    let output = run_git_bytes(worktree, &args).map_err(|e| {
        format!(
            "Could not read the status of {} (nothing was deleted): {}",
            worktree.display(),
            e
        )
    })?;

    let mut changes = Vec::new();
    let mut records = output.split(|&byte| byte == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            continue;
        }
        if matches!(record[0], b'R' | b'C') || matches!(record[1], b'R' | b'C') {
            let _original_path = records.next();
        }
        changes.push(ChangedPath {
            path: String::from_utf8_lossy(&record[3..]).into_owned(),
            untracked: record.starts_with(b"?? "),
        });
    }
    Ok(changes)
}

fn worktree_status(worktree: &Path, excludes: &[String]) -> Result<String, String> {
    // Explicit `--untracked-files=normal` so a `status.showUntrackedFiles=no`
    // config can't hide untracked work from the preservation check, and
    // `--ignore-submodules=none` so a submodule's `ignore` setting can't.
    let mut args = vec![
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--untracked-files=normal",
        "--ignore-submodules=none",
    ];
    if !excludes.is_empty() {
        args.push("--");
        args.extend(excludes.iter().map(String::as_str));
    }
    run_git_command(worktree, &args).map_err(|e| {
        format!(
            "Could not read the status of {} (nothing was deleted): {}",
            worktree.display(),
            e
        )
    })
}

/// A directory that is a git repository of its own (`.git` file or dir).
fn is_nested_repository(worktree: &Path, dir: &str) -> bool {
    worktree.join(dir).join(".git").exists()
}

/// Decides which untracked files the auto-save stash takes and returns the
/// exclude pathspecs for the rest: regenerable directories (see
/// `git_tracker::REGENERABLE_DIRS`) inside wholly untracked directories.
/// Refuses, before anything is stashed, what the stash can't hold: nested
/// repositories (stash skips them) and an untracked set too large to hash.
fn plan_untracked_stash(
    worktree: &Path,
    untracked_dirs: &[String],
    limits: StashLimits,
) -> Result<Vec<String>, String> {
    let mut excluded: BTreeSet<String> = untracked_dirs
        .iter()
        .filter(|dir| is_regenerable_dir(dir) && !is_nested_repository(worktree, dir))
        .cloned()
        .collect();

    // What `stash --include-untracked` would take once the top-level
    // regenerable directories are out (git doesn't walk those).
    let top_level: Vec<String> = excluded.iter().map(|dir| exclude_pathspec(dir)).collect();
    let mut args = vec!["ls-files", "--others", "--exclude-standard", "-z"];
    if !top_level.is_empty() {
        args.push("--");
        args.extend(top_level.iter().map(String::as_str));
    }
    let listing = run_git_bytes(worktree, &args).map_err(|e| {
        format!(
            "Could not list the untracked files of {} (nothing was deleted): {}",
            worktree.display(),
            e
        )
    })?;

    let mut nested_repositories = Vec::new();
    let mut files = 0usize;
    let mut bytes = 0u64;
    for record in listing.split(|&byte| byte == 0) {
        if record.is_empty() {
            continue;
        }
        let path = String::from_utf8_lossy(record);
        if let Some(dir) = regenerable_ancestor(&path, untracked_dirs) {
            if !is_nested_repository(worktree, dir) {
                excluded.insert(dir.to_string());
                continue;
            }
        }
        // `ls-files` lists a nested repository as `dir/`; stash skips it.
        if path.ends_with('/') {
            nested_repositories.push(path.trim_end_matches('/').to_string());
            continue;
        }
        files += 1;
        bytes += std::fs::symlink_metadata(worktree.join(path.as_ref()))
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if files > limits.files || bytes > limits.bytes {
            return Err(format!(
                "Could not delete {}: it has more untracked files than can be auto-saved (over {} files or {} MB). Commit what you need, add generated files to .gitignore or remove them, then delete the pair again (nothing was deleted or stashed).",
                worktree.display(),
                limits.files,
                limits.bytes / (1024 * 1024)
            ));
        }
    }

    if !nested_repositories.is_empty() {
        return Err(format!(
            "Could not delete {}: it contains a nested git repository ({}) that git stash can't save. Move it out of the worktree or remove it, then delete the pair again (nothing was deleted or stashed).",
            worktree.display(),
            nested_repositories.join(", ")
        ));
    }
    if excluded.len() > MAX_STASH_EXCLUDES {
        return Err(format!(
            "Could not delete {}: it has {} untracked dependency or build directories (such as node_modules). Add them to .gitignore or remove them, then delete the pair again (nothing was deleted or stashed).",
            worktree.display(),
            excluded.len()
        ));
    }

    Ok(excluded.iter().map(|dir| exclude_pathspec(dir)).collect())
}

fn stash_tip(directory: &Path) -> Option<String> {
    run_git_command(
        directory,
        &["rev-parse", "--verify", "--quiet", "refs/stash"],
    )
    .ok()
    .filter(|sha| !sha.is_empty())
}

fn has_git_identity(directory: &Path) -> bool {
    run_git_command(directory, &["var", "GIT_COMMITTER_IDENT"]).is_ok()
        && run_git_command(directory, &["var", "GIT_AUTHOR_IDENT"]).is_ok()
}

/// Puts the auto-save stash just made back into the worktree, so a failed
/// delete leaves no stash entry behind to pile up on every retry.
fn undo_auto_save(worktree: &Path, message: &str) -> String {
    match run_git_command(worktree, &["stash", "pop", "--index", "--quiet"]) {
        Ok(_) => "The auto-save was undone (nothing was deleted).".to_string(),
        Err(error) => format!(
            "Undoing the auto-save failed too ({}); the changes are kept in the stash entry \"{}\" (nothing was deleted).",
            error, message
        ),
    }
}

fn stash_worktree_changes(
    worktree: &Path,
    dir_name: &str,
    excludes: &[String],
    pending: &[ChangedPath],
) -> Result<(), String> {
    let message = format!("the-pair: auto-saved from pair worktree {}", dir_name);
    let before = stash_tip(worktree);

    let mut args: Vec<&str> = Vec::new();
    if !has_git_identity(worktree) {
        args.extend(FALLBACK_IDENTITY);
    }
    args.extend([
        "stash",
        "push",
        "--include-untracked",
        "-m",
        message.as_str(),
    ]);
    if !excludes.is_empty() {
        args.push("--");
        args.extend(excludes.iter().map(String::as_str));
    }

    let result = run_git_command(worktree, &args);
    let after = stash_tip(worktree);
    let recorded = after.is_some() && after != before;

    if let Err(error) = result {
        // stash can fail after storing its entry (e.g. while cleaning).
        if recorded {
            return Err(format!(
                "Could not save the uncommitted changes in {}: {}. {}",
                worktree.display(),
                error,
                undo_auto_save(worktree, &message)
            ));
        }
        return Err(format!(
            "Could not save the uncommitted changes in {} (nothing was deleted): {}",
            worktree.display(),
            error
        ));
    }
    if !recorded {
        return Err(format!(
            "git stash did not record the uncommitted changes in {}; nothing was deleted",
            worktree.display()
        ));
    }

    // Anything stash didn't capture must not be deleted with the directory.
    // Untracked files that only show up now were ignored before the stash
    // reverted a `.gitignore` edit; ignored files are never preserved.
    let was_untracked: HashSet<&str> = pending
        .iter()
        .filter(|change| change.untracked)
        .map(|change| change.path.as_str())
        .collect();
    let unsaved: Vec<String> = changed_paths(worktree, excludes)?
        .into_iter()
        .filter(|change| !change.untracked || was_untracked.contains(change.path.as_str()))
        .map(|change| change.path)
        .collect();
    if !unsaved.is_empty() {
        return Err(format!(
            "Some changes in {} could not be stashed ({}). {}",
            worktree.display(),
            unsaved.join(", "),
            undo_auto_save(worktree, &message)
        ));
    }

    println!(
        "[worktree_manager] Saved uncommitted changes of {} to the stash as \"{}\"",
        worktree.display(),
        message
    );
    Ok(())
}

/// True when `sha` is contained in some branch, remote-tracking branch or tag
/// other than `exclude_ref`. Errors count as "not reachable" so callers err on
/// the side of preserving.
fn commit_reachable_from_other_refs(
    directory: &Path,
    sha: &str,
    exclude_ref: Option<&str>,
) -> bool {
    match run_git_command(
        directory,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "--contains",
            sha,
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    ) {
        Ok(output) => output
            .lines()
            .map(str::trim)
            .any(|refname| !refname.is_empty() && Some(refname) != exclude_ref),
        Err(_) => false,
    }
}

/// Creates `the-pair/rescued-<short-sha>` at `sha` unless the commit is already
/// reachable from a branch or tag. Returns the rescue branch name when created
/// (or already present at `sha`).
fn rescue_commit_if_unreachable(directory: &Path, sha: &str) -> Result<Option<String>, String> {
    if commit_reachable_from_other_refs(directory, sha, None) {
        return Ok(None);
    }

    let short = run_git_command(directory, &["rev-parse", "--short=12", sha])
        .unwrap_or_else(|_| sha.chars().take(12).collect());
    let branch = format!("{}{}", RESCUE_BRANCH_PREFIX, short);
    let full_ref = format!("refs/heads/{}", branch);

    if let Ok(existing) =
        run_git_command(directory, &["rev-parse", "--verify", "--quiet", &full_ref])
    {
        if existing == sha {
            return Ok(Some(branch));
        }
        return Err(format!(
            "Could not create rescue branch '{}' for commit {}: the branch already exists (nothing was deleted)",
            branch, sha
        ));
    }

    // The empty old-value makes update-ref refuse to overwrite an existing ref.
    run_git_command(
        directory,
        &[
            "update-ref",
            "-m",
            "the-pair: rescue commits from a detached pair worktree",
            &full_ref,
            sha,
            "",
        ],
    )
    .map_err(|e| {
        format!(
            "Could not create rescue branch '{}' for commit {} (nothing was deleted): {}",
            branch, sha, e
        )
    })?;

    println!(
        "[worktree_manager] Rescued unreachable commit {} as branch {}",
        sha, branch
    );
    Ok(Some(branch))
}

fn remove_linked_worktree(
    common_dir: &Path,
    path: &Path,
    excludes: &[String],
) -> Result<(), String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let absolute_str = absolute.to_string_lossy().to_string();

    // Everything worth keeping is in a branch or the stash by now. The double
    // `--force` also removes locked worktrees. Run from the main repository so
    // git doesn't operate from inside the directory it is deleting.
    let result = run_git_command(
        common_dir,
        &["worktree", "remove", "--force", "--force", &absolute_str],
    );
    if !path.exists() {
        return Ok(());
    }

    // git can refuse (e.g. worktrees containing submodules). Fall back to
    // deleting the directory only while git still reports it clean, so nothing
    // that wasn't preserved is lost.
    let git_error = result.err().unwrap_or_default();
    match worktree_status(path, excludes) {
        Ok(status) if status.is_empty() => std::fs::remove_dir_all(path).map_err(|e| {
            format!(
                "git worktree remove failed ({}) and directory cleanup also failed ({})",
                git_error, e
            )
        }),
        _ => Err(format!(
            "Failed to remove worktree {}: {}",
            absolute_str, git_error
        )),
    }
}

struct WorktreeEntry {
    path: PathBuf,
    head: Option<String>,
    branch: Option<String>,
}

/// Every worktree git knows (the main one first), or `None` when git fails.
fn list_worktree_entries(common_dir: &Path) -> Option<Vec<WorktreeEntry>> {
    let output = run_git_command(common_dir, &["worktree", "list", "--porcelain"]).ok()?;
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for line in output.lines() {
        if let Some(worktree) = line.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: PathBuf::from(worktree),
                head: None,
                branch: None,
            });
        } else if let Some(entry) = current.as_mut() {
            if let Some(head) = line.strip_prefix("HEAD ") {
                entry.head = Some(head.trim().to_string());
            } else if let Some(branch) = line.strip_prefix("branch ") {
                entry.branch = Some(branch.trim().to_string());
            }
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    Some(entries)
}

/// Drops git's entry for the worktree at `path` once its directory is gone,
/// first rescuing a detached HEAD (the entry is the only reference to those
/// commits). Returns the removed entry. Only this one entry is touched: a
/// repository-wide `git worktree prune` would also unregister the user's own
/// worktrees whose directories are merely unavailable (an unmounted volume).
fn forget_missing_worktree(common_dir: &Path, path: &Path) -> Option<WorktreeEntry> {
    let entry = list_worktree_entries(common_dir)?
        .into_iter()
        .skip(1) // the main worktree
        .find(|entry| same_path(&entry.path, path) && !entry.path.exists())?;

    if entry.branch.is_none() {
        if let Some(head) = &entry.head {
            if let Err(error) = rescue_commit_if_unreachable(common_dir, head) {
                println!(
                    "[worktree_manager] Keeping the entry of {} so commit {} stays reachable: {}",
                    path.display(),
                    head,
                    error
                );
                return None;
            }
        }
    }

    // `remove` accepts a missing directory. The path is passed exactly as git
    // recorded it, which git matches even when it can no longer resolve it.
    let registered = entry.path.to_string_lossy().to_string();
    run_git_command(
        common_dir,
        &["worktree", "remove", "--force", "--force", &registered],
    )
    .ok()?;
    Some(entry)
}

/// Deletes the pair's own `the-pair/<dir>` branch once its worktree is gone,
/// but only when every commit on it is also on another branch/tag (no new
/// commits, or already merged). Otherwise the branch stays as the record of the
/// executor's work.
fn delete_pair_branch_if_redundant(
    common_dir: &Path,
    branch_ref: &str,
    head: &str,
    dir_name: &str,
) {
    let expected = format!("refs/heads/{}", pair_branch_name(dir_name));
    if branch_ref != expected {
        return;
    }
    if !commit_reachable_from_other_refs(common_dir, head, Some(branch_ref)) {
        return;
    }
    // The old-value guard only deletes the branch if it still points at `head`.
    let _ = run_git_command(
        common_dir,
        &[
            "update-ref",
            "-m",
            "the-pair: remove pair branch without new commits",
            "-d",
            branch_ref,
            head,
        ],
    );
}

/// Creates (if needed) a local branch tracking `remote_branch` (e.g.
/// `origin/feature` → `feature`) and returns its name. Works purely on the
/// remote-tracking ref that is already present locally — no network fetch.
pub fn ensure_local_tracking_branch(
    repo_path: &str,
    remote_branch: &str,
) -> Result<String, String> {
    if remote_branch.trim().is_empty() || remote_branch.starts_with('-') {
        return Err(format!("Invalid branch name: '{}'", remote_branch));
    }

    let remotes = list_remotes(repo_path);
    let (local_name, remote_ref) = match split_remote_ref(remote_branch, &remotes) {
        Some((_remote, branch)) => (branch.to_string(), remote_branch.to_string()),
        None => (
            remote_branch.to_string(),
            format!("origin/{}", remote_branch),
        ),
    };

    validate_branch_name(repo_path, &local_name)?;

    if ref_exists(repo_path, &format!("refs/heads/{}", local_name)) {
        return Ok(local_name);
    }

    let remote_full_ref = format!("refs/remotes/{}", remote_ref);
    if !ref_exists(repo_path, &remote_full_ref) {
        return Err(format!(
            "Failed to create local tracking branch: remote branch '{}' was not found. Fetch it and try again.",
            remote_ref
        ));
    }

    let result = run_git_command(
        repo_path,
        &["branch", "--track", "--", &local_name, &remote_full_ref],
    );

    match result {
        Ok(_) => Ok(local_name),
        Err(e) => Err(format!("Failed to create local tracking branch: {}", e)),
    }
}

/// Atomic write for plain-text files: write to a temp sibling then rename.
/// Mirrors the `write_json_atomic` pattern from `session_snapshot.rs`.
fn write_text_atomic(path: &Path, content: &str) -> Result<(), String> {
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, content).map_err(|e| format!("Failed to write file: {}", e))?;
    std::fs::rename(&tmp_path, path).map_err(|e| format!("Failed to move file into place: {}", e))
}

/// Path of `info/exclude` as git resolves it — correct when `repo_path` is a
/// subdirectory, a linked worktree (`.git` is a file) or a submodule.
fn resolve_exclude_path(repo_path: &str) -> Result<PathBuf, String> {
    rev_parse_paths(Path::new(repo_path), &["--git-path", "info/exclude"], 1)
        .and_then(|mut paths| paths.pop())
        .ok_or_else(|| "Failed to locate info/exclude".to_string())
}

/// Makes sure `.worktrees/` is ignored via the repository's `info/exclude`.
/// Returns `Ok(true)` when the entry was added, `Ok(false)` when present.
pub fn ensure_gitignore_worktrees(repo_path: &str) -> Result<bool, String> {
    let exclude_path = resolve_exclude_path(repo_path)?;
    let entry = ".worktrees/";

    if exclude_path.exists() {
        let content = std::fs::read_to_string(&exclude_path)
            .map_err(|e| format!("Failed to read info/exclude: {}", e))?;
        if content
            .lines()
            .any(|line| matches!(line.trim(), ".worktrees/" | ".worktrees"))
        {
            return Ok(false);
        }
        let new_content = if content.is_empty() || content.ends_with('\n') {
            format!("{}{}\n", content, entry)
        } else {
            format!("{}\n{}\n", content, entry)
        };
        write_text_atomic(&exclude_path, &new_content)
            .map_err(|e| format!("Failed to update info/exclude: {}", e))?;
        return Ok(true);
    }

    if let Some(parent) = exclude_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create info directory: {}", e))?;
    }
    write_text_atomic(&exclude_path, &format!("{}\n", entry))
        .map_err(|e| format!("Failed to write info/exclude: {}", e))?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Throwaway repository under the system temp dir, removed on drop.
    struct TempRepo {
        root: PathBuf,
        repo: PathBuf,
    }

    impl TempRepo {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "the-pair-worktree-test-{}-{}-{}",
                label,
                std::process::id(),
                nanos
            ));
            let repo = root.join("repo");
            fs::create_dir_all(&repo).unwrap();
            let repo = repo.canonicalize().unwrap();
            let temp = TempRepo { root, repo };
            temp.git(&temp.repo, &["init", "-q"]);
            temp.git(&temp.repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
            temp.git(&temp.repo, &["config", "user.name", "Test"]);
            temp.git(&temp.repo, &["config", "user.email", "test@example.com"]);
            temp.git(&temp.repo, &["config", "commit.gpgsign", "false"]);
            fs::write(temp.repo.join("README.md"), "hello\n").unwrap();
            temp.git(&temp.repo, &["add", "README.md"]);
            temp.commit(&temp.repo, "init");
            temp
        }

        fn git(&self, dir: &Path, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(dir)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .output()
                .expect("git runs");
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        fn commit(&self, dir: &Path, message: &str) -> String {
            self.git(dir, &["commit", "-q", "--no-verify", "-m", message]);
            self.git(dir, &["rev-parse", "HEAD"])
        }

        fn repo_str(&self) -> &str {
            self.repo.to_str().unwrap()
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn create_worktree_uses_a_named_pair_branch_without_upstream() {
        let temp = TempRepo::new("create");
        temp.git(&temp.repo, &["branch", "feature"]);

        let path = create_worktree(temp.repo_str(), "feature", ".worktrees/pair-abc")
            .expect("worktree created");
        let path = PathBuf::from(path);
        assert!(path.join("README.md").exists());

        let head = temp.git(&path, &["symbolic-ref", "HEAD"]);
        assert_eq!(head, "refs/heads/the-pair/pair-abc");
        assert_eq!(
            temp.git(&path, &["rev-parse", "HEAD"]),
            temp.git(&temp.repo, &["rev-parse", "feature"])
        );
        // No upstream: the pair branch must not track the selected branch.
        let upstream = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
            .current_dir(&path)
            .output()
            .unwrap();
        assert!(!upstream.status.success());
    }

    #[test]
    fn create_worktree_starts_from_a_remote_tracking_branch() {
        let temp = TempRepo::new("create-remote");
        let sha = temp.git(&temp.repo, &["rev-parse", "HEAD"]);
        temp.git(
            &temp.repo,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/repo.git",
            ],
        );
        temp.git(
            &temp.repo,
            &["update-ref", "refs/remotes/origin/feature", &sha],
        );

        let path = create_worktree(temp.repo_str(), "origin/feature", ".worktrees/pair-remote")
            .expect("worktree created from remote-tracking branch");
        assert_eq!(
            temp.git(Path::new(&path), &["symbolic-ref", "HEAD"]),
            "refs/heads/the-pair/pair-remote"
        );
    }

    #[test]
    fn create_worktree_rejects_option_like_or_unknown_branches() {
        let temp = TempRepo::new("create-invalid");
        let before = temp.git(&temp.repo, &["for-each-ref", "--format=%(refname)"]);

        assert!(create_worktree(temp.repo_str(), "-D", ".worktrees/pair-x").is_err());
        assert!(create_worktree(temp.repo_str(), "does-not-exist", ".worktrees/pair-y").is_err());

        assert_eq!(
            temp.git(&temp.repo, &["for-each-ref", "--format=%(refname)"]),
            before
        );
        assert!(!temp.repo.join(".worktrees/pair-x").exists());
        assert_eq!(temp.git(&temp.repo, &["branch", "--show-current"]), "main");
    }

    #[test]
    fn delete_worktree_preserves_commits_and_uncommitted_changes() {
        let temp = TempRepo::new("delete-preserve");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(
            create_worktree(temp.repo_str(), "feature", ".worktrees/pair-keep").unwrap(),
        );

        fs::write(path.join("committed.txt"), "committed work\n").unwrap();
        temp.git(&path, &["add", "committed.txt"]);
        let commit = temp.commit(&path, "executor work");

        fs::write(path.join("README.md"), "hello\nmodified\n").unwrap();
        fs::create_dir_all(path.join("new dir")).unwrap();
        fs::write(
            path.join("new dir").join("untracked file.txt"),
            "untracked\n",
        )
        .unwrap();

        delete_worktree(path.to_str().unwrap()).expect("delete succeeds");
        assert!(!path.exists());

        // The executor's commit stays reachable from the pair branch.
        let containing = temp.git(&temp.repo, &["branch", "--contains", &commit]);
        assert!(containing.contains("the-pair/pair-keep"), "{}", containing);

        // Uncommitted tracked and untracked changes are in the stash.
        let stashes = temp.git(&temp.repo, &["stash", "list"]);
        assert!(
            stashes.contains("the-pair: auto-saved from pair worktree pair-keep"),
            "{}",
            stashes
        );
        let tracked = temp.git(
            &temp.repo,
            &["diff", "--name-only", "stash@{0}^1", "stash@{0}"],
        );
        assert!(tracked.contains("README.md"), "{}", tracked);
        let untracked = temp.git(&temp.repo, &["ls-tree", "-r", "--name-only", "stash@{0}^3"]);
        assert!(
            untracked.contains("new dir/untracked file.txt"),
            "{}",
            untracked
        );

        // Worktree metadata is gone from the main repository.
        let worktrees = temp.git(&temp.repo, &["worktree", "list", "--porcelain"]);
        assert!(!worktrees.contains("pair-keep"), "{}", worktrees);
    }

    #[test]
    fn delete_worktree_drops_the_pair_branch_when_it_adds_nothing() {
        let temp = TempRepo::new("delete-clean");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = create_worktree(temp.repo_str(), "feature", ".worktrees/pair-clean").unwrap();

        delete_worktree(&path).expect("delete succeeds");

        assert!(!Path::new(&path).exists());
        let branches = temp.git(
            &temp.repo,
            &["for-each-ref", "--format=%(refname)", "refs/heads"],
        );
        assert!(!branches.contains("the-pair/pair-clean"), "{}", branches);
        assert!(temp.git(&temp.repo, &["stash", "list"]).is_empty());
    }

    #[test]
    fn delete_worktree_rescues_detached_head_commits() {
        let temp = TempRepo::new("delete-detached");
        let path = temp.repo.join(".worktrees").join("pair-legacy");
        temp.git(
            &temp.repo,
            &[
                "worktree",
                "add",
                "--detach",
                path.to_str().unwrap(),
                "main",
            ],
        );
        fs::write(path.join("work.txt"), "detached work\n").unwrap();
        temp.git(&path, &["add", "work.txt"]);
        let commit = temp.commit(&path, "detached commit");
        fs::write(path.join("dirty.txt"), "dirty\n").unwrap();

        delete_worktree(path.to_str().unwrap()).expect("delete succeeds");
        assert!(!path.exists());

        let short = temp.git(&temp.repo, &["rev-parse", "--short=12", &commit]);
        let rescued = temp.git(
            &temp.repo,
            &[
                "rev-parse",
                &format!("refs/heads/the-pair/rescued-{}", short),
            ],
        );
        assert_eq!(rescued, commit);
        let stashes = temp.git(&temp.repo, &["stash", "list"]);
        assert!(stashes.contains("pair-legacy"), "{}", stashes);
    }

    #[test]
    fn delete_worktree_with_missing_directory_prunes_and_rescues() {
        let temp = TempRepo::new("delete-missing");
        let path = temp.repo.join(".worktrees").join("pair-gone");
        temp.git(
            &temp.repo,
            &[
                "worktree",
                "add",
                "--detach",
                path.to_str().unwrap(),
                "main",
            ],
        );
        fs::write(path.join("work.txt"), "work\n").unwrap();
        temp.git(&path, &["add", "work.txt"]);
        let commit = temp.commit(&path, "commit before the dir vanished");
        fs::remove_dir_all(&path).unwrap();

        delete_worktree(path.to_str().unwrap()).expect("missing worktree is Ok");

        let worktrees = temp.git(&temp.repo, &["worktree", "list", "--porcelain"]);
        assert!(!worktrees.contains("pair-gone"), "{}", worktrees);
        let containing = temp.git(&temp.repo, &["branch", "--contains", &commit]);
        assert!(containing.contains("the-pair/rescued-"), "{}", containing);

        // Deleting again is still Ok.
        delete_worktree(path.to_str().unwrap()).expect("idempotent");
    }

    #[test]
    fn delete_worktree_removes_nothing_when_changes_cannot_be_saved() {
        let temp = TempRepo::new("delete-fail");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(
            create_worktree(temp.repo_str(), "feature", ".worktrees/pair-locked").unwrap(),
        );
        fs::write(path.join("README.md"), "unsaved edit\n").unwrap();

        // A stale index.lock makes `git stash` fail.
        let git_dir = path.join(temp.git(&path, &["rev-parse", "--git-dir"]));
        fs::write(git_dir.join("index.lock"), "").unwrap();

        let error = delete_worktree(path.to_str().unwrap()).expect_err("must refuse");
        assert!(error.contains("nothing was deleted"), "{}", error);
        assert_eq!(
            fs::read_to_string(path.join("README.md")).unwrap(),
            "unsaved edit\n"
        );
        let worktrees = temp.git(&temp.repo, &["worktree", "list", "--porcelain"]);
        assert!(worktrees.contains("pair-locked"), "{}", worktrees);
    }

    #[test]
    fn delete_worktree_refuses_directories_that_are_not_linked_worktrees() {
        let temp = TempRepo::new("delete-refuse");
        let plain = temp.repo.join(".worktrees").join("pair-plain");
        fs::create_dir_all(&plain).unwrap();
        fs::write(plain.join("keep.txt"), "user data\n").unwrap();

        assert!(delete_worktree(plain.to_str().unwrap()).is_err());
        assert!(plain.join("keep.txt").exists());

        assert!(delete_worktree(temp.repo_str()).is_err());
        assert!(temp.repo.join("README.md").exists());

        // An empty leftover directory is simply removed.
        let empty = temp.repo.join(".worktrees").join("pair-empty");
        fs::create_dir_all(&empty).unwrap();
        delete_worktree(empty.to_str().unwrap()).expect("empty dir removed");
        assert!(!empty.exists());
    }

    fn pair_dir() -> String {
        format!(".worktrees/pair-{}", uuid::Uuid::new_v4())
    }

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn stash_list(temp: &TempRepo) -> String {
        temp.git(&temp.repo, &["stash", "list"])
    }

    #[test]
    fn delete_worktree_leaves_dependency_directories_out_of_the_stash() {
        let temp = TempRepo::new("delete-deps");
        write(&temp.repo.join("build/tracked.sh"), "echo\n");
        temp.git(&temp.repo, &["add", "build"]);
        temp.commit(&temp.repo, "build scripts");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());

        for index in 0..300 {
            write(
                &path.join(format!("node_modules/pkg{}/f{index}.js", index % 10)),
                "module.exports = 1\n",
            );
        }
        write(&path.join("newpkg/src/index.js"), "export {}\n");
        write(&path.join("newpkg/node_modules/dep/index.js"), "x\n");
        write(&path.join("newpkg/.venv/lib/site.py"), "x\n");
        write(&path.join("notes.txt"), "notes\n");
        // `build/` holds tracked files: new files there are real work.
        write(&path.join("build/new.sh"), "echo new\n");
        write(&path.join("build/tracked.sh"), "echo changed\n");

        delete_worktree(path.to_str().unwrap()).expect("delete succeeds");
        assert!(!path.exists());

        let untracked = temp.git(&temp.repo, &["ls-tree", "-r", "--name-only", "stash@{0}^3"]);
        let mut saved: Vec<&str> = untracked.lines().collect();
        saved.sort();
        assert_eq!(
            saved,
            vec!["build/new.sh", "newpkg/src/index.js", "notes.txt"]
        );
        let tracked = temp.git(
            &temp.repo,
            &["diff", "--name-only", "stash@{0}^1", "stash@{0}"],
        );
        assert_eq!(tracked, "build/tracked.sh");
    }

    #[test]
    fn delete_worktree_without_changes_beyond_dependencies_needs_no_stash() {
        let temp = TempRepo::new("delete-deps-only");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        write(&path.join(".venv/bin/python"), "#!/bin/sh\n");

        delete_worktree(path.to_str().unwrap()).expect("delete succeeds");
        assert!(!path.exists());
        assert!(stash_list(&temp).is_empty());
    }

    #[test]
    fn status_v2_parsing_finds_untracked_dirs_and_changed_submodules() {
        let raw = [
            "1 .M N... 100644 100644 100644 aaa aaa src/a b.rs",
            "2 R. N... 100644 100644 100644 aaa aaa R100 new name.rs",
            "old name.rs",
            "1 .M S.M. 160000 160000 160000 bbb bbb vendor/sub",
            "? newpkg/",
            "? notes.txt",
            "",
        ]
        .join("\0");
        let status = parse_worktree_status_v2(raw.as_bytes());
        assert!(!status.clean);
        assert_eq!(status.untracked_dirs, vec!["newpkg".to_string()]);
        assert_eq!(status.changed_submodules, vec!["vendor/sub".to_string()]);
        assert!(parse_worktree_status_v2(b"").clean);
    }

    #[test]
    fn untracked_stash_plan_refuses_sets_too_large_to_hash() {
        let temp = TempRepo::new("stash-limits");
        for index in 0..6 {
            write(&temp.repo.join(format!("data/f{index}.bin")), "0123456789");
        }
        write(&temp.repo.join("node_modules/a/big.js"), &"x".repeat(1000));
        let untracked = vec!["data".to_string(), "node_modules".to_string()];

        let roomy = StashLimits {
            files: 6,
            bytes: 60,
        };
        let excludes = plan_untracked_stash(&temp.repo, &untracked, roomy).expect("fits");
        assert_eq!(excludes, vec![exclude_pathspec("node_modules")]);

        let few_files = StashLimits {
            files: 5,
            bytes: 1 << 20,
        };
        let error = plan_untracked_stash(&temp.repo, &untracked, few_files).unwrap_err();
        assert!(error.contains("more untracked files"), "{}", error);
        assert!(error.contains("nothing was deleted"), "{}", error);

        let few_bytes = StashLimits {
            files: 100,
            bytes: 59,
        };
        assert!(plan_untracked_stash(&temp.repo, &untracked, few_bytes).is_err());
    }

    #[test]
    fn delete_worktree_refuses_nested_repositories_before_stashing() {
        let temp = TempRepo::new("delete-nested");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        write(&path.join("README.md"), "edited\n");
        write(&path.join("newdir/file.txt"), "new\n");
        write(&path.join("newdir/inner/lib.rs"), "fn f() {}\n");
        temp.git(&path.join("newdir/inner"), &["init", "-q"]);

        let error = delete_worktree(path.to_str().unwrap()).expect_err("must refuse");
        assert!(error.contains("nested git repository"), "{}", error);
        assert!(error.contains("newdir/inner"), "{}", error);
        assert!(error.contains("nothing was deleted"), "{}", error);

        // Nothing stashed, nothing removed: a retry doesn't pile up entries.
        assert!(stash_list(&temp).is_empty());
        assert_eq!(
            fs::read_to_string(path.join("README.md")).unwrap(),
            "edited\n"
        );
        assert!(path.join("newdir/file.txt").exists());
        let worktrees = temp.git(&temp.repo, &["worktree", "list", "--porcelain"]);
        assert!(worktrees.contains(path.file_name().unwrap().to_str().unwrap()));
    }

    #[test]
    fn delete_worktree_refuses_submodule_changes_before_stashing() {
        let temp = TempRepo::new("delete-submodule");
        let source = temp.root.join("subsrc");
        fs::create_dir_all(&source).unwrap();
        temp.git(&source, &["init", "-q"]);
        write(&source.join("lib.txt"), "lib\n");
        temp.git(&source, &["add", "lib.txt"]);
        temp.commit(&source, "lib");
        temp.git(
            &temp.repo,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                source.to_str().unwrap(),
                "sub",
            ],
        );
        temp.commit(&temp.repo, "add submodule");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        temp.git(
            &path,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "update",
                "--init",
                "-q",
            ],
        );
        write(&path.join("sub/lib.txt"), "changed inside the submodule\n");
        write(&path.join("notes.txt"), "notes\n");

        let error = delete_worktree(path.to_str().unwrap()).expect_err("must refuse");
        assert!(error.contains("submodule 'sub'"), "{}", error);
        assert!(stash_list(&temp).is_empty());
        assert_eq!(
            fs::read_to_string(path.join("sub/lib.txt")).unwrap(),
            "changed inside the submodule\n"
        );
        assert!(path.join("notes.txt").exists());
    }

    #[test]
    fn delete_worktree_tolerates_ignored_files_a_gitignore_edit_reveals() {
        let temp = TempRepo::new("delete-gitignore");
        write(&temp.repo.join(".gitignore"), "*.log\n");
        temp.git(&temp.repo, &["add", ".gitignore"]);
        temp.commit(&temp.repo, "ignore logs");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        // Stashing reverts `.gitignore`, which makes `.env` show up untracked.
        write(&path.join(".gitignore"), "*.log\n.env\n");
        write(&path.join(".env"), "SECRET=1\n");

        delete_worktree(path.to_str().unwrap()).expect("delete succeeds");
        assert!(!path.exists());
        let tracked = temp.git(
            &temp.repo,
            &["diff", "--name-only", "stash@{0}^1", "stash@{0}"],
        );
        assert_eq!(tracked, ".gitignore");
    }

    #[test]
    fn undo_auto_save_puts_the_changes_back() {
        let temp = TempRepo::new("undo-stash");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        write(&path.join("README.md"), "staged\n");
        temp.git(&path, &["add", "README.md"]);
        write(&path.join("README.md"), "staged\nand unstaged\n");
        write(&path.join("new.txt"), "untracked\n");
        temp.git(
            &path,
            &[
                "stash",
                "push",
                "-q",
                "--include-untracked",
                "-m",
                "the-pair: t",
            ],
        );

        let outcome = undo_auto_save(&path, "the-pair: t");
        assert!(outcome.contains("undone"), "{}", outcome);
        assert!(stash_list(&temp).is_empty());
        assert_eq!(
            temp.git(&path, &["diff", "--cached", "--name-only"]),
            "README.md"
        );
        assert_eq!(
            fs::read_to_string(path.join("README.md")).unwrap(),
            "staged\nand unstaged\n"
        );
        assert!(path.join("new.txt").exists());
    }

    #[test]
    fn delete_worktree_finishes_a_removal_git_left_half_done() {
        let temp = TempRepo::new("delete-leftover");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        let name = path.file_name().unwrap().to_str().unwrap().to_string();

        // What a part-failed `git worktree remove` leaves: git's entry is gone,
        // the directory (with its `.git` link) is not.
        fs::remove_dir_all(temp.repo.join(".git/worktrees").join(&name)).unwrap();
        write(&path.join("locked.bin"), "x");
        assert!(!temp
            .git(&temp.repo, &["worktree", "list", "--porcelain"])
            .contains(&name));

        delete_worktree(path.to_str().unwrap()).expect("leftover removed");
        assert!(!path.exists());
        // The pair branch added nothing, so it went too.
        let branches = temp.git(&temp.repo, &["for-each-ref", "--format=%(refname)"]);
        assert!(!branches.contains(&name), "{}", branches);

        // A `.git` link into another repository is not ours to remove.
        let foreign = temp.repo.join(pair_dir());
        write(
            &foreign.join(".git"),
            "gitdir: /elsewhere/repo/.git/worktrees/pair-x\n",
        );
        write(&foreign.join("work.txt"), "keep\n");
        assert!(delete_worktree(foreign.to_str().unwrap()).is_err());
        assert!(foreign.join("work.txt").exists());

        // Nor is a leftover-looking directory git still lists.
        let listed =
            PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        fs::remove_file(listed.join(".git")).unwrap();
        assert!(delete_worktree(listed.to_str().unwrap()).is_err());
        assert!(listed.join("README.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_worktree_can_be_retried_after_git_removed_part_of_the_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempRepo::new("delete-partial");
        temp.git(&temp.repo, &["branch", "feature"]);
        let path = PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        write(&path.join("locked/file.txt"), "work\n");
        temp.git(&path, &["add", "locked"]);
        let commit = temp.commit(&path, "work");

        // A directory whose entries can't be unlinked stops `worktree remove`
        // part-way (like a file another program holds open on Windows).
        let locked = path.join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
        let first = delete_worktree(path.to_str().unwrap());
        if locked.exists() {
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        }
        if first.is_ok() {
            // Running as root: nothing could stop the removal.
            return;
        }
        assert!(path.exists());
        assert!(!temp
            .git(&temp.repo, &["worktree", "list", "--porcelain"])
            .contains(&name));

        delete_worktree(path.to_str().unwrap()).expect("retry succeeds");
        assert!(!path.exists());
        let containing = temp.git(&temp.repo, &["branch", "--contains", &commit]);
        assert!(containing.contains(&name), "{}", containing);
    }

    #[test]
    fn delete_worktree_leaves_other_missing_worktrees_registered() {
        let temp = TempRepo::new("delete-no-prune");
        // The user's own worktree, on a volume that is not mounted right now.
        let offline = temp.root.join("offline-worktree");
        temp.git(
            &temp.repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                offline.to_str().unwrap(),
                "main",
            ],
        );
        fs::remove_dir_all(&offline).unwrap();

        temp.git(&temp.repo, &["branch", "feature"]);
        let present =
            PathBuf::from(create_worktree(temp.repo_str(), "feature", &pair_dir()).unwrap());
        delete_worktree(present.to_str().unwrap()).expect("delete succeeds");

        // A pair worktree whose directory is gone loses only its own entry.
        temp.git(&temp.repo, &["branch", "feature2"]);
        let missing =
            PathBuf::from(create_worktree(temp.repo_str(), "feature2", &pair_dir()).unwrap());
        fs::remove_dir_all(&missing).unwrap();
        delete_worktree(missing.to_str().unwrap()).expect("missing worktree is Ok");

        let worktrees = temp.git(&temp.repo, &["worktree", "list", "--porcelain"]);
        assert!(worktrees.contains("offline-worktree"), "{}", worktrees);
        for gone in [&present, &missing] {
            let name = gone.file_name().unwrap().to_str().unwrap();
            assert!(!worktrees.contains(name), "{}", worktrees);
        }
    }

    #[test]
    fn list_branches_parses_nul_fields_and_skips_symbolic_and_option_refs() {
        let temp = TempRepo::new("branches");
        fs::write(temp.repo.join("a.txt"), "a\n").unwrap();
        temp.git(&temp.repo, &["add", "a.txt"]);
        let sha = temp.commit(&temp.repo, "fix: a | b | c");
        temp.git(
            &temp.repo,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/repo.git",
            ],
        );
        temp.git(
            &temp.repo,
            &["update-ref", "refs/remotes/origin/feature", &sha],
        );
        temp.git(
            &temp.repo,
            &["update-ref", "refs/remotes/origin/main", &sha],
        );
        temp.git(&temp.repo, &["update-ref", "refs/remotes/origin/-M", &sha]);
        temp.git(
            &temp.repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );

        let branches = list_branches(temp.repo_str()).unwrap();
        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["main", "origin/feature", "origin/main"]);

        let main = &branches[0];
        assert!(main.is_local && main.is_checked_out_locally);
        assert_eq!(main.last_commit_message.as_deref(), Some("fix: a | b | c"));
        assert!(main.last_commit_date.unwrap_or(0) > 0);

        let origin_main = &branches[2];
        assert!(origin_main.is_remote && origin_main.is_checked_out_locally);
        assert!(!branches[1].is_checked_out_locally);
    }

    #[test]
    fn ensure_local_tracking_branch_uses_existing_ref_and_rejects_options() {
        let temp = TempRepo::new("tracking");
        let sha = temp.git(&temp.repo, &["rev-parse", "HEAD"]);
        temp.git(
            &temp.repo,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/repo.git",
            ],
        );
        temp.git(
            &temp.repo,
            &["update-ref", "refs/remotes/origin/feature/x", &sha],
        );
        temp.git(&temp.repo, &["update-ref", "refs/remotes/origin/-M", &sha]);

        // No fetch: the unreachable remote URL would fail if one were attempted.
        let local = ensure_local_tracking_branch(temp.repo_str(), "origin/feature/x").unwrap();
        assert_eq!(local, "feature/x");
        assert_eq!(
            temp.git(&temp.repo, &["config", "branch.feature/x.remote"]),
            "origin"
        );
        assert_eq!(
            temp.git(&temp.repo, &["config", "branch.feature/x.merge"]),
            "refs/heads/feature/x"
        );
        // Second call reuses the local branch.
        assert_eq!(
            ensure_local_tracking_branch(temp.repo_str(), "origin/feature/x").unwrap(),
            "feature/x"
        );

        assert!(ensure_local_tracking_branch(temp.repo_str(), "origin/-M").is_err());
        assert!(ensure_local_tracking_branch(temp.repo_str(), "-M").is_err());
        assert!(ensure_local_tracking_branch(temp.repo_str(), "origin/missing").is_err());
        // The current branch was not renamed by option injection.
        assert_eq!(temp.git(&temp.repo, &["branch", "--show-current"]), "main");
    }

    #[test]
    fn ensure_gitignore_worktrees_writes_the_real_exclude_file() {
        let temp = TempRepo::new("exclude");
        let subdir = temp.repo.join("packages").join("app");
        fs::create_dir_all(&subdir).unwrap();

        assert!(ensure_gitignore_worktrees(subdir.to_str().unwrap()).unwrap());
        assert!(!ensure_gitignore_worktrees(subdir.to_str().unwrap()).unwrap());
        assert!(!subdir.join(".git").exists());
        let exclude = fs::read_to_string(temp.repo.join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|line| line == ".worktrees/"));

        fs::create_dir_all(subdir.join(".worktrees").join("pair-1")).unwrap();
        fs::write(subdir.join(".worktrees/pair-1/file.txt"), "x").unwrap();
        assert!(!check_is_dirty(temp.repo_str()));

        // From a linked worktree (`.git` is a file) the shared exclude is used.
        temp.git(&temp.repo, &["branch", "feature"]);
        let linked = create_worktree(temp.repo_str(), "feature", ".worktrees/pair-linked").unwrap();
        assert!(!ensure_gitignore_worktrees(&linked).unwrap());
    }

    #[test]
    fn check_is_git_repo_is_false_inside_the_git_dir() {
        let temp = TempRepo::new("is-repo");
        assert!(check_is_git_repo(temp.repo_str()));
        assert!(!check_is_git_repo(temp.repo.join(".git").to_str().unwrap()));
    }
}
