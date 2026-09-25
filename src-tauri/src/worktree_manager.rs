use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn run_git_command(directory: impl AsRef<Path>, args: &[&str]) -> Result<String, String> {
    let directory = directory.as_ref();
    println!(
        "[worktree_manager] run_git_command: dir={}, args={:?}",
        directory.display(),
        args
    );
    let output = git_command().args(args).current_dir(directory).output();

    match output {
        Ok(o) => {
            if o.status.success() {
                Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
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
/// - Directory already gone: stale worktree metadata is pruned from the main
///   repository (after rescuing any detached HEAD commit) and `Ok(())` returned.
/// - Otherwise, before anything is removed, a detached HEAD whose commits are
///   reachable from no branch/tag gets a `the-pair/rescued-<short-sha>` branch,
///   and uncommitted changes (tracked and untracked, not ignored) are saved with
///   `git stash push --include-untracked -m "the-pair: auto-saved from pair worktree <dir>"`.
///   The worktree is then removed and pruned from the main repository.
/// - If anything can't be preserved, nothing is removed and `Err` is returned.
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
            let pruned = prune_missing_worktrees(common_dir);
            if let Some(entry) = pruned.iter().find(|entry| same_path(&entry.path, path)) {
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
            // contents. Only an empty leftover directory is safe to drop.
            if dir_is_empty(path) {
                std::fs::remove_dir(path)
                    .map_err(|e| format!("Failed to remove empty worktree directory: {}", e))?;
                if let Some(common_dir) = &common_dir {
                    prune_missing_worktrees(common_dir);
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
    remove_linked_worktree(&layout.common_dir, path)?;
    prune_missing_worktrees(&layout.common_dir);

    if let (Some(branch), Some(head)) = (&preserved.branch_ref, &preserved.head) {
        delete_pair_branch_if_redundant(&layout.common_dir, branch, head, &dir_name);
    }

    Ok(())
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

    if !worktree_status(worktree)?.is_empty() {
        stash_worktree_changes(worktree, dir_name)?;
    }

    Ok(PreservedWorktree { head, branch_ref })
}

fn worktree_status(worktree: &Path) -> Result<String, String> {
    // Explicit `--untracked-files=normal` so a `status.showUntrackedFiles=no`
    // config can't hide untracked work from the preservation check.
    run_git_command(
        worktree,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=normal",
        ],
    )
    .map_err(|e| {
        format!(
            "Could not read the status of {} (nothing was deleted): {}",
            worktree.display(),
            e
        )
    })
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

fn stash_worktree_changes(worktree: &Path, dir_name: &str) -> Result<(), String> {
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

    run_git_command(worktree, &args).map_err(|e| {
        format!(
            "Could not save the uncommitted changes in {} (nothing was deleted): {}",
            worktree.display(),
            e
        )
    })?;

    let after = stash_tip(worktree);
    if after.is_none() || after == before {
        return Err(format!(
            "git stash did not record the uncommitted changes in {}; nothing was deleted",
            worktree.display()
        ));
    }

    // Anything stash can't capture (e.g. changes inside a submodule) must not
    // be deleted with the directory.
    let remaining = worktree_status(worktree)?;
    if !remaining.is_empty() {
        return Err(format!(
            "Some changes in {} could not be stashed (the rest were saved as \"{}\"); nothing was deleted. Remaining:\n{}",
            worktree.display(),
            message,
            remaining
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

fn remove_linked_worktree(common_dir: &Path, path: &Path) -> Result<(), String> {
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
    match worktree_status(path) {
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

fn list_worktree_entries(common_dir: &Path) -> Vec<WorktreeEntry> {
    let Ok(output) = run_git_command(common_dir, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
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
    entries
}

/// Runs `git worktree prune` from the main repository, first rescuing the
/// detached HEAD of every worktree whose directory is gone (prune would drop
/// the only reference to those commits). Returns the entries that were pruned;
/// if a rescue fails nothing is pruned.
fn prune_missing_worktrees(common_dir: &Path) -> Vec<WorktreeEntry> {
    let missing: Vec<WorktreeEntry> = list_worktree_entries(common_dir)
        .into_iter()
        .skip(1) // the main worktree
        .filter(|entry| !entry.path.exists())
        .collect();

    for entry in &missing {
        if entry.branch.is_some() {
            continue;
        }
        if let Some(head) = &entry.head {
            if let Err(error) = rescue_commit_if_unreachable(common_dir, head) {
                println!(
                    "[worktree_manager] Not pruning stale worktrees so commit {} stays reachable: {}",
                    head, error
                );
                return Vec::new();
            }
        }
    }

    let _ = run_git_command(common_dir, &["worktree", "prune"]);
    missing
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
