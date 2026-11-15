use crate::types::{AcceptanceRecord, ModifiedFile};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Sentinel check name emitted when a review runs with no modified files.
/// `acceptance::run_acceptance_checks` injects one explicit record with this
/// name so an empty diff yields one visible check instead of zero checks.
pub const NO_CHANGES_CHECK_NAME: &str = "no-changes";

/// Structured evidence extracted from a mentor review verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewEvidence {
    /// Files that were reviewed.
    pub files_reviewed: Vec<String>,
    /// Specific checks performed.
    pub checks_performed: Vec<String>,
    /// Quote or reference of code being validated.
    pub code_reference: String,
}

/// Result of quality gate validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualityGateResult {
    /// Evidence is sufficient — proceed with verdict.
    Pass,
    /// Evidence is insufficient — reject with feedback.
    Fail { reason: String },
}

/// Validates that review evidence meets the quality threshold.
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
            reason:
                "No code reference provided. Quote or reference the changed code you validated."
                    .into(),
        };
    }
    QualityGateResult::Pass
}

/// Evidence gate plus file consistency: `FILES_REVIEWED` must cover every
/// file in `modified_files`, and an empty diff must carry the explicit
/// `no-changes` record in `record.checks` (see [`NO_CHANGES_CHECK_NAME`]).
/// This is the check the driver wires in at integration time; the evidence
/// format gate itself stays in [`validate_review`].
fn normalize_review_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_lowercase()
}

pub fn validate_review_with_files(
    evidence: &ReviewEvidence,
    modified_files: &[ModifiedFile],
    record: &AcceptanceRecord,
) -> QualityGateResult {
    if let fail @ QualityGateResult::Fail { .. } = validate_review(evidence) {
        return fail;
    }
    if modified_files.is_empty() {
        let has_marker = record
            .checks
            .iter()
            .any(|run| run.name == NO_CHANGES_CHECK_NAME || run.command == NO_CHANGES_CHECK_NAME);
        if !has_marker {
            return QualityGateResult::Fail {
                reason: "No files changed but the check record has no explicit `no-changes` entry. Re-run the checks so the empty diff is recorded.".into(),
            };
        }
        return QualityGateResult::Pass;
    }
    let reviewed: HashSet<String> = evidence
        .files_reviewed
        .iter()
        .map(|entry| normalize_review_path(entry))
        .collect();
    let missing: Vec<String> = modified_files
        .iter()
        .filter(|file| {
            !reviewed.contains(&normalize_review_path(&file.path))
                && !reviewed.contains(&normalize_review_path(&file.display_path))
        })
        .map(|file| file.path.clone())
        .collect();
    if !missing.is_empty() {
        return QualityGateResult::Fail {
            reason: format!(
                "FILES_REVIEWED misses {} changed file(s): {}. List every file you reviewed.",
                missing.len(),
                missing.join(", ")
            ),
        };
    }
    QualityGateResult::Pass
}

/// Returns the text after `marker` on the first line that starts with it
/// (leading indentation allowed). Markers in the middle of a line — e.g.
/// "EXIT CODE: 1" or "AUTOMATED CHECKS: 3 passed" — don't count.
fn marker_value<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix(marker))
}

/// Extracts evidence from a mentor verdict message.
/// Expects structured sections, each at the start of a line:
/// `FILES_REVIEWED:`, `CHECKS:`, `CODE:`. Returns `None` unless all three are
/// present, i.e. unless the mentor deliberately used the format.
pub fn extract_evidence(verdict_text: &str) -> Option<ReviewEvidence> {
    let files_value = marker_value(verdict_text, "FILES_REVIEWED:")?;
    let checks_value = marker_value(verdict_text, "CHECKS:")?;
    let code_value = marker_value(verdict_text, "CODE:")?;

    let files_reviewed = files_value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let checks_performed = checks_value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let code_reference = code_value.trim().to_string();

    Some(ReviewEvidence {
        files_reviewed,
        checks_performed,
        code_reference,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AcceptanceCheckRun, AcceptanceCheckStatus, AcceptanceRisk};

    fn record_with_checks(names: &[&str]) -> AcceptanceRecord {
        AcceptanceRecord {
            iteration: 1,
            risk: AcceptanceRisk::Low,
            checks: names
                .iter()
                .map(|name| AcceptanceCheckRun {
                    name: name.to_string(),
                    command: name.to_string(),
                    status: AcceptanceCheckStatus::Passed,
                    exit_code: Some(0),
                    duration_ms: 1,
                    summary: format!("{} passed", name),
                    stdout: String::new(),
                    stderr: String::new(),
                })
                .collect(),
            summary: "1 passed".to_string(),
            started_at: 0,
            finished_at: 1,
            verdict: None,
            raw_verdict: None,
            error: None,
            repair_attempts: 0,
        }
    }

    fn modified(path: &str) -> ModifiedFile {
        ModifiedFile {
            path: path.to_string(),
            status: crate::types::FileStatus::M,
            display_path: path.to_string(),
        }
    }

    #[test]
    fn test_validate_pass() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["src/main.rs".into()],
            checks_performed: vec!["error handling".into()],
            code_reference: "Result<T, E> in handle_login()".into(),
        };
        assert_eq!(validate_review(&evidence), QualityGateResult::Pass);
    }

    #[test]
    fn test_validate_fail_no_files() {
        let evidence = ReviewEvidence {
            files_reviewed: vec![],
            checks_performed: vec!["error handling".into()],
            code_reference: "some code".into(),
        };
        assert!(matches!(
            validate_review(&evidence),
            QualityGateResult::Fail { .. }
        ));
    }

    #[test]
    fn test_validate_fail_no_checks() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["src/main.rs".into()],
            checks_performed: vec![],
            code_reference: "some code".into(),
        };
        assert!(matches!(
            validate_review(&evidence),
            QualityGateResult::Fail { .. }
        ));
    }

    #[test]
    fn test_validate_fail_no_code_ref() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["src/main.rs".into()],
            checks_performed: vec!["error handling".into()],
            code_reference: "".into(),
        };
        assert!(matches!(
            validate_review(&evidence),
            QualityGateResult::Fail { .. }
        ));
    }

    #[test]
    fn test_extract_evidence_success() {
        let text = "I approve.\nFILES_REVIEWED: src/main.rs, src/utils.rs\nCHECKS: error handling, edge cases\nCODE: handle_login returns Result";
        let evidence = extract_evidence(text).expect("should extract");
        assert_eq!(evidence.files_reviewed, vec!["src/main.rs", "src/utils.rs"]);
        assert_eq!(
            evidence.checks_performed,
            vec!["error handling", "edge cases"]
        );
        assert_eq!(evidence.code_reference, "handle_login returns Result");
    }

    #[test]
    fn test_extract_evidence_missing_files() {
        let text = "I approve.\nCHECKS: error handling\nCODE: some code";
        assert!(extract_evidence(text).is_none());
    }

    #[test]
    fn test_extract_evidence_missing_checks() {
        let text = "I approve.\nFILES_REVIEWED: src/main.rs\nCODE: some code";
        assert!(extract_evidence(text).is_none());
    }

    #[test]
    fn test_extract_evidence_missing_code() {
        let text = "I approve.\nFILES_REVIEWED: src/main.rs\nCHECKS: error handling";
        assert!(extract_evidence(text).is_none());
    }

    #[test]
    fn test_extract_evidence_ignores_markers_inside_lines() {
        let text = "AUTOMATED CHECKS: 3 passed\nEXIT CODE: 1\nsee FILES_REVIEWED: none";
        assert!(extract_evidence(text).is_none());
    }

    #[test]
    fn test_extract_evidence_allows_indented_markers() {
        let text = "  FILES_REVIEWED: a.rs\n  CHECKS: types\n  CODE: fn a()";
        let evidence = extract_evidence(text).expect("indented markers still count");
        assert_eq!(evidence.files_reviewed, vec!["a.rs"]);
    }

    #[test]
    fn with_files_passes_when_coverage_is_exact() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["src/main.rs".into()],
            checks_performed: vec!["types".into()],
            code_reference: "fn main()".into(),
        };
        let result = validate_review_with_files(
            &evidence,
            &[modified("src/main.rs")],
            &record_with_checks(&["git diff HEAD --check --"]),
        );
        assert_eq!(result, QualityGateResult::Pass);
    }

    #[test]
    fn with_files_matches_display_path_and_ignores_case() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["./SRC\\MAIN.RS".into()],
            checks_performed: vec!["types".into()],
            code_reference: "fn main()".into(),
        };
        let mut file = modified("src/main.rs");
        file.display_path = "src/main.rs".to_string();
        let result = validate_review_with_files(
            &evidence,
            &[file],
            &record_with_checks(&["git diff HEAD --check --"]),
        );
        assert_eq!(result, QualityGateResult::Pass);
    }

    #[test]
    fn with_files_fails_when_a_changed_file_is_missing() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["src/main.rs".into()],
            checks_performed: vec!["types".into()],
            code_reference: "fn main()".into(),
        };
        let result = validate_review_with_files(
            &evidence,
            &[modified("src/main.rs"), modified("src/other.rs")],
            &record_with_checks(&["git diff HEAD --check --"]),
        );
        match result {
            QualityGateResult::Fail { reason } => assert!(
                reason.contains("src/other.rs"),
                "reason must name the gap: {}",
                reason
            ),
            QualityGateResult::Pass => panic!("missing coverage must fail"),
        }
    }

    #[test]
    fn with_files_still_rejects_empty_evidence_first() {
        let evidence = ReviewEvidence {
            files_reviewed: vec![],
            checks_performed: vec!["types".into()],
            code_reference: "fn main()".into(),
        };
        let result = validate_review_with_files(
            &evidence,
            &[modified("src/main.rs")],
            &record_with_checks(&["git diff HEAD --check --"]),
        );
        assert!(matches!(result, QualityGateResult::Fail { .. }));
    }

    #[test]
    fn with_files_empty_diff_needs_the_no_changes_record() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["nothing to review".into()],
            checks_performed: vec!["confirmed empty diff".into()],
            code_reference: "no code changed".into(),
        };
        let missing = validate_review_with_files(&evidence, &[], &record_with_checks(&[]));
        assert!(matches!(missing, QualityGateResult::Fail { .. }));

        let present = validate_review_with_files(
            &evidence,
            &[],
            &record_with_checks(&[NO_CHANGES_CHECK_NAME]),
        );
        assert_eq!(present, QualityGateResult::Pass);
    }
}
