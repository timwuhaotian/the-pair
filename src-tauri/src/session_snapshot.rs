use crate::acceptance::{
    build_executor_acceptance_followup_prompt, build_mentor_acceptance_prompt,
    build_mentor_acceptance_repair_prompt,
};
use crate::harness::driver::{ProcessContext, ProcessContextMap, TurnDriver};
use crate::message_broker::MessageBroker;
use crate::pair_manager::PairManager;
use crate::types::{
    AcceptanceRecord, AgentActivity, AgentRole, GitTracking, Message, MessageSender, MessageType,
    ModifiedFile, Pair, PairState, PairStatus, TurnTokenUsage,
};
use crate::util::{build_mentor_planning_prompt, now_millis};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Manager, State};

// v3 (2.8.2): snapshots are persisted on every status change and turn end, so an
// active status on disk really means "interrupted mid-run". Older files froze at
// their first session-id registration and must not be offered for resume.
const SNAPSHOT_VERSION: u32 = 3;
const SNAPSHOT_DIR_NAME: &str = "pair-snapshots";
pub(crate) const INDEX_FILE_NAME: &str = "index.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotTurnCard {
    pub id: String,
    pub role: AgentRole,
    pub state: String,
    pub content: String,
    pub activity: AgentActivity,
    pub started_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TurnTokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cognitive_events: Option<Vec<crate::types::CognitiveEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRunSummary {
    pub id: String,
    pub spec: String,
    pub status: PairStatus,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub mentor_model: String,
    pub executor_model: String,
    pub iterations: u32,
    /// Absent in snapshots written before run history kept messages.
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_acceptance: Option<AcceptanceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotProcessContext {
    pub mentor_session_id: Option<String>,
    pub executor_session_id: Option<String>,
    /// The harness conversation each agent carries, in OpenAI chat form.
    /// Absent in snapshots written before the direct connection kept one.
    #[serde(default)]
    pub mentor_history: Vec<serde_json::Value>,
    #[serde(default)]
    pub executor_history: Vec<serde_json::Value>,
    /// The model each agent talks to directly. Without these a restored pair
    /// falls back to its provider CLI and never replays the history above.
    #[serde(default)]
    pub mentor_selection: Option<crate::harness::ModelSelection>,
    #[serde(default)]
    pub executor_selection: Option<crate::harness::ModelSelection>,
    /// Absent in snapshots written before v1.3.5.
    #[serde(default)]
    pub run_generation: u32,
    #[serde(default)]
    pub is_smoke_test: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshotRecord {
    pub snapshot_version: u32,
    pub saved_at: u64,
    pub pair_id: String,
    pub name: String,
    pub directory: String,
    pub spec: String,
    pub status: PairStatus,
    pub iterations: u32,
    pub max_iterations: u32,
    pub turn: AgentRole,
    pub mentor_model: String,
    pub executor_model: String,
    pub pending_mentor_model: Option<String>,
    pub pending_executor_model: Option<String>,
    #[serde(rename = "mentorReasoningEffort")]
    pub mentor_reasoning_effort: Option<String>,
    #[serde(rename = "executorReasoningEffort")]
    pub executor_reasoning_effort: Option<String>,
    pub messages: Vec<Message>,
    pub mentor_activity: AgentActivity,
    pub executor_activity: AgentActivity,
    pub modified_files: Vec<ModifiedFile>,
    pub git_tracking: GitTracking,
    pub automation_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_acceptance: Option<AcceptanceRecord>,
    #[serde(default)]
    pub acceptance_history: Vec<AcceptanceRecord>,
    pub current_turn_card: Option<SnapshotTurnCard>,
    pub run_count: u32,
    pub run_history: Vec<SnapshotRunSummary>,
    pub current_run_started_at: u64,
    pub current_run_finished_at: Option<u64>,
    pub created_at: u64,
    pub provider_sessions: SnapshotProcessContext,
    pub branch: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub plan_gate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshotDraft {
    pub pair_id: String,
    pub name: String,
    pub directory: String,
    pub spec: String,
    pub status: PairStatus,
    pub iterations: u32,
    pub max_iterations: u32,
    pub turn: AgentRole,
    pub mentor_model: String,
    pub executor_model: String,
    pub pending_mentor_model: Option<String>,
    pub pending_executor_model: Option<String>,
    #[serde(rename = "mentorReasoningEffort")]
    pub mentor_reasoning_effort: Option<String>,
    #[serde(rename = "executorReasoningEffort")]
    pub executor_reasoning_effort: Option<String>,
    pub messages: Vec<Message>,
    pub mentor_activity: AgentActivity,
    pub executor_activity: AgentActivity,
    pub modified_files: Vec<ModifiedFile>,
    pub git_tracking: GitTracking,
    pub automation_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_acceptance: Option<AcceptanceRecord>,
    #[serde(default)]
    pub acceptance_history: Vec<AcceptanceRecord>,
    pub current_turn_card: Option<SnapshotTurnCard>,
    pub run_count: u32,
    pub run_history: Vec<SnapshotRunSummary>,
    pub current_run_started_at: u64,
    pub current_run_finished_at: Option<u64>,
    pub created_at: u64,
    pub branch: Option<String>,
    pub repo_path: Option<String>,
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub plan_gate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableSessionSummary {
    pub pair_id: String,
    pub name: String,
    pub directory: String,
    pub spec: String,
    pub status: PairStatus,
    pub turn: AgentRole,
    pub mentor_model: String,
    pub executor_model: String,
    pub pending_mentor_model: Option<String>,
    pub pending_executor_model: Option<String>,
    #[serde(rename = "mentorReasoningEffort")]
    pub mentor_reasoning_effort: Option<String>,
    #[serde(rename = "executorReasoningEffort")]
    pub executor_reasoning_effort: Option<String>,
    pub run_count: u32,
    pub current_run_started_at: u64,
    pub current_run_finished_at: Option<u64>,
    pub saved_at: u64,
    pub created_at: u64,
    pub current_turn_card: Option<SnapshotTurnCard>,
    pub has_mentor_session: bool,
    pub has_executor_session: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSessionInput {
    pub pair_id: String,
    pub continue_run: bool,
}

fn to_role_string(role: &AgentRole) -> &'static str {
    match role {
        AgentRole::Mentor => "mentor",
        AgentRole::Executor => "executor",
    }
}

fn build_executor_resume_prompt(snapshot: &SessionSnapshotRecord) -> String {
    if let Some(acceptance) = snapshot.latest_acceptance.as_ref() {
        if let Some(verdict) = acceptance.verdict.as_ref() {
            if matches!(
                verdict.next_step.action,
                crate::types::AcceptanceNextAction::Continue
            ) {
                let previous_executor_result = snapshot
                    .messages
                    .iter()
                    .rev()
                    .find(|message| {
                        matches!(message.from, MessageSender::Executor)
                            && matches!(message.msg_type, MessageType::Result)
                    })
                    .map(|message| message.content.trim().to_string())
                    .unwrap_or_default();

                return build_executor_acceptance_followup_prompt(
                    &snapshot.spec,
                    &previous_executor_result,
                    verdict,
                    acceptance,
                );
            }
        }
    }

    let last_mentor_message = snapshot
        .messages
        .iter()
        .rev()
        .find(|message| {
            matches!(message.from, MessageSender::Mentor)
                && matches!(message.msg_type, MessageType::Plan | MessageType::Result)
        })
        .map(|message| message.content.trim().to_string())
        .unwrap_or_default();

    let mut prompt = String::from(
        "You're picking up a restored pair-programming session as the executor. Carry out the plan below — \
treat it as direct actions to perform right now, not a roadmap to comment on.\n\n\
A few constraints:\n\
- Do the next concrete action the plan calls for; do not restate, summarize, or narrate the plan back.\n\
- If the plan asks for specific output text, reply with exactly that text — no preface, no commentary, no status reports like \"awaiting…\" or \"instruction is set to…\".\n\
- Just carry out the steps; the reviewer will check the work after.\n\
- The workflow decides when to stop, not you — don't add TASK_COMPLETE to your reply.\n\n\
PLAN\n",
    );

    if last_mentor_message.is_empty() {
        prompt.push_str(&snapshot.spec);
    } else {
        prompt.push_str(&last_mentor_message);
    }

    prompt
}

fn build_mentor_resume_prompt(snapshot: &SessionSnapshotRecord) -> String {
    let last_executor_message = snapshot
        .messages
        .iter()
        .rev()
        .find(|message| {
            matches!(message.from, MessageSender::Executor)
                && matches!(message.msg_type, MessageType::Plan | MessageType::Result)
        })
        .map(|message| message.content.trim().to_string())
        .unwrap_or_default();

    match snapshot.status {
        PairStatus::Reviewing => {
            if let Some(acceptance) = snapshot.latest_acceptance.as_ref() {
                if let Some(error) = acceptance.error.as_ref() {
                    if acceptance.repair_attempts > 0 {
                        return build_mentor_acceptance_repair_prompt(error);
                    }
                }

                let executor_result = if last_executor_message.is_empty() {
                    snapshot.spec.clone()
                } else {
                    last_executor_message
                };

                return build_mentor_acceptance_prompt(
                    &snapshot.spec,
                    &executor_result,
                    acceptance,
                );
            }

            let mut prompt = String::from(
                "You're picking up a restored pair-programming session as the reviewer. Read what the executor just did and decide whether the task is done or needs another pass.\n\n\
If you're satisfied the task is complete, include TASK_COMPLETE somewhere in your reply so the orchestrator stops the workflow. Otherwise, describe what should happen next.\n\n\
EXECUTOR OUTPUT\n",
            );
            if last_executor_message.is_empty() {
                prompt.push_str(&snapshot.spec);
            } else {
                prompt.push_str(&last_executor_message);
            }
            prompt
        }
        _ => build_mentor_planning_prompt(&snapshot.spec),
    }
}

fn build_resume_prompt(snapshot: &SessionSnapshotRecord) -> String {
    match snapshot.turn {
        AgentRole::Mentor => build_mentor_resume_prompt(snapshot),
        AgentRole::Executor => build_executor_resume_prompt(snapshot),
    }
}

fn snapshot_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let mut dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    dir.push(SNAPSHOT_DIR_NAME);
    Ok(dir)
}

fn snapshot_index_path_in_dir(snapshot_dir: &Path) -> PathBuf {
    snapshot_dir.join(INDEX_FILE_NAME)
}

fn snapshot_file_path_in_dir(snapshot_dir: &Path, pair_id: &str) -> PathBuf {
    snapshot_dir.join(format!("{}.json", pair_id))
}

/// Validate that a `pair_id` is safe to interpolate into a file path.
/// Accepts UUID-format strings and similar alphanumeric+hyphen identifiers,
/// rejecting path separators, `..`, and unreasonably long values.
fn validate_pair_id(pair_id: &str) -> Result<(), String> {
    if pair_id.is_empty() || pair_id.len() > 128 {
        return Err("Invalid pair_id: length out of range".to_string());
    }
    if pair_id.contains('/') || pair_id.contains('\\') || pair_id.contains("..") {
        return Err("Invalid pair_id: contains path separator or traversal sequence".to_string());
    }
    if !pair_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid pair_id: must be alphanumeric, hyphens, or underscores".to_string());
    }
    Ok(())
}

pub(crate) fn ensure_snapshot_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = snapshot_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create snapshot dir: {}", e))?;
    Ok(dir)
}

/// Serializes every read-modify-write of one pair's snapshot file (the
/// backend writer, frontend saves and deletion), so concurrent writers can't
/// interleave and a delete can't be undone by a persist already in flight.
fn snapshot_lock(pair_id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(pair_id.to_string())
        .or_default()
        .clone()
}

/// Serializes read-modify-write of the shared `index.json`.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

fn write_json_atomic<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("snapshot");
    // A unique temp name per write: two writers never share (and truncate)
    // the same temp file. It doesn't end in `.json`, so scans skip it.
    let tmp_path = path.with_file_name(format!(".{}.{}.tmp", file_name, uuid::Uuid::new_v4()));

    let result = (|| {
        let mut file =
            fs::File::create(&tmp_path).map_err(|e| format!("Failed to write snapshot: {}", e))?;
        file.write_all(&payload)
            .map_err(|e| format!("Failed to write snapshot: {}", e))?;
        // Flush to disk before the rename makes it visible, so a crash can't
        // leave a renamed-but-empty file.
        file.sync_all()
            .map_err(|e| format!("Failed to flush snapshot: {}", e))?;
        fs::rename(&tmp_path, path)
            .map_err(|e| format!("Failed to move snapshot into place: {}", e))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    } else {
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
        }
    }
    result
}

pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    const MAX_SNAPSHOT_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB

    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > MAX_SNAPSHOT_FILE_SIZE {
        return Err(format!(
            "Snapshot file too large ({} bytes, max {})",
            metadata.len(),
            MAX_SNAPSHOT_FILE_SIZE
        ));
    }

    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn save_index_in_dir(
    snapshot_dir: &Path,
    summaries: &[RecoverableSessionSummary],
) -> Result<(), String> {
    let index_path = snapshot_index_path_in_dir(snapshot_dir);
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_json_atomic(&index_path, summaries)
}

fn save_index(app: &AppHandle, summaries: &[RecoverableSessionSummary]) -> Result<(), String> {
    let dir = snapshot_dir(app)?;
    save_index_in_dir(&dir, summaries)
}

fn load_index_from_dir(snapshot_dir: &Path) -> Result<Vec<RecoverableSessionSummary>, String> {
    let index_path = snapshot_index_path_in_dir(snapshot_dir);
    if !index_path.exists() {
        return Ok(Vec::new());
    }

    read_json::<Vec<RecoverableSessionSummary>>(&index_path)
}

fn load_index(app: &AppHandle) -> Result<Vec<RecoverableSessionSummary>, String> {
    let dir = snapshot_dir(app)?;
    load_index_from_dir(&dir)
}

fn scan_snapshot_files_in_dir(
    snapshot_dir: &Path,
) -> Result<Vec<RecoverableSessionSummary>, String> {
    let mut summaries = Vec::new();

    let entries =
        fs::read_dir(snapshot_dir).map_err(|e| format!("Failed to read snapshot dir: {}", e))?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE_NAME) {
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        match read_json::<SessionSnapshotRecord>(&path) {
            Ok(snapshot) => summaries.push(snapshot.to_summary()),
            Err(err) => {
                println!(
                    "[session_snapshot] Skipping unreadable snapshot {:?}: {}",
                    path, err
                );
            }
        }
    }

    summaries.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    Ok(summaries)
}

fn scan_snapshot_files(app: &AppHandle) -> Result<Vec<RecoverableSessionSummary>, String> {
    let dir = ensure_snapshot_dir(app)?;
    scan_snapshot_files_in_dir(&dir)
}

fn load_summaries(app: &AppHandle) -> Result<Vec<RecoverableSessionSummary>, String> {
    let _index_guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    load_summaries_unlocked(app)
}

fn load_summaries_unlocked(app: &AppHandle) -> Result<Vec<RecoverableSessionSummary>, String> {
    match load_index(app) {
        Ok(from_index) if !from_index.is_empty() => return Ok(from_index),
        Ok(_) => {}
        Err(err) => {
            println!(
                "[session_snapshot] Failed to load index, falling back to file scan: {}",
                err
            );
        }
    }

    let scanned = scan_snapshot_files(app)?;
    if !scanned.is_empty() {
        save_index(app, &scanned)?;
    }
    Ok(scanned)
}

fn delete_pair_snapshot_in_dir(snapshot_dir: &Path, pair_id: &str) -> Result<(), String> {
    validate_pair_id(pair_id)?;
    let path = snapshot_file_path_in_dir(snapshot_dir, pair_id);
    if path.exists() {
        let _ = fs::remove_file(&path);
    }

    let _index_guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let index_path = snapshot_index_path_in_dir(snapshot_dir);
    if !index_path.exists() {
        return Ok(());
    }

    let mut summaries = load_index_from_dir(snapshot_dir).unwrap_or_default();
    let before = summaries.len();
    summaries.retain(|entry| entry.pair_id != pair_id);
    if before != summaries.len() {
        save_index_in_dir(snapshot_dir, &summaries)?;
    }

    Ok(())
}

/// Write a pair's snapshot and update the index. Callers hold the pair's
/// `snapshot_lock`.
fn upsert_snapshot_record(app: &AppHandle, snapshot: &SessionSnapshotRecord) -> Result<(), String> {
    validate_pair_id(&snapshot.pair_id)?;
    let path = snapshot_path_for_pair(app, &snapshot.pair_id)?;
    ensure_snapshot_dir(app)?;
    write_json_atomic(&path, snapshot)?;

    let _index_guard = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut summaries = load_summaries_unlocked(app).unwrap_or_default();
    let summary = snapshot.to_summary();
    summaries.retain(|entry| entry.pair_id != summary.pair_id);
    summaries.push(summary);
    summaries.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    save_index(app, &summaries)?;

    Ok(())
}

fn current_turn_activity(activity: &AgentActivity) -> AgentActivity {
    activity.clone()
}

fn idle_activity() -> AgentActivity {
    let now = now_millis();
    AgentActivity {
        phase: crate::types::ActivityPhase::Idle,
        label: "Idle".to_string(),
        detail: None,
        started_at: now,
        updated_at: now,
        last_output_at: None,
        output_line_count: 0,
    }
}

fn last_message_for_role(messages: &[Message], role: MessageSender) -> Option<Message> {
    messages
        .iter()
        .rev()
        .find(|message| {
            message.from == role
                && matches!(message.msg_type, MessageType::Plan | MessageType::Result)
        })
        .cloned()
}

fn last_token_usage_for_role(messages: &[Message], role: MessageSender) -> Option<TurnTokenUsage> {
    messages
        .iter()
        .rev()
        .find(|m| m.from == role && m.token_usage.is_some())
        .and_then(|m| m.token_usage.clone())
}

fn snapshot_turn_card(card: Option<&SnapshotTurnCard>) -> Option<SnapshotTurnCard> {
    card.cloned()
}

impl SessionSnapshotRecord {
    fn to_summary(&self) -> RecoverableSessionSummary {
        RecoverableSessionSummary {
            pair_id: self.pair_id.clone(),
            name: self.name.clone(),
            directory: self.directory.clone(),
            spec: self.spec.clone(),
            status: self.status.clone(),
            turn: self.turn.clone(),
            mentor_model: self.mentor_model.clone(),
            executor_model: self.executor_model.clone(),
            pending_mentor_model: self.pending_mentor_model.clone(),
            pending_executor_model: self.pending_executor_model.clone(),
            mentor_reasoning_effort: self.mentor_reasoning_effort.clone(),
            executor_reasoning_effort: self.executor_reasoning_effort.clone(),
            run_count: self.run_count,
            current_run_started_at: self.current_run_started_at,
            current_run_finished_at: self.current_run_finished_at,
            saved_at: self.saved_at,
            created_at: self.created_at,
            current_turn_card: snapshot_turn_card(self.current_turn_card.as_ref()),
            has_mentor_session: self.provider_sessions.mentor_session_id.is_some(),
            has_executor_session: self.provider_sessions.executor_session_id.is_some(),
        }
    }
}

fn build_process_context(snapshot: &SessionSnapshotRecord) -> ProcessContext {
    // A worktree pair always runs in its worktree. If the worktree is gone the
    // next turn fails with a clear error instead of silently running the agents
    // in the user's main checkout.
    let directory = snapshot
        .worktree_path
        .clone()
        .unwrap_or_else(|| snapshot.directory.clone());
    ProcessContext {
        directory,
        mentor_model: snapshot.mentor_model.clone(),
        executor_model: snapshot.executor_model.clone(),
        mentor_session_id: snapshot.provider_sessions.mentor_session_id.clone(),
        executor_session_id: snapshot.provider_sessions.executor_session_id.clone(),
        mentor_reasoning_effort: snapshot.mentor_reasoning_effort.clone(),
        executor_reasoning_effort: snapshot.executor_reasoning_effort.clone(),
        run_generation: snapshot.provider_sessions.run_generation,
        is_smoke_test: snapshot.provider_sessions.is_smoke_test,
        mentor_history: snapshot.provider_sessions.mentor_history.clone(),
        executor_history: snapshot.provider_sessions.executor_history.clone(),
        mentor_selection: snapshot.provider_sessions.mentor_selection.clone(),
        executor_selection: snapshot.provider_sessions.executor_selection.clone(),
    }
}

fn build_pair(snapshot: &SessionSnapshotRecord) -> Pair {
    Pair {
        pair_id: snapshot.pair_id.clone(),
        name: snapshot.name.clone(),
        directory: snapshot.directory.clone(),
        status: snapshot.status.clone(),
        mentor_model: snapshot.mentor_model.clone(),
        executor_model: snapshot.executor_model.clone(),
        pending_mentor_model: snapshot.pending_mentor_model.clone(),
        pending_executor_model: snapshot.pending_executor_model.clone(),
        mentor_reasoning_effort: snapshot.mentor_reasoning_effort.clone(),
        executor_reasoning_effort: snapshot.executor_reasoning_effort.clone(),
        created_at: snapshot.created_at,
        branch: snapshot.branch.clone(),
        repo_path: snapshot.repo_path.clone(),
        worktree_path: snapshot.worktree_path.clone(),
        // Snapshots from before the field existed didn't record the branch;
        // it is derived from the worktree directory, which names it.
        worktree_branch: snapshot.worktree_branch.clone().or_else(|| {
            snapshot
                .worktree_path
                .as_deref()
                .and_then(crate::worktree_manager::branch_for_worktree)
        }),
        plan_gate: snapshot.plan_gate,
    }
}

fn build_pair_state(snapshot: &SessionSnapshotRecord) -> PairState {
    PairState {
        pair_id: snapshot.pair_id.clone(),
        directory: snapshot.directory.clone(),
        status: snapshot.status.clone(),
        iteration: snapshot.iterations,
        max_iterations: snapshot.max_iterations,
        turn: snapshot.turn.clone(),
        mentor: crate::types::AgentState {
            status: snapshot.status.clone(),
            turn: AgentRole::Mentor,
            last_message: last_message_for_role(&snapshot.messages, MessageSender::Mentor),
            activity: current_turn_activity(&snapshot.mentor_activity),
            token_usage: last_token_usage_for_role(&snapshot.messages, MessageSender::Mentor),
        },
        executor: crate::types::AgentState {
            status: snapshot.status.clone(),
            turn: AgentRole::Executor,
            last_message: last_message_for_role(&snapshot.messages, MessageSender::Executor),
            activity: current_turn_activity(&snapshot.executor_activity),
            token_usage: last_token_usage_for_role(&snapshot.messages, MessageSender::Executor),
        },
        messages: snapshot.messages.clone(),
        mentor_activity: current_turn_activity(&snapshot.mentor_activity),
        executor_activity: current_turn_activity(&snapshot.executor_activity),
        modified_files: snapshot.modified_files.clone(),
        git_tracking: snapshot.git_tracking.clone(),
        automation_mode: snapshot.automation_mode.clone(),
        git_review_available: snapshot.git_tracking.git_review_available.unwrap_or(false),
        finished_at: snapshot.current_run_finished_at,
        latest_acceptance: snapshot.latest_acceptance.clone(),
        acceptance_history: snapshot.acceptance_history.clone(),
        worktree_path: snapshot.worktree_path.clone(),
        turn_started_at: None,
        plan_checklist: Vec::new(),
        key_decisions: Vec::new(),
        cognitive_events: snapshot
            .current_turn_card
            .as_ref()
            .and_then(|tc| tc.cognitive_events.clone())
            .unwrap_or_default(),
        plan_gate: snapshot.plan_gate,
        task_spec: snapshot.spec.clone(),
        run_started_at: Some(snapshot.current_run_started_at),
    }
}

fn build_snapshot_from_state(
    pair: &Pair,
    state: &PairState,
    context: Option<&ProcessContext>,
) -> SessionSnapshotRecord {
    let context = context.cloned().unwrap_or(ProcessContext {
        directory: pair.directory.clone(),
        mentor_model: pair.mentor_model.clone(),
        executor_model: pair.executor_model.clone(),
        mentor_session_id: None,
        executor_session_id: None,
        mentor_reasoning_effort: pair.mentor_reasoning_effort.clone(),
        executor_reasoning_effort: pair.executor_reasoning_effort.clone(),
        run_generation: 0,
        is_smoke_test: false,
        mentor_history: Vec::new(),
        executor_history: Vec::new(),
        mentor_selection: None,
        executor_selection: None,
    });

    let current_turn_card = state
        .messages
        .iter()
        .rev()
        .find(|message| {
            (state.turn == AgentRole::Mentor && matches!(message.from, MessageSender::Mentor))
                || (state.turn == AgentRole::Executor
                    && matches!(message.from, MessageSender::Executor))
        })
        .map(|message| {
            let turn_cognitive_events: Vec<_> = state
                .cognitive_events
                .iter()
                .filter(|e| e.role == state.turn)
                .cloned()
                .collect();
            SnapshotTurnCard {
                id: message.id.clone(),
                role: state.turn.clone(),
                state: "live".to_string(),
                content: message.content.clone(),
                activity: if state.turn == AgentRole::Mentor {
                    state.mentor_activity.clone()
                } else {
                    state.executor_activity.clone()
                },
                started_at: message.timestamp,
                updated_at: message.timestamp,
                token_usage: message.token_usage.clone(),
                cognitive_events: if turn_cognitive_events.is_empty() {
                    None
                } else {
                    Some(turn_cognitive_events)
                },
            }
        });

    SessionSnapshotRecord {
        snapshot_version: SNAPSHOT_VERSION,
        saved_at: now_millis(),
        pair_id: pair.pair_id.clone(),
        name: pair.name.clone(),
        directory: pair.directory.clone(),
        spec: if state.task_spec.trim().is_empty() {
            state
                .messages
                .iter()
                .find(|message| {
                    matches!(message.from, MessageSender::Human) && message.to == "mentor"
                })
                .map(|message| message.content.clone())
                .unwrap_or_default()
        } else {
            state.task_spec.clone()
        },
        status: state.status.clone(),
        iterations: state.iteration,
        max_iterations: state.max_iterations,
        turn: state.turn.clone(),
        mentor_model: pair.mentor_model.clone(),
        executor_model: pair.executor_model.clone(),
        pending_mentor_model: None,
        pending_executor_model: None,
        mentor_reasoning_effort: pair.mentor_reasoning_effort.clone(),
        executor_reasoning_effort: pair.executor_reasoning_effort.clone(),
        messages: state.messages.clone(),
        mentor_activity: state.mentor_activity.clone(),
        executor_activity: state.executor_activity.clone(),
        modified_files: state.modified_files.clone(),
        git_tracking: state.git_tracking.clone(),
        automation_mode: state.automation_mode.clone(),
        latest_acceptance: state.latest_acceptance.clone(),
        acceptance_history: state.acceptance_history.clone(),
        current_turn_card,
        run_count: 1,
        run_history: Vec::new(),
        current_run_started_at: state.run_started_at.unwrap_or_else(now_millis),
        current_run_finished_at: match state.status {
            PairStatus::Finished => Some(state.finished_at.unwrap_or_else(now_millis)),
            PairStatus::Paused | PairStatus::Error => Some(now_millis()),
            _ => None,
        },
        created_at: pair.created_at,
        provider_sessions: SnapshotProcessContext {
            mentor_session_id: context.mentor_session_id.clone(),
            executor_session_id: context.executor_session_id.clone(),
            mentor_history: context.mentor_history.clone(),
            executor_history: context.executor_history.clone(),
            mentor_selection: context.mentor_selection.clone(),
            executor_selection: context.executor_selection.clone(),
            run_generation: context.run_generation,
            is_smoke_test: context.is_smoke_test,
        },
        branch: pair.branch.clone(),
        repo_path: pair.repo_path.clone(),
        worktree_path: pair.worktree_path.clone(),
        worktree_branch: pair.worktree_branch.clone(),
        plan_gate: pair.plan_gate,
    }
}

fn build_snapshot_from_draft(
    draft: SessionSnapshotDraft,
    context: Option<ProcessContext>,
) -> SessionSnapshotRecord {
    let context = context.unwrap_or(ProcessContext {
        directory: draft.directory.clone(),
        mentor_model: draft.mentor_model.clone(),
        executor_model: draft.executor_model.clone(),
        mentor_session_id: None,
        executor_session_id: None,
        mentor_reasoning_effort: draft.mentor_reasoning_effort.clone(),
        executor_reasoning_effort: draft.executor_reasoning_effort.clone(),
        run_generation: 0,
        is_smoke_test: false,
        mentor_history: Vec::new(),
        executor_history: Vec::new(),
        mentor_selection: None,
        executor_selection: None,
    });

    SessionSnapshotRecord {
        snapshot_version: SNAPSHOT_VERSION,
        saved_at: now_millis(),
        pair_id: draft.pair_id,
        name: draft.name,
        directory: draft.directory,
        spec: draft.spec,
        status: draft.status,
        iterations: draft.iterations,
        max_iterations: draft.max_iterations,
        turn: draft.turn,
        mentor_model: draft.mentor_model,
        executor_model: draft.executor_model,
        pending_mentor_model: draft.pending_mentor_model,
        pending_executor_model: draft.pending_executor_model,
        mentor_reasoning_effort: draft.mentor_reasoning_effort,
        executor_reasoning_effort: draft.executor_reasoning_effort,
        messages: draft.messages,
        mentor_activity: draft.mentor_activity,
        executor_activity: draft.executor_activity,
        modified_files: draft.modified_files,
        git_tracking: draft.git_tracking,
        automation_mode: draft.automation_mode,
        latest_acceptance: draft.latest_acceptance,
        acceptance_history: draft.acceptance_history,
        current_turn_card: draft.current_turn_card,
        run_count: draft.run_count,
        run_history: draft.run_history,
        current_run_started_at: draft.current_run_started_at,
        current_run_finished_at: draft.current_run_finished_at,
        created_at: draft.created_at,
        provider_sessions: SnapshotProcessContext {
            mentor_session_id: context.mentor_session_id,
            executor_session_id: context.executor_session_id,
            mentor_history: context.mentor_history,
            executor_history: context.executor_history,
            mentor_selection: context.mentor_selection,
            executor_selection: context.executor_selection,
            run_generation: context.run_generation,
            is_smoke_test: context.is_smoke_test,
        },
        branch: draft.branch,
        repo_path: draft.repo_path,
        worktree_path: draft.worktree_path,
        worktree_branch: draft.worktree_branch,
        plan_gate: draft.plan_gate,
    }
}

fn snapshot_path_for_pair(app: &AppHandle, pair_id: &str) -> Result<PathBuf, String> {
    Ok(snapshot_dir(app)?.join(format!("{}.json", pair_id)))
}

/// Fold the backend's live state into a pair's snapshot. The backend owns
/// status, turn, iteration, verdicts, activity and provider sessions; fields
/// the renderer owns (run history and count, the live turn card, pending
/// models, the plan-gate flag, and messages only the renderer has, such as
/// the human mission card) are kept rather than replaced with empty data.
fn merge_state_into_snapshot(
    snapshot: &mut SessionSnapshotRecord,
    pair: &Pair,
    state: &PairState,
    context: Option<&ProcessContext>,
) {
    let now = now_millis();
    snapshot.snapshot_version = SNAPSHOT_VERSION;
    snapshot.saved_at = now;
    snapshot.name = pair.name.clone();
    snapshot.directory = pair.directory.clone();
    if !state.task_spec.trim().is_empty() {
        snapshot.spec = state.task_spec.clone();
    }
    snapshot.status = state.status.clone();
    snapshot.iterations = state.iteration;
    snapshot.max_iterations = state.max_iterations;
    snapshot.turn = state.turn.clone();
    snapshot.mentor_model = pair.mentor_model.clone();
    snapshot.executor_model = pair.executor_model.clone();
    if pair.pending_mentor_model.is_some() {
        snapshot.pending_mentor_model = pair.pending_mentor_model.clone();
    }
    if pair.pending_executor_model.is_some() {
        snapshot.pending_executor_model = pair.pending_executor_model.clone();
    }
    snapshot.mentor_reasoning_effort = pair.mentor_reasoning_effort.clone();
    snapshot.executor_reasoning_effort = pair.executor_reasoning_effort.clone();

    // Messages: the broker's history plus renderer-only messages of the
    // current run (older ones belong to archived runs).
    let backend_ids: std::collections::HashSet<&str> =
        state.messages.iter().map(|m| m.id.as_str()).collect();
    let run_started_at = state.run_started_at.unwrap_or(0);
    let mut messages: Vec<Message> = snapshot
        .messages
        .iter()
        .filter(|m| !backend_ids.contains(m.id.as_str()) && m.timestamp >= run_started_at)
        .cloned()
        .collect();
    messages.extend(state.messages.iter().cloned());
    messages.sort_by_key(|m| m.timestamp);
    snapshot.messages = messages;

    snapshot.mentor_activity = state.mentor_activity.clone();
    snapshot.executor_activity = state.executor_activity.clone();
    snapshot.modified_files = state.modified_files.clone();
    snapshot.git_tracking = state.git_tracking.clone();
    snapshot.automation_mode = state.automation_mode.clone();
    snapshot.latest_acceptance = state.latest_acceptance.clone();
    snapshot.acceptance_history = state.acceptance_history.clone();

    if let Some(started) = state.run_started_at {
        snapshot.current_run_started_at = started;
    }
    snapshot.current_run_finished_at = match state.status {
        PairStatus::Mentoring | PairStatus::Executing | PairStatus::Reviewing => None,
        PairStatus::Finished => Some(
            state
                .finished_at
                .or(snapshot.current_run_finished_at)
                .unwrap_or(now),
        ),
        PairStatus::Paused | PairStatus::Error => {
            Some(snapshot.current_run_finished_at.unwrap_or(now))
        }
        PairStatus::Idle | PairStatus::AwaitingHumanReview => snapshot.current_run_finished_at,
    };

    if let Some(context) = context {
        snapshot.provider_sessions = SnapshotProcessContext {
            mentor_session_id: context.mentor_session_id.clone(),
            executor_session_id: context.executor_session_id.clone(),
            mentor_history: context.mentor_history.clone(),
            executor_history: context.executor_history.clone(),
            mentor_selection: context.mentor_selection.clone(),
            executor_selection: context.executor_selection.clone(),
            run_generation: context.run_generation,
            is_smoke_test: context.is_smoke_test,
        };
    }
    snapshot.branch = pair.branch.clone();
    snapshot.repo_path = pair.repo_path.clone();
    snapshot.worktree_path = pair.worktree_path.clone();
    snapshot.worktree_branch = pair.worktree_branch.clone();
}

/// Persist the pair's current backend state into its snapshot (merging, see
/// `merge_state_into_snapshot`). A pair that no longer exists is skipped, so
/// a persist racing a delete can never resurrect the snapshot.
pub fn persist_current_pair_snapshot(app: &AppHandle, pair_id: &str) -> Result<(), String> {
    validate_pair_id(pair_id)?;
    let lock = snapshot_lock(pair_id);
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());

    // Read everything under the pair's snapshot lock (one manager/broker/
    // context lock at a time, never nested), so the newest state always wins.
    let pair = {
        let pair_manager = app.state::<std::sync::Mutex<PairManager>>();
        let manager = pair_manager.lock().unwrap_or_else(|e| e.into_inner());
        manager.get_pair(pair_id)
    };
    let Some(pair) = pair else {
        return Ok(());
    };
    let state = {
        let broker = app.state::<std::sync::Mutex<MessageBroker>>();
        let broker_guard = broker.lock().unwrap_or_else(|e| e.into_inner());
        broker_guard.get_state(pair_id)
    };
    let Some(state) = state else {
        return Ok(());
    };
    let context = {
        let spawner = app.state::<TurnDriver>();
        let contexts = spawner
            .pair_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        contexts.get(pair_id).cloned()
    };

    let path = snapshot_path_for_pair(app, pair_id)?;
    let mut snapshot = match read_json::<SessionSnapshotRecord>(&path) {
        Ok(existing) => existing,
        Err(error) => {
            if path.exists() {
                println!(
                    "[session_snapshot] Existing snapshot {:?} is unreadable ({}); rebuilding it from live state",
                    path, error
                );
            }
            build_snapshot_from_state(&pair, &state, context.as_ref())
        }
    };
    merge_state_into_snapshot(&mut snapshot, &pair, &state, context.as_ref());

    upsert_snapshot_record(app, &snapshot)
}

/// Renderer snapshot saves fire on every turn-card/message update and each one
/// fsyncs the snapshot and index, so the work runs off the UI thread (sync
/// commands run on the main thread in Tauri 2).
#[tauri::command]
pub async fn session_save_snapshot(
    app: AppHandle,
    input: SessionSnapshotDraft,
) -> Result<SessionSnapshotRecord, String> {
    tauri::async_runtime::spawn_blocking(move || save_snapshot_blocking(&app, input))
        .await
        .map_err(|e| format!("Snapshot save task failed: {}", e))?
}

fn save_snapshot_blocking(
    app: &AppHandle,
    input: SessionSnapshotDraft,
) -> Result<SessionSnapshotRecord, String> {
    validate_pair_id(&input.pair_id)?;
    let lock = snapshot_lock(&input.pair_id);
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());

    // A save for a pair that was deleted (or never existed) must not write a
    // snapshot that brings it back on the next launch.
    let exists = {
        let pair_manager = app.state::<std::sync::Mutex<PairManager>>();
        let manager = pair_manager.lock().unwrap_or_else(|e| e.into_inner());
        manager.get_pair(&input.pair_id).is_some()
    };
    if !exists {
        return Err(format!("Pair {} not found", input.pair_id));
    }

    let context = {
        let spawner = app.state::<TurnDriver>();
        let contexts = spawner
            .pair_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        contexts.get(&input.pair_id).cloned()
    };
    // The renderer doesn't send the acceptance history; keep the backend's.
    let acceptance_history = if input.acceptance_history.is_empty() {
        let broker = app.state::<std::sync::Mutex<MessageBroker>>();
        let broker_guard = broker.lock().unwrap_or_else(|e| e.into_inner());
        broker_guard
            .get_state(&input.pair_id)
            .map(|state| state.acceptance_history)
            .unwrap_or_default()
    } else {
        input.acceptance_history.clone()
    };

    let mut snapshot = build_snapshot_from_draft(input, context);
    snapshot.acceptance_history = acceptance_history;
    upsert_snapshot_record(app, &snapshot)?;

    Ok(snapshot)
}

pub fn delete_pair_snapshot(app: &AppHandle, pair_id: &str) -> Result<(), String> {
    validate_pair_id(pair_id)?;
    let lock = snapshot_lock(pair_id);
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    let dir = snapshot_dir(app)?;
    delete_pair_snapshot_in_dir(&dir, pair_id)
}

#[tauri::command]
pub fn delete_recoverable_session(app: AppHandle, pair_id: String) -> Result<(), String> {
    validate_pair_id(&pair_id)?;
    delete_pair_snapshot(&app, &pair_id)
}

#[tauri::command]
pub fn list_recoverable_sessions(app: AppHandle) -> Result<Vec<RecoverableSessionSummary>, String> {
    let mut sessions = load_summaries(&app)?;
    sessions.retain(|session| session.status != PairStatus::Finished);
    sessions.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    Ok(sessions)
}

pub fn read_snapshot(app: &AppHandle, pair_id: &str) -> Result<SessionSnapshotRecord, String> {
    validate_pair_id(pair_id)?;
    let path = snapshot_path_for_pair(app, pair_id)?;
    read_json(&path)
}

/// Apply the on-load transformations for a persisted snapshot: reset live
/// activity, drop any in-flight turn card, and turn a run that was
/// interrupted mid-turn (the app quit or crashed) into a Paused pair, which
/// Resume continues from its restored provider sessions. Messages and run
/// history are intentionally preserved so reopening a pair shows the
/// previous conversation; `assignTask` is what archives them into
/// `run_history` for a new run.
fn prepare_loaded_snapshot(snapshot: SessionSnapshotRecord) -> (PairState, SessionSnapshotRecord) {
    const INTERRUPTED_DETAIL: &str = "Interrupted when the app closed. Resume to continue.";

    let was_running = snapshot.status.is_active();
    // Only a snapshot this version wrote (kept current through the run) with a
    // task to resume is offered as a paused run. Pre-v3 files froze at the first
    // session-id registration — resuming one would replay a long-dead turn with
    // an empty task — so those still come back Idle.
    let resumable =
        was_running && snapshot.snapshot_version >= 3 && !snapshot.spec.trim().is_empty();
    let mut snapshot = snapshot;
    if was_running && !resumable {
        snapshot.status = PairStatus::Idle;
    }
    let mut state = build_pair_state(&snapshot);
    let mut idle = idle_activity();
    if resumable {
        idle.label = "Paused".to_string();
        idle.detail = Some(INTERRUPTED_DETAIL.to_string());
    }
    state.mentor_activity = idle.clone();
    state.executor_activity = idle.clone();
    state.mentor.activity = idle.clone();
    state.executor.activity = idle.clone();

    if resumable {
        state.status = PairStatus::Paused;
        state.mentor.status = PairStatus::Paused;
        state.executor.status = PairStatus::Paused;
    }

    let mut snapshot_with_idle = snapshot;
    snapshot_with_idle.mentor_activity = idle.clone();
    snapshot_with_idle.executor_activity = idle;
    if resumable {
        snapshot_with_idle.status = PairStatus::Paused;
    }
    if was_running
        || matches!(
            snapshot_with_idle.status,
            PairStatus::Idle | PairStatus::Finished
        )
    {
        // The live turn card represents in-progress work; drop it when the pair
        // is no longer actively running so we don't render a stale "running"
        // placeholder on top of preserved history.
        snapshot_with_idle.current_turn_card = None;
    }

    (state, snapshot_with_idle)
}

/// The pair state a blocked resume is restored into: paused, with the reason
/// on it, so the pair never looks busy with no turn behind it.
fn paused_for_unresolvable_resume(mut state: PairState, role: &str) -> PairState {
    let mut idle = idle_activity();
    idle.label = "Paused".to_string();
    idle.detail = Some(format!(
        "Could not resume the {} turn: it has no model it can talk to. Pick one in the pair's settings.",
        role
    ));
    state.status = PairStatus::Paused;
    state.mentor.status = PairStatus::Paused;
    state.executor.status = PairStatus::Paused;
    state.mentor_activity = idle.clone();
    state.executor_activity = idle.clone();
    state.mentor.activity = idle.clone();
    state.executor.activity = idle;
    state
}

/// What a restore does about the run its snapshot was interrupted in.
#[derive(Debug)]
enum Resume {
    /// Nothing to resume: the human did not ask, or the pair was already
    /// stopped (a plan waiting for review, an error, a finished run).
    Skip,
    /// Resume this role's turn, on the endpoint its model resolves onto right
    /// now. `None` only in mock mode, which talks to no endpoint at all.
    Start(String, Option<crate::harness::ModelSelection>),
    /// A resume was asked for, but the role has no model it can talk to — the
    /// pair comes back paused instead of restored into a running status with no
    /// turn behind it.
    Blocked(String),
}

/// The endpoint the role's model resolves onto right now.
///
/// The snapshot's own stored selection is kept while it still names the role's
/// model on an endpoint with a ready account — the conversation the snapshot
/// carries was had with it. It is not enough on its own, though: it can outlive
/// the credential it pointed at, the pair's model may have changed since the
/// snapshot was written, and a snapshot from before the harness has none.
fn resume_selection(
    app_dir: &Path,
    snapshot: &SessionSnapshotRecord,
    role: &str,
) -> Option<crate::harness::ModelSelection> {
    let (model, stored) = if role == "mentor" {
        (
            snapshot
                .pending_mentor_model
                .clone()
                .unwrap_or_else(|| snapshot.mentor_model.clone()),
            snapshot.provider_sessions.mentor_selection.as_ref(),
        )
    } else {
        (
            snapshot
                .pending_executor_model
                .clone()
                .unwrap_or_else(|| snapshot.executor_model.clone()),
            snapshot.provider_sessions.executor_selection.as_ref(),
        )
    };
    crate::harness::keep_or_resolve(app_dir, role, &model, stored)
}

/// Put the selection a resume was planned with onto the context its turn will
/// read, so the turn talks to the endpoint the plan checked.
///
/// `plan_resume` decides whether the turn can start from what the model
/// resolves onto today; starting it from the snapshot's stored selection
/// instead would fail a snapshot that has none, or talk to an endpoint whose
/// credential is gone. A role that moved starts its conversation over, the way
/// it does on a handoff: the stored one was had with another endpoint.
fn install_resume_selection(
    context: &mut ProcessContext,
    role: &str,
    selection: crate::harness::ModelSelection,
) {
    let (slot, history, session_id) = if role == "mentor" {
        (
            &mut context.mentor_selection,
            &mut context.mentor_history,
            &mut context.mentor_session_id,
        )
    } else {
        (
            &mut context.executor_selection,
            &mut context.executor_history,
            &mut context.executor_session_id,
        )
    };
    if slot.as_ref().is_some_and(|stored| *stored != selection) {
        history.clear();
        *session_id = None;
    }
    *slot = Some(selection);
}

/// Decide what the interrupted run does, before anything is restored.
fn plan_resume(
    app_dir: Option<&Path>,
    continue_run: bool,
    snapshot: &SessionSnapshotRecord,
) -> Resume {
    let resumable = continue_run
        && !matches!(
            snapshot.status,
            PairStatus::AwaitingHumanReview | PairStatus::Error | PairStatus::Finished
        );
    if !resumable {
        return Resume::Skip;
    }

    let role = to_role_string(&snapshot.turn).to_string();
    // A mock run replays canned output and talks to no endpoint at all, so it
    // never needs a resolvable model.
    if crate::util::is_mock_mode() {
        return Resume::Start(role, None);
    }
    match app_dir.and_then(|dir| resume_selection(dir, snapshot, &role)) {
        Some(selection) => Resume::Start(role, Some(selection)),
        None => Resume::Blocked(role),
    }
}

/// Whether this process already runs `pair_id`: it has both broker state
/// and a turn context, so whatever is on disk for it is older than what is live.
fn pair_is_live(broker: &MessageBroker, contexts: &ProcessContextMap, pair_id: &str) -> bool {
    let has_context = contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(pair_id);
    has_context && broker.get_state(pair_id).is_some()
}

/// Install a restored pair's turn context, one generation past the snapshot's.
///
/// Restore must supersede any turn still live in this process. The snapshot
/// carries an old generation: inserting it as-is would leave a live turn that
/// happens to share that generation looking current — stopped only by its cancel
/// flag, and only once it next checked. Starting one past it makes that turn
/// stale the moment the context is in place. The turn itself is cancelled by the
/// caller.
fn install_restored_context(spawner: &TurnDriver, pair_id: &str, context: &mut ProcessContext) {
    context.run_generation = context.run_generation.wrapping_add(1);
    spawner
        .pair_contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(pair_id.to_string(), context.clone());
}

/// Register one loaded snapshot's pair, broker state and turn context.
///
/// A pair this process already runs keeps its live state: its turn context and
/// broker state are newer than anything on disk, and adopting the snapshot would
/// rewind the context to an older run generation — the live turn would cancel
/// itself and leave the pair Mentoring/Executing/Reviewing with no turn behind
/// it. `load_all_pairs` runs at startup and again from the frontend's handoff
/// path, mid-session. The record is returned either way: the frontend builds
/// its store from it.
fn register_loaded_snapshot(
    pair_manager: &std::sync::Mutex<PairManager>,
    broker: &std::sync::Mutex<MessageBroker>,
    contexts: &ProcessContextMap,
    snapshot: SessionSnapshotRecord,
) -> Result<SessionSnapshotRecord, String> {
    let pair = build_pair(&snapshot);
    let (state, snapshot_with_idle) = prepare_loaded_snapshot(snapshot);
    let context = build_process_context(&snapshot_with_idle);

    let live = {
        let broker_guard = broker.lock().map_err(|e| e.to_string())?;
        pair_is_live(&broker_guard, contexts, &pair.pair_id)
    };
    if live {
        println!(
            "[session_snapshot] Pair {} is live in this session; keeping its state",
            pair.pair_id
        );
        return Ok(snapshot_with_idle);
    }

    {
        pair_manager
            .lock()
            .map_err(|e| e.to_string())?
            .upsert_pair(pair.clone());
    }
    {
        broker
            .lock()
            .map_err(|e| e.to_string())?
            .restore_state(state)?;
    }
    {
        contexts
            .lock()
            .map_err(|e| e.to_string())?
            .insert(pair.pair_id, context);
    }

    Ok(snapshot_with_idle)
}

/// Load all persisted pairs into the backend state on startup.
/// Returns the full snapshot records so the frontend can populate its store.
#[tauri::command]
pub fn load_all_pairs(
    app: AppHandle,
    pair_manager: State<'_, std::sync::Mutex<PairManager>>,
    broker: State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: State<'_, TurnDriver>,
) -> Result<Vec<SessionSnapshotRecord>, String> {
    let dir = ensure_snapshot_dir(&app)?;
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(vec![]),
    };

    let mut snapshots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some(INDEX_FILE_NAME) {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let snapshot = match read_json::<SessionSnapshotRecord>(&path) {
            Ok(s) => s,
            Err(error) => {
                // Don't drop a pair silently: say which file failed and why.
                eprintln!(
                    "[session_snapshot] Skipping snapshot {:?} that failed to load: {}",
                    path, error
                );
                continue;
            }
        };
        if validate_pair_id(&snapshot.pair_id).is_err() {
            eprintln!(
                "[session_snapshot] Skipping snapshot {:?} with an invalid pair id",
                path
            );
            continue;
        }

        let snapshot_with_idle =
            register_loaded_snapshot(&pair_manager, &broker, &spawner.pair_contexts, snapshot)?;
        snapshots.push(snapshot_with_idle);
    }

    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(snapshots)
}

#[tauri::command]
pub async fn restore_session(
    app: AppHandle,
    pair_manager: State<'_, std::sync::Mutex<PairManager>>,
    broker: State<'_, std::sync::Mutex<MessageBroker>>,
    spawner: State<'_, TurnDriver>,
    input: RestoreSessionInput,
) -> Result<SessionSnapshotRecord, String> {
    validate_pair_id(&input.pair_id)?;
    let snapshot = read_snapshot(&app, &input.pair_id)?;
    let pair = build_pair(&snapshot);
    let mut state = build_pair_state(&snapshot);
    let mut context = build_process_context(&snapshot);

    // Reset activity to Idle if not continuing run
    if !input.continue_run {
        let idle = idle_activity();
        state.mentor_activity = idle.clone();
        state.executor_activity = idle.clone();
        state.mentor.activity = idle.clone();
        state.executor.activity = idle;

        // Reset running status to Idle
        if matches!(
            state.status,
            PairStatus::Mentoring | PairStatus::Executing | PairStatus::Reviewing
        ) {
            state.status = PairStatus::Idle;
            state.mentor.status = PairStatus::Idle;
            state.executor.status = PairStatus::Idle;
        }
    }

    // What the interrupted run does next, decided before the pair's status is
    // restored: a turn that cannot start must not come back as a pair in
    // Reviewing/Executing with nothing running behind it.
    let resume = plan_resume(
        app.path().app_data_dir().ok().as_deref(),
        input.continue_run,
        &snapshot,
    );
    match &resume {
        Resume::Blocked(role) => state = paused_for_unresolvable_resume(state, role),
        Resume::Start(role, Some(selection)) => {
            install_resume_selection(&mut context, role, selection.clone())
        }
        Resume::Start(_, None) | Resume::Skip => {}
    }

    {
        let mut manager = pair_manager.lock().map_err(|e| e.to_string())?;
        manager.upsert_pair(pair.clone());
    }

    {
        let broker_guard = broker.lock().map_err(|e| e.to_string())?;
        broker_guard.restore_state(state.clone())?;
    }
    // Restore must supersede any turn still live in this process: the restored
    // context starts one generation past the snapshot's, so a live turn sharing
    // that generation is stale immediately (and is cancelled below).
    install_restored_context(&spawner, &pair.pair_id, &mut context);
    spawner.stop_pair_processes(&pair.pair_id);

    let mut updated_snapshot = snapshot.clone();
    if matches!(resume, Resume::Blocked(_)) {
        // The pair is paused, not mid-run: persist that so a restart does not
        // offer a resume that cannot start.
        updated_snapshot.status = PairStatus::Paused;
    }
    updated_snapshot.provider_sessions = SnapshotProcessContext {
        mentor_session_id: updated_snapshot.provider_sessions.mentor_session_id.clone(),
        executor_session_id: updated_snapshot
            .provider_sessions
            .executor_session_id
            .clone(),
        mentor_history: updated_snapshot.provider_sessions.mentor_history.clone(),
        executor_history: updated_snapshot.provider_sessions.executor_history.clone(),
        mentor_selection: updated_snapshot.provider_sessions.mentor_selection.clone(),
        executor_selection: updated_snapshot
            .provider_sessions
            .executor_selection
            .clone(),
        run_generation: updated_snapshot.provider_sessions.run_generation,
        is_smoke_test: updated_snapshot.provider_sessions.is_smoke_test,
    };
    {
        let lock = snapshot_lock(&updated_snapshot.pair_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        upsert_snapshot_record(&app, &updated_snapshot)?;
    }

    let should_resume = input.continue_run
        && !matches!(
            snapshot.status,
            PairStatus::AwaitingHumanReview | PairStatus::Error | PairStatus::Finished
        );

    if let Resume::Start(role, _) = resume {
        let prompt = build_resume_prompt(&snapshot);
        {
            let broker_guard = broker.lock().map_err(|e| e.to_string())?;
            // Use resume_run to preserve iteration count; prepare_run incorrectly
            // increments for non-planning mentor turns.
            broker_guard.resume_run(&pair.pair_id, &role);
        }
        // `resume_run` has just put the pair back into a running status, so a
        // turn that cannot start must take it out again: `start_turn` marks the
        // pair errored instead of leaving it Mentoring/Executing with nothing
        // behind it.
        crate::pair_manager::start_turn(&app, &broker, &spawner, &pair.pair_id, &role, prompt)
            .await?;
    } else if should_resume {
        println!(
            "[restore_session] Not resuming pair {}: its next turn has no model it can talk to",
            pair.pair_id
        );
    }

    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn activity(label: &str) -> AgentActivity {
        AgentActivity {
            phase: crate::types::ActivityPhase::Idle,
            label: label.to_string(),
            detail: None,
            started_at: 0,
            updated_at: 0,
            last_output_at: None,
            output_line_count: 0,
        }
    }

    fn message(
        id: &str,
        from: MessageSender,
        msg_type: MessageType,
        content: &str,
        iteration: u32,
    ) -> Message {
        Message {
            id: id.to_string(),
            timestamp: 0,
            from,
            to: "human".to_string(),
            msg_type,
            content: content.to_string(),
            iteration,
            token_usage: None,
            attachments: None,
            cognitive_events: None,
            started_at: None,
            finalized_at: None,
        }
    }

    fn snapshot(
        turn: AgentRole,
        status: PairStatus,
        messages: Vec<Message>,
    ) -> SessionSnapshotRecord {
        SessionSnapshotRecord {
            snapshot_version: SNAPSHOT_VERSION,
            saved_at: 10,
            pair_id: "pair-1".to_string(),
            name: "Demo".to_string(),
            directory: "/tmp/project".to_string(),
            spec: "Fallback task spec".to_string(),
            status,
            iterations: 2,
            max_iterations: 3,
            turn: turn.clone(),
            mentor_model: "mentor-model".to_string(),
            executor_model: "executor-model".to_string(),
            pending_mentor_model: None,
            pending_executor_model: None,
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            messages,
            mentor_activity: activity("Mentor reviewing"),
            executor_activity: activity("Executor working"),
            modified_files: vec![],
            git_tracking: GitTracking {
                available: true,
                root_path: Some("/tmp/project".to_string()),
                baseline: Some("abc123".to_string()),
                git_review_available: Some(true),
            },
            automation_mode: "full-auto".to_string(),
            latest_acceptance: None,
            acceptance_history: Vec::new(),
            current_turn_card: Some(SnapshotTurnCard {
                id: "turn-1".to_string(),
                role: turn.clone(),
                state: "live".to_string(),
                content: "Turn content".to_string(),
                activity: activity("Turn activity"),
                started_at: 0,
                updated_at: 0,
                token_usage: None,
                cognitive_events: None,
            }),
            run_count: 1,
            run_history: vec![],
            current_run_started_at: 11,
            current_run_finished_at: None,
            created_at: 9,
            provider_sessions: SnapshotProcessContext {
                mentor_session_id: Some("ses_mentor".to_string()),
                executor_session_id: None,
                mentor_history: Vec::new(),
                executor_history: Vec::new(),
                mentor_selection: None,
                executor_selection: None,
                run_generation: 0,
                is_smoke_test: false,
            },
            branch: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            plan_gate: false,
        }
    }

    fn legacy_snapshot(turn: AgentRole, status: PairStatus) -> SessionSnapshotRecord {
        let mut snapshot = snapshot(turn, status, vec![]);
        snapshot.mentor_model = "claude-3-5-sonnet".to_string();
        snapshot.executor_model = "gpt-4o-mini".to_string();
        snapshot
    }

    fn draft() -> SessionSnapshotDraft {
        SessionSnapshotDraft {
            pair_id: "pair-1".to_string(),
            name: "Demo".to_string(),
            directory: "/tmp/project".to_string(),
            spec: "Fallback task spec".to_string(),
            status: PairStatus::Idle,
            iterations: 0,
            max_iterations: 3,
            turn: AgentRole::Mentor,
            mentor_model: "claude-3-5-sonnet".to_string(),
            executor_model: "gpt-4o-mini".to_string(),
            pending_mentor_model: None,
            pending_executor_model: None,
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            messages: vec![],
            mentor_activity: activity("Mentor idle"),
            executor_activity: activity("Executor idle"),
            modified_files: vec![],
            git_tracking: GitTracking {
                available: false,
                root_path: None,
                baseline: None,
                git_review_available: Some(false),
            },
            automation_mode: "full-auto".to_string(),
            latest_acceptance: None,
            acceptance_history: Vec::new(),
            current_turn_card: None,
            run_count: 1,
            run_history: vec![],
            current_run_started_at: 0,
            current_run_finished_at: None,
            created_at: 0,
            branch: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
            plan_gate: false,
        }
    }

    #[test]
    fn build_resume_prompt_uses_last_mentor_plan_for_executor_turns() {
        let snapshot = snapshot(
            AgentRole::Executor,
            PairStatus::Executing,
            vec![
                message(
                    "msg-1",
                    MessageSender::Mentor,
                    MessageType::Plan,
                    "Implement the parser",
                    1,
                ),
                message(
                    "msg-2",
                    MessageSender::Executor,
                    MessageType::Progress,
                    "Working through the steps",
                    1,
                ),
            ],
        );

        let prompt = build_resume_prompt(&snapshot);
        let lower = prompt.to_lowercase();
        assert!(lower.contains("direct actions"));
        assert!(lower.contains("do not restate, summarize, or narrate"));
        assert!(lower.contains("reply with exactly that text"));
        assert!(prompt.contains("PLAN\n"));
        assert!(prompt.contains("Implement the parser"));
        assert!(!prompt.contains("Fallback task spec"));
    }

    #[test]
    fn build_resume_prompt_uses_last_executor_result_for_reviewing_mentor_turns() {
        let snapshot = snapshot(
            AgentRole::Mentor,
            PairStatus::Reviewing,
            vec![
                message(
                    "msg-1",
                    MessageSender::Executor,
                    MessageType::Result,
                    "The feature is ready",
                    1,
                ),
                message(
                    "msg-2",
                    MessageSender::Mentor,
                    MessageType::Progress,
                    "Reviewing now",
                    1,
                ),
            ],
        );

        let prompt = build_resume_prompt(&snapshot);
        assert!(prompt.contains("EXECUTOR OUTPUT"));
        assert!(prompt.contains("TASK_COMPLETE"));
        assert!(prompt.contains("The feature is ready"));
    }

    #[test]
    fn build_snapshot_from_draft_persists_provider_kinds() {
        let record = build_snapshot_from_draft(draft(), None);

        assert_eq!(record.mentor_model, "claude-3-5-sonnet");
        assert_eq!(record.executor_model, "gpt-4o-mini");
    }

    #[test]
    fn plan_resume_starts_the_turn_when_the_model_still_resolves() {
        let mut record = snapshot(AgentRole::Mentor, PairStatus::Executing, vec![]);
        record.mentor_model = "test-mentor".to_string();
        let dir = app_dir_listing("test-mentor");

        match plan_resume(Some(&dir), true, &record) {
            Resume::Start(role, selection) => {
                assert_eq!(role, "mentor");
                assert_eq!(
                    selection.map(|selection| selection.provider_id),
                    Some("listed-1".to_string()),
                    "the plan carries the endpoint the turn will talk to"
                );
            }
            other => panic!("expected a resume, got {:?}", other),
        }
    }

    #[test]
    fn a_resume_starts_on_the_selection_it_was_planned_with() {
        // A snapshot from before the harness stores no selection at all. The
        // plan resolves the model today, and that — not the snapshot's empty
        // slot — is what the turn must read, or it fails with "no model
        // selected" after the pair was already put back into a running status.
        let mut record = snapshot(AgentRole::Mentor, PairStatus::Executing, vec![]);
        record.mentor_model = "test-mentor".to_string();
        assert!(record.provider_sessions.mentor_selection.is_none());
        let dir = app_dir_listing("test-mentor");
        let Resume::Start(role, Some(selection)) = plan_resume(Some(&dir), true, &record) else {
            panic!("expected a resume with a selection");
        };

        let mut context = build_process_context(&record);
        context.mentor_history = vec![serde_json::json!({"role": "user", "content": "earlier"})];
        install_resume_selection(&mut context, &role, selection.clone());
        assert_eq!(context.mentor_selection, Some(selection.clone()));
        assert_eq!(
            context.mentor_history.len(),
            1,
            "nothing moved, so the conversation carries over"
        );

        // A stored selection whose endpoint is gone is replaced, and the
        // conversation that was had with it is not replayed to the new one.
        let mut gone = selection.clone();
        gone.provider_id = "gone-1".into();
        gone.account_id = "gone-1-a".into();
        record.provider_sessions.mentor_selection = Some(gone.clone());
        let Resume::Start(_, Some(resolved)) = plan_resume(Some(&dir), true, &record) else {
            panic!("expected a resume with a selection");
        };
        assert_eq!(resolved, selection, "the model resolves where it can run");
        context.mentor_selection = Some(gone);
        context.mentor_session_id = Some("session".into());
        install_resume_selection(&mut context, "mentor", resolved.clone());
        assert_eq!(context.mentor_selection, Some(resolved));
        assert!(context.mentor_history.is_empty());
        assert!(context.mentor_session_id.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// An app data dir whose registry has one ready endpoint listing `model`.
    fn app_dir_listing(model: &str) -> std::path::PathBuf {
        use crate::accounts::store::save;
        use crate::accounts::types::{
            Account, AccountKind, AccountStatus, ModelRoute, ModelSource, Protocol, Provider,
            Secret,
        };

        let dir = std::env::temp_dir().join(format!("the-pair-restore-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        save(
            &dir,
            &[Provider {
                id: "listed-1".into(),
                protocol: Protocol::Anthropic,
                base_url: "https://listed.example".into(),
                vendor: "listed".into(),
                display_name: "Listed".into(),
                accounts: vec![Account {
                    id: "listed-1-a".into(),
                    kind: AccountKind::ApiKey,
                    label: "listed".into(),
                    source: "manual".into(),
                    status: AccountStatus::Ready,
                    headers: vec![],
                    tool_names: None,
                    secret: Secret::ApiKey("k".into()),
                }],
                models: vec![ModelRoute {
                    id: model.into(),
                    display_name: model.into(),
                    context_window: None,
                    max_output: None,
                    efforts: vec![],
                    default_effort: None,
                    price: None,
                    source: ModelSource::Manual,
                }],
            }],
        )
        .unwrap();
        dir
    }

    #[test]
    fn plan_resume_blocks_a_turn_whose_model_resolves_to_nothing() {
        // Restored into Reviewing/Executing with no turn behind it, the pair
        // would hang until a human noticed. It comes back paused instead, with
        // the reason on it.
        let record = snapshot(AgentRole::Executor, PairStatus::Executing, vec![]);
        let nowhere = std::path::Path::new("/nonexistent-app-dir");

        match plan_resume(Some(nowhere), true, &record) {
            Resume::Blocked(role) => assert_eq!(role, "executor"),
            other => panic!("expected a blocked resume, got {:?}", other),
        }

        // No resume asked for, or a pair that was already stopped: nothing to
        // block.
        assert!(matches!(
            plan_resume(Some(nowhere), false, &record),
            Resume::Skip
        ));
        for status in [
            PairStatus::AwaitingHumanReview,
            PairStatus::Error,
            PairStatus::Finished,
        ] {
            let stopped = snapshot(AgentRole::Mentor, status, vec![]);
            assert!(matches!(
                plan_resume(Some(nowhere), true, &stopped),
                Resume::Skip
            ));
        }
    }

    #[test]
    fn a_restored_pair_with_no_model_to_talk_to_comes_back_paused() {
        let record = snapshot(AgentRole::Mentor, PairStatus::Reviewing, vec![]);
        let state = build_pair_state(&record);

        // The state the restore installs for a resume that cannot start.
        let state = paused_for_unresolvable_resume(state, "mentor");

        assert_eq!(state.status, PairStatus::Paused);
        assert_eq!(state.mentor.status, PairStatus::Paused);
        assert_eq!(state.executor.status, PairStatus::Paused);
        assert!(
            state
                .mentor_activity
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("no model it can talk to")),
            "the pair must say why it is not running"
        );
        assert_eq!(
            state.executor_activity.detail, state.mentor_activity.detail,
            "both agents carry the reason"
        );
        // The history is still there for the human to read.
        assert_eq!(state.iteration, 2);
    }

    #[test]
    fn install_restored_context_supersedes_a_live_turn_sharing_the_generation() {
        let spawner = TurnDriver::new();
        let mut record = snapshot(AgentRole::Mentor, PairStatus::Executing, vec![]);
        record.provider_sessions.run_generation = 4;

        // A live turn at the snapshot's own generation, registered the way a
        // running turn is.
        spawner
            .pair_contexts
            .lock()
            .unwrap()
            .insert("pair-1".to_string(), build_process_context(&record));
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        spawner
            .turn_cancels
            .lock()
            .unwrap()
            .insert("pair-1-mentor".to_string(), std::sync::Arc::clone(&flag));

        let mut context = build_process_context(&record);
        install_restored_context(&spawner, "pair-1", &mut context);
        spawner.stop_pair_processes("pair-1");

        let stored = spawner.pair_contexts.lock().unwrap();
        assert_eq!(
            stored["pair-1"].run_generation, 5,
            "the restored context must be past any live turn's generation"
        );
        assert_ne!(
            stored["pair-1"].run_generation, 4,
            "a live turn at the snapshot's generation must not still look current"
        );
        assert!(
            flag.load(std::sync::atomic::Ordering::SeqCst),
            "the live turn is cancelled too"
        );
    }

    #[test]
    fn loading_a_snapshot_never_rewinds_a_pair_that_is_live_here() {
        // The frontend reloads the backend mid-session (its handoff path does),
        // so a pair with a turn in flight must not be rewound to its snapshot.
        let manager = std::sync::Mutex::new(PairManager::new());
        let broker = std::sync::Mutex::new(MessageBroker::new());
        let contexts: ProcessContextMap = Arc::new(Mutex::new(HashMap::new()));

        // The pair on disk: interrupted, at the snapshot's generation 0, with
        // the model it was created with.
        let mut record = snapshot(AgentRole::Mentor, PairStatus::Executing, vec![]);
        record.mentor_model = "snapshot-mentor".to_string();
        record.provider_sessions.run_generation = 0;

        // The pair as it lives here: mid-mentoring, with a plan on screen that
        // no snapshot holds and a context three generations further on.
        let mut live_state = build_pair_state(&record);
        live_state.status = PairStatus::Mentoring;
        live_state.messages = vec![message(
            "live-1",
            MessageSender::Mentor,
            MessageType::Plan,
            "the plan in flight",
            1,
        )];
        broker.lock().unwrap().restore_state(live_state).unwrap();
        let mut live_context = build_process_context(&record);
        live_context.run_generation = 3;
        live_context.mentor_model = "live-mentor".to_string();
        contexts
            .lock()
            .unwrap()
            .insert("pair-1".to_string(), live_context);
        let mut live_pair = build_pair(&record);
        live_pair.mentor_model = "live-mentor".to_string();
        manager.lock().unwrap().upsert_pair(live_pair);

        let returned =
            register_loaded_snapshot(&manager, &broker, &contexts, record).expect("load");

        // The frontend still gets its record, so the pair is never missing from
        // the store...
        assert_eq!(returned.pair_id, "pair-1");
        assert_eq!(returned.mentor_model, "snapshot-mentor");
        // ...while the backend keeps everything the live turn has done.
        let contexts = contexts.lock().unwrap();
        assert_eq!(
            contexts["pair-1"].run_generation, 3,
            "the live turn's generation must not be rewound to the snapshot's"
        );
        assert_eq!(contexts["pair-1"].mentor_model, "live-mentor");
        drop(contexts);
        let state = broker.lock().unwrap().get_state("pair-1").unwrap();
        assert_eq!(state.status, PairStatus::Mentoring);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(
            manager
                .lock()
                .unwrap()
                .get_pair("pair-1")
                .unwrap()
                .mentor_model,
            "live-mentor"
        );
    }

    #[test]
    fn loading_a_snapshot_adopts_a_pair_that_is_not_live_here() {
        let manager = std::sync::Mutex::new(PairManager::new());
        let broker = std::sync::Mutex::new(MessageBroker::new());
        let contexts: ProcessContextMap = Arc::new(Mutex::new(HashMap::new()));
        let mut record = snapshot(AgentRole::Mentor, PairStatus::Executing, vec![]);
        record.provider_sessions.run_generation = 2;

        let returned =
            register_loaded_snapshot(&manager, &broker, &contexts, record).expect("load");

        // An interrupted run comes back paused, with its context and history.
        assert_eq!(returned.status, PairStatus::Paused);
        assert_eq!(contexts.lock().unwrap()["pair-1"].run_generation, 2);
        assert_eq!(
            broker.lock().unwrap().get_state("pair-1").unwrap().status,
            PairStatus::Paused
        );
        assert!(manager.lock().unwrap().get_pair("pair-1").is_some());
    }

    #[test]
    fn pair_is_live_only_for_a_pair_this_process_runs() {
        let broker = MessageBroker::new();
        let contexts: ProcessContextMap =
            std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let record = snapshot(AgentRole::Mentor, PairStatus::Executing, vec![]);

        // Nothing loaded yet: the snapshot is what there is.
        assert!(!pair_is_live(&broker, &contexts, "pair-1"));

        // Broker state without a context (a pair that never ran) is not live.
        broker.restore_state(build_pair_state(&record)).unwrap();
        assert!(!pair_is_live(&broker, &contexts, "pair-1"));

        contexts
            .lock()
            .unwrap()
            .insert("pair-1".to_string(), build_process_context(&record));
        assert!(pair_is_live(&broker, &contexts, "pair-1"));
        assert!(!pair_is_live(&broker, &contexts, "pair-other"));
    }

    #[test]
    fn build_pair_and_process_context_infer_provider_kinds_for_legacy_snapshots() {
        let snapshot = legacy_snapshot(AgentRole::Executor, PairStatus::Executing);

        let pair = build_pair(&snapshot);
        let context = build_process_context(&snapshot);

        // A legacy snapshot names its models the CLI way; the pair keeps them
        // verbatim and the registry resolves them when a turn starts.
        assert_eq!(pair.mentor_model, "claude-3-5-sonnet");
        assert_eq!(context.mentor_model, "claude-3-5-sonnet");
    }

    #[test]
    fn a_legacy_snapshot_restores_an_empty_harness_history() {
        // A snapshot written before the direct connection kept one carries
        // provider sessions with no history in them; the pair starts fresh.
        let json =
            serde_json::to_value(snapshot(AgentRole::Mentor, PairStatus::Idle, vec![])).unwrap();
        let mut bare = json.as_object().unwrap().clone();
        let mut sessions = bare["providerSessions"].as_object().unwrap().clone();
        sessions.remove("mentorHistory");
        sessions.remove("executorHistory");
        bare.insert(
            "providerSessions".to_string(),
            serde_json::to_value(sessions).unwrap(),
        );
        let restored: SessionSnapshotRecord =
            serde_json::from_value(serde_json::Value::Object(bare)).unwrap();

        let context = build_process_context(&restored);
        assert!(context.mentor_history.is_empty());
        assert!(context.executor_history.is_empty());
    }

    #[test]
    fn the_harness_history_survives_a_snapshot_round_trip() {
        let mut record = snapshot(AgentRole::Mentor, PairStatus::Executing, vec![]);
        record.provider_sessions.mentor_history = vec![
            serde_json::json!({"role": "user", "content": "plan it"}),
            serde_json::json!({"role": "assistant", "content": "step one"}),
        ];
        record.provider_sessions.executor_history =
            vec![serde_json::json!({"role": "user", "content": "do it"})];

        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("mentorHistory"), "{}", json);
        let restored: SessionSnapshotRecord = serde_json::from_str(&json).unwrap();
        let context = build_process_context(&restored);

        assert_eq!(context.mentor_history.len(), 2);
        assert_eq!(context.mentor_history[1]["content"], "step one");
        assert_eq!(context.executor_history.len(), 1);
    }

    #[test]
    fn a_restored_pair_keeps_the_model_it_was_talking_to() {
        // Without the selection a restart would drop the pair back onto its
        // provider CLI and never replay the history it also restored.
        let mut record = snapshot(AgentRole::Mentor, PairStatus::Executing, vec![]);
        let selection = crate::harness::ModelSelection {
            provider_id: "anthropic-1".into(),
            account_id: "anthropic-1-a".into(),
            model: "claude-sonnet-4-5".into(),
            effort: Some("high".into()),
            backup: None,
        };
        record.provider_sessions.mentor_selection = Some(selection.clone());
        record.provider_sessions.mentor_history =
            vec![serde_json::json!({"role": "user", "content": "plan it"})];

        let restored: SessionSnapshotRecord =
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        let context = build_process_context(&restored);

        assert_eq!(context.mentor_selection.as_ref(), Some(&selection));
        assert_eq!(context.mentor_history.len(), 1);
        assert!(context.executor_selection.is_none());
    }

    #[test]
    fn a_pair_from_before_the_registry_has_no_selection_to_restore() {
        let record = snapshot(AgentRole::Mentor, PairStatus::Idle, vec![]);
        let context = build_process_context(&record);
        assert!(context.mentor_selection.is_none());
        assert!(context.executor_selection.is_none());
    }

    #[test]
    fn merging_live_state_keeps_the_harness_history_the_turn_wrote() {
        let mut record = snapshot(AgentRole::Executor, PairStatus::Executing, vec![]);
        let pair = build_pair(&record);
        let state = build_pair_state(&record);
        let mut context = build_process_context(&record);
        context.executor_history = vec![serde_json::json!({"role": "user", "content": "go"})];

        merge_state_into_snapshot(&mut record, &pair, &state, Some(&context));

        assert_eq!(record.provider_sessions.executor_history.len(), 1);
        assert_eq!(
            record.provider_sessions.executor_history[0]["content"],
            "go"
        );
        assert!(record.provider_sessions.mentor_history.is_empty());
    }

    #[test]
    fn to_summary_preserves_session_card_and_session_presence_flags() {
        let snapshot = snapshot(AgentRole::Mentor, PairStatus::Idle, vec![]);
        let summary = snapshot.to_summary();

        assert_eq!(summary.pair_id, "pair-1");
        assert_eq!(
            summary
                .current_turn_card
                .as_ref()
                .map(|card| card.id.as_str()),
            Some("turn-1")
        );
        assert!(summary.has_mentor_session);
        assert!(!summary.has_executor_session);
    }

    #[test]
    fn prepare_loaded_snapshot_preserves_messages_for_finished_pair() {
        let history = vec![
            message(
                "msg-1",
                MessageSender::Mentor,
                MessageType::Plan,
                "Step 1: design the parser",
                1,
            ),
            message(
                "msg-2",
                MessageSender::Executor,
                MessageType::Result,
                "TASK_COMPLETE",
                1,
            ),
        ];
        let snap = snapshot(AgentRole::Mentor, PairStatus::Finished, history.clone());

        let (state, returned) = prepare_loaded_snapshot(snap);

        assert_eq!(state.messages.len(), history.len());
        assert_eq!(returned.messages.len(), history.len());
        assert_eq!(returned.messages[0].content, "Step 1: design the parser");
        // Live turn card is transient — must not leak through to the console.
        assert!(returned.current_turn_card.is_none());
    }

    #[test]
    fn prepare_loaded_snapshot_demotes_running_status_but_keeps_history() {
        let history = vec![message(
            "msg-1",
            MessageSender::Mentor,
            MessageType::Plan,
            "In-progress plan",
            1,
        )];
        let snap = snapshot(AgentRole::Mentor, PairStatus::Mentoring, history.clone());

        let (state, returned) = prepare_loaded_snapshot(snap);

        // An interrupted run comes back Paused (resumable), not Idle.
        assert!(matches!(state.status, PairStatus::Paused));
        assert!(matches!(returned.status, PairStatus::Paused));
        assert_eq!(state.messages.len(), history.len());
        assert_eq!(returned.messages.len(), history.len());
        assert!(returned.current_turn_card.is_none());
        assert_eq!(
            state.mentor_activity.detail.as_deref(),
            Some("Interrupted when the app closed. Resume to continue.")
        );
        // The restored state carries what Resume needs.
        assert_eq!(state.task_spec, "Fallback task spec");
        assert_eq!(state.turn, AgentRole::Mentor);
    }

    #[test]
    fn prepare_loaded_snapshot_keeps_pre_v3_running_snapshots_idle() {
        // Pre-2.8.2 files froze at the first session-id registration; offering
        // them for resume would replay a long-dead turn.
        let mut snap = snapshot(AgentRole::Executor, PairStatus::Executing, vec![]);
        snap.snapshot_version = 2;
        let (state, returned) = prepare_loaded_snapshot(snap);
        assert!(matches!(state.status, PairStatus::Idle));
        assert!(matches!(returned.status, PairStatus::Idle));
        assert!(state.mentor_activity.detail.is_none());

        // Same for a current-version snapshot that has no task to resume.
        let mut snap = snapshot(AgentRole::Executor, PairStatus::Executing, vec![]);
        snap.spec = "  ".to_string();
        let (state, returned) = prepare_loaded_snapshot(snap);
        assert!(matches!(state.status, PairStatus::Idle));
        assert!(matches!(returned.status, PairStatus::Idle));
    }

    #[test]
    fn prepare_loaded_snapshot_keeps_stopped_statuses() {
        for status in [
            PairStatus::Idle,
            PairStatus::Paused,
            PairStatus::Error,
            PairStatus::Finished,
            PairStatus::AwaitingHumanReview,
        ] {
            let (state, returned) =
                prepare_loaded_snapshot(snapshot(AgentRole::Mentor, status.clone(), vec![]));
            assert_eq!(state.status, status);
            assert_eq!(returned.status, status);
        }
    }

    /// A draft exactly as the renderer's `snapshotPair` builds it: PascalCase
    /// statuses (also inside run history), optional fields omitted or null,
    /// renderer-only message fields, `tool_call` cognitive events.
    const FRONTEND_DRAFT_JSON: &str = r#"{
        "pairId": "5d9c2f7e-8a1b-4c3d-9e0f-112233445566",
        "name": "Demo Pair",
        "directory": "/tmp/repo/.worktrees/pair-5d9c",
        "spec": "Fix the login bug",
        "status": "Awaiting Human Review",
        "iterations": 2,
        "maxIterations": 0,
        "turn": "mentor",
        "mentorProvider": "claude",
        "mentorModel": "claude-sonnet-4-5",
        "executorProvider": "codex",
        "executorModel": "codex/gpt-5-codex",
        "pendingMentorModel": null,
        "mentorReasoningEffort": "high",
        "messages": [
            {"id": "h1", "timestamp": 1000, "from": "human", "to": "mentor", "type": "plan",
             "content": "Fix the login bug", "iteration": 0},
            {"id": "m1", "timestamp": 2000, "from": "mentor", "to": "human", "type": "plan",
             "content": "1. Read auth.ts", "iteration": 1,
             "tokenUsage": {"outputTokens": 120, "inputTokens": 3400, "lastUpdatedAt": 2000,
                            "source": "final", "provider": "claude"},
             "cognitiveEvents": [{"id": "ce-1", "timestamp": 1500, "role": "mentor",
                                  "eventType": "tool_call", "toolName": "Read",
                                  "description": "Calling Read", "status": "completed"}],
             "startedAt": 1100, "finalizedAt": 2000},
            {"id": "e1", "timestamp": 3000, "from": "executor", "to": "both", "type": "result",
             "content": "Done", "iteration": 1,
             "attachments": [{"path": "src/auth.ts", "description": "auth"}],
             "tokenUsage": {"outputTokens": 50, "lastUpdatedAt": 3000, "source": "live"}},
            {"id": "f1", "timestamp": 3500, "from": "human", "to": "mentor", "type": "feedback",
             "content": "Approved", "iteration": 1}
        ],
        "mentorActivity": {"phase": "waiting", "label": "Awaiting human review",
                           "detail": "Plan ready", "startedAt": 1, "updatedAt": 2,
                           "lastOutputAt": 3, "outputLineCount": 12},
        "executorActivity": {"phase": "using_tools", "label": "Calling Bash",
                             "startedAt": 1, "updatedAt": 2},
        "mentorCpu": 1.5, "mentorMemMb": 120.25, "executorCpu": 0, "executorMemMb": 0,
        "cpuUsage": 1.5, "memUsage": 120.25,
        "modifiedFiles": [
            {"path": "src/auth.ts", "status": "M", "displayPath": "src/auth.ts"},
            {"path": "notes.md", "status": "??", "displayPath": "notes.md"}
        ],
        "gitTracking": {"available": true, "rootPath": "/tmp/repo"},
        "automationMode": "full-auto",
        "latestAcceptance": {
            "iteration": 1, "risk": "medium",
            "checks": [{"name": "npm run test", "command": "npm run test", "status": "failed",
                        "exitCode": null, "durationMs": 1200, "summary": "1 failing",
                        "stdout": "", "stderr": "boom"}],
            "summary": "0 passed, 1 failed, 0 skipped", "startedAt": 10, "finishedAt": 20,
            "verdict": {"verdict": "fail", "risk": "medium", "confidence": 0.4, "issues": ["x"],
                        "evidence": ["y"], "reasoning": "r", "summary": "s",
                        "nextStep": {"action": "continue", "instructions": ["fix test"]}},
            "rawVerdict": "{}", "repairAttempts": 1
        },
        "currentTurnCard": {
            "id": "turn-mentor-2", "role": "mentor", "state": "live", "content": "Working...",
            "activity": {"phase": "thinking", "label": "Reviewing", "startedAt": 1, "updatedAt": 2},
            "startedAt": 1, "updatedAt": 2, "finalizedAt": 3,
            "tokenUsage": {"outputTokens": 5, "lastUpdatedAt": 2, "source": "live"},
            "cognitiveEvents": [{"id": "ce-2", "timestamp": 2, "role": "mentor",
                                 "eventType": "reasoning", "description": "Thinking",
                                 "status": "running"}]
        },
        "runCount": 2,
        "runHistory": [{
            "id": "5d9c-run-1", "spec": "Previous task", "status": "Finished",
            "startedAt": 100, "finishedAt": 200, "mentorModel": "claude-sonnet-4-5",
            "executorModel": "gpt-5", "iterations": 3,
            "messages": [{"id": "old", "timestamp": 150, "from": "mentor", "to": "human",
                          "type": "acceptance", "content": "ok", "iteration": 3}],
            "latestAcceptance": null
        }, {
            "id": "5d9c-run-0", "spec": "Aborted task", "status": "Mentoring",
            "startedAt": 50, "finishedAt": 60, "mentorModel": "m", "executorModel": "e",
            "iterations": 1, "messages": []
        }],
        "currentRunStartedAt": 1000,
        "createdAt": 50,
        "branch": "the-pair/pair-5d9c",
        "repoPath": "/tmp/repo",
        "worktreePath": "/tmp/repo/.worktrees/pair-5d9c",
        "planGate": true
    }"#;

    #[test]
    fn realistic_frontend_draft_deserializes_and_round_trips() {
        let draft: SessionSnapshotDraft =
            serde_json::from_str(FRONTEND_DRAFT_JSON).expect("frontend draft must deserialize");

        assert_eq!(draft.status, PairStatus::AwaitingHumanReview);
        assert_eq!(draft.run_history[0].status, PairStatus::Finished);
        assert_eq!(draft.run_history[1].status, PairStatus::Mentoring);
        assert!(draft.plan_gate);
        let card = draft.current_turn_card.as_ref().unwrap();
        assert_eq!(
            card.cognitive_events.as_ref().unwrap()[0].event_type,
            crate::types::CognitiveEventType::Reasoning
        );

        let record = build_snapshot_from_draft(draft, None);
        let json = serde_json::to_string(&record).unwrap();
        let reloaded: SessionSnapshotRecord =
            serde_json::from_str(&json).expect("persisted record must load back");

        // Renderer-only message fields survive the round trip.
        let mentor = &reloaded.messages[1];
        assert_eq!(mentor.started_at, Some(1100));
        assert_eq!(mentor.finalized_at, Some(2000));
        assert_eq!(
            mentor.cognitive_events.as_ref().unwrap()[0]["eventType"],
            "tool_call"
        );
        assert!(reloaded.messages[2].attachments.is_some());
        assert_eq!(reloaded.run_history.len(), 2);
        assert_eq!(reloaded.status, PairStatus::AwaitingHumanReview);
        // Status keeps its canonical wire spelling when written back.
        assert!(json.contains(r#""status":"finished""#));
    }

    #[test]
    fn every_frontend_status_spelling_deserializes() {
        for (spelling, expected) in [
            ("Idle", PairStatus::Idle),
            ("Mentoring", PairStatus::Mentoring),
            ("Executing", PairStatus::Executing),
            ("Reviewing", PairStatus::Reviewing),
            ("Paused", PairStatus::Paused),
            ("Awaiting Human Review", PairStatus::AwaitingHumanReview),
            ("Error", PairStatus::Error),
            ("Finished", PairStatus::Finished),
        ] {
            let json = FRONTEND_DRAFT_JSON.replace(
                r#""status": "Awaiting Human Review""#,
                &format!(r#""status": "{}""#, spelling),
            );
            let draft: SessionSnapshotDraft = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{} should deserialize: {}", spelling, e));
            assert_eq!(draft.status, expected);
        }
    }

    #[test]
    fn legacy_snapshot_without_newer_fields_still_loads() {
        // Written before runGeneration, acceptanceHistory, planGate, run history
        // messages, the verdict's confidence/issues/reasoning, the harness
        // history and selection, and before Phase 4 dropped the provider kinds
        // and the CPU/memory fields entirely.
        let json = r#"{
            "snapshotVersion": 1, "savedAt": 1, "pairId": "legacy-pair", "name": "Old",
            "directory": "/tmp/old", "spec": "Old task", "status": "reviewing",
            "iterations": 2, "maxIterations": 20, "turn": "mentor",
            "mentorModel": "gpt-4o", "executorModel": "gpt-4o-mini",
            "pendingMentorModel": null, "pendingExecutorModel": null,
            "messages": [], "mentorActivity": {"phase": "idle", "label": "x", "detail": null,
            "startedAt": 0, "updatedAt": 0}, "executorActivity": {"phase": "idle", "label": "x",
            "detail": null, "startedAt": 0, "updatedAt": 0},
            "mentorCpu": 0, "mentorMemMb": 0, "executorCpu": 0, "executorMemMb": 0,
            "cpuUsage": 0, "memUsage": 0, "modifiedFiles": [],
            "gitTracking": {"available": false, "rootPath": null, "baseline": null,
                            "gitReviewAvailable": null},
            "automationMode": "full-auto",
            "latestAcceptance": {"iteration": 1, "risk": "low", "checks": [], "summary": "s",
                "startedAt": 1, "finishedAt": 2,
                "verdict": {"verdict": "pass", "risk": "low", "evidence": [], "summary": "ok",
                            "nextStep": {"action": "finish", "instructions": []}}},
            "currentTurnCard": null, "runCount": 1,
            "runHistory": [{"id": "r", "spec": "s", "status": "finished", "startedAt": 1,
                            "finishedAt": 2, "mentorModel": "m", "executorModel": "e",
                            "iterations": 1}],
            "currentRunStartedAt": 1, "currentRunFinishedAt": null, "createdAt": 1,
            "mentorProvider": "claude", "executorProvider": "codex",
            "mentorCpu": 0, "mentorMemMb": 0, "executorCpu": 0, "executorMemMb": 0,
            "cpuUsage": 0, "memUsage": 0,
            "providerSessions": {"mentorSessionId": null, "executorSessionId": null}
        }"#;

        let snapshot: SessionSnapshotRecord =
            serde_json::from_str(json).expect("legacy snapshot must load");
        assert_eq!(snapshot.provider_sessions.run_generation, 0);
        assert!(snapshot.run_history[0].messages.is_empty());
        let verdict = snapshot.latest_acceptance.unwrap().verdict.unwrap();
        assert_eq!(verdict.confidence, 1.0);
        assert_eq!(verdict.reasoning, "ok");
        assert!(verdict.issues.is_empty());
    }

    fn live_state(status: PairStatus) -> PairState {
        let mut state = build_pair_state(&snapshot(AgentRole::Executor, status.clone(), vec![]));
        state.status = status;
        state.task_spec = "Live task".to_string();
        state.run_started_at = Some(500);
        state.messages = vec![Message {
            timestamp: 600,
            ..message(
                "backend-1",
                MessageSender::Mentor,
                MessageType::Plan,
                "plan",
                1,
            )
        }];
        state
    }

    #[test]
    fn merge_keeps_renderer_owned_fields_and_the_mission_message() {
        let mut existing = snapshot(AgentRole::Mentor, PairStatus::Idle, vec![]);
        existing.messages = vec![
            Message {
                timestamp: 100,
                ..message(
                    "old-run",
                    MessageSender::Mentor,
                    MessageType::Plan,
                    "old",
                    1,
                )
            },
            Message {
                timestamp: 500,
                to: "mentor".to_string(),
                ..message(
                    "mission",
                    MessageSender::Human,
                    MessageType::Plan,
                    "Live task",
                    0,
                )
            },
        ];
        existing.run_count = 3;
        existing.run_history = vec![SnapshotRunSummary {
            id: "r1".to_string(),
            spec: "old".to_string(),
            status: PairStatus::Finished,
            started_at: 1,
            finished_at: Some(2),
            mentor_model: "m".to_string(),
            executor_model: "e".to_string(),
            iterations: 1,
            messages: vec![],
            total_output_tokens: None,
            latest_acceptance: None,
        }];
        existing.pending_mentor_model = Some("pending".to_string());
        existing.plan_gate = true;
        existing.current_run_finished_at = Some(99);

        let pair = build_pair(&existing);
        let mut pair_without_pending = pair.clone();
        pair_without_pending.pending_mentor_model = None;
        pair_without_pending.plan_gate = false;

        let state = live_state(PairStatus::Executing);
        merge_state_into_snapshot(&mut existing, &pair_without_pending, &state, None);

        assert_eq!(existing.spec, "Live task");
        assert_eq!(existing.run_count, 3);
        assert_eq!(existing.run_history.len(), 1);
        assert_eq!(existing.pending_mentor_model.as_deref(), Some("pending"));
        assert!(existing.plan_gate);
        assert!(existing.current_turn_card.is_some());
        let ids: Vec<&str> = existing.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["mission", "backend-1"]);
        assert_eq!(existing.status, PairStatus::Executing);
        assert_eq!(existing.current_run_started_at, 500);
        assert_eq!(
            existing.current_run_finished_at, None,
            "a running run has no finish time"
        );

        let mut finished = live_state(PairStatus::Finished);
        finished.finished_at = Some(777);
        merge_state_into_snapshot(&mut existing, &pair_without_pending, &finished, None);
        assert_eq!(existing.current_run_finished_at, Some(777));
    }

    #[test]
    fn write_json_atomic_leaves_no_temp_files_under_concurrency() {
        let dir =
            std::env::temp_dir().join(format!("the-pair-atomic-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pair-1.json");

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut record = snapshot(AgentRole::Mentor, PairStatus::Idle, vec![]);
                    record.name = format!("writer-{}", i);
                    record.spec = "x".repeat(20_000);
                    write_json_atomic(&path, &record).unwrap();
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }

        let loaded: SessionSnapshotRecord = read_json(&path).expect("result must be valid JSON");
        assert!(loaded.name.starts_with("writer-"));
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_pair_snapshot_rejects_path_traversal_ids() {
        let dir =
            std::env::temp_dir().join(format!("the-pair-delete-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let victim = dir.join("victim.json");
        fs::write(&victim, "{}").unwrap();

        assert!(delete_pair_snapshot_in_dir(&dir.join("sub"), "../victim").is_err());
        assert!(delete_pair_snapshot_in_dir(&dir, "/etc/passwd").is_err());
        assert!(victim.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_process_context_never_falls_back_to_the_main_checkout() {
        let repo = std::env::temp_dir();
        let mut snap = snapshot(AgentRole::Executor, PairStatus::Paused, vec![]);
        snap.worktree_path = Some("/definitely/missing/worktree".to_string());
        snap.directory = "/definitely/missing/worktree".to_string();
        snap.repo_path = Some(repo.to_string_lossy().to_string());

        let context = build_process_context(&snap);
        assert_eq!(context.directory, "/definitely/missing/worktree");
    }

    #[test]
    fn build_pair_derives_the_worktree_branch_for_legacy_snapshots() {
        let mut snap = snapshot(AgentRole::Mentor, PairStatus::Idle, vec![]);
        snap.worktree_path = Some("/tmp/repo/.worktrees/pair-abc".to_string());
        // Snapshots from before the field existed record no worktree branch;
        // it is derived from the worktree directory, which names it.
        snap.worktree_branch = None;

        let pair = build_pair(&snap);
        assert_eq!(pair.worktree_branch.as_deref(), Some("the-pair/pair-abc"));

        snap.worktree_branch = Some("the-pair/pair-recorded".to_string());
        assert_eq!(
            build_pair(&snap).worktree_branch.as_deref(),
            Some("the-pair/pair-recorded")
        );
    }

    #[test]
    fn build_snapshot_from_state_records_the_pairs_worktree_branch() {
        let snap = snapshot(AgentRole::Executor, PairStatus::Paused, vec![]);
        let state = build_pair_state(&snap);
        let mut pair = build_pair(&snap);
        pair.branch = Some("main".to_string());
        pair.repo_path = Some("/tmp/repo".to_string());
        pair.worktree_path = Some("/tmp/repo/.worktrees/pair-abc".to_string());
        pair.worktree_branch = Some("the-pair/pair-abc".to_string());

        let record = build_snapshot_from_state(&pair, &state, None);

        assert_eq!(
            record.worktree_branch.as_deref(),
            Some("the-pair/pair-abc"),
            "the snapshot must record the checked-out pair branch"
        );
        assert_eq!(
            record.worktree_path.as_deref(),
            Some("/tmp/repo/.worktrees/pair-abc")
        );
    }

    #[test]
    fn delete_pair_snapshot_in_dir_removes_snapshot_file_and_index_entry() {
        let dir =
            std::env::temp_dir().join(format!("the-pair-snapshot-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        let snapshot = snapshot(AgentRole::Mentor, PairStatus::Idle, vec![]);
        let pair_id = snapshot.pair_id.clone();
        let snapshot_path = dir.join(format!("{}.json", pair_id));
        let index_path = dir.join("index.json");

        fs::write(
            &snapshot_path,
            serde_json::to_vec_pretty(&snapshot).unwrap(),
        )
        .unwrap();

        let summaries = vec![
            snapshot.to_summary(),
            RecoverableSessionSummary {
                pair_id: "pair-2".to_string(),
                name: "Keep".to_string(),
                directory: "/tmp/keep".to_string(),
                spec: "Keep this".to_string(),
                status: PairStatus::Idle,
                turn: AgentRole::Executor,
                mentor_model: "mentor".to_string(),
                executor_model: "executor".to_string(),
                pending_mentor_model: None,
                pending_executor_model: None,
                mentor_reasoning_effort: None,
                executor_reasoning_effort: None,
                run_count: 1,
                current_run_started_at: 1,
                current_run_finished_at: None,
                saved_at: 20,
                created_at: 2,
                current_turn_card: None,
                has_mentor_session: false,
                has_executor_session: false,
            },
        ];
        fs::write(&index_path, serde_json::to_vec_pretty(&summaries).unwrap()).unwrap();

        delete_pair_snapshot_in_dir(&dir, &pair_id).unwrap();

        assert!(!snapshot_path.exists());
        let remaining = fs::read_to_string(&index_path).unwrap();
        assert!(remaining.contains("pair-2"));
        assert!(!remaining.contains(&pair_id));

        let _ = fs::remove_dir_all(&dir);
    }
}
