//! The pair's turn driver: one agent's turn, end to end, on the direct
//! connection.
//!
//! Nothing here spawns a process. A turn resolves its model from the credential
//! registry, streams the reply through `harness::run_turn`, runs the tools the
//! model asks for, and lands on the same end-of-turn decision every other
//! transport used to — so the pair loop around it (acceptance, plan gate,
//! iteration budget, snapshots, insights) is unchanged.
//!
//! # Turn lifecycle
//!
//! ```text
//! trigger_turn ──▶ run_direct_turn ──▶ harness::run_turn ──▶ complete_turn
//!                      │                                     │
//!                      │ cancel flag ◀── bump_run_generation  └─▶ decide_turn_outcome
//!                      └── history replay/write-back                └─▶ apply + persist
//! ```
//!
//! Every state change a turn makes after its output ends goes through
//! `TurnContext::if_current`, so a paused, killed, deleted or superseded turn
//! can never touch the pair again.
//!
//! # What the pair is told while a turn streams
//!
//! Every streamed chunk still reaches the console as a log line. What is paced
//! is the pair state: the broker's `pair:state` event carries the whole of it —
//! every message, cognitive event, modified file and acceptance record — so a
//! 4k-token reply published it 4k times and re-rendered the pair list per token.
//! [`StatePace`] holds a publish back to roughly one a second and the turn
//! flushes the last one when it ends, so the state still converges.

use crate::acceptance::{
    canonical_acceptance_verdict_json, parse_review_verdict_with_quality, run_acceptance_checks,
    should_stop_iteration,
};
use crate::message_broker::MessageBroker;
use crate::session_snapshot::persist_current_pair_snapshot;
use crate::types::{
    AcceptanceNextAction, AcceptanceNextStep, AcceptanceRecord, AcceptanceRisk, AcceptanceVerdict,
    AcceptanceVerdictDecision, ActivityPhase, AgentRole, Message, MessageSender, MessageType,
    PairStatus, TurnTokenUsage,
};
use crate::util::{is_mock_mode, now_millis};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

/// `pair_id` → the pair's turn state.
pub type ProcessContextMap = Arc<Mutex<HashMap<String, ProcessContext>>>;

#[derive(Clone)]
pub struct TurnDriver {
    pub pair_contexts: ProcessContextMap,
    /// Set when a turn must stop: the stream is cancelled where it is instead
    /// of being read to the end.
    pub turn_cancels: TurnCancelMap,
}

/// `"{pair_id}-{role}"` → the flag its in-flight turn checks.
pub type TurnCancelMap = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

/// How often a streaming turn may publish the pair's state.
///
/// The broker's `pair:state` event carries the whole pair state, so publishing
/// per token serialized it once per token; a second is still far finer than
/// anything a human reads, and the stall detector only needs `last_output_at`
/// to move inside its 120 s warning.
const STATE_PUBLISH_INTERVAL: Duration = Duration::from_millis(1000);

/// Holds a streaming turn's state publishes back to [`STATE_PUBLISH_INTERVAL`].
///
/// The turn still sees every chunk — the console log is written per chunk — but
/// the activity, the progress counter and the reasoning events ride on the
/// pair state, so they are published on the pace instead. `needs_flush` says
/// whether something was held back, and the turn publishes it when it ends so
/// the state the pair is left in is the state of the last chunk.
#[derive(Default)]
struct StatePace {
    last: Option<Instant>,
    held: bool,
}

impl StatePace {
    /// Whether a publish may go out now. The first one always may, so a turn
    /// whose whole output is one chunk still shows it.
    fn due(&mut self) -> bool {
        let now = Instant::now();
        let due = match self.last {
            Some(last) => now.duration_since(last) >= STATE_PUBLISH_INTERVAL,
            None => true,
        };
        match due {
            true => {
                self.last = Some(now);
                self.held = false;
            }
            false => self.held = true,
        }
        due
    }

    /// Whether the last chunk was held back and still has to be published.
    fn needs_flush(&self) -> bool {
        self.held
    }

    /// Records a publish that happened outside [`Self::due`] — the turn's final
    /// flush — so nothing is owed any more.
    fn published(&mut self) {
        self.last = Some(Instant::now());
        self.held = false;
    }
}
/// How long a turn about to start waits for the cancelled turn before it to let
/// go of its (pair, role). A cancelled turn stops within a poll of its flag —
/// plus the time it takes to end a command's process tree — so this is only
/// ever reached by a turn that is stuck, and the new one starts regardless.
const UNWIND_GRACE: Duration = Duration::from_secs(5);
const UNWIND_POLL: Duration = Duration::from_millis(25);

/// Registers a turn's cancel flag, unless a live turn already holds `key`.
///
/// A flag that is still down means a turn is in flight for that (pair, role),
/// and a second turn would take the key over: the first turn's flag would then
/// be unreachable, so nothing could stop it. The frontend only serializes
/// handoffs, so a handoff racing `pair_resume`, `pair_retry_turn` or
/// `resolvePlanReview` is exactly that.
///
/// A flag that is already raised is another matter: that turn was told to stop
/// and nobody needs to reach it again, while the turn asking now is the one the
/// pair's status is waiting on. Refusing it would leave the pair looking busy
/// with nothing running — the cancelled turn changes no state on its way out —
/// so the key is taken over. [`clear_cancel`] only ever removes a turn's own
/// flag, so the old turn finishing later leaves the new one registered.
fn register_cancel(cancels: &TurnCancelMap, key: &str) -> Option<Arc<AtomicBool>> {
    let mut guard = cancels.lock().unwrap_or_else(|e| e.into_inner());
    if guard
        .get(key)
        .is_some_and(|running| !running.load(Ordering::SeqCst))
    {
        return None;
    }
    let flag = Arc::new(AtomicBool::new(false));
    guard.insert(key.to_string(), Arc::clone(&flag));
    Some(flag)
}

/// Waits, up to [`UNWIND_GRACE`], for a cancelled turn to unregister.
///
/// Resume, retry and restore cancel the turn in flight and start the next one
/// in the same breath. Giving the old turn a moment to unwind keeps two turns
/// from working in one directory at once; a turn that is still live (its flag
/// is down) is not waited for, because [`register_cancel`] refuses that case.
async fn wait_for_unwind(cancels: &TurnCancelMap, key: &str) {
    let deadline = Instant::now() + UNWIND_GRACE;
    while Instant::now() < deadline {
        let unwinding = cancels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .is_some_and(|running| running.load(Ordering::SeqCst));
        if !unwinding {
            return;
        }
        tokio::time::sleep(UNWIND_POLL).await;
    }
}

/// Raises a turn's cancel flag, if it is running.
fn raise_cancel(cancels: &TurnCancelMap, key: &str) {
    if let Some(flag) = cancels.lock().unwrap_or_else(|e| e.into_inner()).get(key) {
        flag.store(true, Ordering::SeqCst);
    }
}

/// Unregisters a turn's cancel flag — but only while it is still that turn's.
///
/// A superseded turn can finish long after the turn that replaced it registered
/// its own flag under the same `<pair>-<role>` key; removing the entry then
/// would leave the live turn with nothing to cancel.
fn clear_cancel(cancels: &TurnCancelMap, key: &str, mine: &Arc<AtomicBool>) {
    let mut guard = cancels.lock().unwrap_or_else(|e| e.into_inner());
    if guard
        .get(key)
        .is_some_and(|registered| Arc::ptr_eq(registered, mine))
    {
        guard.remove(key);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessContext {
    pub directory: String,
    pub mentor_model: String,
    pub executor_model: String,
    pub mentor_session_id: Option<String>,
    pub executor_session_id: Option<String>,
    pub mentor_reasoning_effort: Option<String>,
    pub executor_reasoning_effort: Option<String>,
    /// The direct-connection model each agent talks to. `None` means the pair
    /// has no model it can reach, so its turns fail with a readable error until
    /// a human picks one in the pair's settings.
    #[serde(default)]
    pub mentor_selection: Option<crate::harness::ModelSelection>,
    #[serde(default)]
    pub executor_selection: Option<crate::harness::ModelSelection>,
    /// The harness conversation each agent carries, in OpenAI chat form. A new
    /// run — or a role that moved to another endpoint — starts a fresh one.
    #[serde(default)]
    pub mentor_history: Vec<serde_json::Value>,
    #[serde(default)]
    pub executor_history: Vec<serde_json::Value>,
    /// Bumped whenever the current turn is invalidated: a new run, pause,
    /// kill, resume/retry and delete. A turn captures the value when it starts
    /// and does nothing to pair state once it no longer matches.
    pub run_generation: u32,
    pub is_smoke_test: bool,
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
const MENTOR_FINISH_SIGNAL: &str = "TASK_COMPLETE";

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
    /// A budget pause was already handed to the mentor and this review is its
    /// aftermath: another executor turn cannot make progress, so a `continue`
    /// verdict must finish for a human instead of ping-ponging.
    budget_review: bool,
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
            if facts.budget_review {
                // The executor already hit the iteration budget and this
                // review is the pause's aftermath. Handing back would re-hit
                // the budget check before any work — pause for a human
                // instead (assign a new task, or resume and let the mentor
                // decide how the run ends).
                return TurnDecision::Pause {
                    detail: format!(
                        "Iteration budget reached ({}/{}). The mentor reviewed the last work; assign a new task, or resume to let the mentor decide how the run ends.",
                        facts.iteration, facts.max_iterations
                    ),
                    hand_to_mentor: false,
                    ends_run: true,
                };
            }
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
            "[TurnDriver] [{}] {} turn is stale ({}); leaving pair state untouched",
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
                "[TurnDriver] [{}] Triggering handoff to {}",
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
                "[TurnDriver] [{}] Plan gate engaged — awaiting human review before executor",
                tc.pair_id
            );
            broker.set_pair_status(
                &tc.pair_id,
                PairStatus::AwaitingHumanReview,
                Some("Plan ready — review it before the executor starts.".to_string()),
            );
        }
        TurnDecision::Finish { detail } => {
            println!("[TurnDriver] [{}] Finished: {}", tc.pair_id, detail);
            broker.set_pair_status(&tc.pair_id, PairStatus::Finished, Some(detail.clone()));
        }
        TurnDecision::Pause {
            detail,
            hand_to_mentor,
            ..
        } => {
            println!("[TurnDriver] [{}] Pausing: {}", tc.pair_id, detail);
            if *hand_to_mentor {
                broker.set_turn(&tc.pair_id, AgentRole::Mentor);
            }
            broker.set_pair_status(&tc.pair_id, PairStatus::Paused, Some(detail.clone()));
        }
        TurnDecision::Error { detail } => {
            println!("[TurnDriver] [{}] Error: {}", tc.pair_id, detail);
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
                "[TurnDriver] [{}] Failed to persist snapshot: {}",
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
    let context = app.try_state::<TurnDriver>().and_then(|spawner| {
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

    // The endpoint the mentor actually talked to, which is what the insights
    // leaderboard groups by. A pair with no selection never ran a turn.
    let mentor_provider = context
        .as_ref()
        .and_then(|context| context.mentor_selection.as_ref())
        .map(|selection| selection.provider_id.clone())
        .unwrap_or_default();
    let (mentor_model, executor_model) = match context.as_ref() {
        Some(context) => (context.mentor_model.clone(), context.executor_model.clone()),
        None => (pair.mentor_model.clone(), pair.executor_model.clone()),
    };
    let mut record = crate::intelligence_store::build_run_record(
        pair_id,
        started_at,
        finished_at,
        &mentor_model,
        &executor_model,
        &mentor_provider,
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
                "[TurnDriver] [{}] Session report saved to {:?}",
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
                "[TurnDriver] [{}] Failed to write session report: {}",
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
            "[TurnDriver] [{}] WARNING: Executor attempted TASK_COMPLETE (ignored - only Mentor can finish)",
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
        budget_review: state.max_iterations > 0 && state.iteration >= state.max_iterations,
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

async fn run_direct_turn(
    app: tauri::AppHandle,
    cancels: TurnCancelMap,
    tc: TurnContext,
    selection: crate::harness::ModelSelection,
    message: String,
    directory: String,
) {
    let pair_id = tc.pair_id.clone();
    let role = tc.role.clone();
    let key = format!("{}-{}", pair_id, role);
    // The turn this one replaces — cancelled by the resume, retry or restore
    // that asked for this one — gets a moment to stop first.
    wait_for_unwind(&cancels, &key).await;
    // A live flag for this (pair, role) means a turn is in flight: taking the
    // key over would orphan its cancel flag, so this one does not start. The
    // turn already running drives the pair to its next state.
    let Some(cancel) = register_cancel(&cancels, &key) else {
        tc.log_stale("a turn is already running for this role");
        return;
    };

    tc.with_broker(|broker| {
        broker.reset_token_usage(&pair_id, &role);
        broker.update_agent_activity(
            &pair_id,
            &role,
            ActivityPhase::Thinking,
            "Connecting...".to_string(),
            None,
        );
        broker.set_turn_started_at(&pair_id, now_millis());
    });

    // A turn superseded before its first byte is not a turn at all.
    if tc.if_current(|_| true).is_none() {
        clear_cancel(&cancels, &key, &cancel);
        tc.log_stale("superseded before connecting");
        return;
    }

    let app_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(error) => {
            clear_cancel(&cancels, &key, &cancel);
            fail_direct_turn(
                &tc,
                &format!("Failed to resolve the app data dir: {}", error),
            )
            .await;
            return;
        }
    };

    // Never fall back to another directory (e.g. the user's main checkout):
    // an agent must only ever run where the pair was created. The tools would
    // refuse every path anyway, with a far less readable message.
    if !std::path::Path::new(&directory).is_dir() {
        clear_cancel(&cancels, &key, &cancel);
        fail_direct_turn(
            &tc,
            &format!(
                "The pair's working directory no longer exists: {}. If it was a worktree, its \
                 commits are kept on its the-pair/… branch; start a new pair to continue.",
                directory
            ),
        )
        .await;
        return;
    }

    // This agent's conversation: what it said in earlier turns of this run,
    // then the turn it is about to answer.
    let mut history = prior_conversation(&tc, &role);
    history.push(crate::harness::history::user(&message));
    let effort = tc
        .contexts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&pair_id)
        .and_then(|context| {
            if tc.role == "mentor" {
                context.mentor_reasoning_effort.clone()
            } else {
                context.executor_reasoning_effort.clone()
            }
        });
    let request = crate::harness::TurnRequest {
        selection: selection.clone(),
        // The effort the pair was given for this role. It has sat on the pair's
        // context since it was picked, and every transport implements it.
        effort,
        system: direct_system_prompt(&tc.role, &directory),
        history,
        cache_key: format!("{}-{}", pair_id, role),
        cancel: Arc::clone(&cancel),
        directory: directory.clone(),
        policy: if tc.role == "mentor" {
            crate::tools::ToolPolicy::ReadOnly
        } else {
            crate::tools::ToolPolicy::Full
        },
    };

    // The tool call whose arguments are streaming right now, so the console can
    // name it once instead of on every piece.
    let mut streaming_tool: Option<usize> = None;
    // What the pace has held back from the pair state, and how many reasoning
    // events this attempt already published to it. A discarded attempt's
    // reasoning goes with its text: the broker cannot retract a cognitive event,
    // so the count is what says how many would have to go.
    let mut pace = StatePace::default();
    let mut unshown_reasoning = String::new();
    // Counted per model request: a tool call starts the next one, so the tool
    // callback resets it. Otherwise a retry in a later request would retract
    // that many of the role's trailing events — the tool calls among them.
    let streamed_events = AtomicUsize::new(0);
    let outcome = crate::harness::run_turn(
        &app_dir,
        &request,
        &mut |delta| {
            if !tc.generation_is_current() {
                return crate::llm::CancelResult::Cancel;
            }
            match delta {
                crate::llm::Delta::Text(chunk) => {
                    tc.with_broker(|broker| {
                        broker.add_log_line(&pair_id, &role, chunk);
                        if pace.due() {
                            broker.update_output_progress(&pair_id, &role);
                            broker.update_agent_activity(
                                &pair_id,
                                &role,
                                ActivityPhase::Responding,
                                "Responding...".to_string(),
                                None,
                            );
                        }
                    });
                }
                crate::llm::Delta::Reasoning(chunk) => {
                    // Coalesced rather than dropped: the reasoning still reaches
                    // the timeline, as one event per interval instead of one per
                    // chunk, and whatever is left over is flushed when the turn
                    // ends.
                    unshown_reasoning.push_str(chunk);
                    if pace.due() {
                        let text = std::mem::take(&mut unshown_reasoning);
                        tc.with_broker(|broker| {
                            broker.add_cognitive_event(
                                &pair_id,
                                &role,
                                crate::types::CognitiveEventType::Reasoning,
                                None,
                                text,
                                crate::types::CognitiveEventStatus::Running,
                            );
                        });
                        streamed_events.fetch_add(1, Ordering::SeqCst);
                    }
                }
                crate::llm::Delta::Discard => {
                    // The attempt that streamed this is being thrown away, so
                    // what it published is no longer part of the turn.
                    let retracted = streamed_events.swap(0, Ordering::SeqCst);
                    tc.with_broker(|broker| {
                        broker.retract_cognitive_events(&pair_id, &role, retracted);
                    });
                    unshown_reasoning.clear();
                }
                crate::llm::Delta::Tool { index, name, .. } => {
                    if streaming_tool != Some(index) {
                        streaming_tool = Some(index);
                        let label = format!("Calling {}...", name);
                        tc.with_broker(|broker| {
                            broker.update_agent_activity(
                                &pair_id,
                                &role,
                                ActivityPhase::UsingTools,
                                label,
                                None,
                            );
                        });
                    }
                }
                crate::llm::Delta::Retry {
                    attempt,
                    wait,
                    reason,
                } => {
                    println!(
                        "[TurnDriver] [{}] {} retry {} in {:?}: {}",
                        pair_id, role, attempt, wait, reason
                    );
                    tc.with_broker(|broker| {
                        broker.add_log_line(
                            &pair_id,
                            &role,
                            &format!("[retry {} in {:?}] {}", attempt, wait, reason),
                        );
                    });
                }
            }
            crate::llm::CancelResult::Continue
        },
        &mut |event| {
            if !tc.generation_is_current() {
                return crate::llm::CancelResult::Cancel;
            }
            match event {
                crate::harness::ToolEvent::Started { name, summary, .. } => {
                    // The reasoning published so far belongs to a request that
                    // completed: it is part of the turn and stays.
                    streamed_events.store(0, Ordering::SeqCst);
                    tc.with_broker(|broker| {
                        broker.add_cognitive_event(
                            &pair_id,
                            &role,
                            crate::types::CognitiveEventType::ToolCall,
                            Some(name.to_string()),
                            summary.to_string(),
                            crate::types::CognitiveEventStatus::Running,
                        );
                        broker.update_agent_activity(
                            &pair_id,
                            &role,
                            ActivityPhase::UsingTools,
                            summary.to_string(),
                            None,
                        );
                    });
                }
                crate::harness::ToolEvent::Finished {
                    name, ok, output, ..
                } => {
                    tc.with_broker(|broker| {
                        broker.add_cognitive_event(
                            &pair_id,
                            &role,
                            crate::types::CognitiveEventType::ToolCall,
                            Some(name.to_string()),
                            if ok {
                                output.to_string()
                            } else {
                                format!("failed: {}", output)
                            },
                            crate::types::CognitiveEventStatus::Completed,
                        );
                        // A tool's output is the turn's progress: without this the
                        // stall detector sees a silent agent between model calls.
                        broker.update_output_progress(&pair_id, &role);
                    });
                }
            }
            crate::llm::CancelResult::Continue
        },
    )
    .await;

    clear_cancel(&cancels, &key, &cancel);

    // The last of what the turn streamed has to reach the pair state even though
    // the pace held it back: the broker converges on the state of the last chunk,
    // and the next turn starts from it. A turn that is no longer the pair's
    // leaves the state as the human left it.
    if pace.needs_flush() && tc.if_current(|_| true).is_some() {
        if !unshown_reasoning.is_empty() {
            let text = std::mem::take(&mut unshown_reasoning);
            tc.with_broker(|broker| {
                broker.add_cognitive_event(
                    &pair_id,
                    &role,
                    crate::types::CognitiveEventType::Reasoning,
                    None,
                    text,
                    crate::types::CognitiveEventStatus::Running,
                );
            });
        }
        tc.with_broker(|broker| {
            broker.update_output_progress(&pair_id, &role);
            broker.update_agent_activity(
                &pair_id,
                &role,
                ActivityPhase::Responding,
                "Responding...".to_string(),
                None,
            );
        });
        pace.published();
    }

    let turn_output = match outcome {
        Ok(outcome) => {
            if outcome.text.trim().is_empty() {
                // The tools already ran, so their calls and results are the only
                // record of what this turn changed: store them, or the next turn
                // replays a conversation that never mentions them.
                if !outcome.tool_calls.is_empty() {
                    remember_history(&tc, &outcome.history);
                }
                fail_direct_turn(&tc, "the model returned no text").await;
                return;
            }
            remember_session(&tc, outcome.session_id.as_deref());
            // The exchange this turn added is what the next one replays. Only
            // the live run writes it, so a superseded turn leaves the pair's
            // conversation as it was.
            remember_history(&tc, &outcome.history);
            TurnOutput {
                final_output: outcome.text,
                no_text_output: false,
                turn_error: None,
                token_usage: turn_token_usage(outcome.usage, &role, &selection.provider_id),
            }
        }
        Err(failure) => {
            if failure.error.is_cancelled() {
                tc.log_stale("turn was stopped");
                return;
            }
            // The tools that ran before the failure changed the workspace, and
            // their calls and results are the only record of it: store them, or
            // a retry replays a conversation that never mentions them and does
            // the same work again on files that are already edited.
            if !failure.history.is_empty() {
                remember_history(&tc, &failure.history);
            }
            // A credential the provider just refused is expired as far as the
            // Models section is concerned, so it asks for a sign-in. The turn
            // names the model that was refused — the backup, when it stepped in
            // and was the one turned away.
            if failure.error.kind == crate::llm::ErrorKind::Auth {
                crate::harness::mark_account_expired(
                    &app_dir,
                    &failure.selection.provider_id,
                    &failure.selection.account_id,
                );
            }
            fail_direct_turn(&tc, &failure.error.to_string()).await;
            return;
        }
    };

    complete_turn(tc, turn_output).await;
}

/// Records the provider's own session id on the pair, so a later run can be
/// correlated with it. Only the live run may write it.
fn remember_session(tc: &TurnContext, session_id: Option<&str>) {
    let Some(session_id) = session_id else {
        return;
    };
    let mut guard = tc.contexts.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(context) = guard
        .get_mut(&tc.pair_id)
        .filter(|context| context.run_generation == tc.run_generation)
    {
        let slot = if tc.role == "mentor" {
            &mut context.mentor_session_id
        } else {
            &mut context.executor_session_id
        };
        if slot.as_deref() != Some(session_id) {
            *slot = Some(session_id.to_string());
        }
    }
}

/// The conversation this agent's earlier turns of the current run left behind.
fn prior_conversation(tc: &TurnContext, role: &str) -> Vec<serde_json::Value> {
    let guard = tc.contexts.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get(&tc.pair_id)
        .map(|context| {
            if role == "mentor" {
                context.mentor_history.clone()
            } else {
                context.executor_history.clone()
            }
        })
        .unwrap_or_default()
}

/// Stores the conversation a finished turn produced, so the next one replays
/// it. Only the live run may write it.
fn remember_history(tc: &TurnContext, history: &[serde_json::Value]) {
    let mut guard = tc.contexts.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(context) = guard
        .get_mut(&tc.pair_id)
        .filter(|context| context.run_generation == tc.run_generation)
    {
        if tc.role == "mentor" {
            context.mentor_history = history.to_vec();
        } else {
            context.executor_history = history.to_vec();
        }
    }
}

/// Ends a turn that never produced a usable reply, with the same shape a failed
/// provider turn reports.
async fn fail_direct_turn(tc: &TurnContext, detail: &str) {
    tc.with_broker(|broker| {
        broker.update_agent_activity(
            &tc.pair_id,
            &tc.role,
            ActivityPhase::Responding,
            "Failed".to_string(),
            None,
        );
    });
    complete_turn(
        tc.clone(),
        TurnOutput {
            final_output: format!("{} reported an error: {}", tc.role, detail),
            no_text_output: true,
            turn_error: Some(detail.to_string()),
            token_usage: None,
        },
    )
    .await;
}

/// The turn's token usage, in the shape the broker reports for every transport.
fn turn_token_usage(
    usage: crate::llm::Usage,
    role: &str,
    provider_id: &str,
) -> Option<crate::types::TurnTokenUsage> {
    Some(crate::types::TurnTokenUsage {
        output_tokens: usage.completion_tokens,
        input_tokens: Some(usage.prompt_tokens),
        last_updated_at: now_millis(),
        source: crate::types::TokenUsageSource::Final,
        provider: Some(format!("{}:{}", provider_id, role)),
    })
}

/// The system prompt a direct turn runs under: the agent's role and its
/// output contract, then the workspace it is working in.
///
/// The pair's own instructions arrive as the turn's message — the frontend
/// builds the full handoff prompt — so the system prompt only has to state the
/// role and the output contract `context_bridge` and `acceptance` parse.
fn direct_system_prompt(role: &str, directory: &str) -> String {
    let mut prompt = if role == "mentor" {
        "You are the mentor in an automated pair-programming workflow with one other AI agent, \
         the executor. You plan and review; you never edit files or run commands yourself. You \
         have read, grep, glob and the task list, and no shell: every check you perform is \
         something you verified by reading.\n\
         \n\
         When you review the executor's work, end your reply with these two blocks, in this \
         order:\n\
         \n\
         FILES_REVIEWED: <the files you actually read, comma-separated>\n\
         CHECKS: <what you verified by reading them, comma-separated — the error path you \
         traced, the types you compared, the plan step you checked against. Never \"none\" and \
         never a command you did not run>\n\
         CODE: <one concrete line or symbol you based the decision on>\n\
         \n\
         {\"verdict\": \"pass\"|\"fail\", \"risk\": \"low\"|\"medium\"|\"high\", \
         \"confidence\": 0.0-1.0, \"issues\": [\"…\"], \"evidence\": [\"…\"], \
         \"reasoning\": \"…\", \"summary\": \"…\", \
         \"nextStep\": {\"action\": \"continue\"|\"finish\", \"instructions\": [\"…\"]}}\n\
         \n\
         Never claim a file, a check or a result you did not actually see."
            .to_string()
    } else {
        "You are the executor in an automated pair-programming workflow with one other AI \
         agent, the mentor. You carry out the work the mentor planned, in the pair's working \
         directory, and report what you changed.\n\
         \n\
         Report concisely and factually: what you changed, what you verified by running it, \
         and anything you could not do. Never claim a command you did not run."
            .to_string()
    };
    prompt.push_str(&environment_block(directory));
    prompt
}

/// Where the agent is working, stated the way killi's own skeleton states it:
/// the machine, the directory, the day, and the workspace's own conventions.
///
/// A model that does not know it is on macOS, in this repository, today, asks
/// for the wrong paths and reasons about the wrong platform.
fn environment_block(directory: &str) -> String {
    let mut block = format!(
        "\n\n# Environment\n\n- Operating system: {} ({})\n- Working directory: {}\n- Date: {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        directory,
        today(),
    );
    if let Some(agents) = workspace_agents_md(directory) {
        block.push_str("\n\n# Workspace instructions (AGENTS.md)\n\n");
        block.push_str(&agents);
    }
    block
}

/// The workspace's `AGENTS.md`, when it has one, capped so a huge file cannot
/// crowd out the turn itself.
fn workspace_agents_md(directory: &str) -> Option<String> {
    const MAX_BYTES: usize = 16 * 1024;
    let path = std::path::Path::new(directory).join("AGENTS.md");
    let text = std::fs::read_to_string(&path).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    Some(match text.char_indices().nth(MAX_BYTES) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
    })
}

/// Today's date as `YYYY-MM-DD`, from the same clock the rest of the app uses.
fn today() -> String {
    let days = (crate::util::now_millis() / 86_400_000) as i64;
    let (year, month, day) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", year, month, day)
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's `civil_from_days`,
/// so the app needs no date library for one line of a system prompt.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (day_of_year * 5 + 2) / 153;
    let day = day_of_year - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month as u32, day as u32)
}
impl TurnDriver {
    pub fn new() -> Self {
        Self {
            pair_contexts: Arc::new(Mutex::new(HashMap::new())),
            turn_cancels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Invalidate the pair's current turn: its reader will make no further
    /// state changes and its handoff is dropped.
    pub fn bump_run_generation(&self, pair_id: &str) {
        // A new run supersedes whatever is in flight, so both roles stop.
        for role in ["mentor", "executor"] {
            raise_cancel(&self.turn_cancels, &format!("{}-{}", pair_id, role));
        }
        let mut contexts = self.pair_contexts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(context) = contexts.get_mut(pair_id) {
            context.run_generation = context.run_generation.wrapping_add(1);
        }
    }

    /// Stop the pair's live turn: its stream is cancelled where it is and it
    /// makes no further state changes.
    pub fn stop_process(&self, pair_id: &str, role: &str) {
        raise_cancel(&self.turn_cancels, &format!("{}-{}", pair_id, role));
    }

    /// Stop both roles' turns for a pair.
    pub fn stop_pair_processes(&self, pair_id: &str) {
        for role in ["mentor", "executor"] {
            self.stop_process(pair_id, role);
        }
    }

    /// Start one agent's turn. A pair whose endpoint and credential are both in
    /// the registry talks to the model directly; every other pair replays
    /// canned output in mock mode, which is what the e2e layers drive.
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

        let selection = if role == "mentor" {
            ctx.mentor_selection.clone()
        } else {
            ctx.executor_selection.clone()
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

        // Mock mode first: a smoke run must replay canned output, never reach a
        // real endpoint the developer happens to have a credential for.
        if is_mock_mode() {
            println!(
                "[TurnDriver] [MOCK] {} {} (mock mode enabled)",
                pair_id, role
            );
            tokio::spawn(run_mock_turn(turn));
            return Ok(());
        }

        let Some(selection) = selection.filter(|selection| selection.is_usable()) else {
            // Every pair now runs on the direct connection; reaching here means
            // the pair has no model it can talk to, so say so rather than
            // spawning anything.
            return Err(format!(
                "The {} has no model selected. Pick one in the pair's settings.",
                role
            ));
        };

        println!(
            "[TurnDriver] [{}] {} on the direct connection: {}/{}",
            pair_id, role, selection.provider_id, selection.model
        );
        tokio::spawn(run_direct_turn(
            app,
            self.turn_cancels.clone(),
            turn,
            selection,
            message,
            ctx.directory.clone(),
        ));
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The role contract has to survive the move to the harness, because
    /// `acceptance` and `quality_gate` parse what the mentor writes.
    #[test]
    fn the_mentor_contract_names_the_markers_acceptance_parses() {
        let system = direct_system_prompt("mentor", "/repo");
        for marker in [
            "FILES_REVIEWED:",
            "CHECKS:",
            "CODE:",
            "\"verdict\"",
            "\"nextStep\"",
        ] {
            assert!(system.contains(marker), "missing {} in {}", marker, system);
        }
        assert!(system.contains("Never claim"));
        assert!(!system.contains("review this"));
        // A read-only mentor has no shell, so "none" is the honest answer a
        // model reaches for — and the one the quality gate rejects.
        assert!(!system.contains("or none"), "{}", system);
        assert!(system.contains("Never \"none\""), "{}", system);
    }

    #[test]
    fn the_executor_contract_asks_for_a_factual_report() {
        let system = direct_system_prompt("executor", "/repo");
        assert!(system.contains("executor"));
        assert!(system.contains("Never claim a command you did not run"));
        assert!(
            !system.contains("FILES_REVIEWED:"),
            "that is the mentor's contract"
        );
    }

    #[test]
    fn the_prompt_states_the_workspace_the_agent_is_working_in() {
        let system = direct_system_prompt("executor", "/repo/pair-1");
        assert!(system.contains("# Environment"), "{}", system);
        assert!(
            system.contains("Working directory: /repo/pair-1"),
            "{}",
            system
        );
        assert!(system.contains("Operating system: "), "{}", system);
        assert!(
            system.lines().any(|line| line.starts_with("- Date: 20")),
            "today's date, YYYY-MM-DD: {}",
            system
        );
    }

    #[test]
    fn the_prompt_carries_the_workspaces_own_instructions() {
        let dir = std::env::temp_dir().join(format!("the-pair-agents-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "# Workspace rules\n\nRun npm test.").unwrap();

        let system = direct_system_prompt("executor", &dir.display().to_string());
        assert!(system.contains("Run npm test."), "{}", system);
        assert!(system.contains("# Workspace instructions (AGENTS.md)"));

        // A workspace without one simply gets no section.
        let bare = std::env::temp_dir().join(format!("the-pair-bare-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&bare).unwrap();
        let system = direct_system_prompt("executor", &bare.display().to_string());
        assert!(!system.contains("AGENTS.md"), "{}", system);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&bare).ok();
    }

    #[test]
    fn a_huge_agents_file_is_capped_rather_than_inlined_whole() {
        let dir = std::env::temp_dir().join(format!("the-pair-fat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "x".repeat(40 * 1024)).unwrap();

        let inlined = workspace_agents_md(&dir.display().to_string()).unwrap();
        assert!(inlined.ends_with('\u{2026}'), "capped with a marker");
        assert!(inlined.len() < 20 * 1024, "{} bytes", inlined.len());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_streaming_turn_publishes_its_state_at_most_once_a_second() {
        let mut pace = StatePace::default();
        assert!(pace.due(), "the first chunk is published at once");
        assert!(!pace.due(), "the ones after it are held back");
        assert!(pace.needs_flush(), "so the pair state is owed a publish");

        // Once the interval has passed the next chunk goes out, and nothing is
        // owed any more — which is what the turn checks before it ends.
        pace.last = Some(Instant::now() - STATE_PUBLISH_INTERVAL - Duration::from_millis(1));
        assert!(pace.due());
        assert!(!pace.needs_flush());
    }

    #[test]
    fn a_second_turn_for_the_same_pair_and_role_cannot_register_a_flag() {
        let cancels: TurnCancelMap = Arc::new(Mutex::new(HashMap::new()));
        let running = register_cancel(&cancels, "pair-1-mentor").expect("the first turn");

        // A handoff racing a resume, a retry or a plan decision would take the
        // key over and orphan the running turn's flag.
        assert!(register_cancel(&cancels, "pair-1-mentor").is_none());
        assert!(
            register_cancel(&cancels, "pair-1-executor").is_some(),
            "the other role is another turn"
        );

        // The running turn is still the one that gets cancelled, and the key is
        // free again once it clears its own flag.
        raise_cancel(&cancels, "pair-1-mentor");
        assert!(running.load(Ordering::SeqCst));
        clear_cancel(&cancels, "pair-1-mentor", &running);
        assert!(register_cancel(&cancels, "pair-1-mentor").is_some());
    }

    #[test]
    fn a_turn_takes_the_key_over_from_one_that_was_cancelled() {
        let cancels: TurnCancelMap = Arc::new(Mutex::new(HashMap::new()));
        let paused = register_cancel(&cancels, "pair-1-executor").expect("the first turn");

        // Pause raises the flag; the turn is still inside a tool call and has
        // not unregistered. Resume must not be dropped for that: the pair is
        // already Executing again and this is the turn it is waiting on.
        raise_cancel(&cancels, "pair-1-executor");
        let resumed = register_cancel(&cancels, "pair-1-executor").expect("the resumed turn");
        assert!(!resumed.load(Ordering::SeqCst), "it starts uncancelled");

        // The old turn unwinding later leaves the new one registered and
        // reachable.
        clear_cancel(&cancels, "pair-1-executor", &paused);
        raise_cancel(&cancels, "pair-1-executor");
        assert!(resumed.load(Ordering::SeqCst));
        clear_cancel(&cancels, "pair-1-executor", &resumed);
        assert!(cancels.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_starting_turn_waits_for_the_cancelled_one_to_unwind() {
        let cancels: TurnCancelMap = Arc::new(Mutex::new(HashMap::new()));
        let old = register_cancel(&cancels, "pair-1-mentor").expect("the first turn");
        raise_cancel(&cancels, "pair-1-mentor");

        let unwinding = Arc::clone(&cancels);
        let flag = Arc::clone(&old);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            clear_cancel(&unwinding, "pair-1-mentor", &flag);
        });

        let started = Instant::now();
        wait_for_unwind(&cancels, "pair-1-mentor").await;
        assert!(started.elapsed() >= Duration::from_millis(60));
        assert!(
            started.elapsed() < UNWIND_GRACE,
            "it did not sit out the grace"
        );
        assert!(cancels.lock().unwrap().is_empty());

        // A live turn is not waited for at all: registering refuses it instead.
        let _live = register_cancel(&cancels, "pair-1-mentor").expect("a live turn");
        let started = Instant::now();
        wait_for_unwind(&cancels, "pair-1-mentor").await;
        assert!(started.elapsed() < Duration::from_millis(60));
        assert!(register_cancel(&cancels, "pair-1-mentor").is_none());
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // A leap day either side of a century boundary.
        assert_eq!(civil_from_days(18_321), (2020, 2, 29));
        assert_eq!(civil_from_days(20_513), (2026, 3, 1));
    }

    #[test]
    fn a_finish_signal_only_counts_on_a_line_of_its_own() {
        assert!(has_signal_token_on_own_line(
            "`TASK_COMPLETE`",
            "TASK_COMPLETE"
        ));
        assert!(!has_signal_token_on_own_line(
            "TASK_COMPLETE please",
            "TASK_COMPLETE"
        ));
        // A mentor that finishes mid-sentence during a greeting smoke run is
        // not signalling anything.
        assert!(!has_signal_token_on_own_line(
            "Ready to receive remaining greetings.",
            MENTOR_FINISH_SIGNAL
        ));
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
    fn mock_dev_smoke_executor_advances_greetings_by_iteration() {
        // The desktop e2e suite drives this scenario, so the mapping from
        // iteration to greeting has to stay deterministic.
        assert_eq!(
            mock_responses_for_scenario("executor", 1, "dev-smoke"),
            vec!["Greeting 1/3"]
        );
        assert_eq!(
            mock_responses_for_scenario("executor", 2, "dev-smoke"),
            vec!["Greeting 2/3"]
        );
        assert_eq!(
            mock_responses_for_scenario("mentor", 1, "dev-smoke"),
            vec!["Send Greeting 1/3"]
        );
        // The verdict keeps the loop going until the third greeting finishes it.
        let verdict = mock_dev_smoke_verdict(1);
        assert!(verdict.contains("\"action\":\"continue\""), "{}", verdict);
        assert!(mock_dev_smoke_verdict(3).contains("TASK_COMPLETE"));
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
            budget_review: false,
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
    fn budget_review_with_a_continue_verdict_pauses_instead_of_ping_ponging() {
        // After a budget pause handed to the mentor, a `continue` verdict must
        // not hand back to an executor that re-hits the budget before working.
        let cont = verdict(
            AcceptanceVerdictDecision::Pass,
            AcceptanceNextAction::Continue,
            0.8,
        );
        let mut f = facts("mentor", true, "{}");
        f.verdict = Some(&cont);
        f.iteration = 3;
        f.max_iterations = 3;
        f.budget_review = true;
        assert!(matches!(
            decide_turn_outcome(&f),
            TurnDecision::Pause {
                ends_run: true,
                hand_to_mentor: false,
                ..
            }
        ));

        // Same review below budget still hands off normally.
        f.budget_review = false;
        assert_eq!(
            decide_turn_outcome(&f),
            TurnDecision::Handoff {
                next_role: "executor"
            }
        );
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
}
