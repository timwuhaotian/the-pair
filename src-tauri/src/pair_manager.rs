use crate::acceptance::{
    build_executor_acceptance_followup_prompt, build_mentor_acceptance_prompt,
    build_mentor_acceptance_repair_prompt,
};
use crate::message_broker::MessageBroker;
use crate::process_spawner::ProcessSpawner;
use crate::provider_adapter::ProviderAdapter;
use crate::session_snapshot::delete_pair_snapshot;
use crate::session_snapshot::persist_current_pair_snapshot;
use crate::types::{
    AssignTaskInput, CreatePairInput, Message, MessageSender, MessageType, Pair, PairStatus,
};
use crate::util::{
    build_mentor_planning_prompt, now_millis,
};
use crate::worktree_manager::{
    check_repo_state, create_worktree, delete_worktree, ensure_gitignore_worktrees,
    ensure_local_tracking_branch, BranchInfo, RepoState,
};
use std::collections::HashMap;

pub struct PairManager {
    pairs: HashMap<String, Pair>,
}

impl PairManager {
    pub fn new() -> Self {
        Self {
            pairs: HashMap::new(),
        }
    }

    pub fn create_pair(
        &mut self,
        input: &CreatePairInput,
        broker: &MessageBroker,
    ) -> Result<Pair, String> {
        println!("[PairManager::create_pair] Starting pair creation...");

        let pair_id = uuid::Uuid::new_v4().to_string();
        let created_at = now_millis();

        println!("[PairManager::create_pair] Generated pair_id: {}", pair_id);

        let (directory, branch, repo_path, worktree_path) = if let Some(selected_branch) =
            &input.branch
        {
            let repo_state = check_repo_state(&input.directory);
            if !repo_state.is_git_repo {
                return Err(
                    "Directory is not a git repository. Cannot create worktree.".to_string()
                );
            }

            let is_current_branch =
                repo_state.current_branch.as_deref() == Some(selected_branch.as_str());

            if is_current_branch {
                println!("[PairManager::create_pair] Selected branch '{}' is the current branch — working in-place without worktree", selected_branch);
                (input.directory.clone(), None, None, None)
            } else {
                if repo_state.is_dirty {
                    return Err("Repository has uncommitted changes. Please commit or stash before creating a pair with branch.".to_string());
                }

                let branch_conflict = self.pairs.values().any(|p| {
                    p.branch.as_deref() == Some(selected_branch.as_str())
                        && p.worktree_path.is_some()
                });
                if branch_conflict {
                    return Err(format!(
                        "Another pair is already using branch '{}'. Only one pair can use a branch at a time.",
                        selected_branch
                    ));
                }

                let is_local = repo_state
                    .branches
                    .iter()
                    .any(|b| b.name == selected_branch.as_str() && b.is_local);

                let effective_branch = if !is_local {
                    println!("[PairManager::create_pair] Remote branch detected, creating local tracking branch for {}", selected_branch);
                    ensure_local_tracking_branch(&input.directory, selected_branch)?
                } else {
                    selected_branch.clone()
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

                (
                    worktree_full_path.clone(),
                    Some(effective_branch),
                    Some(input.directory.clone()),
                    Some(worktree_full_path),
                )
            }
        } else {
            (input.directory.clone(), None, None, None)
        };

        let pair = Pair {
            pair_id: pair_id.clone(),
            name: input.name.clone(),
            directory,
            status: PairStatus::Idle,
            mentor_provider: input.mentor.provider,
            mentor_model: input.mentor.model.clone(),
            executor_provider: input.executor.provider,
            executor_model: input.executor.model.clone(),
            pending_mentor_model: None,
            pending_executor_model: None,
            mentor_reasoning_effort: input.mentor_reasoning_effort.clone(),
            executor_reasoning_effort: input.executor_reasoning_effort.clone(),
            created_at,
            branch,
            repo_path,
            worktree_path,
        };

        self.pairs.insert(pair_id.clone(), pair.clone());
        println!("[PairManager::create_pair] Pair inserted into HashMap");

        println!("[PairManager::create_pair] Initializing broker state...");
        let effective_dir = pair.worktree_path.as_deref().or(Some(&pair.directory));
        if let Err(e) = broker.initialize_pair(&pair_id, input.clone(), effective_dir) {
            if let Some(wt_path) = &pair.worktree_path {
                println!(
                    "[PairManager::create_pair] Broker init failed, cleaning up worktree: {}",
                    wt_path
                );
                let _ = delete_worktree(wt_path);
            }
            self.pairs.remove(&pair_id);
            return Err(e);
        }
        println!("[PairManager::create_pair] Broker state initialized successfully");

        Ok(pair)
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

fn stop_pair_processes(spawner: &ProcessSpawner, pair_id: &str, remove_contexts: bool) {
    {
        let mut active_processes = spawner.active_processes.lock().unwrap();

        let mentor_key = format!("{}-mentor", pair_id);
        if let Some(mut child) = active_processes.remove(&mentor_key) {
            println!("[pair_stop] Killing mentor process for {}", pair_id);
            let _ = child.start_kill();
        }

        let executor_key = format!("{}-executor", pair_id);
        if let Some(mut child) = active_processes.remove(&executor_key) {
            println!("[pair_stop] Killing executor process for {}", pair_id);
            let _ = child.start_kill();
        }
    }

    if remove_contexts {
        let mut pair_contexts = spawner.pair_contexts.lock().unwrap();
        pair_contexts.remove(pair_id);
    }
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
    println!("[pair_create] Initial task spec: {}", input.spec);

    let mentor_reasoning_effort = input.mentor_reasoning_effort.clone();
    let executor_reasoning_effort = input.executor_reasoning_effort.clone();
    let mentor_bootstrap_prompt = build_mentor_planning_prompt(&input.spec);

    let pair = {
        let mut manager = state.lock().unwrap();
        let broker_guard = broker.lock().unwrap();

        let pair = manager.create_pair(&input, &broker_guard)?;
        let pair_id = pair.pair_id.clone();

        println!(
            "[pair_create] Successfully created pair: id={}, name={}",
            pair.pair_id, pair.name
        );

        // Set up process context
        {
            let mut ctx_guard = spawner.pair_contexts.lock().unwrap();
            let effective_directory = pair
                .worktree_path
                .clone()
                .unwrap_or_else(|| pair.directory.clone());
            ctx_guard.insert(
                pair_id.clone(),
                crate::process_spawner::ProcessContext {
                    directory: effective_directory,
                    mentor_provider: pair.mentor_provider,
                    executor_provider: pair.executor_provider,
                    mentor_model: pair.mentor_model.clone(),
                    executor_model: pair.executor_model.clone(),
                    mentor_session_id: None,
                    executor_session_id: None,
                    mentor_reasoning_effort,
                    executor_reasoning_effort,
                    run_generation: 0,
                    is_smoke_test: false,
                },
            );
        }

        // Start watching
        broker_guard.prepare_run(&pair_id, "mentor", spawner.active_processes.clone());

        pair
    };

    let pair_id = pair.pair_id.clone();

    // Trigger the initial task
    if !input.spec.trim().is_empty() {
        println!("[pair_create] Starting initial task...");
        spawner
            .trigger_turn(app, pair_id, "mentor".to_string(), mentor_bootstrap_prompt)
            .await?;
        println!("[pair_create] Initial task triggered successfully");
    }

    Ok(pair)
}

#[tauri::command]
pub async fn pair_assign_task(
    app: tauri::AppHandle,
    pair_manager: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, ProcessSpawner>,
    pair_id: String,
    input: AssignTaskInput,
) -> Result<(), String> {
    println!("[pair_assign_task] Called for pair_id: {}", pair_id);
    println!("[pair_assign_task] Task spec: {}", input.spec);

    let pair = {
        let manager = pair_manager.lock().unwrap();
        manager.pairs.get(&pair_id).ok_or("Pair not found")?.clone()
    };

    println!(
        "[pair_assign_task] Found pair: {} at {}",
        pair.name, pair.directory
    );
    println!(
        "[pair_assign_task] Mentor model: {}, Executor model: {}",
        pair.mentor_model, pair.executor_model
    );

    {
        let mut ctx_guard = spawner.pair_contexts.lock().unwrap();
        let existing = ctx_guard.get(&pair_id).map(|ctx| {
            (
                ctx.mentor_provider,
                ctx.executor_provider,
                ctx.mentor_session_id.clone(),
                ctx.executor_session_id.clone(),
                ctx.mentor_reasoning_effort.clone(),
                ctx.executor_reasoning_effort.clone(),
                ctx.run_generation,
            )
        });
        let is_new_run = input.role.is_none();

        if is_new_run {
            // remove_contexts=false: stop_pair_processes only locks active_processes,
            // not pair_contexts, so no deadlock with ctx_guard held.
            stop_pair_processes(&spawner, &pair_id, false);
        }

        let effective_mentor_model = pair
            .pending_mentor_model
            .as_ref()
            .unwrap_or(&pair.mentor_model)
            .clone();
        let effective_executor_model = pair
            .pending_executor_model
            .as_ref()
            .unwrap_or(&pair.executor_model)
            .clone();

        let inferred_mentor_provider =
            ProviderAdapter::infer_provider_kind(&effective_mentor_model);
        let inferred_executor_provider =
            ProviderAdapter::infer_provider_kind(&effective_executor_model);

        let (
            existing_mentor_provider,
            existing_executor_provider,
            existing_mentor_sid,
            existing_executor_sid,
            existing_run_generation,
        ) = existing
            .as_ref()
            .map(|(mp, ep, ms, es, _, _, gen)| {
                (*mp, *ep, ms.clone(), es.clone(), *gen)
            })
            .unwrap_or((
                pair.mentor_provider,
                pair.executor_provider,
                None,
                None,
                0u32,
            ));

        let mentor_provider_changed = inferred_mentor_provider != existing_mentor_provider;
        let executor_provider_changed = inferred_executor_provider != existing_executor_provider;

        println!(
            "[pair_assign_task] Resolved providers: mentor={:?} (from model {}), executor={:?} (from model {})",
            inferred_mentor_provider,
            effective_mentor_model,
            inferred_executor_provider,
            effective_executor_model
        );
        if mentor_provider_changed {
            println!(
                "[pair_assign_task] Mentor provider changed from {:?} → {:?}, clearing session",
                existing_mentor_provider, inferred_mentor_provider
            );
        }
        if executor_provider_changed {
            println!(
                "[pair_assign_task] Executor provider changed from {:?} → {:?}, clearing session",
                existing_executor_provider, inferred_executor_provider
            );
        }

        ctx_guard.insert(
            pair_id.clone(),
            crate::process_spawner::ProcessContext {
                directory: pair
                    .worktree_path
                    .clone()
                    .unwrap_or_else(|| pair.directory.clone()),
                mentor_provider: inferred_mentor_provider,
                executor_provider: inferred_executor_provider,
                mentor_model: effective_mentor_model,
                executor_model: effective_executor_model,
                mentor_session_id: if is_new_run || mentor_provider_changed {
                    None
                } else {
                    existing_mentor_sid
                },
                executor_session_id: if is_new_run || executor_provider_changed {
                    None
                } else {
                    existing_executor_sid
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

    let role = input.role.clone().unwrap_or("mentor".to_string());
    let turn_prompt = if input.role.is_none() {
        build_mentor_planning_prompt(&input.spec)
    } else {
        input.spec
    };

    {
        let broker_guard = broker.lock().unwrap();
        if input.role.is_none() {
            broker_guard.reset_session(&pair_id);
        }
        broker_guard.prepare_run(&pair_id, &role, spawner.active_processes.clone());
    }

    println!("[pair_assign_task] About to trigger {} turn...", role);
    spawner
        .trigger_turn(app, pair_id, role.clone(), turn_prompt)
        .await?;
    println!("[pair_assign_task] {} turn triggered successfully", role);

    Ok(())
}

#[tauri::command]
pub fn pair_update_models(
    state: tauri::State<std::sync::Mutex<PairManager>>,
    pair_id: String,
    input: crate::types::UpdatePairModelsInput,
) -> Result<crate::types::UpdatePairModelsInput, String> {
    let mut manager = state.lock().unwrap();

    let pair = manager
        .pairs
        .get_mut(&pair_id)
        .ok_or_else(|| format!("Pair {} not found", pair_id))?;

    let old_mentor_provider = pair.mentor_provider;
    let old_executor_provider = pair.executor_provider;

    pair.mentor_model = input.mentor_model.clone();
    pair.executor_model = input.executor_model.clone();
    pair.pending_mentor_model = input.pending_mentor_model.clone();
    pair.pending_executor_model = input.pending_executor_model.clone();
    pair.mentor_reasoning_effort = input.mentor_reasoning_effort.clone();
    pair.executor_reasoning_effort = input.executor_reasoning_effort.clone();

    pair.mentor_provider = ProviderAdapter::infer_provider_kind(&pair.mentor_model);
    pair.executor_provider = ProviderAdapter::infer_provider_kind(&pair.executor_model);

    println!(
        "[pair_update_models] Updated pair {}: mentor={} (provider {:?}→{:?}), executor={} (provider {:?}→{:?})",
        pair_id,
        pair.mentor_model,
        old_mentor_provider,
        pair.mentor_provider,
        pair.executor_model,
        old_executor_provider,
        pair.executor_provider
    );

    Ok(input)
}

#[tauri::command]
pub fn pair_list(state: tauri::State<std::sync::Mutex<PairManager>>) -> Result<Vec<Pair>, String> {
    let manager = state.lock().unwrap();
    Ok(manager.list_pairs())
}

#[tauri::command]
pub fn pair_delete(
    app: tauri::AppHandle,
    state: tauri::State<std::sync::Mutex<PairManager>>,
    spawner: tauri::State<'_, ProcessSpawner>,
    pair_id: String,
) -> Result<(), String> {
    stop_pair_processes(&spawner, &pair_id, true);

    let worktree_path: Option<String> = {
        let manager = state.lock().unwrap();
        manager
            .get_pair(&pair_id)
            .and_then(|p| p.worktree_path.clone())
    };

    if let Some(wt_path) = worktree_path {
        println!("[pair_delete] Deleting worktree at {}", wt_path);
        let _ = delete_worktree(&wt_path);
    }

    let mut manager = state.lock().unwrap();
    manager.delete_pair(&pair_id)?;

    let _ = delete_pair_snapshot(&app, &pair_id);

    Ok(())
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
        let manager = state.lock().unwrap();
        if !manager.pairs.contains_key(&pair_id) {
            return Err(format!("Pair {} not found", pair_id));
        }
    }

    stop_pair_processes(&spawner, &pair_id, false);

    {
        let mut manager = state.lock().unwrap();
        let pair = manager
            .pairs
            .get_mut(&pair_id)
            .ok_or_else(|| format!("Pair {} not found", pair_id))?;
        pair.status = PairStatus::Paused;
    }

    {
        let broker = broker.lock().unwrap();
        broker.set_pair_status(
            &pair_id,
            PairStatus::Paused,
            Some("Paused by user".to_string()),
        );
    }

    let _ = persist_current_pair_snapshot(&app, &pair_id);

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
            "### ROLE: EXECUTOR\n\
Continue the previously paused session.\n\
- DO NOT create a new plan.\n\
- DO NOT review your own work.\n\
- Keep going from the restored context.\n\
- You CANNOT declare the task complete. Only the MENTOR can decide when to finish.\n\
- Never output \"TASK_COMPLETE\" - this is reserved for MENTOR only.\n\n\
--- COMMAND TO EXECUTE ---\n{}\n",
            mentor_msg
        )
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
            "### ROLE: MENTOR\n\
Continue the restored review session.\n\
- DO NOT execute files or commands.\n\
- Review the executor's work and decide whether the task is complete.\n\n\
--- REVIEW REQUEST ---\n{}\n",
            executor_msg
        )
    }
}

async fn resume_pair_core(
    manager: &std::sync::Mutex<PairManager>,
    broker: &std::sync::Mutex<MessageBroker>,
    spawner: &ProcessSpawner,
    pair_id: &str,
) -> Result<(String, String), String> {
    let (role_str, prompt) = {
        let manager_guard = manager.lock().unwrap();
        let pair = manager_guard
            .pairs
            .get(pair_id)
            .ok_or_else(|| format!("Pair {} not found", pair_id))?;
        if !matches!(
            pair.status,
            PairStatus::Paused | PairStatus::AwaitingHumanReview
        ) {
            return Err(format!(
                "Pair {} is not paused (status: {:?})",
                pair_id, pair.status
            ));
        }

        let broker_guard = broker.lock().unwrap();
        let (lm, le) = broker_guard.get_last_messages(pair_id);
        let state = broker_guard
            .get_state(pair_id)
            .ok_or_else(|| format!("No broker state found for pair {}", pair_id))?;

        let role_str = match state.turn {
            crate::types::AgentRole::Mentor => "mentor",
            crate::types::AgentRole::Executor => "executor",
        };

        let last_mentor_msg = find_last_message_by_role(&state.messages, MessageSender::Mentor)
            .or(lm.map(|m| m.content));
        let last_executor_msg = find_last_message_by_role(&state.messages, MessageSender::Executor)
            .or(le.map(|m| m.content));
        let task_spec = state
            .messages
            .iter()
            .find(|message| matches!(message.from, MessageSender::Human) && message.to == "mentor")
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let prompt = build_live_resume_prompt(
            role_str,
            &task_spec,
            last_mentor_msg,
            last_executor_msg,
            state.latest_acceptance.clone(),
        );

        (role_str.to_string(), prompt)
    };

    let resumed_status = {
        let broker_guard = broker.lock().unwrap();
        broker_guard.resume_run(pair_id, &role_str, spawner.active_processes.clone())
    };

    {
        let mut manager_guard = manager.lock().unwrap();
        if let Some(pair) = manager_guard.pairs.get_mut(pair_id) {
            pair.status = resumed_status.clone();
        }
    }

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
    let (role_str, prompt) = resume_pair_core(&state, &broker, &spawner, &pair_id).await?;

    // Bump run_generation to invalidate any stale handoffs from pre-pause turns.
    {
        let mut ctx_guard = spawner.pair_contexts.lock().unwrap();
        if let Some(ctx) = ctx_guard.get_mut(&pair_id) {
            ctx.run_generation = ctx.run_generation.wrapping_add(1);
        }
    }

    let _ = persist_current_pair_snapshot(&app, &pair_id);

    spawner.trigger_turn(app, pair_id, role_str, prompt).await?;

    Ok(())
}

#[tauri::command]
pub fn repo_check_state(directory: String) -> Result<RepoState, String> {
    println!("[repo_check_state] Called with directory: {}", directory);
    let result = check_repo_state(&directory);
    println!(
        "[repo_check_state] Result: is_git_repo={}, is_dirty={}, branches_count={}",
        result.is_git_repo,
        result.is_dirty,
        result.branches.len()
    );
    Ok(result)
}

#[tauri::command]
pub fn repo_list_branches(directory: String) -> Result<Vec<BranchInfo>, String> {
    crate::worktree_manager::list_branches(&directory)
}

#[tauri::command]
pub async fn kill_process(
    state: tauri::State<'_, std::sync::Mutex<PairManager>>,
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: tauri::State<'_, ProcessSpawner>,
    pair_id: String,
    role: String,
) -> Result<(), String> {
    {
        let manager = state.lock().unwrap();
        if !manager.pairs.contains_key(&pair_id) {
            return Err(format!("Pair {} not found", pair_id));
        }
    }

    let key = format!("{}-{}", pair_id, role);
    {
        let mut active_processes = spawner.active_processes.lock().unwrap();
        if let Some(mut child) = active_processes.remove(&key) {
            println!(
                "[kill_process] Killing {} process for pair {}",
                role, pair_id
            );
            let _ = child.start_kill();
        } else {
            return Err(format!(
                "No active {} process found for pair {}",
                role, pair_id
            ));
        }
    }

    {
        let broker = broker.lock().unwrap();
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_broker::MessageBroker;
    use crate::process_spawner::ProcessSpawner;
    use crate::provider_registry::ProviderKind;
    use crate::types::{AgentConfig, AgentRole, CreatePairInput, PairStatus};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn make_test_pair_manager() -> (
        Arc<Mutex<PairManager>>,
        Arc<Mutex<MessageBroker>>,
        ProcessSpawner,
    ) {
        let manager = Arc::new(Mutex::new(PairManager::new()));
        let broker = Arc::new(Mutex::new(MessageBroker::new()));
        let spawner = ProcessSpawner {
            active_processes: Arc::new(Mutex::new(HashMap::new())),
            pair_contexts: Arc::new(Mutex::new(HashMap::new())),
        };
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

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id).await;
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

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id).await;
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

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id).await;
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

        let result = resume_pair_core(&manager, &broker, &spawner, pair_id).await;
        assert!(result.is_ok());

        let state = broker.lock().unwrap().get_state(pair_id).unwrap();
        assert_eq!(
            state.iteration, 5,
            "iteration should remain 5, not reset or incremented"
        );
    }

    #[test]
    fn build_mentor_planning_prompt_requires_actionable_steps_and_keeps_task_in_view() {
        let prompt = build_mentor_planning_prompt("Add a dark mode toggle");

        assert!(prompt.contains("ROLE: MENTOR"));
        assert!(prompt.contains("Add a dark mode toggle"));
        assert!(prompt.contains("numbered executable steps"));
        assert!(prompt.contains("DO NOT execute it yourself"));
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
}
