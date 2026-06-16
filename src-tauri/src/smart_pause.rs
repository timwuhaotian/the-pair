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
pub fn should_gate_plan(
    role: &str,
    is_review_turn: bool,
    plan_gate_enabled: bool,
    should_handoff: bool,
) -> bool {
    should_handoff && plan_gate_enabled && role == "mentor" && !is_review_turn
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
        // Mentor planning turn (initial plan or re-plan), gate on, handoff pending → gated.
        assert!(should_gate_plan("mentor", false, true, true));
        // Gate disabled → never gated.
        assert!(!should_gate_plan("mentor", false, false, true));
        // Review turn → not gated (executor already ran once).
        assert!(!should_gate_plan("mentor", true, true, true));
        // Executor turn → not gated.
        assert!(!should_gate_plan("executor", false, true, true));
        // No handoff pending (finished/paused/error) → not gated.
        assert!(!should_gate_plan("mentor", false, true, false));
    }
}
