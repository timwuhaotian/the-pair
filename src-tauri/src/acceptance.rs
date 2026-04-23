use crate::types::{
    AcceptanceCheckRun, AcceptanceCheckStatus, AcceptanceNextAction, AcceptanceNextStep,
    AcceptanceRecord, AcceptanceRisk, AcceptanceVerdict, AcceptanceVerdictDecision, ModifiedFile,
};
use crate::util::now_millis;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::time::Instant;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AcceptanceCheckPlan {
    name: String,
    command: String,
    program: String,
    args: Vec<String>,
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
        match serde_json::from_str::<AcceptanceVerdict>(&candidate) {
            Ok(verdict) => {
                // Validate confidence range
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
                    return Err(
                        "Acceptance verdict cannot include instructions when finishing".to_string(),
                    );
                }
                return Ok(verdict);
            }
            Err(error) => last_error = error.to_string(),
        }
    }

    Err(last_error)
}

pub const CONFIDENCE_THRESHOLD: f64 = 0.8;

pub fn should_stop_iteration(verdict: &AcceptanceVerdict) -> bool {
    matches!(verdict.verdict, AcceptanceVerdictDecision::Pass)
        && verdict.confidence >= CONFIDENCE_THRESHOLD
}

pub fn format_verdict_summary(verdict: &AcceptanceVerdict) -> String {
    format!(
        "{} (confidence: {:.0}%, risk: {:?})",
        if matches!(verdict.verdict, AcceptanceVerdictDecision::Pass) {
            "PASSED"
        } else {
            "FAILED"
        },
        verdict.confidence * 100.0,
        verdict.risk
    )
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
    [
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
        "### TASK SPEC".to_string(),
        task_spec.trim().to_string(),
        "".to_string(),
        "### EXECUTOR RESULT".to_string(),
        executor_result.trim().to_string(),
        "".to_string(),
        "### ACCEPTANCE REPORT".to_string(),
        serde_json::to_string_pretty(acceptance).unwrap_or_else(|_| "{}".to_string()),
    ]
    .join("\n")
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
    task_spec: &str,
    previous_executor_result: &str,
    verdict: &AcceptanceVerdict,
    acceptance: &AcceptanceRecord,
) -> String {
    let mut lines = vec![
        "### ROLE: EXECUTOR".to_string(),
        "Your mission is ONLY to EXECUTE the acceptance follow-up.".to_string(),
        "- DO NOT create a new plan.".to_string(),
        "- DO NOT review your own work.".to_string(),
        "- Apply the mentor follow-up instructions exactly, then report what changed."
            .to_string(),
        "".to_string(),
        "### TASK SPEC".to_string(),
        task_spec.trim().to_string(),
        "".to_string(),
        "### PREVIOUS EXECUTOR RESULT".to_string(),
        previous_executor_result.trim().to_string(),
        "".to_string(),
        "### MENTOR ACCEPTANCE VERDICT".to_string(),
        serde_json::to_string_pretty(verdict).unwrap_or_else(|_| "{}".to_string()),
        "".to_string(),
        "### ACCEPTANCE REPORT".to_string(),
        serde_json::to_string_pretty(acceptance).unwrap_or_else(|_| "{}".to_string()),
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
            vec!["Fix the TS error".to_string(), "Re-run typecheck".to_string()]
        );
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
        assert_eq!(names, vec!["git diff --check", "npm run typecheck", "npm run test"]);
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
