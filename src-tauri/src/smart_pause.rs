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
}
