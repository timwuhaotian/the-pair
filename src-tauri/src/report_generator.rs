use crate::types::{
    AcceptanceRecord, AcceptanceVerdictDecision, IterationMetric, Message, ModifiedFile,
    SessionReport,
};
use crate::util::now_millis;
use std::fs;
use std::path::Path;

const REPORTS_DIR: &str = ".pair/reports";

pub fn ensure_reports_dir(workspace_root: &Path) -> Result<std::path::PathBuf, String> {
    let reports_path = workspace_root.join(REPORTS_DIR);
    fs::create_dir_all(&reports_path)
        .map_err(|e| format!("Failed to create reports directory: {}", e))?;
    Ok(reports_path)
}

pub fn generate_session_report(
    session_id: &str,
    pair_name: &str,
    task_spec: &str,
    started_at: u64,
    acceptance_records: &[AcceptanceRecord],
    git_changes: &[ModifiedFile],
    messages: &[Message],
) -> Result<SessionReport, String> {
    let finished_at = now_millis();

    // Get the final verdict from the last acceptance record
    let final_verdict = acceptance_records
        .last()
        .and_then(|record| record.verdict.clone())
        .ok_or("No final verdict found")?;

    // Calculate token usage from messages
    let mut total_input_tokens: u64 = 0;
    let mut total_output_tokens: u64 = 0;

    for msg in messages {
        if let Some(usage) = &msg.token_usage {
            total_input_tokens += usage.input_tokens.unwrap_or(0);
            total_output_tokens += usage.output_tokens;
        }
    }

    // Build iteration metrics
    let mut iteration_metrics: Vec<IterationMetric> = Vec::new();

    for record in acceptance_records {
        let duration_ms = record.finished_at.saturating_sub(record.started_at);

        // Find messages for this iteration
        let iteration_messages: Vec<&Message> = messages
            .iter()
            .filter(|m| m.iteration == record.iteration)
            .collect();

        let input_tokens: u64 = iteration_messages
            .iter()
            .filter_map(|m| m.token_usage.as_ref().and_then(|u| u.input_tokens))
            .sum();

        let output_tokens: u64 = iteration_messages
            .iter()
            .map(|m| m.token_usage.as_ref().map_or(0, |u| u.output_tokens))
            .sum();

        // Collect error logs from failed checks
        let error_logs: Vec<String> = record
            .checks
            .iter()
            .filter(|check| matches!(check.status, crate::types::AcceptanceCheckStatus::Failed))
            .map(|check| format!("{}: {}", check.name, check.summary))
            .collect();

        iteration_metrics.push(IterationMetric {
            iteration: record.iteration,
            duration_ms,
            input_tokens,
            output_tokens,
            files_changed: git_changes.to_vec(),
            error_logs,
        });
    }

    Ok(SessionReport {
        session_id: session_id.to_string(),
        pair_name: pair_name.to_string(),
        task_spec: task_spec.to_string(),
        started_at,
        finished_at,
        iterations: acceptance_records.len() as u32,
        final_verdict,
        validation_history: acceptance_records.to_vec(),
        git_changes: git_changes.to_vec(),
        messages: messages.to_vec(),
        total_input_tokens,
        total_output_tokens,
        iteration_metrics,
    })
}

pub fn format_report_markdown(report: &SessionReport) -> String {
    let mut md = String::new();

    // Header
    md.push_str(&format!("# Session Report: {}\n\n", report.pair_name));
    md.push_str(&format!("**Session ID:** {}\n\n", report.session_id));
    md.push_str(&format!("**Task:** {}\n\n", report.task_spec));
    md.push_str(&format!(
        "**Status:** {}\n\n",
        if matches!(
            report.final_verdict.verdict,
            AcceptanceVerdictDecision::Pass
        ) {
            "✅ PASSED"
        } else {
            "❌ FAILED"
        }
    ));

    // Summary
    md.push_str("## Summary\n\n");
    md.push_str(&format!("- **Iterations:** {}\n", report.iterations));
    md.push_str(&format!(
        "- **Duration:** {:.1} minutes\n",
        (report.finished_at - report.started_at) as f64 / 1000.0 / 60.0
    ));
    md.push_str(&format!(
        "- **Confidence:** {:.0}%\n",
        report.final_verdict.confidence * 100.0
    ));
    md.push_str(&format!(
        "- **Risk Level:** {:?}\n",
        report.final_verdict.risk
    ));
    md.push_str(&format!(
        "- **Total Tokens:** {} input, {} output\n\n",
        report.total_input_tokens, report.total_output_tokens
    ));

    // Final Verdict
    md.push_str("## Final Verdict\n\n");
    md.push_str(&format!(
        "**Decision:** {:?}\n",
        report.final_verdict.verdict
    ));
    md.push_str(&format!(
        "**Confidence:** {:.0}%\n",
        report.final_verdict.confidence * 100.0
    ));
    md.push_str(&format!("**Risk:** {:?}\n", report.final_verdict.risk));
    md.push_str(&format!(
        "**Summary:** {}\n\n",
        report.final_verdict.summary
    ));
    md.push_str("### Reasoning\n\n");
    md.push_str(&report.final_verdict.reasoning);
    md.push_str("\n\n");

    if !report.final_verdict.issues.is_empty() {
        md.push_str("### Issues\n\n");
        for issue in &report.final_verdict.issues {
            md.push_str(&format!("- {}\n", issue));
        }
        md.push('\n');
    }

    if !report.final_verdict.evidence.is_empty() {
        md.push_str("### Evidence\n\n");
        for evidence in &report.final_verdict.evidence {
            md.push_str(&format!("- {}\n", evidence));
        }
        md.push('\n');
    }

    // Iteration Metrics
    md.push_str("## Iteration Metrics\n\n");
    md.push_str(
        "| Iteration | Duration | Input Tokens | Output Tokens | Files Changed | Errors |\n",
    );
    md.push_str(
        "|-----------|----------|--------------|---------------|---------------|--------|\n",
    );

    for metric in &report.iteration_metrics {
        md.push_str(&format!(
            "| {} | {:.1}m | {} | {} | {} | {} |\n",
            metric.iteration,
            metric.duration_ms as f64 / 1000.0 / 60.0,
            metric.input_tokens,
            metric.output_tokens,
            metric.files_changed.len(),
            if metric.error_logs.is_empty() {
                "-".to_string()
            } else {
                format!("{} errors", metric.error_logs.len())
            }
        ));
    }
    md.push('\n');

    // Git Changes
    if !report.git_changes.is_empty() {
        md.push_str("## Git Changes\n\n");
        md.push_str("| Status | Path |\n");
        md.push_str("|--------|------|\n");
        for file in &report.git_changes {
            md.push_str(&format!("| {:?} | {} |\n", file.status, file.path));
        }
        md.push('\n');
    }

    // Validation History
    md.push_str("## Validation History\n\n");
    for record in &report.validation_history {
        md.push_str(&format!("### Iteration {}\n\n", record.iteration));
        md.push_str(&format!("- **Risk:** {:?}\n", record.risk));
        md.push_str(&format!("- **Summary:** {}\n", record.summary));
        md.push_str(&format!(
            "- **Duration:** {:.1}s\n",
            (record.finished_at - record.started_at) as f64 / 1000.0
        ));

        if !record.checks.is_empty() {
            md.push_str("\n**Checks:**\n\n");
            for check in &record.checks {
                let status_emoji = match check.status {
                    crate::types::AcceptanceCheckStatus::Passed => "✅",
                    crate::types::AcceptanceCheckStatus::Failed => "❌",
                    crate::types::AcceptanceCheckStatus::Skipped => "⏭️",
                };
                md.push_str(&format!("- {} {}\n", status_emoji, check.name));
            }
        }
        md.push('\n');
    }

    md
}

pub fn save_report_to_file(
    report: &SessionReport,
    workspace_root: &Path,
) -> Result<std::path::PathBuf, String> {
    let reports_dir = ensure_reports_dir(workspace_root)?;
    let safe_id: String = report
        .session_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    let filename = format!("{}.md", safe_id);
    let filepath = reports_dir.join(&filename);

    let markdown = format_report_markdown(report);

    // Atomic write: write to a temp sibling then rename so a crash mid-write
    // never leaves a partial report file behind.
    let tmp_path = filepath.with_extension("md.tmp");
    fs::write(&tmp_path, markdown)
        .map_err(|e| format!("Failed to write report file: {}", e))?;
    fs::rename(&tmp_path, &filepath)
        .map_err(|e| format!("Failed to move report file into place: {}", e))?;

    Ok(filepath)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn create_test_verdict() -> AcceptanceVerdict {
        AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Pass,
            risk: AcceptanceRisk::Low,
            confidence: 0.95,
            issues: vec![],
            evidence: vec!["All tests pass".to_string()],
            reasoning: "Implementation is complete and correct".to_string(),
            summary: "Task completed successfully".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Finish,
                instructions: vec![],
            },
        }
    }

    fn create_test_acceptance_record(iteration: u32) -> AcceptanceRecord {
        AcceptanceRecord {
            iteration,
            risk: AcceptanceRisk::Low,
            checks: vec![AcceptanceCheckRun {
                name: "git diff --check".to_string(),
                command: "git diff --check".to_string(),
                status: AcceptanceCheckStatus::Passed,
                exit_code: Some(0),
                duration_ms: 100,
                summary: "No whitespace errors".to_string(),
                stdout: String::new(),
                stderr: String::new(),
            }],
            summary: "All checks passed".to_string(),
            started_at: 1000,
            finished_at: 2000,
            verdict: Some(create_test_verdict()),
            raw_verdict: None,
            error: None,
            repair_attempts: 0,
        }
    }

    #[test]
    fn generate_session_report_creates_complete_report() {
        let acceptance_records = vec![
            create_test_acceptance_record(1),
            create_test_acceptance_record(2),
        ];

        let git_changes = vec![ModifiedFile {
            path: "src/main.rs".to_string(),
            status: FileStatus::M,
            display_path: "src/main.rs".to_string(),
        }];

        let messages: Vec<Message> = vec![];

        let report = generate_session_report(
            "test-session-1",
            "Test Pair",
            "Implement feature X",
            1000,
            &acceptance_records,
            &git_changes,
            &messages,
        )
        .expect("should generate report");

        assert_eq!(report.session_id, "test-session-1");
        assert_eq!(report.pair_name, "Test Pair");
        assert_eq!(report.task_spec, "Implement feature X");
        assert_eq!(report.iterations, 2);
        assert_eq!(report.git_changes.len(), 1);
        assert_eq!(report.validation_history.len(), 2);
        assert!(matches!(
            report.final_verdict.verdict,
            AcceptanceVerdictDecision::Pass
        ));
        assert!((report.final_verdict.confidence - 0.95).abs() < 0.001);
    }

    #[test]
    fn format_report_markdown_includes_all_sections() {
        let acceptance_records = vec![create_test_acceptance_record(1)];
        let git_changes = vec![];
        let messages = vec![];

        let report = generate_session_report(
            "test-session-2",
            "Test Pair",
            "Test task",
            1000,
            &acceptance_records,
            &git_changes,
            &messages,
        )
        .expect("should generate report");

        let markdown = format_report_markdown(&report);

        assert!(markdown.contains("# Session Report: Test Pair"));
        assert!(markdown.contains("Session ID:** test-session-2"));
        assert!(markdown.contains("Task:** Test task"));
        assert!(markdown.contains("✅ PASSED"));
        assert!(markdown.contains("95%"));
        assert!(markdown.contains("## Summary"));
        assert!(markdown.contains("## Final Verdict"));
        assert!(markdown.contains("## Iteration Metrics"));
        assert!(markdown.contains("## Validation History"));
    }
}
