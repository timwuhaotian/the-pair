use crate::acceptance::{
    build_executor_acceptance_followup_prompt, build_mentor_acceptance_prompt,
    build_mentor_acceptance_repair_prompt,
};
use crate::message_broker::MessageBroker;
use crate::process_spawner::ProcessSpawner;
use crate::provider_adapter::ProviderAdapter;
use crate::provider_registry::ProviderKind;
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
        return Err(
            "Directory is not a git repository. Cannot create a worktree.".to_string(),
        );
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
            mentor_provider: input.mentor.provider,
            mentor_model: input.mentor.model.clone(),
            executor_provider: input.executor.provider,
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

/// Start a turn; if it cannot start (spawn failure, missing binary, prompt
/// too large …) mark the pair as errored instead of leaving it looking busy.
async fn start_turn(
    app: &tauri::AppHandle,
    broker: &std::sync::Mutex<MessageBroker>,
    spawner: &ProcessSpawner,
    pair_id: &str,
    role: &str,
    prompt: String,
) -> Result<(), String> {
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

/// Contract C6: keep the provider already known for a model id and only infer
/// one for a model that changed (bare ids such as grok aliases or `codex-*`
/// slugs don't infer back to their provider).
fn resolve_provider_for_model(model: &str, known: &[(&str, ProviderKind)]) -> ProviderKind {
    known
        .iter()
        .find(|(known_model, _)| *known_model == model)
        .map(|(_, provider)| *provider)
        .unwrap_or_else(|| ProviderAdapter::infer_provider_kind(model))
}

#[tauri::command]
pub async fn pair_create(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, ProcessSpawner>,
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
            crate::process_spawner::ProcessContext {
                directory: pair
                    .worktree_path
                    .clone()
                    .unwrap_or_else(|| pair.directory.clone()),
                mentor_provider: pair.mentor_provider,
                executor_provider: pair.executor_provider,
                mentor_model: pair.mentor_model.clone(),
                executor_model: pair.executor_model.clone(),
                mentor_session_id: None,
                executor_session_id: None,
                mentor_reasoning_effort: input.mentor_reasoning_effort.clone(),
                executor_reasoning_effort: input.executor_reasoning_effort.clone(),
                run_generation: 0,
                is_smoke_test: false,
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
            .prepare_run(&pair_id, "mentor", spawner.active_processes.clone());

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
    spawner: &ProcessSpawner,
    pair_id: &str,
    pair: Pair,
    fresh_worktree: bool,
) -> Result<Pair, String> {
    if fresh_worktree {
        spawner.bump_run_generation(pair_id);
        let stopped = spawner.stop_pair_processes(pair_id);
        for mut child in stopped {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
        }
    } else {
        spawner.stop_pair_processes(pair_id);
    }

    if !fresh_worktree {
        return Ok(pair);
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

/// (Re)build the pair's process context for the turn about to start: resolve
/// effective providers for each role (contract C6), keep provider sessions
/// where possible, and bump the run generation for a new run.
fn prepare_run_context(
    spawner: &ProcessSpawner,
    pair: &Pair,
    pair_id: &str,
    is_new_run: bool,
) {
    let mut ctx_guard = spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let existing = ctx_guard.get(pair_id).cloned();

    let effective_mentor_model = pair
        .pending_mentor_model
        .clone()
        .unwrap_or_else(|| pair.mentor_model.clone());
    let effective_executor_model = pair
        .pending_executor_model
        .clone()
        .unwrap_or_else(|| pair.executor_model.clone());

    let mut known_mentor = vec![(pair.mentor_model.as_str(), pair.mentor_provider)];
    let mut known_executor = vec![(pair.executor_model.as_str(), pair.executor_provider)];
    if let Some(ctx) = existing.as_ref() {
        known_mentor.insert(0, (ctx.mentor_model.as_str(), ctx.mentor_provider));
        known_executor.insert(0, (ctx.executor_model.as_str(), ctx.executor_provider));
    }
    let mentor_provider = resolve_provider_for_model(&effective_mentor_model, &known_mentor);
    let executor_provider = resolve_provider_for_model(&effective_executor_model, &known_executor);

    let (existing_mentor_provider, existing_executor_provider) = existing
        .as_ref()
        .map(|ctx| (ctx.mentor_provider, ctx.executor_provider))
        .unwrap_or((pair.mentor_provider, pair.executor_provider));
    let mentor_provider_changed = mentor_provider != existing_mentor_provider;
    let executor_provider_changed = executor_provider != existing_executor_provider;

    println!(
        "[pair_assign_task] Resolved providers: mentor={:?} (model {}), executor={:?} (model {})",
        mentor_provider, effective_mentor_model, executor_provider, effective_executor_model
    );
    if mentor_provider_changed {
        println!(
            "[pair_assign_task] Mentor provider changed from {:?} → {:?}, clearing session",
            existing_mentor_provider, mentor_provider
        );
    }
    if executor_provider_changed {
        println!(
            "[pair_assign_task] Executor provider changed from {:?} → {:?}, clearing session",
            existing_executor_provider, executor_provider
        );
    }

    let existing_run_generation = existing.as_ref().map(|ctx| ctx.run_generation).unwrap_or(0);
    ctx_guard.insert(
        pair_id.to_string(),
        crate::process_spawner::ProcessContext {
            directory: pair
                .worktree_path
                .clone()
                .unwrap_or_else(|| pair.directory.clone()),
            mentor_provider,
            executor_provider,
            mentor_model: effective_mentor_model,
            executor_model: effective_executor_model,
            mentor_session_id: if is_new_run || mentor_provider_changed {
                None
            } else {
                existing
                    .as_ref()
                    .and_then(|ctx| ctx.mentor_session_id.clone())
            },
            executor_session_id: if is_new_run || executor_provider_changed {
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
        },
    );
}

#[tauri::command]
pub async fn pair_assign_task(
    app: tauri::AppHandle,
    pair_manager: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, ProcessSpawner>,
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

    let pair = if is_new_run {
        // Stop the previous run and, when requested, rotate the pair into a
        // fresh worktree before the new run starts. A failed rotation aborts
        // the assignment with the pair left on its old workspace.
        prepare_new_run_workspace(
            &pair_manager,
            &broker,
            &spawner,
            &pair_id,
            pair,
            fresh_worktree,
        )
        .await?
    } else {
        pair
    };

    prepare_run_context(&spawner, &pair, &pair_id, is_new_run);

    let turn_prompt = if is_new_run {
        build_mentor_planning_prompt(&input.spec)
    } else {
        input.spec.clone()
    };

    {
        let broker_guard = broker.lock().unwrap_or_else(|e| e.into_inner());
        // Re-check under the same lock as prepare_run: a pause may have landed
        // since the first check.
        if !is_new_run {
            if let Some(rejection) = broker_guard
                .status_and_turn(&pair_id)
                .and_then(|(status, _)| handoff_rejection(&status))
            {
                println!("[pair_assign_task] {}", rejection);
                return Err(rejection);
            }
        } else {
            broker_guard.begin_new_run(&pair_id, &input.spec);
        }
        broker_guard.prepare_run(&pair_id, &role, spawner.active_processes.clone());
    }

    if is_new_run {
        persist_snapshot_in_background(&app, &pair_id);
    }

    println!("[pair_assign_task] About to trigger {} turn...", role);
    start_turn(&app, &broker, &spawner, &pair_id, &role, turn_prompt).await?;
    println!("[pair_assign_task] {} turn triggered successfully", role);

    // The pair (possibly rotated into a fresh worktree) so the caller can
    // sync workspace fields the frontend holds.
    Ok(pair)
}

#[tauri::command]
pub fn pair_update_models(
    state: tauri::State<std::sync::Mutex<PairManager>>,
    pair_id: String,
    input: crate::types::UpdatePairModelsInput,
) -> Result<crate::types::UpdatePairModelsInput, String> {
    let mut manager = state.lock().unwrap_or_else(|e| e.into_inner());

    let pair = manager
        .pairs
        .get_mut(&pair_id)
        .ok_or_else(|| format!("Pair {} not found", pair_id))?;

    apply_model_update(pair, &input);

    Ok(input)
}

/// Apply a model update, keeping each role's stored provider unless its
/// model actually changed (contract C6).
fn apply_model_update(pair: &mut Pair, input: &crate::types::UpdatePairModelsInput) {
    let old_mentor_provider = pair.mentor_provider;
    let old_executor_provider = pair.executor_provider;

    pair.mentor_provider = resolve_provider_for_model(
        &input.mentor_model,
        &[(pair.mentor_model.as_str(), pair.mentor_provider)],
    );
    pair.executor_provider = resolve_provider_for_model(
        &input.executor_model,
        &[(pair.executor_model.as_str(), pair.executor_provider)],
    );
    pair.mentor_model = input.mentor_model.clone();
    pair.executor_model = input.executor_model.clone();
    pair.pending_mentor_model = input.pending_mentor_model.clone();
    pair.pending_executor_model = input.pending_executor_model.clone();
    pair.mentor_reasoning_effort = input.mentor_reasoning_effort.clone();
    pair.executor_reasoning_effort = input.executor_reasoning_effort.clone();

    println!(
        "[pair_update_models] Updated pair {}: mentor={} (provider {:?}→{:?}), executor={} (provider {:?}→{:?})",
        pair.pair_id,
        pair.mentor_model,
        old_mentor_provider,
        pair.mentor_provider,
        pair.executor_model,
        old_executor_provider,
        pair.executor_provider
    );
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
    spawner: tauri::State<'_, ProcessSpawner>,
    pair_id: String,
) -> Result<(), String> {
    let existing = state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_pair(&pair_id);

    // Invalidate the running turn and take its whole process tree down, then
    // give it a moment to exit so it stops writing into the worktree before
    // the worktree's changes are preserved and it is removed.
    spawner.bump_run_generation(&pair_id);
    for mut child in spawner.stop_pair_processes(&pair_id) {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
    }

    if let Some(worktree_path) = existing
        .as_ref()
        .and_then(|pair| pair.worktree_path.clone())
    {
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
    spawner: tauri::State<'_, ProcessSpawner>,
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
    spawner: &ProcessSpawner,
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

        let resolved = broker_guard.resume_run(pair_id, role_str, spawner.active_processes.clone());

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
    spawner: tauri::State<'_, ProcessSpawner>,
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
    spawner: tauri::State<'_, ProcessSpawner>,
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
    spawner: tauri::State<'_, ProcessSpawner>,
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

    if spawner.stop_process(&pair_id, &role).is_none() {
        return Err(format!(
            "No active {} process found for pair {}",
            role, pair_id
        ));
    }
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
    use crate::message_broker::MessageBroker;
    use crate::process_spawner::ProcessSpawner;
    use crate::provider_registry::ProviderKind;
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
        ProcessSpawner,
    ) {
        let manager = Arc::new(Mutex::new(PairManager::new()));
        let broker = Arc::new(Mutex::new(MessageBroker::new()));
        let spawner = ProcessSpawner::new();
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
                provider: ProviderKind::Opencode,
                model: "test-mentor".to_string(),
                reasoning_effort: None,
            },
            executor: AgentConfig {
                role: AgentRole::Executor,
                provider: ProviderKind::Codex,
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
            mentor_provider: ProviderKind::Opencode,
            mentor_model: "test-mentor".to_string(),
            executor_provider: ProviderKind::Codex,
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
                provider: ProviderKind::Opencode,
                model: "openai/gpt-4o-mini".to_string(),
                reasoning_effort: None,
            },
            executor: AgentConfig {
                role: AgentRole::Executor,
                provider: ProviderKind::Codex,
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
    fn create_pair_keeps_explicit_provider_kinds_on_the_returned_pair() {
        let mut manager = PairManager::new();
        let broker = MessageBroker::new();

        let pair = manager
            .create_pair(&sample_input(), &broker)
            .expect("pair should be created");

        assert_eq!(pair.mentor_provider, ProviderKind::Opencode);
        assert_eq!(pair.mentor_model, "openai/gpt-4o-mini");
        assert_eq!(pair.executor_provider, ProviderKind::Codex);
        assert_eq!(pair.executor_model, "gpt-4o-mini");
    }

    #[test]
    fn provider_infer_kind_maps_claude_and_gemini_models() {
        assert_eq!(
            ProviderAdapter::infer_provider_kind("claude-3-5-sonnet"),
            ProviderKind::Claude
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("gemini-2.5-pro"),
            ProviderKind::Gemini
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("claude/claude-3-5-sonnet"),
            ProviderKind::Claude
        );
        assert_eq!(
            ProviderAdapter::infer_provider_kind("gemini/gemini-2.5-pro"),
            ProviderKind::Gemini
        );
    }

    #[test]
    fn pair_update_models_propagates_provider_inference() {
        let mut manager = PairManager::new();
        let broker = MessageBroker::new();

        let pair = manager.create_pair(&sample_input(), &broker).unwrap();
        let pair_id = pair.pair_id.clone();

        let input = crate::types::UpdatePairModelsInput {
            mentor_model: "claude-3-5-sonnet".to_string(),
            executor_model: "gemini-2.5-pro".to_string(),
            pending_mentor_model: None,
            pending_executor_model: None,
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
        };

        let pair_updated = manager.pairs.get_mut(&pair_id).unwrap();
        pair_updated.mentor_model = input.mentor_model.clone();
        pair_updated.executor_model = input.executor_model.clone();
        pair_updated.pending_mentor_model = input.pending_mentor_model.clone();
        pair_updated.pending_executor_model = input.pending_executor_model.clone();
        pair_updated.mentor_provider =
            ProviderAdapter::infer_provider_kind(&pair_updated.mentor_model);
        pair_updated.executor_provider =
            ProviderAdapter::infer_provider_kind(&pair_updated.executor_model);

        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(
            pair.mentor_provider,
            ProviderKind::Claude,
            "mentor provider should be inferred from claude model"
        );
        assert_eq!(
            pair.executor_provider,
            ProviderKind::Gemini,
            "executor provider should be inferred from gemini model"
        );
    }

    #[test]
    fn pair_update_models_stores_provider_for_cross_provider_switch() {
        let mut manager = PairManager::new();
        let broker = MessageBroker::new();

        let pair = manager.create_pair(&sample_input(), &broker).unwrap();
        let pair_id = pair.pair_id.clone();

        let pair_updated = manager.pairs.get_mut(&pair_id).unwrap();
        pair_updated.mentor_model = "gemini-2.5-pro".to_string();
        pair_updated.executor_model = "claude-3-5-sonnet".to_string();
        pair_updated.mentor_provider =
            ProviderAdapter::infer_provider_kind(&pair_updated.mentor_model);
        pair_updated.executor_provider =
            ProviderAdapter::infer_provider_kind(&pair_updated.executor_model);

        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(
            pair.mentor_provider,
            ProviderKind::Gemini,
            "mentor provider should switch to Gemini"
        );
        assert_eq!(
            pair.executor_provider,
            ProviderKind::Claude,
            "executor provider should switch to Claude"
        );
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
        let (manager, broker, spawner) = make_test_pair_manager();
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
        broker
            .lock()
            .unwrap()
            .prepare_run(pair_id, "executor", spawner.active_processes.clone());
        let state = broker.lock().unwrap().get_state(pair_id).unwrap();
        assert_eq!(state.status, PairStatus::Executing);
        assert_eq!(state.turn, AgentRole::Executor);
        assert_eq!(state.iteration, 1);
    }

    #[test]
    fn plan_gate_rejection_replans_instead_of_reviewing() {
        let (manager, broker, spawner) = make_test_pair_manager();
        let pair_id = "test-plan-reject";
        insert_paused_pair(&manager, &broker, pair_id, AgentRole::Mentor, 1);
        broker
            .lock()
            .unwrap()
            .set_pair_status(pair_id, PairStatus::AwaitingHumanReview, None);

        broker
            .lock()
            .unwrap()
            .prepare_run(pair_id, "mentor", spawner.active_processes.clone());
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
            crate::process_spawner::ProcessContext {
                directory: "/tmp/test".to_string(),
                mentor_provider: ProviderKind::Opencode,
                executor_provider: ProviderKind::Codex,
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: None,
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 7,
                is_smoke_test: false,
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
    fn provider_is_kept_for_an_unchanged_model_and_inferred_for_a_new_one() {
        // A grok alias doesn't infer back to grok; the stored provider wins.
        assert_eq!(
            resolve_provider_for_model("my-model", &[("my-model", ProviderKind::Grok)]),
            ProviderKind::Grok
        );
        assert_eq!(
            resolve_provider_for_model("claude-sonnet-4-5", &[("my-model", ProviderKind::Grok)]),
            ProviderKind::Claude
        );
        // The first matching entry (the live context) wins over the pair.
        assert_eq!(
            resolve_provider_for_model(
                "codex-mini-latest",
                &[
                    ("codex-mini-latest", ProviderKind::Codex),
                    ("codex-mini-latest", ProviderKind::Opencode)
                ]
            ),
            ProviderKind::Codex
        );
    }

    #[test]
    fn apply_model_update_keeps_the_provider_of_unchanged_models() {
        let mut manager = PairManager::new();
        let broker = MessageBroker::new();
        let mut input = sample_input();
        input.mentor.provider = ProviderKind::Grok;
        input.mentor.model = "my-model".to_string();
        let pair = manager.create_pair(&input, &broker).unwrap();
        let pair_id = pair.pair_id.clone();

        let pair = manager.pairs.get_mut(&pair_id).unwrap();
        apply_model_update(
            pair,
            &crate::types::UpdatePairModelsInput {
                mentor_model: "my-model".to_string(),
                executor_model: "claude-3-5-sonnet".to_string(),
                pending_mentor_model: None,
                pending_executor_model: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
            },
        );

        let pair = manager.get_pair(&pair_id).unwrap();
        assert_eq!(pair.mentor_provider, ProviderKind::Grok);
        assert_eq!(pair.executor_provider, ProviderKind::Claude);
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
                provider: ProviderKind::Opencode,
                model: "mentor-model".to_string(),
                reasoning_effort: None,
            },
            executor: AgentConfig {
                role: AgentRole::Executor,
                provider: ProviderKind::Codex,
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
            mentor_provider: ProviderKind::Opencode,
            mentor_model: "mentor-model".to_string(),
            executor_provider: ProviderKind::Codex,
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

        let workspace = prepare_workspace(
            &worktree_input(temp.repo_str()),
            "pair-wt1",
            &[],
        )
        .expect("worktree workspace");

        let worktree_path = std::path::PathBuf::from(
            workspace.worktree_path.clone().expect("worktree path"),
        );
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

        let error = prepare_workspace(
            &worktree_input(plain.to_str().unwrap()),
            "pair-wt5",
            &[],
        )
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
        let old = create_worktree(temp.repo_str(), "main", ".worktrees/pair-old")
            .expect("old worktree");
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
        assert_eq!(pair.worktree_path.as_deref(), Some("/repo/.worktrees/pair-new"));
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
        let spawner = ProcessSpawner::new();
        let mut pair = git_pair("pair-ctx", "/repo", Some("main"));
        pair.worktree_path = Some("/repo/.worktrees/pair-abc".to_string());
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-ctx".to_string(),
            crate::process_spawner::ProcessContext {
                directory: "/repo".to_string(),
                mentor_provider: ProviderKind::Opencode,
                executor_provider: ProviderKind::Codex,
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: Some("mentor-session".to_string()),
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 7,
                is_smoke_test: false,
            },
        );

        // A handoff keeps provider sessions and the run generation, and runs in
        // the pair's worktree.
        prepare_run_context(&spawner, &pair, "pair-ctx", false);
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
        prepare_run_context(&spawner, &pair, "pair-ctx", true);
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
    fn prepare_run_context_clears_sessions_when_the_provider_changed() {
        let spawner = ProcessSpawner::new();
        let mut pair = git_pair("pair-ctx-swap", "/repo", Some("main"));
        pair.mentor_provider = ProviderKind::Claude;
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-ctx-swap".to_string(),
            crate::process_spawner::ProcessContext {
                directory: "/repo".to_string(),
                mentor_provider: ProviderKind::Codex,
                executor_provider: ProviderKind::Codex,
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: Some("mentor-session".to_string()),
                executor_session_id: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 1,
                is_smoke_test: false,
            },
        );

        // Handoff, but the mentor's stored provider differs from the pair's:
        // the resolved provider changed, so the session must not carry over.
        prepare_run_context(&spawner, &pair, "pair-ctx-swap", false);
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx-swap")
            .cloned()
            .unwrap();
        assert_eq!(ctx.mentor_session_id, None);
    }

    #[test]
    fn validate_assign_task_accepts_a_new_run_for_an_existing_pair() {
        let manager = Mutex::new(PairManager::new());
        manager.lock().unwrap().upsert_pair(git_pair("pair-ok", "/tmp/repo", None));
        let broker = assign_broker();

        let (pair, role, is_new_run, fresh_worktree) =
            validate_assign_task(&manager, &broker, "pair-ok", &base_assign("go the extra mile"))
                .expect("a valid new-run assignment");

        assert_eq!(pair.pair_id, "pair-ok");
        assert_eq!(role, "mentor", "a new run starts with the mentor");
        assert!(is_new_run);
        assert!(!fresh_worktree);
    }

    #[test]
    fn prepare_run_context_clears_sessions_when_either_provider_changed() {
        let spawner = ProcessSpawner::new();
        let mut pair = git_pair("pair-ctx-exec", "/repo", Some("main"));
        pair.executor_provider = ProviderKind::Claude;
        spawner.pair_contexts.lock().unwrap().insert(
            "pair-ctx-exec".to_string(),
            crate::process_spawner::ProcessContext {
                directory: "/repo".to_string(),
                mentor_provider: ProviderKind::Opencode,
                executor_provider: ProviderKind::Codex,
                mentor_model: "test-mentor".to_string(),
                executor_model: "test-executor".to_string(),
                mentor_session_id: None,
                executor_session_id: Some("executor-session".to_string()),
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_generation: 1,
                is_smoke_test: false,
            },
        );

        // Handoff, but the executor's stored provider differs: its session is
        // dropped while the mentor's (unchanged) context is left alone.
        prepare_run_context(&spawner, &pair, "pair-ctx-exec", false);
        let ctx = spawner
            .pair_contexts
            .lock()
            .unwrap()
            .get("pair-ctx-exec")
            .cloned()
            .unwrap();
        assert_eq!(ctx.executor_session_id, None);
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
            &ProcessSpawner::new(),
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
        assert!(std::path::PathBuf::from(&new_path).join("README.md").exists());
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
        let old = create_worktree(temp.repo_str(), "main", ".worktrees/pair-old")
            .expect("old worktree");
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
            .initialize_pair(
                "pair-rot-fail",
                worktree_input(temp.repo_str()),
                Some(&old),
            )
            .unwrap();

        let error = prepare_new_run_workspace(
            &manager,
            &broker,
            &ProcessSpawner::new(),
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
        let old = create_worktree(temp.repo_str(), "main", ".worktrees/pair-old")
            .expect("old worktree");

        let mut pair = git_pair("pair-rot-off", &old, Some("main"));
        pair.repo_path = Some(temp.repo_str().to_string());
        pair.worktree_path = Some(old.clone());
        pair.worktree_branch = Some("the-pair/pair-old".to_string());

        let manager = Mutex::new(PairManager::new());
        manager.lock().unwrap().upsert_pair(pair.clone());

        let unchanged = prepare_new_run_workspace(
            &manager,
            &assign_broker(),
            &ProcessSpawner::new(),
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

        let error = prepare_workspace(
            &worktree_input(temp.repo_str()),
            "pair-wt-detached",
            &[],
        )
        .unwrap_err();
        assert!(error.contains("no current branch"));
    }

    #[test]
    fn prepare_workspace_use_worktree_rejects_a_base_branch_another_pair_holds() {
        let temp = TempRepo::new("use-worktree-conflict");
        temp.git(&temp.repo, &["branch", "feature"]);

        let mut input = worktree_input(temp.repo_str());
        input.branch = Some("feature".to_string());

        let error = prepare_workspace(
            &input,
            "pair-wt-conflict",
            &["feature".to_string()],
        )
        .unwrap_err();
        assert!(error.contains("already using branch"));
    }

    #[test]
    fn prepare_workspace_use_worktree_starts_from_a_remote_branch() {
        let temp = TempRepo::new("use-worktree-remote");
        let sha = temp.git(&temp.repo, &["rev-parse", "HEAD"]);
        temp.git(
            &temp.repo,
            &["remote", "add", "origin", "https://example.invalid/repo.git"],
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
}
