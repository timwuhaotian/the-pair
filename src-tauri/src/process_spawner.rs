use crate::acceptance::{
    canonical_acceptance_verdict_json, parse_acceptance_verdict, run_acceptance_checks,
};
use crate::message_broker::MessageBroker;
use crate::provider_adapter::{ProviderAdapter, ProviderTurnRequest};
use crate::provider_registry::{cli_environment_overrides, homedir, ProviderKind};
use crate::session_snapshot::persist_current_pair_snapshot;
use crate::types::{
    AcceptanceRecord, ActivityPhase, PairStatus, TokenUsageSource, TurnTokenUsage,
};
use crate::util::is_mock_mode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

#[derive(Clone)]
pub struct ProcessSpawner {
    pub active_processes: Arc<Mutex<HashMap<String, Child>>>,
    pub pair_contexts: Arc<Mutex<HashMap<String, ProcessContext>>>,
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
    /// Monotonically increasing counter bumped on every new-run assignment.
    /// Stale spawned tasks compare their captured value against the current one
    /// to avoid emitting handoff events after their process was killed.
    pub run_generation: u32,
}

const MENTOR_FINISH_SIGNAL: &str = "TASK_COMPLETE";
const EMPTY_OUTPUT_PLACEHOLDER: &str = "(No textual output produced)";

fn mock_responses(role: &str, iteration: u32) -> Vec<String> {
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
        last_updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
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
    event
        .get("sessionID")
        .and_then(|s| s.as_str())
        .or_else(|| event.get("session_id").and_then(|s| s.as_str()))
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

fn extract_gemini_event_texts(event: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(candidates) = event.get("candidates").and_then(|value| value.as_array()) {
        for candidate in candidates {
            collect_text_candidates(candidate, &mut out);
        }
    }

    if let Some(server_content) = event.get("serverContent") {
        if let Some(model_turn) = server_content.get("modelTurn") {
            collect_text_candidates(model_turn, &mut out);
        } else {
            collect_text_candidates(server_content, &mut out);
        }
    }

    if let Some(model_turn) = event.get("modelTurn") {
        collect_text_candidates(model_turn, &mut out);
    }

    if out.is_empty() {
        collect_text_candidates(event, &mut out);
    }

    out
}

fn extract_token_usage_from_claude(event: &serde_json::Value) -> Option<TurnTokenUsage> {
    let event_type = event.get("type").and_then(|v| v.as_str())?;

    let (usage_obj, is_final) = match event_type {
        "result" => {
            let usage = event.get("usage")?;
            (usage, true)
        }
        "content_block_delta" | "content_block_stop" => {
            let usage = event.get("usage")?;
            (usage, false)
        }
        _ => return None,
    };

    let output_tokens = usage_obj
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| usage_obj.get("completion_tokens").and_then(|v| v.as_u64()))?;

    let input_tokens = usage_obj
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| usage_obj.get("prompt_tokens").and_then(|v| v.as_u64()));

    Some(TurnTokenUsage {
        output_tokens,
        input_tokens,
        last_updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        source: if is_final {
            TokenUsageSource::Final
        } else {
            TokenUsageSource::Live
        },
        provider: Some("claude".to_string()),
    })
}

fn extract_token_usage_from_codex(event: &serde_json::Value) -> Option<TurnTokenUsage> {
    let usage = event.get("usage")?;

    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| usage.get("output_tokens").and_then(|v| v.as_u64()))?;

    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| usage.get("input_tokens").and_then(|v| v.as_u64()));

    Some(TurnTokenUsage {
        output_tokens,
        input_tokens,
        last_updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        source: TokenUsageSource::Final,
        provider: Some("codex".to_string()),
    })
}

fn extract_token_usage_from_opencode(event: &serde_json::Value) -> Option<TurnTokenUsage> {
    let usage = event.get("usage")?;

    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(|v| v.as_u64())?;

    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(|v| v.as_u64());

    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let is_final = event_type == "result" || event_type == "complete" || event_type == "done";

    Some(TurnTokenUsage {
        output_tokens,
        input_tokens,
        last_updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        source: if is_final {
            TokenUsageSource::Final
        } else {
            TokenUsageSource::Live
        },
        provider: Some("opencode".to_string()),
    })
}

fn extract_token_usage_from_gemini(event: &serde_json::Value) -> Option<TurnTokenUsage> {
    let usage = event.get("usageMetadata")?;

    let output_tokens = usage
        .get("candidatesTokenCount")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_u64())?;

    let input_tokens = usage
        .get("promptTokenCount")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_u64());

    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let is_final = event_type == "result" || event_type == "complete" || event_type == "done";

    Some(TurnTokenUsage {
        output_tokens,
        input_tokens,
        last_updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        source: if is_final {
            TokenUsageSource::Final
        } else {
            TokenUsageSource::Live
        },
        provider: Some("gemini".to_string()),
    })
}

fn extract_token_usage(
    provider_kind: ProviderKind,
    event: &serde_json::Value,
) -> Option<TurnTokenUsage> {
    match provider_kind {
        ProviderKind::Claude => extract_token_usage_from_claude(event),
        ProviderKind::Codex => extract_token_usage_from_codex(event),
        ProviderKind::Opencode => extract_token_usage_from_opencode(event),
        ProviderKind::Gemini => extract_token_usage_from_gemini(event),
    }
}

fn extract_claude_final_output(event: &serde_json::Value) -> Option<String> {
    let event_type = event.get("type").and_then(|value| value.as_str())?;
    if event_type != "result" {
        return None;
    }

    event
        .get("result")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn collect_json_candidates_for_provider(
    provider_kind: ProviderKind,
    event: &serde_json::Value,
    out: &mut Vec<String>,
) {
    if provider_kind == ProviderKind::Claude {
        if let Some(text) = extract_claude_final_output(event) {
            push_trimmed(out, &text);
        }
        return;
    }

    let event_type = event
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if is_noise_event_type_for_final(event_type) {
        return;
    }

    let texts = if provider_kind == ProviderKind::Gemini {
        extract_gemini_event_texts(event)
    } else {
        extract_event_texts(event)
    };

    for text in texts {
        if !is_noise_text_candidate(&text) {
            push_trimmed(out, &text);
        }
    }
}

fn collapse_candidates(candidates: &[String]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    if candidates.windows(2).all(|window| window[0] == window[1]) {
        return Some(candidates[0].clone());
    }

    let joined = candidates.join("\n");
    let longest = candidates
        .iter()
        .max_by_key(|item| item.len())
        .map(|item| item.to_string())
        .unwrap_or_default();

    // Heuristic: stream snapshots tend to repeat full content each event.
    if longest.len().saturating_mul(2) >= joined.len() {
        return Some(longest);
    }

    Some(joined)
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
) -> AcceptanceVerdictOutcome {
    let Some(mut acceptance) = existing_acceptance else {
        return AcceptanceVerdictOutcome {
            parsed_acceptance: None,
            acceptance_error: Some("Missing acceptance record for mentor review".to_string()),
            stored_output,
        };
    };

    match parse_acceptance_verdict(raw_output) {
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

async fn maybe_run_executor_acceptance(
    app: &tauri::AppHandle,
    pair_id: &str,
    final_output: &str,
) -> Result<Option<AcceptanceRecord>, String> {
    use crate::message_broker::MessageBroker;
    use tauri::Manager;

    let state = {
        let Some(broker_state) = app.try_state::<Mutex<MessageBroker>>() else {
            return Ok(None);
        };
        let broker = broker_state.lock().unwrap();
        broker.get_state(pair_id)
    };

    let Some(state) = state else {
        return Ok(None);
    };

    let acceptance = run_acceptance_checks(
        std::path::Path::new(&state.directory),
        &state.modified_files,
        final_output,
        state.iteration,
        state.max_iterations,
    )
    .await;

    if let Some(broker_state) = app.try_state::<Mutex<MessageBroker>>() {
        let broker = broker_state.lock().unwrap();
        broker.set_latest_acceptance(pair_id, Some(acceptance.clone()));
    }

    Ok(Some(acceptance))
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

impl ProcessSpawner {
    pub fn new() -> Self {
        Self {
            active_processes: Arc::new(Mutex::new(HashMap::new())),
            pair_contexts: Arc::new(Mutex::new(HashMap::new())),
        }
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
            let guard = contexts.lock().unwrap();
            let c = guard
                .get(&pair_id)
                .ok_or_else(|| format!("Context not found for pair {}", pair_id))?;
            (
                c.directory.clone(),
                c.mentor_provider,
                c.executor_provider,
                c.mentor_model.clone(),
                c.executor_model.clone(),
                c.mentor_session_id.clone(),
                c.executor_session_id.clone(),
                c.mentor_reasoning_effort.clone(),
                c.executor_reasoning_effort.clone(),
                c.run_generation,
            )
        };

        let (
            directory,
            mentor_provider,
            executor_provider,
            mentor_model,
            executor_model,
            mentor_sid,
            executor_sid,
            mentor_reasoning_effort,
            executor_reasoning_effort,
            captured_run_generation,
        ) = ctx;
        let (model, session_id, provider_kind, reasoning_effort) = if role == "mentor" {
            (
                mentor_model.as_str(),
                mentor_sid.as_deref(),
                mentor_provider,
                mentor_reasoning_effort.as_deref(),
            )
        } else {
            (
                executor_model.as_str(),
                executor_sid.as_deref(),
                executor_provider,
                executor_reasoning_effort.as_deref(),
            )
        };

        // ── Mock mode: skip real process spawn ──
        if is_mock_mode() {
            println!(
                "[ProcessSpawner] [MOCK] {} {} (mock mode enabled)",
                pair_id, role
            );

            let pair_id_mock = pair_id.clone();
            let role_mock = role.clone();
            let app_mock = app.clone();
            let _active_mock = self.active_processes.clone();

            tokio::spawn(async move {
                use crate::message_broker::MessageBroker;
                use crate::types::{ActivityPhase, Message, MessageSender, MessageType};
                use tauri::Manager;

                if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker.lock().unwrap();
                    broker.reset_token_usage(&pair_id_mock, &role_mock);
                    let now = crate::util::now_millis();
                    broker.update_agent_activity(
                        &pair_id_mock,
                        &role_mock,
                        ActivityPhase::Thinking,
                        "Starting process...".to_string(),
                        None,
                    );
                    broker.set_turn_started_at(&pair_id_mock, now);
                }

                let current_iteration =
                    if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                        let broker = broker.lock().unwrap();
                        broker
                            .get_state(&pair_id_mock)
                            .map(|s| s.iteration)
                            .unwrap_or(0)
                    } else {
                        0
                    };

                if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker.lock().unwrap();
                    broker.update_agent_activity(
                        &pair_id_mock,
                        &role_mock,
                        ActivityPhase::Responding,
                        "Processing response".to_string(),
                        None,
                    );
                }

                let mock_scenario = std::env::var("THE_PAIR_E2E_MOCK_SCENARIO")
                    .unwrap_or_else(|_| "success".to_string());

                let responses = if mock_scenario == "error" && role_mock == "executor" {
                    mock_error_response()
                } else {
                    mock_responses(&role_mock, current_iteration)
                };

                for line in &responses {
                    if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                        let broker = broker.lock().unwrap();
                        broker.add_log_line(&pair_id_mock, &role_mock, line);
                        broker.update_output_progress(&pair_id_mock, &role_mock);
                    }
                }

                if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker.lock().unwrap();
                    broker.update_agent_activity(
                        &pair_id_mock,
                        &role_mock,
                        ActivityPhase::Idle,
                        "Turn finished".to_string(),
                        None,
                    );
                }

                let final_output = responses.join("\n");
                let state_before_completion =
                    if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                        let broker = broker.lock().unwrap();
                        broker.get_state(&pair_id_mock)
                    } else {
                        None
                    };

                let is_mentor_review_turn = matches!(
                    state_before_completion.as_ref().map(|state| &state.status),
                    Some(PairStatus::Reviewing)
                ) && role_mock == "mentor";

                let (stored_output, parsed_acceptance, acceptance_error) =
                    if is_mentor_review_turn {
                        let outcome = process_mentor_review_verdict(
                            state_before_completion
                                .as_ref()
                                .and_then(|state| state.latest_acceptance.clone()),
                            &final_output,
                            final_output.clone(),
                        );
                        (outcome.stored_output, outcome.parsed_acceptance, outcome.acceptance_error)
                    } else {
                        (final_output.clone(), None, None)
                    };

                let outgoing_type = if role_mock == "mentor" {
                    if is_mentor_review_turn {
                        MessageType::Acceptance
                    } else {
                        MessageType::Plan
                    }
                } else {
                    MessageType::Result
                };

                if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker.lock().unwrap();
                    broker.add_message(
                        &pair_id_mock,
                        Message {
                            id: uuid::Uuid::new_v4().to_string(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64,
                            from: if role_mock == "mentor" {
                                MessageSender::Mentor
                            } else {
                                MessageSender::Executor
                            },
                            to: "human".to_string(),
                            msg_type: outgoing_type,
                            content: stored_output.clone(),
                            iteration: current_iteration,
                            token_usage: Some(mock_token_usage()),
                        },
                    );
                }

                if let Some(acceptance) = parsed_acceptance.clone() {
                    if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                        let broker = broker.lock().unwrap();
                        broker.set_latest_acceptance(&pair_id_mock, Some(acceptance));
                    }
                }

                if role_mock == "executor" && mock_scenario != "error" {
                    if let Ok(Some(acceptance)) =
                        maybe_run_executor_acceptance(&app_mock, &pair_id_mock, &final_output).await
                    {
                        if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                            let broker = broker.lock().unwrap();
                            broker.set_pair_status(
                                &pair_id_mock,
                                PairStatus::Reviewing,
                                Some(format!("Acceptance checks complete. {}", acceptance.summary)),
                            );
                        }
                    }
                }

                if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker.lock().unwrap();
                    broker.update_token_usage(&pair_id_mock, &role_mock, mock_token_usage());
                }

                let mut should_handoff = true;
                let mut next_role = if role_mock == "mentor" {
                    "executor"
                } else {
                    "mentor"
                };
                let mentor_finish_signaled = role_mock == "mentor"
                    && has_signal_token_on_own_line(&final_output, MENTOR_FINISH_SIGNAL);

                if role_mock == "executor"
                    && has_signal_token_on_own_line(&final_output, MENTOR_FINISH_SIGNAL)
                {
                    println!(
                        "[ProcessSpawner] [MOCK] [{}] WARNING: Executor attempted TASK_COMPLETE (ignored)",
                        pair_id_mock
                    );
                }

                if let Some(broker) = app_mock.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker.lock().unwrap();

                    if is_mentor_review_turn {
                        if let Some(acceptance) = parsed_acceptance.as_ref() {
                            if let Some(verdict) = acceptance.verdict.as_ref() {
                                if matches!(
                                    verdict.next_step.action,
                                    crate::types::AcceptanceNextAction::Finish
                                ) {
                                    broker.set_pair_status(
                                        &pair_id_mock,
                                        crate::types::PairStatus::Finished,
                                        Some("Mock: Mentor acceptance marked task finished".to_string()),
                                    );
                                    should_handoff = false;
                                }
                            } else if let Some(error) = acceptance_error.as_ref() {
                                // Verdict parse failed — retry or pause
                                let repair_attempts = acceptance.repair_attempts;
                                if repair_attempts > 1 {
                                    broker.set_pair_status(
                                        &pair_id_mock,
                                        crate::types::PairStatus::Paused,
                                        Some(format!("Acceptance verdict parse failed: {}", error)),
                                    );
                                    should_handoff = false;
                                } else {
                                    next_role = "mentor";
                                }
                            }
                            }
                        } else if role_mock == "mentor"
                        && mentor_finish_signaled
                    {
                        println!(
                            "[ProcessSpawner] [MOCK] [{}] Mentor finished, marking as Finished",
                            pair_id_mock
                        );
                        broker.set_pair_status(
                            &pair_id_mock,
                            crate::types::PairStatus::Finished,
                            Some("Mock: Mentor signaled TASK_COMPLETE".to_string()),
                        );
                        should_handoff = false;
                    } else if mock_scenario == "error" && role_mock == "executor" {
                        println!(
                            "[ProcessSpawner] [MOCK] [{}] Mock error, setting Error status",
                            pair_id_mock
                        );
                        broker.set_pair_status(
                            &pair_id_mock,
                            crate::types::PairStatus::Error,
                            Some("Mock: Permission denied".to_string()),
                        );
                        should_handoff = false;
                    } else if role_mock == "executor" {
                        if let Some(state) = broker.get_state(&pair_id_mock) {
                            if state.iteration >= state.max_iterations {
                                println!(
                                    "[ProcessSpawner] [MOCK] [{}] Max iterations reached",
                                    pair_id_mock
                                );
                                broker.set_pair_status(
                                    &pair_id_mock,
                                    crate::types::PairStatus::Paused,
                                    Some(
                                        "Mock: Max iterations reached. Awaiting human intervention."
                                            .to_string(),
                                    ),
                                );
                                should_handoff = false;
                            }
                        }
                    }
                }

                if should_handoff {
                    println!(
                        "[ProcessSpawner] [MOCK] [{}] Triggering handoff to {}",
                        pair_id_mock, next_role
                    );
                    let _ = app_mock.emit(
                        "pair:handoff",
                        serde_json::json!({
                            "pairId": pair_id_mock,
                            "nextRole": next_role
                        }),
                    );
                }
            });

            return Ok(());
        }
        // ── End mock mode ──

        let command = ProviderAdapter::build_turn_command(ProviderTurnRequest {
            provider_kind,
            model,
            session_id,
            role: &role,
            pair_id: &pair_id,
            message: &message,
            reasoning_effort,
        });
        let spec = ProviderAdapter::runtime_spec(provider_kind);
        let executable = command.executable.clone();
        let args = command.args;
        let codex_last_message_path = command.last_message_path;

        println!(
            "[ProcessSpawner] Spawning: {} {:?} (protocol: {:?})",
            executable, args, spec
        );

        let mut child_command = Command::new(&executable);
        child_command
            .args(&args)
            .current_dir(&directory)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_provider_cli_env(&mut child_command);

        let mut child = child_command
            .spawn()
            .map_err(|e| format!("Failed to spawn process: {}", e))?;

        let now = crate::util::now_millis();
        if let Some(broker) = app.try_state::<Mutex<MessageBroker>>() {
            let broker = broker.lock().unwrap();
            broker.update_agent_activity(
                &pair_id,
                &role,
                ActivityPhase::Thinking,
                "Starting process...".to_string(),
                None,
            );
            broker.set_turn_started_at(&pair_id, now);
        }

        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;
        let mut reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let pair_id_clone = pair_id.clone();
        let role_clone = role.clone();
        let codex_last_message_path_clone = codex_last_message_path.clone();
        let app_clone = app.clone();
        let active_processes_for_cleanup = self.active_processes.clone();
        let provider_kind_clone = provider_kind;
        let captured_run_gen = captured_run_generation;

        // Stderr watcher
        let pair_id_clone_err = pair_id.clone();
        let role_clone_err = role.clone();
        let app_clone_err = app.clone();
        let provider_kind_clone_err = provider_kind;
        tokio::spawn(async move {
            use crate::message_broker::MessageBroker;
            use tauri::Manager;

            while let Ok(Some(line)) = stderr_reader.next_line().await {
                println!(
                    "[ProcessSpawner] [STDERR] [{}] {}: {}",
                    pair_id_clone_err, role_clone_err, line
                );

                if provider_kind_clone_err != ProviderKind::Claude {
                    if let Some(broker) = app_clone_err.try_state::<Mutex<MessageBroker>>() {
                        let broker = broker.lock().unwrap();
                        broker.add_log_line(
                            &pair_id_clone_err,
                            &role_clone_err,
                            &format!("[STDERR] {}", line),
                        );
                    }
                }
            }
        });

        tokio::spawn(async move {
            use crate::message_broker::MessageBroker;
            use crate::types::{ActivityPhase, Message, MessageSender, MessageType};
            use tauri::Manager;

            // Reset token usage at the start of the turn
            if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                let broker = broker.lock().unwrap();
                broker.reset_token_usage(&pair_id_clone, &role_clone);
            }

            let mut first_output = true;
            let mut accumulated_plain_output = String::new();
            let mut json_candidates: Vec<String> = Vec::new();
            let mut last_token_usage: Option<TurnTokenUsage> = None;

            // Get current iteration from state
            let current_iteration =
                if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker.lock().unwrap();
                    broker
                        .get_state(&pair_id_clone)
                        .map(|s| s.iteration)
                        .unwrap_or(0)
                } else {
                    0
                };

            while let Ok(Some(line)) = reader.next_line().await {
                let mut is_internal_json = false;

                if let Some(event) = parse_json_event(&line) {
                    is_internal_json = true;
                    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let event_type_lower = event_type.to_lowercase();

                    if !event_type.is_empty() {
                        println!(
                            "[ProcessSpawner] [{}] {}: [JSON] [TYPE: {}]",
                            pair_id_clone, role_clone, event_type
                        );
                    } else {
                        println!(
                            "[ProcessSpawner] [{}] {}: [JSON]",
                            pair_id_clone, role_clone
                        );
                    }

                    collect_json_candidates_for_provider(
                        provider_kind_clone,
                        &event,
                        &mut json_candidates,
                    );

                    if let Some(usage) = extract_token_usage(provider_kind_clone, &event) {
                        last_token_usage = Some(usage.clone());
                        if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                            let broker = broker.lock().unwrap();
                            broker.update_token_usage(&pair_id_clone, &role_clone, usage);
                        }
                    }

                    if first_output {
                        if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                            let broker = broker.lock().unwrap();
                            let (phase, label, detail) = if event_type_lower.contains("tool")
                                || event_type_lower.contains("function_call")
                            {
                                let tool_name = event
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("tool");
                                (
                                    ActivityPhase::UsingTools,
                                    format!("Calling {}", tool_name),
                                    None,
                                )
                            } else if event_type_lower.contains("content_block_delta")
                                || event_type_lower.contains("text")
                                || event_type_lower.contains("content")
                            {
                                (
                                    ActivityPhase::Responding,
                                    "Processing response".to_string(),
                                    None,
                                )
                            } else if event_type_lower.contains("message_start")
                                || event_type_lower.contains("turn_start")
                            {
                                (
                                    ActivityPhase::Thinking,
                                    "Analyzing task".to_string(),
                                    None,
                                )
                            } else if event_type_lower.contains("thinking") {
                                (
                                    ActivityPhase::Thinking,
                                    "Reasoning...".to_string(),
                                    None,
                                )
                            } else if event_type_lower.contains("step_start") {
                                (
                                    ActivityPhase::Thinking,
                                    "Starting step".to_string(),
                                    None,
                                )
                            } else if event_type_lower.contains("result")
                                || event_type_lower.contains("complete")
                                || event_type_lower.contains("done")
                            {
                                (
                                    ActivityPhase::Responding,
                                    "Finalizing response".to_string(),
                                    None,
                                )
                            } else {
                                (
                                    ActivityPhase::Responding,
                                    "Processing response".to_string(),
                                    None,
                                )
                            };
                            broker.update_agent_activity(
                                &pair_id_clone,
                                &role_clone,
                                phase,
                                label,
                                detail,
                            );
                        }
                        first_output = false;
                    } else if event_type_lower.contains("tool")
                        || event_type_lower.contains("function_call")
                    {
                        if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                            let broker = broker.lock().unwrap();
                            let tool_name = event
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("tool");
                            broker.update_agent_activity(
                                &pair_id_clone,
                                &role_clone,
                                ActivityPhase::UsingTools,
                                format!("Calling {}", tool_name),
                                None,
                            );
                        }
                    }

                    if let Some(sid) = extract_session_id(&event) {
                        let mut should_persist_snapshot = false;
                        let mut guard = contexts.lock().unwrap();
                        if let Some(c) = guard.get_mut(&pair_id_clone) {
                            if role_clone == "mentor" {
                                if c.mentor_session_id.as_deref() != Some(sid.as_str()) {
                                    println!(
                                        "[ProcessSpawner] [{}] Registered mentor session: {}",
                                        pair_id_clone, sid
                                    );
                                    c.mentor_session_id = Some(sid);
                                    should_persist_snapshot = true;
                                }
                            } else {
                                if c.executor_session_id.as_deref() != Some(sid.as_str()) {
                                    println!(
                                        "[ProcessSpawner] [{}] Registered executor session: {}",
                                        pair_id_clone, sid
                                    );
                                    c.executor_session_id = Some(sid);
                                    should_persist_snapshot = true;
                                }
                            }
                        }
                        drop(guard);
                        if should_persist_snapshot {
                            let _ = persist_current_pair_snapshot(&app_clone, &pair_id_clone);
                        }
                    }
                } else {
                    println!(
                        "[ProcessSpawner] [{}] {}: {}",
                        pair_id_clone, role_clone, line
                    );

                    if first_output {
                        if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                            let broker = broker.lock().unwrap();
                            broker.update_agent_activity(
                                &pair_id_clone,
                                &role_clone,
                                ActivityPhase::Responding,
                                "Processing response".to_string(),
                                None,
                            );
                        }
                        first_output = false;
                    }
                }

                // Keep detailed logs, but avoid polluting plain output fallback with JSON internals.
                if provider_kind_clone != ProviderKind::Claude {
                    if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                        let broker = broker.lock().unwrap();
                        if !should_skip_plain_output_line(&line) {
                            broker.add_log_line(&pair_id_clone, &role_clone, &line);
                        }
                    }
                }

                if !is_internal_json && !should_skip_plain_output_line(&line) {
                    if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                        let broker = broker.lock().unwrap();
                        broker.update_output_progress(&pair_id_clone, &role_clone);
                    }
                    if !accumulated_plain_output.is_empty() {
                        accumulated_plain_output.push('\n');
                    }
                    accumulated_plain_output.push_str(&line);
                }
            }

            // Process exited (stream closed)
            println!(
                "[ProcessSpawner] [{}] {} process completed",
                pair_id_clone, role_clone
            );

            // Emit exactly one final user-facing message for the turn.
            let final_output_from_file = codex_last_message_path_clone
                .as_ref()
                .and_then(ProviderAdapter::read_last_message_file);

            if let Some(path) = &codex_last_message_path_clone {
                let _ = std::fs::remove_file(path);
            }

            let mut final_output = final_output_from_file
                .or_else(|| collapse_candidates(&json_candidates).filter(|s| !s.trim().is_empty()))
                .unwrap_or_else(|| accumulated_plain_output.trim().to_string());

            if final_output.trim().is_empty() {
                final_output = EMPTY_OUTPUT_PLACEHOLDER.to_string();
            }

            let no_text_output = final_output.trim() == EMPTY_OUTPUT_PLACEHOLDER;
            if no_text_output {
                final_output = format!(
                    "No textual output captured from {}. Paused for manual review.",
                    role_clone
                );
            }

            let state_before_completion =
                if let Some(broker_state) = app_clone.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker_state.lock().unwrap();
                    broker.get_state(&pair_id_clone)
                } else {
                    None
                };

            let is_mentor_review_turn = matches!(
                state_before_completion.as_ref().map(|state| &state.status),
                Some(PairStatus::Reviewing)
            ) && role_clone == "mentor";

            let (stored_output, parsed_acceptance, acceptance_error) =
                if is_mentor_review_turn && !no_text_output {
                    let outcome = process_mentor_review_verdict(
                        state_before_completion
                            .as_ref()
                            .and_then(|state| state.latest_acceptance.clone()),
                        &final_output,
                        final_output.clone(),
                    );
                    (outcome.stored_output, outcome.parsed_acceptance, outcome.acceptance_error)
                } else {
                    (final_output.clone(), None, None)
                };

            let outgoing_type = if role_clone == "mentor" {
                if is_mentor_review_turn {
                    MessageType::Acceptance
                } else {
                    MessageType::Plan
                }
            } else {
                MessageType::Result
            };

            if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                let broker = broker.lock().unwrap();
                broker.add_message(
                    &pair_id_clone,
                    Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64,
                        from: if role_clone == "mentor" {
                            MessageSender::Mentor
                        } else {
                            MessageSender::Executor
                        },
                        to: "human".to_string(),
                        msg_type: outgoing_type,
                        content: stored_output.clone(),
                        iteration: current_iteration,
                        token_usage: last_token_usage.clone(),
                    },
                );
            }

            if let Some(acceptance) = parsed_acceptance.clone() {
                if let Some(broker_state) = app_clone.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker_state.lock().unwrap();
                    broker.set_latest_acceptance(&pair_id_clone, Some(acceptance));
                }
            }

            if role_clone == "executor" && !no_text_output {
                if let Ok(Some(acceptance)) =
                    maybe_run_executor_acceptance(&app_clone, &pair_id_clone, &final_output).await
                {
                    if let Some(broker_state) = app_clone.try_state::<Mutex<MessageBroker>>() {
                        let broker = broker_state.lock().unwrap();
                        broker.set_pair_status(
                            &pair_id_clone,
                            PairStatus::Reviewing,
                            Some(format!("Acceptance checks complete. {}", acceptance.summary)),
                        );
                    }
                }
            }

            // Clean up from active processes
            {
                let mut guard = active_processes_for_cleanup.lock().unwrap();
                guard.remove(&format!("{}-{}", pair_id_clone, role_clone));
            }

            if let Some(broker) = app_clone.try_state::<Mutex<MessageBroker>>() {
                let broker = broker.lock().unwrap();
                broker.update_agent_activity(
                    &pair_id_clone,
                    &role_clone,
                    ActivityPhase::Idle,
                    "Turn finished".to_string(),
                    None,
                );
            }

            let mut should_handoff = true;
            let mut next_role = if role_clone == "mentor" {
                "executor"
            } else {
                "mentor"
            };
            let mentor_finish_signaled = role_clone == "mentor"
                && has_signal_token_on_own_line(&final_output, MENTOR_FINISH_SIGNAL);

            if role_clone == "executor"
                && has_signal_token_on_own_line(&final_output, MENTOR_FINISH_SIGNAL)
            {
                println!(
                    "[ProcessSpawner] [{}] WARNING: Executor attempted TASK_COMPLETE (ignored - only Mentor can finish)",
                    pair_id_clone
                );
            }

            if let Some(broker_state) = app_clone.try_state::<Mutex<MessageBroker>>() {
                let broker = broker_state.lock().unwrap();

                if is_mentor_review_turn {
                    if let Some(acceptance) = parsed_acceptance.as_ref() {
                        if let Some(verdict) = acceptance.verdict.as_ref() {
                            if matches!(
                                verdict.next_step.action,
                                crate::types::AcceptanceNextAction::Finish
                            ) {
                                println!(
                                    "[ProcessSpawner] [{}] Mentor acceptance finished the task",
                                    pair_id_clone
                                );
                                broker.set_pair_status(
                                    &pair_id_clone,
                                    crate::types::PairStatus::Finished,
                                    Some("Mentor acceptance marked the task finished".to_string()),
                                );
                                should_handoff = false;
                            }
                        } else if let Some(error) = acceptance_error.as_ref() {
                            // Verdict parse failed — retry or pause
                            let repair_attempts = acceptance.repair_attempts;
                            if repair_attempts > 1 {
                                println!(
                                    "[ProcessSpawner] [{}] Acceptance verdict parse failed ({} attempts), pausing: {}",
                                    pair_id_clone, repair_attempts, error
                                );
                                broker.set_pair_status(
                                    &pair_id_clone,
                                    crate::types::PairStatus::Paused,
                                    Some(format!("Acceptance verdict parse failed: {}", error)),
                                );
                                should_handoff = false;
                            } else {
                                println!(
                                    "[ProcessSpawner] [{}] Acceptance verdict parse failed, retrying mentor: {}",
                                    pair_id_clone, error
                                );
                                next_role = "mentor";
                            }
                        }
                    } else if let Some(error) = acceptance_error.as_ref() {
                        let repair_attempts = parsed_acceptance
                            .as_ref()
                            .map(|a| a.repair_attempts)
                            .unwrap_or(0);

                        if repair_attempts > 1 {
                            broker.set_pair_status(
                                &pair_id_clone,
                                crate::types::PairStatus::Paused,
                                Some(format!("Acceptance verdict parse failed: {}", error)),
                            );
                            should_handoff = false;
                        } else {
                            next_role = "mentor";
                        }
                    }
                } else if role_clone == "mentor"
                    && mentor_finish_signaled
                {
                    println!(
                        "[ProcessSpawner] [{}] Mentor emitted finish signal {}, marking session as finished",
                        pair_id_clone, MENTOR_FINISH_SIGNAL
                    );
                    broker.set_pair_status(
                        &pair_id_clone,
                        crate::types::PairStatus::Finished,
                        Some(format!("Mentor signaled {}", MENTOR_FINISH_SIGNAL)),
                    );
                    should_handoff = false;
                } else if no_text_output {
                    println!(
                        "[ProcessSpawner] [{}] {} returned no textual output, pausing for human review",
                        pair_id_clone, role_clone
                    );
                    broker.set_pair_status(
                        &pair_id_clone,
                        crate::types::PairStatus::Paused,
                        Some(format!("{} returned no textual output", role_clone)),
                    );
                    should_handoff = false;
                } else if role_clone == "executor" {
                    if let Some(state) = broker.get_state(&pair_id_clone) {
                        if state.iteration >= state.max_iterations {
                            println!(
                                "[ProcessSpawner] [{}] Max iterations reached ({}), pausing for human review",
                                pair_id_clone, state.max_iterations
                            );
                            broker.set_pair_status(
                                &pair_id_clone,
                                crate::types::PairStatus::Paused,
                                Some(
                                    "Max iterations reached. Awaiting human intervention."
                                        .to_string(),
                                ),
                            );
                            should_handoff = false;
                        }
                    }
                }
            }

            if should_handoff {
                // Check if a newer run was started while this process was running.
                // If the run_generation in the context has advanced past the value we
                // captured at the start of this turn, a new task was assigned and our
                // handoff is stale.
                if let Some(current_gen) = contexts
                    .lock()
                    .unwrap()
                    .get(&pair_id_clone)
                    .map(|c| c.run_generation)
                {
                    if current_gen != captured_run_gen {
                        println!(
                            "[ProcessSpawner] [{}] Stale run detected (captured gen={}, current gen={}), skipping handoff",
                            pair_id_clone, captured_run_gen, current_gen
                        );
                        should_handoff = false;
                    }
                }
            }

            if should_handoff {
                if let Some(broker_state) = app_clone.try_state::<Mutex<MessageBroker>>() {
                    let broker = broker_state.lock().unwrap();
                    if let Some(state) = broker.get_state(&pair_id_clone) {
                        if state.status == crate::types::PairStatus::Paused {
                            println!(
                                "[ProcessSpawner] [{}] Skipping handoff - pair is paused",
                                pair_id_clone
                            );
                            should_handoff = false;
                        }
                    }
                }
            }

            if should_handoff {
                println!(
                    "[ProcessSpawner] [{}] Triggering handoff to {}",
                    pair_id_clone, next_role
                );
                let _ = app_clone.emit(
                    "pair:handoff",
                    serde_json::json!({
                        "pairId": pair_id_clone,
                        "nextRole": next_role
                    }),
                );
            } else {
                println!("[ProcessSpawner] [{}] Handoff skipped", pair_id_clone);
            }
        });

        let mut guard = self.active_processes.lock().unwrap();
        guard.insert(format!("{}-{}", pair_id, role), child);

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
    fn gemini_result_events_prefer_structured_candidate_text_over_protocol_message_fields() {
        let event = json!({
            "type": "result",
            "message": "}",
            "candidates": [
                {
                    "content": {
                        "parts": [
                            { "text": "Structured final answer" }
                        ]
                    }
                }
            ]
        });

        let mut candidates = Vec::new();
        collect_json_candidates_for_provider(ProviderKind::Gemini, &event, &mut candidates);

        assert_eq!(candidates, vec!["Structured final answer".to_string()]);
    }

    #[test]
    fn gemini_stream_events_collect_server_content_parts() {
        let event = json!({
            "type": "content",
            "serverContent": {
                "modelTurn": {
                    "parts": [
                        { "text": "Cross-provider handoff is ready." }
                    ]
                }
            }
        });

        let mut candidates = Vec::new();
        collect_json_candidates_for_provider(ProviderKind::Gemini, &event, &mut candidates);

        assert_eq!(
            candidates,
            vec!["Cross-provider handoff is ready.".to_string()]
        );
    }

    #[test]
    fn extract_token_usage_from_claude_parses_result_and_streaming_events() {
        let result_event = json!({
            "type": "result",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 250
            }
        });

        let usage =
            extract_token_usage_from_claude(&result_event).expect("should parse claude result");
        assert_eq!(usage.output_tokens, 250);
        assert_eq!(usage.input_tokens, Some(100));
        assert!(matches!(usage.source, TokenUsageSource::Final));

        let streaming_event = json!({
            "type": "content_block_delta",
            "usage": {
                "input_tokens": 50,
                "output_tokens": 75
            }
        });

        let usage = extract_token_usage_from_claude(&streaming_event)
            .expect("should parse claude streaming");
        assert_eq!(usage.output_tokens, 75);
        assert!(matches!(usage.source, TokenUsageSource::Live));
    }

    #[test]
    fn extract_token_usage_from_codex_extracts_from_usage_field() {
        let event = json!({
            "type": "message",
            "usage": {
                "prompt_tokens": 200,
                "completion_tokens": 350
            }
        });

        let usage = extract_token_usage_from_codex(&event).expect("should parse codex usage");
        assert_eq!(usage.output_tokens, 350);
        assert_eq!(usage.input_tokens, Some(200));
        assert!(matches!(usage.source, TokenUsageSource::Final));
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

        let usage =
            extract_token_usage_from_opencode(&live_event).expect("should parse opencode live");
        assert_eq!(usage.output_tokens, 120);
        assert!(matches!(usage.source, TokenUsageSource::Live));

        let final_event = json!({
            "type": "result",
            "usage": {
                "input_tokens": 80,
                "output_tokens": 150
            }
        });

        let usage =
            extract_token_usage_from_opencode(&final_event).expect("should parse opencode final");
        assert_eq!(usage.output_tokens, 150);
        assert!(matches!(usage.source, TokenUsageSource::Final));
    }

    #[test]
    fn extract_token_usage_from_gemini_parses_usage_metadata() {
        let event = json!({
            "type": "result",
            "usageMetadata": {
                "promptTokenCount": 300,
                "candidatesTokenCount": 450
            }
        });

        let usage = extract_token_usage_from_gemini(&event).expect("should parse gemini usage");
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

        assert!(extract_token_usage_from_claude(&no_usage).is_none());
        assert!(extract_token_usage_from_codex(&no_usage).is_none());
        assert!(extract_token_usage_from_opencode(&no_usage).is_none());
        assert!(extract_token_usage_from_gemini(&no_usage).is_none());
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
            "type": "result",
            "usageMetadata": { "candidatesTokenCount": 400 }
        });
        let usage =
            extract_token_usage(ProviderKind::Gemini, &gemini_event).expect("gemini dispatch");
        assert_eq!(usage.output_tokens, 400);
    }

    #[test]
    fn token_usage_live_to_final_transition_preserves_latest_value() {
        let live_event = json!({
            "type": "content_block_delta",
            "usage": {
                "input_tokens": 50,
                "output_tokens": 120
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
        let claude_usage = extract_token_usage_from_claude(&claude_with_alternate_fields)
            .expect("claude alternate field names");
        assert_eq!(claude_usage.input_tokens, Some(100));
        assert_eq!(claude_usage.output_tokens, 200);

        let codex_with_alternate_fields = json!({
            "usage": {
                "input_tokens": 150,
                "output_tokens": 250
            }
        });
        let codex_usage = extract_token_usage_from_codex(&codex_with_alternate_fields)
            .expect("codex alternate field names");
        assert_eq!(codex_usage.input_tokens, Some(150));
        assert_eq!(codex_usage.output_tokens, 250);

        let gemini_with_alternate_fields = json!({
            "type": "result",
            "usageMetadata": {
                "input_tokens": 175,
                "output_tokens": 275
            }
        });
        let gemini_usage = extract_token_usage_from_gemini(&gemini_with_alternate_fields)
            .expect("gemini alternate field names");
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
            "type": "content_block_delta",
            "usage": { "output_tokens": 100, "input_tokens": 50 }
        });
        last_token_usage = update_last_token_usage(last_token_usage, &event1, ProviderKind::Claude);
        assert!(last_token_usage.is_some());
        assert_eq!(last_token_usage.as_ref().unwrap().output_tokens, 100);

        let event2 = json!({
            "type": "content_block_delta",
            "usage": { "output_tokens": 200, "input_tokens": 50 }
        });
        last_token_usage = update_last_token_usage(last_token_usage, &event2, ProviderKind::Claude);
        assert_eq!(last_token_usage.as_ref().unwrap().output_tokens, 200);

        let event_no_usage = json!({
            "type": "content_block_delta",
            "delta": { "text": "some text" }
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
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", uuid::Uuid::new_v4()));
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
    }
}
