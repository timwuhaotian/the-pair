use crate::context_bridge::PlanItem;
use crate::types::{
    AcceptanceRecord, ActivityPhase, AgentActivity, AgentRole, AgentState, CreatePairInput,
    GitTracking, Message, MessageSender, MessageType, PairResources, PairState, PairStatus,
    ResourceInfo, TurnTokenUsage,
};
use crate::util::now_millis;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct MessageBroker {
    pair_states: Arc<Mutex<HashMap<String, PairState>>>,
    app_handle: Option<AppHandle>,
}

impl MessageBroker {
    pub fn new() -> Self {
        Self {
            pair_states: Arc::new(Mutex::new(HashMap::new())),
            app_handle: None,
        }
    }

    pub fn set_app_handle(&mut self, handle: AppHandle) {
        self.app_handle = Some(handle);
    }

    fn create_idle_activity(label: &str) -> AgentActivity {
        let now = now_millis();
        AgentActivity {
            phase: ActivityPhase::Idle,
            label: label.to_string(),
            detail: None,
            started_at: now,
            updated_at: now,
            last_output_at: None,
            output_line_count: 0,
        }
    }

    fn update_activity(
        activity: &mut AgentActivity,
        phase: ActivityPhase,
        label: &str,
        detail: Option<String>,
    ) {
        activity.phase = phase;
        activity.label = label.to_string();
        activity.detail = detail;
        activity.updated_at = now_millis();
    }

    fn update_both_activities(
        mentor_activity: &mut AgentActivity,
        executor_activity: &mut AgentActivity,
        mentor_phase: ActivityPhase,
        mentor_label: &str,
        mentor_detail: Option<String>,
        executor_phase: ActivityPhase,
        executor_label: &str,
        executor_detail: Option<String>,
    ) {
        Self::update_activity(mentor_activity, mentor_phase, mentor_label, mentor_detail);
        Self::update_activity(
            executor_activity,
            executor_phase,
            executor_label,
            executor_detail,
        );
    }

    pub fn initialize_pair(
        &self,
        pair_id: &str,
        input: CreatePairInput,
        effective_directory: Option<&str>,
    ) -> Result<(), String> {
        println!(
            "[MessageBroker::initialize_pair] Initializing pair: {}",
            pair_id
        );

        let directory = effective_directory
            .map(|s| s.to_string())
            .unwrap_or(input.directory.clone());

        let empty_resources = PairResources {
            mentor: ResourceInfo {
                cpu: 0.0,
                mem_mb: 0.0,
            },
            executor: ResourceInfo {
                cpu: 0.0,
                mem_mb: 0.0,
            },
            pair_total: ResourceInfo {
                cpu: 0.0,
                mem_mb: 0.0,
            },
        };

        let state = PairState {
            pair_id: pair_id.to_string(),
            directory,
            status: PairStatus::Idle,
            iteration: 0,
            // 0 = unlimited. Pairs run until the mentor signals completion (or a human
            // stops them); the iteration budget is no longer a default safety cap.
            max_iterations: input.max_iterations.unwrap_or(0),
            turn: AgentRole::Mentor,
            mentor: AgentState {
                status: PairStatus::Idle,
                turn: AgentRole::Mentor,
                last_message: None,
                activity: Self::create_idle_activity("Mentor idle"),
                token_usage: None,
            },
            executor: AgentState {
                status: PairStatus::Idle,
                turn: AgentRole::Executor,
                last_message: None,
                activity: Self::create_idle_activity("Executor idle"),
                token_usage: None,
            },
            messages: Vec::new(),
            mentor_activity: Self::create_idle_activity("Mentor idle"),
            executor_activity: Self::create_idle_activity("Executor idle"),
            resources: empty_resources,
            modified_files: Vec::new(),
            git_tracking: GitTracking {
                available: false,
                root_path: None,
                baseline: None,
                git_review_available: Some(false),
            },
            automation_mode: "full-auto".to_string(),
            git_review_available: false,
            finished_at: None,
            latest_acceptance: None,
            acceptance_history: Vec::new(),
            worktree_path: effective_directory.map(|s| s.to_string()),
            turn_started_at: None,
            plan_checklist: Vec::new(),
            key_decisions: Vec::new(),
            cognitive_events: Vec::new(),
            plan_gate: input.plan_gate.unwrap_or(false),
        };

        let mut pair_states = self.pair_states.lock().unwrap();
        pair_states.insert(pair_id.to_string(), state);
        println!("[MessageBroker::initialize_pair] Pair state inserted successfully");
        Ok(())
    }

    pub fn get_last_messages(&self, pair_id: &str) -> (Option<Message>, Option<Message>) {
        let pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get(pair_id) {
            (
                state.mentor.last_message.clone(),
                state.executor.last_message.clone(),
            )
        } else {
            (None, None)
        }
    }

    pub fn add_message(&self, pair_id: &str, mut message: Message) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            // Assign current iteration
            message.iteration = state.iteration;

            // Update last message for the sender - only for high-signal messages
            if matches!(
                message.msg_type,
                MessageType::Plan | MessageType::Result | MessageType::Acceptance
            ) {
                if matches!(message.from, MessageSender::Mentor) {
                    state.mentor.last_message = Some(message.clone());
                } else if matches!(message.from, MessageSender::Executor) {
                    state.executor.last_message = Some(message.clone());
                }

                // Only add high-signal messages to the conversation history
                state.messages.push(message.clone());
            }

            if message.msg_type == MessageType::Handoff {
                state.turn = if matches!(state.turn, AgentRole::Mentor) {
                    AgentRole::Executor
                } else {
                    AgentRole::Mentor
                };
                state.iteration += 1;
            }

            self.notify_state_update(pair_id, state);

            // Emit all message types for real-time UI updates
            if let Some(handle) = &self.app_handle {
                let _ = handle.emit(
                    "pair:message",
                    serde_json::json!({
                        "pairId": pair_id,
                        "message": message
                    }),
                );
            }
        }
    }

    pub fn add_log_line(&self, pair_id: &str, role: &str, line: &str) {
        let pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get(pair_id) {
            let msg = Message {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: now_millis(),
                from: if role == "mentor" {
                    MessageSender::Mentor
                } else {
                    MessageSender::Executor
                },
                to: "human".to_string(),
                msg_type: MessageType::Progress,
                content: line.to_string(),
                iteration: state.iteration,
                token_usage: None,
            };

            if let Some(handle) = &self.app_handle {
                let _ = handle.emit(
                    "pair:message",
                    serde_json::json!({
                        "pairId": pair_id,
                        "message": msg
                    }),
                );
                // We no longer push to state.messages to keep context clean for agents
            }
        }
    }

    #[cfg(test)]
    pub fn record_human_feedback(
        &self,
        pair_id: &str,
        approved: bool,
    ) -> Result<Option<AgentRole>, String> {
        let mut pair_states = self.pair_states.lock().map_err(|e| e.to_string())?;
        let state = pair_states
            .get_mut(pair_id)
            .ok_or_else(|| format!("Pair {} not found", pair_id))?;

        if !matches!(state.status, PairStatus::AwaitingHumanReview) {
            return Err(format!("Pair {} is not waiting for human review", pair_id));
        }

        let feedback_text = if approved {
            "Human approved review. Continuing."
        } else {
            "Human rejected review. Stopping run."
        };

        let feedback = Message {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: now_millis(),
            from: MessageSender::Human,
            to: "both".to_string(),
            msg_type: MessageType::Feedback,
            content: feedback_text.to_string(),
            iteration: state.iteration,
            token_usage: None,
        };

        state.messages.push(feedback.clone());

        if approved {
            self.notify_state_update(pair_id, state);
            if let Some(handle) = &self.app_handle {
                let _ = handle.emit(
                    "pair:message",
                    serde_json::json!({
                        "pairId": pair_id,
                        "message": feedback
                    }),
                );
            }

            let next_role = match state.turn {
                AgentRole::Mentor => AgentRole::Executor,
                AgentRole::Executor => AgentRole::Mentor,
            };

            Ok(Some(next_role))
        } else {
            state.status = PairStatus::Error;
            state.mentor.status = PairStatus::Error;
            state.executor.status = PairStatus::Error;

            Self::update_both_activities(
                &mut state.mentor_activity,
                &mut state.executor_activity,
                ActivityPhase::Error,
                "Human rejected review",
                Some("Manual intervention required".to_string()),
                ActivityPhase::Error,
                "Human rejected review",
                Some("Manual intervention required".to_string()),
            );

            self.notify_state_update(pair_id, state);
            if let Some(handle) = &self.app_handle {
                let _ = handle.emit(
                    "pair:message",
                    serde_json::json!({
                        "pairId": pair_id,
                        "message": feedback
                    }),
                );
            }

            Ok(None)
        }
    }

    pub fn reset_session(&self, pair_id: &str) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            state.messages.clear();
            state.iteration = 0;
            state.latest_acceptance = None;
            state.acceptance_history.clear();
            println!("[MessageBroker] Session reset for pair {}", pair_id);
        }
    }

    pub fn prepare_run(
        &self,
        pair_id: &str,
        role: &str,
        active_processes: Arc<Mutex<HashMap<String, tokio::process::Child>>>,
    ) {
        let mut pair_states = self.pair_states.lock().unwrap();
        let mut should_spawn_monitor = false;

        if let Some(state) = pair_states.get_mut(pair_id) {
            let previous_status = state.status.clone();

            // Only spawn monitor if we're starting from a stopped state
            if matches!(
                state.status,
                PairStatus::Idle
                    | PairStatus::Finished
                    | PairStatus::Error
                    | PairStatus::Paused
                    | PairStatus::AwaitingHumanReview
            ) {
                should_spawn_monitor = true;
            }

            // Update status based on role
            state.status = if role == "mentor" {
                PairStatus::Mentoring
            } else {
                PairStatus::Executing
            };
            state.turn = if role == "mentor" {
                AgentRole::Mentor
            } else {
                AgentRole::Executor
            };

            if role == "mentor" {
                // A mentor turn resumed from AwaitingHumanReview is always a
                // re-plan: the gate fires on planning turns (never review), so a
                // rejected plan should re-plan (Mentoring), not review.
                let is_planning_turn = matches!(
                    previous_status,
                    PairStatus::Idle
                        | PairStatus::Finished
                        | PairStatus::Error
                        | PairStatus::AwaitingHumanReview
                ) || state.iteration == 0;

                if is_planning_turn {
                    state.iteration = 1;
                    state.status = PairStatus::Mentoring;
                    state.latest_acceptance = None;
                    state.acceptance_history.clear();
                    state.mentor.status = PairStatus::Executing;
                    Self::update_both_activities(
                        &mut state.mentor_activity,
                        &mut state.executor_activity,
                        ActivityPhase::Thinking,
                        "Analyzing task",
                        Some("Preparing first instruction".to_string()),
                        ActivityPhase::Waiting,
                        "Executor standing by",
                        None,
                    );

                    state.executor.status = PairStatus::Idle;
                } else {
                    state.iteration = state.iteration.saturating_add(1);
                    state.status = PairStatus::Reviewing;
                    state.mentor.status = PairStatus::Reviewing;
                    Self::update_both_activities(
                        &mut state.mentor_activity,
                        &mut state.executor_activity,
                        ActivityPhase::Thinking,
                        "Reviewing changes",
                        Some("Checking the work".to_string()),
                        ActivityPhase::Waiting,
                        "Executor standing by",
                        Some("Executor paused for review".to_string()),
                    );

                    state.executor.status = PairStatus::Idle;
                }
            } else {
                state.executor.status = PairStatus::Executing;
                Self::update_both_activities(
                    &mut state.mentor_activity,
                    &mut state.executor_activity,
                    ActivityPhase::Waiting,
                    "Mentor observing",
                    None,
                    ActivityPhase::Thinking,
                    "Executing plan",
                    Some("Processing instructions".to_string()),
                );
            }

            self.notify_state_update(pair_id, state);

            println!(
                "[MessageBroker] Prepared run for pair {} as {}",
                pair_id, role
            );
        }
        drop(pair_states);

        if should_spawn_monitor {
            Self::spawn_monitor(
                self.pair_states.clone(),
                self.app_handle.clone(),
                pair_id.to_string(),
                active_processes.clone(),
            );
        }
    }

    fn spawn_monitor(
        pair_states: Arc<Mutex<HashMap<String, PairState>>>,
        app_handle: Option<tauri::AppHandle>,
        pair_id_string: String,
        active_processes: Arc<Mutex<HashMap<String, tokio::process::Child>>>,
    ) {
        const STALL_WARNING_SECS: u64 = 120;
        const STALL_CRITICAL_SECS: u64 = 600;

        tauri::async_runtime::spawn(async move {
            let mut sys = sysinfo::System::new_all();
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                let mut guard = pair_states.lock().unwrap();
                if let Some(state) = guard.get_mut(&pair_id_string) {
                    if matches!(
                        state.status,
                        PairStatus::Finished
                            | PairStatus::Error
                            | PairStatus::Idle
                            | PairStatus::AwaitingHumanReview
                            | PairStatus::Paused
                    ) {
                        break;
                    }

                    crate::git_tracker::GitTracker::update_state(state);
                    let active = active_processes.clone();
                    crate::resource_monitor::ResourceMonitor::update_state(state, &mut sys, active);

                    let now = now_millis();
                    let active_role = if state.turn == AgentRole::Mentor {
                        "mentor"
                    } else {
                        "executor"
                    };
                    let activity = if state.turn == AgentRole::Mentor {
                        &state.mentor_activity
                    } else {
                        &state.executor_activity
                    };

                    let mut activity_for_update = false;
                    let mut new_phase = activity.phase.clone();
                    let mut new_label = activity.label.clone();
                    let mut new_detail = activity.detail.clone();

                    if activity.last_output_at.is_some() {
                        // saturating_sub guards against a backward wall-clock jump
                        // (NTP correction, manual change, VM resume) which would
                        // otherwise underflow and report a spurious multi-century stall.
                        let elapsed_secs = now.saturating_sub(activity.last_output_at.unwrap()) / 1000;
                        if elapsed_secs >= STALL_CRITICAL_SECS {
                            new_phase = ActivityPhase::Stalled;
                            new_label =
                                format!("No output for {}s — process may be stuck", elapsed_secs);
                            new_detail =
                                Some(format!("Stalled after {}s of inactivity", elapsed_secs));
                            activity_for_update = true;
                        } else if elapsed_secs >= STALL_WARNING_SECS
                            && activity.phase != ActivityPhase::Stalled
                        {
                            new_detail = Some(format!("No new output for {}s", elapsed_secs));
                            activity_for_update = true;
                        }
                    } else if let Some(started) = state.turn_started_at {
                        let elapsed_secs = now.saturating_sub(started) / 1000;
                        if elapsed_secs >= STALL_CRITICAL_SECS {
                            new_phase = ActivityPhase::Stalled;
                            new_label =
                                format!("No output after {}s — process may be stuck", elapsed_secs);
                            new_detail =
                                Some(format!("Waiting for first output for {}s", elapsed_secs));
                            activity_for_update = true;
                        } else if elapsed_secs >= STALL_WARNING_SECS
                            && activity.phase != ActivityPhase::Stalled
                        {
                            new_detail =
                                Some(format!("Waiting for first output... ({}s)", elapsed_secs));
                            activity_for_update = true;
                        }
                    }

                    drop(guard);
                    if activity_for_update {
                        let mut guard2 = pair_states.lock().unwrap();
                        if let Some(state) = guard2.get_mut(&pair_id_string) {
                            let activity = if active_role == "mentor" {
                                &mut state.mentor_activity
                            } else {
                                &mut state.executor_activity
                            };
                            activity.phase = new_phase;
                            activity.label = new_label;
                            activity.detail = new_detail;
                            activity.updated_at = now_millis();
                            if let Some(handle) = &app_handle {
                                let _ = handle.emit("pair:state", state.clone());
                            }
                        }
                    } else {
                        let mut guard2 = pair_states.lock().unwrap();
                        if let Some(state) = guard2.get_mut(&pair_id_string) {
                            if let Some(handle) = &app_handle {
                                let _ = handle.emit("pair:state", state.clone());
                            }
                        }
                    }
                } else {
                    break;
                }
            }
        });
    }

    pub fn resume_run(
        &self,
        pair_id: &str,
        role: &str,
        active_processes: Arc<Mutex<HashMap<String, tokio::process::Child>>>,
    ) -> PairStatus {
        let resolved_status = {
            let mut pair_states = self.pair_states.lock().unwrap();

            if let Some(state) = pair_states.get_mut(pair_id) {
                let turn = state.turn.clone();
                let iteration = state.iteration;
                let is_planning_turn =
                    matches!(&turn, AgentRole::Mentor) && (iteration == 1 || iteration == 0);

                let resolved = if role == "mentor" {
                    if is_planning_turn {
                        state.iteration = if iteration == 0 { 1 } else { iteration };
                        PairStatus::Mentoring
                    } else {
                        PairStatus::Reviewing
                    }
                } else {
                    PairStatus::Executing
                };

                state.status = resolved.clone();
                state.turn = if role == "mentor" {
                    AgentRole::Mentor
                } else {
                    AgentRole::Executor
                };

                if role == "mentor" {
                    if is_planning_turn {
                        state.mentor.status = PairStatus::Executing;
                        Self::update_both_activities(
                            &mut state.mentor_activity,
                            &mut state.executor_activity,
                            ActivityPhase::Thinking,
                            "Analyzing task",
                            Some("Resuming from pause".to_string()),
                            ActivityPhase::Waiting,
                            "Executor standing by",
                            None,
                        );

                        state.executor.status = PairStatus::Idle;
                    } else {
                        state.mentor.status = PairStatus::Reviewing;
                        Self::update_both_activities(
                            &mut state.mentor_activity,
                            &mut state.executor_activity,
                            ActivityPhase::Thinking,
                            "Reviewing changes",
                            Some("Resuming from pause".to_string()),
                            ActivityPhase::Waiting,
                            "Executor standing by",
                            Some("Executor paused for review".to_string()),
                        );

                        state.executor.status = PairStatus::Idle;
                    }
                } else {
                    state.executor.status = PairStatus::Executing;
                    Self::update_both_activities(
                        &mut state.mentor_activity,
                        &mut state.executor_activity,
                        ActivityPhase::Waiting,
                        "Mentor observing",
                        None,
                        ActivityPhase::Thinking,
                        "Executing plan",
                        Some("Resuming from pause".to_string()),
                    );

                    state.mentor.status = PairStatus::Idle;
                }

                self.notify_state_update(pair_id, state);

                println!(
                    "[MessageBroker] Resumed run for pair {} as {} (status={:?}, planning={}, iter={})",
                    pair_id, role, resolved, is_planning_turn, state.iteration
                );

                resolved
            } else {
                if role == "mentor" {
                    PairStatus::Mentoring
                } else {
                    PairStatus::Executing
                }
            }
        };

        Self::spawn_monitor(
            self.pair_states.clone(),
            self.app_handle.clone(),
            pair_id.to_string(),
            active_processes.clone(),
        );

        resolved_status
    }

    pub fn get_state(&self, pair_id: &str) -> Option<PairState> {
        let pair_states = self.pair_states.lock().unwrap();
        pair_states.get(pair_id).cloned()
    }

    pub fn set_latest_acceptance(&self, pair_id: &str, acceptance: Option<AcceptanceRecord>) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            if let Some(ref record) = acceptance {
                state.acceptance_history.push(record.clone());
            }
            state.latest_acceptance = acceptance;
            self.notify_state_update(pair_id, state);
        }
    }

    pub fn set_plan_checklist(&self, pair_id: &str, checklist: Vec<PlanItem>) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            state.plan_checklist = checklist
                .into_iter()
                .filter_map(|item| serde_json::to_value(&item).ok())
                .collect();
            self.notify_state_update(pair_id, state);
        }
    }

    pub fn restore_state(&self, state: PairState) -> Result<(), String> {
        let pair_id = state.pair_id.clone();
        let mut pair_states = self.pair_states.lock().map_err(|e| e.to_string())?;
        pair_states.insert(pair_id.clone(), state.clone());
        drop(pair_states);
        self.notify_state_update(&pair_id, &state);
        Ok(())
    }

    pub fn update_agent_activity(
        &self,
        pair_id: &str,
        role: &str,
        phase: crate::types::ActivityPhase,
        label: String,
        detail: Option<String>,
    ) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            let activity = if role == "mentor" {
                &mut state.mentor_activity
            } else {
                &mut state.executor_activity
            };

            activity.phase = phase;
            activity.label = label;
            activity.detail = detail;
            activity.updated_at = now_millis();

            self.notify_state_update(pair_id, state);
        }
    }

    pub fn update_output_progress(&self, pair_id: &str, role: &str) {
        let now = now_millis();
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            let activity = if role == "mentor" {
                &mut state.mentor_activity
            } else {
                &mut state.executor_activity
            };

            if activity.phase == ActivityPhase::Stalled {
                activity.phase = ActivityPhase::Responding;
                activity.label = "Processing response".to_string();
                activity.detail = None;
            }

            activity.last_output_at = Some(now);
            activity.output_line_count += 1;
            activity.updated_at = now;

            let should_notify =
                activity.output_line_count <= 5 || activity.output_line_count % 10 == 0;

            drop(pair_states);
            if should_notify {
                let pair_states = self.pair_states.lock().unwrap();
                if let Some(state) = pair_states.get(pair_id) {
                    self.notify_state_update(pair_id, state);
                }
            }
        }
    }

    pub fn add_cognitive_event(
        &self,
        pair_id: &str,
        role: &str,
        event_type: crate::types::CognitiveEventType,
        tool_name: Option<String>,
        description: String,
        status: crate::types::CognitiveEventStatus,
    ) {
        let now = crate::util::now_millis();
        let event_id = format!("ce-{}-{}", role, now);
        let agent_role = if role == "mentor" {
            crate::types::AgentRole::Mentor
        } else {
            crate::types::AgentRole::Executor
        };
        let event = crate::types::CognitiveEvent {
            id: event_id,
            timestamp: now,
            role: agent_role,
            event_type,
            tool_name,
            description,
            status,
        };

        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            state.cognitive_events.push(event);
            // Keep only last 50 events to prevent memory bloat
            if state.cognitive_events.len() > 50 {
                state.cognitive_events.drain(0..state.cognitive_events.len() - 50);
            }
            self.notify_state_update(pair_id, state);
        }
    }

    pub fn set_turn_started_at(&self, pair_id: &str, timestamp: u64) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            // Clear cognitive events for new turn
            state.cognitive_events.clear();
            let activity = if state.turn == AgentRole::Mentor {
                &mut state.mentor_activity
            } else {
                &mut state.executor_activity
            };
            activity.output_line_count = 0;
            activity.last_output_at = None;
            activity.phase = ActivityPhase::Thinking;
            activity.label = "Starting process...".to_string();
            activity.detail = None;
            activity.updated_at = timestamp;
            state.turn_started_at = Some(timestamp);
            self.notify_state_update(pair_id, state);
        }
    }

    pub fn update_token_usage(&self, pair_id: &str, role: &str, usage: TurnTokenUsage) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            let agent_state = if role == "mentor" {
                &mut state.mentor
            } else {
                &mut state.executor
            };

            agent_state.token_usage = Some(usage);
            self.notify_state_update(pair_id, state);
        }
    }

    pub fn reset_token_usage(&self, pair_id: &str, role: &str) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            let agent_state = if role == "mentor" {
                &mut state.mentor
            } else {
                &mut state.executor
            };

            agent_state.token_usage = None;
            self.notify_state_update(pair_id, state);
        }
    }

    pub fn set_pair_status(&self, pair_id: &str, status: PairStatus, detail: Option<String>) {
        let mut pair_states = self.pair_states.lock().unwrap();
        if let Some(state) = pair_states.get_mut(pair_id) {
            state.status = status.clone();
            state.mentor.status = status.clone();
            state.executor.status = status.clone();

            if status != PairStatus::Finished {
                state.finished_at = None;
            }

            match status {
                PairStatus::Finished => {
                    state.finished_at = Some(now_millis());
                    Self::update_both_activities(
                        &mut state.mentor_activity,
                        &mut state.executor_activity,
                        ActivityPhase::Idle,
                        "Mission finished",
                        detail.clone(),
                        ActivityPhase::Idle,
                        "Executor idle",
                        None,
                    );
                }
                PairStatus::AwaitingHumanReview => {
                    Self::update_both_activities(
                        &mut state.mentor_activity,
                        &mut state.executor_activity,
                        ActivityPhase::Waiting,
                        "Awaiting human review",
                        detail.clone(),
                        ActivityPhase::Waiting,
                        "Awaiting human review",
                        None,
                    );
                }
                PairStatus::Reviewing => {
                    Self::update_both_activities(
                        &mut state.mentor_activity,
                        &mut state.executor_activity,
                        ActivityPhase::Thinking,
                        "Reviewing changes",
                        detail.clone(),
                        ActivityPhase::Waiting,
                        "Executor standing by",
                        None,
                    );
                }
                PairStatus::Paused => {
                    Self::update_both_activities(
                        &mut state.mentor_activity,
                        &mut state.executor_activity,
                        ActivityPhase::Idle,
                        "Paused",
                        detail.clone(),
                        ActivityPhase::Idle,
                        "Paused",
                        detail,
                    );
                }
                PairStatus::Error => {
                    Self::update_both_activities(
                        &mut state.mentor_activity,
                        &mut state.executor_activity,
                        ActivityPhase::Error,
                        "Error",
                        detail.clone(),
                        ActivityPhase::Error,
                        "Error",
                        None,
                    );
                }
                _ => {}
            }

            self.notify_state_update(pair_id, state);
        }
    }

    fn notify_state_update(&self, _pair_id: &str, state: &PairState) {
        if let Some(handle) = &self.app_handle {
            let _ = handle.emit("pair:state", state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AcceptanceRecord, AcceptanceRisk, ActivityPhase, AgentActivity, AgentConfig, AgentRole,
        AgentState, CreatePairInput, GitTracking, Message, MessageSender, MessageType,
        PairResources, PairState, PairStatus, ResourceInfo,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn activity(label: &str, phase: ActivityPhase) -> AgentActivity {
        AgentActivity {
            phase,
            label: label.to_string(),
            detail: None,
            started_at: 0,
            updated_at: 0,
            last_output_at: None,
            output_line_count: 0,
        }
    }

    fn pair_state(status: PairStatus, iteration: u32) -> PairState {
        PairState {
            pair_id: "pair-1".to_string(),
            directory: "/tmp/project".to_string(),
            status,
            iteration,
            max_iterations: 3,
            turn: AgentRole::Mentor,
            mentor: AgentState {
                status: PairStatus::Idle,
                turn: AgentRole::Mentor,
                last_message: None,
                activity: activity("Mentor idle", ActivityPhase::Idle),
                token_usage: None,
            },
            executor: AgentState {
                status: PairStatus::Idle,
                turn: AgentRole::Executor,
                last_message: None,
                activity: activity("Executor idle", ActivityPhase::Idle),
                token_usage: None,
            },
            messages: Vec::new(),
            mentor_activity: activity("Mentor idle", ActivityPhase::Idle),
            executor_activity: activity("Executor idle", ActivityPhase::Idle),
            resources: PairResources {
                mentor: ResourceInfo {
                    cpu: 0.0,
                    mem_mb: 0.0,
                },
                executor: ResourceInfo {
                    cpu: 0.0,
                    mem_mb: 0.0,
                },
                pair_total: ResourceInfo {
                    cpu: 0.0,
                    mem_mb: 0.0,
                },
            },
            modified_files: Vec::new(),
            git_tracking: GitTracking {
                available: false,
                root_path: None,
                baseline: None,
                git_review_available: Some(false),
            },
            automation_mode: "full-auto".to_string(),
            git_review_available: false,
            finished_at: None,
            latest_acceptance: None,
            acceptance_history: Vec::new(),
            worktree_path: None,
            turn_started_at: None,
            plan_checklist: Vec::new(),
            key_decisions: Vec::new(),
            cognitive_events: Vec::new(),
            plan_gate: false,
        }
    }

    fn sample_input() -> CreatePairInput {
        CreatePairInput {
            name: "Demo".to_string(),
            directory: "/tmp/project".to_string(),
            spec: "Build the feature".to_string(),
            mentor: AgentConfig {
                role: AgentRole::Mentor,
                provider: crate::provider_registry::ProviderKind::Opencode,
                model: "mentor-model".to_string(),
                reasoning_effort: None,
            },
            executor: AgentConfig {
                role: AgentRole::Executor,
                provider: crate::provider_registry::ProviderKind::Codex,
                model: "executor-model".to_string(),
                reasoning_effort: None,
            },
            mentor_reasoning_effort: None,
            executor_reasoning_effort: None,
            max_iterations: None,
            branch: None,
            plan_gate: None,
        }
    }

    #[test]
    fn prepare_run_advances_idle_mentor_pairs_into_mentoring() {
        let broker = MessageBroker::new();
        broker
            .initialize_pair("pair-1", sample_input(), None)
            .unwrap();
        broker
            .restore_state(pair_state(PairStatus::Mentoring, 0))
            .unwrap();

        broker.prepare_run(
            "pair-1",
            "mentor",
            Arc::new(Mutex::new(HashMap::<String, tokio::process::Child>::new())),
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(state.status, PairStatus::Mentoring);
        assert_eq!(state.iteration, 1);
        assert_eq!(state.turn, AgentRole::Mentor);
        assert_eq!(state.mentor.status, PairStatus::Executing);
        assert!(matches!(
            state.mentor_activity.phase,
            ActivityPhase::Thinking
        ));
        assert!(matches!(
            state.executor_activity.phase,
            ActivityPhase::Waiting
        ));
    }

    #[test]
    fn prepare_run_clears_acceptance_history_before_new_mentor_planning_run() {
        let broker = MessageBroker::new();
        let mut state = pair_state(PairStatus::Finished, 4);
        let prior_acceptance = AcceptanceRecord {
            iteration: 3,
            risk: AcceptanceRisk::Medium,
            checks: Vec::new(),
            summary: "prior run check".to_string(),
            started_at: 100,
            finished_at: 200,
            verdict: None,
            raw_verdict: None,
            error: None,
            repair_attempts: 0,
        };

        state.latest_acceptance = Some(prior_acceptance.clone());
        state.acceptance_history = vec![prior_acceptance];

        broker.restore_state(state).unwrap();

        broker.prepare_run("pair-1", "mentor", Arc::new(Mutex::new(HashMap::new())));

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert!(state.acceptance_history.is_empty());
        assert!(state.latest_acceptance.is_none());
    }

    #[test]
    fn prepare_run_switches_mentor_turns_after_executor_work_into_reviewing() {
        let broker = MessageBroker::new();
        broker
            .initialize_pair("pair-1", sample_input(), None)
            .unwrap();
        broker
            .restore_state(pair_state(PairStatus::Executing, 1))
            .unwrap();

        broker.prepare_run(
            "pair-1",
            "mentor",
            Arc::new(Mutex::new(HashMap::<String, tokio::process::Child>::new())),
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(state.status, PairStatus::Reviewing);
        assert_eq!(state.mentor_activity.label, "Reviewing changes");
        assert_eq!(
            state.mentor_activity.detail.as_deref(),
            Some("Checking the work")
        );
        assert!(matches!(
            state.mentor_activity.phase,
            ActivityPhase::Thinking
        ));
    }

    #[test]
    fn set_pair_status_marks_paused_pairs_as_idle_with_pause_copy() {
        let broker = MessageBroker::new();
        broker
            .initialize_pair("pair-1", sample_input(), None)
            .unwrap();
        broker
            .restore_state(pair_state(PairStatus::Paused, 4))
            .unwrap();

        broker.set_pair_status(
            "pair-1",
            PairStatus::Paused,
            Some("Paused by user".to_string()),
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(state.status, PairStatus::Paused);
        assert_eq!(state.mentor.status, PairStatus::Paused);
        assert_eq!(state.executor.status, PairStatus::Paused);
        assert_eq!(state.mentor_activity.label, "Paused");
        assert_eq!(state.executor_activity.label, "Paused");
        assert_eq!(
            state.mentor_activity.detail.as_deref(),
            Some("Paused by user")
        );
        assert_eq!(
            state.executor_activity.detail.as_deref(),
            Some("Paused by user")
        );
        assert!(matches!(state.mentor_activity.phase, ActivityPhase::Idle));
        assert!(matches!(state.executor_activity.phase, ActivityPhase::Idle));
    }

    #[test]
    fn add_message_only_persists_high_signal_messages_and_handoffs_turns() {
        let broker = MessageBroker::new();
        broker
            .initialize_pair("pair-1", sample_input(), None)
            .unwrap();

        broker.add_message(
            "pair-1",
            Message {
                id: "msg-1".to_string(),
                timestamp: 1,
                from: MessageSender::Mentor,
                to: "executor".to_string(),
                msg_type: MessageType::Plan,
                content: "Plan the work".to_string(),
                iteration: 0,
                token_usage: None,
            },
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(state.iteration, 0);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(
            state
                .mentor
                .last_message
                .as_ref()
                .map(|message| message.content.as_str()),
            Some("Plan the work")
        );

        broker.add_message(
            "pair-1",
            Message {
                id: "msg-2".to_string(),
                timestamp: 2,
                from: MessageSender::Executor,
                to: "mentor".to_string(),
                msg_type: MessageType::Progress,
                content: "Still working".to_string(),
                iteration: 0,
                token_usage: None,
            },
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(
            state.messages.len(),
            1,
            "progress logs should stay out of history"
        );
        assert!(state.executor.last_message.is_none());

        broker.add_message(
            "pair-1",
            Message {
                id: "msg-3".to_string(),
                timestamp: 3,
                from: MessageSender::Mentor,
                to: "executor".to_string(),
                msg_type: MessageType::Handoff,
                content: "Handoff to executor".to_string(),
                iteration: 0,
                token_usage: None,
            },
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(state.turn, AgentRole::Executor);
        assert_eq!(state.iteration, 1);
        assert_eq!(state.messages.len(), 1);
    }

    #[test]
    fn record_human_feedback_approval_persists_feedback_and_returns_next_role() {
        let broker = MessageBroker::new();
        broker
            .initialize_pair("pair-1", sample_input(), None)
            .unwrap();
        broker
            .restore_state(pair_state(PairStatus::AwaitingHumanReview, 2))
            .unwrap();

        let next_role = broker
            .record_human_feedback("pair-1", true)
            .expect("approval should succeed");

        assert_eq!(next_role, Some(AgentRole::Executor));

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(state.status, PairStatus::AwaitingHumanReview);
        assert_eq!(state.messages.len(), 1);
        assert_eq!(state.messages[0].from, MessageSender::Human);
        assert_eq!(state.messages[0].msg_type, MessageType::Feedback);
        assert_eq!(
            state.messages[0].content,
            "Human approved review. Continuing."
        );
    }

    fn paused_mentor_planning_state() -> PairState {
        let mut state = pair_state(PairStatus::Paused, 1);
        state.turn = AgentRole::Mentor;
        state.status = PairStatus::Paused;
        state.mentor.status = PairStatus::Paused;
        state.executor.status = PairStatus::Paused;
        state
    }

    fn paused_mentor_review_state() -> PairState {
        let mut state = pair_state(PairStatus::Paused, 2);
        state.turn = AgentRole::Mentor;
        state.status = PairStatus::Paused;
        state.mentor.status = PairStatus::Paused;
        state.executor.status = PairStatus::Paused;
        state
    }

    fn paused_executor_state() -> PairState {
        let mut state = pair_state(PairStatus::Paused, 2);
        state.turn = AgentRole::Executor;
        state.status = PairStatus::Paused;
        state.mentor.status = PairStatus::Paused;
        state.executor.status = PairStatus::Paused;
        state
    }

    #[test]
    fn resume_run_restores_paused_mentor_planning_as_mentoring_not_reviewing() {
        let broker = MessageBroker::new();
        broker
            .initialize_pair("pair-1", sample_input(), None)
            .unwrap();
        broker
            .restore_state(paused_mentor_planning_state())
            .unwrap();

        broker.resume_run(
            "pair-1",
            "mentor",
            Arc::new(Mutex::new(HashMap::<String, tokio::process::Child>::new())),
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(
            state.status,
            PairStatus::Mentoring,
            "status should be Mentoring, not Reviewing"
        );
        assert_eq!(
            state.iteration, 1,
            "iteration should be preserved (1), not incremented"
        );
        assert_eq!(state.turn, AgentRole::Mentor, "turn should be Mentor");
        assert_eq!(
            state.mentor_activity.label, "Analyzing task",
            "mentor should be in planning mode"
        );
        assert!(matches!(
            state.mentor_activity.phase,
            ActivityPhase::Thinking
        ));
        assert!(matches!(
            state.executor_activity.phase,
            ActivityPhase::Waiting
        ));
        assert_eq!(
            state.executor_activity.label, "Executor standing by",
            "executor should be standing by"
        );
        assert_eq!(
            state.mentor.status,
            PairStatus::Executing,
            "mentor status should be Executing (not flattened to Mentoring)"
        );
        assert_eq!(
            state.executor.status,
            PairStatus::Idle,
            "executor status should be Idle (not flattened)"
        );
    }

    #[test]
    fn resume_run_restores_paused_mentor_review_as_reviewing() {
        let broker = MessageBroker::new();
        broker
            .initialize_pair("pair-1", sample_input(), None)
            .unwrap();
        broker.restore_state(paused_mentor_review_state()).unwrap();

        broker.resume_run(
            "pair-1",
            "mentor",
            Arc::new(Mutex::new(HashMap::<String, tokio::process::Child>::new())),
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(
            state.status,
            PairStatus::Reviewing,
            "status should be Reviewing for review turn"
        );
        assert_eq!(
            state.iteration, 2,
            "iteration should be preserved (2), not incremented"
        );
        assert_eq!(
            state.mentor_activity.label, "Reviewing changes",
            "mentor should be in review mode"
        );
        assert!(matches!(
            state.executor_activity.phase,
            ActivityPhase::Waiting
        ));
        assert_eq!(
            state.mentor.status,
            PairStatus::Reviewing,
            "mentor status should be Reviewing (not flattened)"
        );
        assert_eq!(
            state.executor.status,
            PairStatus::Idle,
            "executor status should be Idle (not flattened)"
        );
    }

    #[test]
    fn resume_run_restores_paused_executor_as_executing() {
        let broker = MessageBroker::new();
        broker
            .initialize_pair("pair-1", sample_input(), None)
            .unwrap();
        broker.restore_state(paused_executor_state()).unwrap();

        broker.resume_run(
            "pair-1",
            "executor",
            Arc::new(Mutex::new(HashMap::<String, tokio::process::Child>::new())),
        );

        let state = broker.get_state("pair-1").expect("pair state should exist");
        assert_eq!(
            state.status,
            PairStatus::Executing,
            "status should be Executing for executor resume"
        );
        assert_eq!(
            state.iteration, 2,
            "iteration should be preserved (2), not incremented"
        );
        assert_eq!(
            state.executor_activity.label, "Executing plan",
            "executor should be executing"
        );
        assert!(matches!(
            state.executor_activity.phase,
            ActivityPhase::Thinking
        ));
        assert!(matches!(
            state.mentor_activity.phase,
            ActivityPhase::Waiting
        ));
        assert_eq!(
            state.mentor_activity.label, "Mentor observing",
            "mentor should be observing"
        );
        assert_eq!(
            state.executor.status,
            PairStatus::Executing,
            "executor status should be Executing (not flattened)"
        );
        assert_eq!(
            state.mentor.status,
            PairStatus::Idle,
            "mentor status should be Idle (not flattened)"
        );
    }
}
