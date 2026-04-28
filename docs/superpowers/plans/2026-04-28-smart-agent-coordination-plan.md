# Smart Agent Coordination Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add adaptive stopping, review quality gates, context preservation, and smart pause triggers to the Mentor/Executor coordination loop.

**Architecture:** Four new Rust modules (`adaptive_stop`, `context_bridge`, `quality_gate`, `smart_pause`) integrated into the existing `process_spawner` (iteration loop), `acceptance` (verdict parsing), and `message_broker` (state management). All features degrade gracefully to current behavior.

**Tech Stack:** Rust (Tauri backend), existing message broker state machine, git_tracker for diff analysis

---

## Chunk 1: Adaptive Stop Module

### Task 1: Create adaptive_stop.rs with budget computation

**Files:**
- Create: `src-tauri/src/adaptive_stop.rs`
- Test: `src-tauri/src/adaptive_stop.rs` (inline tests)

- [ ] **Step 1: Define the budget computation logic**

Create `src-tauri/src/adaptive_stop.rs`:

```rust
/// Complexity tier for adaptive iteration budgeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityTier {
    None,   /// No changes detected
    Simple, /// ≤5 files, <20 lines
    Medium, /// ≤15 files, <100 lines
    Complex,/// >15 files or >100 lines
}

impl ComplexityTier {
    pub fn max_iterations(self, configured_max: u32) -> u32 {
        let budget = match self {
            ComplexityTier::None => 1,
            ComplexityTier::Simple => 3,
            ComplexityTier::Medium => 6,
            ComplexityTier::Complex => 10,
        };
        budget.min(configured_max)
    }
}

/// Weight factor for file types.
fn file_type_weight(path: &str) -> f64 {
    if path.ends_with(".test.") || path.ends_with("_test.") || path.ends_with(".spec.") {
        0.7 // Tests are simpler changes
    } else if path.ends_with(".json") || path.ends_with(".yaml") || path.ends_with(".yml") || path.ends_with(".toml") {
        0.8 // Config files are simpler
    } else if path.contains("/src/") || path.contains("/lib/") {
        1.2 // Business logic is more complex
    } else {
        1.0
    }
}

/// Computes the complexity tier from diff metrics.
/// Uses weighted line count based on file types touched.
pub fn compute_tier(files_changed: usize, total_diff_lines: usize, changed_files: &[String]) -> ComplexityTier {
    if files_changed == 0 && total_diff_lines == 0 {
        return ComplexityTier::None;
    }

    // Weighted line count
    let weighted_lines: f64 = if changed_files.is_empty() {
        total_diff_lines as f64
    } else {
        let avg_weight: f64 = changed_files.iter()
            .map(|f| file_type_weight(f))
            .sum::<f64>() / changed_files.len() as f64;
        total_diff_lines as f64 * avg_weight
    };

    let weighted = weighted_lines as usize;

    if files_changed <= 5 && weighted < 20 {
        return ComplexityTier::Simple;
    }
    if files_changed <= 15 && weighted < 100 {
        return ComplexityTier::Medium;
    }
    ComplexityTier::Complex
}

/// Computes the adaptive iteration budget.
pub fn compute_adaptive_budget(
    files_changed: usize,
    total_diff_lines: usize,
    changed_files: &[String],
    configured_max: u32,
) -> u32 {
    compute_tier(files_changed, total_diff_lines, changed_files).max_iterations(configured_max)
}

/// Computes diff metrics from git tracking state.
/// Returns (files_changed, total_diff_lines, file_list).
/// Falls back to (0, 0, []) on error for graceful degradation.
pub fn diff_metrics_from_tracking(
    git_tracking: &crate::types::GitTracking,
) -> (usize, usize, Vec<String>) {
    let mut files = Vec::new();
    let mut total_lines = 0;

    // Count from modified/added/deleted entries
    for entry in &git_tracking.modified {
        files.push(entry.path.clone());
        total_lines += entry.lines_changed.unwrap_or(0) as usize;
    }
    for entry in &git_tracking.added {
        files.push(entry.path.clone());
        total_lines += entry.lines_changed.unwrap_or(0) as usize;
    }
    for entry in &git_tracking.deleted {
        files.push(entry.path.clone());
        total_lines += entry.lines_changed.unwrap_or(0) as usize;
    }

    (files.len(), total_lines, files)
}
```

- [ ] **Step 2: Add inline unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_tier() {
        assert_eq!(compute_tier(0, 0, &[]), ComplexityTier::None);
    }

    #[test]
    fn test_simple_tier() {
        assert_eq!(compute_tier(3, 15, &["src/main.rs".into()]), ComplexityTier::Simple);
    }

    #[test]
    fn test_medium_tier() {
        assert_eq!(compute_tier(10, 50, &["src/a.rs".into()]), ComplexityTier::Medium);
    }

    #[test]
    fn test_complex_tier() {
        assert_eq!(compute_tier(20, 150, &["src/a.rs".into()]), ComplexityTier::Complex);
    }

    #[test]
    fn test_budget_respects_configured_max() {
        assert_eq!(compute_adaptive_budget(20, 150, &[], 5), 5);
    }

    #[test]
    fn test_file_type_weight_test_files() {
        assert!(file_type_weight("src/foo.test.ts") < 1.0);
    }

    #[test]
    fn test_file_type_weight_config_files() {
        assert!(file_type_weight("package.json") < 1.0);
    }

    #[test]
    fn test_file_type_weight_business_logic() {
        assert!(file_type_weight("src/lib/service.rs") > 1.0);
    }

    #[test]
    fn test_weighted_tier_boundary() {
        // 15 lines in a test file (0.7 weight) = 10.5 weighted → still simple
        let tier = compute_tier(1, 15, &["src/foo.test.ts".into()]);
        assert_eq!(tier, ComplexityTier::Simple);
    }
}
```

- [ ] **Step 3: Register module in lib.rs**

Add to `src-tauri/src/lib.rs`:

```rust
mod adaptive_stop;
```

- [ ] **Step 4: Run tests and commit**

```bash
cd src-tauri && cargo test adaptive_stop -- --nocapture
git add src-tauri/src/adaptive_stop.rs src-tauri/src/lib.rs
git commit -m "feat: add adaptive_stop module with complexity-based budgeting"
```

---

## Chunk 2: Context Bridge Module

### Task 2: Create context_bridge.rs with handoff context builder

**Files:**
- Create: `src-tauri/src/context_bridge.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/context_bridge.rs` (inline tests)

- [ ] **Step 1: Define handoff context structure and formatters**

Create `src-tauri/src/context_bridge.rs`:

```rust
/// Structured context passed between agent turns.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HandoffContext {
    pub task_spec: String,
    pub current_sub_goal: String,
    pub progress_summary: String,
    pub key_decisions: Vec<String>,
    pub plan_checklist: Vec<PlanItem>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanItem {
    pub description: String,
    pub completed: bool,
}

pub fn build_handoff_context(
    task_spec: &str,
    current_sub_goal: &str,
    progress_summary: &str,
    key_decisions: &[String],
    plan_checklist: &[PlanItem],
) -> HandoffContext {
    HandoffContext {
        task_spec: task_spec.to_string(),
        current_sub_goal: current_sub_goal.to_string(),
        progress_summary: progress_summary.to_string(),
        key_decisions: key_decisions.to_vec(),
        plan_checklist: plan_checklist.to_vec(),
    }
}

pub fn format_mentor_prompt(context: &HandoffContext) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("## Original Task\n{}\n\n", context.task_spec));
    if !context.progress_summary.is_empty() {
        prompt.push_str(&format!("## Progress So Far\n{}\n\n", context.progress_summary));
    }
    if !context.key_decisions.is_empty() {
        prompt.push_str("## Key Decisions Made\n");
        for d in &context.key_decisions {
            prompt.push_str(&format!("- {}\n", d));
        }
        prompt.push('\n');
    }
    prompt.push_str("## Your Task\nReview the current state and provide a structured plan as a checklist.\n");
    prompt
}

pub fn format_executor_prompt(context: &HandoffContext) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("## Original Task\n{}\n\n", context.task_spec));
    prompt.push_str(&format!("## Your Goal\n{}\n\n", context.current_sub_goal));
    if !context.progress_summary.is_empty() {
        prompt.push_str(&format!("## Progress So Far\n{}\n\n", context.progress_summary));
    }
    if !context.plan_checklist.is_empty() {
        prompt.push_str("## Plan Checklist\nComplete these items:\n");
        for item in &context.plan_checklist {
            let mark = if item.completed { "x" } else { " " };
            prompt.push_str(&format!("- [{}] {}\n", mark, item.description));
        }
        prompt.push('\n');
    }
    prompt
}

pub fn format_review_prompt(context: &HandoffContext, diff_summary: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("## Original Task\n{}\n\n", context.task_spec));
    if !context.plan_checklist.is_empty() {
        prompt.push_str("## Expected Plan\n");
        for item in &context.plan_checklist {
            let mark = if item.completed { "x" } else { " " };
            prompt.push_str(&format!("- [{}] {}\n", mark, item.description));
        }
        prompt.push('\n');
    }
    prompt.push_str(&format!("## Changes Made\n{}\n\n", diff_summary));
    prompt.push_str("## Your Task\nReview the changes against the plan and task spec. Provide structured evidence for your verdict.\n");
    prompt
}

/// Parses a checklist from mentor output text.
/// Looks for lines matching "- [ ] item" or "- [x] item".
pub fn parse_checklist(text: &str) -> Vec<PlanItem> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("- [") {
                let completed = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
                let desc = trimmed.trim_start_matches("- [ ]").trim_start_matches("- [x]").trim_start_matches("- [X]").trim();
                if !desc.is_empty() {
                    return Some(PlanItem { description: desc.to_string(), completed });
                }
            }
            None
        })
        .collect()
}
```

- [ ] **Step 2: Add inline unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_context() {
        let ctx = build_handoff_context("Fix bug", "Goal", "Done", &["decided"], &[PlanItem { description: "Step".into(), completed: false }]);
        assert_eq!(ctx.task_spec, "Fix bug");
        assert_eq!(ctx.plan_checklist.len(), 1);
    }

    #[test]
    fn test_mentor_prompt_sections() {
        let ctx = build_handoff_context("Task", "", "", &[], &[]);
        let p = format_mentor_prompt(&ctx);
        assert!(p.contains("Original Task") && p.contains("Your Task"));
    }

    #[test]
    fn test_executor_prompt_checklist() {
        let ctx = build_handoff_context("Task", "Goal", "", &[], &[PlanItem { description: "Step 1".into(), completed: false }]);
        assert!(format_executor_prompt(&ctx).contains("[ ] Step 1"));
    }

    #[test]
    fn test_parse_checklist() {
        let text = "- [ ] Do something\n- [x] Done thing";
        let items = parse_checklist(text);
        assert_eq!(items.len(), 2);
        assert!(!items[0].completed);
        assert!(items[1].completed);
    }
}
```

- [ ] **Step 3: Register module in lib.rs**

```rust
mod context_bridge;
```

- [ ] **Step 4: Run tests and commit**

```bash
cd src-tauri && cargo test context_bridge -- --nocapture
git add src-tauri/src/context_bridge.rs src-tauri/src/lib.rs
git commit -m "feat: add context_bridge for structured handoff prompts"
```

---

## Chunk 3: Quality Gate Module

### Task 3: Create quality_gate.rs with evidence validation

**Files:**
- Create: `src-tauri/src/quality_gate.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/acceptance.rs` (add quality gate check before verdict parsing)
- Test: `src-tauri/src/quality_gate.rs` (inline tests)

- [ ] **Step 1: Define quality gate logic**

Create `src-tauri/src/quality_gate.rs`:

```rust
/// Structured evidence extracted from a mentor review verdict.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewEvidence {
    pub files_reviewed: Vec<String>,
    pub checks_performed: Vec<String>,
    pub code_reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualityGateResult {
    Pass,
    Fail { reason: String },
}

/// Validates review evidence meets the quality threshold.
pub fn validate_review(evidence: &ReviewEvidence) -> QualityGateResult {
    if evidence.files_reviewed.is_empty() {
        return QualityGateResult::Fail {
            reason: "No files listed as reviewed. Please specify which files you reviewed.".into(),
        };
    }
    if evidence.checks_performed.is_empty() {
        return QualityGateResult::Fail {
            reason: "No specific checks listed. Describe what you checked (error handling, edge cases, type safety).".into(),
        };
    }
    if evidence.code_reference.trim().is_empty() {
        return QualityGateResult::Fail {
            reason: "No code reference provided. Quote or reference the changed code you validated.".into(),
        };
    }
    QualityGateResult::Pass
}

/// Extracts evidence from a mentor verdict message.
/// Expects structured sections: FILES_REVIEWED:, CHECKS:, CODE:
pub fn extract_evidence(verdict_text: &str) -> Option<ReviewEvidence> {
    let files_line = verdict_text.lines().find(|l| l.starts_with("FILES_REVIEWED:"))?;
    let checks_line = verdict_text.lines().find(|l| l.starts_with("CHECKS:"))?;
    let code_line = verdict_text.lines().find(|l| l.starts_with("CODE:"))?;

    let files_reviewed = files_line["FILES_REVIEWED:".len()..]
        .split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let checks_performed = checks_line["CHECKS:".len()..]
        .split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let code_reference = code_line["CODE:".len()..].trim().to_string();

    Some(ReviewEvidence { files_reviewed, checks_performed, code_reference })
}
```

- [ ] **Step 2: Add inline unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_pass() {
        let e = ReviewEvidence {
            files_reviewed: vec!["src/main.rs".into()],
            checks_performed: vec!["error handling".into()],
            code_reference: "Result<T,E>".into(),
        };
        assert_eq!(validate_review(&e), QualityGateResult::Pass);
    }

    #[test]
    fn test_validate_fail_no_files() {
        let e = ReviewEvidence { files_reviewed: vec![], checks_performed: vec!["x".into()], code_reference: "x".into() };
        assert!(matches!(validate_review(&e), QualityGateResult::Fail { .. }));
    }

    #[test]
    fn test_validate_fail_no_checks() {
        let e = ReviewEvidence { files_reviewed: vec!["x".into()], checks_performed: vec![], code_reference: "x".into() };
        assert!(matches!(validate_review(&e), QualityGateResult::Fail { .. }));
    }

    #[test]
    fn test_validate_fail_no_code() {
        let e = ReviewEvidence { files_reviewed: vec!["x".into()], checks_performed: vec!["x".into()], code_reference: "".into() };
        assert!(matches!(validate_review(&e), QualityGateResult::Fail { .. }));
    }

    #[test]
    fn test_extract_evidence() {
        let text = "FILES_REVIEWED: src/main.rs, src/utils.rs\nCHECKS: error handling, edge cases\nCODE: handle_login returns Result\nVERDICT: PASS";
        let e = extract_evidence(text).unwrap();
        assert_eq!(e.files_reviewed, vec!["src/main.rs", "src/utils.rs"]);
        assert_eq!(e.checks_performed, vec!["error handling", "edge cases"]);
    }

    #[test]
    fn test_extract_evidence_missing_section() {
        let text = "CHECKS: x\nCODE: y";
        assert!(extract_evidence(text).is_none());
    }
}
```

- [ ] **Step 3: Register module in lib.rs**

```rust
mod quality_gate;
```

- [ ] **Step 4: Run tests and commit**

```bash
cd src-tauri && cargo test quality_gate -- --nocapture
git add src-tauri/src/quality_gate.rs src-tauri/src/lib.rs
git commit -m "feat: add quality_gate for review evidence validation"
```

---

## Chunk 4: Smart Pause Module

### Task 4: Create smart_pause.rs with pause trigger logic

**Files:**
- Create: `src-tauri/src/smart_pause.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/smart_pause.rs` (inline tests)

- [ ] **Step 1: Define smart pause logic**

Create `src-tauri/src/smart_pause.rs`:

```rust
/// Reasons why a pair run pauses for human intervention.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum PauseReason {
    BudgetExhausted { iterations: u32, budget: u32 },
    ScopeDrift { planned_files: Vec<String>, actual_files: Vec<String> },
}

/// Determines whether to pause based on current state.
pub fn should_pause(
    current_iteration: u32,
    adaptive_budget: u32,
    configured_max: u32,
    planned_files: &[String],
    actual_changed_files: &[String],
    task_complete: bool,
) -> Option<PauseReason> {
    if task_complete {
        return None;
    }

    let effective_budget = adaptive_budget.min(configured_max);
    if current_iteration >= effective_budget {
        return Some(PauseReason::BudgetExhausted {
            iterations: current_iteration,
            budget: effective_budget,
        });
    }

    if !planned_files.is_empty() && !actual_changed_files.is_empty() {
        let unplanned: Vec<_> = actual_changed_files.iter()
            .filter(|f| !planned_files.contains(f))
            .collect();
        if !unplanned.is_empty() {
            return Some(PauseReason::ScopeDrift {
                planned_files: planned_files.to_vec(),
                actual_files: actual_changed_files.to_vec(),
            });
        }
    }

    None
}

/// Generates a human-readable pause message.
pub fn format_pause_message(reason: &PauseReason) -> String {
    match reason {
        PauseReason::BudgetExhausted { iterations, budget } => {
            format!("Agent reached iteration budget ({iterations}/{budget}). The task may need refinement or the budget should be increased.")
        }
        PauseReason::ScopeDrift { planned_files, actual_files } => {
            let unplanned: Vec<_> = actual_files.iter().filter(|f| !planned_files.contains(f)).collect();
            format!("Scope drift detected. Executor modified files outside the plan: {}. Please review and decide whether to allow these changes.", unplanned.join(", "))
        }
    }
}
```

- [ ] **Step 2: Add inline unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_pause_when_complete() {
        assert!(should_pause(5, 3, 10, &[], &[], true).is_none());
    }

    #[test]
    fn test_pause_budget() {
        let r = should_pause(4, 3, 10, &[], &[], false);
        assert!(matches!(r, Some(PauseReason::BudgetExhausted { .. })));
    }

    #[test]
    fn test_pause_scope_drift() {
        let r = should_pause(1, 3, 10, &["src/main.rs".into()], &["src/main.rs".into(), "src/new.rs".into()], false);
        assert!(matches!(r, Some(PauseReason::ScopeDrift { .. })));
    }

    #[test]
    fn test_no_drift_when_subset() {
        let r = should_pause(1, 3, 10, &["a.rs".into(), "b.rs".into()], &["a.rs".into()], false);
        assert!(r.is_none());
    }

    #[test]
    fn test_pause_message_budget() {
        assert!(format_pause_message(&PauseReason::BudgetExhausted { iterations: 3, budget: 3 }).contains("3/3"));
    }
}
```

- [ ] **Step 3: Register module in lib.rs**

```rust
mod smart_pause;
```

- [ ] **Step 4: Run tests and commit**

```bash
cd src-tauri && cargo test smart_pause -- --nocapture
git add src-tauri/src/smart_pause.rs src-tauri/src/lib.rs
git commit -m "feat: add smart_pause with budget and scope drift detection"
```

---

## Chunk 5: Integrate into Process Spawner

### Task 5: Wire AdaptiveStop + SmartPause into the iteration loop

**Files:**
- Modify: `src-tauri/src/process_spawner.rs` (lines ~1057-1077 mock, ~1857-1875 real loop)
- Modify: `src-tauri/src/types.rs` (add `adaptive_budget` and `pause_message` to `PairState`)
- Test: `src-tauri/src/adaptive_stop.rs` (already tested), `src-tauri/src/smart_pause.rs` (already tested)

- [ ] **Step 1: Add new fields to PairState**

In `src-tauri/src/types.rs`, add to `PairState` (line ~343, before closing brace):

```rust
    #[serde(rename = "adaptiveBudget", default, skip_serializing_if = "Option::is_none")]
    pub adaptive_budget: Option<u32>,
    #[serde(rename = "pauseMessage", default, skip_serializing_if = "Option::is_none")]
    pub pause_message: Option<String>,
    #[serde(rename = "planChecklist", default)]
    pub plan_checklist: Vec<serde_json::Value>,
    #[serde(rename = "keyDecisions", default)]
    pub key_decisions: Vec<String>,
```

- [ ] **Step 2: Replace hardcoded max_iterations check in real loop**

In `src-tauri/src/process_spawner.rs`, at line ~1857-1875, replace:

```rust
// BEFORE (lines ~1857-1875):
} else if role_clone == "executor" || role_clone == "mentor" {
    if let Some(state) = broker.get_state(&pair_id_clone) {
        if state.iteration >= state.max_iterations {
            // ... set Paused status ...
        }
    }
}
```

With:

```rust
} else if role_clone == "executor" || role_clone == "mentor" {
    if let Some(state) = broker.get_state(&pair_id_clone) {
        // Use adaptive budget if available, otherwise fall back to configured max
        let effective_budget = state.adaptive_budget.unwrap_or(state.max_iterations);
        if state.iteration >= effective_budget {
            let pause_reason = crate::smart_pause::PauseReason::BudgetExhausted {
                iterations: state.iteration,
                budget: effective_budget,
            };
            let pause_msg = crate::smart_pause::format_pause_message(&pause_reason);
            println!(
                "[ProcessSpawner] [{}] Adaptive budget exhausted ({}), pausing for human review",
                pair_id_clone, effective_budget
            );
            broker.set_pair_status(
                &pair_id_clone,
                crate::types::PairStatus::Paused,
                Some(pause_msg),
            );
            should_handoff = false;
        }
    }
}
```

- [ ] **Step 3: Add adaptive budget computation after executor turn**

In the executor turn completion path (near line ~1857, before the iteration check), add:

```rust
// Compute adaptive budget from git tracking state
if role_clone == "executor" {
    if let Some(state) = broker.get_state(&pair_id_clone) {
        let (files_changed, total_lines, file_list) =
            crate::adaptive_stop::diff_metrics_from_tracking(&state.git_tracking);
        let budget = crate::adaptive_stop::compute_adaptive_budget(
            files_changed, total_lines, &file_list, state.max_iterations,
        );
        // Store budget in state (requires a method on MessageBroker or direct state update)
        // For now, emit via a new broker method or use set_pair_status side-channel
        // The cleanest approach: add set_adaptive_budget to MessageBroker
    }
}
```

- [ ] **Step 4: Run tests and commit**

```bash
cd src-tauri && cargo test -- --nocapture
git add src-tauri/src/process_spawner.rs src-tauri/src/types.rs
git commit -m "feat: integrate adaptive_stop and smart_pause into process_spawner loop"
```

---

## Chunk 6: Integrate QualityGate into Acceptance

### Task 6: Wire quality gate into acceptance verdict parsing

**Files:**
- Modify: `src-tauri/src/acceptance.rs`
- Test: `src-tauri/src/quality_gate.rs` (already tested)

- [ ] **Step 1: Add quality gate wrapper around parse_acceptance_verdict**

In `src-tauri/src/acceptance.rs`, add a new function near `parse_acceptance_verdict` (after line ~332):

```rust
/// Parses and validates a mentor review verdict through the quality gate.
/// Returns Err with a specific reason if evidence is insufficient.
pub fn parse_review_verdict_with_quality(raw: &str) -> Result<AcceptanceVerdict, String> {
    // First, try to extract and validate structured evidence
    if let Some(evidence) = crate::quality_gate::extract_evidence(raw) {
        match crate::quality_gate::validate_review(&evidence) {
            crate::quality_gate::QualityGateResult::Fail { reason } => {
                return Err(format!("Review quality gate failed: {}", reason));
            }
            crate::quality_gate::QualityGateResult::Pass => {
                // Evidence is valid, proceed with normal parsing
            }
        }
    }
    // Fall through to normal parsing (graceful degradation)
    parse_acceptance_verdict(raw)
}
```

- [ ] **Step 2: Update the verdict parsing call site**

In `src-tauri/src/process_spawner.rs` at line ~530, where `parse_acceptance_verdict(raw_output)` is called, replace with `parse_review_verdict_with_quality(raw_output)`. Also update the import at line 2.

- [ ] **Step 3: Update mentor prompt to require structured format**

Find where the mentor review prompt is constructed (in `process_spawner.rs` or the provider adapter). Append:

```
Your verdict must include these sections:
FILES_REVIEWED: <comma-separated list of files you reviewed>
CHECKS: <comma-separated list of checks performed, e.g., error handling, edge cases, type safety>
CODE: <quote or reference of the changed code you validated>
Then provide your JSON verdict as usual.
```

- [ ] **Step 4: Run tests and commit**

```bash
cd src-tauri && cargo test -- --nocapture
git add src-tauri/src/acceptance.rs src-tauri/src/process_spawner.rs
git commit -m "feat: integrate quality_gate into acceptance verdict parsing"
```

---

## Chunk 7: Integrate ContextBridge into Prompt Construction

### Task 7: Wire context bridge into turn prompts

**Files:**
- Modify: `src-tauri/src/process_spawner.rs` (prompt construction areas)
- Test: `src-tauri/src/context_bridge.rs` (already tested)

- [ ] **Step 1: Find prompt construction points**

Read `process_spawner.rs` around the `build_turn_command` call (line ~1099-1107). The `message` parameter contains the prompt. Find where this message is constructed (likely in `pair_manager.rs` or the caller of `spawn_agent_process`).

- [ ] **Step 2: Build HandoffContext and format prompts**

At the prompt construction site, build the `HandoffContext` from pair state and use the appropriate formatter (`format_mentor_prompt`, `format_executor_prompt`, `format_review_prompt`) instead of the existing raw string.

- [ ] **Step 3: Parse plan checklist from mentor output**

After the mentor turn completes (in the event processing loop), call `context_bridge::parse_checklist(&mentor_output)` and store the result in `PairState.plan_checklist`.

- [ ] **Step 4: Run tests and commit**

```bash
cd src-tauri && cargo test -- --nocapture
git add src-tauri/src/process_spawner.rs
git commit -m "feat: integrate context_bridge into turn prompt construction"
```

---

## Chunk 8: Add MessageBroker helper methods

### Task 8: Add broker methods for new state fields

**Files:**
- Modify: `src-tauri/src/message_broker.rs`

- [ ] **Step 1: Add set_adaptive_budget method**

In `message_broker.rs`, add:

```rust
pub fn set_adaptive_budget(&self, pair_id: &str, budget: u32) {
    let mut states = self.pair_states.lock().unwrap();
    if let Some(state) = states.get_mut(pair_id) {
        state.adaptive_budget = Some(budget);
    }
}
```

- [ ] **Step 2: Add set_plan_checklist method**

```rust
pub fn set_plan_checklist(&self, pair_id: &str, checklist: Vec<PlanItem>) {
    let mut states = self.pair_states.lock().unwrap();
    if let Some(state) = states.get_mut(pair_id) {
        state.plan_checklist = checklist.into_iter().map(|item| serde_json::to_value(item).unwrap_or_default()).collect();
    }
}
```

- [ ] **Step 3: Run tests and commit**

```bash
cd src-tauri && cargo test -- --nocapture
git add src-tauri/src/message_broker.rs
git commit -m "feat: add broker methods for adaptive_budget and plan_checklist"
```

---

## Chunk 9: Frontend Integration

### Task 9: Surface smart coordination state in UI

**Files:**
- Modify: `src/renderer/src/components/IterationProgress.tsx`
- Modify: `src/renderer/src/store/usePairStore.ts` (type updates for new PairState fields)

- [ ] **Step 1: Update TypeScript types**

In the frontend, add the new fields to the `PairState` type:
- `adaptiveBudget?: number`
- `pauseMessage?: string`
- `planChecklist?: Array<{ description: string; completed: boolean }>`
- `keyDecisions?: string[]`

- [ ] **Step 2: Show adaptive budget in IterationProgress**

Update `IterationProgress.tsx` to display `adaptiveBudget` alongside or instead of `maxIterations`. Show "Iteration X / Y (adaptive)" when available.

- [ ] **Step 3: Show pause message in error/pause UI**

When status is `Paused`, display `pauseMessage` if available instead of the generic message.

- [ ] **Step 4: Run quality checks and commit**

```bash
npm run typecheck && npm run lint
git add src/renderer/src/
git commit -m "feat: surface smart coordination state in frontend"
```

---

## Chunk 10: Final Verification

### Task 10: Full test suite and quality gate

- [ ] **Step 1: Run full test suite**

```bash
npm test
```

- [ ] **Step 2: Run typecheck and lint**

```bash
npm run typecheck && npm run lint
```

- [ ] **Step 3: Commit final state**

```bash
git add -A
git commit -m "chore: smart agent coordination — full integration complete"
```
