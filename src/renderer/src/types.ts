type TokenUsageSource = 'live' | 'final' | 'none'

export type AcceptanceCheckStatus = 'passed' | 'failed' | 'skipped'
export type AcceptanceRisk = 'low' | 'medium' | 'high'
type AcceptanceVerdictDecision = 'pass' | 'fail'
type AcceptanceNextAction = 'continue' | 'finish'

export interface AcceptanceCheckRun {
  name: string
  command: string
  status: AcceptanceCheckStatus
  exitCode: number | null
  durationMs: number
  summary: string
  stdout: string
  stderr: string
}

interface AcceptanceNextStep {
  action: AcceptanceNextAction
  instructions: string[]
}

export interface AcceptanceVerdict {
  verdict: AcceptanceVerdictDecision
  risk: AcceptanceRisk
  confidence: number
  issues: string[]
  evidence: string[]
  reasoning: string
  summary: string
  nextStep: AcceptanceNextStep
}

export interface AcceptanceRecord {
  iteration: number
  risk: AcceptanceRisk
  checks: AcceptanceCheckRun[]
  summary: string
  startedAt: number
  finishedAt: number
  verdict?: AcceptanceVerdict
  rawVerdict?: string
  error?: string
  repairAttempts?: number
}

export interface TurnTokenUsage {
  outputTokens: number
  inputTokens?: number
  lastUpdatedAt: number
  source: TokenUsageSource
  provider?: string
}

export type PairStatus =
  | 'Idle'
  | 'Mentoring'
  | 'Executing'
  | 'Reviewing'
  | 'Paused'
  | 'Awaiting Human Review'
  | 'Error'
  | 'Finished'

export interface CreatePairInput {
  name: string
  directory: string
  spec: string
  /** A model id the registry can resolve onto an endpoint and a credential. */
  mentor: { role: 'mentor' | 'executor'; model: string }
  executor: { role: 'mentor' | 'executor'; model: string }
  mentorReasoningEffort?: string
  executorReasoningEffort?: string
  branch?: string
  /** When true, work in a fresh worktree on an auto-named branch (see Rust `CreatePairInput`). */
  useWorktree?: boolean
  maxIterations?: number
  planGate?: boolean
}

export interface PairModelSelection {
  mentorModel: string
  executorModel: string
  pendingMentorModel?: string
  pendingExecutorModel?: string
  mentorReasoningEffort?: string
  executorReasoningEffort?: string
}

/** The model one agent talks to directly, and the account it authenticates with.
 *  Mirrors the Rust `harness::ModelSelection`. */
export interface HarnessModelSelection {
  providerId: string
  accountId: string
  model: string
  effort?: string
  backup?: HarnessModelSelection
}

interface AgentActivity {
  phase: 'idle' | 'thinking' | 'using_tools' | 'responding' | 'waiting' | 'error' | 'stalled'
  label: string
  detail?: string
  startedAt: number
  updatedAt: number
  lastOutputAt?: number
  outputLineCount?: number
}

interface SnapshotTurnCard {
  id: string
  role: 'mentor' | 'executor'
  state: 'live' | 'final'
  content: string
  activity: AgentActivity
  startedAt: number
  updatedAt: number
  tokenUsage?: TurnTokenUsage
  cognitiveEvents?: SnapshotCognitiveEvent[]
}

interface SnapshotCognitiveEvent {
  id: string
  timestamp: number
  role: 'mentor' | 'executor'
  eventType: 'tool_call' | 'reasoning' | 'error'
  toolName?: string
  description: string
  status: 'running' | 'completed' | 'error'
}

interface PairRunSummary {
  id: string
  spec: string
  status: PairStatus
  startedAt: number
  finishedAt?: number
  mentorModel: string
  executorModel: string
  iterations: number
  messages: Array<{
    id: string
    timestamp: number
    from: 'mentor' | 'executor' | 'human'
    to: 'mentor' | 'executor' | 'both' | 'human'
    type: 'plan' | 'feedback' | 'progress' | 'result' | 'question' | 'handoff' | 'acceptance'
    content: string
    attachments?: { path: string; description: string }[]
    iteration: number
    tokenUsage?: TurnTokenUsage
  }>
  totalOutputTokens?: number
  latestAcceptance?: AcceptanceRecord
}

export interface SessionSnapshotDraft {
  pairId: string
  name: string
  directory: string
  spec: string
  status: PairStatus
  iterations: number
  maxIterations: number
  turn: 'mentor' | 'executor'
  mentorModel: string
  executorModel: string
  pendingMentorModel?: string
  pendingExecutorModel?: string
  mentorReasoningEffort?: string
  executorReasoningEffort?: string
  messages: Array<{
    id: string
    timestamp: number
    from: 'mentor' | 'executor' | 'human'
    to: 'mentor' | 'executor' | 'both' | 'human'
    type: 'plan' | 'feedback' | 'progress' | 'result' | 'question' | 'handoff' | 'acceptance'
    content: string
    attachments?: { path: string; description: string }[]
    iteration: number
    tokenUsage?: TurnTokenUsage
  }>
  mentorActivity: AgentActivity
  executorActivity: AgentActivity
  modifiedFiles: Array<{ path: string; status: 'A' | 'M' | 'D' | 'R' | '??'; displayPath: string }>
  gitTracking: {
    available: boolean
    rootPath?: string
    baseline?: string
    gitReviewAvailable?: boolean
  }
  automationMode: 'full-auto'
  latestAcceptance?: AcceptanceRecord
  currentTurnCard?: SnapshotTurnCard
  runCount: number
  runHistory: PairRunSummary[]
  currentRunStartedAt: number
  currentRunFinishedAt?: number
  createdAt: number
  branch?: string
  repoPath?: string
  worktreePath?: string
  worktreeBranch?: string
  planGate?: boolean
  cognitiveEvents?: SnapshotCognitiveEvent[]
}

export interface SessionSnapshotRecord extends SessionSnapshotDraft {
  snapshotVersion: number
  savedAt: number
  providerSessions: {
    mentorSessionId?: string
    executorSessionId?: string
    /** The harness conversation each agent carries, replayed on every turn. */
    mentorHistory?: unknown[]
    executorHistory?: unknown[]
    /** The model each agent talks to directly. */
    mentorSelection?: HarnessModelSelection
    executorSelection?: HarnessModelSelection
  }
}

export interface BranchInfo {
  name: string
  isLocal: boolean
  isRemote: boolean
  lastCommitSha?: string
  lastCommitMessage?: string
  lastCommitDate?: number
  isCheckedOutLocally: boolean
}

export interface RepoState {
  isGitRepo: boolean
  isDirty: boolean
  currentBranch?: string
  branches: BranchInfo[]
}

export interface PairPreset {
  id: string
  name: string
  description: string
  icon: string
  mentorPromptTemplate: string
  executorPromptTemplate: string
  recommendedSkills: string[]
  recommendedMentorModel?: string
  recommendedExecutorModel?: string
  pauseOnIteration?: number
}

/** One mentor+executor combo row in the cross-run insights leaderboard. */
export interface ConfigStat {
  mentorModel: string
  executorModel: string
  providerKind: string
  runs: number
  successes: number
  successRate: number
  avgIterations: number
  avgTokens: number
}

/** Aggregated cross-run stats returned by `get_insights_summary`. */
export interface InsightsSummary {
  totalRuns: number
  successfulRuns: number
  successRate: number
  combos: ConfigStat[]
}

/** A recommended mentor+executor combo for a draft task spec. */
export interface ConfigRecommendation {
  mentorModel: string
  executorModel: string
  runs: number
  successes: number
  successRate: number
}

// ---------------------------------------------------------------------------
// Direct-connection model registry (accounts_* commands)
// ---------------------------------------------------------------------------

/** The wire protocol a provider speaks. Mirrors `accounts::types::Protocol`. */
export type ModelProtocol = 'openaiChat' | 'anthropic' | 'responses' | 'commandCode'

/** How an account authenticates. Mirrors `accounts::types::AccountKind`. */
export type AccountKind = 'apiKey' | 'chatgptOauth' | 'commandCodeOauth' | 'opencodeGo' | 'env'

/** Whether an account can be used right now. */
export type AccountStatus = 'ready' | 'needsKey' | 'authExpired' | 'rateLimited'

/** Where a model entry came from. */
export type ModelSource = 'preset' | 'imported' | 'endpoint' | 'manual'

export interface ModelPrice {
  inputPerMillion: number
  outputPerMillion: number
}

export interface ModelRoute {
  id: string
  displayName: string
  contextWindow?: number
  maxOutput?: number
  efforts: string[]
  defaultEffort?: string
  price?: ModelPrice
  source: ModelSource
}

export interface HeaderPair {
  name: string
  value: string
}

export interface AccountView {
  id: string
  kind: AccountKind
  label: string
  /** Which importer or sign-in flow produced the account. */
  source: string
  status: AccountStatus
  headers: HeaderPair[]
  toolNames?: Record<string, string>
  hasSecret: boolean
  maskedSecret?: string
}

export interface ProviderView {
  id: string
  protocol: ModelProtocol
  baseUrl: string
  vendor: string
  displayName: string
  accounts: AccountView[]
  models: ModelRoute[]
  readyAccountCount: number
}

export interface CandidateAccountView {
  kind: AccountKind
  label: string
  source: string
  status: AccountStatus
  maskedSecret?: string
  headers: HeaderPair[]
}

export interface CandidateProviderView {
  protocol: ModelProtocol
  baseUrl: string
  vendor: string
  displayName: string
  accounts: CandidateAccountView[]
  models: ModelRoute[]
  /** True when this endpoint is already in the registry. */
  alreadyImported: boolean
}

/** What a dry-run scan found for one source, before anything is written. */
export interface CandidateView {
  sourceId: string
  sourceName: string
  configPath?: string
  providers: CandidateProviderView[]
  notes: string[]
  error?: string
  accountCount: number
  newProviderCount: number
}

export interface LoginServiceView {
  id: string
  displayName: string
  needsBrowser: boolean
  needsKey: boolean
}

export type OAuthState = 'waiting' | 'done' | 'failed' | 'cancelled' | 'unknown'

export interface OAuthStatusView {
  state: OAuthState
  service: string
  waitingMs: number
  message?: string
  providers?: ProviderView[]
}

export interface AddProviderInput {
  protocol: ModelProtocol
  baseUrl: string
  apiKey: string
  label?: string
  models: string[]
}
