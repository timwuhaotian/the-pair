use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub fn is_mock_mode() -> bool {
    std::env::var("THE_PAIR_E2E_MOCK")
        .map(|v| v == "true")
        .unwrap_or(false)
}

fn is_dev_smoke_pair_spec(spec: &str) -> bool {
    spec.contains("This is a smoke test of the pair execution loop")
        && spec.contains("Each time the executor sends a greeting")
        && spec.contains("Greeting N/3 received.")
}

pub fn build_mentor_planning_prompt(task_spec: &str) -> String {
    if is_dev_smoke_pair_spec(task_spec) {
        format!(
            "ROLE: MENTOR (Smoke Test).\n\
You are running a 3-round pair loop smoke test:\n\
- Round 1: Plan → Executor sends Greeting 1 → You review it\n\
- Round 2: Executor sends Greeting 2 → You review it\n\
- Round 3: Executor sends Greeting 3 → You review and finish\n\n\
For this PLANNING turn: Output a single instruction: \"Send Greeting 1/3\".\n\
DO NOT run commands or edit files. DO NOT respond with a greeting count yet.\n\n\
In future REVIEW turns you MUST output a strict JSON acceptance verdict:\n\
- If greeting 1 or 2: FAIL verdict, risk=low, action=continue, instructions=[\"Send Greeting {{N+1}}/3\"]\n\
- If greeting 3: PASS verdict, risk=low, action=finish, confidence=1.0, instructions=[], include TASK_COMPLETE on its own line after the JSON\n\n\
Always include \"Greeting N/3 received\" somewhere in your response text (outside the JSON).",
        )
    } else {
        format!(
            "ROLE: MENTOR. Analyze the following task and provide a detailed PLAN for the EXECUTOR. \
DO NOT execute it yourself. \
DO NOT run commands or edit files. \
Return ONLY a concrete PLAN with numbered executable steps (no intent-only preface).\n\nTASK: {}",
            task_spec
        )
    }
}
