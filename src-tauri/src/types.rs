use crate::provider_registry::ProviderKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TokenUsageSource {
    Live,
    Final,
    #[default]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AcceptanceCheckStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AcceptanceRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AcceptanceVerdictDecision {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AcceptanceNextAction {
    Continue,
    Finish,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceCheckRun {
    pub name: String,
    pub command: String,
    pub status: AcceptanceCheckStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub summary: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceNextStep {
    pub action: AcceptanceNextAction,
    pub instructions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceVerdict {
    pub verdict: AcceptanceVerdictDecision,
    pub risk: AcceptanceRisk,
    pub confidence: f64,     // Required: 0.0-1.0
    pub issues: Vec<String>, // Required: can be empty
    pub evidence: Vec<String>,
    pub reasoning: String, // Required: detailed assessment
    pub summary: String,
    pub next_step: AcceptanceNextStep,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceRecord {
    pub iteration: u32,
    pub risk: AcceptanceRisk,
    pub checks: Vec<AcceptanceCheckRun>,
    pub summary: String,
    pub started_at: u64,
    pub finished_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<AcceptanceVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub repair_attempts: u32,
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IterationMetric {
    pub iteration: u32,
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub files_changed: Vec<ModifiedFile>,
    pub error_logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReport {
    pub session_id: String,
    pub pair_name: String,
    pub task_spec: String,
    pub started_at: u64,
    pub finished_at: u64,
    pub iterations: u32,
    pub final_verdict: AcceptanceVerdict,
    pub validation_history: Vec<AcceptanceRecord>,
    pub git_changes: Vec<ModifiedFile>,
    pub messages: Vec<Message>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub iteration_metrics: Vec<IterationMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnTokenUsage {
    pub output_tokens: u64,
    pub input_tokens: Option<u64>,
    pub last_updated_at: u64,
    pub source: TokenUsageSource,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PairStatus {
    Idle,
    Mentoring,
    Executing,
    Reviewing,
    Paused,
    #[serde(rename = "Awaiting Human Review")]
    AwaitingHumanReview,
    Error,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    Mentor,
    Executor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Plan,
    Feedback,
    Progress,
    Result,
    Question,
    Acceptance,
    Handoff,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageSender {
    Mentor,
    Executor,
    Human,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityPhase {
    Idle,
    Thinking,
    #[serde(rename = "using_tools")]
    UsingTools,
    Responding,
    Waiting,
    Error,
    Stalled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivity {
    pub phase: ActivityPhase,
    pub label: String,
    pub detail: Option<String>,
    #[serde(rename = "startedAt")]
    pub started_at: u64,
    #[serde(rename = "updatedAt")]
    pub updated_at: u64,
    #[serde(rename = "lastOutputAt", skip_serializing_if = "Option::is_none")]
    pub last_output_at: Option<u64>,
    #[serde(rename = "outputLineCount", default)]
    pub output_line_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub cpu: f64,
    #[serde(rename = "memMb")]
    pub mem_mb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResources {
    pub mentor: ResourceInfo,
    pub executor: ResourceInfo,
    #[serde(rename = "pairTotal")]
    pub pair_total: ResourceInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileStatus {
    A,
    M,
    D,
    R,
    #[serde(rename = "??")]
    Untracked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedFile {
    pub path: String,
    pub status: FileStatus,
    #[serde(rename = "displayPath")]
    pub display_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitTracking {
    pub available: bool,
    #[serde(rename = "rootPath")]
    pub root_path: Option<String>,
    pub baseline: Option<String>,
    #[serde(rename = "gitReviewAvailable")]
    pub git_review_available: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub role: AgentRole,
    pub provider: ProviderKind,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePairInput {
    pub name: String,
    pub directory: String,
    pub spec: String,
    pub mentor: AgentConfig,
    pub executor: AgentConfig,
    #[serde(rename = "mentorReasoningEffort")]
    pub mentor_reasoning_effort: Option<String>,
    #[serde(rename = "executorReasoningEffort")]
    pub executor_reasoning_effort: Option<String>,
    pub branch: Option<String>,
    #[serde(rename = "maxIterations")]
    pub max_iterations: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignTaskInput {
    pub spec: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct UpdatePairModelsInput {
    #[serde(rename = "mentorModel")]
    pub mentor_model: String,
    #[serde(rename = "executorModel")]
    pub executor_model: String,
    #[serde(rename = "pendingMentorModel")]
    pub pending_mentor_model: Option<String>,
    #[serde(rename = "pendingExecutorModel")]
    pub pending_executor_model: Option<String>,
    #[serde(rename = "mentorReasoningEffort")]
    pub mentor_reasoning_effort: Option<String>,
    #[serde(rename = "executorReasoningEffort")]
    pub executor_reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub status: PairStatus,
    pub turn: AgentRole,
    #[serde(rename = "lastMessage")]
    pub last_message: Option<Message>,
    pub activity: AgentActivity,
    #[serde(rename = "tokenUsage")]
    pub token_usage: Option<TurnTokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairState {
    #[serde(rename = "pairId")]
    pub pair_id: String,
    pub directory: String,
    pub status: PairStatus,
    pub iteration: u32,
    #[serde(rename = "maxIterations")]
    pub max_iterations: u32,
    pub turn: AgentRole,
    pub mentor: AgentState,
    pub executor: AgentState,
    pub messages: Vec<Message>,
    #[serde(rename = "mentorActivity")]
    pub mentor_activity: AgentActivity,
    #[serde(rename = "executorActivity")]
    pub executor_activity: AgentActivity,
    pub resources: PairResources,
    #[serde(rename = "modifiedFiles")]
    pub modified_files: Vec<ModifiedFile>,
    #[serde(rename = "gitTracking")]
    pub git_tracking: GitTracking,
    #[serde(rename = "automationMode")]
    pub automation_mode: String,
    #[serde(rename = "gitReviewAvailable")]
    pub git_review_available: bool,
    #[serde(rename = "finishedAt")]
    pub finished_at: Option<u64>,
    #[serde(rename = "latestAcceptance")]
    pub latest_acceptance: Option<AcceptanceRecord>,
    #[serde(rename = "acceptanceHistory", default)]
    pub acceptance_history: Vec<AcceptanceRecord>,
    #[serde(rename = "worktreePath")]
    pub worktree_path: Option<String>,
    #[serde(rename = "turnStartedAt", skip_serializing_if = "Option::is_none")]
    pub turn_started_at: Option<u64>,
    #[serde(rename = "planChecklist", default)]
    pub plan_checklist: Vec<serde_json::Value>,
    #[serde(rename = "keyDecisions", default)]
    pub key_decisions: Vec<String>,
    #[serde(rename = "cognitiveEvents", default)]
    pub cognitive_events: Vec<CognitiveEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pair {
    #[serde(rename = "pairId")]
    pub pair_id: String,
    pub name: String,
    pub directory: String,
    pub status: PairStatus,
    #[serde(rename = "mentorProvider")]
    pub mentor_provider: ProviderKind,
    #[serde(rename = "mentorModel")]
    pub mentor_model: String,
    #[serde(rename = "executorProvider")]
    pub executor_provider: ProviderKind,
    #[serde(rename = "executorModel")]
    pub executor_model: String,
    #[serde(rename = "pendingMentorModel")]
    pub pending_mentor_model: Option<String>,
    #[serde(rename = "pendingExecutorModel")]
    pub pending_executor_model: Option<String>,
    #[serde(rename = "mentorReasoningEffort")]
    pub mentor_reasoning_effort: Option<String>,
    #[serde(rename = "executorReasoningEffort")]
    pub executor_reasoning_effort: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: u64,
    pub branch: Option<String>,
    #[serde(rename = "repoPath")]
    pub repo_path: Option<String>,
    #[serde(rename = "worktreePath")]
    pub worktree_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveEvent {
    pub id: String,
    pub timestamp: u64,
    pub role: AgentRole,
    #[serde(rename = "eventType")]
    pub event_type: CognitiveEventType,
    #[serde(rename = "toolName", skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    pub description: String,
    pub status: CognitiveEventStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CognitiveEventType {
    ToolCall,
    Reasoning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CognitiveEventStatus {
    Running,
    Completed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub timestamp: u64,
    pub from: MessageSender,
    pub to: String,
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    pub content: String,
    pub iteration: u32,
    #[serde(rename = "tokenUsage", skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TurnTokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityType {
    StatusChange,
    Handoff,
    Result,
    Acceptance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentActivity {
    pub pair_id: String,
    pub pair_name: String,
    pub activity_type: ActivityType,
    pub description: String,
    pub timestamp: u64,
    pub role: Option<String>,
}
