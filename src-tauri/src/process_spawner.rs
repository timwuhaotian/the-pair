use crate::acceptance::{
    canonical_acceptance_verdict_json, parse_review_verdict_with_quality, run_acceptance_checks,
    should_stop_iteration,
};
use crate::message_broker::MessageBroker;
use crate::provider_adapter::{OutputTransport, ProviderAdapter, ProviderTurnRequest};
use crate::provider_registry::{cli_environment_overrides, homedir, ProviderKind};
use crate::session_snapshot::persist_current_pair_snapshot;
use crate::types::{
    AcceptanceNextAction, AcceptanceNextStep, AcceptanceRecord, AcceptanceRisk, AcceptanceVerdict,
    AcceptanceVerdictDecision, ActivityPhase, AgentRole, Message, MessageSender, MessageType,
    PairStatus, TurnTokenUsage,
};
use crate::util::{is_mock_mode, now_millis};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// A provider CLI process that is currently running a turn. `turn_id` lets a
/// turn's reader tell its own child apart from a newer one registered under
/// the same `<pair>-<role>` key.
pub struct ActiveProcess {
    pub child: Child,
    pub turn_id: u64,
}

pub type ActiveProcessMap = Arc<Mutex<HashMap<String, ActiveProcess>>>;
pub type ProcessContextMap = Arc<Mutex<HashMap<String, ProcessContext>>>;

#[derive(Clone)]
pub struct ProcessSpawner {
    pub active_processes: ActiveProcessMap,
    pub pair_contexts: ProcessContextMap,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessContext {
    pub directory: String,
    pub mentor_provider: ProviderKind,
    pub executor_provider: ProviderKind,
    pub mentor_model: String,
    pub executor_model: String,
    pub mentor_session_id: Option<String>,
    pub executor_session_id: Option<String>,
    pub mentor_reasoning_effort: Option<String>,
    pub executor_reasoning_effort: Option<String>,
    /// Bumped whenever the current turn is invalidated: a new run, pause,
    /// kill, resume/retry and delete. A turn captures the value when it starts
    /// and does nothing to pair state once it no longer matches.
    pub run_generation: u32,
    pub is_smoke_test: bool,
}

const MENTOR_FINISH_SIGNAL: &str = "TASK_COMPLETE";

/// Absolute cap on `step_start` events in one turn. Real multi-file tasks run
/// well over a hundred model steps, so this only catches genuine runaways.
const MAX_STEP_CYCLES_PER_TURN: u32 = 1000;
/// A burst of this many steps inside `RUNAWAY_WINDOW_MS` is a loop, not work:
/// every real step waits on at least one model call.
const RUNAWAY_STEP_BURST: usize = 50;
const RUNAWAY_WINDOW_MS: u64 = 10_000;
const MIN_STEP_INTERVAL_MS: u64 = 50;

/// How long a killed provider gets to exit after SIGTERM before SIGKILL.
#[cfg(unix)]
const KILL_GRACE: Duration = Duration::from_secs(2);
/// How long to wait for a provider to exit after it closed stdout.
const EXIT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Stderr lines kept per turn for error reporting.
const STDERR_TAIL_LINES: usize = 40;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static NEXT_TURN_ID: AtomicU64 = AtomicU64::new(1);

/// Outcome of recording one `step_start` event within a turn.
enum StepCycleVerdict {
    Ok,
    /// Steps are cycling suspiciously fast — worth a warning log.
    Rapid {
        interval_ms: u64,
        count: u32,
    },
    /// The turn is looping (or exceeded the absolute cap) and must be terminated.
    Terminate {
        count: u32,
    },
}

/// Counts `step_start` events within a single turn so a runaway agent that
/// never stops cycling steps trips a kill switch. Must be fed every
/// `step_start` event in the stream — feeding only the first output line
/// (a previous bug) leaves the counter stuck at 1 and the guard inert.
///
/// The guard is rate-aware: a long but steady turn is fine, only a burst of
/// steps with no time for real model calls in between (or an absurd total)
/// terminates it.
struct StepCycleGuard {
    count: u32,
    last_step_timestamp: u64,
    recent: VecDeque<u64>,
}

impl StepCycleGuard {
    fn new() -> Self {
        Self {
            count: 0,
            last_step_timestamp: 0,
            recent: VecDeque::with_capacity(RUNAWAY_STEP_BURST),
        }
    }

    fn record_step(&mut self, now: u64) -> StepCycleVerdict {
        self.count += 1;
        let previous = self.last_step_timestamp;
        self.last_step_timestamp = now;

        self.recent.push_back(now);
        while self.recent.len() > RUNAWAY_STEP_BURST {
            self.recent.pop_front();
        }

        if self.count > MAX_STEP_CYCLES_PER_TURN {
            return StepCycleVerdict::Terminate { count: self.count };
        }
        if self.recent.len() == RUNAWAY_STEP_BURST {
            if let Some(oldest) = self.recent.front() {
                if now.saturating_sub(*oldest) < RUNAWAY_WINDOW_MS {
                    return StepCycleVerdict::Terminate { count: self.count };
                }
            }
        }
        if previous > 0 {
            let interval = now.saturating_sub(previous);
            if interval < MIN_STEP_INTERVAL_MS {
                return StepCycleVerdict::Rapid {
                    interval_ms: interval,
                    count: self.count,
                };
            }
        }
        StepCycleVerdict::Ok
    }
}

#[cfg(unix)]
mod process_group {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }

    pub const SIGTERM: i32 = 15;
    pub const SIGKILL: i32 = 9;

    /// Send `signal` to every process in the group led by `pgid`.
    pub fn signal(pgid: u32, signal: i32) -> bool {
        let Ok(pgid) = i32::try_from(pgid) else {
            return false;
        };
        // Never let a bogus id turn into kill(0)/kill(-1), which would hit our
        // own group or every process we may signal.
        if pgid <= 1 {
            return false;
        }
        // SAFETY: kill(2) takes plain integers and touches no memory; a
        // negative pid addresses the process group.
        unsafe { kill(-pgid, signal) == 0 }
    }
}

/// Terminate a provider CLI together with everything it started. Provider
/// CLIs are spawned as process-group leaders (unix) so npm launchers such as
/// codex's Node wrapper, the real binary behind them and any tool
/// subprocesses all go down together; on Windows `taskkill /T` walks the tree.
pub(crate) fn kill_process_tree(child: &mut Child) {
    let Some(pid) = child.id() else {
        // Already reaped.
        return;
    };

    #[cfg(unix)]
    {
        if process_group::signal(pid, process_group::SIGTERM) {
            std::thread::spawn(move || {
                std::thread::sleep(KILL_GRACE);
                process_group::signal(pid, process_group::SIGKILL);
            });
            return;
        }
        let _ = child.start_kill();
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        let _ = child.start_kill();
    }
}

/// Remove `key` from the process map only if it still holds this turn's child.
fn take_own_process(active: &ActiveProcessMap, key: &str, turn_id: u64) -> Option<ActiveProcess> {
    let mut guard = active.lock().unwrap_or_else(|e| e.into_inner());
    if guard.get(key).map(|process| process.turn_id) == Some(turn_id) {
        guard.remove(key)
    } else {
        None
    }
}

enum TurnExit {
    /// The child exited (or had to be killed after closing stdout); the
    /// status is `None` when it could not be determined.
    Exited(Option<std::process::ExitStatus>),
    /// Someone else (pause, kill, delete, a newer turn) took the child.
    Cancelled,
}

/// Wait for this turn's child to exit after its stdout closed, keeping it in
/// the process map (so pause/kill can still reach it) until it is reaped.
async fn await_turn_exit(active: &ActiveProcessMap, key: &str, turn_id: u64) -> TurnExit {
    let deadline = Instant::now() + EXIT_WAIT_TIMEOUT;
    loop {
        {
            let mut guard = active.lock().unwrap_or_else(|e| e.into_inner());
            let Some(process) = guard
                .get_mut(key)
                .filter(|process| process.turn_id == turn_id)
            else {
                return TurnExit::Cancelled;
            };
            match process.child.try_wait() {
                Ok(Some(status)) => {
                    guard.remove(key);
                    return TurnExit::Exited(Some(status));
                }
                Ok(None) if Instant::now() >= deadline => {
                    if let Some(mut process) = guard.remove(key) {
                        drop(guard);
                        println!(
                            "[ProcessSpawner] {} closed stdout but did not exit within {:?}; killing it",
                            key, EXIT_WAIT_TIMEOUT
                        );
                        kill_process_tree(&mut process.child);
                    }
                    return TurnExit::Exited(None);
                }
                Ok(None) => {}
                Err(_) => {
                    guard.remove(key);
                    return TurnExit::Exited(None);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Human-readable reason for a failed provider exit, or `None` on success.
/// Prefers the CLI's own `error: …` stderr line (Kimi, Kiro, …).
fn exit_failure_detail(
    success: bool,
    code: Option<i32>,
    signal: Option<i32>,
    stderr_tail: &[String],
) -> Option<String> {
    if success {
        return None;
    }
    let how = match (code, signal) {
        (Some(code), _) => format!("exited with code {}", code),
        (None, Some(signal)) => format!("was terminated by signal {}", signal),
        (None, None) => "exited abnormally".to_string(),
    };

    if let Some(line) = stderr_tail
        .iter()
        .rev()
        .map(|line| line.trim())
        .find(|line| line.to_ascii_lowercase().starts_with("error:"))
    {
        return Some(format!("{} (process {})", line, how));
    }

    let tail: Vec<&str> = stderr_tail
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    let tail = tail[tail.len().saturating_sub(5)..].join(" | ");
    if tail.is_empty() {
        Some(format!("process {}", how))
    } else {
        let tail: String = tail.chars().take(600).collect();
        Some(format!("process {}: {}", how, tail))
    }
}

fn exit_status_failure(
    status: &std::process::ExitStatus,
    stderr_tail: &[String],
) -> Option<String> {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    exit_failure_detail(status.success(), status.code(), signal, stderr_tail)
}

/// Deletes codex's `--output-last-message` temp file however the turn ends.
struct RemoveFileOnDrop(Option<PathBuf>);

impl Drop for RemoveFileOnDrop {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Strip the line terminator and decode lossily, so one invalid UTF-8 byte
/// never ends the stream.
fn decode_output_line(buf: &[u8]) -> String {
    let mut end = buf.len();
    while end > 0 && (buf[end - 1] == b'\n' || buf[end - 1] == b'\r') {
        end -= 1;
    }
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

fn mock_responses(role: &str, iteration: u32) -> Vec<String> {
    let scenario = std::env::var("THE_PAIR_E2E_MOCK_SCENARIO").unwrap_or_default();
    mock_responses_for_scenario(role, iteration, &scenario)
}

fn mock_responses_for_scenario(role: &str, iteration: u32, scenario: &str) -> Vec<String> {
    if scenario == "dev-smoke" {
        return mock_dev_smoke_responses(role, iteration);
    }

    match (role, iteration) {
        ("mentor", 1) => vec![
            "I'll analyze the task and create a plan.".to_string(),
            "## Plan\n1. Read the existing code\n2. Implement the changes\n3. Verify the result"
                .to_string(),
        ],
        ("mentor", _) => vec![
            "The implementation looks correct.".to_string(),
            "All requirements are met.\n\nTASK_COMPLETE".to_string(),
        ],
        ("executor", _) => vec![
            "Reading the relevant files...".to_string(),
            "Implementing the changes now.".to_string(),
            "Done. All changes applied successfully.".to_string(),
        ],
        _ => vec!["Processing...".to_string()],
    }
}

fn mock_dev_smoke_verdict(greeting: u32) -> String {
    // Each greeting is correct on its own (verdict "pass"); the loop keeps going via
    // action "continue" until the final greeting, which finishes. This mirrors a real
    // multi-step review where a good intermediate step is pass + continue rather than
    // a misleading "fail".
    let (verdict, action, instructions) = if greeting >= 3 {
        ("pass", "finish", Vec::<String>::new())
    } else {
        (
            "pass",
            "continue",
            vec![format!("Send Greeting {}/3", greeting + 1)],
        )
    };

    let payload = serde_json::json!({
        "verdict": verdict,
        "risk": "low",
        "confidence": if greeting >= 3 { 1.0 } else { 0.95 },
        "issues": [],
        "evidence": [format!("Executor sent Greeting {}/3", greeting)],
        "reasoning": format!("Greeting {}/3 received by deterministic mock provider.", greeting),
        "summary": format!("Greeting {}/3 received.", greeting),
        "nextStep": {
            "action": action,
            "instructions": instructions
        }
    });

    let mut response = format!(
        "Greeting {}/3 received.\n{}",
        greeting,
        serde_json::to_string(&payload).unwrap()
    );
    if greeting >= 3 {
        response.push_str("\nTASK_COMPLETE");
    }
    response
}

fn mock_dev_smoke_responses(role: &str, iteration: u32) -> Vec<String> {
    match role {
        "mentor" if iteration <= 1 => vec!["Send Greeting 1/3".to_string()],
        "mentor" => vec![mock_dev_smoke_verdict(iteration.saturating_sub(1).min(3))],
        "executor" => vec![format!("Greeting {}/3", iteration.clamp(1, 3))],
        _ => vec!["Processing...".to_string()],
    }
}

fn mock_error_response() -> Vec<String> {
    vec![
        "Attempting to execute the command...".to_string(),
        "Error: Permission denied while writing to file.".to_string(),
    ]
}

fn mock_token_usage() -> crate::types::TurnTokenUsage {
    crate::types::TurnTokenUsage {
        output_tokens: 42,
        input_tokens: Some(100),
        last_updated_at: now_millis(),
        source: crate::types::TokenUsageSource::Final,
        provider: Some("mock".to_string()),
    }
}

fn apply_provider_cli_env(command: &mut Command) {
    for (key, value) in cli_environment_overrides(&homedir()) {
        command.env(key, value);
    }
}

fn parse_json_event(line: &str) -> Option<serde_json::Value> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        return Some(v);
    }

    if let Some(stripped) = line.strip_prefix("data:") {
        return serde_json::from_str::<serde_json::Value>(stripped.trim()).ok();
    }

    None
}

fn extract_session_id(event: &serde_json::Value) -> Option<String> {
    // Muse (`muse exec --json`) tags every envelope with the stream it belongs
    // to; the session-kind stream id is exactly what `--session-id` takes to
    // resume. No other provider emits a `stream` object, so this cannot
    // shadow their ids.
    if let Some(stream) = event.get("stream") {
        if stream.get("kind").and_then(|k| k.as_str()) == Some("session") {
            if let Some(id) = stream.get("id").and_then(|s| s.as_str()) {
                if !id.trim().is_empty() {
                    return Some(id.to_string());
                }
            }
        }
    }
    // Antigravity (`agy --output-format stream-json`) opens every turn with
    // {"event":"init","conversation_id":"…"}; `--conversation <id>` resumes it.
    if event.get("event").and_then(|v| v.as_str()) == Some("init") {
        if let Some(id) = event.get("conversation_id").and_then(|s| s.as_str()) {
            if !id.trim().is_empty() {
                return Some(id.to_string());
            }
        }
    }
    // Pi session header: {"type":"session","version":3,"id":"uuid",...}
    if event.get("type").and_then(|v| v.as_str()) == Some("session") {
        if let Some(id) = event.get("id").and_then(|s| s.as_str()) {
            return Some(id.to_string());
        }
    }
    event
        .get("sessionID")
        .and_then(|s| s.as_str())
        .or_else(|| event.get("session_id").and_then(|s| s.as_str()))
        // Grok Build (`grok --output-format streaming-json`) reports the
        // resumable session as camelCase `sessionId` on the terminal `end`
        // event; `grok --resume <id>` takes exactly this value.
        .or_else(|| event.get("sessionId").and_then(|s| s.as_str()))
        // Codex (`codex exec --json`) exposes its resumable session as
        // `thread_id` on the `thread.started` event; `codex exec resume <id>`
        // takes exactly this value.
        .or_else(|| event.get("thread_id").and_then(|s| s.as_str()))
        .or_else(|| {
            event
                .get("part")
                .and_then(|p| p.get("sessionID"))
                .and_then(|s| s.as_str())
        })
        .or_else(|| {
            event
                .get("part")
                .and_then(|p| p.get("session_id"))
                .and_then(|s| s.as_str())
        })
        .map(|s| s.to_string())
}

fn push_trimmed(out: &mut Vec<String>, s: &str) {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return;
    }

    if out.last().map(|last| last == trimmed).unwrap_or(false) {
        return;
    }

    out.push(trimmed.to_string());
}

fn collect_text_candidates(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => push_trimmed(out, s),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_text_candidates(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for key in [
                "text",
                "content",
                "message",
                "delta",
                "part",
                "parts",
                "output_text",
                "response",
                "output",
            ] {
                if let Some(v) = map.get(key) {
                    collect_text_candidates(v, out);
                }
            }
        }
        _ => {}
    }
}

fn extract_event_texts(event: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_text_candidates(event, &mut out);
    out
}

fn extract_token_usage(
    provider_kind: ProviderKind,
    event: &serde_json::Value,
) -> Option<TurnTokenUsage> {
    crate::providers::provider_for_kind(provider_kind)?.extract_token_usage(event)
}

fn collect_json_candidates_for_provider(
    provider_kind: ProviderKind,
    event: &serde_json::Value,
    out: &mut Vec<String>,
) {
    let Some(provider) = crate::providers::provider_for_kind(provider_kind) else {
        return;
    };

    // Provider-specific extraction (Claude, Gemini override the default).
    if let Some(candidates) = provider.collect_json_candidates(event) {
        for text in candidates {
            push_trimmed(out, &text);
        }
        return;
    }

    // Generic extraction with noise filtering.
    let event_type = event
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if is_noise_event_type_for_final(event_type) {
        return;
    }

    let texts = extract_event_texts(event);
    for text in texts {
        if !is_noise_text_candidate(&text) {
            push_trimmed(out, &text);
        }
    }
}

/// Merge the text fragments a JSON stream produced into one reply. A later
/// fragment that extends the previous one (a cumulative stream snapshot)
/// replaces it; distinct fragments are joined in order. Nothing is ever
/// dropped for being shorter — picking the longest fragment used to throw
/// away a short final answer (and its TASK_COMPLETE) in favour of narration.
fn collapse_candidates(candidates: &[String]) -> Option<String> {
    let mut kept: Vec<&str> = Vec::new();
    for candidate in candidates {
        let candidate = candidate.as_str();
        if let Some(last) = kept.last().copied() {
            if candidate.starts_with(last) {
                kept.pop();
            } else if last.starts_with(candidate) {
                continue;
            }
        }
        kept.push(candidate);
    }

    if kept.is_empty() {
        None
    } else {
        Some(kept.join("\n"))
    }
}

/// OpenCode reports every model step as its own assistant message
/// (`part.messageID`). Text from a step that ended in `tool-calls` is
/// narration; the reply is the text of the last step that finished for
/// another reason (normally `stop`).
#[derive(Default)]
struct OpencodeStepTexts {
    order: Vec<String>,
    /// messageID -> (partID, text) in arrival order; a re-sent part replaces
    /// its earlier text.
    texts: HashMap<String, Vec<(String, String)>>,
    finish_reasons: HashMap<String, String>,
}

impl OpencodeStepTexts {
    fn observe(&mut self, event: &serde_json::Value) {
        let Some(part) = event.get("part") else {
            return;
        };
        let Some(message_id) = part.get("messageID").and_then(|v| v.as_str()) else {
            return;
        };
        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");

        if !self.order.iter().any(|id| id == message_id) {
            self.order.push(message_id.to_string());
        }

        if event_type == "text" || part_type == "text" {
            let Some(text) = part.get("text").and_then(|v| v.as_str()) else {
                return;
            };
            let part_id = part
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let entries = self.texts.entry(message_id.to_string()).or_default();
            match entries
                .iter_mut()
                .find(|(id, _)| !part_id.is_empty() && *id == part_id)
            {
                Some(entry) => entry.1 = text.to_string(),
                None => entries.push((part_id, text.to_string())),
            }
        } else if matches!(event_type, "step_finish" | "step-finish")
            || matches!(part_type, "step-finish" | "step_finish")
        {
            let reason = part
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("stop")
                .to_string();
            self.finish_reasons.insert(message_id.to_string(), reason);
        }
    }

    fn final_answer(&self) -> Option<String> {
        let message_id = self.order.iter().rev().find(|id| {
            self.finish_reasons
                .get(*id)
                .is_some_and(|reason| reason != "tool-calls")
        })?;
        let text = self
            .texts
            .get(message_id)?
            .iter()
            .map(|(_, text)| text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        (!text.is_empty()).then_some(text)
    }
}

/// Sum per-step usage into the turn total for providers that report usage
/// per model step (OpenCode `step_finish`); other providers report running or
/// final totals, which simply replace the previous value.
fn accumulate_token_usage(
    provider_kind: ProviderKind,
    event: &serde_json::Value,
    previous: Option<&TurnTokenUsage>,
    usage: TurnTokenUsage,
) -> TurnTokenUsage {
    let per_step = provider_kind == ProviderKind::Opencode
        && (matches!(
            event.get("type").and_then(|v| v.as_str()),
            Some("step_finish" | "step-finish")
        ) || matches!(
            event
                .get("part")
                .and_then(|part| part.get("type"))
                .and_then(|v| v.as_str()),
            Some("step-finish" | "step_finish")
        ));
    match (per_step, previous) {
        (true, Some(previous)) => TurnTokenUsage {
            output_tokens: previous.output_tokens + usage.output_tokens,
            input_tokens: match (previous.input_tokens, usage.input_tokens) {
                (Some(a), Some(b)) => Some(a + b),
                (a, b) => a.or(b),
            },
            ..usage
        },
        _ => usage,
    }
}

/// Collects what a turn printed and decides which part is the reply.
struct TurnOutputCollector {
    provider_kind: ProviderKind,
    parses_json_events: bool,
    json_candidates: Vec<String>,
    /// The provider's own final answer (e.g. Claude's `result`), which wins
    /// over everything accumulated from intermediate events.
    authoritative_final: Option<String>,
    opencode_steps: OpencodeStepTexts,
    plain_output: String,
}

impl TurnOutputCollector {
    fn new(provider_kind: ProviderKind, parses_json_events: bool) -> Self {
        Self {
            provider_kind,
            parses_json_events,
            json_candidates: Vec::new(),
            authoritative_final: None,
            opencode_steps: OpencodeStepTexts::default(),
            plain_output: String::new(),
        }
    }

    fn observe_json(&mut self, event: &serde_json::Value) {
        collect_json_candidates_for_provider(self.provider_kind, event, &mut self.json_candidates);
        if let Some(provider) = crate::providers::provider_for_kind(self.provider_kind) {
            if let Some(text) = provider
                .final_output_text(event)
                .filter(|text| !text.trim().is_empty())
            {
                self.authoritative_final = Some(text.trim().to_string());
            }
        }
        if self.provider_kind == ProviderKind::Opencode {
            self.opencode_steps.observe(event);
        }
    }

    fn observe_plain_line(&mut self, line: &str) {
        if self.parses_json_events {
            // Stray non-JSON lines from a JSON provider are only a fallback;
            // keep the noise filter for them.
            if should_skip_plain_output_line(line) {
                return;
            }
        }
        // For plain-text providers (aider, kiro) this text *is* the reply, so
        // keep it verbatim — including `{`, `}` lines and blank lines —
        // otherwise pretty-printed JSON verdicts and code lose their braces.
        if !self.plain_output.is_empty() {
            self.plain_output.push('\n');
        }
        self.plain_output.push_str(line);
    }

    /// Returns the reply text and whether it came from a terminal success
    /// event (codex's last-message file, Claude's `result`, OpenCode's final
    /// step).
    fn finish(&self, last_message_file: Option<String>) -> (String, bool) {
        if let Some(text) = last_message_file.filter(|text| !text.trim().is_empty()) {
            return (text, true);
        }
        if let Some(text) = self.authoritative_final.clone() {
            return (text, true);
        }
        if let Some(text) = self.opencode_steps.final_answer() {
            return (text, true);
        }
        let text = collapse_candidates(&self.json_candidates)
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| self.plain_output.trim().to_string());
        (text, false)
    }
}

fn is_noise_event_type_for_final(event_type: &str) -> bool {
    if event_type.is_empty() {
        return false;
    }

    let lower = event_type.to_ascii_lowercase();
    lower.contains("thread.started")
        || lower.contains("turn.started")
        || lower.contains("step_start")
        || lower.contains("step_finish")
        || lower.contains("step_end")
        || (lower.contains("tool") && !lower.contains("tool_result"))
        || lower.contains("progress")
        || lower.contains("error")
        || lower.contains("warning")
        || lower.contains("log")
}

fn is_noise_text_candidate(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }

    let lower = trimmed.to_ascii_lowercase();
    let punctuation_only = trimmed.chars().all(|ch| {
        matches!(
            ch,
            '{' | '}' | '[' | ']' | '(' | ')' | ',' | '.' | ':' | ';' | '"' | '\''
        )
    });

    punctuation_only
        || lower.starts_with("reconnecting...")
        || lower.contains("stream disconnected before completion")
        || lower.contains("failed to lookup address information")
        || lower.contains("falling back from websockets")
}

fn should_skip_plain_output_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || is_noise_text_candidate(trimmed)
}

struct AcceptanceVerdictOutcome {
    parsed_acceptance: Option<AcceptanceRecord>,
    acceptance_error: Option<String>,
    stored_output: String,
}

fn process_mentor_review_verdict(
    existing_acceptance: Option<AcceptanceRecord>,
    raw_output: &str,
    stored_output: String,
    fallback_iteration: u32,
) -> AcceptanceVerdictOutcome {
    // Normally the executor's check record for this iteration. Without one
    // (checks skipped, or a snapshot restored without it) start a fresh record
    // so the verdict still parses and repair attempts are still counted —
    // otherwise every reply failed with "missing record" and the repair loop
    // never reached its pause.
    let now = now_millis();
    let mut acceptance = existing_acceptance.unwrap_or(AcceptanceRecord {
        iteration: fallback_iteration,
        risk: AcceptanceRisk::Low,
        checks: Vec::new(),
        summary: "No automated checks were run".to_string(),
        started_at: now,
        finished_at: now,
        verdict: None,
        raw_verdict: None,
        error: None,
        repair_attempts: 0,
    });

    match parse_review_verdict_with_quality(raw_output) {
        Ok(verdict) => {
            let output = canonical_acceptance_verdict_json(&verdict);
            acceptance.raw_verdict = Some(raw_output.trim().to_string());
            acceptance.verdict = Some(verdict);
            acceptance.error = None;
            AcceptanceVerdictOutcome {
                parsed_acceptance: Some(acceptance),
                acceptance_error: None,
                stored_output: output,
            }
        }
        Err(error) => {
            // A verdict left over from an earlier attempt must not be acted on
            // as if this reply had produced it.
            acceptance.verdict = None;
            acceptance.error = Some(error.clone());
            acceptance.raw_verdict = Some(raw_output.trim().to_string());
            acceptance.repair_attempts += 1;
            AcceptanceVerdictOutcome {
                parsed_acceptance: Some(acceptance),
                acceptance_error: Some(error),
                stored_output,
            }
        }
    }
}

fn is_dev_smoke_pair_spec(spec: &str) -> bool {
    spec.contains("This is a smoke test of the pair execution loop")
        && spec.contains("Each time the executor sends a greeting")
        && spec.contains("Greeting N/3 received.")
}

fn is_dev_smoke_greeting_output(output: &str) -> bool {
    let trimmed = output.trim();
    if matches!(trimmed, "Greeting 1/3" | "Greeting 2/3" | "Greeting 3/3") {
        return true;
    }
    // Accept paraphrased forms ("Send Greeting 1/3", "Greeting 1/3 received",
    // "Acknowledged: Greeting 2/3", etc.). Real executor CLIs sometimes echo
    // the mentor's instruction text instead of emitting the bare greeting, and
    // we don't want that to drag the run through code-quality checks.
    let lower = trimmed.to_lowercase();
    ["greeting 1/3", "greeting 2/3", "greeting 3/3"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn has_dev_smoke_pair_spec(messages: &[crate::types::Message]) -> bool {
    messages.iter().any(|message| {
        matches!(&message.from, crate::types::MessageSender::Human)
            && message.to == "mentor"
            && is_dev_smoke_pair_spec(&message.content)
    })
}

fn should_skip_executor_acceptance_for_dev_smoke(
    messages: &[crate::types::Message],
    final_output: &str,
) -> bool {
    is_dev_smoke_greeting_output(final_output) && has_dev_smoke_pair_spec(messages)
}

fn should_allow_mentor_finish_before_review_for_dev_smoke(
    messages: &[crate::types::Message],
    final_output: &str,
) -> bool {
    has_signal_token_on_own_line(final_output, MENTOR_FINISH_SIGNAL)
        && has_dev_smoke_pair_spec(messages)
}

fn count_mentor_greeting_confirmations(
    acceptance_history: &[crate::types::AcceptanceRecord],
) -> u32 {
    let mut max_greeting: u32 = 0;
    for record in acceptance_history {
        if let Some(raw) = &record.raw_verdict {
            max_greeting = max_greeting.max(extract_greeting_number_from_text(raw));
        }
    }
    max_greeting
}

fn count_executor_greetings(messages: &[crate::types::Message]) -> u32 {
    messages
        .iter()
        .filter(|m| matches!(m.from, crate::types::MessageSender::Executor))
        .filter(|m| {
            let content = m.content.trim().to_lowercase();
            is_dev_smoke_greeting_output(m.content.trim())
                || content.contains("greeting 1")
                || content.contains("greeting 2")
                || content.contains("greeting 3")
                || content.contains("greeting1")
                || content.contains("greeting2")
                || content.contains("greeting3")
        })
        .count() as u32
}

fn extract_greeting_number_from_text(text: &str) -> u32 {
    let lower = text.to_lowercase();
    let mut max_n: u32 = 0;
    for line in lower.lines() {
        if let Some(pos) = line.find("greeting") {
            let after = &line[pos + "greeting".len()..];
            let num_str: String = after
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = num_str.parse::<u32>() {
                max_n = max_n.max(n);
            }
        }
    }
    max_n
}

fn dev_smoke_needs_more_greetings(
    messages: &[crate::types::Message],
    acceptance_history: &[crate::types::AcceptanceRecord],
) -> bool {
    if !has_dev_smoke_pair_spec(messages) {
        return false;
    }
    let mentor_count = count_mentor_greeting_confirmations(acceptance_history);
    let executor_count = count_executor_greetings(messages);
    let confirmed = mentor_count.max(executor_count);
    confirmed < 3
}

fn has_signal_token_on_own_line(content: &str, token: &str) -> bool {
    let upper_token = token.to_ascii_uppercase();

    content.lines().any(|line| {
        let normalized = line
            .trim()
            .trim_matches(|c: char| {
                c.is_whitespace()
                    || c == '`'
                    || c == '"'
                    || c == '\''
                    || c == '*'
                    || c == '_'
                    || c == '#'
                    || c == '-'
                    || c == ':'
                    || c == '.'
                    || c == ','
                    || c == '!'
                    || c == '?'
                    || c == '['
                    || c == ']'
                    || c == '('
                    || c == ')'
                    || c == '{'
                    || c == '}'
            })
            .to_ascii_uppercase();
        normalized == upper_token
    })
}

fn mentor_finish_signal_is_actionable(
    role: &str,
    is_mentor_review_turn: bool,
    content: &str,
) -> bool {
    role == "mentor"
        && is_mentor_review_turn
        && has_signal_token_on_own_line(content, MENTOR_FINISH_SIGNAL)
}

// ── End-of-turn decision (shared by the real and mock paths) ─────────────

/// What should happen to the pair once a turn's output is in.
#[derive(Debug, Clone, PartialEq)]
enum TurnDecision {
    /// Hand the next turn to `next_role` via a `pair:handoff` event.
    Handoff {
        next_role: &'static str,
    },
    /// The mentor's plan is ready and the pair has the plan gate on.
    AwaitHumanReview,
    /// The run is complete.
    Finish {
        detail: String,
    },
    /// Stop for a human. `hand_to_mentor` makes Resume start with a mentor
    /// review; `ends_run` marks automatic stops that are recorded as run
    /// outcomes.
    Pause {
        detail: String,
        hand_to_mentor: bool,
        ends_run: bool,
    },
    Error {
        detail: String,
    },
}

impl TurnDecision {
    fn is_terminal_outcome(&self) -> bool {
        matches!(
            self,
            TurnDecision::Finish { .. }
                | TurnDecision::Error { .. }
                | TurnDecision::Pause { ends_run: true, .. }
        )
    }
}

/// The facts `decide_turn_outcome` needs, gathered once at the end of a turn.
struct TurnFacts<'a> {
    role: &'a str,
    is_review_turn: bool,
    output: &'a str,
    no_text_output: bool,
    turn_error: Option<&'a str>,
    /// The mentor's parsed verdict (review turns, when it parsed).
    verdict: Option<&'a AcceptanceVerdict>,
    /// Why the mentor's verdict failed to parse (review turns).
    verdict_error: Option<&'a str>,
    repair_attempts: u32,
    iteration: u32,
    max_iterations: u32,
    plan_gate_enabled: bool,
    /// Dev-smoke guard: the smoke run still needs greetings before finishing.
    smoke_needs_more: bool,
    /// Dev-smoke shortcut: accept a finish signal on a planning turn.
    smoke_finish_before_review: bool,
}

/// A reply that tried to include a JSON verdict (and so must parse) rather
/// than being plain prose.
fn attempts_json_verdict(output: &str) -> bool {
    output.contains('{') && output.contains('}')
}

fn decide_turn_outcome(facts: &TurnFacts<'_>) -> TurnDecision {
    if let Some(error) = facts.turn_error {
        return TurnDecision::Error {
            detail: format!("{} error: {}", facts.role, error),
        };
    }

    if facts.no_text_output {
        // Must come before the review branch: a content-free review carries no
        // verdict, and handing off would send the executor a placeholder.
        return TurnDecision::Pause {
            detail: format!("{} returned no textual output", facts.role),
            hand_to_mentor: false,
            ends_run: true,
        };
    }

    if facts.role == "mentor" && facts.is_review_turn {
        let finish_signaled =
            mentor_finish_signal_is_actionable(facts.role, facts.is_review_turn, facts.output);

        // The verdict decides; TASK_COMPLETE only counts when the verdict
        // agrees with it (or when the reply has no JSON verdict at all).
        let finish = match facts.verdict {
            Some(verdict) => {
                should_stop_iteration(verdict)
                    || (finish_signaled
                        && matches!(verdict.verdict, AcceptanceVerdictDecision::Pass)
                        && matches!(verdict.next_step.action, AcceptanceNextAction::Finish))
            }
            None => finish_signaled && !attempts_json_verdict(facts.output),
        };

        if finish {
            if facts.smoke_needs_more {
                return TurnDecision::Handoff {
                    next_role: "executor",
                };
            }
            let detail = match facts.verdict {
                Some(verdict) => format!(
                    "Task completed with {:.0}% confidence",
                    verdict.confidence * 100.0
                ),
                None => format!("Mentor signaled {} after review", MENTOR_FINISH_SIGNAL),
            };
            return TurnDecision::Finish { detail };
        }

        if facts.verdict.is_some() {
            // continue (or an unexpected finish the mentor didn't confirm):
            // the executor works through the instructions.
            return TurnDecision::Handoff {
                next_role: "executor",
            };
        }

        if let Some(error) = facts.verdict_error {
            if facts.repair_attempts > 1 {
                return TurnDecision::Pause {
                    detail: format!("Acceptance verdict parse failed: {}", error),
                    hand_to_mentor: false,
                    ends_run: true,
                };
            }
            // Ask the mentor to restate its verdict.
            return TurnDecision::Handoff {
                next_role: "mentor",
            };
        }

        return TurnDecision::Handoff {
            next_role: "executor",
        };
    }

    if facts.role == "mentor" {
        // Planning turn. TASK_COMPLETE before the executor ever ran is ignored
        // (except for the dev-smoke shortcut), and the iteration budget does
        // not apply: a plan is not an iteration of work.
        if facts.smoke_finish_before_review
            && has_signal_token_on_own_line(facts.output, MENTOR_FINISH_SIGNAL)
        {
            return TurnDecision::Finish {
                detail: format!(
                    "Mentor signaled {} for dev smoke test",
                    MENTOR_FINISH_SIGNAL
                ),
            };
        }
        if crate::smart_pause::should_gate_plan(
            facts.role,
            facts.is_review_turn,
            facts.plan_gate_enabled,
            true,
        ) {
            return TurnDecision::AwaitHumanReview;
        }
        return TurnDecision::Handoff {
            next_role: "executor",
        };
    }

    // Executor turn. 0 = unlimited budget.
    if facts.max_iterations > 0 && facts.iteration >= facts.max_iterations {
        let reason = crate::smart_pause::PauseReason::BudgetExhausted {
            iterations: facts.iteration,
            budget: facts.max_iterations,
        };
        // The executor's work is done but unreviewed: Resume must start with
        // the mentor's review, not re-run the executor into the same wall.
        return TurnDecision::Pause {
            detail: crate::smart_pause::format_pause_message(&reason),
            hand_to_mentor: true,
            ends_run: true,
        };
    }

    TurnDecision::Handoff {
        next_role: "mentor",
    }
}

/// Acceptance record for a review that finished on a bare TASK_COMPLETE (no
/// JSON verdict): the mentor accepted the work, so record it as a pass.
fn synthesize_finish_acceptance(
    existing: Option<AcceptanceRecord>,
    raw_output: &str,
    fallback_iteration: u32,
) -> AcceptanceRecord {
    let now = now_millis();
    let mut record = existing.unwrap_or(AcceptanceRecord {
        iteration: fallback_iteration,
        risk: AcceptanceRisk::Low,
        checks: Vec::new(),
        summary: "No automated checks were run".to_string(),
        started_at: now,
        finished_at: now,
        verdict: None,
        raw_verdict: None,
        error: None,
        repair_attempts: 0,
    });
    let summary: String = raw_output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !has_signal_token_on_own_line(line, MENTOR_FINISH_SIGNAL))
        .unwrap_or("Mentor signaled TASK_COMPLETE")
        .chars()
        .take(200)
        .collect();
    record.verdict = Some(AcceptanceVerdict {
        verdict: AcceptanceVerdictDecision::Pass,
        risk: record.risk.clone(),
        confidence: 1.0,
        issues: Vec::new(),
        evidence: Vec::new(),
        reasoning: format!(
            "The mentor signaled {} without a structured verdict.",
            MENTOR_FINISH_SIGNAL
        ),
        summary,
        next_step: AcceptanceNextStep {
            action: AcceptanceNextAction::Finish,
            instructions: Vec::new(),
        },
    });
    record.raw_verdict = Some(raw_output.trim().to_string());
    record.error = None;
    record
}

fn role_matches(turn: &AgentRole, role: &str) -> bool {
    match turn {
        AgentRole::Mentor => role == "mentor",
        AgentRole::Executor => role == "executor",
    }
}

/// Identity of one running turn, shared by the real and mock paths. Every
/// state change a turn makes after its output ends goes through
/// `if_current`, so a paused, killed, deleted or superseded turn can never
/// touch the pair again.
#[derive(Clone)]
struct TurnContext {
    app: tauri::AppHandle,
    contexts: ProcessContextMap,
    pair_id: String,
    role: String,
    run_generation: u32,
    current_iteration: u32,
    is_review_turn: bool,
}

impl TurnContext {
    fn generation_is_current(&self) -> bool {
        self.contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&self.pair_id)
            .map(|context| context.run_generation == self.run_generation)
            .unwrap_or(false)
    }

    /// Lock order: MessageBroker, then pair_contexts (same as pair_create).
    fn is_current_locked(&self, broker: &MessageBroker) -> bool {
        if !self.generation_is_current() {
            return false;
        }
        match broker.status_and_turn(&self.pair_id) {
            Some((status, turn)) => status.is_active() && role_matches(&turn, &self.role),
            None => false,
        }
    }

    fn with_broker<R>(&self, f: impl FnOnce(&MessageBroker) -> R) -> Option<R> {
        let broker = self.app.try_state::<Mutex<MessageBroker>>()?;
        let guard = broker.lock().unwrap_or_else(|e| e.into_inner());
        Some(f(&guard))
    }

    /// Run `f` under the broker lock only while this turn is still the live
    /// turn of its pair; returns `None` (doing nothing) for a stale turn.
    fn if_current<R>(&self, f: impl FnOnce(&MessageBroker) -> R) -> Option<R> {
        let broker = self.app.try_state::<Mutex<MessageBroker>>()?;
        let guard = broker.lock().unwrap_or_else(|e| e.into_inner());
        if !self.is_current_locked(&guard) {
            return None;
        }
        Some(f(&guard))
    }

    fn log_stale(&self, stage: &str) {
        println!(
            "[ProcessSpawner] [{}] {} turn is stale ({}); leaving pair state untouched",
            self.pair_id, self.role, stage
        );
    }
}

/// The text a finished turn produced.
struct TurnOutput {
    final_output: String,
    no_text_output: bool,
    turn_error: Option<String>,
    token_usage: Option<TurnTokenUsage>,
}

/// Build the executor's acceptance record: refresh the modified-file list
/// from git right now (the monitor's copy can be up to 5 s old, or empty when
/// the executor committed its work) and run the checks.
async fn run_executor_acceptance(tc: &TurnContext, final_output: &str) -> Option<AcceptanceRecord> {
    let state = tc
        .with_broker(|broker| broker.get_state(&tc.pair_id))
        .flatten()?;
    if should_skip_executor_acceptance_for_dev_smoke(&state.messages, final_output) {
        return None;
    }

    let refreshed = tauri::async_runtime::spawn_blocking(move || {
        let mut state = state;
        crate::git_tracker::GitTracker::update_state(&mut state);
        state
    })
    .await
    .ok()?;
    tc.if_current(|broker| {
        broker.set_modified_files(&tc.pair_id, refreshed.modified_files.clone());
    })?;

    // Race the checks against the turn being invalidated: pause, kill,
    // delete or a new run cancel them (dropping the future stops the check
    // processes) instead of letting build/test commands run on.
    let checks = run_acceptance_checks(
        std::path::Path::new(&refreshed.directory),
        &refreshed.modified_files,
        final_output,
        refreshed.iteration,
        refreshed.max_iterations,
    );
    tokio::pin!(checks);
    loop {
        tokio::select! {
            record = &mut checks => return Some(record),
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if !tc.generation_is_current() {
                    tc.log_stale("acceptance checks cancelled");
                    return None;
                }
            }
        }
    }
}

fn apply_turn_decision(tc: &TurnContext, broker: &MessageBroker, decision: &TurnDecision) {
    match decision {
        TurnDecision::Handoff { next_role } => {
            println!(
                "[ProcessSpawner] [{}] Triggering handoff to {}",
                tc.pair_id, next_role
            );
            let _ = tc.app.emit(
                "pair:handoff",
                serde_json::json!({
                    "pairId": tc.pair_id,
                    "nextRole": next_role
                }),
            );
        }
        TurnDecision::AwaitHumanReview => {
            println!(
                "[ProcessSpawner] [{}] Plan gate engaged — awaiting human review before executor",
                tc.pair_id
            );
            broker.set_pair_status(
                &tc.pair_id,
                PairStatus::AwaitingHumanReview,
                Some("Plan ready — review it before the executor starts.".to_string()),
            );
        }
        TurnDecision::Finish { detail } => {
            println!("[ProcessSpawner] [{}] Finished: {}", tc.pair_id, detail);
            broker.set_pair_status(&tc.pair_id, PairStatus::Finished, Some(detail.clone()));
        }
        TurnDecision::Pause {
            detail,
            hand_to_mentor,
            ..
        } => {
            println!("[ProcessSpawner] [{}] Pausing: {}", tc.pair_id, detail);
            if *hand_to_mentor {
                broker.set_turn(&tc.pair_id, AgentRole::Mentor);
            }
            broker.set_pair_status(&tc.pair_id, PairStatus::Paused, Some(detail.clone()));
        }
        TurnDecision::Error { detail } => {
            println!("[ProcessSpawner] [{}] Error: {}", tc.pair_id, detail);
            broker.set_pair_status(&tc.pair_id, PairStatus::Error, Some(detail.clone()));
        }
    }
}

/// Everything that happens after the turn's state changes, outside every
/// lock: persist the snapshot, and for run-ending outcomes write the session
/// report (successful finish) and record the run for insights.
async fn finish_turn_bookkeeping(tc: &TurnContext, decision: &TurnDecision) {
    let app = tc.app.clone();
    let pair_id = tc.pair_id.clone();
    let decision = decision.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = persist_current_pair_snapshot(&app, &pair_id) {
            println!(
                "[ProcessSpawner] [{}] Failed to persist snapshot: {}",
                pair_id, error
            );
        }
        if decision.is_terminal_outcome() {
            record_run_outcome(
                &app,
                &pair_id,
                matches!(decision, TurnDecision::Finish { .. }),
            );
        }
    })
    .await;
}

/// Write the session report (on a successful finish) and the insights run
/// record. Takes PairManager and MessageBroker one at a time, never nested:
/// the turn path must not hold the broker while locking the manager
/// (pair_create/resume lock them in the opposite order).
fn record_run_outcome(app: &tauri::AppHandle, pair_id: &str, finished: bool) {
    if is_mock_mode() {
        // e2e smoke runs must never pollute real reports or stats.
        return;
    }

    let pair = app
        .try_state::<Mutex<crate::pair_manager::PairManager>>()
        .and_then(|manager| {
            manager
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_pair(pair_id)
        });
    let Some(pair) = pair else {
        return;
    };
    let state = app.try_state::<Mutex<MessageBroker>>().and_then(|broker| {
        broker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_state(pair_id)
    });
    let Some(state) = state else {
        return;
    };
    let context = app.try_state::<ProcessSpawner>().and_then(|spawner| {
        spawner
            .pair_contexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(pair_id)
            .cloned()
    });

    let finished_at = now_millis();
    let started_at = state
        .run_started_at
        .or_else(|| state.messages.first().map(|message| message.timestamp))
        .unwrap_or(finished_at);

    if finished {
        write_session_report(app, &pair, &state, started_at);
    }

    let (mentor_model, executor_model, mentor_provider) = match context.as_ref() {
        Some(context) => (
            context.mentor_model.clone(),
            context.executor_model.clone(),
            context.mentor_provider,
        ),
        None => (
            pair.mentor_model.clone(),
            pair.executor_model.clone(),
            pair.mentor_provider,
        ),
    };
    let mut record = crate::intelligence_store::build_run_record(
        pair_id,
        started_at,
        finished_at,
        &mentor_model,
        &executor_model,
        &format!("{:?}", mentor_provider).to_lowercase(),
        &state.task_spec,
        &state,
    );
    // Only a finished run counts as a success; a run that stopped on an error
    // or an automatic pause is a rejection when the mentor last failed the
    // work, and verdict-less otherwise.
    record.verdict = if finished {
        Some("accept".to_string())
    } else {
        match state
            .latest_acceptance
            .as_ref()
            .and_then(|acceptance| acceptance.verdict.as_ref())
            .map(|verdict| &verdict.verdict)
        {
            Some(AcceptanceVerdictDecision::Fail) => Some("reject".to_string()),
            _ => None,
        }
    };
    crate::intelligence_store::record_run_safely(app, &record);
}

fn write_session_report(
    app: &tauri::AppHandle,
    pair: &crate::types::Pair,
    state: &crate::types::PairState,
    started_at: u64,
) {
    let fallback_verdict = state
        .latest_acceptance
        .as_ref()
        .and_then(|acceptance| acceptance.verdict.clone());
    let result = crate::report_generator::generate_session_report_with_fallback(
        &pair.pair_id,
        &pair.name,
        &state.task_spec,
        started_at,
        &state.acceptance_history,
        &state.modified_files,
        &state.messages,
        fallback_verdict,
    )
    .and_then(|report| {
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
        crate::report_generator::save_report_to_file(&report, &base)
    });

    match result {
        Ok(report_path) => {
            println!(
                "[ProcessSpawner] [{}] Session report saved to {:?}",
                pair.pair_id, report_path
            );
            let _ = app.emit(
                "pair:report_generated",
                serde_json::json!({
                    "pairId": pair.pair_id,
                    "reportPath": report_path.to_string_lossy().to_string()
                }),
            );
        }
        Err(error) => {
            println!(
                "[ProcessSpawner] [{}] Failed to write session report: {}",
                pair.pair_id, error
            );
            let _ = app.emit(
                "pair:report_failed",
                serde_json::json!({
                    "pairId": pair.pair_id,
                    "error": error
                }),
            );
        }
    }
}

/// End-of-turn processing shared by the real and mock paths: decide whether
/// the turn still owns the pair, record its reply, run the executor's
/// acceptance checks, then finish, pause, gate or hand off.
async fn complete_turn(tc: TurnContext, out: TurnOutput) {
    let role = tc.role.as_str();

    // Decide staleness once, before touching anything: a paused, killed,
    // deleted or superseded turn leaves the pair exactly as the human left it.
    let state = tc
        .with_broker(|broker| {
            if tc.is_current_locked(broker) {
                broker.get_state(&tc.pair_id)
            } else {
                None
            }
        })
        .flatten();
    let Some(state) = state else {
        tc.log_stale("output ended");
        return;
    };

    if role == "executor" && has_signal_token_on_own_line(&out.final_output, MENTOR_FINISH_SIGNAL) {
        println!(
            "[ProcessSpawner] [{}] WARNING: Executor attempted TASK_COMPLETE (ignored - only Mentor can finish)",
            tc.pair_id
        );
    }

    let is_review = role == "mentor" && tc.is_review_turn;
    let (mut stored_output, mut parsed_acceptance, acceptance_error) =
        if is_review && !out.no_text_output && out.turn_error.is_none() {
            let outcome = process_mentor_review_verdict(
                state.latest_acceptance.clone(),
                &out.final_output,
                out.final_output.clone(),
                state.iteration.saturating_sub(1),
            );
            (
                outcome.stored_output,
                outcome.parsed_acceptance,
                outcome.acceptance_error,
            )
        } else {
            (out.final_output.clone(), None, None)
        };
    let verdict = if acceptance_error.is_none() {
        parsed_acceptance
            .as_ref()
            .and_then(|acceptance| acceptance.verdict.clone())
    } else {
        None
    };

    let mut smoke_history = state.acceptance_history.clone();
    if let Some(acceptance) = parsed_acceptance.as_ref() {
        smoke_history.push(acceptance.clone());
    }
    let smoke_needs_more = dev_smoke_needs_more_greetings(&state.messages, &smoke_history);
    let facts = TurnFacts {
        role,
        is_review_turn: is_review,
        output: &out.final_output,
        no_text_output: out.no_text_output,
        turn_error: out.turn_error.as_deref(),
        verdict: verdict.as_ref(),
        verdict_error: acceptance_error.as_deref(),
        repair_attempts: parsed_acceptance
            .as_ref()
            .map(|acceptance| acceptance.repair_attempts)
            .unwrap_or(0),
        iteration: state.iteration,
        max_iterations: state.max_iterations,
        plan_gate_enabled: state.plan_gate,
        smoke_needs_more,
        smoke_finish_before_review: should_allow_mentor_finish_before_review_for_dev_smoke(
            &state.messages,
            &out.final_output,
        ) && !smoke_needs_more,
    };
    let decision = decide_turn_outcome(&facts);

    if is_review && verdict.is_none() && matches!(decision, TurnDecision::Finish { .. }) {
        // Finished on a bare TASK_COMPLETE: record the acceptance as a pass
        // instead of a parse error so the UI, report and insights agree.
        parsed_acceptance = Some(synthesize_finish_acceptance(
            state.latest_acceptance.clone(),
            &out.final_output,
            state.iteration.saturating_sub(1),
        ));
        stored_output = out.final_output.clone();
    }

    let message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: now_millis(),
        from: if role == "mentor" {
            MessageSender::Mentor
        } else {
            MessageSender::Executor
        },
        to: "human".to_string(),
        msg_type: if role == "mentor" {
            if is_review {
                MessageType::Acceptance
            } else {
                MessageType::Plan
            }
        } else {
            MessageType::Result
        },
        content: stored_output.clone(),
        iteration: tc.current_iteration,
        token_usage: out.token_usage.clone(),
        attachments: None,
        cognitive_events: None,
        started_at: None,
        finalized_at: None,
    };

    let recorded = tc.if_current(|broker| {
        broker.add_message(&tc.pair_id, message);
        if let Some(acceptance) = parsed_acceptance.clone() {
            broker.set_latest_acceptance(&tc.pair_id, Some(acceptance));
        }
        if role == "mentor" {
            let checklist = crate::context_bridge::parse_checklist(&stored_output);
            if !checklist.is_empty() {
                broker.set_plan_checklist(&tc.pair_id, checklist);
            }
        }
    });
    if recorded.is_none() {
        tc.log_stale("before recording the reply");
        return;
    }

    if role == "executor" && !out.no_text_output && out.turn_error.is_none() {
        if let Some(acceptance) = run_executor_acceptance(&tc, &out.final_output).await {
            // A pause during the (possibly long) checks invalidated this turn:
            // don't record the checks or flip the pair to Reviewing.
            let applied = tc.if_current(|broker| {
                broker.set_latest_acceptance(&tc.pair_id, Some(acceptance.clone()));
                broker.set_pair_status(
                    &tc.pair_id,
                    PairStatus::Reviewing,
                    Some(format!(
                        "Acceptance checks complete. {}",
                        acceptance.summary
                    )),
                );
            });
            if applied.is_none() {
                tc.log_stale("after acceptance checks");
                return;
            }
        }
    }

    let applied = tc.if_current(|broker| {
        broker.update_agent_activity(
            &tc.pair_id,
            role,
            ActivityPhase::Idle,
            "Turn finished".to_string(),
            None,
        );
        apply_turn_decision(&tc, broker, &decision);
    });
    if applied.is_none() {
        tc.log_stale("before the handoff");
        return;
    }

    finish_turn_bookkeeping(&tc, &decision).await;
}

/// Mock mode: replay canned output, then run exactly the same end-of-turn
/// logic as a real turn (plan gate, pause/staleness checks, verdicts).
async fn run_mock_turn(tc: TurnContext) {
    let role = tc.role.clone();
    let pair_id = tc.pair_id.clone();

    tc.with_broker(|broker| {
        broker.reset_token_usage(&pair_id, &role);
        broker.update_agent_activity(
            &pair_id,
            &role,
            ActivityPhase::Thinking,
            "Starting process...".to_string(),
            None,
        );
        broker.set_turn_started_at(&pair_id, now_millis());
        broker.update_agent_activity(
            &pair_id,
            &role,
            ActivityPhase::Responding,
            "Processing response".to_string(),
            None,
        );
    });

    let mock_scenario =
        std::env::var("THE_PAIR_E2E_MOCK_SCENARIO").unwrap_or_else(|_| "success".to_string());
    let is_error = mock_scenario == "error" && role == "executor";
    let responses = if is_error {
        mock_error_response()
    } else {
        mock_responses(&role, tc.current_iteration)
    };

    tc.with_broker(|broker| {
        for line in &responses {
            broker.add_log_line(&pair_id, &role, line);
            broker.update_output_progress(&pair_id, &role);
        }
        broker.update_token_usage(&pair_id, &role, mock_token_usage());
    });

    let turn_error = is_error.then(|| "Mock: Permission denied".to_string());
    let mut final_output = responses.join("\n");
    if let Some(detail) = turn_error.as_ref() {
        final_output = format!("{}\n\n[error] {}", final_output, detail);
    }

    complete_turn(
        tc,
        TurnOutput {
            final_output,
            no_text_output: false,
            turn_error,
            token_usage: Some(mock_token_usage()),
        },
    )
    .await;
}

// ── Launching the provider CLI ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandLinePlatform {
    Linux,
    MacOs,
    Windows,
    Other,
}

impl CommandLinePlatform {
    fn current() -> Self {
        if cfg!(target_os = "linux") {
            CommandLinePlatform::Linux
        } else if cfg!(target_os = "macos") {
            CommandLinePlatform::MacOs
        } else if cfg!(windows) {
            CommandLinePlatform::Windows
        } else {
            CommandLinePlatform::Other
        }
    }
}

/// The prompt travels as one argv element. Refuse up front, with an
/// actionable message, when it cannot fit the OS limits (spawn would fail
/// with an opaque E2BIG / "command line too long" otherwise).
fn command_line_limit_error(
    platform: CommandLinePlatform,
    program: &str,
    args: &[String],
    env_bytes: usize,
) -> Option<String> {
    let largest = args.iter().map(|arg| arg.len()).max().unwrap_or(0);
    let (too_long, limit_bytes, label) = match platform {
        // MAX_ARG_STRLEN: a single argument may not exceed 32 pages.
        CommandLinePlatform::Linux => (largest + 1 > 131_072, 131_072, "Linux"),
        // ARG_MAX covers argv and the environment together.
        CommandLinePlatform::MacOs => {
            let total: usize = std::iter::once(program)
                .chain(args.iter().map(String::as_str))
                .map(|arg| arg.len() + 1)
                .sum::<usize>()
                + env_bytes;
            (total > 1_048_576 - 4_096, 1_048_576, "macOS")
        }
        // CreateProcess: 32,767 UTF-16 units for the whole command line,
        // including quoting.
        CommandLinePlatform::Windows => {
            let units: usize = std::iter::once(program)
                .chain(args.iter().map(String::as_str))
                .map(|arg| {
                    arg.encode_utf16().count()
                        + 3
                        + arg.chars().filter(|c| matches!(c, '"' | '\\')).count()
                })
                .sum();
            (units > 32_767, 32_767 * 2, "Windows")
        }
        CommandLinePlatform::Other => (false, 0, ""),
    };
    if !too_long {
        return None;
    }
    Some(format!(
        "The prompt for this turn is {} KB, which exceeds the {} command-line limit (about {} KB). Remove attached files or shorten the task so the prompt fits, then try again.",
        largest.div_ceil(1024),
        label,
        limit_bytes / 1024
    ))
}

/// What an npm `.cmd` shim actually runs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NpmShimTarget {
    /// `node <script>` (the script is a JS entry point).
    Node { script: PathBuf },
    /// A native executable the shim forwards to.
    Executable { program: PathBuf },
}

/// Parse a standard npm cmd-shim (`cmd-shim`, both the current `%dp0%` and
/// the legacy `%~dp0` layouts) to find what it launches. Returns `None` for
/// anything that isn't recognisably an npm shim.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_npm_cmd_shim(content: &str, shim_dir: &std::path::Path) -> Option<NpmShimTarget> {
    let line = content.lines().rev().find(|line| line.contains("%*"))?;
    let before_args = &line[..line.rfind("%*")?];

    let quoted: Vec<&str> = before_args
        .split('"')
        .enumerate()
        .filter_map(|(index, segment)| (index % 2 == 1).then_some(segment))
        .collect();
    let target = quoted.iter().rev().find(|segment| {
        let lower = segment.to_ascii_lowercase();
        (lower.contains("%dp0%") || lower.contains("%~dp0"))
            && !lower.ends_with("\\node.exe")
            && !lower.ends_with("/node.exe")
    })?;

    let relative = target
        .replace("%~dp0", "")
        .replace("%dp0%", "")
        .replace("%DP0%", "");
    let relative = relative.trim_start_matches(['\\', '/']);
    if relative.is_empty() {
        return None;
    }
    let path = shim_dir.join(relative);

    let lower = relative.to_ascii_lowercase();
    if lower.ends_with(".exe") {
        return Some(NpmShimTarget::Executable { program: path });
    }
    if content.to_ascii_lowercase().contains("node") {
        return Some(NpmShimTarget::Node { script: path });
    }
    None
}

/// Resolve how to start `executable` with `args`. On Windows, npm-installed
/// CLIs are `.cmd` batch shims: Rust refuses to pass arguments containing
/// newlines to batch files (every prompt has them), so the shim is unwrapped
/// into `node <script>` / the native binary it forwards to.
fn resolve_launch(executable: &str, args: Vec<String>) -> Result<(PathBuf, Vec<String>), String> {
    let resolved = crate::provider_registry::which_binary(executable)
        .unwrap_or_else(|| PathBuf::from(executable));

    #[cfg(windows)]
    {
        let resolved = windows_launchable(resolved);
        let extension = resolved
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .unwrap_or_default();
        if extension == "cmd" || extension == "bat" {
            let shim_dir = resolved
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            let content = std::fs::metadata(&resolved)
                .ok()
                .filter(|meta| meta.len() <= 64 * 1024)
                .and_then(|_| std::fs::read(&resolved).ok())
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
            match content.and_then(|content| parse_npm_cmd_shim(&content, &shim_dir)) {
                Some(NpmShimTarget::Executable { program }) if program.exists() => {
                    return Ok((program, args));
                }
                Some(NpmShimTarget::Node { script }) if script.exists() => {
                    let local_node = shim_dir.join("node.exe");
                    let node = if local_node.exists() {
                        local_node
                    } else {
                        crate::provider_registry::which_binary("node")
                            .map(windows_launchable)
                            .unwrap_or_else(|| PathBuf::from("node.exe"))
                    };
                    let mut node_args = vec![script.to_string_lossy().into_owned()];
                    node_args.extend(args);
                    return Ok((node, node_args));
                }
                _ => {}
            }
            if args
                .iter()
                .any(|arg| arg.contains('\n') || arg.contains('\r'))
            {
                return Err(format!(
                    "Cannot launch {}: it is a Windows batch launcher, which cannot receive multi-line prompts. Reinstall the CLI with npm (so The Pair can run its Node entry point directly) or install its native .exe build.",
                    resolved.display()
                ));
            }
        }
        return Ok((resolved, args));
    }

    #[cfg(not(windows))]
    Ok((resolved, args))
}

/// Prefer a runnable sibling when a lookup returned an extensionless npm
/// shell script (which Windows cannot execute).
#[cfg(windows)]
fn windows_launchable(path: PathBuf) -> PathBuf {
    if path.extension().is_some() {
        return path;
    }
    for extension in ["exe", "cmd", "bat"] {
        let candidate = path.with_extension(extension);
        if candidate.exists() {
            return candidate;
        }
    }
    path
}

impl ProcessSpawner {
    pub fn new() -> Self {
        Self {
            active_processes: Arc::new(Mutex::new(HashMap::new())),
            pair_contexts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Invalidate the pair's current turn: its reader will make no further
    /// state changes and its handoff is dropped.
    pub fn bump_run_generation(&self, pair_id: &str) {
        let mut contexts = self.pair_contexts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(context) = contexts.get_mut(pair_id) {
            context.run_generation = context.run_generation.wrapping_add(1);
        }
    }

    /// Kill (process tree) and unregister the live process for `role`.
    /// Returns the child so callers can wait for it if they need to.
    pub fn stop_process(&self, pair_id: &str, role: &str) -> Option<Child> {
        let removed = {
            let mut guard = self
                .active_processes
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.remove(&format!("{}-{}", pair_id, role))
        };
        removed.map(|mut process| {
            println!("[ProcessSpawner] Killing {} process for {}", role, pair_id);
            kill_process_tree(&mut process.child);
            process.child
        })
    }

    /// Kill both roles' processes for a pair.
    pub fn stop_pair_processes(&self, pair_id: &str) -> Vec<Child> {
        ["mentor", "executor"]
            .into_iter()
            .filter_map(|role| self.stop_process(pair_id, role))
            .collect()
    }

    pub async fn trigger_turn(
        &self,
        app: tauri::AppHandle,
        pair_id: String,
        role: String,
        message: String,
    ) -> Result<(), String> {
        let contexts = self.pair_contexts.clone();
        let ctx = {
            let guard = contexts.lock().unwrap_or_else(|e| e.into_inner());
            guard
                .get(&pair_id)
                .cloned()
                .ok_or_else(|| format!("Context not found for pair {}", pair_id))?
        };

        let (model, session_id, provider_kind, reasoning_effort) = if role == "mentor" {
            (
                ctx.mentor_model.as_str(),
                ctx.mentor_session_id.as_deref(),
                ctx.mentor_provider,
                ctx.mentor_reasoning_effort.as_deref(),
            )
        } else {
            (
                ctx.executor_model.as_str(),
                ctx.executor_session_id.as_deref(),
                ctx.executor_provider,
                ctx.executor_reasoning_effort.as_deref(),
            )
        };

        // What kind of turn this is (planning vs review) is fixed by the
        // status prepare_run/resume_run set before the turn starts.
        let (current_iteration, start_status) = app
            .try_state::<Mutex<MessageBroker>>()
            .and_then(|broker| {
                broker
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .status_and_iteration(&pair_id)
            })
            .unwrap_or((0, PairStatus::Idle));

        let turn = TurnContext {
            app: app.clone(),
            contexts: contexts.clone(),
            pair_id: pair_id.clone(),
            role: role.clone(),
            run_generation: ctx.run_generation,
            current_iteration,
            is_review_turn: role == "mentor" && start_status == PairStatus::Reviewing,
        };

        // ── Mock mode: skip real process spawn ──
        if is_mock_mode() {
            println!(
                "[ProcessSpawner] [MOCK] {} {} (mock mode enabled)",
                pair_id, role
            );
            tokio::spawn(run_mock_turn(turn));
            return Ok(());
        }

        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind,
            model,
            session_id,
            role: &role,
            pair_id: &pair_id,
            message: &message,
            reasoning_effort,
        })?;
        let spec = ProviderAdapter::runtime_spec(provider_kind)?;
        let codex_last_message_path = command.last_message_path;

        let (program, args) = resolve_launch(&command.executable, command.args)?;
        let program_display = program.to_string_lossy().into_owned();
        let env_bytes: usize = std::env::vars_os()
            .map(|(key, value)| key.len() + value.len() + 2)
            .sum();
        if let Some(error) = command_line_limit_error(
            CommandLinePlatform::current(),
            &program_display,
            &args,
            env_bytes,
        ) {
            return Err(error);
        }

        println!(
            "[ProcessSpawner] Spawning: {} ({} args, protocol: {:?})",
            program_display,
            args.len(),
            spec
        );

        let mut child_command = Command::new(&program);
        child_command
            .args(&args)
            .current_dir(&ctx.directory)
            // Nothing is ever written to a provider's stdin. Leaving it inherited
            // makes CLIs that read piped stdin before starting (pi, `claude -p`)
            // wait on whatever the app itself was launched with.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Own process group, so stopping the turn can take down the whole
        // tree (npm launchers, the real binary, tool subprocesses).
        #[cfg(unix)]
        child_command.process_group(0);
        #[cfg(windows)]
        child_command.creation_flags(CREATE_NO_WINDOW);
        apply_provider_cli_env(&mut child_command);

        let mut child = child_command
            .spawn()
            .map_err(|e| format!("Failed to start {}: {}", program_display, e))?;
        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

        // Register immediately so a pause landing right now can reach it.
        let process_key = format!("{}-{}", pair_id, role);
        let turn_id = NEXT_TURN_ID.fetch_add(1, Ordering::Relaxed);
        let displaced = {
            let mut guard = self
                .active_processes
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.insert(process_key.clone(), ActiveProcess { child, turn_id })
        };
        if let Some(mut old) = displaced {
            println!(
                "[ProcessSpawner] [{}] Replacing a still-running {} process",
                pair_id, role
            );
            kill_process_tree(&mut old.child);
        }

        if let Some(broker) = app.try_state::<Mutex<MessageBroker>>() {
            let broker = broker.lock().unwrap_or_else(|e| e.into_inner());
            broker.reset_token_usage(&pair_id, &role);
            broker.update_agent_activity(
                &pair_id,
                &role,
                ActivityPhase::Thinking,
                "Starting process...".to_string(),
                None,
            );
            broker.set_turn_started_at(&pair_id, now_millis());
        }

        // Plain-text providers (aider, kiro) never emit JSON events, so a reply
        // line that merely looks like JSON must stay part of the text output
        // instead of being parsed as an event and overriding the answer.
        let parses_json_events = spec.output_transport != OutputTransport::Stdio;
        let suppress_stderr = crate::providers::provider_for_kind(provider_kind)
            .map(|p| p.suppress_stderr())
            .unwrap_or(false);
        let suppress_plain_logging = crate::providers::provider_for_kind(provider_kind)
            .map(|p| p.suppress_plain_output_logging())
            .unwrap_or(false);
        let stderr_tail: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LINES)));

        // Stderr watcher
        {
            let turn = turn.clone();
            let stderr_tail = stderr_tail.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf).await {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            println!(
                                "[ProcessSpawner] [STDERR] [{}] {}: read error: {}",
                                turn.pair_id, turn.role, error
                            );
                            break;
                        }
                    }
                    let line = decode_output_line(&buf);
                    println!(
                        "[ProcessSpawner] [STDERR] [{}] {}: {}",
                        turn.pair_id, turn.role, line
                    );
                    {
                        let mut tail = stderr_tail.lock().unwrap_or_else(|e| e.into_inner());
                        if tail.len() == STDERR_TAIL_LINES {
                            tail.pop_front();
                        }
                        tail.push_back(line.clone());
                    }
                    if !suppress_stderr && turn.generation_is_current() {
                        turn.with_broker(|broker| {
                            broker.add_log_line(
                                &turn.pair_id,
                                &turn.role,
                                &format!("[STDERR] {}", line),
                            );
                        });
                    }
                }
            });
        }

        let active_processes = self.active_processes.clone();
        tokio::spawn(async move {
            let tc = turn;
            let pair_id = tc.pair_id.clone();
            let role = tc.role.clone();
            let last_message_file = RemoveFileOnDrop(codex_last_message_path);
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();

            let mut first_output = true;
            let mut collector = TurnOutputCollector::new(provider_kind, parses_json_events);
            let mut last_token_usage: Option<TurnTokenUsage> = None;
            // Captured when a provider emits a hard turn-level error (e.g. Claude Code
            // `result` with is_error/subtype=error). Surfaced after the stream closes.
            let mut provider_turn_error: Option<String> = None;
            let provider = crate::providers::provider_for_kind(provider_kind);

            // Step cycle detection to prevent infinite loops
            let mut step_cycle_guard = StepCycleGuard::new();

            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        println!(
                            "[ProcessSpawner] [{}] {}: stdout read error: {}",
                            pair_id, role, error
                        );
                        break;
                    }
                }
                // A turn that was paused, killed or superseded must not keep
                // feeding logs, activity or tokens into the pair's new state.
                if !tc.generation_is_current() {
                    if let Some(mut process) =
                        take_own_process(&active_processes, &process_key, turn_id)
                    {
                        kill_process_tree(&mut process.child);
                    }
                    tc.log_stale("output after the turn was invalidated");
                    return;
                }

                // Lossy decoding: one invalid UTF-8 byte must not end the turn.
                let line = decode_output_line(&buf);
                let mut is_internal_json = false;

                if let Some(event) = parses_json_events
                    .then(|| parse_json_event(&line))
                    .flatten()
                {
                    is_internal_json = true;
                    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let event_type_lower = event_type.to_lowercase();

                    if !event_type.is_empty() {
                        println!(
                            "[ProcessSpawner] [{}] {}: [JSON] [TYPE: {}]",
                            pair_id, role, event_type
                        );
                    } else {
                        println!("[ProcessSpawner] [{}] {}: [JSON]", pair_id, role);
                    }

                    collector.observe_json(&event);

                    // Capture provider-specific turn errors so we can surface them after the
                    // stream closes instead of treating the empty result text as success.
                    if let Some(provider) = provider.as_ref() {
                        if let Some(detail) = provider.extract_error_detail(&event) {
                            provider_turn_error = Some(detail);
                        } else if provider.clears_turn_error(&event) {
                            provider_turn_error = None;
                        }
                    }

                    if let Some(usage) = extract_token_usage(provider_kind, &event) {
                        let usage = accumulate_token_usage(
                            provider_kind,
                            &event,
                            last_token_usage.as_ref(),
                            usage,
                        );
                        last_token_usage = Some(usage.clone());
                        tc.with_broker(|broker| {
                            broker.update_token_usage(&pair_id, &role, usage);
                        });
                    }

                    // Step cycle detection runs on every step_start event in the
                    // stream — not just the first output line — so a runaway
                    // agent actually trips the kill switch.
                    if event_type_lower.contains("step_start") {
                        match step_cycle_guard.record_step(now_millis()) {
                            StepCycleVerdict::Terminate { count } => {
                                println!(
                                    "[ProcessSpawner] [{}] [{}] Step cycle limit exceeded ({} cycles), terminating turn to prevent infinite loop",
                                    pair_id, role, count
                                );
                                if let Some(mut process) =
                                    take_own_process(&active_processes, &process_key, turn_id)
                                {
                                    kill_process_tree(&mut process.child);
                                }
                                let decision = TurnDecision::Error {
                                    detail: format!(
                                        "Agent entered an infinite step loop ({} cycles). Terminated to prevent CPU exhaustion.",
                                        count
                                    ),
                                };
                                if tc
                                    .if_current(|broker| {
                                        apply_turn_decision(&tc, broker, &decision)
                                    })
                                    .is_some()
                                {
                                    finish_turn_bookkeeping(&tc, &decision).await;
                                }
                                // The turn is over: nothing below may run for it.
                                return;
                            }
                            StepCycleVerdict::Rapid { interval_ms, count } => {
                                println!(
                                    "[ProcessSpawner] [{}] [{}] WARNING: Rapid step cycling detected ({}ms interval, cycle #{})",
                                    pair_id, role, interval_ms, count
                                );
                            }
                            StepCycleVerdict::Ok => {}
                        }
                    }

                    let is_tool_event = event_type_lower.contains("tool")
                        || event_type_lower.contains("function_call");
                    if first_output || is_tool_event {
                        tc.with_broker(|broker| {
                            let (phase, label) = if is_tool_event {
                                let tool_name =
                                    event.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                                broker.add_cognitive_event(
                                    &pair_id,
                                    &role,
                                    crate::types::CognitiveEventType::ToolCall,
                                    Some(tool_name.to_string()),
                                    format!("Calling {}", tool_name),
                                    crate::types::CognitiveEventStatus::Running,
                                );
                                (ActivityPhase::UsingTools, format!("Calling {}", tool_name))
                            } else if event_type_lower.contains("content_block_delta")
                                || event_type_lower.contains("text")
                                || event_type_lower.contains("content")
                                || event_type_lower.contains("stream")
                                || event_type_lower.contains("message")
                            {
                                broker.add_cognitive_event(
                                    &pair_id,
                                    &role,
                                    crate::types::CognitiveEventType::Reasoning,
                                    None,
                                    "Processing response".to_string(),
                                    crate::types::CognitiveEventStatus::Running,
                                );
                                (ActivityPhase::Responding, "Processing response".to_string())
                            } else if event_type_lower.contains("turn_start") {
                                broker.add_cognitive_event(
                                    &pair_id,
                                    &role,
                                    crate::types::CognitiveEventType::Reasoning,
                                    None,
                                    "Analyzing task".to_string(),
                                    crate::types::CognitiveEventStatus::Running,
                                );
                                (ActivityPhase::Thinking, "Analyzing task".to_string())
                            } else if event_type_lower.contains("thinking") {
                                broker.add_cognitive_event(
                                    &pair_id,
                                    &role,
                                    crate::types::CognitiveEventType::Reasoning,
                                    None,
                                    "Reasoning...".to_string(),
                                    crate::types::CognitiveEventStatus::Running,
                                );
                                (ActivityPhase::Thinking, "Reasoning...".to_string())
                            } else if event_type_lower.contains("step_start") {
                                // Cycle counting happens above for every event;
                                // here we only label the activity phase.
                                (ActivityPhase::Thinking, "Starting step".to_string())
                            } else if event_type_lower.contains("result")
                                || event_type_lower.contains("complete")
                                || event_type_lower.contains("done")
                            {
                                (ActivityPhase::Responding, "Finalizing response".to_string())
                            } else {
                                broker.add_cognitive_event(
                                    &pair_id,
                                    &role,
                                    crate::types::CognitiveEventType::Reasoning,
                                    None,
                                    "Processing".to_string(),
                                    crate::types::CognitiveEventStatus::Running,
                                );
                                (ActivityPhase::Responding, "Processing response".to_string())
                            };
                            broker.update_agent_activity(&pair_id, &role, phase, label, None);
                        });
                        first_output = false;
                    }

                    if let Some(sid) = extract_session_id(&event) {
                        // Only the live run may record a session: a killed turn
                        // draining its buffered output must not make the next
                        // run resume the previous run's conversation.
                        let mut should_persist_snapshot = false;
                        {
                            let mut guard = tc.contexts.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(c) = guard
                                .get_mut(&pair_id)
                                .filter(|c| c.run_generation == tc.run_generation)
                            {
                                let slot = if role == "mentor" {
                                    &mut c.mentor_session_id
                                } else {
                                    &mut c.executor_session_id
                                };
                                if slot.as_deref() != Some(sid.as_str()) {
                                    println!(
                                        "[ProcessSpawner] [{}] Registered {} session: {}",
                                        pair_id, role, sid
                                    );
                                    *slot = Some(sid);
                                    should_persist_snapshot = true;
                                }
                            }
                        }
                        if should_persist_snapshot {
                            let app = tc.app.clone();
                            let pair_id = pair_id.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                let _ = persist_current_pair_snapshot(&app, &pair_id);
                            });
                        }
                    }
                } else {
                    println!("[ProcessSpawner] [{}] {}: {}", pair_id, role, line);

                    if first_output && !line.trim().is_empty() {
                        tc.with_broker(|broker| {
                            broker.update_agent_activity(
                                &pair_id,
                                &role,
                                ActivityPhase::Responding,
                                "Processing response".to_string(),
                                None,
                            );
                        });
                        first_output = false;
                    }
                }

                // Keep detailed logs, but avoid polluting the progress view
                // with punctuation-only lines and JSON internals.
                let show_in_log = !suppress_plain_logging && !should_skip_plain_output_line(&line);
                let counts_as_output = !line.trim().is_empty();
                if show_in_log || counts_as_output {
                    tc.with_broker(|broker| {
                        if show_in_log {
                            broker.add_log_line(&pair_id, &role, &line);
                        }
                        // Every event counts as progress — JSON-streaming
                        // providers used to look "stalled" because only plain
                        // lines refreshed last_output_at.
                        if counts_as_output {
                            broker.update_output_progress(&pair_id, &role);
                        }
                    });
                }

                if !is_internal_json {
                    collector.observe_plain_line(&line);
                }
            }

            println!("[ProcessSpawner] [{}] {} output closed", pair_id, role);

            // Reap the child to learn how it ended. If the slot no longer
            // holds this turn's child, the turn was cancelled.
            let exit = await_turn_exit(&active_processes, &process_key, turn_id).await;

            let final_output_from_file = last_message_file
                .0
                .as_ref()
                .and_then(ProviderAdapter::read_last_message_file);
            drop(last_message_file);

            let exit_status = match exit {
                TurnExit::Cancelled => {
                    tc.log_stale("process was stopped");
                    return;
                }
                TurnExit::Exited(status) => status,
            };

            let (mut final_output, terminal_success) = collector.finish(final_output_from_file);

            let stderr_lines: Vec<String> = stderr_tail
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect();
            let exit_failure = exit_status
                .as_ref()
                .and_then(|status| exit_status_failure(status, &stderr_lines));
            if let Some(status) = exit_status.as_ref() {
                println!(
                    "[ProcessSpawner] [{}] {} process exited: {}",
                    pair_id, role, status
                );
            }

            // A hard error reported in the stream wins; otherwise a failed exit
            // without a terminal success event is the turn's error (the only
            // failure signal plain-text providers such as aider/kiro have).
            let turn_error = provider_turn_error
                .take()
                .or_else(|| exit_failure.filter(|_| !terminal_success));

            let no_text_output = final_output.trim().is_empty();
            if no_text_output {
                final_output = format!(
                    "No textual output captured from {}. Paused for manual review.",
                    role
                );
            }

            // Make an error unambiguous so it isn't mistaken for a normal result.
            if let Some(detail) = turn_error.as_ref() {
                final_output = if no_text_output {
                    format!("{} reported an error: {}", role, detail)
                } else {
                    format!("{}\n\n[error] {}", final_output, detail)
                };
            }

            complete_turn(
                tc,
                TurnOutput {
                    final_output,
                    no_text_output,
                    turn_error,
                    token_usage: last_token_usage,
                },
            )
            .await;
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_registry::ProviderKind;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn parse_json_event_handles_raw_and_data_prefixed_payloads() {
        assert_eq!(
            parse_json_event(r#"{"type":"step_start"}"#).and_then(|event| event
                .get("type")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())),
            Some("step_start".to_string())
        );

        assert_eq!(
            parse_json_event(r#"data: {"type":"step_start"}"#).and_then(|event| event
                .get("type")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())),
            Some("step_start".to_string())
        );

        assert!(parse_json_event("plain text").is_none());
    }

    #[test]
    fn extract_session_id_prefers_top_level_fields_before_nested_payloads() {
        let event = json!({
            "sessionID": "ses_top_level",
            "part": {
                "sessionID": "ses_nested"
            }
        });

        assert_eq!(extract_session_id(&event).as_deref(), Some("ses_top_level"));
    }

    #[test]
    fn extract_session_id_reads_codex_thread_id() {
        // Codex (`codex exec --json`) exposes its resumable session only as
        // `thread_id` on `thread.started`; without this the resume arg is never
        // populated and every codex turn starts a brand-new session.
        let event = json!({
            "type": "thread.started",
            "thread_id": "019d1c0a-0137-73f3-bf4a-88c90739150c"
        });

        assert_eq!(
            extract_session_id(&event).as_deref(),
            Some("019d1c0a-0137-73f3-bf4a-88c90739150c")
        );
    }

    #[test]
    fn extract_session_id_reads_grok_camel_case_session_id() {
        // Grok Build's streaming-json `end` event reports the resumable
        // session as camelCase `sessionId`.
        let event = json!({
            "type": "end",
            "stopReason": "end_turn",
            "sessionId": "abc123"
        });

        assert_eq!(extract_session_id(&event).as_deref(), Some("abc123"));
    }

    #[test]
    fn step_cycle_guard_terminates_after_limit() {
        let mut guard = StepCycleGuard::new();
        let mut now = 1_000u64;
        for _ in 0..MAX_STEP_CYCLES_PER_TURN {
            now += 1_000;
            assert!(matches!(guard.record_step(now), StepCycleVerdict::Ok));
        }
        now += 1_000;
        assert!(matches!(
            guard.record_step(now),
            StepCycleVerdict::Terminate { .. }
        ));
    }

    #[test]
    fn step_cycle_guard_flags_rapid_cycling_without_terminating() {
        let mut guard = StepCycleGuard::new();
        assert!(matches!(guard.record_step(1_000), StepCycleVerdict::Ok));
        assert!(matches!(
            guard.record_step(1_010),
            StepCycleVerdict::Rapid {
                interval_ms: 10,
                count: 2
            }
        ));
    }

    #[test]
    fn extract_event_texts_collects_nested_text_like_fields() {
        let event = json!({
            "text": "hello",
            "part": {
                "content": "world"
            },
            "parts": [
                { "message": "again" }
            ]
        });

        assert_eq!(
            extract_event_texts(&event),
            vec![
                "hello".to_string(),
                "world".to_string(),
                "again".to_string()
            ]
        );
    }

    #[test]
    fn collapse_candidates_prefers_deduplicated_snapshots_or_joins_distinct_fragments() {
        assert_eq!(
            collapse_candidates(&[
                "full snapshot".to_string(),
                "full snapshot".to_string(),
                "full snapshot".to_string()
            ]),
            Some("full snapshot".to_string())
        );

        assert_eq!(
            collapse_candidates(&["step one".to_string(), "step two".to_string()]),
            Some("step one\nstep two".to_string())
        );
    }

    #[test]
    fn has_signal_token_on_own_line_and_noise_checks_match_the_output_filters() {
        assert!(has_signal_token_on_own_line(
            "`TASK_COMPLETE`",
            "TASK_COMPLETE"
        ));
        assert!(!has_signal_token_on_own_line(
            "TASK_COMPLETE please",
            "TASK_COMPLETE"
        ));

        assert!(is_noise_event_type_for_final("thread.started"));
        assert!(is_noise_event_type_for_final("tool_call"));
        assert!(is_noise_event_type_for_final("tool_start"));
        assert!(!is_noise_event_type_for_final("tool_result"));
        assert!(is_noise_text_candidate("reconnecting..."));
        assert!(is_noise_text_candidate("}"));
        assert!(should_skip_plain_output_line(" ] "));
        assert!(!is_noise_text_candidate("Work finished successfully"));
    }

    #[test]
    fn mentor_finish_signal_is_only_actionable_during_review() {
        let output = "Greeting 1/3 received.\nTASK_COMPLETE";

        assert!(!mentor_finish_signal_is_actionable("mentor", false, output));
        assert!(!mentor_finish_signal_is_actionable(
            "executor", true, output
        ));
        assert!(mentor_finish_signal_is_actionable("mentor", true, output));
    }

    #[test]
    fn dev_smoke_greetings_skip_acceptance_and_allow_plain_finish() {
        let spec = "This is a smoke test of the pair execution loop.\nEach time the executor sends a greeting, respond with exactly: \"Greeting N/3 received.\"";

        assert!(is_dev_smoke_pair_spec(spec));
        assert!(is_dev_smoke_greeting_output("Greeting 2/3"));
        // Paraphrased forms should also count — real executor CLIs sometimes
        // echo the mentor's "Send Greeting N/3" wording instead of the bare
        // greeting, and that shouldn't trigger code-quality checks.
        assert!(is_dev_smoke_greeting_output("Send Greeting 1/3"));
        assert!(is_dev_smoke_greeting_output("Greeting 1/3 received"));
        assert!(is_dev_smoke_greeting_output(
            "Acknowledged: Greeting 2/3 is on the way."
        ));
        assert!(!is_dev_smoke_greeting_output(
            "Acknowledged. Ready to receive remaining greetings."
        ));
        assert!(has_signal_token_on_own_line(
            "Greeting 3/3 received.\nTASK_COMPLETE",
            MENTOR_FINISH_SIGNAL
        ));
    }

    #[test]
    fn mock_dev_smoke_executor_advances_greetings_by_iteration() {
        assert_eq!(
            mock_responses_for_scenario("executor", 1, "dev-smoke"),
            vec!["Greeting 1/3"]
        );
        assert_eq!(
            mock_responses_for_scenario("executor", 2, "dev-smoke"),
            vec!["Greeting 2/3"]
        );
        assert_eq!(
            mock_responses_for_scenario("executor", 3, "dev-smoke"),
            vec!["Greeting 3/3"]
        );
    }

    #[test]
    fn claude_result_events_are_used_for_final_output_only() {
        let thinking_event = json!({
            "type": "content_block_delta",
            "delta": {
                "type": "thinking_delta",
                "thinking": "I should not leak this"
            }
        });
        let result_event = json!({
            "type": "result",
            "result": "Final answer only"
        });

        let mut candidates = Vec::new();
        collect_json_candidates_for_provider(
            ProviderKind::Claude,
            &thinking_event,
            &mut candidates,
        );
        collect_json_candidates_for_provider(ProviderKind::Claude, &result_event, &mut candidates);

        assert_eq!(candidates, vec!["Final answer only".to_string()]);
    }

    #[test]
    fn claude_assistant_text_blocks_are_used_when_result_is_missing() {
        let assistant_event = json!({
            "type": "assistant",
            "message": {
                "content": [
                    { "type": "thinking", "thinking": "private reasoning" },
                    { "type": "text", "text": "Visible mentor plan" }
                ]
            }
        });

        let mut candidates = Vec::new();
        collect_json_candidates_for_provider(
            ProviderKind::Claude,
            &assistant_event,
            &mut candidates,
        );

        assert_eq!(candidates, vec!["Visible mentor plan".to_string()]);
    }

    #[test]
    fn gemini_result_event_is_the_only_text_source() {
        // agy stream-json: step_update deltas repeat what `result.response`
        // holds in full, so only the result envelope contributes text.
        let delta = json!({
            "event": "step_update",
            "step_update": { "step_type": "agent_response", "text_delta": "Cross-provider" }
        });
        let result = json!({
            "event": "result",
            "result": { "status": "SUCCESS", "response": "Cross-provider handoff is ready.\n" }
        });

        let mut candidates = Vec::new();
        collect_json_candidates_for_provider(ProviderKind::Gemini, &delta, &mut candidates);
        collect_json_candidates_for_provider(ProviderKind::Gemini, &result, &mut candidates);

        assert_eq!(
            candidates,
            vec!["Cross-provider handoff is ready.".to_string()]
        );
    }

    #[test]
    fn extract_session_id_reads_agy_init_conversation_id() {
        let init = json!({
            "event": "init",
            "conversation_id": "235ff5af-1c2d",
            "init": { "permission_mode": "request-review" }
        });
        assert_eq!(extract_session_id(&init).as_deref(), Some("235ff5af-1c2d"));

        // A `conversation_id` outside agy's init envelope is not a session id.
        let other = json!({"type": "message", "conversation_id": "not-a-session"});
        assert_eq!(extract_session_id(&other), None);
    }

    use crate::types::TokenUsageSource;

    #[test]
    fn extract_token_usage_from_claude_parses_result_and_streaming_events() {
        let result_event = json!({
            "type": "result",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 250
            }
        });

        let usage = extract_token_usage(ProviderKind::Claude, &result_event)
            .expect("should parse claude result");
        assert_eq!(usage.output_tokens, 250);
        assert_eq!(usage.input_tokens, Some(100));
        assert!(matches!(usage.source, TokenUsageSource::Final));

        // Claude Code's stream-json protocol emits per-message usage on
        // `assistant` events (verified against claude-code 2.1.267), not on
        // `content_block_delta` (which is Anthropic's raw Messages API SSE).
        let streaming_event = json!({
            "type": "assistant",
            "message": {
                "usage": {
                    "input_tokens": 50,
                    "output_tokens": 75
                }
            }
        });

        let usage = extract_token_usage(ProviderKind::Claude, &streaming_event)
            .expect("should parse claude streaming");
        assert_eq!(usage.output_tokens, 75);
        assert!(matches!(usage.source, TokenUsageSource::Live));
    }

    #[test]
    fn extract_token_usage_from_codex_extracts_from_usage_field() {
        let event = json!({
            "type": "result",
            "usage": {
                "prompt_tokens": 200,
                "completion_tokens": 350
            }
        });

        let usage =
            extract_token_usage(ProviderKind::Codex, &event).expect("should parse codex usage");
        assert_eq!(usage.output_tokens, 350);
        assert_eq!(usage.input_tokens, Some(200));
        assert!(matches!(usage.source, TokenUsageSource::Final));
    }

    #[test]
    fn extract_token_usage_from_codex_marks_turn_completed_as_final() {
        // codex exec's terminal event is `turn.completed` (with input_tokens/output_tokens,
        // not prompt/completion_tokens). It must be classified Final — without this the
        // UI token chip never flips from the live spinner.
        let event = json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 24763,
                "cached_input_tokens": 24448,
                "output_tokens": 122,
                "reasoning_output_tokens": 0
            }
        });

        let usage =
            extract_token_usage(ProviderKind::Codex, &event).expect("should parse turn.completed");
        assert_eq!(usage.output_tokens, 122);
        assert_eq!(usage.input_tokens, Some(24763));
        assert!(matches!(usage.source, TokenUsageSource::Final));
    }

    #[test]
    fn extract_token_usage_from_claude_reads_assistant_message_usage_as_live() {
        // stream-json emits per-message usage on `assistant` events (message.usage).
        // This is the real live token source; the prior content_block_delta path never
        // fired because stream-json does not emit those raw SSE events.
        let event = json!({
            "type": "assistant",
            "session_id": "session_01",
            "message": {
                "id": "msg_1",
                "usage": { "input_tokens": 120, "output_tokens": 45 }
            }
        });

        let usage = extract_token_usage(ProviderKind::Claude, &event)
            .expect("should parse assistant usage");
        assert_eq!(usage.output_tokens, 45);
        assert_eq!(usage.input_tokens, Some(120));
        assert!(matches!(usage.source, TokenUsageSource::Live));
    }

    #[test]
    fn claude_result_error_detail_detects_error_and_permission_denials() {
        let provider = crate::providers::provider_for_kind(ProviderKind::Claude)
            .expect("Claude provider should be registered");

        let is_error_result = json!({
            "type": "result",
            "subtype": "error",
            "is_error": true,
            "result": "",
            "error": "Permission denied"
        });
        assert_eq!(
            provider.extract_error_detail(&is_error_result).as_deref(),
            Some("Permission denied")
        );

        let denial_result = json!({
            "type": "result",
            "subtype": "success",
            "is_error": true,
            "permission_denials": [
                { "tool_name": "Bash", "tool_use_id": "t1" }
            ]
        });
        let detail = provider
            .extract_error_detail(&denial_result)
            .expect("should detect denials");
        assert!(detail.contains("Bash"));

        // Success results must not be flagged.
        let success = json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": "Done."
        });
        assert!(provider.extract_error_detail(&success).is_none());
    }

    #[test]
    fn opencode_step_finish_tokens_are_final_only_when_reason_is_stop() {
        let final_step = json!({
            "type": "step_finish",
            "part": {
                "type": "step-finish",
                "reason": "stop",
                "tokens": { "input": 671, "output": 8 }
            }
        });
        let usage = extract_token_usage(ProviderKind::Opencode, &final_step)
            .expect("should parse stop step-finish");
        assert_eq!(usage.output_tokens, 8);
        assert!(matches!(usage.source, TokenUsageSource::Final));

        let tool_step = json!({
            "type": "step_finish",
            "part": {
                "type": "step-finish",
                "reason": "tool-calls",
                "tokens": { "input": 21772, "output": 110 }
            }
        });
        let usage = extract_token_usage(ProviderKind::Opencode, &tool_step)
            .expect("should parse tool-calls step-finish");
        assert_eq!(usage.output_tokens, 110);
        assert!(matches!(usage.source, TokenUsageSource::Live));
    }

    #[test]
    fn extract_token_usage_from_opencode_detects_live_vs_final() {
        let live_event = json!({
            "type": "stream",
            "usage": {
                "input_tokens": 80,
                "output_tokens": 120
            }
        });

        let usage = extract_token_usage(ProviderKind::Opencode, &live_event)
            .expect("should parse opencode live");
        assert_eq!(usage.output_tokens, 120);
        assert!(matches!(usage.source, TokenUsageSource::Live));

        let final_event = json!({
            "type": "result",
            "usage": {
                "input_tokens": 80,
                "output_tokens": 150
            }
        });

        let usage = extract_token_usage(ProviderKind::Opencode, &final_event)
            .expect("should parse opencode final");
        assert_eq!(usage.output_tokens, 150);
        assert!(matches!(usage.source, TokenUsageSource::Final));
    }

    #[test]
    fn extract_token_usage_from_gemini_parses_result_usage() {
        let event = json!({
            "event": "result",
            "result": {
                "status": "SUCCESS",
                "usage": { "input_tokens": 300, "output_tokens": 450 }
            }
        });

        let usage =
            extract_token_usage(ProviderKind::Gemini, &event).expect("should parse gemini usage");
        assert_eq!(usage.output_tokens, 450);
        assert_eq!(usage.input_tokens, Some(300));
        assert!(matches!(usage.source, TokenUsageSource::Final));
    }

    #[test]
    fn extract_token_usage_returns_none_for_events_without_usage() {
        let no_usage = json!({
            "type": "content_block_delta",
            "delta": { "text": "hello" }
        });

        assert!(extract_token_usage(ProviderKind::Claude, &no_usage).is_none());
        assert!(extract_token_usage(ProviderKind::Codex, &no_usage).is_none());
        assert!(extract_token_usage(ProviderKind::Opencode, &no_usage).is_none());
        assert!(extract_token_usage(ProviderKind::Gemini, &no_usage).is_none());
        assert!(extract_token_usage(ProviderKind::Kimi, &no_usage).is_none());
    }

    #[test]
    fn extract_token_usage_dispatches_to_correct_provider_parser() {
        let claude_event = json!({
            "type": "result",
            "usage": { "output_tokens": 100 }
        });
        let usage =
            extract_token_usage(ProviderKind::Claude, &claude_event).expect("claude dispatch");
        assert_eq!(usage.output_tokens, 100);

        let codex_event = json!({
            "usage": { "completion_tokens": 200 }
        });
        let usage = extract_token_usage(ProviderKind::Codex, &codex_event).expect("codex dispatch");
        assert_eq!(usage.output_tokens, 200);

        let opencode_event = json!({
            "type": "result",
            "usage": { "output_tokens": 300 }
        });
        let usage = extract_token_usage(ProviderKind::Opencode, &opencode_event)
            .expect("opencode dispatch");
        assert_eq!(usage.output_tokens, 300);

        let gemini_event = json!({
            "event": "result",
            "result": { "usage": { "output_tokens": 400 } }
        });
        let usage =
            extract_token_usage(ProviderKind::Gemini, &gemini_event).expect("gemini dispatch");
        assert_eq!(usage.output_tokens, 400);

        // Kimi stream-json events carry no usage data — the parser must stay silent
        // even for events that other providers would read a `usage` object from.
        let kimi_event = json!({
            "role": "assistant",
            "content": "done",
            "usage": { "output_tokens": 500 }
        });
        assert!(extract_token_usage(ProviderKind::Kimi, &kimi_event).is_none());
    }

    #[test]
    fn muse_stream_pipeline_extracts_final_text_and_session_id() {
        // Verbatim stream captured from `muse exec --json` (Muse Code 1.0.3 on
        // 2026-09-19), trimmed to the payload types that matter. The run below
        // completed successfully *while* an internal reminder subtask reported
        // `task.lifecycle.failed` — that must not be read as a turn error.
        let lines = [
            r#"{"schema_version":1,"stream":{"kind":"session","id":"01a0b758-cd4f-7f32-9ce7-79c437f3e1e7"},"sequence":1,"record_type":"reconciliation","payload_type":"runtime.command.accepted","payload":{"kind":"command_accepted","command_kind":"turn.submit"}}"#,
            r#"{"schema_version":1,"stream":{"kind":"session","id":"01a0b758-cd4f-7f32-9ce7-79c437f3e1e7"},"sequence":4,"record_type":"status","payload_type":"turn.input.user","payload":{"kind":"turn_input_user","prompt":"Reply with exactly the word: PONG"}}"#,
            r#"{"schema_version":1,"stream":{"kind":"session","id":"01a0b758-cd4f-7f32-9ce7-79c437f3e1e7"},"sequence":20,"record_type":"status","payload_type":"run.output.delta","payload":{"kind":"run_output_delta","text":"PONG"}}"#,
            r#"{"schema_version":1,"stream":{"kind":"session","id":"01a0b758-cd4f-7f32-9ce7-79c437f3e1e7"},"sequence":27,"record_type":"event","payload_type":"task.lifecycle.failed","payload":{"kind":"task_lifecycle","event":{"kind":"failed","reason":"invalid run configuration: provider does not support base instructions"}}}"#,
            r#"{"schema_version":1,"stream":{"kind":"session","id":"01a0b758-cd4f-7f32-9ce7-79c437f3e1e7"},"sequence":38,"record_type":"event","payload_type":"run.terminal.completed","payload":{"kind":"run_terminal","terminal":"completed","text":"PONG","reason":null}}"#,
        ];

        let mut candidates = Vec::new();
        let mut session_id = None;
        for line in lines {
            let event = parse_json_event(line).expect("muse line parses as JSON");
            collect_json_candidates_for_provider(ProviderKind::Muse, &event, &mut candidates);
            if session_id.is_none() {
                session_id = extract_session_id(&event);
            }
            // Muse emits no usage data on any payload type.
            assert!(extract_token_usage(ProviderKind::Muse, &event).is_none());
        }

        // The prompt echo and the streaming delta must not join the terminal
        // text — otherwise the reply would read "…: PONG\nPONG\nPONG".
        assert_eq!(collapse_candidates(&candidates).as_deref(), Some("PONG"));
        assert_eq!(
            session_id.as_deref(),
            Some("01a0b758-cd4f-7f32-9ce7-79c437f3e1e7")
        );
    }

    #[test]
    fn muse_session_stream_id_is_read_without_shadowing_other_providers() {
        let muse = json!({
            "stream": { "kind": "session", "id": "01a0b758-cd4f-7f32" },
            "payload_type": "run.lifecycle.started"
        });
        assert_eq!(
            extract_session_id(&muse).as_deref(),
            Some("01a0b758-cd4f-7f32")
        );

        // A non-session stream (run/task envelopes) must not be mistaken for one.
        let run_stream = json!({ "stream": { "kind": "run", "id": "not-a-session" } });
        assert_eq!(extract_session_id(&run_stream), None);

        // Existing providers keep their own extraction paths.
        let codex = json!({ "thread_id": "thread_1" });
        assert_eq!(extract_session_id(&codex).as_deref(), Some("thread_1"));
    }

    #[test]
    fn kimi_stream_pipeline_extracts_final_message_and_session_id() {
        // Verbatim stream captured from `kimi -p … --output-format stream-json`
        // (kimi-code 2.0.1 on 2026-09-19; previously verified against
        // 0.42.0 — event schema unchanged across the major version bump).
        // Model: wanqing-streamlake/kat-coder-pro-v2.5. The first event
        // carries a whitespace-only `content` alongside `tool_calls`.
        let lines = [
            r#"{"role":"assistant","content":"\n\n","tool_calls":[{"type":"function","id":"call_68cef8bf9e02409aabfa9830","function":{"name":"Write","arguments":"{\"content\":\"verified\",\"path\":\"kat-probe.txt\"}"}}]}"#,
            r#"{"role":"tool","tool_call_id":"call_68cef8bf9e02409aabfa9830","content":"Wrote 8 bytes to kat-probe.txt"}"#,
            r#"{"role":"assistant","content":"\n\ndone"}"#,
            r#"{"role":"meta","type":"session.resume_hint","session_id":"session_b2ef5dc9-4101-465b-a731-8f9a5a625b92","command":"kimi -r session_b2ef5dc9-4101-465b-a731-8f9a5a625b92","content":"To resume this session: kimi -r session_b2ef5dc9-4101-465b-a731-8f9a5a625b92"}"#,
        ];

        let mut candidates = Vec::new();
        let mut session_id = None;
        for line in lines {
            let event = parse_json_event(line).expect("kimi line parses as JSON");
            collect_json_candidates_for_provider(ProviderKind::Kimi, &event, &mut candidates);
            if session_id.is_none() {
                session_id = extract_session_id(&event);
            }
            assert!(extract_token_usage(ProviderKind::Kimi, &event).is_none());
        }

        // Tool output, the resume hint, and whitespace-only assistant deltas
        // must not leak into the turn message.
        assert_eq!(collapse_candidates(&candidates).as_deref(), Some("done"));
        assert_eq!(
            session_id.as_deref(),
            Some("session_b2ef5dc9-4101-465b-a731-8f9a5a625b92")
        );
    }

    #[test]
    fn token_usage_live_to_final_transition_preserves_latest_value() {
        let live_event = json!({
            "type": "assistant",
            "message": {
                "usage": {
                    "input_tokens": 50,
                    "output_tokens": 120
                }
            }
        });

        let final_event = json!({
            "type": "result",
            "usage": {
                "input_tokens": 50,
                "output_tokens": 150
            }
        });

        let live_usage =
            extract_token_usage(ProviderKind::Claude, &live_event).expect("live usage");
        assert_eq!(live_usage.output_tokens, 120);
        assert!(matches!(live_usage.source, TokenUsageSource::Live));

        let final_usage =
            extract_token_usage(ProviderKind::Claude, &final_event).expect("final usage");
        assert_eq!(final_usage.output_tokens, 150);
        assert!(matches!(final_usage.source, TokenUsageSource::Final));
        assert!(final_usage.output_tokens >= live_usage.output_tokens);
    }

    #[test]
    fn token_usage_provider_specific_fields_are_correctly_mapped() {
        let claude_with_alternate_fields = json!({
            "type": "result",
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 200
            }
        });
        let claude_usage = extract_token_usage(ProviderKind::Claude, &claude_with_alternate_fields)
            .expect("claude alternate field names");
        assert_eq!(claude_usage.input_tokens, Some(100));
        assert_eq!(claude_usage.output_tokens, 200);

        let codex_with_alternate_fields = json!({
            "usage": {
                "input_tokens": 150,
                "output_tokens": 250
            }
        });
        let codex_usage = extract_token_usage(ProviderKind::Codex, &codex_with_alternate_fields)
            .expect("codex alternate field names");
        assert_eq!(codex_usage.input_tokens, Some(150));
        assert_eq!(codex_usage.output_tokens, 250);

        let gemini_step_update = json!({
            "event": "step_update",
            "step_update": {
                "usage": {
                    "input_tokens": 175,
                    "output_tokens": 275
                }
            }
        });
        let gemini_usage = extract_token_usage(ProviderKind::Gemini, &gemini_step_update)
            .expect("gemini step_update usage");
        assert_eq!(gemini_usage.input_tokens, Some(175));
        assert_eq!(gemini_usage.output_tokens, 275);
    }

    #[test]
    fn last_token_usage_state_transition_across_stream_events() {
        fn update_last_token_usage(
            current: Option<TurnTokenUsage>,
            event: &serde_json::Value,
            provider: ProviderKind,
        ) -> Option<TurnTokenUsage> {
            extract_token_usage(provider, event).or(current)
        }

        let mut last_token_usage: Option<TurnTokenUsage> = None;

        let event1 = json!({
            "type": "assistant",
            "message": { "usage": { "output_tokens": 100, "input_tokens": 50 } }
        });
        last_token_usage = update_last_token_usage(last_token_usage, &event1, ProviderKind::Claude);
        assert!(last_token_usage.is_some());
        assert_eq!(last_token_usage.as_ref().unwrap().output_tokens, 100);

        let event2 = json!({
            "type": "assistant",
            "message": { "usage": { "output_tokens": 200, "input_tokens": 50 } }
        });
        last_token_usage = update_last_token_usage(last_token_usage, &event2, ProviderKind::Claude);
        assert_eq!(last_token_usage.as_ref().unwrap().output_tokens, 200);

        let event_no_usage = json!({
            "type": "assistant",
            "message": { "content": [{"type": "text", "text": "some text"}] }
        });
        let before_no_usage = last_token_usage.clone();
        last_token_usage =
            update_last_token_usage(last_token_usage, &event_no_usage, ProviderKind::Claude);
        assert_eq!(
            last_token_usage, before_no_usage,
            "should preserve last usage when event has no usage"
        );

        let final_event = json!({
            "type": "result",
            "usage": { "output_tokens": 350, "input_tokens": 50 }
        });
        last_token_usage =
            update_last_token_usage(last_token_usage, &final_event, ProviderKind::Claude);
        assert_eq!(last_token_usage.as_ref().unwrap().output_tokens, 350);
        assert!(matches!(
            last_token_usage.as_ref().unwrap().source,
            TokenUsageSource::Final
        ));
    }

    #[test]
    fn last_token_usage_uses_latest_when_final_event_has_no_usage() {
        fn update_last_token_usage(
            current: Option<TurnTokenUsage>,
            event: &serde_json::Value,
            provider: ProviderKind,
        ) -> Option<TurnTokenUsage> {
            extract_token_usage(provider, event).or(current)
        }

        let mut last_token_usage: Option<TurnTokenUsage> = None;

        let live_event = json!({
            "type": "stream",
            "usage": { "output_tokens": 500, "input_tokens": 100 }
        });
        last_token_usage =
            update_last_token_usage(last_token_usage, &live_event, ProviderKind::Opencode);
        assert_eq!(last_token_usage.as_ref().unwrap().output_tokens, 500);

        let final_no_usage = json!({
            "type": "result",
            "content": "done"
        });
        last_token_usage =
            update_last_token_usage(last_token_usage, &final_no_usage, ProviderKind::Opencode);
        assert!(
            last_token_usage.is_some(),
            "last live usage should be preserved when final event lacks usage"
        );
        assert_eq!(last_token_usage.as_ref().unwrap().output_tokens, 500);
    }

    #[test]
    fn apply_provider_cli_env_propagates_home_and_appdata_overrides() {
        let _guard = crate::test_env::lock_env();
        let temp_home =
            std::env::temp_dir().join(format!("the-pair-test-{}", uuid::Uuid::new_v4()));
        let roaming = temp_home.join("enterprise/Roaming");
        let local = temp_home.join("enterprise/Local");
        let original_home = std::env::var_os("HOME");
        let original_userprofile = std::env::var_os("USERPROFILE");
        let original_appdata = std::env::var_os("APPDATA");
        let original_local_appdata = std::env::var_os("LOCALAPPDATA");

        std::env::set_var("HOME", &temp_home);
        std::env::remove_var("USERPROFILE");
        std::env::set_var("APPDATA", &roaming);
        std::env::set_var("LOCALAPPDATA", &local);

        let mut command = Command::new("opencode");
        apply_provider_cli_env(&mut command);

        let envs: HashMap<_, _> = command
            .as_std()
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value.to_owned())))
            .collect();

        if let Some(value) = original_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(value) = original_userprofile {
            std::env::set_var("USERPROFILE", value);
        } else {
            std::env::remove_var("USERPROFILE");
        }

        if let Some(value) = original_appdata {
            std::env::set_var("APPDATA", value);
        } else {
            std::env::remove_var("APPDATA");
        }

        if let Some(value) = original_local_appdata {
            std::env::set_var("LOCALAPPDATA", value);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }

        assert_eq!(
            envs.get(std::ffi::OsStr::new("HOME")),
            Some(&temp_home.as_os_str().to_owned())
        );
        assert_eq!(
            envs.get(std::ffi::OsStr::new("USERPROFILE")),
            Some(&temp_home.as_os_str().to_owned())
        );
        assert_eq!(
            envs.get(std::ffi::OsStr::new("APPDATA")),
            Some(&roaming.as_os_str().to_owned())
        );
        assert_eq!(
            envs.get(std::ffi::OsStr::new("LOCALAPPDATA")),
            Some(&local.as_os_str().to_owned())
        );
        assert!(envs.contains_key(std::ffi::OsStr::new("PATH")));
    }

    // ── Regression tests for the turn lifecycle fixes ──────────────────────

    #[test]
    fn step_cycle_guard_allows_long_steady_turns_but_stops_bursts() {
        let mut guard = StepCycleGuard::new();
        let mut now = 1_000u64;
        // 200 steps, one every 2 s: a long but legitimate turn.
        for _ in 0..200 {
            now += 2_000;
            assert!(!matches!(
                guard.record_step(now),
                StepCycleVerdict::Terminate { .. }
            ));
        }

        // 50 steps inside 10 s is a loop.
        let mut runaway = StepCycleGuard::new();
        let mut terminated = false;
        for i in 0..RUNAWAY_STEP_BURST as u64 {
            if matches!(
                runaway.record_step(1_000 + i * 100),
                StepCycleVerdict::Terminate { .. }
            ) {
                terminated = true;
            }
        }
        assert!(terminated);
    }

    #[test]
    fn collapse_candidates_never_drops_the_short_final_answer() {
        let analysis = "a".repeat(1000);
        let verdict = "{\"verdict\":\"pass\"}\nTASK_COMPLETE".to_string();
        let collapsed = collapse_candidates(&[analysis.clone(), verdict.clone()]).unwrap();
        assert!(collapsed.contains(&verdict));

        // Cumulative stream snapshots collapse to the final snapshot.
        assert_eq!(
            collapse_candidates(&[
                "Hel".to_string(),
                "Hello wor".to_string(),
                "Hello world".to_string()
            ])
            .as_deref(),
            Some("Hello world")
        );
        assert_eq!(collapse_candidates(&[]), None);
    }

    #[test]
    fn claude_result_event_replaces_narration() {
        let mut collector = TurnOutputCollector::new(ProviderKind::Claude, true);
        collector.observe_json(&json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "I'll check the tests first."}]}
        }));
        collector.observe_json(&json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "Found it; fixing."}]}
        }));
        collector.observe_json(
            &json!({"type": "result", "subtype": "success", "result": "Fixed the bug."}),
        );

        assert_eq!(collector.finish(None), ("Fixed the bug.".to_string(), true));

        // Without a result event the assistant text is the fallback.
        let mut no_result = TurnOutputCollector::new(ProviderKind::Claude, true);
        no_result.observe_json(&json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "Partial"}]}
        }));
        assert_eq!(no_result.finish(None), ("Partial".to_string(), false));
    }

    #[test]
    fn opencode_reply_is_the_last_non_tool_step() {
        let events = [
            json!({"type": "step_start", "part": {"messageID": "msg_1", "type": "step-start"}}),
            json!({"type": "text", "part": {"id": "p1", "messageID": "msg_1", "type": "text", "text": "Let me read the file."}}),
            json!({"type": "tool_use", "part": {"messageID": "msg_1", "type": "tool", "tool": "read"}}),
            json!({"type": "step_finish", "part": {"messageID": "msg_1", "type": "step-finish", "reason": "tool-calls", "tokens": {"input": 100, "output": 10}}}),
            json!({"type": "step_start", "part": {"messageID": "msg_2", "type": "step-start"}}),
            json!({"type": "text", "part": {"id": "p2", "messageID": "msg_2", "type": "text", "text": "All done: the parser is fixed."}}),
            json!({"type": "step_finish", "part": {"messageID": "msg_2", "type": "step-finish", "reason": "stop", "tokens": {"input": 30, "output": 5}}}),
        ];
        let mut collector = TurnOutputCollector::new(ProviderKind::Opencode, true);
        let mut usage: Option<TurnTokenUsage> = None;
        for event in &events {
            collector.observe_json(event);
            if let Some(step) = extract_token_usage(ProviderKind::Opencode, event) {
                usage = Some(accumulate_token_usage(
                    ProviderKind::Opencode,
                    event,
                    usage.as_ref(),
                    step,
                ));
            }
        }

        assert_eq!(
            collector.finish(None),
            ("All done: the parser is fixed.".to_string(), true)
        );
        // Per-step usage is summed over the turn.
        let usage = usage.unwrap();
        assert_eq!(usage.output_tokens, 15);
        assert_eq!(usage.input_tokens, Some(130));
        assert!(matches!(usage.source, TokenUsageSource::Final));
    }

    #[test]
    fn other_providers_token_usage_is_replaced_not_summed() {
        let first = extract_token_usage(
            ProviderKind::Claude,
            &json!({"type": "assistant", "message": {"usage": {"input_tokens": 10, "output_tokens": 5}}}),
        )
        .unwrap();
        let event = json!({"type": "result", "usage": {"input_tokens": 10, "output_tokens": 9}});
        let second = extract_token_usage(ProviderKind::Claude, &event).unwrap();
        let merged = accumulate_token_usage(ProviderKind::Claude, &event, Some(&first), second);
        assert_eq!(merged.output_tokens, 9);
    }

    #[test]
    fn plain_text_output_keeps_braces_and_blank_lines() {
        let mut collector = TurnOutputCollector::new(ProviderKind::Aider, false);
        for line in [
            "Verdict:",
            "{",
            "  \"verdict\": \"pass\"",
            "},",
            "",
            "}",
            "TASK_COMPLETE",
        ] {
            collector.observe_plain_line(line);
        }
        let (text, terminal) = collector.finish(None);
        assert_eq!(
            text,
            "Verdict:\n{\n  \"verdict\": \"pass\"\n},\n\n}\nTASK_COMPLETE"
        );
        assert!(!terminal);

        // JSON providers still filter stray punctuation-only lines.
        let mut json_provider = TurnOutputCollector::new(ProviderKind::Codex, true);
        json_provider.observe_plain_line("}");
        json_provider.observe_plain_line("warning text");
        assert_eq!(json_provider.finish(None).0, "warning text");
    }

    #[test]
    fn decode_output_line_survives_invalid_utf8() {
        let line = decode_output_line(b"caf\xe9 ok\r\n");
        assert_eq!(line, "caf\u{FFFD} ok");
        assert_eq!(decode_output_line(b"\n"), "");
    }

    #[test]
    fn exit_failure_detail_prefers_the_cli_error_line() {
        assert_eq!(exit_failure_detail(true, Some(0), None, &[]), None);
        let stderr = vec![
            "loading config".to_string(),
            "error: failed to run prompt: model not found".to_string(),
            "hint: check --model".to_string(),
        ];
        assert_eq!(
            exit_failure_detail(false, Some(1), None, &stderr).as_deref(),
            Some("error: failed to run prompt: model not found (process exited with code 1)")
        );
        assert_eq!(
            exit_failure_detail(false, Some(3), None, &["mcp server crashed".to_string()])
                .as_deref(),
            Some("process exited with code 3: mcp server crashed")
        );
        assert_eq!(
            exit_failure_detail(false, None, Some(9), &[]).as_deref(),
            Some("process was terminated by signal 9")
        );
    }

    #[test]
    fn command_line_limits_are_reported_before_spawning() {
        let small = vec!["-p".to_string(), "hello".to_string()];
        for platform in [
            CommandLinePlatform::Linux,
            CommandLinePlatform::MacOs,
            CommandLinePlatform::Windows,
        ] {
            assert!(command_line_limit_error(platform, "claude", &small, 4096).is_none());
        }

        let linux_big = vec!["-p".to_string(), "x".repeat(200 * 1024)];
        let error = command_line_limit_error(CommandLinePlatform::Linux, "claude", &linux_big, 0)
            .expect("200 KB exceeds MAX_ARG_STRLEN");
        assert!(error.contains("200 KB"));
        assert!(error.contains("Remove attached files"));

        let windows_big = vec!["x".repeat(40_000)];
        assert!(
            command_line_limit_error(CommandLinePlatform::Windows, "node", &windows_big, 0)
                .is_some()
        );
        assert!(
            command_line_limit_error(CommandLinePlatform::MacOs, "claude", &windows_big, 0)
                .is_none()
        );
        let mac_big = vec!["x".repeat(1_100_000)];
        assert!(
            command_line_limit_error(CommandLinePlatform::MacOs, "claude", &mac_big, 0).is_some()
        );
    }

    #[test]
    fn npm_cmd_shims_are_unwrapped_to_their_entry_point() {
        let dir = std::path::Path::new("C:\\Users\\me\\AppData\\Roaming\\npm");
        let modern = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n";
        assert_eq!(
            parse_npm_cmd_shim(modern, dir),
            Some(NpmShimTarget::Node {
                script: dir.join("node_modules\\@openai\\codex\\bin\\codex.js")
            })
        );

        let legacy = "@IF EXIST \"%~dp0\\node.exe\" (\r\n  \"%~dp0\\node.exe\"  \"%~dp0\\node_modules\\npm\\bin\\npm-cli.js\" %*\r\n) ELSE (\r\n  @SETLOCAL\r\n  @SET PATHEXT=%PATHEXT:;.JS;=;%\r\n  node  \"%~dp0\\node_modules\\npm\\bin\\npm-cli.js\" %*\r\n)\r\n";
        assert_eq!(
            parse_npm_cmd_shim(legacy, dir),
            Some(NpmShimTarget::Node {
                script: dir.join("node_modules\\npm\\bin\\npm-cli.js")
            })
        );

        let native = "@ECHO off\r\n\"%~dp0\\node_modules\\@pkg\\cli\\bin\\tool.exe\"   %*\r\n";
        assert_eq!(
            parse_npm_cmd_shim(native, dir),
            Some(NpmShimTarget::Executable {
                program: dir.join("node_modules\\@pkg\\cli\\bin\\tool.exe")
            })
        );

        assert_eq!(parse_npm_cmd_shim("@echo hello %*\r\n", dir), None);
        assert_eq!(parse_npm_cmd_shim("", dir), None);
    }

    fn facts<'a>(role: &'a str, is_review_turn: bool, output: &'a str) -> TurnFacts<'a> {
        TurnFacts {
            role,
            is_review_turn,
            output,
            no_text_output: false,
            turn_error: None,
            verdict: None,
            verdict_error: None,
            repair_attempts: 0,
            iteration: 1,
            max_iterations: 0,
            plan_gate_enabled: false,
            smoke_needs_more: false,
            smoke_finish_before_review: false,
        }
    }

    fn verdict(
        decision: AcceptanceVerdictDecision,
        action: AcceptanceNextAction,
        confidence: f64,
    ) -> AcceptanceVerdict {
        AcceptanceVerdict {
            verdict: decision,
            risk: AcceptanceRisk::Low,
            confidence,
            issues: vec![],
            evidence: vec![],
            reasoning: "r".to_string(),
            summary: "s".to_string(),
            next_step: AcceptanceNextStep {
                instructions: if matches!(action, AcceptanceNextAction::Continue) {
                    vec!["more".to_string()]
                } else {
                    vec![]
                },
                action,
            },
        }
    }

    #[test]
    fn task_complete_only_finishes_when_the_verdict_agrees() {
        let output = "{\"verdict\":\"fail\"}\nTASK_COMPLETE";
        let fail = verdict(
            AcceptanceVerdictDecision::Fail,
            AcceptanceNextAction::Continue,
            0.3,
        );
        let mut f = facts("mentor", true, output);
        f.verdict = Some(&fail);
        assert_eq!(
            decide_turn_outcome(&f),
            TurnDecision::Handoff {
                next_role: "executor"
            }
        );

        let pass = verdict(
            AcceptanceVerdictDecision::Pass,
            AcceptanceNextAction::Finish,
            0.95,
        );
        let mut f = facts("mentor", true, "{...}");
        f.verdict = Some(&pass);
        assert!(matches!(
            decide_turn_outcome(&f),
            TurnDecision::Finish { .. }
        ));

        // A pass + continue verdict keeps going even with a stray TASK_COMPLETE.
        let pass_continue = verdict(
            AcceptanceVerdictDecision::Pass,
            AcceptanceNextAction::Continue,
            0.9,
        );
        let mut f = facts("mentor", true, "{}\nTASK_COMPLETE");
        f.verdict = Some(&pass_continue);
        assert_eq!(
            decide_turn_outcome(&f),
            TurnDecision::Handoff {
                next_role: "executor"
            }
        );
    }

    #[test]
    fn bare_task_complete_finishes_but_a_broken_json_verdict_is_repaired() {
        let mut plain = facts("mentor", true, "All requirements are met.\n\nTASK_COMPLETE");
        plain.verdict_error = Some("no JSON");
        plain.repair_attempts = 1;
        assert!(matches!(
            decide_turn_outcome(&plain),
            TurnDecision::Finish { .. }
        ));

        let mut broken = facts("mentor", true, "{\"verdict\": \"pass\",\nTASK_COMPLETE }");
        broken.verdict_error = Some("EOF while parsing");
        broken.repair_attempts = 1;
        assert_eq!(
            decide_turn_outcome(&broken),
            TurnDecision::Handoff {
                next_role: "mentor"
            }
        );

        broken.repair_attempts = 2;
        assert!(matches!(
            decide_turn_outcome(&broken),
            TurnDecision::Pause { ends_run: true, .. }
        ));
    }

    #[test]
    fn planning_turns_gate_and_ignore_the_budget() {
        let mut plan = facts("mentor", false, "1. Do it\nTASK_COMPLETE");
        plan.max_iterations = 1;
        plan.iteration = 1;
        assert_eq!(
            decide_turn_outcome(&plan),
            TurnDecision::Handoff {
                next_role: "executor"
            }
        );
        plan.plan_gate_enabled = true;
        assert_eq!(decide_turn_outcome(&plan), TurnDecision::AwaitHumanReview);
    }

    #[test]
    fn executor_budget_pause_hands_the_next_turn_to_the_mentor() {
        let mut exec = facts("executor", false, "done");
        exec.max_iterations = 3;
        exec.iteration = 3;
        match decide_turn_outcome(&exec) {
            TurnDecision::Pause {
                hand_to_mentor,
                ends_run,
                detail,
            } => {
                assert!(hand_to_mentor);
                assert!(ends_run);
                assert!(detail.contains("3/3"));
            }
            other => panic!("expected a budget pause, got {:?}", other),
        }

        exec.max_iterations = 0;
        assert_eq!(
            decide_turn_outcome(&exec),
            TurnDecision::Handoff {
                next_role: "mentor"
            }
        );
    }

    #[test]
    fn errors_and_empty_output_stop_the_run() {
        let mut errored = facts("executor", false, "partial");
        errored.turn_error = Some("process exited with code 1");
        assert_eq!(
            decide_turn_outcome(&errored),
            TurnDecision::Error {
                detail: "executor error: process exited with code 1".to_string()
            }
        );

        let mut empty = facts("mentor", true, "");
        empty.no_text_output = true;
        assert!(matches!(
            decide_turn_outcome(&empty),
            TurnDecision::Pause {
                ends_run: true,
                hand_to_mentor: false,
                ..
            }
        ));
    }

    #[test]
    fn mock_success_review_finishes_through_the_shared_decision() {
        let review = mock_responses_for_scenario("mentor", 2, "success").join("\n");
        let mut f = facts("mentor", true, &review);
        f.verdict_error = Some("no verdict");
        f.repair_attempts = 1;
        assert!(matches!(
            decide_turn_outcome(&f),
            TurnDecision::Finish { .. }
        ));

        let plan = mock_responses_for_scenario("mentor", 1, "success").join("\n");
        let mut f = facts("mentor", false, &plan);
        f.plan_gate_enabled = true;
        assert_eq!(decide_turn_outcome(&f), TurnDecision::AwaitHumanReview);
    }

    #[test]
    fn a_review_without_an_executor_record_still_parses_and_counts_repairs() {
        let good = "{\"verdict\":\"pass\",\"risk\":\"low\",\"confidence\":0.9,\"issues\":[],\"evidence\":[\"e\"],\"reasoning\":\"r\",\"summary\":\"s\",\"nextStep\":{\"action\":\"finish\",\"instructions\":[]}}";
        let outcome = process_mentor_review_verdict(None, good, good.to_string(), 2);
        assert!(outcome.acceptance_error.is_none());
        let record = outcome.parsed_acceptance.unwrap();
        assert_eq!(record.iteration, 2);
        assert!(record.verdict.is_some());

        let first = process_mentor_review_verdict(None, "nope", "nope".to_string(), 2);
        let second = process_mentor_review_verdict(
            first.parsed_acceptance.clone(),
            "still nope",
            "still nope".to_string(),
            2,
        );
        assert_eq!(second.parsed_acceptance.unwrap().repair_attempts, 2);
    }

    #[test]
    fn synthesized_finish_acceptance_is_a_pass_without_error() {
        let record =
            synthesize_finish_acceptance(None, "All requirements are met.\n\nTASK_COMPLETE", 1);
        let verdict = record.verdict.expect("verdict");
        assert!(matches!(verdict.verdict, AcceptanceVerdictDecision::Pass));
        assert!(matches!(
            verdict.next_step.action,
            AcceptanceNextAction::Finish
        ));
        assert!(should_stop_iteration(&verdict));
        assert_eq!(verdict.summary, "All requirements are met.");
        assert!(record.error.is_none());
        assert_eq!(record.iteration, 1);
    }

    #[test]
    fn a_failed_parse_clears_a_verdict_left_from_an_earlier_attempt() {
        let mut existing = synthesize_finish_acceptance(None, "ok", 1);
        existing.verdict = Some(verdict(
            AcceptanceVerdictDecision::Pass,
            AcceptanceNextAction::Finish,
            1.0,
        ));
        let outcome =
            process_mentor_review_verdict(Some(existing), "not json", "not json".to_string(), 1);
        let acceptance = outcome.parsed_acceptance.unwrap();
        assert!(acceptance.verdict.is_none());
        assert!(outcome.acceptance_error.is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn kill_process_tree_takes_down_grandchildren() {
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & echo $!; wait"])
            .stdout(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().expect("spawn sh");
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let grandchild: i32 = line.trim().parse().expect("grandchild pid");

        kill_process_tree(&mut child);
        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;

        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        let mut gone = false;
        for _ in 0..50 {
            // SAFETY: signal 0 only checks for existence.
            if unsafe { kill(grandchild, 0) } != 0 {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(gone, "the grandchild must die with its process group");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn await_turn_exit_reports_status_or_cancellation() {
        let active: ActiveProcessMap = Arc::new(Mutex::new(HashMap::new()));
        let child = Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        active.lock().unwrap().insert(
            "p-executor".to_string(),
            ActiveProcess { child, turn_id: 7 },
        );

        match await_turn_exit(&active, "p-executor", 7).await {
            TurnExit::Exited(Some(status)) => assert_eq!(status.code(), Some(3)),
            _ => panic!("expected the exit status"),
        }
        assert!(active.lock().unwrap().is_empty());

        // A slot now held by another turn means this one was cancelled, and
        // the newer child must be left alone.
        let other = Command::new("sh").args(["-c", "sleep 5"]).spawn().unwrap();
        active.lock().unwrap().insert(
            "p-executor".to_string(),
            ActiveProcess {
                child: other,
                turn_id: 8,
            },
        );
        assert!(matches!(
            await_turn_exit(&active, "p-executor", 7).await,
            TurnExit::Cancelled
        ));
        assert!(take_own_process(&active, "p-executor", 7).is_none());
        let mut newer = take_own_process(&active, "p-executor", 8).expect("newer child");
        kill_process_tree(&mut newer.child);
    }
}
