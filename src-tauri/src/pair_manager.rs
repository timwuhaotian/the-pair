use crate::acceptance::{
    build_executor_acceptance_followup_prompt, build_mentor_acceptance_prompt,
    build_mentor_acceptance_repair_prompt,
};
use crate::harness::driver::TurnDriver;
use crate::message_broker::MessageBroker;
use crate::session_snapshot::delete_pair_snapshot;
use crate::session_snapshot::persist_current_pair_snapshot;
use crate::types::{
    AssignTaskInput, CreatePairInput, Message, MessageSender, MessageType, Pair, PairStatus,
};
use crate::util::{build_mentor_planning_prompt, now_millis};
use crate::worktree_manager::{
    branch_for_worktree, check_repo_state, create_worktree, delete_worktree,
    ensure_gitignore_worktrees, ensure_local_tracking_branch, BranchInfo, RepoState,
};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};
use tauri::Manager;

pub struct PairManager {
    pairs: HashMap<String, Pair>,
}

/// Where a new pair works: the user's directory, or a fresh worktree.
#[derive(Debug, Clone)]
pub struct PairWorkspace {
    pub directory: String,
    /// Base branch the worktree was started from. `Some` only when the pair
    /// explicitly holds that branch (conflict-guarded); worktrees created from
    /// the repo's current branch record `None` so several pairs can isolate
    /// tasks off the same branch.
    pub branch: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    /// The branch the worktree is checked out on (`the-pair/…`).
    pub worktree_branch: Option<String>,
}

fn in_place_workspace(input: &CreatePairInput) -> PairWorkspace {
    PairWorkspace {
        directory: input.directory.clone(),
        branch: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
    }
}

/// How long a delete or a worktree rotation waits for an in-flight turn to
/// unwind before it goes ahead without it.
const TURN_STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// Poll interval while waiting for a turn to stop.
const TURN_STOP_POLL: Duration = Duration::from_millis(50);

/// Resolve (and, for a worktree pair, create) the pair's workspace. Runs all
/// the git work — including a possible fetch and checkout — without any app
/// lock held. `branches_in_use` is a snapshot of branches other pairs hold;
/// `PairManager::register_pair` re-checks it under the lock.
///
/// Three modes:
/// - No branch, no worktree opt-in: work in-place in the user's directory.
/// - Branch without opt-in (legacy): work in-place when it is the current
///   branch, otherwise a worktree started from that branch.
/// - Worktree opt-in: always a fresh worktree on an auto-named `the-pair/…`
///   branch, based on the selected branch or — without one — the current
///   branch. A dirty repository is fine in that case: `git worktree add`
///   never touches the user's working tree.
pub fn prepare_workspace(
    input: &CreatePairInput,
    pair_id: &str,
    branches_in_use: &[String],
) -> Result<PairWorkspace, String> {
    let use_worktree = input.use_worktree.unwrap_or(false);
    let selected_branch = input.branch.as_deref();

    if !use_worktree && selected_branch.is_none() {
        return Ok(in_place_workspace(input));
    }

    let repo_state = check_repo_state(&input.directory);
    if !repo_state.is_git_repo {
        return Err(if use_worktree {
            "Directory is not a git repository. Cannot create a worktree.".to_string()
        } else {
            "Directory is not a git repository. Cannot create worktree.".to_string()
        });
    }

    let current_branch = repo_state.current_branch.as_deref();

    if !use_worktree && selected_branch == current_branch {
        println!(
            "[PairManager::create_pair] Selected branch '{}' is the current branch — working in-place without worktree",
            selected_branch.unwrap_or_default()
        );
        return Ok(in_place_workspace(input));
    }

    // Without an explicit base, a worktree pair starts from the current
    // branch; it doesn't hold it, so other pairs may base off it too.
    let base_branch = match selected_branch {
        Some(branch) => branch.to_string(),
        None => current_branch
            .ok_or_else(|| {
                "Repository has no current branch (empty repository?). Cannot create a worktree."
                    .to_string()
            })?
            .to_string(),
    };

    // Creating a worktree from the current branch never touches the user's
    // working tree, so uncommitted changes are safe to leave in place. Any
    // other base keeps requiring a clean repository.
    if repo_state.is_dirty && base_branch != current_branch.unwrap_or_default() {
        return Err("Repository has uncommitted changes. Please commit or stash before creating a pair with branch.".to_string());
    }

    if selected_branch.is_some() && branches_in_use.iter().any(|branch| *branch == base_branch) {
        return Err(branch_conflict_error(&base_branch));
    }

    let is_local = repo_state
        .branches
        .iter()
        .any(|b| b.name == base_branch && b.is_local);

    let effective_branch = if !is_local {
        println!(
            "[PairManager::create_pair] Remote branch detected, creating local tracking branch for {}",
            base_branch
        );
        ensure_local_tracking_branch(&input.directory, &base_branch)?
    } else {
        base_branch.clone()
    };

    let worktree_rel_path = format!(".worktrees/pair-{}", pair_id);
    println!(
        "[PairManager::create_pair] Creating worktree at {} for branch {}",
        worktree_rel_path, effective_branch
    );

    match ensure_gitignore_worktrees(&input.directory) {
        Ok(true) => println!(
            "[PairManager::create_pair] Added .worktrees/ to .gitignore in {}",
            input.directory
        ),
        Ok(false) => {}
        Err(e) => println!(
            "[PairManager::create_pair] Warning: could not update .gitignore: {}",
            e
        ),
    }

    let worktree_full_path =
        create_worktree(&input.directory, &effective_branch, &worktree_rel_path)?;

    println!(
        "[PairManager::create_pair] Worktree created at {}",
        worktree_full_path
    );

    let worktree_branch = branch_for_worktree(&worktree_full_path);

    Ok(PairWorkspace {
        directory: worktree_full_path.clone(),
        // A base picked explicitly is held (conflict-guarded); the current
        // branch a worktree happens to start from is not.
        branch: if selected_branch.is_some() {
            Some(effective_branch)
        } else {
            None
        },
        repo_path: Some(input.directory.clone()),
        worktree_path: Some(worktree_full_path),
        worktree_branch,
    })
}

fn branch_conflict_error(branch: &str) -> String {
    format!(
        "Another pair is already using branch '{}'. Only one pair can use a branch at a time.",
        branch
    )
}

/// Copy a rotated workspace onto the pair record.
fn apply_workspace(mut pair: Pair, workspace: &PairWorkspace) -> Pair {
    pair.directory = workspace.directory.clone();
    pair.branch = workspace.branch.clone();
    pair.repo_path = workspace.repo_path.clone();
    pair.worktree_path = workspace.worktree_path.clone();
    pair.worktree_branch = workspace.worktree_branch.clone();
    pair
}

/// Move a pair into a fresh worktree on a new auto-named `the-pair/…` branch
/// before a new task run. The previous worktree's work is preserved by
/// `delete_worktree` (uncommitted changes stashed, non-redundant `the-pair/…`
/// commits kept); on failure the pair keeps its old workspace and the caller
/// aborts the assignment. An in-place pair is converted to a worktree pair
/// based on the repository's current branch.
pub fn rotate_workspace(pair: &Pair) -> Result<PairWorkspace, String> {
    let repo_path = pair
        .repo_path
        .clone()
        .unwrap_or_else(|| pair.directory.clone());

    let repo_state = check_repo_state(&repo_path);
    if !repo_state.is_git_repo {
        return Err("Directory is not a git repository. Cannot create a worktree.".to_string());
    }

    // A held base branch stays the base; a worktree created from the current
    // branch has none recorded and rotates off wherever the user's repository
    // is now.
    let base_branch = pair
        .branch
        .clone()
        .or_else(|| repo_state.current_branch.clone())
        .ok_or_else(|| {
            "Repository has no current branch (empty repository?). Cannot create a worktree."
                .to_string()
        })?;

    if repo_state.is_dirty && base_branch != repo_state.current_branch.unwrap_or_default() {
        return Err("Repository has uncommitted changes. Please commit or stash before rotating to a fresh worktree.".to_string());
    }

    if let Some(worktree_path) = &pair.worktree_path {
        println!(
            "[pair_manager] Preserving and removing previous worktree at {}",
            worktree_path
        );
        delete_worktree(worktree_path)?;
    }

    let worktree_rel_path = format!(".worktrees/pair-{}", uuid::Uuid::new_v4());
    if let Err(e) = ensure_gitignore_worktrees(&repo_path) {
        println!("[pair_manager] Warning: could not update .gitignore: {}", e);
    }

    let worktree_full_path = create_worktree(&repo_path, &base_branch, &worktree_rel_path)?;
    println!(
        "[pair_manager] Fresh worktree created at {} for pair {}",
        worktree_full_path, pair.pair_id
    );

    Ok(PairWorkspace {
        directory: worktree_full_path.clone(),
        branch: pair.branch.clone(),
        repo_path: Some(repo_path),
        worktree_path: Some(worktree_full_path.clone()),
        worktree_branch: branch_for_worktree(&worktree_full_path),
    })
}

impl PairManager {
    pub fn new() -> Self {
        Self {
            pairs: HashMap::new(),
        }
    }

    /// Branches currently held by worktree pairs.
    pub fn branches_in_use(&self) -> Vec<String> {
        self.pairs
            .values()
            .filter(|pair| pair.worktree_path.is_some())
            .filter_map(|pair| pair.branch.clone())
            .collect()
    }

    /// Insert a pair whose workspace is already prepared, and initialize its
    /// broker state. Re-checks the branch conflict under the manager lock.
    pub fn register_pair(
        &mut self,
        pair_id: &str,
        input: &CreatePairInput,
        workspace: PairWorkspace,
        broker: &MessageBroker,
    ) -> Result<Pair, String> {
        if let (Some(branch), Some(_)) = (&workspace.branch, &workspace.worktree_path) {
            if self.branches_in_use().iter().any(|used| used == branch) {
                return Err(branch_conflict_error(branch));
            }
        }

        let pair = Pair {
            pair_id: pair_id.to_string(),
            name: input.name.clone(),
            directory: workspace.directory,
            status: PairStatus::Idle,
            mentor_model: input.mentor.model.clone(),
            executor_model: input.executor.model.clone(),
            pending_mentor_model: None,
            pending_executor_model: None,
            mentor_reasoning_effort: input.mentor_reasoning_effort.clone(),
            executor_reasoning_effort: input.executor_reasoning_effort.clone(),
            created_at: now_millis(),
            branch: workspace.branch,
            repo_path: workspace.repo_path,
            worktree_path: workspace.worktree_path,
            worktree_branch: workspace.worktree_branch,
            plan_gate: input.plan_gate.unwrap_or(false),
        };

        let effective_dir = pair.worktree_path.as_deref().or(Some(&pair.directory));
        broker.initialize_pair(pair_id, input.clone(), effective_dir)?;
        self.pairs.insert(pair_id.to_string(), pair.clone());
        println!("[PairManager::create_pair] Pair {} registered", pair_id);

        Ok(pair)
    }

    /// Create a pair in one step (workspace + registration). The command path
    /// splits these so no lock is held during git work.
    #[cfg(test)]
    pub fn create_pair(
        &mut self,
        input: &CreatePairInput,
        broker: &MessageBroker,
    ) -> Result<Pair, String> {
        let pair_id = uuid::Uuid::new_v4().to_string();
        let workspace = prepare_workspace(input, &pair_id, &self.branches_in_use())?;
        self.register_pair(&pair_id, input, workspace, broker)
    }

    pub fn get_pair(&self, pair_id: &str) -> Option<Pair> {
        self.pairs.get(pair_id).cloned()
    }

    pub fn upsert_pair(&mut self, pair: Pair) {
        self.pairs.insert(pair.pair_id.clone(), pair);
    }

    pub fn list_pairs(&self) -> Vec<Pair> {
        self.pairs.values().cloned().collect()
    }

    pub fn delete_pair(&mut self, pair_id: &str) -> Result<(), String> {
        self.pairs
            .remove(pair_id)
            .ok_or_else(|| format!("Pair {} not found", pair_id))?;
        Ok(())
    }
}

/// Persist the pair's snapshot off the calling thread.
fn persist_snapshot_in_background(app: &tauri::AppHandle, pair_id: &str) {
    let app = app.clone();
    let pair_id = pair_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = persist_current_pair_snapshot(&app, &pair_id) {
            println!(
                "[pair_manager] Failed to persist snapshot for {}: {}",
                pair_id, error
            );
        }
    });
}

/// The model a role will actually talk to: the pending one when the human has
/// just changed it, otherwise the pair's own.
fn effective_model(pair: &Pair, role: &str) -> String {
    if role == "mentor" {
        pair.pending_mentor_model
            .clone()
            .unwrap_or_else(|| pair.mentor_model.clone())
    } else {
        pair.pending_executor_model
            .clone()
            .unwrap_or_else(|| pair.executor_model.clone())
    }
}

/// Whether either role still has a turn registered for this pair.
///
/// A turn registers a cancel flag when it starts and clears it when it ends, so
/// this is the harness's own answer to "is a turn running".
fn turn_in_flight(spawner: &TurnDriver, pair_id: &str) -> bool {
    let cancels = spawner
        .turn_cancels
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    ["mentor", "executor"]
        .iter()
        .any(|role| cancels.contains_key(&format!("{}-{}", pair_id, role)))
}

/// Wait for the pair's in-flight turns to finish, up to `timeout`.
///
/// A raised cancel flag stops a turn where it is — a model call is dropped and
/// a running `bash` command is killed with its process tree — but that still
/// takes a moment. Waiting a bounded one before a worktree is stashed and
/// removed gives the turn the chance to unwind first; past the timeout the
/// caller goes ahead, because a delete must not hang on it.
async fn wait_for_turns_to_stop(spawner: &TurnDriver, pair_id: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !turn_in_flight(spawner, pair_id) {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        tokio::time::sleep(TURN_STOP_POLL.min(deadline - now)).await;
    }
}

/// Start a turn; if it cannot start (spawn failure, missing binary, prompt
/// too large …) mark the pair as errored instead of leaving it looking busy.
pub(crate) async fn start_turn(
    app: &tauri::AppHandle,
    broker: &std::sync::Mutex<MessageBroker>,
    spawner: &TurnDriver,
    pair_id: &str,
    role: &str,
    prompt: String,
) -> Result<(), String> {
    // A pair whose model did not resolve when it was created — no credential on
    // the machine yet, say — gets another chance now: resume and retry must not
    // be blocked forever by a registry that has since gained the endpoint.
    refresh_unresolved_selection(app, spawner, pair_id, role);

    match spawner
        .trigger_turn(app.clone(), pair_id.to_string(), role.to_string(), prompt)
        .await
    {
        Ok(()) => Ok(()),
        Err(error) => {
            println!(
                "[pair_manager] Could not start {} turn for {}: {}",
                role, pair_id, error
            );
            broker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_pair_status(
                    pair_id,
                    PairStatus::Error,
                    Some(format!("Could not start the {} turn: {}", role, error)),
                );
            persist_snapshot_in_background(app, pair_id);
            Err(error)
        }
    }
}

/// Resolves the selection for a role that still has none, from the pair's own
/// stored model. A pair whose selection is already set is left alone: the model
/// the human chose wins over what the registry would infer today.
fn refresh_unresolved_selection(
    app: &tauri::AppHandle,
    spawner: &TurnDriver,
    pair_id: &str,
    role: &str,
) {
    let needs_refresh = {
        let contexts = spawner
            .pair_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        contexts
            .get(pair_id)
            .map(|context| {
                if role == "mentor" {
                    context.mentor_selection.is_none()
                } else {
                    context.executor_selection.is_none()
                }
            })
            .unwrap_or(false)
    };
    if !needs_refresh {
        return;
    }

    let Some(pair) = app
        .try_state::<std::sync::Mutex<PairManager>>()
        .and_then(|manager| {
            manager
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_pair(pair_id)
        })
    else {
        return;
    };
    let Ok(app_dir) = app.path().app_data_dir() else {
        return;
    };
    let model = if role == "mentor" {
        pair.pending_mentor_model
            .clone()
            .unwrap_or_else(|| pair.mentor_model.clone())
    } else {
        pair.pending_executor_model
            .clone()
            .unwrap_or_else(|| pair.executor_model.clone())
    };
    let Some(selection) = crate::harness::selection_for_role(&app_dir, role, &model) else {
        return;
    };

    let mut contexts = spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(context) = contexts.get_mut(pair_id) {
        println!(
            "[pair_manager] {} model {} now resolves onto {}",
            role, model, selection.provider_id
        );
        if role == "mentor" {
            context.mentor_selection = Some(selection);
        } else {
            context.executor_selection = Some(selection);
        }
    }
}

/// Contract C3: a role handoff for a pair that was stopped (paused, finished,
/// errored) is stale and must not restart it. Plan approval/rejection comes
/// from `AwaitingHumanReview` through the same path, so that stays allowed.
fn handoff_rejection(status: &PairStatus) -> Option<String> {
    matches!(
        status,
        PairStatus::Paused | PairStatus::Finished | PairStatus::Error
    )
    .then(|| format!("HANDOFF_IGNORED: pair is {}", status.as_wire_str()))
}

#[tauri::command]
pub async fn pair_create(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, TurnDriver>,
    input: CreatePairInput,
) -> Result<Pair, String> {
    println!(
        "[pair_create] Called with input: name={}, directory={}",
        input.name, input.directory
    );
    println!(
        "[pair_create] Mentor model: {}, Executor model: {}",
        input.mentor.model, input.executor.model
    );

    let pair_id = uuid::Uuid::new_v4().to_string();

    // Resolved before anything is registered or created: a failure here must
    // leave no pair, no worktree and no branch behind.
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    // A pair whose model resolves onto an endpoint with a credential runs on the
    // direct connection from its very first turn; one that resolves to nothing
    // fails its first turn with a readable error.
    // The endpoint each model was picked under decides, not just the id: two
    // endpoints can list the same model.
    let mentor_selection =
        crate::harness::selection_for_role(&app_dir, "mentor", &input.mentor.model);
    let executor_selection =
        crate::harness::selection_for_role(&app_dir, "executor", &input.executor.model);

    // All git/worktree work (fetch, checkout) runs with no lock held and off
    // the async runtime's worker threads.
    let branches_in_use = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .branches_in_use();
    let workspace = {
        let input = input.clone();
        let pair_id = pair_id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            prepare_workspace(&input, &pair_id, &branches_in_use)
        })
        .await
        .map_err(|e| format!("Workspace setup failed: {}", e))??
    };

    let registered = {
        let mut manager = state.lock().unwrap_or_else(|e| e.into_inner());
        let broker_guard = broker.lock().unwrap_or_else(|e| e.into_inner());
        manager.register_pair(&pair_id, &input, workspace.clone(), &broker_guard)
    };
    let pair = match registered {
        Ok(pair) => pair,
        Err(error) => {
            if let Some(worktree_path) = workspace.worktree_path {
                remove_worktree_logged(worktree_path).await;
            }
            return Err(error);
        }
    };

    println!(
        "[pair_create] Successfully created pair: id={}, name={}",
        pair.pair_id, pair.name
    );

    {
        let mut ctx_guard = spawner
            .pair_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        ctx_guard.insert(
            pair_id.clone(),
            crate::harness::driver::ProcessContext {
                directory: pair
                    .worktree_path
                    .clone()
                    .unwrap_or_else(|| pair.directory.clone()),
                mentor_model: pair.mentor_model.clone(),
                executor_model: pair.executor_model.clone(),
                mentor_session_id: None,
                executor_session_id: None,
                mentor_reasoning_effort: input.mentor_reasoning_effort.clone(),
                executor_reasoning_effort: input.executor_reasoning_effort.clone(),
                run_generation: 0,
                is_smoke_test: false,
                mentor_selection,
                executor_selection,
                mentor_history: Vec::new(),
                executor_history: Vec::new(),
            },
        );
    }

    // Trigger the initial task (a pair created without a task stays Idle
    // until one is assigned).
    if !input.spec.trim().is_empty() {
        println!("[pair_create] Starting initial task...");
        broker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .prepare_run(&pair_id, "mentor");

        if let Err(error) = spawner
            .trigger_turn(
                app.clone(),
                pair_id.clone(),
                "mentor".to_string(),
                build_mentor_planning_prompt(&input.spec),
            )
            .await
        {
            // The frontend never learns about a pair whose creation failed, so
            // undo it entirely instead of leaving a half-created, busy pair
            // that also blocks its branch.
            println!(
                "[pair_create] Initial turn failed, rolling back pair {}: {}",
                pair_id, error
            );
            spawner.bump_run_generation(&pair_id);
            spawner.stop_pair_processes(&pair_id);
            spawner
                .pair_contexts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&pair_id);
            broker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove_pair(&pair_id);
            let _ = state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .delete_pair(&pair_id);
            if let Some(worktree_path) = pair.worktree_path.clone() {
                remove_worktree_logged(worktree_path).await;
            }
            return Err(error);
        }
        println!("[pair_create] Initial task triggered successfully");
    }

    persist_snapshot_in_background(&app, &pair_id);

    Ok(pair)
}

async fn remove_worktree_logged(worktree_path: String) {
    let path = worktree_path.clone();
    match tauri::async_runtime::spawn_blocking(move || delete_worktree(&path)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => println!(
            "[pair_manager] Could not remove worktree {}: {}",
            worktree_path, error
        ),
        Err(error) => println!(
            "[pair_manager] Worktree removal task failed for {}: {}",
            worktree_path, error
        ),
    }
}

/// Result of validating an assign-task request: the pair to assign to, the
/// role whose turn starts, whether this is a new run, and whether a fresh
/// worktree was requested. Runs before anything changes.
type ValidatedAssign = (Pair, String, bool, bool);

fn validate_assign_task(
    pair_manager: &std::sync::Mutex<PairManager>,
    broker: &std::sync::Mutex<MessageBroker>,
    pair_id: &str,
    input: &AssignTaskInput,
) -> Result<ValidatedAssign, String> {
    if let Some(ref role) = input.role {
        if role != "mentor" && role != "executor" {
            return Err("Invalid role: must be 'mentor' or 'executor'".to_string());
        }
    }
    let is_new_run = input.role.is_none();
    let fresh_worktree = input.fresh_worktree.unwrap_or(false);
    if fresh_worktree && !is_new_run {
        return Err("A fresh worktree can only be created for a new task run.".to_string());
    }

    let pair = {
        let manager = pair_manager.lock().unwrap_or_else(|e| e.into_inner());
        manager.pairs.get(pair_id).ok_or("Pair not found")?.clone()
    };

    // Stale handoffs for stopped pairs are rejected before anything changes.
    if !is_new_run {
        let status = broker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status_and_turn(pair_id)
            .map(|(status, _)| status);
        if let Some(rejection) = status.as_ref().and_then(handoff_rejection) {
            println!("[pair_assign_task] {}", rejection);
            return Err(rejection);
        }
    }

    let role = input.role.clone().unwrap_or_else(|| "mentor".to_string());
    Ok((pair, role, is_new_run, fresh_worktree))
}

/// New-run teardown: stop whatever the previous run still has running (whole
/// trees) and, when requested, rotate the pair into a fresh worktree before
/// the new run starts. The rotation also invalidates the turn and waits for
/// the agents to exit, so they stop writing into the old worktree before its
/// work is preserved and it is removed (mirrors pair_delete). A failed
/// rotation preserves the old workspace and the pair; the caller aborts the
/// assignment instead of starting a run in stale surroundings.
async fn prepare_new_run_workspace(
    pair_manager: &std::sync::Mutex<PairManager>,
    broker: &std::sync::Mutex<MessageBroker>,
    spawner: &TurnDriver,
    pair_id: &str,
    pair: Pair,
    fresh_worktree: bool,
) -> Result<Pair, String> {
    if fresh_worktree {
        spawner.bump_run_generation(pair_id);
    } else {
        spawner.stop_pair_processes(pair_id);
    }

    if !fresh_worktree {
        return Ok(pair);
    }

    // The rotation preserves the old worktree's work and removes it, so the turn
    // that was writing into it has to be gone first. The wait is bounded: a tool
    // call in flight takes no cancel token, and an assignment must not hang.
    if !wait_for_turns_to_stop(spawner, pair_id, TURN_STOP_TIMEOUT).await {
        println!(
            "[pair_manager] Pair {} still has a turn in flight; rotating anyway",
            pair_id
        );
    }

    let rotated = {
        let pair = pair.clone();
        tauri::async_runtime::spawn_blocking(move || rotate_workspace(&pair))
            .await
            .map_err(|e| format!("Workspace rotation failed: {}", e))?
    }?;
    let pair = apply_workspace(pair, &rotated);
    pair_manager
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .upsert_pair(pair.clone());
    broker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .update_workspace(pair_id, &pair.directory, pair.worktree_path.as_deref());
    Ok(pair)
}

/// (Re)build the pair's process context for the turn about to start: install
/// the endpoints resolved for each role (contract C6), keep provider sessions
/// where possible, and bump the run generation for a new run.
///
/// The selections are resolved by the caller, before anything about the run
/// changes — see [`RoleSelections`].
fn prepare_run_context(
    spawner: &TurnDriver,
    pair: &Pair,
    pair_id: &str,
    is_new_run: bool,
    selections: RoleSelections,
) {
    let existing = spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(pair_id)
        .cloned();

    let effective_mentor_model = effective_model(pair, "mentor");
    let effective_executor_model = effective_model(pair, "executor");
    let mentor_selection = selections.mentor;
    let executor_selection = selections.executor;

    if mentor_selection.is_some() || executor_selection.is_some() {
        println!(
            "[pair_assign_task] Direct connection: mentor={:?} executor={:?}",
            mentor_selection.as_ref().map(|s| &s.model),
            executor_selection.as_ref().map(|s| &s.model),
        );
    }

    // A conversation belongs to the endpoint it was had with: moving a role to
    // another account or another model starts a fresh one, the way a provider
    // change does.
    let mentor_target_changed = selection_changed(
        existing
            .as_ref()
            .and_then(|ctx| ctx.mentor_selection.as_ref()),
        mentor_selection.as_ref(),
    );
    let executor_target_changed = selection_changed(
        existing
            .as_ref()
            .and_then(|ctx| ctx.executor_selection.as_ref()),
        executor_selection.as_ref(),
    );

    // Re-read under the lock: the credential store read above released it, and
    // two assignments racing on one pair must still leave a strictly increasing
    // generation, or neither turn supersedes the other.
    let mut ctx_guard = spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let existing = ctx_guard.get(pair_id).cloned();
    let existing_run_generation = existing.as_ref().map(|ctx| ctx.run_generation).unwrap_or(0);
    ctx_guard.insert(
        pair_id.to_string(),
        crate::harness::driver::ProcessContext {
            directory: pair
                .worktree_path
                .clone()
                .unwrap_or_else(|| pair.directory.clone()),
            mentor_model: effective_mentor_model,
            executor_model: effective_executor_model,
            mentor_session_id: if is_new_run || mentor_target_changed {
                None
            } else {
                existing
                    .as_ref()
                    .and_then(|ctx| ctx.mentor_session_id.clone())
            },
            executor_session_id: if is_new_run || executor_target_changed {
                None
            } else {
                existing
                    .as_ref()
                    .and_then(|ctx| ctx.executor_session_id.clone())
            },
            mentor_reasoning_effort: pair.mentor_reasoning_effort.clone(),
            executor_reasoning_effort: pair.executor_reasoning_effort.clone(),
            run_generation: if is_new_run {
                existing_run_generation.wrapping_add(1)
            } else {
                existing_run_generation
            },
            is_smoke_test: false,
            // A pair whose endpoint and credential are both in the registry
            // runs on the direct connection.
            mentor_selection,
            executor_selection,
            // A new run is a new conversation; a resumed one keeps what its
            // agents already said, so the next turn replays it. A role that
            // moved to another endpoint starts fresh, the way its session does.
            mentor_history: carried_history(
                is_new_run || mentor_target_changed,
                existing.as_ref(),
                true,
            ),
            executor_history: carried_history(
                is_new_run || executor_target_changed,
                existing.as_ref(),
                false,
            ),
        },
    );
}

/// The conversation a role carries into the turn about to start: the one it
/// already had, unless something about the run or the target changed.
fn carried_history(
    start_fresh: bool,
    existing: Option<&crate::harness::driver::ProcessContext>,
    mentor: bool,
) -> Vec<serde_json::Value> {
    match start_fresh {
        true => Vec::new(),
        false => existing
            .map(|ctx| {
                if mentor {
                    ctx.mentor_history.clone()
                } else {
                    ctx.executor_history.clone()
                }
            })
            .unwrap_or_default(),
    }
}

/// Whether a role is about to talk to a different endpoint, account or model
/// than the conversation it still holds was had with.
fn selection_changed(
    before: Option<&crate::harness::ModelSelection>,
    after: Option<&crate::harness::ModelSelection>,
) -> bool {
    match (before, after) {
        (None, None) => false,
        (Some(before), Some(after)) => before != after,
        // Moving onto the direct connection (or off it) is a change of target.
        _ => true,
    }
}

/// The endpoint each of a pair's roles talks to for its effective model.
///
/// Resolved before anything about a run changes — see [`prepare_assignment`].
/// A role that is already talking to an endpoint keeps it while that endpoint
/// can still run the model: this runs on every handoff, and re-deriving the
/// endpoint from the model id each time would move a role — and start its
/// conversation over — whenever a re-scan reordered the registry. A role with
/// nothing to keep gets the endpoint its model was picked under, and failing
/// that the one the id resolves onto.
#[derive(Debug, Clone, Default)]
struct RoleSelections {
    mentor: Option<crate::harness::ModelSelection>,
    executor: Option<crate::harness::ModelSelection>,
}

impl RoleSelections {
    fn resolve(
        app_dir: &Path,
        pair: &Pair,
        current: Option<&crate::harness::driver::ProcessContext>,
    ) -> Self {
        // The credential store is read on disk, so it is read before the
        // contexts lock is taken: every pair's turn bookkeeping waits on it.
        Self {
            mentor: crate::harness::keep_or_resolve(
                app_dir,
                "mentor",
                &effective_model(pair, "mentor"),
                current.and_then(|context| context.mentor_selection.as_ref()),
            ),
            executor: crate::harness::keep_or_resolve(
                app_dir,
                "executor",
                &effective_model(pair, "executor"),
                current.and_then(|context| context.executor_selection.as_ref()),
            ),
        }
    }

    fn for_role(&self, role: &str) -> Option<&crate::harness::ModelSelection> {
        if role == "mentor" {
            self.mentor.as_ref()
        } else {
            self.executor.as_ref()
        }
    }
}

/// Refuse an assignment whose starting role has no endpoint to talk to.
///
/// Checked before `begin_new_run` clears the transcript and before the old
/// worktree is rotated away: the frontend archives the previous run only once
/// the assignment succeeded, so a turn that cannot start would otherwise leave
/// the pair with no run at all — no history, no worktree.
fn ensure_role_resolvable(role: &str, selections: &RoleSelections) -> Result<(), String> {
    match selections.for_role(role) {
        Some(_) => Ok(()),
        None => Err(format!(
            "The {} has no model it can talk to. Pick one in the pair's settings.",
            role
        )),
    }
}

/// Everything between a validated assignment and the turn itself: resolve the
/// endpoints, stop the previous run (and rotate the workspace when asked),
/// rebuild the turn context, and reset the broker for the new run.
///
/// Ordered so nothing is destroyed before the turn is known to be able to
/// start. `start_turn` failing used to land after `begin_new_run` had cleared
/// the transcript and after the old worktree had been rotated away, and the
/// frontend archives the previous run only after a successful assignment — so
/// the pair was left with neither run.
async fn prepare_assignment(
    pair_manager: &std::sync::Mutex<PairManager>,
    broker: &std::sync::Mutex<MessageBroker>,
    spawner: &TurnDriver,
    pair_id: &str,
    pair: Pair,
    role: &str,
    is_new_run: bool,
    fresh_worktree: bool,
    spec: &str,
    app_dir: &Path,
) -> Result<Pair, String> {
    let current = spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(pair_id)
        .cloned();
    let selections = RoleSelections::resolve(app_dir, &pair, current.as_ref());
    ensure_role_resolvable(role, &selections)?;

    // Stop the previous run and, when requested, rotate the pair into a fresh
    // worktree before the new run starts. A failed rotation aborts the
    // assignment with the pair left on its old workspace.
    let pair =
        prepare_new_run_workspace(pair_manager, broker, spawner, pair_id, pair, fresh_worktree)
            .await?;

    prepare_run_context(spawner, &pair, pair_id, is_new_run, selections);

    {
        let broker_guard = broker.lock().unwrap_or_else(|e| e.into_inner());
        // Re-check under the same lock as prepare_run: a pause may have landed
        // since the first check.
        if !is_new_run {
            if let Some(rejection) = broker_guard
                .status_and_turn(pair_id)
                .and_then(|(status, _)| handoff_rejection(&status))
            {
                println!("[pair_assign_task] {}", rejection);
                return Err(rejection);
            }
        } else {
            broker_guard.begin_new_run(pair_id, spec);
        }
        broker_guard.prepare_run(pair_id, role);
    }

    Ok(pair)
}

#[tauri::command]
pub async fn pair_assign_task(
    app: tauri::AppHandle,
    pair_manager: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, TurnDriver>,
    pair_id: String,
    input: AssignTaskInput,
) -> Result<Pair, String> {
    println!("[pair_assign_task] Called for pair_id: {}", pair_id);

    let (pair, role, is_new_run, fresh_worktree) =
        validate_assign_task(&pair_manager, &broker, &pair_id, &input)?;

    println!(
        "[pair_assign_task] Found pair: {} at {}",
        pair.name, pair.directory
    );

    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    let pair = prepare_assignment(
        &pair_manager,
        &broker,
        &spawner,
        &pair_id,
        pair,
        &role,
        is_new_run,
        fresh_worktree,
        &input.spec,
        &app_dir,
    )
    .await?;

    if is_new_run {
        persist_snapshot_in_background(&app, &pair_id);
    }

    println!("[pair_assign_task] About to trigger {} turn...", role);
    let turn_prompt = if is_new_run {
        build_mentor_planning_prompt(&input.spec)
    } else {
        input.spec.clone()
    };
    start_turn(&app, &broker, &spawner, &pair_id, &role, turn_prompt).await?;
    println!("[pair_assign_task] {} turn triggered successfully", role);

    // The pair (possibly rotated into a fresh worktree) so the caller can
    // sync workspace fields the frontend holds.
    Ok(pair)
}

#[tauri::command]
pub fn pair_update_models(
    app: tauri::AppHandle,
    state: tauri::State<std::sync::Mutex<PairManager>>,
    spawner: tauri::State<TurnDriver>,
    pair_id: String,
    input: crate::types::UpdatePairModelsInput,
) -> Result<crate::types::UpdatePairModelsInput, String> {
    let (mut change, pair) = {
        let mut manager = state.lock().unwrap_or_else(|e| e.into_inner());
        let pair = manager
            .pairs
            .get_mut(&pair_id)
            .ok_or_else(|| format!("Pair {} not found", pair_id))?;
        let change = apply_model_update(pair, &input);
        (change, pair.clone())
    };

    // The same model picked again under another endpoint changes no id, but it
    // is still a move the human asked for.
    if let Ok(app_dir) = app.path().app_data_dir() {
        let moved = endpoint_moves(&app_dir, &spawner, &pair);
        change.mentor |= moved.mentor;
        change.executor |= moved.executor;
    }

    // The model the human just picked has to reach the run in flight: a turn
    // talks to the selection stored on the pair's context, and
    // `refresh_unresolved_selection` leaves a pair that already has one alone.
    // Dropping it makes the next turn resolve the new model — otherwise the
    // stale selection is persisted with the snapshot and survives restarts.
    invalidate_selections(&spawner, &pair_id, &change);

    Ok(input)
}

/// Which roles the human moved to another endpoint without changing the model:
/// the endpoint picked for the role's model is not the one the role is talking
/// to.
///
/// Only an explicit pick moves a role this way. A role with no pick on record
/// stays where it is, however the registry would resolve its id today.
fn endpoint_moves(app_dir: &Path, spawner: &TurnDriver, pair: &Pair) -> ModelChange {
    let current = spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&pair.pair_id)
        .cloned();
    let Some(current) = current else {
        return ModelChange::default();
    };
    let moved = |role: &str, talking_to: Option<&crate::harness::ModelSelection>| {
        let Some(talking_to) = talking_to else {
            return false;
        };
        crate::harness::preferred_selection(app_dir, role, &effective_model(pair, role))
            .is_some_and(|picked| {
                picked.provider_id != talking_to.provider_id
                    || picked.account_id != talking_to.account_id
            })
    };
    ModelChange {
        mentor: moved("mentor", current.mentor_selection.as_ref()),
        executor: moved("executor", current.executor_selection.as_ref()),
    }
}

/// Which of a pair's roles had their model changed by an update.
#[derive(Debug, Default, PartialEq, Eq)]
struct ModelChange {
    mentor: bool,
    executor: bool,
}

impl ModelChange {
    fn any(&self) -> bool {
        self.mentor || self.executor
    }
}

/// Drop the stored selection for every role whose model changed, so the next
/// turn resolves the new one instead of talking to the old endpoint.
fn invalidate_selections(spawner: &TurnDriver, pair_id: &str, change: &ModelChange) {
    if !change.any() {
        return;
    }
    let mut contexts = spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(context) = contexts.get_mut(pair_id) {
        if change.mentor {
            context.mentor_selection = None;
        }
        if change.executor {
            context.executor_selection = None;
        }
        println!(
            "[pair_update_models] Pair {} re-resolves its models (mentor={} executor={})",
            pair_id, change.mentor, change.executor
        );
    }
}

/// Apply a model update, reporting which roles actually moved. The endpoint a
/// role talks to is resolved from the model id when the next turn starts, so
/// the only thing to keep in step here is the stored selection — which the
/// caller invalidates for the roles that moved.
fn apply_model_update(pair: &mut Pair, input: &crate::types::UpdatePairModelsInput) -> ModelChange {
    let mentor_before = effective_model(pair, "mentor");
    let executor_before = effective_model(pair, "executor");

    pair.mentor_model = input.mentor_model.clone();
    pair.executor_model = input.executor_model.clone();
    pair.pending_mentor_model = input.pending_mentor_model.clone();
    pair.pending_executor_model = input.pending_executor_model.clone();
    pair.mentor_reasoning_effort = input.mentor_reasoning_effort.clone();
    pair.executor_reasoning_effort = input.executor_reasoning_effort.clone();

    println!(
        "[pair_update_models] Updated pair {}: mentor={}, executor={}",
        pair.pair_id, pair.mentor_model, pair.executor_model
    );

    ModelChange {
        mentor: mentor_before != effective_model(pair, "mentor"),
        executor: executor_before != effective_model(pair, "executor"),
    }
}

#[tauri::command]
pub fn pair_list(state: tauri::State<std::sync::Mutex<PairManager>>) -> Result<Vec<Pair>, String> {
    let manager = state.lock().unwrap_or_else(|e| e.into_inner());
    Ok(manager.list_pairs())
}

#[tauri::command]
pub async fn pair_delete(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, TurnDriver>,
    pair_id: String,
) -> Result<(), String> {
    let existing = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_pair(&pair_id);

    // Invalidate the running turn so it stops writing into the worktree before
    // the worktree's changes are preserved and it is removed.
    spawner.bump_run_generation(&pair_id);
    spawner.stop_pair_processes(&pair_id);

    if let Some(worktree_path) = existing
        .as_ref()
        .and_then(|pair| pair.worktree_path.clone())
    {
        // The cancelled turn stops where it is — its model call is dropped,
        // its `bash` command killed — but not instantly, so wait a bounded
        // moment for it to unwind before the worktree is stashed and removed
        // under it.
        if !wait_for_turns_to_stop(&spawner, &pair_id, TURN_STOP_TIMEOUT).await {
            println!(
                "[pair_delete] Pair {} still has a turn in flight; deleting anyway",
                pair_id
            );
        }

        println!("[pair_delete] Deleting worktree at {}", worktree_path);
        let path = worktree_path.clone();
        let result = tauri::async_runtime::spawn_blocking(move || delete_worktree(&path))
            .await
            .map_err(|e| format!("Worktree removal task failed: {}", e))
            .and_then(|result| result);
        if let Err(error) = result {
            // Contract C1: the worktree's work could not be preserved, so
            // nothing is removed. Keep the pair (stopped) and tell the user.
            let message = format!(
                "Could not delete the pair: its worktree at {} could not be removed safely ({}). The pair was kept.",
                worktree_path, error
            );
            {
                let broker_guard = broker.lock().unwrap_or_else(|e| e.into_inner());
                if broker_guard
                    .status_and_turn(&pair_id)
                    .is_some_and(|(status, _)| status.is_active())
                {
                    broker_guard.set_pair_status(
                        &pair_id,
                        PairStatus::Paused,
                        Some("Stopped for deletion; the worktree could not be removed".to_string()),
                    );
                }
            }
            persist_snapshot_in_background(&app, &pair_id);
            return Err(message);
        }
    }

    let delete_result = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .delete_pair(&pair_id);
    broker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove_pair(&pair_id);
    spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&pair_id);

    // Always remove the on-disk snapshot, even when the in-memory pair was
    // already gone (e.g. a double-delete), so it can never be orphaned. This
    // runs after the pair left PairManager, so a racing persist sees it gone
    // and cannot resurrect the file.
    let snapshot_app = app.clone();
    let snapshot_pair_id = pair_id.clone();
    let snapshot_result = tauri::async_runtime::spawn_blocking(move || {
        delete_pair_snapshot(&snapshot_app, &snapshot_pair_id)
    })
    .await;
    if let Ok(Err(error)) = snapshot_result {
        println!(
            "[pair_delete] Failed to delete snapshot for {}: {}",
            pair_id, error
        );
    }

    delete_result
}

#[tauri::command]
pub fn pair_pause(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, TurnDriver>,
    pair_id: String,
) -> Result<(), String> {
    {
        let manager = state.lock().unwrap_or_else(|e| e.into_inner());
        if !manager.pairs.contains_key(&pair_id) {
            return Err(format!("Pair {} not found", pair_id));
        }
    }

    // Invalidate first, so the killed turn's reader (and any acceptance
    // checks it is running) can no longer change the pair or hand off.
    spawner.bump_run_generation(&pair_id);
    spawner.stop_pair_processes(&pair_id);

    {
        let mut manager = state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pair) = manager.pairs.get_mut(&pair_id) {
            pair.status = PairStatus::Paused;
        }
    }

    broker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .set_pair_status(
            &pair_id,
            PairStatus::Paused,
            Some("Paused by user".to_string()),
        );

    persist_snapshot_in_background(&app, &pair_id);

    Ok(())
}

fn find_last_message_by_role(messages: &[Message], role: MessageSender) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|m| {
            m.from == role && (m.msg_type == MessageType::Plan || m.msg_type == MessageType::Result)
        })
        .map(|m| m.content.trim().to_string())
}

fn build_live_resume_prompt(
    turn: &str,
    mentor_is_planning: bool,
    task_spec: &str,
    last_mentor: Option<String>,
    last_executor: Option<String>,
    latest_acceptance: Option<crate::types::AcceptanceRecord>,
) -> String {
    if turn == "executor" {
        if let Some(acceptance) = latest_acceptance.as_ref() {
            if let Some(verdict) = acceptance.verdict.as_ref() {
                if matches!(
                    verdict.next_step.action,
                    crate::types::AcceptanceNextAction::Continue
                ) {
                    return build_executor_acceptance_followup_prompt(
                        task_spec,
                        &last_executor.unwrap_or_default(),
                        verdict,
                        acceptance,
                    );
                }
            }
        }

        let mentor_msg = last_mentor.unwrap_or_default();
        format!(
            "You're resuming a paused pair-programming session as the executor. Carry out the plan below — \
treat it as direct actions to perform right now, not a roadmap to comment on.\n\n\
A few constraints:\n\
- Do the next concrete action the plan calls for; do not restate, summarize, or narrate the plan back.\n\
- If the plan asks for specific output text, reply with exactly that text — no preface, no commentary, no status reports like \"awaiting…\" or \"instruction is set to…\".\n\
- Keep going from the restored context; do not create a new plan or review your own work.\n\
- You cannot declare the task complete — only the reviewer can finish the workflow, so do not output TASK_COMPLETE.\n\n\
PLAN\n{}\n",
            mentor_msg
        )
    } else if mentor_is_planning {
        // A paused (or gated) planning turn: the executor hasn't run yet, so
        // there is nothing to review — plan the task again.
        build_mentor_planning_prompt(task_spec)
    } else {
        if let Some(acceptance) = latest_acceptance.as_ref() {
            if let Some(error) = acceptance.error.as_ref() {
                if acceptance.repair_attempts > 0 {
                    return build_mentor_acceptance_repair_prompt(error);
                }
            }

            return build_mentor_acceptance_prompt(
                task_spec,
                &last_executor.unwrap_or_default(),
                acceptance,
            );
        }

        let executor_msg = last_executor
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| task_spec.to_string());
        format!(
            "You're resuming a paused pair-programming session as the reviewer. Read what the executor just did and decide whether the task is done or needs another pass.\n\n\
For this review turn, focus on analysis — no need to run commands or edit files.\n\n\
If you're satisfied the task is complete, include TASK_COMPLETE somewhere in your reply so the orchestrator stops the workflow. Otherwise, write a refined plan describing what the executor should do next.\n\n\
EXECUTOR OUTPUT\n{}\n",
            executor_msg
        )
    }
}

/// Resume the pair's current turn. The status check, the transition and the
/// prompt are all computed under one broker lock, so a double-click on
/// Resume/Retry cannot pass the check twice and spawn two turns.
async fn resume_pair_core(
    manager: &std::sync::Mutex<PairManager>,
    broker: &std::sync::Mutex<MessageBroker>,
    spawner: &TurnDriver,
    pair_id: &str,
    allowed: &[PairStatus],
) -> Result<(String, String), String> {
    {
        let manager_guard = manager.lock().unwrap_or_else(|e| e.into_inner());
        if !manager_guard.pairs.contains_key(pair_id) {
            return Err(format!("Pair {} not found", pair_id));
        }
    }

    let (role_str, prompt, resumed_status) = {
        let broker_guard = broker.lock().unwrap_or_else(|e| e.into_inner());
        let state = broker_guard
            .get_state(pair_id)
            .ok_or_else(|| format!("No broker state found for pair {}", pair_id))?;

        // Gate on the broker's live status, not the manager's Pair.status: the
        // manager copy is only written on manual pause/resume, so gating on it
        // rejects resumes after automatic transitions (budget exhausted,
        // provider turn errors, plan gate, verdict parse failures).
        if !allowed.contains(&state.status) {
            return Err(format!(
                "Pair {} is not in a resumable state (status: {:?})",
                pair_id, state.status
            ));
        }

        let role_str = match state.turn {
            crate::types::AgentRole::Mentor => "mentor",
            crate::types::AgentRole::Executor => "executor",
        };

        let resolved = broker_guard.resume_run(pair_id, role_str);

        let last_mentor_msg =
            find_last_message_by_role(&state.messages, MessageSender::Mentor).or(state
                .mentor
                .last_message
                .as_ref()
                .map(|m| m.content.clone()));
        let last_executor_msg = find_last_message_by_role(&state.messages, MessageSender::Executor)
            .or(state
                .executor
                .last_message
                .as_ref()
                .map(|m| m.content.clone()));
        let prompt = build_live_resume_prompt(
            role_str,
            resolved == PairStatus::Mentoring,
            &state.task_spec,
            last_mentor_msg,
            last_executor_msg,
            state.latest_acceptance.clone(),
        );

        (role_str.to_string(), prompt, resolved)
    };

    {
        let mut manager_guard = manager.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pair) = manager_guard.pairs.get_mut(pair_id) {
            pair.status = resumed_status;
        }
    }

    // Invalidate anything left over from the turn being resumed.
    spawner.bump_run_generation(pair_id);

    Ok((role_str, prompt))
}

#[tauri::command]
pub async fn pair_resume(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, TurnDriver>,
    pair_id: String,
) -> Result<(), String> {
    // Resume is scoped to Paused / AwaitingHumanReview.
    let (role_str, prompt) = resume_pair_core(
        &state,
        &broker,
        &spawner,
        &pair_id,
        &[PairStatus::Paused, PairStatus::AwaitingHumanReview],
    )
    .await?;

    persist_snapshot_in_background(&app, &pair_id);

    start_turn(&app, &broker, &spawner, &pair_id, &role_str, prompt).await
}

/// Retry the current turn for a pair that is in Error, Paused, or
/// AwaitingHumanReview state.  Unlike `pair_resume` (which is scoped to
/// Paused / AwaitingHumanReview), this command also accepts `Error` status –
/// the UI shows a "Retry Turn" button only when the pair has errored.
#[tauri::command]
pub async fn pair_retry_turn(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, TurnDriver>,
    pair_id: String,
) -> Result<(), String> {
    let (role_str, prompt) = resume_pair_core(
        &state,
        &broker,
        &spawner,
        &pair_id,
        &[
            PairStatus::Paused,
            PairStatus::AwaitingHumanReview,
            PairStatus::Error,
        ],
    )
    .await?;

    // Record the human's retry as an intervention (fire-and-forget; failures
    // are logged and swallowed inside the store).
    crate::intelligence_store::record_intervention_safely(&app, &pair_id, "retry", None);

    persist_snapshot_in_background(&app, &pair_id);

    start_turn(&app, &broker, &spawner, &pair_id, &role_str, prompt).await
}

#[tauri::command]
pub async fn repo_check_state(directory: String) -> Result<RepoState, String> {
    println!("[repo_check_state] Called with directory: {}", directory);
    let result = tauri::async_runtime::spawn_blocking(move || check_repo_state(&directory))
        .await
        .map_err(|e| format!("Repository check failed: {}", e))?;
    println!(
        "[repo_check_state] Result: is_git_repo={}, is_dirty={}, branches_count={}",
        result.is_git_repo,
        result.is_dirty,
        result.branches.len()
    );
    Ok(result)
}

#[tauri::command]
pub async fn repo_list_branches(directory: String) -> Result<Vec<BranchInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || crate::worktree_manager::list_branches(&directory))
        .await
        .map_err(|e| format!("Listing branches failed: {}", e))?
}

#[tauri::command]
pub async fn kill_process(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, TurnDriver>,
    pair_id: String,
    role: String,
) -> Result<(), String> {
    if role != "mentor" && role != "executor" {
        return Err("Invalid role: must be 'mentor' or 'executor'".to_string());
    }

    {
        let manager = state.lock().unwrap_or_else(|e| e.into_inner());
        if !manager.pairs.contains_key(&pair_id) {
            return Err(format!("Pair {} not found", pair_id));
        }
    }

    // A turn on the direct connection has no child process to kill: its
    // cancel flag is raised by `bump_run_generation` below, which is what stops
    // the stream. Without this the kill would report "no active process" and
    // leave the pair Mentoring until the stall detector gave up on it.
    spawner.stop_process(&pair_id, &role);
    // The killed turn must not finish, run checks or hand off afterwards.
    spawner.bump_run_generation(&pair_id);

    {
        let broker = broker.lock().unwrap_or_else(|e| e.into_inner());
        broker.update_agent_activity(
            &pair_id,
            &role,
            crate::types::ActivityPhase::Idle,
            "Process killed".to_string(),
            Some("Terminated by user".to_string()),
        );
        broker.set_pair_status(
            &pair_id,
            PairStatus::Paused,
            Some(format!("{} process terminated", role)),
        );
    }

    persist_snapshot_in_background(&app, &pair_id);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::driver::TurnDriver;
    use crate::message_broker::MessageBroker;
    use crate::types::{AgentConfig, AgentRole, CreatePairInput, PairStatus};
    use std::sync::{Arc, Mutex};

    const RETRYABLE: &[PairStatus] = &[
        PairStatus::Paused,
        PairStatus::AwaitingHumanReview,
        PairStatus::Error,
    ];

    fn make_test_pair_manager() -> (
        Arc<Mutex<PairManager>>,
        Arc<Mutex<MessageBroker>>,
        TurnDriver,
    ) {
        let manager = Arc::new(Mutex::new(PairManager::new()));
        let broker = Arc::new(Mutex::new(MessageBroker::new()));
        let spawner = TurnDriver::new();
        (manager, broker, spawner)
    }

    fn insert_paused_pair(
        manager: &std::sync::Arc<std::sync::Mutex<PairManager>>,
        broker: &std::sync::Arc<std::sync::Mutex<MessageBroker>>,
        pair_id: &str,
        turn: AgentRole,
        iteration: u32,
    ) {
        let input = CreatePairInput {
            name: "Test Pair".to_string(),
            directory: "/tmp/test".to_string(),
            spec: "Test task".to_string(),
            mentor: AgentConfig {
                role: AgentRole::Mentor,
                model: "test-mentor".to_string(),
                reasoning_effort: None,
            },
            executor: AgentConfig {
                role: AgentRole::Executor,
                model: "test-executor".to_string(),
                reasoning_effort: None,
            },
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            max_iterations: None,
            branch: None,
            use_worktree: None,
            plan_gate: None,
        };
        let broker_new = broker.lock().unwrap();
        manager
            .lock()
            .unwrap()
            .create_pair(&input, &broker_new)
            .unwrap();
        let created_pair_id = manager.lock().unwrap().list_pairs()[0].pair_id.clone();
        drop(broker_new);

        manager.lock().unwrap().upsert_pair(crate::types::Pair {
            pair_id: pair_id.to_string(),
            name: "Test Pair".to_string(),
            directory: "/tmp/test".to_string(),
            status: PairStatus::Paused,
            mentor_model: "test-mentor".to_string(),
            executor_model: "test-executor".to_string(),
            pending_mentor_model: None,
            pending_executor_model: None,
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            created_at: 0,
            branch: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            plan_gate: false,
        });

        let broker_guard = broker.lock().unwrap();
        let state = broker_guard.get_state(&created_pair_id).unwrap();
        drop(broker_guard);
        let mut state = state;
        state.pair_id = pair_id.to_string();
        state.status = PairStatus::Paused;
        state.turn = turn;
        state.iteration = iteration;
        state.mentor.status = PairStatus::Paused;
        state.executor.status = PairStatus::Paused;
        broker.lock().unwrap().restore_state(state).unwrap();
        manager.lock().unwrap().delete_pair(&created_pair_id).ok();
    }

    #[tokio::test]
    async fn pair_resume_command_restores_paused_mentor_planning_as_mentoring_not_reviewing() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-planning";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 1);

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE).await;
        assert!(result.is_ok(), "resume should succeed");

        let pair = manager.lock().unwrap().get_pair(pair_id).unwrap();
        assert_eq!(
            pair.status,
            PairStatus::Mentoring,
            "pair status should be Mentoring (planning), not Reviewing"
        );

        let state = broker.lock().unwrap().get_state(pair_id).unwrap();
        assert_eq!(state.status, PairStatus::Mentoring);
        assert_eq!(
            state.iteration, 1,
            "iteration should be preserved, not incremented"
        );
        assert_eq!(state.mentor_activity.label, "Analyzing task");
        assert_eq!(
            state.mentor.status,
            PairStatus::Executing,
            "mentor status should be Executing (not flattened to Mentoring)"
        );
        assert_eq!(
            state.executor.status,
            PairStatus::Idle,
            "executor status should be Idle (not overwritten by set_pair_status)"
        );
    }

    #[tokio::test]
    async fn pair_resume_command_restores_paused_mentor_review_as_reviewing() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-review";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 2);

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE).await;
        assert!(result.is_ok(), "resume should succeed");

        let pair = manager.lock().unwrap().get_pair(pair_id).unwrap();
        assert_eq!(
            pair.status,
            PairStatus::Reviewing,
            "pair status should be Reviewing (not Mentoring) for review turn"
        );

        let state = broker.lock().unwrap().get_state(pair_id).unwrap();
        assert_eq!(state.status, PairStatus::Reviewing);
        assert_eq!(
            state.iteration, 2,
            "iteration should be preserved, not incremented"
        );
        assert_eq!(state.mentor_activity.label, "Reviewing changes");
        assert_eq!(
            state.mentor.status,
            PairStatus::Reviewing,
            "mentor status should be Reviewing (not flattened)"
        );
        assert_eq!(
            state.executor.status,
            PairStatus::Idle,
            "executor status should be Idle (not overwritten by set_pair_status)"
        );
    }

    #[tokio::test]
    async fn pair_resume_command_restores_paused_executor_as_executing() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-executor";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Executor, 2);

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE).await;
        assert!(result.is_ok(), "resume should succeed");

        let pair = manager.lock().unwrap().get_pair(pair_id).unwrap();
        assert_eq!(
            pair.status,
            PairStatus::Executing,
            "pair status should be Executing"
        );

        let state = broker.lock().unwrap().get_state(pair_id).unwrap();
        assert_eq!(state.status, PairStatus::Executing);
        assert_eq!(state.iteration, 2, "iteration should be preserved");
        assert_eq!(state.executor_activity.label, "Executing plan");
        assert_eq!(state.mentor_activity.label, "Mentor observing");
        assert_eq!(
            state.executor.status,
            PairStatus::Executing,
            "executor status should be Executing (not flattened)"
        );
        assert_eq!(
            state.mentor.status,
            PairStatus::Idle,
            "mentor status should be Idle (not overwritten by set_pair_status)"
        );
    }

    #[tokio::test]
    async fn pair_resume_command_preserves_iteration_across_resume() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-iteration";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 5);

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE).await;
        assert!(result.is_ok());

        let state = broker.lock().unwrap().get_state(pair_id).unwrap();
        assert_eq!(
            state.iteration, 5,
            "iteration should remain 5, not reset or incremented"
        );
    }

    #[tokio::test]
    async fn pair_resume_core_gates_on_broker_status_not_stale_manager_status() {
        // Automatic transitions (budget exhausted, provider turn error, plan
        // gate) only update the broker state — the manager's Pair.status keeps
        // its last manual value. Resume/retry must still work in that state.
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-stale-manager";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 2);
        manager
            .lock()
            .unwrap()
            .pairs
            .get_mut(pair_id)
            .unwrap()
            .status = PairStatus::Idle;

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE).await;
        assert!(
            result.is_ok(),
            "resume should gate on broker status, not the stale manager copy: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn pair_resume_core_rejects_pair_that_is_running_in_broker() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-running";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 2);
        broker
            .lock()
            .unwrap()
            .set_pair_status(pair_id, PairStatus::Executing, None);
        // Even a stale manager status that looks retryable must not allow a
        // resume while the broker says the pair is mid-run.
        manager
            .lock()
            .unwrap()
            .pairs
            .get_mut(pair_id)
            .unwrap()
            .status = PairStatus::Paused;

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE).await;
        assert!(result.is_err(), "resume must reject a running pair");
    }

    #[test]
    fn build_mentor_planning_prompt_requires_actionable_steps_and_keeps_task_in_view() {
        let prompt = build_mentor_planning_prompt("Add a dark mode toggle");

        assert!(prompt.contains("Add a dark mode toggle"));
        assert!(prompt.to_lowercase().contains("numbered plan"));
        assert!(prompt.to_lowercase().contains("planning"));
        // Avoid role-play / anti-introspection markers that trip modern model safety filters.
        assert!(!prompt.contains("### ROLE:"));
        assert!(!prompt.contains("DO NOT"));
    }

    #[test]
    fn build_live_resume_prompt_for_executor_treats_plan_as_direct_actions() {
        let prompt = build_live_resume_prompt(
            "executor",
            false,
            "Smoke test spec",
            Some("Instruction to executor: \"Send Greeting 1/3\"".to_string()),
            None,
            None,
        );

        // Action framing — the executor should DO the plan, not narrate it back.
        let lower = prompt.to_lowercase();
        assert!(
            lower.contains("direct actions"),
            "resume prompt should frame the plan as direct actions"
        );
        assert!(
            lower.contains("do not restate, summarize, or narrate"),
            "resume prompt should forbid restating the plan"
        );
        assert!(
            lower.contains("reply with exactly that text"),
            "resume prompt should require exact-text output for text-only tasks"
        );
        // Mentor's instruction body still flows through verbatim so the executor sees what to do.
        assert!(
            prompt.contains("Send Greeting 1/3"),
            "mentor message body should be embedded in the resume prompt"
        );
        // TASK_COMPLETE remains reserved for the mentor.
        assert!(prompt.contains("TASK_COMPLETE"));
    }

    #[test]
    fn build_live_resume_prompt_for_mentor_uses_softer_review_framing() {
        let prompt = build_live_resume_prompt(
            "mentor",
            false,
            "Implement feature X",
            None,
            Some("Greeting 2/3".to_string()),
            None,
        );

        // Executor body still flows through verbatim.
        assert!(prompt.contains("Greeting 2/3"));
        // Mentor is told TASK_COMPLETE is how it ends the workflow.
        assert!(prompt.contains("TASK_COMPLETE"));
        assert!(prompt.contains("EXECUTOR OUTPUT"));
        // Avoid role-play / anti-introspection markers that trip modern model safety filters.
        assert!(!prompt.contains("### ROLE:"));
        assert!(!prompt.contains("DO NOT"));
    }

    fn sample_input() -> CreatePairInput {
        CreatePairInput {
            name: "Demo".to_string(),
            directory: "/tmp/project".to_string(),
            spec: "Build the feature".to_string(),
            mentor: AgentConfig {
                role: AgentRole::Mentor,
                model: "openai/gpt-4o-mini".to_string(),
                reasoning_effort: None,
            },
            executor: AgentConfig {
                role: AgentRole::Executor,
                model: "gpt-4o-mini".to_string(),
                reasoning_effort: None,
            },
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            max_iterations: None,
            branch: None,
            use_worktree: None,
            plan_gate: None,
        }
    }

    #[test]
    fn create_pair_keeps_the_models_it_was_given() {
        let mut manager = PairManager::new();
        let broker = MessageBroker::new();

        let pair = manager
            .create_pair(&sample_input(), &broker)
            .expect("pair should be created");

        assert_eq!(pair.mentor_model, "openai/gpt-4o-mini");
        assert_eq!(pair.executor_model, "gpt-4o-mini");
    }

    fn message(from: MessageSender, msg_type: MessageType, content: &str) -> Message {
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: 1,
            from,
            to: "human".to_string(),
            msg_type,
            content: content.to_string(),
            iteration: 1,
            token_usage: None,
            attachments: None,
            cognitive_events: None,
            started_at: None,
            finalized_at: None,
        }
    }

    #[test]
    fn handoffs_are_rejected_only_for_stopped_pairs() {
        assert_eq!(
            handoff_rejection(&PairStatus::Paused).as_deref(),
            Some("HANDOFF_IGNORED: pair is paused")
        );
        assert_eq!(
            handoff_rejection(&PairStatus::Finished).as_deref(),
            Some("HANDOFF_IGNORED: pair is finished")
        );
        assert_eq!(
            handoff_rejection(&PairStatus::Error).as_deref(),
            Some("HANDOFF_IGNORED: pair is error")
        );
        // Plan approval/rejection is a role handoff from Awaiting Human Review.
        assert!(handoff_rejection(&PairStatus::AwaitingHumanReview).is_none());
        for status in [
            PairStatus::Idle,
            PairStatus::Mentoring,
            PairStatus::Executing,
            PairStatus::Reviewing,
        ] {
            assert!(handoff_rejection(&status).is_none(), "{:?}", status);
        }
    }

    #[test]
    fn plan_gate_approval_hands_the_gated_plan_to_the_executor() {
        let (manager, broker, _spawner) = make_test_pair_manager();
        let pair_id = "test-plan-gate";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 1);
        {
            let broker = broker.lock().unwrap();
            let mut state = broker.get_state(pair_id).unwrap();
            state.messages = vec![message(
                MessageSender::Mentor,
                MessageType::Plan,
                "1. Do it",
            )];
            broker.restore_state(state).unwrap();
            broker.set_pair_status(pair_id, PairStatus::AwaitingHumanReview, None);
        }

        let status = broker.lock().unwrap().status_and_turn(pair_id).unwrap().0;
        assert!(handoff_rejection(&status).is_none());

        // Approve: the executor starts on the same iteration.
        broker.lock().unwrap().prepare_run(pair_id, "executor");
        let state = broker.lock().unwrap().get_state(pair_id).unwrap();
        assert_eq!(state.status, PairStatus::Executing);
        assert_eq!(state.turn, AgentRole::Executor);
        assert_eq!(state.iteration, 1);
    }

    #[test]
    fn plan_gate_rejection_replans_instead_of_reviewing() {
        let (manager, broker, _spawner) = make_test_pair_manager();
        let pair_id = "test-plan-reject";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 1);
        broker
            .lock()
            .unwrap()
            .set_pair_status(pair_id, PairStatus::AwaitingHumanReview, None);

        broker.lock().unwrap().prepare_run(pair_id, "mentor");
        let state = broker.lock().unwrap().get_state(pair_id).unwrap();
        assert_eq!(state.status, PairStatus::Mentoring);
        assert_eq!(state.iteration, 1);
    }

    #[tokio::test]
    async fn resuming_a_paused_planning_turn_uses_the_planning_prompt() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-resume-plan";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 1);
        {
            let broker = broker.lock().unwrap();
            let mut state = broker.get_state(pair_id).unwrap();
            state.task_spec = "Add a dark mode toggle".to_string();
            broker.restore_state(state).unwrap();
        }

        let (role, prompt) = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE)
            .await
            .expect("resume should succeed");

        assert_eq!(role, "mentor");
        assert_eq!(
            prompt,
            build_mentor_planning_prompt("Add a dark mode toggle")
        );
        assert!(!prompt.contains("EXECUTOR OUTPUT"));
    }

    #[tokio::test]
    async fn resuming_after_a_budget_pause_reviews_the_executor_work() {
        // budget = 1: the executor ran iteration 1 and the budget pause handed
        // the turn to the mentor. Resume must review, not re-plan.
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-resume-budget";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 1);
        {
            let broker = broker.lock().unwrap();
            let mut state = broker.get_state(pair_id).unwrap();
            state.task_spec = "Fix the parser".to_string();
            state.messages = vec![
                message(MessageSender::Mentor, MessageType::Plan, "1. Fix it"),
                message(
                    MessageSender::Executor,
                    MessageType::Result,
                    "Fixed the parser",
                ),
            ];
            broker.restore_state(state).unwrap();
        }

        let (role, prompt) = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE)
            .await
            .expect("resume should succeed");

        assert_eq!(role, "mentor");
        assert!(prompt.contains("Fixed the parser"));
        assert_eq!(
            broker.lock().unwrap().get_state(pair_id).unwrap().status,
            PairStatus::Reviewing
        );
    }

    #[tokio::test]
    async fn resume_cannot_start_the_same_turn_twice() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-double-resume";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Executor, 2);

        let first = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE).await;
        let second = resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE).await;

        assert!(first.is_ok());
        assert!(
            second.is_err(),
            "a double-click must not spawn a second turn"
        );
    }

    #[tokio::test]
    async fn resume_invalidates_the_previous_turn() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-resume-generation";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Executor, 2);
        spawner.pair_contexts.lock().unwrap().insert(
            pair_id.to_string(),
            crate::harness::driver::ProcessContext {
                directory: "/tmp/test".to_string(),
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: None,
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 7,
                is_smoke_test: false,
                mentor_selection: None,
                executor_selection: None,
                mentor_history: Vec::new(),
                executor_history: Vec::new(),
            },
        );

        resume_pair_core(&manager, &broker, &spawner, pair_id, RETRYABLE)
            .await
            .unwrap();

        assert_eq!(
            spawner.pair_contexts.lock().unwrap()[pair_id].run_generation,
            8
        );
    }

    #[test]
    fn register_pair_rejects_a_branch_another_worktree_pair_holds() {
        let mut manager = PairManager::new();
        let broker = MessageBroker::new();
        let workspace = PairWorkspace {
            directory: "/tmp/repo/.worktrees/pair-a".to_string(),
            branch: Some("feature".to_string()),
            repo_path: Some("/tmp/repo".to_string()),
            worktree_path: Some("/tmp/repo/.worktrees/pair-a".to_string()),
            worktree_branch: Some("the-pair/pair-a".to_string()),
        };
        manager
            .register_pair("pair-a", &sample_input(), workspace.clone(), &broker)
            .unwrap();

        let second = manager.register_pair("pair-b", &sample_input(), workspace, &broker);
        assert!(second.is_err());
        assert!(manager.get_pair("pair-b").is_none());
        assert_eq!(manager.branches_in_use(), vec!["feature".to_string()]);
    }

    #[test]
    fn register_pair_records_the_task_spec_for_the_backend() {
        let mut manager = PairManager::new();
        let broker = MessageBroker::new();
        let pair = manager.create_pair(&sample_input(), &broker).unwrap();

        let state = broker.get_state(&pair.pair_id).unwrap();
        assert_eq!(state.task_spec, "Build the feature");
        assert!(state.run_started_at.is_some());
    }

    /// Throwaway repository under the system temp dir, removed on drop.
    struct TempRepo {
        root: std::path::PathBuf,
        repo: std::path::PathBuf,
    }

    impl TempRepo {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "the-pair-pair-manager-test-{}-{}-{}",
                label,
                std::process::id(),
                nanos
            ));
            let repo = root.join("repo");
            std::fs::create_dir_all(&repo).unwrap();
            let repo = repo.canonicalize().unwrap();
            let temp = TempRepo { root, repo };
            temp.git(&temp.repo, &["init", "-q"]);
            temp.git(&temp.repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
            temp.git(&temp.repo, &["config", "user.name", "Test"]);
            temp.git(&temp.repo, &["config", "user.email", "test@example.com"]);
            temp.git(&temp.repo, &["config", "commit.gpgsign", "false"]);
            std::fs::write(temp.repo.join("README.md"), "hello\n").unwrap();
            temp.git(&temp.repo, &["add", "README.md"]);
            temp.git(&temp.repo, &["commit", "-q", "--no-verify", "-m", "init"]);
            temp
        }

        fn git(&self, dir: &std::path::Path, args: &[&str]) -> String {
            let output = std::process::Command::new("git")
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

        fn repo_str(&self) -> &str {
            self.repo.to_str().unwrap()
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn worktree_input(directory: &str) -> CreatePairInput {
        CreatePairInput {
            name: "Worktree Pair".to_string(),
            directory: directory.to_string(),
            spec: "Isolated task".to_string(),
            mentor: AgentConfig {
                role: AgentRole::Mentor,
                model: "mentor-model".to_string(),
                reasoning_effort: None,
            },
            executor: AgentConfig {
                role: AgentRole::Executor,
                model: "executor-model".to_string(),
                reasoning_effort: None,
            },
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            branch: None,
            use_worktree: Some(true),
            max_iterations: None,
            plan_gate: None,
        }
    }

    fn git_pair(pair_id: &str, directory: &str, branch: Option<&str>) -> Pair {
        Pair {
            pair_id: pair_id.to_string(),
            name: "Worktree Pair".to_string(),
            directory: directory.to_string(),
            status: PairStatus::Idle,
            mentor_model: "mentor-model".to_string(),
            executor_model: "executor-model".to_string(),
            pending_mentor_model: None,
            pending_executor_model: None,
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            created_at: 0,
            branch: branch.map(str::to_string),
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            plan_gate: false,
        }
    }

    #[test]
    fn prepare_workspace_use_worktree_creates_worktree_from_current_branch() {
        let temp = TempRepo::new("use-worktree-current");
        // A dirty main tree must not block isolation: `git worktree add`
        // never touches it.
        std::fs::write(temp.repo.join("dirty.txt"), "uncommitted\n").unwrap();

        let workspace = prepare_workspace(&worktree_input(temp.repo_str()), "pair-wt1", &[])
            .expect("worktree workspace");

        let worktree_path =
            std::path::PathBuf::from(workspace.worktree_path.clone().expect("worktree path"));
        assert!(worktree_path.join("README.md").exists());
        assert!(!worktree_path.join("dirty.txt").exists());
        // The pair starts from the current branch but does not hold it.
        assert_eq!(workspace.branch, None);
        assert_eq!(workspace.repo_path.as_deref(), Some(temp.repo_str()));
        assert_eq!(
            workspace.worktree_branch.as_deref(),
            Some("the-pair/pair-pair-wt1")
        );
        assert_eq!(
            temp.git(&worktree_path, &["rev-parse", "HEAD"]),
            temp.git(&temp.repo, &["rev-parse", "main"])
        );
        assert!(temp.repo.join("dirty.txt").exists());
    }

    #[test]
    fn prepare_workspace_use_worktree_ignores_branch_held_by_another_pair() {
        let temp = TempRepo::new("use-worktree-shared-base");
        // Another pair holds the current branch as its base; a worktree pair
        // starting from the same branch doesn't hold it, so no conflict.
        let workspace = prepare_workspace(
            &worktree_input(temp.repo_str()),
            "pair-wt2",
            &["main".to_string()],
        )
        .expect("worktree workspace despite main being held elsewhere");
        assert_eq!(workspace.branch, None);
    }

    #[test]
    fn prepare_workspace_use_worktree_with_explicit_base_creates_worktree() {
        let temp = TempRepo::new("use-worktree-base");
        temp.git(&temp.repo, &["branch", "feature"]);

        let mut input = worktree_input(temp.repo_str());
        input.branch = Some("feature".to_string());

        let workspace = prepare_workspace(&input, "pair-wt3", &[]).expect("worktree workspace");
        let worktree_path = std::path::PathBuf::from(workspace.worktree_path.clone().unwrap());
        assert_eq!(
            temp.git(&worktree_path, &["rev-parse", "HEAD"]),
            temp.git(&temp.repo, &["rev-parse", "feature"])
        );
        // An explicitly picked base is held by the pair.
        assert_eq!(workspace.branch.as_deref(), Some("feature"));
        assert_eq!(
            workspace.worktree_branch.as_deref(),
            Some("the-pair/pair-pair-wt3")
        );
    }

    #[test]
    fn prepare_workspace_use_worktree_keeps_requiring_clean_repo_for_other_base() {
        let temp = TempRepo::new("use-worktree-dirty-other-base");
        temp.git(&temp.repo, &["branch", "feature"]);
        std::fs::write(temp.repo.join("dirty.txt"), "uncommitted\n").unwrap();

        let mut input = worktree_input(temp.repo_str());
        input.branch = Some("feature".to_string());

        let error = prepare_workspace(&input, "pair-wt4", &[]).unwrap_err();
        assert!(error.contains("uncommitted changes"));
    }

    #[test]
    fn prepare_workspace_use_worktree_rejects_non_git_directory() {
        let temp = TempRepo::new("use-worktree-non-git");
        let plain = temp.root.join("plain");
        std::fs::create_dir_all(&plain).unwrap();

        let error = prepare_workspace(&worktree_input(plain.to_str().unwrap()), "pair-wt5", &[])
            .unwrap_err();
        assert!(error.contains("not a git repository"));
    }

    #[test]
    fn prepare_workspace_rejects_non_git_directory_for_a_legacy_branch_pair() {
        let temp = TempRepo::new("legacy-non-git");
        let plain = temp.root.join("plain");
        std::fs::create_dir_all(&plain).unwrap();

        let mut input = worktree_input(plain.to_str().unwrap());
        input.use_worktree = None;
        input.branch = Some("main".to_string());

        let error = prepare_workspace(&input, "pair-legacy", &[]).unwrap_err();
        assert!(error.contains("not a git repository"));
    }

    #[test]
    fn prepare_workspace_without_opt_in_still_works_in_place() {
        let temp = TempRepo::new("use-worktree-off");
        let mut input = worktree_input(temp.repo_str());
        input.use_worktree = None;
        input.branch = Some("main".to_string());

        // Legacy behavior: picking the current branch works in-place.
        let workspace = prepare_workspace(&input, "pair-wt6", &[]).unwrap();
        assert_eq!(workspace.directory, temp.repo_str());
        assert_eq!(workspace.worktree_path, None);
    }

    #[test]
    fn rotate_workspace_preserves_old_work_and_starts_fresh() {
        let temp = TempRepo::new("rotate-preserve");
        let old =
            create_worktree(temp.repo_str(), "main", ".worktrees/pair-old").expect("old worktree");
        let old = std::path::PathBuf::from(old);
        std::fs::write(old.join("work.txt"), "committed work\n").unwrap();
        temp.git(&old, &["add", "work.txt"]);
        temp.git(&old, &["commit", "-q", "--no-verify", "-m", "pair work"]);
        let old_head = temp.git(&old, &["rev-parse", "HEAD"]);
        std::fs::write(old.join("uncommitted.txt"), "in flight\n").unwrap();

        let mut pair = git_pair("pair-rot", old.to_str().unwrap(), Some("main"));
        pair.worktree_path = Some(old.to_str().unwrap().to_string());
        pair.worktree_branch = Some("the-pair/pair-old".to_string());
        pair.repo_path = Some(temp.repo_str().to_string());

        let workspace = rotate_workspace(&pair).expect("rotation");

        // The old worktree is gone; its commits survive on its branch and its
        // uncommitted change was stashed.
        assert!(!old.exists());
        assert_eq!(
            temp.git(&temp.repo, &["rev-parse", "--verify", "the-pair/pair-old"]),
            old_head
        );

        let new = std::path::PathBuf::from(workspace.worktree_path.clone().unwrap());
        assert!(new.join("README.md").exists());
        assert!(!new.join("work.txt").exists());
        assert_eq!(
            temp.git(&new, &["rev-parse", "HEAD"]),
            temp.git(&temp.repo, &["rev-parse", "main"])
        );
        assert_eq!(workspace.branch.as_deref(), Some("main"));
        assert_eq!(
            workspace.worktree_branch,
            Some(format!(
                "the-pair/{}",
                new.file_name().unwrap().to_str().unwrap()
            ))
        );
        assert_eq!(workspace.repo_path.as_deref(), Some(temp.repo_str()));
    }

    #[test]
    fn rotate_workspace_converts_an_in_place_pair_to_a_worktree() {
        let temp = TempRepo::new("rotate-in-place");
        let pair = git_pair("pair-inplace", temp.repo_str(), None);

        let workspace = rotate_workspace(&pair).expect("rotation");

        let new = std::path::PathBuf::from(workspace.worktree_path.clone().unwrap());
        assert!(new.join("README.md").exists());
        // An in-place pair has no held base; the fresh worktree starts from
        // the current branch and still holds nothing.
        assert_eq!(workspace.branch, None);
        assert_eq!(workspace.repo_path.as_deref(), Some(temp.repo_str()));
        assert_ne!(workspace.directory, temp.repo_str());
    }

    #[test]
    fn rotate_workspace_rejects_a_non_git_directory() {
        let temp = TempRepo::new("rotate-non-git");
        let plain = temp.root.join("plain");
        std::fs::create_dir_all(&plain).unwrap();

        let pair = git_pair("pair-plain", plain.to_str().unwrap(), None);
        let error = rotate_workspace(&pair).unwrap_err();
        assert!(error.contains("not a git repository"));
    }

    #[test]
    fn rotate_workspace_rejects_a_repository_without_a_current_branch() {
        let temp = TempRepo::new("rotate-detached");
        temp.git(&temp.repo, &["checkout", "-q", "--detach", "HEAD"]);

        let pair = git_pair("pair-detached", temp.repo_str(), None);
        let error = rotate_workspace(&pair).unwrap_err();
        assert!(error.contains("no current branch"));
    }

    #[test]
    fn rotate_workspace_keeps_requiring_clean_repo_for_a_held_base() {
        let temp = TempRepo::new("rotate-dirty-held-base");
        temp.git(&temp.repo, &["branch", "feature"]);
        std::fs::write(temp.repo.join("dirty.txt"), "uncommitted\n").unwrap();
        let old = create_worktree(temp.repo_str(), "feature", ".worktrees/pair-old")
            .expect("old worktree");

        let mut pair = git_pair("pair-held", &old, Some("feature"));
        pair.worktree_path = Some(old.clone());
        pair.repo_path = Some(temp.repo_str().to_string());

        let error = rotate_workspace(&pair).unwrap_err();
        assert!(error.contains("uncommitted changes"));
        // Nothing was removed — the pair keeps its workspace on failure.
        assert!(std::path::PathBuf::from(&old).exists());
    }

    #[test]
    fn apply_workspace_copies_the_rotated_workspace_onto_the_pair() {
        let workspace = PairWorkspace {
            directory: "/repo/.worktrees/pair-new".to_string(),
            branch: Some("main".to_string()),
            repo_path: Some("/repo".to_string()),
            worktree_path: Some("/repo/.worktrees/pair-new".to_string()),
            worktree_branch: Some("the-pair/pair-new".to_string()),
        };

        let pair = apply_workspace(git_pair("pair-apply", "/repo", None), &workspace);

        assert_eq!(pair.directory, "/repo/.worktrees/pair-new");
        assert_eq!(pair.branch.as_deref(), Some("main"));
        assert_eq!(pair.repo_path.as_deref(), Some("/repo"));
        assert_eq!(
            pair.worktree_path.as_deref(),
            Some("/repo/.worktrees/pair-new")
        );
        assert_eq!(pair.worktree_branch.as_deref(), Some("the-pair/pair-new"));
    }

    fn base_assign(spec: &str) -> crate::types::AssignTaskInput {
        crate::types::AssignTaskInput {
            spec: spec.to_string(),
            role: None,
            fresh_worktree: None,
        }
    }

    fn assign_broker() -> Mutex<MessageBroker> {
        Mutex::new(MessageBroker::new())
    }

    #[test]
    fn prepare_run_context_uses_the_worktree_directory_and_keeps_sessions_for_handoffs() {
        let spawner = TurnDriver::new();
        let mut pair = git_pair("pair-ctx", "/repo", Some("main"));
        pair.worktree_path = Some("/repo/.worktrees/pair-abc".to_string());
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-ctx".to_string(),
            crate::harness::driver::ProcessContext {
                directory: "/repo".to_string(),
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: Some("mentor-session".to_string()),
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 7,
                is_smoke_test: false,
                mentor_selection: None,
                executor_selection: None,
                mentor_history: Vec::new(),
                executor_history: Vec::new(),
            },
        );

        // A handoff keeps provider sessions and the run generation, and runs in
        // the pair's worktree.
        prepare_run_context(
            &spawner,
            &pair,
            "pair-ctx",
            false,
            RoleSelections::default(),
        );
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx")
            .cloned()
            .unwrap();
        assert_eq!(ctx.directory, "/repo/.worktrees/pair-abc");
        assert_eq!(ctx.mentor_session_id.as_deref(), Some("mentor-session"));
        assert_eq!(ctx.run_generation, 7);

        // A new run clears sessions and bumps the generation.
        prepare_run_context(&spawner, &pair, "pair-ctx", true, RoleSelections::default());
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx")
            .cloned()
            .unwrap();
        assert_eq!(ctx.mentor_session_id, None);
        assert_eq!(ctx.run_generation, 8);
        assert_eq!(ctx.directory, "/repo/.worktrees/pair-abc");
    }

    #[test]
    fn killing_a_turn_with_no_process_still_supersedes_it() {
        // A turn on the direct connection has no child process, so a kill that
        // demanded one would report an error and leave the pair Mentoring until
        // the stall detector gave up on it.
        let spawner = TurnDriver::new();
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-killed".to_string(),
            crate::harness::driver::ProcessContext {
                directory: "/repo".to_string(),
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: None,
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 4,
                is_smoke_test: false,
                mentor_selection: None,
                executor_selection: None,
                mentor_history: Vec::new(),
                executor_history: Vec::new(),
            },
        );

        // The turn registered its own flag when it started.
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        spawner.turn_cancels.lock().unwrap().insert(
            "pair-killed-mentor".to_string(),
            std::sync::Arc::clone(&flag),
        );

        // There is no process to kill: stopping a turn only raises its cancel
        // flag, and the generation bump is what stops a streamed turn.
        spawner.stop_process("pair-killed", "mentor");
        spawner.bump_run_generation("pair-killed");
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-killed")
            .cloned()
            .unwrap();
        assert_eq!(ctx.run_generation, 5);
        assert!(
            flag.load(std::sync::atomic::Ordering::SeqCst),
            "the in-flight turn's cancel flag is raised"
        );
    }

    #[test]
    fn prepare_run_context_keeps_the_harness_history_only_for_the_same_run() {
        let spawner = TurnDriver::new();
        let pair = git_pair("pair-ctx-history", "/repo", Some("main"));
        let said = vec![serde_json::json!({"role": "user", "content": "earlier turn"})];
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-ctx-history".to_string(),
            crate::harness::driver::ProcessContext {
                directory: "/repo".to_string(),
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: None,
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 3,
                is_smoke_test: false,
                mentor_selection: None,
                executor_selection: None,
                mentor_history: said.clone(),
                executor_history: said.clone(),
            },
        );
        let nowhere = RoleSelections::default();

        // A handoff inside the same run replays what the agents already said.
        prepare_run_context(&spawner, &pair, "pair-ctx-history", false, nowhere.clone());
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx-history")
            .cloned()
            .unwrap();
        assert_eq!(ctx.mentor_history, said);
        assert_eq!(ctx.executor_history, said);
        assert_eq!(ctx.run_generation, 3);

        // A new run is a new conversation.
        prepare_run_context(&spawner, &pair, "pair-ctx-history", true, nowhere);
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx-history")
            .cloned()
            .unwrap();
        assert!(ctx.mentor_history.is_empty());
        assert!(ctx.executor_history.is_empty());
    }

    /// The selection `test-mentor` resolves to in `app_dir_with_selection`.
    fn resolved_selection() -> crate::harness::ModelSelection {
        crate::harness::ModelSelection {
            provider_id: "moved-1".into(),
            account_id: "moved-1-a".into(),
            model: "test-mentor".into(),
            effort: None,
            backup: None,
        }
    }

    /// The same model on another endpoint, so the target has moved.
    fn elsewhere_selection() -> crate::harness::ModelSelection {
        crate::harness::ModelSelection {
            provider_id: "stayed-1".into(),
            account_id: "stayed-1-a".into(),
            model: "test-mentor".into(),
            effort: None,
            backup: None,
        }
    }

    #[test]
    fn prepare_run_context_clears_the_session_when_the_target_moved() {
        let spawner = TurnDriver::new();
        let pair = git_pair("pair-ctx-swap", "/repo", Some("main"));
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-ctx-swap".to_string(),
            crate::harness::driver::ProcessContext {
                directory: "/repo".to_string(),
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: Some("mentor-session".to_string()),
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 1,
                is_smoke_test: false,
                // The pair was talking somewhere else last turn.
                mentor_selection: Some(elsewhere_selection()),
                executor_selection: None,
                mentor_history: vec![serde_json::json!({"role": "user", "content": "earlier"})],
                executor_history: Vec::new(),
            },
        );

        // The model id resolves onto a different endpoint, so neither the
        // session nor the conversation may carry over.
        let moved = RoleSelections {
            mentor: Some(resolved_selection()),
            executor: None,
        };
        prepare_run_context(&spawner, &pair, "pair-ctx-swap", false, moved);
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx-swap")
            .cloned()
            .unwrap();
        assert_eq!(ctx.mentor_session_id, None);
        assert!(ctx.mentor_history.is_empty());
    }

    #[test]
    fn validate_assign_task_accepts_a_new_run_for_an_existing_pair() {
        let manager = Mutex::new(PairManager::new());
        manager
            .lock()
            .unwrap()
            .upsert_pair(git_pair("pair-ok", "/tmp/repo", None));
        let broker = assign_broker();

        let (pair, role, is_new_run, fresh_worktree) = validate_assign_task(
            &manager,
            &broker,
            "pair-ok",
            &base_assign("go the extra mile"),
        )
        .expect("a valid new-run assignment");

        assert_eq!(pair.pair_id, "pair-ok");
        assert_eq!(role, "mentor", "a new run starts with the mentor");
        assert!(is_new_run);
        assert!(!fresh_worktree);
    }

    #[test]
    fn prepare_run_context_clears_a_session_when_the_role_moved_endpoints() {
        let spawner = TurnDriver::new();
        let pair = git_pair("pair-ctx-exec", "/repo", Some("main"));
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-ctx-exec".to_string(),
            crate::harness::driver::ProcessContext {
                directory: "/repo".to_string(),
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: None,
                executor_session_id: Some("executor-session".to_string()),
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 1,
                is_smoke_test: false,
                // The executor was talking to another endpoint last turn, so
                // neither its session nor its conversation may carry over.
                mentor_selection: None,
                executor_selection: Some(elsewhere_selection()),
                mentor_history: Vec::new(),
                executor_history: vec![serde_json::json!({"role": "user", "content": "earlier"})],
            },
        );

        let moved = RoleSelections {
            mentor: None,
            executor: Some(resolved_selection()),
        };
        prepare_run_context(&spawner, &pair, "pair-ctx-exec", false, moved);
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx-exec")
            .cloned()
            .unwrap();
        assert_eq!(ctx.executor_session_id, None);
        assert!(ctx.executor_history.is_empty());
    }

    #[test]
    fn prepare_run_context_keeps_a_session_when_the_target_did_not_move() {
        let spawner = TurnDriver::new();
        let mut pair = git_pair("pair-ctx-same", "/repo", Some("main"));
        pair.mentor_model = "test-mentor".to_string();
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-ctx-same".to_string(),
            crate::harness::driver::ProcessContext {
                directory: "/repo".to_string(),
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: Some("mentor-session".to_string()),
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 1,
                is_smoke_test: false,
                mentor_selection: Some(resolved_selection()),
                executor_selection: None,
                mentor_history: vec![serde_json::json!({"role": "user", "content": "earlier"})],
                executor_history: Vec::new(),
            },
        );

        // The same endpoint, so the handoff keeps both the session and the
        // conversation the mentor already had.
        let stayed = RoleSelections {
            mentor: Some(resolved_selection()),
            executor: None,
        };
        prepare_run_context(&spawner, &pair, "pair-ctx-same", false, stayed);
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx-same")
            .cloned()
            .unwrap();
        assert_eq!(ctx.mentor_session_id.as_deref(), Some("mentor-session"));
        assert_eq!(ctx.mentor_history.len(), 1);
    }

    /// An app data dir whose registry resolves `test-mentor` onto `moved-1`, with
    /// a second endpoint listing the same model so a target change is observable.
    fn app_dir_with_selection() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("the-pair-target-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::accounts::store::save(
            &dir,
            &[
                crate::accounts::types::Provider {
                    id: "moved-1".into(),
                    protocol: crate::accounts::types::Protocol::Anthropic,
                    base_url: "https://moved.example".into(),
                    vendor: "moved".into(),
                    display_name: "Moved".into(),
                    accounts: vec![crate::accounts::types::Account {
                        id: "moved-1-a".into(),
                        kind: crate::accounts::types::AccountKind::ApiKey,
                        label: "moved".into(),
                        source: "manual".into(),
                        status: crate::accounts::types::AccountStatus::Ready,
                        headers: vec![],
                        tool_names: None,
                        secret: crate::accounts::types::Secret::ApiKey("k".into()),
                    }],
                    models: vec![model_route("test-mentor")],
                },
                // A second endpoint listing the same model, so a selection can be
                // told apart from the one resolution picks.
                crate::accounts::types::Provider {
                    id: "stayed-1".into(),
                    protocol: crate::accounts::types::Protocol::Anthropic,
                    base_url: "https://stayed.example".into(),
                    vendor: "stayed".into(),
                    display_name: "Stayed".into(),
                    accounts: vec![crate::accounts::types::Account {
                        id: "stayed-1-a".into(),
                        kind: crate::accounts::types::AccountKind::ApiKey,
                        label: "stayed".into(),
                        source: "manual".into(),
                        status: crate::accounts::types::AccountStatus::Ready,
                        headers: vec![],
                        tool_names: None,
                        secret: crate::accounts::types::Secret::ApiKey("k".into()),
                    }],
                    models: vec![model_route("test-mentor")],
                },
            ],
        )
        .unwrap();
        dir
    }

    fn model_route(id: &str) -> crate::accounts::types::ModelRoute {
        crate::accounts::types::ModelRoute {
            id: id.into(),
            display_name: id.into(),
            context_window: None,
            max_output: None,
            efforts: vec![],
            default_effort: None,
            price: None,
            source: crate::accounts::types::ModelSource::Manual,
        }
    }

    #[test]
    fn validate_assign_task_rejects_an_invalid_role() {
        let manager = Mutex::new(PairManager::new());
        let broker = assign_broker();
        let mut input = base_assign("go");
        input.role = Some("planner".to_string());

        let error = validate_assign_task(&manager, &broker, "pair-1", &input).unwrap_err();
        assert!(error.contains("Invalid role"));
    }

    #[test]
    fn validate_assign_task_rejects_fresh_worktree_for_a_handoff() {
        let manager = Mutex::new(PairManager::new());
        let broker = assign_broker();
        let mut input = base_assign("go");
        input.role = Some("mentor".to_string());
        input.fresh_worktree = Some(true);

        let error = validate_assign_task(&manager, &broker, "pair-1", &input).unwrap_err();
        assert!(error.contains("only be created for a new task run"));
    }

    #[test]
    fn validate_assign_task_reports_a_missing_pair() {
        let manager = Mutex::new(PairManager::new());
        let broker = assign_broker();

        let error = validate_assign_task(&manager, &broker, "pair-missing", &base_assign("go"))
            .unwrap_err();
        assert!(error.contains("Pair not found"));
    }

    #[test]
    fn validate_assign_task_rejects_a_stale_handoff_for_a_stopped_pair() {
        let (manager, broker, _spawner) = make_test_pair_manager();
        insert_paused_pair(&manager, &broker, "pair-stopped", AgentRole::Mentor, 2);

        let mut input = base_assign("go");
        input.role = Some("mentor".to_string());

        let error = validate_assign_task(&manager, &broker, "pair-stopped", &input).unwrap_err();
        assert!(error.contains("HANDOFF_IGNORED"));
    }

    #[tokio::test]
    async fn prepare_new_run_workspace_rotates_and_updates_manager_and_broker() {
        let temp = TempRepo::new("rotate-command");
        let old = std::path::PathBuf::from(
            create_worktree(temp.repo_str(), "main", ".worktrees/pair-old").expect("old worktree"),
        );
        std::fs::write(old.join("scratch.txt"), "work in flight\n").unwrap();

        let mut pair = git_pair("pair-rot-cmd", old.to_str().unwrap(), Some("main"));
        pair.repo_path = Some(temp.repo_str().to_string());
        pair.worktree_path = Some(old.to_str().unwrap().to_string());
        pair.worktree_branch = Some("the-pair/pair-old".to_string());

        let manager = Mutex::new(PairManager::new());
        manager.lock().unwrap().upsert_pair(pair.clone());
        let broker = assign_broker();
        broker
            .lock()
            .unwrap()
            .initialize_pair(
                "pair-rot-cmd",
                worktree_input(temp.repo_str()),
                Some(old.to_str().unwrap()),
            )
            .unwrap();

        let rotated = prepare_new_run_workspace(
            &manager,
            &broker,
            &TurnDriver::new(),
            "pair-rot-cmd",
            pair,
            true,
        )
        .await
        .expect("rotation");

        // The old worktree is preserved and removed; the pair runs in the new.
        assert!(!old.exists());
        let new_path = rotated.worktree_path.clone().expect("new worktree");
        assert_ne!(new_path, old.to_str().unwrap());
        assert!(std::path::PathBuf::from(&new_path)
            .join("README.md")
            .exists());
        assert!(rotated.worktree_branch.is_some());
        assert_eq!(rotated.branch.as_deref(), Some("main"));

        // Both the pair record and the broker state follow the new worktree.
        let stored = manager.lock().unwrap().get_pair("pair-rot-cmd").unwrap();
        assert_eq!(stored.worktree_path.as_deref(), Some(new_path.as_str()));
        let state = broker.lock().unwrap().get_state("pair-rot-cmd").unwrap();
        assert_eq!(state.directory, new_path);
    }

    #[tokio::test]
    async fn prepare_new_run_workspace_keeps_the_pair_put_when_rotation_fails() {
        let temp = TempRepo::new("rotate-command-fail");
        let old =
            create_worktree(temp.repo_str(), "main", ".worktrees/pair-old").expect("old worktree");
        // A held base other than the current branch requires a clean repo; a
        // dirty tree makes the rotation refuse before touching the old work.
        temp.git(&temp.repo, &["branch", "feature"]);
        std::fs::write(temp.repo.join("dirty.txt"), "uncommitted\n").unwrap();

        let mut pair = git_pair("pair-rot-fail", &old, Some("feature"));
        pair.repo_path = Some(temp.repo_str().to_string());
        pair.worktree_path = Some(old.clone());
        pair.worktree_branch = Some("the-pair/pair-old".to_string());

        let manager = Mutex::new(PairManager::new());
        manager.lock().unwrap().upsert_pair(pair.clone());
        let broker = assign_broker();
        broker
            .lock()
            .unwrap()
            .initialize_pair("pair-rot-fail", worktree_input(temp.repo_str()), Some(&old))
            .unwrap();

        let error = prepare_new_run_workspace(
            &manager,
            &broker,
            &TurnDriver::new(),
            "pair-rot-fail",
            pair,
            true,
        )
        .await
        .expect_err("rotation should refuse a dirty repo for another base");
        assert!(error.contains("uncommitted changes"));

        let stored = manager.lock().unwrap().get_pair("pair-rot-fail").unwrap();
        assert_eq!(stored.worktree_path.as_deref(), Some(old.as_str()));
        assert_eq!(stored.branch.as_deref(), Some("feature"));
        assert!(std::path::PathBuf::from(&old).exists());
        let state = broker.lock().unwrap().get_state("pair-rot-fail").unwrap();
        assert_eq!(state.directory, old);
    }

    #[tokio::test]
    async fn prepare_new_run_workspace_without_the_flag_keeps_the_workspace() {
        let temp = TempRepo::new("rotate-command-off");
        let old =
            create_worktree(temp.repo_str(), "main", ".worktrees/pair-old").expect("old worktree");

        let mut pair = git_pair("pair-rot-off", &old, Some("main"));
        pair.repo_path = Some(temp.repo_str().to_string());
        pair.worktree_path = Some(old.clone());
        pair.worktree_branch = Some("the-pair/pair-old".to_string());

        let manager = Mutex::new(PairManager::new());
        manager.lock().unwrap().upsert_pair(pair.clone());

        let unchanged = prepare_new_run_workspace(
            &manager,
            &assign_broker(),
            &TurnDriver::new(),
            "pair-rot-off",
            pair,
            false,
        )
        .await
        .expect("no rotation requested");

        assert_eq!(unchanged.worktree_path.as_deref(), Some(old.as_str()));
        assert!(std::path::PathBuf::from(&old).exists());
    }

    #[test]
    fn prepare_workspace_use_worktree_rejects_a_repository_without_a_current_branch() {
        let temp = TempRepo::new("use-worktree-detached");
        temp.git(&temp.repo, &["checkout", "-q", "--detach", "HEAD"]);

        let error = prepare_workspace(&worktree_input(temp.repo_str()), "pair-wt-detached", &[])
            .unwrap_err();
        assert!(error.contains("no current branch"));
    }

    #[test]
    fn prepare_workspace_use_worktree_rejects_a_base_branch_another_pair_holds() {
        let temp = TempRepo::new("use-worktree-conflict");
        temp.git(&temp.repo, &["branch", "feature"]);

        let mut input = worktree_input(temp.repo_str());
        input.branch = Some("feature".to_string());

        let error =
            prepare_workspace(&input, "pair-wt-conflict", &["feature".to_string()]).unwrap_err();
        assert!(error.contains("already using branch"));
    }

    #[test]
    fn prepare_workspace_use_worktree_starts_from_a_remote_branch() {
        let temp = TempRepo::new("use-worktree-remote");
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

        let mut input = worktree_input(temp.repo_str());
        input.branch = Some("origin/feature".to_string());

        let workspace = prepare_workspace(&input, "pair-wt-remote", &[]).expect("remote base");
        let worktree_path = std::path::PathBuf::from(workspace.worktree_path.clone().unwrap());
        assert_eq!(temp.git(&worktree_path, &["rev-parse", "HEAD"]), sha);
        // The remote base is materialized into a local tracking branch the pair holds.
        assert_eq!(workspace.branch.as_deref(), Some("feature"));
    }
    fn models_input(
        mentor_model: &str,
        executor_model: &str,
        pending_mentor: Option<&str>,
    ) -> crate::types::UpdatePairModelsInput {
        crate::types::UpdatePairModelsInput {
            mentor_model: mentor_model.to_string(),
            executor_model: executor_model.to_string(),
            pending_mentor_model: pending_mentor.map(str::to_string),
            pending_executor_model: None,
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
        }
    }

    #[test]
    fn role_selections_resolve_each_roles_effective_model() {
        let dir = app_dir_with_selection();
        let mut pair = git_pair("pair-resolve", "/repo", None);
        pair.mentor_model = "test-mentor".to_string();

        // The executor's model is on no endpoint, so only the mentor resolves.
        let selections = RoleSelections::resolve(&dir, &pair, None);
        assert_eq!(
            selections.mentor.as_ref().map(|s| s.provider_id.as_str()),
            Some("moved-1")
        );
        assert!(selections.executor.is_none());

        // The pending model wins, so the executor resolves once one is picked.
        pair.pending_executor_model = Some("test-mentor".to_string());
        assert!(RoleSelections::resolve(&dir, &pair, None)
            .executor
            .is_some());
    }

    /// A context whose mentor talks to `selection`.
    fn context_on(
        selection: Option<crate::harness::ModelSelection>,
    ) -> crate::harness::driver::ProcessContext {
        crate::harness::driver::ProcessContext {
            directory: "/repo".to_string(),
            mentor_model: "test-mentor".to_string(),
            executor_model: "test-executor".to_string(),
            mentor_session_id: None,
            executor_session_id: None,
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            run_generation: 1,
            is_smoke_test: false,
            mentor_selection: selection,
            executor_selection: None,
            mentor_history: Vec::new(),
            executor_history: Vec::new(),
        }
    }

    #[test]
    fn a_handoff_keeps_a_role_on_the_endpoint_it_is_talking_to() {
        let dir = app_dir_with_selection();
        let mut pair = git_pair("pair-sticky", "/repo", None);
        pair.mentor_model = "test-mentor".to_string();

        // The id alone resolves onto `moved-1`, the first endpoint listing it —
        // but the mentor's conversation is with `stayed-1`.
        let current = context_on(Some(elsewhere_selection()));
        let selections = RoleSelections::resolve(&dir, &pair, Some(&current));
        assert_eq!(selections.mentor, Some(elsewhere_selection()));

        // A role with nothing to keep takes the endpoint its model was picked
        // under, ahead of the first one that lists the id.
        crate::harness::prefer_route(&dir, "mentor", "test-mentor", "stayed-1").unwrap();
        let fresh = RoleSelections::resolve(&dir, &pair, Some(&context_on(None)));
        assert_eq!(fresh.mentor, Some(elsewhere_selection()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn picking_the_same_model_under_another_endpoint_moves_the_role() {
        let dir = app_dir_with_selection();
        let mut pair = git_pair("pair-repick", "/repo", None);
        pair.mentor_model = "test-mentor".to_string();
        let spawner = TurnDriver::new();
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-repick".to_string(),
            context_on(Some(resolved_selection())),
        );

        // No pick on record: the role stays where it is, whatever the registry
        // would resolve today.
        assert_eq!(
            endpoint_moves(&dir, &spawner, &pair),
            ModelChange::default()
        );

        // The human picks the same model under the other endpoint.
        crate::harness::prefer_route(&dir, "mentor", "test-mentor", "stayed-1").unwrap();
        let moved = endpoint_moves(&dir, &spawner, &pair);
        assert_eq!(
            moved,
            ModelChange {
                mentor: true,
                executor: false
            }
        );

        // Invalidated, the next turn resolves onto the picked endpoint.
        invalidate_selections(&spawner, "pair-repick", &moved);
        let current = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-repick")
            .cloned();
        let selections = RoleSelections::resolve(&dir, &pair, current.as_ref());
        assert_eq!(selections.mentor, Some(elsewhere_selection()));

        // A pick that matches where the role already is moves nothing.
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-repick".to_string(),
            context_on(Some(elsewhere_selection())),
        );
        assert_eq!(
            endpoint_moves(&dir, &spawner, &pair),
            ModelChange::default()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ensure_role_resolvable_names_the_role_that_has_no_endpoint() {
        let error = ensure_role_resolvable("mentor", &RoleSelections::default())
            .expect_err("no endpoint anywhere");
        assert!(error.contains("mentor"), "{}", error);
        assert!(error.contains("no model it can talk to"), "{}", error);

        let only_executor = RoleSelections {
            mentor: None,
            executor: Some(resolved_selection()),
        };
        assert!(ensure_role_resolvable("executor", &only_executor).is_ok());
        assert!(ensure_role_resolvable("mentor", &only_executor).is_err());
    }

    #[tokio::test]
    async fn an_assignment_that_cannot_start_keeps_the_previous_run_intact() {
        // A new run whose mentor model resolves to nothing used to clear the
        // transcript and rotate the old worktree away before `start_turn`
        // failed — and the frontend only archives the previous run after a
        // successful assignment, so the pair was left with no run at all.
        let temp = TempRepo::new("assign-abort");
        let old = std::path::PathBuf::from(
            create_worktree(temp.repo_str(), "main", ".worktrees/pair-old").expect("old worktree"),
        );
        std::fs::write(old.join("work.txt"), "in flight\n").unwrap();

        let mut pair = git_pair("pair-abort", old.to_str().unwrap(), Some("main"));
        pair.repo_path = Some(temp.repo_str().to_string());
        pair.worktree_path = Some(old.to_str().unwrap().to_string());
        pair.worktree_branch = Some("the-pair/pair-old".to_string());

        let manager = Mutex::new(PairManager::new());
        manager.lock().unwrap().upsert_pair(pair.clone());
        let broker = assign_broker();
        broker
            .lock()
            .unwrap()
            .initialize_pair(
                "pair-abort",
                worktree_input(temp.repo_str()),
                Some(old.to_str().unwrap()),
            )
            .unwrap();
        {
            let broker = broker.lock().unwrap();
            let mut state = broker.get_state("pair-abort").unwrap();
            state.messages = vec![message(
                MessageSender::Mentor,
                MessageType::Plan,
                "the previous plan",
            )];
            broker.restore_state(state).unwrap();
        }
        let spawner = TurnDriver::new();
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-abort".to_string(),
            crate::harness::driver::ProcessContext {
                directory: old.to_str().unwrap().to_string(),
                mentor_model: "mentor-model".to_string(),
                executor_model: "executor-model".to_string(),
                mentor_session_id: None,
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 5,
                is_smoke_test: false,
                mentor_selection: None,
                executor_selection: None,
                mentor_history: Vec::new(),
                executor_history: Vec::new(),
            },
        );

        // No endpoint on the machine: the mentor's model resolves to nothing.
        let error = prepare_assignment(
            &manager,
            &broker,
            &spawner,
            "pair-abort",
            pair,
            "mentor",
            true,
            true,
            "New task",
            std::path::Path::new("/nonexistent-app-dir"),
        )
        .await
        .expect_err("the turn cannot start");
        assert!(error.contains("no model it can talk to"), "{}", error);

        // Nothing was destroyed: the old worktree still holds the work in flight.
        assert!(old.join("work.txt").exists());
        assert_eq!(
            manager
                .lock()
                .unwrap()
                .get_pair("pair-abort")
                .unwrap()
                .worktree_path
                .as_deref(),
            Some(old.to_str().unwrap()),
            "the pair keeps its workspace"
        );
        // The transcript is still there to read, and to archive by hand.
        let state = broker.lock().unwrap().get_state("pair-abort").unwrap();
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].content, "the previous plan");
        // The turn context was never rebuilt.
        assert_eq!(
            spawner.pair_contexts.lock().unwrap()["pair-abort"].run_generation,
            5
        );
    }

    #[tokio::test]
    async fn an_assignment_whose_model_resolves_runs_in_the_fresh_worktree() {
        let temp = TempRepo::new("assign-proceeds");
        let old = std::path::PathBuf::from(
            create_worktree(temp.repo_str(), "main", ".worktrees/pair-old").expect("old worktree"),
        );
        std::fs::write(old.join("work.txt"), "previous run\n").unwrap();

        let mut pair = git_pair("pair-go", old.to_str().unwrap(), Some("main"));
        pair.repo_path = Some(temp.repo_str().to_string());
        pair.worktree_path = Some(old.to_str().unwrap().to_string());
        pair.worktree_branch = Some("the-pair/pair-old".to_string());
        pair.mentor_model = "test-mentor".to_string();

        let manager = Mutex::new(PairManager::new());
        manager.lock().unwrap().upsert_pair(pair.clone());
        let broker = assign_broker();
        broker
            .lock()
            .unwrap()
            .initialize_pair(
                "pair-go",
                worktree_input(temp.repo_str()),
                Some(old.to_str().unwrap()),
            )
            .unwrap();
        let spawner = TurnDriver::new();

        let rotated = prepare_assignment(
            &manager,
            &broker,
            &spawner,
            "pair-go",
            pair,
            "mentor",
            true,
            true,
            "New task",
            &app_dir_with_selection(),
        )
        .await
        .expect("the mentor has an endpoint");

        // The pair moved into a fresh worktree, its old work preserved.
        assert!(!old.exists());
        let new_path = rotated.worktree_path.clone().expect("new worktree");
        assert!(std::path::PathBuf::from(&new_path)
            .join("README.md")
            .exists());
        assert_eq!(
            broker
                .lock()
                .unwrap()
                .get_state("pair-go")
                .unwrap()
                .directory,
            new_path
        );
        // The new run started, with the resolved selection installed.
        let state = broker.lock().unwrap().get_state("pair-go").unwrap();
        assert_eq!(state.status, PairStatus::Mentoring);
        assert_eq!(state.task_spec, "New task");
        assert_eq!(state.iteration, 1);
        let contexts = spawner.pair_contexts.lock().unwrap();
        assert_eq!(contexts["pair-go"].directory, new_path);
        assert_eq!(
            contexts["pair-go"]
                .mentor_selection
                .as_ref()
                .map(|s| s.provider_id.as_str()),
            Some("moved-1")
        );
    }

    #[test]
    fn a_turn_in_flight_is_visible_until_it_unwinds() {
        let spawner = TurnDriver::new();
        assert!(
            !turn_in_flight(&spawner, "pair-1"),
            "no turn registered yet"
        );

        // A turn registers its cancel flag when it starts.
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        spawner
            .turn_cancels
            .lock()
            .unwrap()
            .insert("pair-1-mentor".to_string(), std::sync::Arc::clone(&flag));
        assert!(turn_in_flight(&spawner, "pair-1"));

        // Stopping the pair raises that turn's flag.
        spawner.stop_pair_processes("pair-1");
        assert!(flag.load(std::sync::atomic::Ordering::SeqCst));

        // The other role has none, so the pair as a whole is still in flight.
        assert!(turn_in_flight(&spawner, "pair-1"));
        spawner.turn_cancels.lock().unwrap().remove("pair-1-mentor");
        assert!(!turn_in_flight(&spawner, "pair-1"));
    }

    #[tokio::test]
    async fn waiting_for_a_turn_to_stop_gives_up_after_the_bound() {
        let spawner = TurnDriver::new();
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        spawner
            .turn_cancels
            .lock()
            .unwrap()
            .insert("pair-1-mentor".to_string(), std::sync::Arc::clone(&flag));

        // A turn that has not unwound yet must not hang the delete, so the
        // wait is bounded: it reports the turn is still there.
        assert!(!wait_for_turns_to_stop(&spawner, "pair-1", Duration::from_millis(150)).await);

        // Once the turn clears its flag, the wait returns at once.
        spawner.turn_cancels.lock().unwrap().remove("pair-1-mentor");
        assert!(wait_for_turns_to_stop(&spawner, "pair-1", Duration::from_secs(5)).await);
        // A pair with no turn at all is stopped from the start.
        assert!(wait_for_turns_to_stop(&spawner, "pair-other", Duration::from_secs(5)).await);
    }

    #[test]
    fn changing_a_model_invalidates_the_stale_selection_for_that_role() {
        // The selection on the pair's context is what a turn talks to, and
        // `refresh_unresolved_selection` leaves a pair that already has one
        // alone — so a model change has to drop it, or the stale endpoint is
        // persisted with the snapshot and survives restarts.
        let spawner = TurnDriver::new();
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-models".to_string(),
            crate::harness::driver::ProcessContext {
                directory: "/repo".to_string(),
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: None,
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 1,
                is_smoke_test: false,
                mentor_selection: Some(resolved_selection()),
                executor_selection: Some(resolved_selection()),
                mentor_history: Vec::new(),
                executor_history: Vec::new(),
            },
        );
        let mut pair = git_pair("pair-models", "/repo", None);

        // The mentor moves to another model; the executor stays put.
        let change = apply_model_update(
            &mut pair,
            &models_input("new-mentor", "executor-model", None),
        );
        assert_eq!(
            change,
            ModelChange {
                mentor: true,
                executor: false
            }
        );

        invalidate_selections(&spawner, "pair-models", &change);
        let contexts = spawner.pair_contexts.lock().unwrap();
        assert!(
            contexts["pair-models"].mentor_selection.is_none(),
            "the next turn must re-resolve the new model"
        );
        assert!(
            contexts["pair-models"].executor_selection.is_some(),
            "the role that did not move keeps its endpoint"
        );

        // The same models again change nothing.
        let change = apply_model_update(
            &mut pair,
            &models_input("new-mentor", "executor-model", None),
        );
        assert!(!change.any());
        drop(contexts);
        invalidate_selections(&spawner, "pair-models", &change);
        assert!(spawner.pair_contexts.lock().unwrap()["pair-models"]
            .executor_selection
            .is_some());
    }

    #[test]
    fn a_pending_model_counts_as_the_role_that_moved() {
        let mut pair = git_pair("pair-pending", "/repo", None);
        pair.mentor_model = "test-mentor".to_string();

        // The human picked another model: it is pending, not yet the pair's own.
        let change = apply_model_update(
            &mut pair,
            &models_input("test-mentor", "executor-model", Some("swapped-mentor")),
        );
        assert!(change.mentor);
        assert!(!change.executor);
        assert_eq!(effective_model(&pair, "mentor"), "swapped-mentor");
    }
}
