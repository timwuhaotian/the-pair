import { create } from 'zustand'
import type {
  AcceptanceRecord,
  AvailableModel,
  CreatePairInput,
  PairModelSelection,
  SessionSnapshotDraft,
  SessionSnapshotRecord,
  TurnTokenUsage
} from '../types'
import {
  turnCardToMessage as turnCardToMessageImpl,
  resolveCurrentTurnTokenUsage,
  syncTokenUsage as syncTokenUsageImpl,
  type TokenUsageTurnCard,
  type TokenUsageMessage
} from '../lib/tokenUsage'
import {
  buildAgentConfig,
  getModelByQualifiedId,
  inferProviderFromModel
} from '../lib/providerResolution'
import {
  buildExecutorAcceptanceFollowupPrompt,
  buildMentorAcceptanceRepairPrompt,
  buildMentorAcceptancePrompt
} from '../lib/acceptance'
import {
  resolveEffectiveModels,
  buildUpdateModelsPayload,
  shouldSyncModelsToBackend
} from '../lib/modelResolution'
import { shouldSaveSnapshot as shouldSaveSnapshotImpl } from '../lib/snapshotDiff'
import { shouldIgnoreHandoffEvent } from '../lib/handoffGuard'
import {
  buildInitialExecutorHandoffPrompt,
  buildInitialMentorReviewPrompt,
  buildPlanRevisionPrompt
} from '../lib/handoffPrompts'
import { playFinishChime, playErrorAlert, playPauseConfirm } from '../lib/sound'
import { resolvePairSoundCue, type PairSoundCue } from '../lib/pairSoundCue'
import { extractErrorMessage } from '../lib/utils'
import { isPairActive } from '../lib/pairStatus'
import i18n from '../i18n'

export type PairStatus =
  | 'Idle'
  | 'Mentoring'
  | 'Executing'
  | 'Reviewing'
  | 'Paused'
  | 'Awaiting Human Review'
  | 'Error'
  | 'Finished'

export type ActivityPhase =
  | 'idle'
  | 'thinking'
  | 'using_tools'
  | 'responding'
  | 'waiting'
  | 'error'
  | 'stalled'

export interface AgentActivity {
  phase: ActivityPhase
  label: string
  detail?: string
  startedAt: number
  updatedAt: number
  lastOutputAt?: number
  outputLineCount?: number
}

export interface ResourceInfo {
  cpu: number
  memMb: number
}

export interface PairResources {
  mentor: ResourceInfo
  executor: ResourceInfo
  pairTotal: ResourceInfo
}

export type FileStatus = 'A' | 'M' | 'D' | 'R' | '??'

export interface ModifiedFile {
  path: string
  status: FileStatus
  displayPath: string
}

export interface GitTracking {
  available: boolean
  rootPath?: string
}

export type AutomationMode = 'full-auto'

export interface Message {
  id: string
  timestamp: number
  from: 'mentor' | 'executor' | 'human'
  to: 'mentor' | 'executor' | 'both' | 'human'
  type: 'plan' | 'feedback' | 'progress' | 'result' | 'question' | 'handoff' | 'acceptance'
  content: string
  attachments?: { path: string; description: string }[]
  iteration: number
  tokenUsage?: TurnTokenUsage
  /** Tool calls / reasoning events from the turn that produced this message — used for post-hoc step replay. */
  cognitiveEvents?: CognitiveEvent[]
  /** Wall-clock start of the turn (used to compute duration in chat-history display). */
  startedAt?: number
  /** Wall-clock end of the turn. */
  finalizedAt?: number
}

export type CognitiveEventType = 'tool_call' | 'reasoning' | 'error'
export type CognitiveEventStatus = 'running' | 'completed' | 'error'

export interface CognitiveEvent {
  id: string
  timestamp: number
  role: 'mentor' | 'executor'
  eventType: CognitiveEventType
  toolName?: string
  description: string
  status: CognitiveEventStatus
}

export interface TurnCard {
  id: string
  role: 'mentor' | 'executor'
  state: 'live' | 'final'
  content: string
  activity: AgentActivity
  startedAt: number
  updatedAt: number
  finalizedAt?: number
  tokenUsage?: TurnTokenUsage
  cognitiveEvents: CognitiveEvent[]
}

export interface PairRunSummary {
  id: string
  spec: string
  status: PairStatus
  startedAt: number
  finishedAt?: number
  mentorModel: string
  executorModel: string
  iterations: number
  messages: Message[]
  latestAcceptance?: AcceptanceRecord
}

export interface Pair {
  id: string
  name: string
  directory: string
  createdAt: number
  status: PairStatus
  iterations: number
  maxIterations: number
  cpuUsage: number
  memUsage: number
  spec: string
  mentorProvider: AvailableModel['provider']
  mentorModel: string
  executorProvider: AvailableModel['provider']
  executorModel: string
  pendingMentorModel?: string
  pendingExecutorModel?: string
  mentorReasoningEffort?: string
  executorReasoningEffort?: string
  messages: Message[]
  mentorActivity: AgentActivity
  executorActivity: AgentActivity
  mentorCpu: number
  mentorMemMb: number
  executorCpu: number
  executorMemMb: number
  modifiedFiles: ModifiedFile[]
  gitTracking: GitTracking
  automationMode: AutomationMode
  latestAcceptance?: AcceptanceRecord
  turn: 'mentor' | 'executor'
  currentTurnCard?: TurnCard
  runCount: number
  runHistory: PairRunSummary[]
  currentRunStartedAt: number
  currentRunFinishedAt?: number
  mentorTokenUsage?: TurnTokenUsage
  executorTokenUsage?: TurnTokenUsage
  branch?: string
  repoPath?: string
  worktreePath?: string
  turnStartedAt?: number
  adaptiveBudget?: number
  pauseMessage?: string
  planChecklist?: Array<{ description: string; completed: boolean }>
  keyDecisions?: string[]
  /** When true, the pair pauses for human plan approval before the executor starts. */
  planGate?: boolean
}

interface PairStateSnapshot {
  pairId?: string
  status?: PairStatus | string
  iteration?: number
  maxIterations?: number
  turn?: 'mentor' | 'executor' | string
  finishedAt?: number
  mentorStatus?: PairStatus
  executorStatus?: PairStatus
  mentorActivity?: AgentActivity
  executorActivity?: AgentActivity
  resources?: PairResources
  modifiedFiles?: ModifiedFile[]
  gitTracking?: GitTracking
  automationMode?: AutomationMode
  latestAcceptance?: AcceptanceRecord
  mentor?: { tokenUsage: TurnTokenUsage | null }
  executor?: { tokenUsage: TurnTokenUsage | null }
  messages?: Message[]
  turnStartedAt?: number
  adaptiveBudget?: number
  pauseMessage?: string
  planChecklist?: Array<{ description: string; completed: boolean }>
  keyDecisions?: string[]
  cognitiveEvents?: CognitiveEvent[]
}

interface PairMessageEvent {
  pairId: string
  message: Message
}

interface PairHandoffEvent {
  pairId: string
  nextRole: 'mentor' | 'executor'
}

interface PairCreatedResponse {
  pairId: string
  branch?: string
  repoPath?: string
  worktreePath?: string
  directory?: string
}

interface BackendPairState {
  pairId?: string
  status?: PairStatus | string
  iteration?: number
  messages?: Message[]
  latestAcceptance?: AcceptanceRecord
}

interface PairStore {
  pairs: Pair[]
  availableModels: AvailableModel[]
  isLoading: boolean
  isLoadingModels: boolean
  modelsLastUpdatedAt: number | null
  modelsError: string | null
  error: string | null
  viewingRunId: string | null
  restoringSpec: { spec: string; mentorModel: string; executorModel: string } | null

  loadAvailableModels: () => Promise<void>
  loadAllPairs: () => Promise<void>
  flushSnapshots: () => Promise<void>
  createPair: (
    input: Omit<CreatePairInput, 'mentor' | 'executor'> & {
      mentorModel: string
      executorModel: string
      mentorReasoningEffort?: string
      executorReasoningEffort?: string
      branch?: string
      maxIterations?: number
      pauseOnIteration?: number
      autoAttachGitBaseline?: boolean
      planGate?: boolean
    }
  ) => Promise<void>
  assignTask: (
    pairId: string,
    spec: string,
    role?: string,
    modelOverrides?: { mentorModel?: string; executorModel?: string },
    options?: { maxIterations?: number }
  ) => Promise<void>
  updatePairModels: (pairId: string, selection: PairModelSelection) => Promise<void>
  pausePair: (id: string) => Promise<void>
  resumePair: (id: string) => Promise<void>
  resolvePlanReview: (
    pairId: string,
    decision: 'approve' | 'reject',
    feedback?: string
  ) => Promise<void>
  deletePair: (id: string) => Promise<void>
  killProcess: (pairId: string, role: string) => Promise<void>
  updatePairStatus: (id: string, status: PairStatus) => void
  updatePairUsage: (id: string, cpu: number, mem: number) => void
  addMessage: (pairId: string, message: Message) => void
  setMessages: (pairId: string, messages: Message[]) => void
  syncState: (pairId: string, status: PairStatus, iteration: number) => void
  syncFullState: (pairId: string, state: Record<string, unknown>) => void
  retryTurn: (id: string) => Promise<void>
  initMessageListener: () => void
  viewTaskHistory: (pairId: string, runId: string) => void
  clearViewingTask: (pairId: string) => void
  setViewingRunId: (runId: string | null) => void
  setRestoringSpec: (
    spec: { spec: string; mentorModel: string; executorModel: string } | null
  ) => void
}

let _listenersInitialized = false
let _modelsLoading = false
const _handoffLocks = new Map<string, Promise<void>>()
const _manualPauseTimestamps = new Map<string, number>()

function dispatchPairSoundCue(cue: PairSoundCue): void {
  switch (cue) {
    case 'finish':
      playFinishChime()
      break
    case 'attention':
      // Reuse the finish chime: mentor delivered a verdict that needs review.
      playFinishChime()
      break
    case 'pause':
      playPauseConfirm()
      break
    case 'error':
      playErrorAlert()
      break
    default:
      break
  }
}

function createIdleActivity(label: string): AgentActivity {
  const now = Date.now()
  return {
    phase: 'idle',
    label,
    startedAt: now,
    updatedAt: now,
    lastOutputAt: undefined,
    outputLineCount: 0
  }
}

function snapshotPair(pair: Pair): SessionSnapshotDraft {
  return {
    pairId: pair.id,
    name: pair.name,
    directory: pair.directory,
    spec: pair.spec,
    status: pair.status,
    iterations: pair.iterations,
    maxIterations: pair.maxIterations,
    turn: pair.turn,
    mentorProvider: pair.mentorProvider,
    mentorModel: pair.mentorModel,
    executorProvider: pair.executorProvider,
    executorModel: pair.executorModel,
    pendingMentorModel: pair.pendingMentorModel,
    pendingExecutorModel: pair.pendingExecutorModel,
    mentorReasoningEffort: pair.mentorReasoningEffort,
    executorReasoningEffort: pair.executorReasoningEffort,
    messages: pair.messages,
    mentorActivity: pair.mentorActivity,
    executorActivity: pair.executorActivity,
    mentorCpu: pair.mentorCpu,
    mentorMemMb: pair.mentorMemMb,
    executorCpu: pair.executorCpu,
    executorMemMb: pair.executorMemMb,
    cpuUsage: pair.cpuUsage,
    memUsage: pair.memUsage,
    modifiedFiles: pair.modifiedFiles,
    gitTracking: {
      available: pair.gitTracking.available,
      rootPath: pair.gitTracking.rootPath,
      baseline: undefined,
      gitReviewAvailable: undefined
    },
    automationMode: pair.automationMode,
    latestAcceptance: pair.latestAcceptance,
    currentTurnCard: pair.currentTurnCard,
    runCount: pair.runCount,
    runHistory: pair.runHistory,
    currentRunStartedAt: pair.currentRunStartedAt,
    currentRunFinishedAt: pair.currentRunFinishedAt,
    createdAt: pair.createdAt,
    branch: pair.branch,
    repoPath: pair.repoPath,
    worktreePath: pair.worktreePath,
    planGate: pair.planGate
  }
}

function snapshotToPair(snapshot: SessionSnapshotRecord): Pair {
  return {
    id: snapshot.pairId,
    name: snapshot.name,
    directory: snapshot.directory,
    createdAt: snapshot.createdAt,
    status: normalizePairStatus(snapshot.status) ?? 'Idle',
    iterations: snapshot.iterations,
    maxIterations: snapshot.maxIterations,
    cpuUsage: snapshot.cpuUsage,
    memUsage: snapshot.memUsage,
    spec: snapshot.spec,
    mentorProvider: snapshot.mentorProvider ?? inferProviderFromModel(snapshot.mentorModel),
    mentorModel: snapshot.mentorModel,
    executorProvider: snapshot.executorProvider ?? inferProviderFromModel(snapshot.executorModel),
    executorModel: snapshot.executorModel,
    pendingMentorModel: snapshot.pendingMentorModel,
    pendingExecutorModel: snapshot.pendingExecutorModel,
    mentorReasoningEffort: snapshot.mentorReasoningEffort,
    executorReasoningEffort: snapshot.executorReasoningEffort,
    messages: snapshot.messages,
    mentorActivity: snapshot.mentorActivity,
    executorActivity: snapshot.executorActivity,
    mentorCpu: snapshot.mentorCpu,
    mentorMemMb: snapshot.mentorMemMb,
    executorCpu: snapshot.executorCpu,
    executorMemMb: snapshot.executorMemMb,
    modifiedFiles: snapshot.modifiedFiles,
    gitTracking: {
      available: snapshot.gitTracking.available,
      rootPath: snapshot.gitTracking.rootPath
    },
    automationMode: snapshot.automationMode,
    planGate: snapshot.planGate,
    latestAcceptance: snapshot.latestAcceptance,
    turn: snapshot.turn,
    currentTurnCard: snapshot.currentTurnCard
      ? {
          ...snapshot.currentTurnCard,
          cognitiveEvents: snapshot.currentTurnCard.cognitiveEvents ?? []
        }
      : undefined,
    runCount: snapshot.runCount,
    runHistory: snapshot.runHistory,
    currentRunStartedAt: snapshot.currentRunStartedAt,
    currentRunFinishedAt: snapshot.currentRunFinishedAt,
    branch: snapshot.branch,
    repoPath: snapshot.repoPath,
    worktreePath: snapshot.worktreePath
  }
}

export const shouldSaveSnapshot = shouldSaveSnapshotImpl

async function saveSnapshotForPair(pair: Pair): Promise<void> {
  if (typeof window === 'undefined' || !window.api?.session?.saveSnapshot) return

  try {
    await window.api.session.saveSnapshot(snapshotPair(pair))
  } catch (error) {
    console.warn('[usePairStore] Failed to save session snapshot:', error)
  }
}

function buildTurnCardContent(
  activity: AgentActivity,
  fallbackLabel: string,
  role: 'mentor' | 'executor'
): string {
  const detail = activity.detail?.trim()
  if (detail) return sanitizeProgressDetail(detail, fallbackLabel, role)

  const label = activity.label.trim()
  if (label) return label

  return fallbackLabel
}

function stableTurnCardId(role: 'mentor' | 'executor', iteration: number): string {
  return `turn-${role}-iter${iteration}`
}

function createTurnCard(
  role: 'mentor' | 'executor',
  activity: AgentActivity,
  content: string,
  state: 'live' | 'final' = 'live',
  stableId?: string
): TurnCard {
  const now = Date.now()
  return {
    id: stableId ?? `turn-${role}-${now}-${Math.random().toString(36).slice(2, 8)}`,
    role,
    state,
    content,
    activity,
    startedAt: now,
    updatedAt: now,
    cognitiveEvents: []
  }
}

function castMessageType(
  type: string
): 'plan' | 'feedback' | 'progress' | 'result' | 'question' | 'handoff' | 'acceptance' {
  if (type === 'acceptance') return 'acceptance'
  return type as 'plan' | 'feedback' | 'progress' | 'result' | 'question' | 'handoff'
}

export function turnCardToMessage(card: TurnCard, iteration = 0): Message {
  const result = turnCardToMessageImpl(card as TokenUsageTurnCard, iteration) as TokenUsageMessage
  return {
    id: result.id,
    timestamp: result.timestamp,
    from: result.from,
    to: result.to as 'mentor' | 'executor' | 'both' | 'human',
    type: castMessageType(result.type),
    content: result.content,
    iteration: result.iteration,
    tokenUsage: result.tokenUsage,
    cognitiveEvents:
      (result.cognitiveEvents as CognitiveEvent[] | undefined) ?? card.cognitiveEvents,
    startedAt: result.startedAt ?? card.startedAt,
    finalizedAt: result.finalizedAt ?? card.finalizedAt ?? card.updatedAt
  }
}

export const syncTokenUsage = syncTokenUsageImpl

function commitTurnCard(messages: Message[], card?: TurnCard, iteration = 0): Message[] {
  if (!card) return messages
  return [...messages, turnCardToMessage({ ...card, state: 'final' }, iteration)]
}

function sanitizeProgressDetail(
  detail: string,
  fallback: string,
  role: 'mentor' | 'executor'
): string {
  const text = detail.trim()
  if (!text) return fallback

  const lower = text.toLowerCase()
  if (
    lower === 'step_start' ||
    lower === 'step_finish' ||
    lower === 'step_end' ||
    lower === 'tool' ||
    lower === 'progress' ||
    lower === 'respond' ||
    lower === 'final' ||
    lower === 'thinking' ||
    lower === 'using_tools'
  ) {
    return fallback
  }

  if (text === text.toUpperCase() && text.length <= 24) {
    return fallback
  }

  if (text.startsWith('{') || text.startsWith('[')) {
    return fallback
  }

  if (text.includes('step_start') || text.includes('step_finish') || text.includes('step_end')) {
    return fallback
  }

  return role === 'mentor'
    ? text.replace(/^Mentor[:：]\s*/i, '')
    : text.replace(/^Executor[:：]\s*/i, '')
}

function normalizePairStatus(raw: unknown): PairStatus | undefined {
  if (typeof raw !== 'string') return undefined
  const normalized = raw
    .trim()
    .toLowerCase()
    .replace(/[_\s]+/g, '-')

  switch (normalized) {
    case 'idle':
      return 'Idle'
    case 'mentoring':
      return 'Mentoring'
    case 'executing':
      return 'Executing'
    case 'reviewing':
      return 'Reviewing'
    case 'paused':
      return 'Paused'
    case 'awaiting-human-review':
      return 'Awaiting Human Review'
    case 'error':
      return 'Error'
    case 'finished':
      return 'Finished'
    default:
      return undefined
  }
}

function normalizeTurn(raw: unknown): 'mentor' | 'executor' | undefined {
  if (typeof raw !== 'string') return undefined
  const normalized = raw.trim().toLowerCase()
  if (normalized === 'mentor') return 'mentor'
  if (normalized === 'executor') return 'executor'
  return undefined
}

function createRunSummary(pair: Pair): PairRunSummary | null {
  if (!pair.spec.trim()) {
    return null
  }

  return {
    id: `${pair.id}-run-${pair.runCount}`,
    spec: pair.spec,
    status: pair.status,
    startedAt: pair.currentRunStartedAt,
    finishedAt: pair.currentRunFinishedAt ?? Date.now(),
    mentorModel: pair.mentorModel,
    executorModel: pair.executorModel,
    iterations: pair.iterations,
    messages: pair.messages,
    latestAcceptance: pair.latestAcceptance
  }
}

function resetPairForNewRun(
  pair: Pair,
  nextSpec: string,
  selection: PairModelSelection,
  maxIterations?: number
): Pair {
  const archivedRun = createRunSummary(pair)
  const now = Date.now()

  // Display only the bare task spec to the user. The backend (see
  // `build_mentor_planning_prompt` in src-tauri/src/util.rs) builds the actual
  // ROLE/PLAN wrapper when it sends the prompt to the mentor — keeping that
  // scaffolding out of the human message means we don't need to strip it later.
  const userMessage: Message = {
    id: Math.random().toString(36).substring(7),
    timestamp: now,
    from: 'human',
    to: 'mentor',
    type: 'plan',
    content: nextSpec,
    iteration: 0
  }

  return {
    ...pair,
    status: 'Idle',
    iterations: 0,
    maxIterations: maxIterations ?? pair.maxIterations,
    cpuUsage: 0,
    memUsage: 0,
    spec: nextSpec,
    mentorModel: selection.mentorModel,
    executorModel: selection.executorModel,
    pendingMentorModel: selection.pendingMentorModel,
    pendingExecutorModel: selection.pendingExecutorModel,
    messages: [userMessage],
    mentorActivity: createIdleActivity('Mentor idle'),
    executorActivity: createIdleActivity('Executor idle'),
    mentorCpu: 0,
    mentorMemMb: 0,
    executorCpu: 0,
    executorMemMb: 0,
    runCount: pair.runCount + 1,
    runHistory: archivedRun ? [...pair.runHistory, archivedRun] : pair.runHistory,
    currentRunStartedAt: now,
    currentRunFinishedAt: undefined,
    currentTurnCard: undefined
  }
}

function syncPairFromState(pair: Pair, state: PairStateSnapshot): Pair {
  const nextStatus = normalizePairStatus(state.status) ?? pair.status
  const nextTurn = normalizeTurn(state.turn) ?? pair.turn
  const nextMentorActivity = state.mentorActivity ?? pair.mentorActivity
  const nextExecutorActivity = state.executorActivity ?? pair.executorActivity
  const nextActiveActivity = nextTurn === 'mentor' ? nextMentorActivity : nextExecutorActivity
  const shouldHaveCurrentCard = isPairActive(nextStatus)
  const closedNow =
    pair.currentRunFinishedAt === undefined &&
    (nextStatus === 'Finished' || nextStatus === 'Error' || nextStatus === 'Paused') &&
    pair.status !== nextStatus

  let messages = pair.messages
  let currentTurnCard = pair.currentTurnCard

  if (state.messages && state.messages.length > 0) {
    // Merge: keep frontend messages that aren't in the backend (e.g. human mission card)
    // so that reset_session on the backend doesn't erase them from the UI.
    const backendIds = new Set(state.messages.map((m) => m.id))
    const preservedMessages = pair.messages.filter((m) => !backendIds.has(m.id))
    const existingTokenUsage = new Map(pair.messages.map((m) => [m.id, m.tokenUsage]))
    messages = [
      ...preservedMessages,
      ...state.messages.map((m) => ({
        ...m,
        tokenUsage: m.tokenUsage ?? existingTokenUsage.get(m.id)
      }))
    ].sort((a, b) => a.timestamp - b.timestamp)
  }

  if (currentTurnCard && currentTurnCard.role !== nextTurn) {
    currentTurnCard = undefined
  }

  if (shouldHaveCurrentCard) {
    const latestFromTurn = [...messages].reverse().find((m) => m.from === nextTurn)
    const hasFinalMessageFromTurnRole =
      latestFromTurn !== undefined &&
      (latestFromTurn.type === 'plan' ||
        latestFromTurn.type === 'result' ||
        latestFromTurn.type === 'acceptance')

    const nextContent = buildTurnCardContent(
      nextActiveActivity,
      nextTurn === 'mentor' ? 'Mentor working' : 'Executor working',
      nextTurn
    )
    const nextTokenUsage =
      nextTurn === 'mentor'
        ? resolveCurrentTurnTokenUsage(
            state.mentor?.tokenUsage,
            currentTurnCard?.tokenUsage,
            pair.mentorTokenUsage
          )
        : resolveCurrentTurnTokenUsage(
            state.executor?.tokenUsage,
            currentTurnCard?.tokenUsage,
            pair.executorTokenUsage
          )

    const incomingCognitiveEvents = (state.cognitiveEvents ?? []).filter((e) => e.role === nextTurn)

    if (hasFinalMessageFromTurnRole) {
      const activityPhase = nextActiveActivity.phase
      if (activityPhase === 'idle' || activityPhase === 'waiting') {
        currentTurnCard = undefined
      } else {
        if (!currentTurnCard) {
          currentTurnCard = createTurnCard(
            nextTurn,
            nextActiveActivity,
            nextContent,
            'live',
            stableTurnCardId(nextTurn, pair.iterations)
          )
          currentTurnCard.tokenUsage = nextTokenUsage
          currentTurnCard.cognitiveEvents = incomingCognitiveEvents
        } else if (currentTurnCard.role === nextTurn) {
          currentTurnCard = {
            ...currentTurnCard,
            activity: nextActiveActivity,
            content: currentTurnCard.state === 'live' ? nextContent : currentTurnCard.content,
            updatedAt: nextActiveActivity.updatedAt,
            tokenUsage: nextTokenUsage,
            cognitiveEvents:
              incomingCognitiveEvents.length > 0
                ? incomingCognitiveEvents
                : (currentTurnCard.cognitiveEvents ?? [])
          }
        }
      }
    } else {
      if (!currentTurnCard) {
        currentTurnCard = createTurnCard(
          nextTurn,
          nextActiveActivity,
          nextContent,
          'live',
          stableTurnCardId(nextTurn, pair.iterations)
        )
        currentTurnCard.tokenUsage = nextTokenUsage
        currentTurnCard.cognitiveEvents = incomingCognitiveEvents
      } else if (currentTurnCard.role === nextTurn) {
        currentTurnCard = {
          ...currentTurnCard,
          activity: nextActiveActivity,
          content: currentTurnCard.state === 'live' ? nextContent : currentTurnCard.content,
          updatedAt: nextActiveActivity.updatedAt,
          tokenUsage: nextTokenUsage,
          cognitiveEvents:
            incomingCognitiveEvents.length > 0
              ? incomingCognitiveEvents
              : (currentTurnCard.cognitiveEvents ?? [])
        }
      }
    }
  } else if (currentTurnCard) {
    currentTurnCard = undefined
  }

  return {
    ...pair,
    status: nextStatus,
    iterations: state.iteration ?? pair.iterations,
    turn: nextTurn,
    messages,
    currentTurnCard,
    mentorActivity: nextMentorActivity,
    executorActivity: nextExecutorActivity,
    mentorCpu: state.resources?.mentor?.cpu ?? pair.mentorCpu,
    mentorMemMb: state.resources?.mentor?.memMb ?? pair.mentorMemMb,
    executorCpu: state.resources?.executor?.cpu ?? pair.executorCpu,
    executorMemMb: state.resources?.executor?.memMb ?? pair.executorMemMb,
    cpuUsage: state.resources?.pairTotal?.cpu ?? pair.cpuUsage,
    memUsage: state.resources?.pairTotal?.memMb ?? pair.memUsage,
    modifiedFiles: state.modifiedFiles ?? pair.modifiedFiles,
    gitTracking: state.gitTracking ?? pair.gitTracking,
    automationMode: state.automationMode ?? pair.automationMode,
    latestAcceptance:
      state.latestAcceptance !== undefined ? state.latestAcceptance : pair.latestAcceptance,
    mentorTokenUsage: syncTokenUsage(state.mentor?.tokenUsage, pair.mentorTokenUsage),
    executorTokenUsage: syncTokenUsage(state.executor?.tokenUsage, pair.executorTokenUsage),
    currentRunFinishedAt: state.finishedAt ?? (closedNow ? Date.now() : pair.currentRunFinishedAt),
    turnStartedAt: state.turnStartedAt ?? pair.turnStartedAt
  }
}

function parseProgressUpdate(
  content: string,
  role: 'mentor' | 'executor'
): { detail: string; phase?: ActivityPhase } | null {
  const line = content.trim()
  if (!line || line === '{}' || line === '[]') return null

  const parseJson = (raw: string): Record<string, unknown> | null => {
    try {
      return JSON.parse(raw) as Record<string, unknown>
    } catch {
      return null
    }
  }

  const event =
    parseJson(line) ?? (line.startsWith('data:') ? parseJson(line.slice(5).trim()) : null)

  if (event) {
    const type = typeof event.type === 'string' ? event.type : ''
    const lowerType = type.toLowerCase()
    const part = (event.part as Record<string, unknown> | undefined) ?? {}
    const item = (event.item as Record<string, unknown> | undefined) ?? {}
    const partText = typeof part.text === 'string' ? part.text : ''
    const contentText = typeof event.content === 'string' ? event.content : ''
    const eventMessage = typeof event.message === 'string' ? event.message : ''
    const itemType = typeof item.type === 'string' ? item.type.toLowerCase() : ''
    const itemText =
      (typeof item.text === 'string' ? item.text : '') ||
      (typeof item.content === 'string' ? item.content : '') ||
      (typeof item.message === 'string' ? item.message : '')
    const toolName =
      typeof part.tool === 'string'
        ? part.tool
        : typeof (part.name as string | undefined) === 'string'
          ? (part.name as string)
          : ''
    // Build a meaningful action description for tool events
    const toolActionDetail = (() => {
      if (!toolName) return ''
      const inputObj = part.input as Record<string, unknown> | undefined
      const inputStr =
        typeof inputObj?.command === 'string'
          ? inputObj.command
          : typeof inputObj?.file_path === 'string'
            ? inputObj.file_path
            : typeof inputObj?.path === 'string'
              ? inputObj.path
              : ''
      const rawText = (inputStr || partText || '').split('\n')[0].trim()
      const snippet = rawText.length > 80 ? rawText.slice(0, 77) + '...' : rawText
      const lowerTool = toolName.toLowerCase()

      if (snippet && !snippet.startsWith('{')) {
        if (lowerTool === 'bash' || lowerTool === 'shell' || lowerTool.includes('exec'))
          return `Running: ${snippet}`
        if (['read', 'readfile'].some((n) => lowerTool.includes(n))) return `Reading: ${snippet}`
        if (['write', 'writefile', 'edit', 'apply', 'create'].some((n) => lowerTool.includes(n)))
          return `Editing: ${snippet}`
        if (
          lowerTool.includes('grep') ||
          lowerTool.includes('search') ||
          lowerTool.includes('glob')
        )
          return `Searching: ${snippet}`
        return `${toolName}: ${snippet}`
      }
      return `Using ${toolName}`
    })()

    const fallbackDetail =
      toolActionDetail ||
      (lowerType.includes('tool')
        ? 'Using tools'
        : lowerType.includes('step_start')
          ? 'Starting next step'
          : lowerType.includes('step_finish') || lowerType.includes('step_end')
            ? 'Step finished'
            : lowerType.includes('respond')
              ? 'Preparing response'
              : 'Working...')

    if (lowerType.includes('error') || itemType === 'error') {
      const errorDetail = itemText || eventMessage || contentText || 'Agent error'
      return {
        detail: sanitizeProgressDetail(errorDetail.slice(0, 260), 'Agent error', role),
        phase: 'error'
      }
    }

    if (lowerType.includes('tool') || itemType.includes('tool') || toolName) {
      return {
        detail: sanitizeProgressDetail(toolActionDetail || fallbackDetail, fallbackDetail, role),
        phase: 'using_tools'
      }
    }

    if (lowerType.includes('turn.started')) {
      return {
        detail: sanitizeProgressDetail(fallbackDetail, 'Starting turn', role),
        phase: 'thinking'
      }
    }

    if (lowerType.includes('step_start')) {
      return {
        detail: sanitizeProgressDetail(partText || fallbackDetail, fallbackDetail, role),
        phase: 'thinking'
      }
    }

    if (lowerType.includes('step_finish') || lowerType.includes('step_end')) {
      return {
        detail: sanitizeProgressDetail(partText || fallbackDetail, fallbackDetail, role),
        phase: 'responding'
      }
    }

    const text = partText || itemText || contentText || eventMessage
    if (text) {
      return {
        detail: sanitizeProgressDetail(text.slice(0, 260), fallbackDetail, role),
        phase: lowerType.includes('respond') ? 'responding' : 'thinking'
      }
    }

    return null
  }

  const normalized = line.replace(/^\[STDERR\]\s*/i, '').trim()
  if (!normalized) return null

  const lower = normalized.toLowerCase()
  let phase: ActivityPhase | undefined
  if (
    lower.includes('tool') ||
    lower.includes('apply_patch') ||
    lower.includes('bash') ||
    lower.includes('command')
  ) {
    phase = 'using_tools'
  } else if (lower.includes('respond') || lower.includes('final')) {
    phase = 'responding'
  } else if (lower.includes('step') || lower.includes('think') || lower.includes('plan')) {
    phase = 'thinking'
  }

  return {
    detail: normalized.slice(0, 260),
    phase
  }
}

export const usePairStore = create<PairStore>((set) => ({
  pairs: [],
  availableModels: [],
  isLoading: false,
  isLoadingModels: false,
  modelsLastUpdatedAt: null,
  modelsError: null,
  error: null,
  viewingRunId: null,
  restoringSpec: null,

  initMessageListener: () => {
    if (_listenersInitialized) return
    _listenersInitialized = true

    window.api.pair.onMessage((payload) => {
      const data = payload as PairMessageEvent
      const incoming = data.message

      if (incoming.type === 'progress') {
        const progress = parseProgressUpdate(
          incoming.content,
          incoming.from === 'mentor' ? 'mentor' : 'executor'
        )
        if (!progress) return

        set((state) => ({
          pairs: state.pairs.map((pair) => {
            if (pair.id !== data.pairId) return pair

            if (
              pair.status === 'Finished' ||
              pair.status === 'Paused' ||
              pair.status === 'Error' ||
              pair.status === 'Idle'
            ) {
              return pair
            }

            const role = incoming.from === 'mentor' ? 'mentor' : 'executor'
            const nextActivity =
              role === 'mentor'
                ? {
                    ...pair.mentorActivity,
                    phase: progress.phase ?? pair.mentorActivity.phase,
                    detail: progress.detail,
                    updatedAt: Date.now()
                  }
                : {
                    ...pair.executorActivity,
                    phase: progress.phase ?? pair.executorActivity.phase,
                    detail: progress.detail,
                    updatedAt: Date.now()
                  }

            let messages = pair.messages
            let currentTurnCard = pair.currentTurnCard

            if (currentTurnCard && currentTurnCard.role !== role) {
              messages = commitTurnCard(messages, currentTurnCard, pair.iterations)
              currentTurnCard = undefined
            }

            const nextContent =
              progress.detail || buildTurnCardContent(nextActivity, 'Working...', role)
            if (!currentTurnCard) {
              currentTurnCard = createTurnCard(
                role,
                nextActivity,
                nextContent,
                'live',
                stableTurnCardId(role, pair.iterations)
              )
            } else {
              currentTurnCard = {
                ...currentTurnCard,
                activity: nextActivity,
                content: nextContent,
                state: 'live',
                updatedAt: Date.now(),
                tokenUsage: currentTurnCard.tokenUsage
              }
            }

            return role === 'mentor'
              ? {
                  ...pair,
                  messages,
                  currentTurnCard,
                  mentorActivity: nextActivity
                }
              : {
                  ...pair,
                  messages,
                  currentTurnCard,
                  executorActivity: nextActivity
                }
          })
        }))
        return
      }

      if (incoming.type === 'handoff') {
        return
      }

      set((state) => ({
        pairs: state.pairs.map((p) =>
          p.id === data.pairId
            ? (() => {
                const role =
                  incoming.from === 'mentor'
                    ? 'mentor'
                    : incoming.from === 'executor'
                      ? 'executor'
                      : null
                if (!role) return p

                const nextActivity =
                  role === 'mentor'
                    ? {
                        ...p.mentorActivity,
                        detail: incoming.content.slice(0, 260),
                        updatedAt: Date.now()
                      }
                    : {
                        ...p.executorActivity,
                        detail: incoming.content.slice(0, 260),
                        updatedAt: Date.now()
                      }

                let messages = p.messages
                let currentTurnCard = p.currentTurnCard

                if (currentTurnCard) {
                  messages = commitTurnCard(messages, currentTurnCard, p.iterations)
                  currentTurnCard = undefined
                }

                const messageExists = messages.some((m) => m.id === incoming.id)
                if (!messageExists) {
                  messages = [
                    ...messages,
                    {
                      ...incoming,
                      type: castMessageType(incoming.type),
                      content:
                        incoming.content.trim() ||
                        buildTurnCardContent(nextActivity, 'Working...', role)
                    }
                  ]
                }

                return role === 'mentor'
                  ? {
                      ...p,
                      messages,
                      currentTurnCard,
                      mentorActivity: nextActivity
                    }
                  : {
                      ...p,
                      messages,
                      currentTurnCard,
                      executorActivity: nextActivity
                    }
              })()
            : p
        )
      }))

      const currentPair = usePairStore.getState().pairs.find((pair) => pair.id === data.pairId)
      if (currentPair) {
        void saveSnapshotForPair(currentPair)
      }
    })

    window.api.pair.onState((payload) => {
      const pairState = payload as PairStateSnapshot
      if (!pairState?.pairId) return

      let shouldSave = false
      let prevStatus: string | undefined
      let nextStatus: string | undefined

      set((state) => ({
        pairs: state.pairs.map((p) => {
          if (p.id !== pairState.pairId) return p
          prevStatus = p.status
          const nextPair = syncPairFromState(p, pairState)
          nextStatus = nextPair.status
          shouldSave = shouldSave || shouldSaveSnapshot(p, nextPair)
          return nextPair
        })
      }))

      const cue = resolvePairSoundCue({
        prevStatus: prevStatus as PairStatus | undefined,
        nextStatus: nextStatus as PairStatus | undefined,
        manualPauseAt: _manualPauseTimestamps.get(pairState.pairId) ?? null
      })
      dispatchPairSoundCue(cue)
      if (nextStatus && nextStatus !== 'Paused') {
        _manualPauseTimestamps.delete(pairState.pairId)
      }

      if (shouldSave) {
        const currentPair = usePairStore
          .getState()
          .pairs.find((pair) => pair.id === pairState.pairId)
        if (currentPair) {
          void saveSnapshotForPair(currentPair)
        }
      }
    })

    // Listen for handoff events to trigger next agent
    window.api.pair.onHandoff(async (payload) => {
      const data = payload as PairHandoffEvent

      const existing = _handoffLocks.get(data.pairId)
      if (existing) return
      const promise = (async () => {
        try {
          let backendState: BackendPairState | null = null
          try {
            backendState = (await window.api.pair.getState(data.pairId)) as BackendPairState | null
          } catch (error) {
            console.warn(
              '[usePairStore] Failed to load backend state before handoff processing',
              error
            )
          }

          if (backendState?.status === 'Finished') {
            return
          }

          if (backendState?.status === 'Paused') {
            return
          }

          const state = usePairStore.getState()
          const pair = state.pairs.find((p) => p.id === data.pairId)

          if (!pair) {
            console.warn('[usePairStore] Pair not found for handoff:', data.pairId)
            return
          }

          if (
            shouldIgnoreHandoffEvent({
              pairStatus: pair.status,
              backendStatus: backendState?.status
            })
          ) {
            return
          }

          let contextMessages = pair.messages
          try {
            if (backendState?.messages?.length) {
              contextMessages = backendState.messages
            }
          } catch (error) {
            console.warn(
              '[usePairStore] Failed to load backend state for handoff, falling back to local messages',
              error
            )
          }

          let message = ''
          const lastMentorMessage = [...contextMessages]
            .reverse()
            .find((m) => m.from === 'mentor' && (m.type === 'plan' || m.type === 'acceptance'))
          const lastExecutorMessage = [...contextMessages]
            .reverse()
            .find((m) => m.from === 'executor' && m.type === 'result')
          const latestAcceptance = backendState?.latestAcceptance ?? pair.latestAcceptance

          if (data.nextRole === 'executor') {
            if (latestAcceptance?.verdict?.nextStep.action === 'continue' && lastExecutorMessage) {
              message = buildExecutorAcceptanceFollowupPrompt({
                taskSpec: pair.spec,
                previousExecutorResult:
                  lastExecutorMessage?.content ?? '(previous executor result unavailable)',
                verdict: latestAcceptance.verdict,
                acceptance: latestAcceptance
              })

              const { assignTask } = state
              await assignTask(data.pairId, message, data.nextRole)
              return
            }

            message = buildInitialExecutorHandoffPrompt({
              mentorPlan: lastMentorMessage?.content
            })
          } else {
            if (latestAcceptance?.error && (latestAcceptance.repairAttempts ?? 0) > 0) {
              message = buildMentorAcceptanceRepairPrompt(latestAcceptance.error)

              const { assignTask } = state
              await assignTask(data.pairId, message, data.nextRole)
              return
            }

            if (latestAcceptance && lastExecutorMessage) {
              message = buildMentorAcceptancePrompt({
                taskSpec: pair.spec,
                executorResult: lastExecutorMessage.content,
                acceptance: latestAcceptance
              })

              const { assignTask } = state
              await assignTask(data.pairId, message, data.nextRole)
              return
            }

            message = buildInitialMentorReviewPrompt({
              executorOutput: lastExecutorMessage?.content
            })
          }

          const { assignTask } = state
          await assignTask(data.pairId, message, data.nextRole)
        } catch (error) {
          console.error('[usePairStore] Handoff processing failed:', error)
          const errorMessage = error instanceof Error ? error.message : String(error)
          set({ error: `Handoff failed: ${errorMessage}` })
          try {
            await window.api.pair.pause(data.pairId)
          } catch (pauseError) {
            console.error('[usePairStore] Failed to pause pair after handoff error:', pauseError)
          }
        }
      })()
      _handoffLocks.set(data.pairId, promise)
      promise.finally(() => _handoffLocks.delete(data.pairId))
    })
  },

  loadAvailableModels: async () => {
    if (_modelsLoading) return
    _modelsLoading = true
    set({ isLoadingModels: true, modelsError: null })
    try {
      const cachedModels = (await window.api.config.getCachedModels()) as AvailableModel[]
      if (cachedModels.length > 0) {
        set({ availableModels: cachedModels })
      }

      const models = (await window.api.config.refreshModels()) as AvailableModel[]
      set({
        availableModels: models,
        modelsLastUpdatedAt: Date.now(),
        modelsError: null
      })
    } catch (error) {
      console.error('Failed to load models:', error)
      set({ error: 'Failed to load models', modelsError: 'Failed to load models' })
    } finally {
      _modelsLoading = false
      set({ isLoadingModels: false })
    }
  },

  loadAllPairs: async () => {
    try {
      const snapshots = (await window.api.session.loadAllPairs()) as SessionSnapshotRecord[]
      const pairs = snapshots.map(snapshotToPair)
      set({ pairs })
    } catch (error) {
      console.error('Failed to load pairs:', error)
    }
  },

  flushSnapshots: async () => {
    const pairs = usePairStore.getState().pairs
    await Promise.all(pairs.map((pair) => saveSnapshotForPair(pair)))
  },

  createPair: async (input) => {
    set({ isLoading: true, error: null })

    try {
      const availableModels = usePairStore.getState().availableModels
      const mentorConfig = buildAgentConfig('mentor', input.mentorModel, availableModels)
      const executorConfig = buildAgentConfig('executor', input.executorModel, availableModels)
      const pairProcess = (await window.api.pair.create({
        name: input.name,
        directory: input.directory,
        spec: input.spec,
        mentor: mentorConfig,
        executor: executorConfig,
        mentorReasoningEffort: input.mentorReasoningEffort,
        executorReasoningEffort: input.executorReasoningEffort,
        branch: input.branch,
        maxIterations: input.maxIterations,
        planGate: input.planGate
      })) as PairCreatedResponse

      const now = Date.now()
      // Store only the user's task verbatim. The backend wraps it with
      // mentor instructions before sending it to the agent (see
      // build_mentor_planning_prompt in src-tauri/src/util.rs).
      const initialMessage: Message = {
        id: Math.random().toString(36).substring(7),
        timestamp: now,
        from: 'human',
        to: 'mentor',
        type: 'plan',
        content: input.spec,
        iteration: 0
      }

      const newPair: Pair = {
        id: pairProcess.pairId,
        name: input.name,
        directory: pairProcess.worktreePath || pairProcess.directory || input.directory,
        createdAt: now,
        status: 'Idle',
        iterations: 0,
        maxIterations: input.maxIterations ?? 0,
        cpuUsage: 0,
        memUsage: 0,
        spec: input.spec,
        mentorModel: input.mentorModel,
        mentorProvider: mentorConfig.provider,
        executorModel: input.executorModel,
        executorProvider: executorConfig.provider,
        messages: [initialMessage],
        mentorActivity: createIdleActivity('Mentor idle'),
        executorActivity: createIdleActivity('Executor idle'),
        mentorCpu: 0,
        mentorMemMb: 0,
        executorCpu: 0,
        executorMemMb: 0,
        modifiedFiles: [],
        gitTracking: { available: false },
        automationMode: 'full-auto',
        turn: 'mentor',
        runCount: 1,
        runHistory: [],
        currentRunStartedAt: now,
        currentTurnCard: undefined,
        branch: pairProcess.branch,
        repoPath: pairProcess.repoPath,
        worktreePath: pairProcess.worktreePath,
        planGate: input.planGate
      }

      set((state) => ({
        pairs: [...state.pairs, newPair],
        isLoading: false
      }))

      await saveSnapshotForPair(newPair)
    } catch (error: unknown) {
      console.error('[usePairStore] createPair error:', error)
      const message = extractErrorMessage(error, 'Failed to create pair')
      set({
        isLoading: false,
        error: message
      })
      throw error instanceof Error ? error : new Error(message)
    }
  },

  assignTask: async (
    pairId,
    spec,
    roleOrModelOverrides?,
    modelOverrides?: { mentorModel?: string; executorModel?: string },
    options?: { maxIterations?: number }
  ) => {
    let role: string | undefined
    let overrides: { mentorModel?: string; executorModel?: string } | undefined

    // Handle multiple calling conventions:
    // 1. Handoff: assignTask(pairId, spec, 'mentor') or assignTask(pairId, spec, 'executor', overrides)
    // 2. New task: assignTask(pairId, spec, undefined, overrides) or assignTask(pairId, spec, overrides)
    if (typeof roleOrModelOverrides === 'string') {
      // Handoff: first optional arg is a role string
      role = roleOrModelOverrides
      overrides = modelOverrides
    } else {
      // New task: first optional arg is overrides object (or undefined)
      // If both are provided (e.g., assignTask(id, spec, undefined, overrides)),
      // use the explicit fourth argument
      overrides = modelOverrides ?? roleOrModelOverrides
    }

    set({ isLoading: true, error: null })

    try {
      const currentPair = usePairStore.getState().pairs.find((pair) => pair.id === pairId)
      if (!currentPair) {
        throw new Error(`Pair ${pairId} not found`)
      }

      // For new runs (not handoffs), compute effective models
      let effectiveMentorModel = currentPair.mentorModel
      let effectiveExecutorModel = currentPair.executorModel

      if (!role) {
        // Compute effective models: override > pending > default
        const effective = resolveEffectiveModels(currentPair, overrides)
        effectiveMentorModel = effective.mentorModel
        effectiveExecutorModel = effective.executorModel

        // Only sync to backend when explicit overrides are provided.
        // Without overrides, the backend already has pending or default models.
        // This avoids unnecessary IPC and prevents partial state on failure.
        if (shouldSyncModelsToBackend(overrides)) {
          await window.api.pair.updateModels(
            pairId,
            buildUpdateModelsPayload(currentPair, effective)
          )
        }
      }

      await window.api.pair.assignTask(pairId, { spec, role })

      // Only update state AFTER backend succeeds
      set((state) => ({
        isLoading: false,
        pairs: state.pairs.map((pair) => {
          if (pair.id !== pairId) return pair

          // For handoffs, do NOT reset the pair - just pass through
          if (role) {
            return pair
          }

          // New run: apply effective models and reset
          return resetPairForNewRun(
            pair,
            spec,
            {
              mentorModel: effectiveMentorModel,
              executorModel: effectiveExecutorModel
            },
            options?.maxIterations
          )
        })
      }))

      // Only snapshot for new runs (not handoffs)
      if (!role) {
        const currentPair = usePairStore.getState().pairs.find((pair) => pair.id === pairId)
        if (currentPair) {
          await saveSnapshotForPair(currentPair)
        }
      }
    } catch (error) {
      console.error('[usePairStore] assignTask error:', error)
      const message = extractErrorMessage(error, 'Failed to assign task')
      set({
        isLoading: false,
        error: message
      })
      throw error instanceof Error ? error : new Error(message)
    }
  },

  updatePairModels: async (pairId, selection) => {
    set({ isLoading: true, error: null })

    try {
      const result = await window.api.pair.updateModels(pairId, selection)
      const typedResult = result as PairModelSelection
      const availableModels = usePairStore.getState().availableModels
      const mentorModelEntry = getModelByQualifiedId(availableModels, typedResult.mentorModel)
      const executorModelEntry = getModelByQualifiedId(availableModels, typedResult.executorModel)

      set((state) => ({
        isLoading: false,
        pairs: state.pairs.map((pair) =>
          pair.id === pairId
            ? {
                ...pair,
                mentorModel: typedResult.mentorModel,
                mentorProvider: mentorModelEntry?.provider ?? pair.mentorProvider,
                executorModel: typedResult.executorModel,
                executorProvider: executorModelEntry?.provider ?? pair.executorProvider,
                pendingMentorModel: typedResult.pendingMentorModel,
                pendingExecutorModel: typedResult.pendingExecutorModel,
                mentorReasoningEffort: typedResult.mentorReasoningEffort,
                executorReasoningEffort: typedResult.executorReasoningEffort
              }
            : pair
        )
      }))

      const currentPair = usePairStore.getState().pairs.find((pair) => pair.id === pairId)
      if (currentPair) {
        await saveSnapshotForPair(currentPair)
      }
    } catch (error) {
      const message = extractErrorMessage(error, 'Failed to update pair models')
      set({
        isLoading: false,
        error: message
      })
      throw error instanceof Error ? error : new Error(message)
    }
  },

  pausePair: async (id) => {
    set({ isLoading: true, error: null })
    _manualPauseTimestamps.set(id, Date.now())

    try {
      await window.api.pair.pause(id)
      playPauseConfirm()
      set({ isLoading: false })
    } catch (error) {
      _manualPauseTimestamps.delete(id)
      const message = extractErrorMessage(error, 'Failed to pause pair')
      set({
        isLoading: false,
        error: message
      })
      throw error instanceof Error ? error : new Error(message)
    }
  },

  resumePair: async (id) => {
    set({ isLoading: true, error: null })

    try {
      await window.api.pair.resume(id)
      set({ isLoading: false })
    } catch (error) {
      const message = extractErrorMessage(error, 'Failed to resume pair')
      set({
        isLoading: false,
        error: message
      })
      throw error instanceof Error ? error : new Error(message)
    }
  },

  resolvePlanReview: async (pairId, decision, feedback) => {
    // Resolve a gated plan (status "Awaiting Human Review"). Approve releases the
    // suppressed handoff to the executor; reject sends the plan back to the mentor
    // to revise with the human's feedback. Both reuse the assignTask handoff path,
    // which manages loading/error and starts the next turn without resetting state.
    const pair = usePairStore.getState().pairs.find((p) => p.id === pairId)
    if (!pair) return

    // Prefer the backend's authoritative message log (it may hold the full plan
    // when the local mirror is collapsed); fall back to local messages.
    let contextMessages = pair.messages
    try {
      const backendState = (await window.api.pair.getState(pairId)) as BackendPairState | null
      if (backendState?.messages?.length) {
        contextMessages = backendState.messages
      }
    } catch (error) {
      console.warn(
        '[usePairStore] Failed to load backend state for plan review, using local messages',
        error
      )
    }

    const lastMentorPlan = [...contextMessages]
      .reverse()
      .find((m) => m.from === 'mentor' && m.type === 'plan')?.content

    // Record the human's decision as a visible transcript message so the
    // conversation shows why the plan was approved or sent back. The feedback is
    // also threaded into the mentor's re-plan prompt below.
    const trimmedFeedback = feedback?.trim()
    const decisionContent =
      decision === 'approve'
        ? i18n.t('planReview.approvedMessage')
        : trimmedFeedback && trimmedFeedback.length > 0
          ? trimmedFeedback
          : i18n.t('planReview.sentBackMessage')

    usePairStore.getState().addMessage(pairId, {
      id: Math.random().toString(36).substring(7),
      timestamp: Date.now(),
      from: 'human',
      to: 'mentor',
      type: 'feedback',
      content: decisionContent,
      iteration: pair.iterations
    })

    const { assignTask } = usePairStore.getState()
    if (decision === 'approve') {
      await assignTask(
        pairId,
        buildInitialExecutorHandoffPrompt({ mentorPlan: lastMentorPlan }),
        'executor'
      )
    } else {
      await assignTask(
        pairId,
        buildPlanRevisionPrompt({
          taskSpec: pair.spec,
          previousPlan: lastMentorPlan,
          feedback
        }),
        'mentor'
      )
    }
  },

  deletePair: async (id) => {
    set({ isLoading: true, error: null })

    try {
      await window.api.pair.delete(id)
      set((state) => ({
        isLoading: false,
        pairs: state.pairs.filter((p) => p.id !== id)
      }))
    } catch (error) {
      const message = extractErrorMessage(error, 'Failed to delete pair')
      set({
        isLoading: false,
        error: message
      })
      throw error instanceof Error ? error : new Error(message)
    }
  },

  retryTurn: async (id) => {
    await window.api.pair.retryTurn(id)
  },

  killProcess: async (pairId, role) => {
    set({ isLoading: true, error: null })
    try {
      await window.api.pair.killProcess(pairId, role)
      set({ isLoading: false })
    } catch (error) {
      const message = extractErrorMessage(error, 'Failed to kill process')
      set({ isLoading: false, error: message })
      throw error instanceof Error ? error : new Error(message)
    }
  },

  updatePairStatus: (id, status) =>
    set((state) => ({
      pairs: state.pairs.map((p) => (p.id === id ? { ...p, status } : p))
    })),

  updatePairUsage: (id, cpu, mem) =>
    set((state) => ({
      pairs: state.pairs.map((p) => (p.id === id ? { ...p, cpuUsage: cpu, memUsage: mem } : p))
    })),

  addMessage: (pairId, message) =>
    set((state) => ({
      pairs: state.pairs.map((p) =>
        p.id === pairId ? { ...p, messages: [...p.messages, message] } : p
      )
    })),

  setMessages: (pairId, messages) =>
    set((state) => ({
      pairs: state.pairs.map((p) => (p.id === pairId ? { ...p, messages } : p))
    })),

  syncState: (pairId, status, iteration) =>
    set((state) => ({
      pairs: state.pairs.map((p) => (p.id === pairId ? { ...p, status, iterations: iteration } : p))
    })),

  syncFullState: (pairId, state) =>
    set((s) => ({
      pairs: s.pairs.map((p) =>
        p.id === pairId ? syncPairFromState(p, state as PairStateSnapshot) : p
      )
    })),

  viewTaskHistory: (pairId, runId) => {
    set({ viewingRunId: runId })
    void saveSnapshotForPair(usePairStore.getState().pairs.find((p) => p.id === pairId)!)
  },

  clearViewingTask: (pairId) => {
    set({ viewingRunId: null })
    void saveSnapshotForPair(usePairStore.getState().pairs.find((p) => p.id === pairId)!)
  },

  setViewingRunId: (runId) => {
    set({ viewingRunId: runId })
  },

  setRestoringSpec: (spec) => {
    set({ restoringSpec: spec })
  }
}))
