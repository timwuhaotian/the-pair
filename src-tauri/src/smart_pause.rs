use serde::Serialize;

/// Reasons why a pair run pauses for human intervention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PauseReason {
    /// Iteration budget exhausted without completion.
    BudgetExhausted { iterations: u32, budget: u32 },
    /// Executor modified files not in the Mentor's plan.
    ScopeDrift {
        planned_files: Vec<String>,
        actual_files: Vec<String>,
    },
}

/// Determines whether a pair run should pause based on current state.
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
        let unplanned: Vec<_> = actual_changed_files
            .iter()
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
            format!(
                "Agent reached iteration budget ({iterations}/{budget}). The task may need refinement or the budget should be increased."
            )
        }
        PauseReason::ScopeDrift {
            planned_files,
            actual_files,
        } => {
            let unplanned: Vec<String> = actual_files
                .iter()
                .filter(|f| !planned_files.contains(f))
                .cloned()
                .collect();
            format!(
                "Scope drift detected. Executor modified files outside the plan: {}. Please review and decide whether to allow these changes.",
                unplanned.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_pause_when_complete() {
        assert!(should_pause(5, 3, 10, &[], &[], true).is_none());
    }

    #[test]
    fn test_pause_budget_exhausted() {
        let result = should_pause(4, 3, 10, &[], &[], false);
        assert!(matches!(result, Some(PauseReason::BudgetExhausted { .. })));
    }

    #[test]
    fn test_pause_budget_uses_effective_min() {
        // adaptive_budget=3, configured_max=2 → effective=2
        let result = should_pause(2, 3, 2, &[], &[], false);
        assert!(matches!(result, Some(PauseReason::BudgetExhausted { iterations: 2, budget: 2 })));
    }

    #[test]
    fn test_pause_scope_drift() {
        let planned = vec!["src/main.rs".to_string()];
        let actual = vec!["src/main.rs".to_string(), "src/new_file.rs".to_string()];
        let result = should_pause(1, 3, 10, &planned, &actual, false);
        assert!(matches!(result, Some(PauseReason::ScopeDrift { .. })));
    }

    #[test]
    fn test_no_scope_drift_when_subset() {
        let planned = vec!["src/main.rs".to_string(), "src/utils.rs".to_string()];
        let actual = vec!["src/main.rs".to_string()];
        let result = should_pause(1, 3, 10, &planned, &actual, false);
        assert!(result.is_none());
    }

    #[test]
    fn test_no_drift_when_no_planned() {
        let actual = vec!["src/main.rs".to_string()];
        let result = should_pause(1, 3, 10, &[], &actual, false);
        assert!(result.is_none());
    }

    #[test]
    fn test_pause_message_budget() {
        let reason = PauseReason::BudgetExhausted {
            iterations: 3,
            budget: 3,
        };
        let msg = format_pause_message(&reason);
        assert!(msg.contains("3/3"));
    }

    #[test]
    fn test_pause_message_scope_drift() {
        let reason = PauseReason::ScopeDrift {
            planned_files: vec!["a.rs".to_string()],
            actual_files: vec!["a.rs".to_string(), "b.rs".to_string()],
        };
        let msg = format_pause_message(&reason);
        assert!(msg.contains("Scope drift"));
        assert!(msg.contains("b.rs"));
    }
}
