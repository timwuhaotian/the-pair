use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn is_mock_mode() -> bool {
    std::env::var("THE_PAIR_E2E_MOCK")
        .map(|v| v == "true")
        .unwrap_or(false)
}

pub fn build_mentor_planning_prompt(task_spec: &str) -> String {
    format!(
        "You're collaborating with another AI agent in an automated pair-programming workflow. On this turn you're planning the work — read the task below, then outline a concrete plan the executor agent can carry out next.\n\n\
Write a numbered plan with concrete steps. You don't need to run commands or edit files on this turn; the executor handles that after handoff.\n\n\
TASK\n{}",
        task_spec
    )
}
