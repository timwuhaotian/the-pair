use crate::types::{
    AcceptanceCheckRun, AcceptanceCheckStatus, AcceptanceNextAction, AcceptanceNextStep,
    AcceptanceRecord, AcceptanceRisk, AcceptanceVerdict, AcceptanceVerdictDecision, ModifiedFile,
};
use crate::util::now_millis;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Stdio;

/// Parses and validates a mentor review verdict through the quality gate.
/// First checks for structured evidence (FILES_REVIEWED/CHECKS/CODE).
/// If evidence is present but invalid, returns a specific quality error.
/// Falls back to normal parsing for graceful degradation.
pub fn parse_review_verdict_with_quality(raw: &str) -> Result<AcceptanceVerdict, String> {
    // Check for structured evidence format
    if raw.contains("FILES_REVIEWED:") || raw.contains("CHECKS:") || raw.contains("CODE:") {
        if let Some(evidence) = crate::quality_gate::extract_evidence(raw) {
            match crate::quality_gate::validate_review(&evidence) {
                crate::quality_gate::QualityGateResult::Fail { reason } => {
                    return Err(format!("Review quality gate: {}", reason));
                }
                crate::quality_gate::QualityGateResult::Pass => {
                    // Evidence valid, proceed with normal parsing
                }
            }
        } else {
            // Had markers but couldn't extract — quality issue
            return Err(
                "Review verdict has evidence markers but sections are incomplete.".into(),
            );
        }
    }
    // Fall through to normal parsing (graceful degradation)
    parse_acceptance_verdict(raw)
}
use std::time::Instant;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AcceptanceCheckPlan {
    name: String,
    command: String,
    program: String,
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LooseAcceptanceVerdict {
    verdict: AcceptanceVerdictDecision,
    risk: AcceptanceRisk,
    confidence: Option<f64>,
    #[serde(default)]
    issues: Vec<String>,
    evidence: Vec<String>,
    reasoning: Option<String>,
    summary: String,
    #[serde(alias = "next_step", alias = "next-step")]
    next_step: AcceptanceNextStep,
}

impl LooseAcceptanceVerdict {
    fn into_verdict(self) -> AcceptanceVerdict {
        let confidence = self.confidence.unwrap_or(match self.verdict {
            AcceptanceVerdictDecision::Pass => 1.0,
            AcceptanceVerdictDecision::Fail => 0.0,
        });
        let reasoning = self
            .reasoning
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.summary.clone());

        AcceptanceVerdict {
            verdict: self.verdict,
            risk: self.risk,
            confidence,
            issues: self.issues,
            evidence: self.evidence,
            reasoning,
            summary: self.summary,
            next_step: self.next_step,
        }
    }
}

impl AcceptanceCheckPlan {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        let program = program.into();
        let command = std::iter::once(program.clone())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");

        Self {
            name: command.clone(),
            command,
            program,
            args,
        }
    }
}

fn trim_output(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= max_chars {
        return trimmed.to_string();
    }
    chars[chars.len() - max_chars..].iter().collect()
}

fn parse_package_json(workspace_root: &Path) -> Option<Value> {
    let raw = fs::read_to_string(workspace_root.join("package.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

fn package_scripts(package_json: Option<&Value>) -> Vec<String> {
    package_json
        .and_then(|value| value.get("scripts"))
        .and_then(|value| value.as_object())
        .map(|scripts| scripts.keys().cloned().collect())
        .unwrap_or_default()
}

fn completion_signal_present(executor_output: &str) -> bool {
    let lower = executor_output.to_lowercase();
    [
        "done",
        "complete",
        "completed",
        "implemented",
        "fixed",
        "ready for review",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub fn classify_acceptance_risk(modified_files: &[ModifiedFile]) -> AcceptanceRisk {
    let total_files = modified_files.len();
    let has_delete_or_rename = modified_files.iter().any(|file| {
        matches!(
            file.status,
            crate::types::FileStatus::D | crate::types::FileStatus::R
        )
    });
    let has_migrations_or_schema = modified_files.iter().any(|file| {
        let path = file.path.to_lowercase();
        path.contains("migration") || path.contains("migrations") || path.contains("schema")
    });

    let mut has_backend = false;
    for file in modified_files {
        let path = file.path.to_lowercase();
        if path.contains("src-tauri/") || path.ends_with(".rs") {
            has_backend = true;
        }
    }

    if has_delete_or_rename || has_migrations_or_schema || total_files >= 12 {
        return AcceptanceRisk::High;
    }

    if total_files >= 6 || has_backend {
        return AcceptanceRisk::Medium;
    }

    AcceptanceRisk::Low
}

fn should_add_full_test(
    risk: &AcceptanceRisk,
    executor_output: &str,
    iteration: u32,
    max_iterations: u32,
) -> bool {
    if matches!(risk, AcceptanceRisk::Medium | AcceptanceRisk::High) {
        return true;
    }
    if completion_signal_present(executor_output) {
        return true;
    }
    max_iterations > 0 && iteration.saturating_add(1) >= max_iterations.saturating_sub(1)
}

fn build_acceptance_check_plan(
    workspace_root: &str,
    package_json_override: Option<&Value>,
    modified_files: &[ModifiedFile],
    executor_output: &str,
    iteration: u32,
    max_iterations: u32,
) -> Vec<AcceptanceCheckPlan> {
    let workspace_path = Path::new(workspace_root);
    let package_json = package_json_override
        .cloned()
        .or_else(|| parse_package_json(workspace_path));
    let scripts = package_scripts(package_json.as_ref());
    let risk = classify_acceptance_risk(modified_files);

    let mut checks = vec![AcceptanceCheckPlan::new(
        "git",
        vec!["diff".to_string(), "--check".to_string()],
    )];

    if scripts.iter().any(|script| script == "typecheck") {
        checks.push(AcceptanceCheckPlan::new(
            "npm",
            vec!["run".to_string(), "typecheck".to_string()],
        ));
    }

    if scripts.iter().any(|script| script == "test")
        && should_add_full_test(&risk, executor_output, iteration, max_iterations)
    {
        checks.push(AcceptanceCheckPlan::new(
            "npm",
            vec!["run".to_string(), "test".to_string()],
        ));
    }

    checks
}

async fn run_check(workspace_root: &Path, check: &AcceptanceCheckPlan) -> AcceptanceCheckRun {
    let started = Instant::now();
    let output = Command::new(&check.program)
        .args(&check.args)
        .current_dir(workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    match output {
        Ok(output) => {
            let success = output.status.success();
            AcceptanceCheckRun {
                name: check.name.clone(),
                command: check.command.clone(),
                status: if success {
                    AcceptanceCheckStatus::Passed
                } else {
                    AcceptanceCheckStatus::Failed
                },
                exit_code: output.status.code(),
                duration_ms: started.elapsed().as_millis() as u64,
                summary: if success {
                    format!("{} passed", check.command)
                } else {
                    format!("{} failed", check.command)
                },
                stdout: trim_output(&String::from_utf8_lossy(&output.stdout), 4_000),
                stderr: trim_output(&String::from_utf8_lossy(&output.stderr), 4_000),
            }
        }
        Err(error) => AcceptanceCheckRun {
            name: check.name.clone(),
            command: check.command.clone(),
            status: AcceptanceCheckStatus::Failed,
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as u64,
            summary: format!("{} could not start", check.command),
            stdout: String::new(),
            stderr: error.to_string(),
        },
    }
}

fn extract_json_candidates(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let mut candidates = Vec::new();
    if trimmed.is_empty() {
        return candidates;
    }

    candidates.push(trimmed.to_string());

    if trimmed.starts_with("```") {
        if let Some((_, rest)) = trimmed.split_once('\n') {
            if let Some(end) = rest.rfind("```") {
                candidates.push(rest[..end].trim().to_string());
            }
        }
    }

    let chars: Vec<char> = trimmed.chars().collect();
    for i in 0..chars.len() {
        if chars[i] != '{' {
            continue;
        }

        let mut depth = 0;
        let mut in_string = false;
        let mut escaped = false;

        for j in i..chars.len() {
            let ch = chars[j];

            if in_string {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            if ch == '"' {
                in_string = true;
                continue;
            }

            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    candidates.push(chars[i..=j].iter().collect());
                    break;
                }
            }
        }
    }

    candidates
}

pub fn parse_acceptance_verdict(raw: &str) -> Result<AcceptanceVerdict, String> {
    let mut last_error = "Acceptance verdict was empty".to_string();
    for candidate in extract_json_candidates(raw) {
        let parsed =
            serde_json::from_str::<AcceptanceVerdict>(&candidate).or_else(|strict_error| {
                last_error = strict_error.to_string();
                serde_json::from_str::<LooseAcceptanceVerdict>(&candidate)
                    .map(LooseAcceptanceVerdict::into_verdict)
            });

        match parsed {
            Ok(verdict) => match validate_acceptance_verdict(verdict) {
                Ok(validated) => return Ok(validated),
                Err(error) => return Err(error),
            },
            Err(error) => {
                last_error = error.to_string();
            }
        }
    }

    Err(last_error)
}

fn validate_acceptance_verdict(verdict: AcceptanceVerdict) -> Result<AcceptanceVerdict, String> {
    if verdict.confidence < 0.0 || verdict.confidence > 1.0 {
        return Err(format!(
            "Invalid confidence value: {}. Must be between 0.0 and 1.0",
            verdict.confidence
        ));
    }

    if matches!(verdict.next_step.action, AcceptanceNextAction::Continue)
        && verdict.next_step.instructions.is_empty()
    {
        return Err("Acceptance verdict requires instructions for continue".to_string());
    }
    if matches!(verdict.next_step.action, AcceptanceNextAction::Finish)
        && !verdict.next_step.instructions.is_empty()
    {
        return Err("Acceptance verdict cannot include instructions when finishing".to_string());
    }
    if matches!(verdict.verdict, AcceptanceVerdictDecision::Pass)
        && !matches!(verdict.next_step.action, AcceptanceNextAction::Finish)
    {
        return Err("Acceptance pass verdict must use nextStep.action finish".to_string());
    }
    if matches!(verdict.verdict, AcceptanceVerdictDecision::Fail)
        && !matches!(verdict.next_step.action, AcceptanceNextAction::Continue)
    {
        return Err("Acceptance fail verdict must use nextStep.action continue".to_string());
    }

    Ok(verdict)
}

pub const CONFIDENCE_THRESHOLD: f64 = 0.8;

pub fn should_stop_iteration(verdict: &AcceptanceVerdict) -> bool {
    matches!(verdict.verdict, AcceptanceVerdictDecision::Pass)
        && matches!(verdict.next_step.action, AcceptanceNextAction::Finish)
        && verdict.confidence >= CONFIDENCE_THRESHOLD
}

pub async fn run_acceptance_checks(
    workspace_root: &Path,
    modified_files: &[ModifiedFile],
    executor_output: &str,
    iteration: u32,
    max_iterations: u32,
) -> AcceptanceRecord {
    let started_at = now_millis();
    let checks = build_acceptance_check_plan(
        &workspace_root.to_string_lossy(),
        None,
        modified_files,
        executor_output,
        iteration,
        max_iterations,
    );

    let mut runs = Vec::with_capacity(checks.len());
    for check in &checks {
        runs.push(run_check(workspace_root, check).await);
    }

    let passed = runs
        .iter()
        .filter(|run| matches!(run.status, AcceptanceCheckStatus::Passed))
        .count();
    let failed = runs
        .iter()
        .filter(|run| matches!(run.status, AcceptanceCheckStatus::Failed))
        .count();
    let skipped = runs
        .iter()
        .filter(|run| matches!(run.status, AcceptanceCheckStatus::Skipped))
        .count();

    AcceptanceRecord {
        iteration,
        risk: classify_acceptance_risk(modified_files),
        checks: runs,
        summary: format!("{} passed, {} failed, {} skipped", passed, failed, skipped),
        started_at,
        finished_at: now_millis(),
        verdict: None,
        raw_verdict: None,
        error: None,
        repair_attempts: 0,
    }
}

pub fn build_mentor_acceptance_prompt(
    task_spec: &str,
    executor_result: &str,
    acceptance: &AcceptanceRecord,
) -> String {
    let is_smoke = task_spec.contains("This is a smoke test of the pair execution loop")
        && task_spec.contains("Each time the executor sends a greeting");

    let mut parts: Vec<String> = vec![
        "### ROLE: MENTOR".to_string(),
        "Your mission is ONLY to REVIEW and emit a structured acceptance verdict.".to_string(),
        "- DO NOT execute commands or edit files.".to_string(),
        "- Return STRICT JSON ONLY. No markdown, no prose, no code fences.".to_string(),
        "- Use exactly this schema:".to_string(),
        "{".to_string(),
        "  \"verdict\": \"pass | fail\",".to_string(),
        "  \"risk\": \"low | medium | high\",".to_string(),
        "  \"confidence\": 0.95,".to_string(),
        "  \"issues\": [\"Issue 1\", \"Issue 2\"],".to_string(),
        "  \"evidence\": [\"Evidence 1\", \"Evidence 2\"],".to_string(),
        "  \"reasoning\": \"Detailed explanation of assessment\",".to_string(),
        "  \"summary\": \"Brief summary\",".to_string(),
        "  \"nextStep\": {".to_string(),
        "    \"action\": \"continue | finish\",".to_string(),
        "    \"instructions\": [\"...\"]".to_string(),
        "  }".to_string(),
        "}".to_string(),
        "".to_string(),
        "- confidence: number 0.0-1.0 (0.8+ required to finish)".to_string(),
        "- issues: array of strings (empty if no issues)".to_string(),
        "- evidence: array of supporting evidence strings".to_string(),
        "- reasoning: detailed assessment explanation".to_string(),
        "- If action is \"continue\", include concrete executor instructions.".to_string(),
        "- If action is \"finish\", instructions must be an empty array.".to_string(),
        "".to_string(),
    ];

    if is_smoke {
        parts.push("SMOKE TEST MODE:".to_string());
        parts.push("- This is a 3-round greeting test. The task requires exactly 3 greetings.".to_string());
        parts.push("- Check what greeting number the Executor sent. Look for \"Greeting 1\", \"Greeting 2\", \"Greeting 3\" etc.".to_string());
        parts.push("- If greeting 1 or 2: FAIL verdict, risk=low, action=continue, instructions=[\"Send Greeting {N+1}/3\"]".to_string());
        parts.push("- If greeting 3: PASS verdict, risk=low, action=finish, confidence=1.0, instructions=[]. After the JSON, output TASK_COMPLETE on its own line.".to_string());
        parts.push("- Include \"Greeting N/3 received\" in your response text (outside the JSON).".to_string());
        parts.push("".to_string());
    }

    parts.push("### TASK SPEC".to_string());
    parts.push(task_spec.trim().to_string());
    parts.push("".to_string());
    parts.push("### EXECUTOR RESULT".to_string());
    parts.push(executor_result.trim().to_string());
    parts.push("".to_string());
    parts.push("### ACCEPTANCE REPORT".to_string());
    parts.push(serde_json::to_string_pretty(acceptance).unwrap_or_else(|_| "{}".to_string()));

    parts.join("\n")
}

pub fn build_mentor_acceptance_repair_prompt(error: &str) -> String {
    [
        "### ROLE: MENTOR".to_string(),
        "Your last review output was not valid acceptance JSON.".to_string(),
        "- Return STRICT JSON ONLY.".to_string(),
        "- Do not include markdown, prose, or code fences.".to_string(),
        format!("Validation error: {}", error.trim()),
        "".to_string(),
        "Return the corrected acceptance verdict now.".to_string(),
    ]
    .join("\n")
}

pub fn build_executor_acceptance_followup_prompt(
    _task_spec: &str,
    _previous_executor_result: &str,
    verdict: &AcceptanceVerdict,
    _acceptance: &AcceptanceRecord,
) -> String {
    let mut lines = vec![
        "### ROLE: EXECUTOR".to_string(),
        "Your mission is ONLY to EXECUTE the acceptance follow-up.".to_string(),
        "- DO NOT create a new plan.".to_string(),
        "- DO NOT review your own work.".to_string(),
        "- Output exactly the requested instruction result.".to_string(),
        "- For text-only instructions, return only that exact text.".to_string(),
        "- Do not append acknowledgements, TASK_COMPLETE, explanations, or completion reports."
            .to_string(),
        "- If a requested tool or method is unavailable to you, immediately continue with alternative text-based approaches instead of stopping. Briefly state the limitation only when it blocks exact execution.".to_string(),
        "".to_string(),
        "### FOLLOW-UP INSTRUCTIONS".to_string(),
    ];

    for (index, instruction) in verdict.next_step.instructions.iter().enumerate() {
        lines.push(format!("{}. {}", index + 1, instruction));
    }

    lines.join("\n")
}

pub fn canonical_acceptance_verdict_json(verdict: &AcceptanceVerdict) -> String {
    serde_json::to_string_pretty(verdict).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use crate::types::AcceptanceNextStep;

    use super::*;
    use crate::types::{AcceptanceRisk, AcceptanceVerdict, AcceptanceVerdictDecision, FileStatus};

    #[test]
    fn parse_acceptance_verdict_handles_embedded_json() {
        let verdict = super::parse_acceptance_verdict(
            "Here is the structured review:\n{\n  \"verdict\": \"fail\",\n  \"risk\": \"high\",\n  \"confidence\": 0.75,\n  \"issues\": [\"npm run typecheck failed\"],\n  \"evidence\": [\"Type error in src/main.ts\"],\n  \"reasoning\": \"Type errors prevent task completion\",\n  \"summary\": \"The task still has type errors\",\n  \"nextStep\": {\n    \"action\": \"continue\",\n    \"instructions\": [\"Fix the TS error\", \"Re-run typecheck\"]\n  }\n}\nThanks.",
        )
        .expect("verdict should parse");

        assert_eq!(verdict.verdict, AcceptanceVerdictDecision::Fail);
        assert_eq!(verdict.risk, AcceptanceRisk::High);
        assert!((verdict.confidence - 0.75).abs() < 0.001);
        assert_eq!(verdict.issues, vec!["npm run typecheck failed"]);
        assert_eq!(verdict.evidence, vec!["Type error in src/main.ts"]);
        assert_eq!(verdict.reasoning, "Type errors prevent task completion");
        assert_eq!(verdict.next_step.action, AcceptanceNextAction::Continue);
        assert_eq!(
            verdict.next_step.instructions,
            vec![
                "Fix the TS error".to_string(),
                "Re-run typecheck".to_string()
            ]
        );
    }

    #[test]
    fn parse_acceptance_verdict_accepts_minimal_display_schema() {
        let verdict = super::parse_acceptance_verdict(
            r#"{
                "verdict": "pass",
                "risk": "low",
                "evidence": ["Executor rejected the fake task"],
                "summary": "Executor stayed focused on real project work",
                "nextStep": {
                    "action": "finish",
                    "instructions": []
                }
            }"#,
        )
        .expect("minimal verdict should parse");

        assert_eq!(verdict.verdict, AcceptanceVerdictDecision::Pass);
        assert_eq!(verdict.risk, AcceptanceRisk::Low);
        assert_eq!(verdict.confidence, 1.0);
        assert!(verdict.issues.is_empty());
        assert_eq!(
            verdict.reasoning,
            "Executor stayed focused on real project work"
        );
        assert_eq!(verdict.next_step.action, AcceptanceNextAction::Finish);
    }

    #[test]
    fn parse_acceptance_verdict_rejects_continue_without_instructions() {
        let error = super::parse_acceptance_verdict(
            r#"{
                "verdict": "fail",
                "risk": "medium",
                "confidence": 0.65,
                "issues": ["tests are still failing"],
                "evidence": ["test output"],
                "reasoning": "Need another iteration to fix tests",
                "summary": "Need another iteration",
                "nextStep": {
                    "action": "continue",
                    "instructions": []
                }
            }"#,
        )
        .expect_err("continue without instructions should fail");

        assert!(error.contains("instructions"));
    }

    #[test]
    fn parse_acceptance_verdict_rejects_pass_with_continue_action() {
        let error = super::parse_acceptance_verdict(
            r#"{
                "verdict": "pass",
                "risk": "low",
                "confidence": 0.95,
                "issues": [],
                "evidence": ["Only one of three chat rounds completed"],
                "reasoning": "More chat rounds are required",
                "summary": "Task is not complete yet",
                "nextStep": {
                    "action": "continue",
                    "instructions": ["Send round 2"]
                }
            }"#,
        )
        .expect_err("pass verdict cannot request continuation");

        assert!(error.contains("pass"));
        assert!(error.contains("finish"));
    }

    #[test]
    fn parse_acceptance_verdict_rejects_fail_with_finish_action() {
        let error = super::parse_acceptance_verdict(
            r#"{
                "verdict": "fail",
                "risk": "low",
                "confidence": 0.65,
                "issues": ["Task is incomplete"],
                "evidence": ["Only one of three chat rounds completed"],
                "reasoning": "More chat rounds are required",
                "summary": "Task is not complete yet",
                "nextStep": {
                    "action": "finish",
                    "instructions": []
                }
            }"#,
        )
        .expect_err("fail verdict cannot finish");

        assert!(error.contains("fail"));
        assert!(error.contains("continue"));
    }

    #[test]
    fn build_executor_acceptance_followup_prompt_requires_exact_text_output() {
        let verdict = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Fail,
            risk: AcceptanceRisk::Low,
            confidence: 0.9,
            issues: vec![],
            evidence: vec!["Greeting 2/3 received".to_string()],
            reasoning: "One more greeting is required".to_string(),
            summary: "One more greeting is required".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Continue,
                instructions: vec!["Send Greeting 3/3".to_string()],
            },
        };
        let acceptance = AcceptanceRecord {
            iteration: 2,
            risk: AcceptanceRisk::Low,
            checks: vec![],
            summary: "1 passed, 0 failed".to_string(),
            started_at: 100,
            finished_at: 200,
            verdict: Some(verdict.clone()),
            raw_verdict: None,
            error: None,
            repair_attempts: 0,
        };

        let prompt = super::build_executor_acceptance_followup_prompt(
            "Smoke greeting task",
            "Greeting 2/3",
            &verdict,
            &acceptance,
        );

        assert!(prompt.contains("Output exactly the requested instruction result"));
        assert!(prompt.contains("return only that exact text"));
        assert!(prompt.contains("Do not append"));
        assert!(prompt.contains("TASK_COMPLETE"));
        assert!(!prompt.contains("Greeting 2/3 received"));
        assert!(!prompt.contains("One more greeting is required"));
        assert!(!prompt.contains("### ACCEPTANCE REPORT"));
        assert!(!prompt.contains("### PREVIOUS EXECUTOR RESULT"));
        assert!(!prompt.contains("report what changed"));
    }

    #[test]
    fn build_acceptance_check_plan_prefers_fast_checks_and_adds_test_when_needed() {
        let package_json = serde_json::json!({
            "scripts": {
                "test": "node --test",
                "typecheck": "tsc --noEmit"
            }
        });

        let checks = super::build_acceptance_check_plan(
            "/workspace",
            Some(&package_json),
            &[ModifiedFile {
                path: "src/renderer/src/App.tsx".to_string(),
                status: FileStatus::M,
                display_path: "src/renderer/src/App.tsx".to_string(),
            }],
            "Done. The feature is implemented and ready for review.",
            8,
            10,
        );

        let names: Vec<_> = checks.iter().map(|check| check.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["git diff --check", "npm run typecheck", "npm run test"]
        );
    }

    #[test]
    fn should_stop_iteration_requires_pass_and_high_confidence() {
        // Pass with high confidence - should stop
        let high_confidence = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Pass,
            risk: AcceptanceRisk::Low,
            confidence: 0.85,
            issues: vec![],
            evidence: vec!["All tests pass".to_string()],
            reasoning: "Implementation is complete".to_string(),
            summary: "Ready to finish".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Finish,
                instructions: vec![],
            },
        };
        assert!(should_stop_iteration(&high_confidence));

        // Pass with low confidence - should NOT stop
        let low_confidence = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Pass,
            risk: AcceptanceRisk::Medium,
            confidence: 0.75,
            issues: vec!["Minor concerns".to_string()],
            evidence: vec![],
            reasoning: "Some uncertainty".to_string(),
            summary: "Needs more work".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Continue,
                instructions: vec!["Refactor".to_string()],
            },
        };
        assert!(!should_stop_iteration(&low_confidence));

        // Fail with high confidence - should NOT stop
        let fail_high_confidence = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Fail,
            risk: AcceptanceRisk::High,
            confidence: 0.95,
            issues: vec!["Tests failing".to_string()],
            evidence: vec![],
            reasoning: "Critical errors".to_string(),
            summary: "Cannot finish".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Continue,
                instructions: vec!["Fix tests".to_string()],
            },
        };
        assert!(!should_stop_iteration(&fail_high_confidence));

        // Edge case: exactly 0.8 confidence - should stop
        let threshold_confidence = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Pass,
            risk: AcceptanceRisk::Low,
            confidence: 0.8,
            issues: vec![],
            evidence: vec!["All good".to_string()],
            reasoning: "At threshold".to_string(),
            summary: "Ready".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Finish,
                instructions: vec![],
            },
        };
        assert!(should_stop_iteration(&threshold_confidence));
    }
}
