/// Complexity tier for adaptive iteration budgeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplexityTier {
    /// No changes detected — verify and finish.
    None,
    /// ≤5 files changed, simple modifications.
    Simple,
    /// ≤15 files changed, moderate modifications.
    Medium,
    /// >15 files changed or high-complexity file types.
    Complex,
}

impl ComplexityTier {
    /// Returns the recommended iteration budget for this tier, capped by configured max.
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

/// Weight factor for file types — higher means more complex.
fn file_type_weight(path: &str) -> f64 {
    let lower = path.to_lowercase();
    if lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with("_test.rs")
        || lower.ends_with(".test.js")
    {
        0.7 // Test files are simpler
    } else if lower.ends_with(".json")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".toml")
        || lower.ends_with(".md")
    {
        0.8 // Config/docs are simpler
    } else if lower.contains("/src/")
        || lower.contains("/lib/")
        || lower.contains("/app/")
        || lower.starts_with("src/")
        || lower.starts_with("lib/")
        || lower.starts_with("app/")
    {
        1.2 // Business logic is more complex
    } else {
        1.0
    }
}

/// Computes the complexity tier from changed file metrics.
/// Uses weighted scoring based on file types.
pub fn compute_tier(files_changed: usize, changed_files: &[String]) -> ComplexityTier {
    if files_changed == 0 {
        return ComplexityTier::None;
    }

    // Weighted score
    let weighted_score: f64 = changed_files.iter().map(|f| file_type_weight(f)).sum();

    // Thresholds for weighted score
    if files_changed <= 5 && weighted_score < 5.0 {
        return ComplexityTier::Simple;
    }
    if files_changed <= 15 && weighted_score < 15.0 {
        return ComplexityTier::Medium;
    }
    ComplexityTier::Complex
}

/// Computes the adaptive iteration budget.
pub fn compute_adaptive_budget(
    files_changed: usize,
    changed_files: &[String],
    configured_max: u32,
) -> u32 {
    compute_tier(files_changed, changed_files).max_iterations(configured_max)
}

/// Extracts changed file paths from the modified_files list.
pub fn extract_changed_paths(modified_files: &[crate::types::ModifiedFile]) -> Vec<String> {
    modified_files.iter().map(|f| f.path.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileStatus, ModifiedFile};

    #[test]
    fn test_none_tier() {
        assert_eq!(compute_tier(0, &[]), ComplexityTier::None);
    }

    #[test]
    fn test_simple_tier() {
        let files = vec!["src/main.rs".into(), "src/utils.rs".into()];
        assert_eq!(compute_tier(2, &files), ComplexityTier::Simple);
    }

    #[test]
    fn test_medium_tier() {
        let files = (0..8).map(|i| format!("src/module_{}.rs", i)).collect::<Vec<_>>();
        assert_eq!(compute_tier(8, &files), ComplexityTier::Medium);
    }

    #[test]
    fn test_complex_tier_by_count() {
        let files = (0..20).map(|i| format!("src/module_{}.rs", i)).collect::<Vec<_>>();
        assert_eq!(compute_tier(20, &files), ComplexityTier::Complex);
    }

    #[test]
    fn test_complex_tier_by_weight() {
        // 13 business logic files at 1.2 weight = 15.6 → Complex
        let files = (0..13).map(|i| format!("src/lib/service_{}.rs", i)).collect::<Vec<_>>();
        assert_eq!(compute_tier(13, &files), ComplexityTier::Complex);
    }

    #[test]
    fn test_budget_respects_configured_max() {
        let files = (0..20).map(|i| format!("src/f{}.rs", i)).collect::<Vec<_>>();
        assert_eq!(compute_adaptive_budget(20, &files, 5), 5);
    }

    #[test]
    fn test_budget_none_tier() {
        assert_eq!(compute_adaptive_budget(0, &[], 10), 1);
    }

    #[test]
    fn test_file_type_weight_test_files() {
        assert!((file_type_weight("src/foo.test.ts") - 0.7).abs() < f64::EPSILON);
        assert!((file_type_weight("src/bar.spec.ts") - 0.7).abs() < f64::EPSILON);
        assert!((file_type_weight("src/baz_test.rs") - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_file_type_weight_config_files() {
        assert!((file_type_weight("package.json") - 0.8).abs() < f64::EPSILON);
        assert!((file_type_weight("config.yaml") - 0.8).abs() < f64::EPSILON);
        assert!((file_type_weight("Cargo.toml") - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_file_type_weight_business_logic() {
        assert!((file_type_weight("src/lib/service.rs") - 1.2).abs() < f64::EPSILON);
        assert!((file_type_weight("app/controllers/home.ts") - 1.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_file_type_weight_default() {
        assert!((file_type_weight("Makefile") - 1.0).abs() < f64::EPSILON);
        assert!((file_type_weight("README") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_extract_changed_paths() {
        let files = vec![
            ModifiedFile {
                path: "src/main.rs".into(),
                status: FileStatus::M,
                display_path: "src/main.rs".into(),
            },
            ModifiedFile {
                path: "tests/test.rs".into(),
                status: FileStatus::A,
                display_path: "tests/test.rs".into(),
            },
        ];
        let paths = extract_changed_paths(&files);
        assert_eq!(paths, vec!["src/main.rs", "tests/test.rs"]);
    }
}
