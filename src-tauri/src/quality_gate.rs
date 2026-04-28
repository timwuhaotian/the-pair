use serde::{Deserialize, Serialize};

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
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let checks_performed = checks_line["CHECKS:".len()..]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let code_reference = code_line["CODE:".len()..].trim().to_string();

    Some(ReviewEvidence {
        files_reviewed,
        checks_performed,
        code_reference,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(validate_review(&evidence), QualityGateResult::Fail { .. }));
    }

    #[test]
    fn test_validate_fail_no_checks() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["src/main.rs".into()],
            checks_performed: vec![],
            code_reference: "some code".into(),
        };
        assert!(matches!(validate_review(&evidence), QualityGateResult::Fail { .. }));
    }

    #[test]
    fn test_validate_fail_no_code_ref() {
        let evidence = ReviewEvidence {
            files_reviewed: vec!["src/main.rs".into()],
            checks_performed: vec!["error handling".into()],
            code_reference: "".into(),
        };
        assert!(matches!(validate_review(&evidence), QualityGateResult::Fail { .. }));
    }

    #[test]
    fn test_extract_evidence_success() {
        let text = "I approve.\nFILES_REVIEWED: src/main.rs, src/utils.rs\nCHECKS: error handling, edge cases\nCODE: handle_login returns Result";
        let evidence = extract_evidence(text).expect("should extract");
        assert_eq!(evidence.files_reviewed, vec!["src/main.rs", "src/utils.rs"]);
        assert_eq!(evidence.checks_performed, vec!["error handling", "edge cases"]);
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
}
