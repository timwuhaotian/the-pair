use serde::Serialize;

/// Reasons why a pair run pauses for human intervention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PauseReason {
    /// Iteration budget exhausted without completion.
    BudgetExhausted { iterations: u32, budget: u32 },
}

/// Generates a human-readable pause message.
pub fn format_pause_message(reason: &PauseReason) -> String {
    match reason {
        PauseReason::BudgetExhausted { iterations, budget } => {
            format!(
                "Reached iteration limit ({iterations}/{budget}). Review the progress and decide whether to continue, assign a new task, or finish."
            )
        }
    }
}

/// Decides whether the mentor's freshly produced plan should pause for human
/// approval before the executor starts. Gates the mentor's planning turns (the
/// initial plan and any re-plan after a rejection); review turns and the
/// executor are never gated here.
pub fn should_gate_plan(is_review_turn: bool, plan_gate_enabled: bool) -> bool {
    plan_gate_enabled && !is_review_turn
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn gates_mentor_planning_turns_when_enabled() {
        assert!(should_gate_plan(false, true));
        assert!(!should_gate_plan(false, false));
        assert!(!should_gate_plan(true, true));
    }
}
