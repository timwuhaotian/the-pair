use serde::{Deserialize, Serialize};

/// Structured context passed between agent turns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffContext {
    /// The original task specification.
    pub task_spec: String,
    /// What this specific turn should accomplish.
    pub current_sub_goal: String,
    /// Summary of what has been done so far.
    pub progress_summary: String,
    /// Key decisions made during the session.
    pub key_decisions: Vec<String>,
    /// Mentor's plan extracted as a checklist.
    pub plan_checklist: Vec<PlanItem>,
}

/// A single item in the mentor's plan checklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub description: String,
    pub completed: bool,
}

/// Builds a handoff context from session state.
pub fn build_handoff_context(
    task_spec: &str,
    current_sub_goal: &str,
    progress_summary: &str,
    key_decisions: &[String],
    plan_checklist: &[PlanItem],
) -> HandoffContext {
    HandoffContext {
        task_spec: task_spec.to_string(),
        current_sub_goal: current_sub_goal.to_string(),
        progress_summary: progress_summary.to_string(),
        key_decisions: key_decisions.to_vec(),
        plan_checklist: plan_checklist.to_vec(),
    }
}

/// Formats the handoff context into a prompt section for the Mentor.
pub fn format_mentor_prompt(context: &HandoffContext) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("## Original Task\n{}\n\n", context.task_spec));
    if !context.progress_summary.is_empty() {
        prompt.push_str(&format!("## Progress So Far\n{}\n\n", context.progress_summary));
    }
    if !context.key_decisions.is_empty() {
        prompt.push_str("## Key Decisions Made\n");
        for d in &context.key_decisions {
            prompt.push_str(&format!("- {}\n", d));
        }
        prompt.push('\n');
    }
    prompt.push_str("## Your Task\nReview the current state and provide a structured plan as a checklist.\n");
    prompt
}

/// Formats the handoff context into a prompt section for the Executor.
pub fn format_executor_prompt(context: &HandoffContext) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("## Original Task\n{}\n\n", context.task_spec));
    prompt.push_str(&format!("## Your Goal\n{}\n\n", context.current_sub_goal));
    if !context.progress_summary.is_empty() {
        prompt.push_str(&format!("## Progress So Far\n{}\n\n", context.progress_summary));
    }
    if !context.plan_checklist.is_empty() {
        prompt.push_str("## Plan Checklist\nComplete these items:\n");
        for item in &context.plan_checklist {
            let mark = if item.completed { "x" } else { " " };
            prompt.push_str(&format!("- [{}] {}\n", mark, item.description));
        }
        prompt.push('\n');
    }
    prompt
}

/// Formats the handoff context into a prompt section for the Review turn.
pub fn format_review_prompt(context: &HandoffContext, diff_summary: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("## Original Task\n{}\n\n", context.task_spec));
    if !context.plan_checklist.is_empty() {
        prompt.push_str("## Expected Plan\n");
        for item in &context.plan_checklist {
            let mark = if item.completed { "x" } else { " " };
            prompt.push_str(&format!("- [{}] {}\n", mark, item.description));
        }
        prompt.push('\n');
    }
    prompt.push_str(&format!("## Changes Made\n{}\n\n", diff_summary));
    prompt.push_str(
        "## Your Task\nReview the changes against the plan and task spec. Provide structured evidence for your verdict.\n",
    );
    prompt
}

/// Parses a checklist from mentor output text.
/// Looks for lines matching "- [ ] item" or "- [x] item".
pub fn parse_checklist(text: &str) -> Vec<PlanItem> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
                let desc = trimmed[5..].trim();
                if !desc.is_empty() {
                    return Some(PlanItem {
                        description: desc.to_string(),
                        completed: true,
                    });
                }
            } else if trimmed.starts_with("- [ ]") {
                let desc = trimmed[5..].trim();
                if !desc.is_empty() {
                    return Some(PlanItem {
                        description: desc.to_string(),
                        completed: false,
                    });
                }
            }
            None
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_context() {
        let ctx = build_handoff_context(
            "Fix login bug",
            "Implement error handling",
            "Analyzed the issue",
            &["Use Result type".to_string()],
            &[PlanItem {
                description: "Add error handling".to_string(),
                completed: false,
            }],
        );
        assert_eq!(ctx.task_spec, "Fix login bug");
        assert_eq!(ctx.current_sub_goal, "Implement error handling");
        assert_eq!(ctx.plan_checklist.len(), 1);
        assert_eq!(ctx.key_decisions.len(), 1);
    }

    #[test]
    fn test_mentor_prompt_contains_sections() {
        let ctx = build_handoff_context("Task", "", "", &[], &[]);
        let prompt = format_mentor_prompt(&ctx);
        assert!(prompt.contains("## Original Task"));
        assert!(prompt.contains("## Your Task"));
    }

    #[test]
    fn test_mentor_prompt_with_decisions() {
        let ctx = build_handoff_context(
            "Task",
            "",
            "",
            &["Decision A".to_string(), "Decision B".to_string()],
            &[],
        );
        let prompt = format_mentor_prompt(&ctx);
        assert!(prompt.contains("## Key Decisions Made"));
        assert!(prompt.contains("- Decision A"));
        assert!(prompt.contains("- Decision B"));
    }

    #[test]
    fn test_executor_prompt_contains_checklist() {
        let ctx = build_handoff_context(
            "Task",
            "Goal",
            "",
            &[],
            &[PlanItem {
                description: "Step 1".to_string(),
                completed: false,
            }],
        );
        let prompt = format_executor_prompt(&ctx);
        assert!(prompt.contains("[ ] Step 1"));
        assert!(prompt.contains("## Your Goal"));
    }

    #[test]
    fn test_completed_checklist_shows_x() {
        let ctx = build_handoff_context(
            "Task",
            "Goal",
            "",
            &[],
            &[PlanItem {
                description: "Done item".to_string(),
                completed: true,
            }],
        );
        let prompt = format_executor_prompt(&ctx);
        assert!(prompt.contains("[x] Done item"));
    }

    #[test]
    fn test_review_prompt_shows_plan_and_diff() {
        let ctx = build_handoff_context(
            "Task",
            "Review",
            "",
            &[],
            &[PlanItem {
                description: "Check X".to_string(),
                completed: true,
            }],
        );
        let prompt = format_review_prompt(&ctx, "2 files changed");
        assert!(prompt.contains("## Expected Plan"));
        assert!(prompt.contains("[x] Check X"));
        assert!(prompt.contains("## Changes Made"));
        assert!(prompt.contains("2 files changed"));
    }

    #[test]
    fn test_parse_checklist_mixed() {
        let text = "- [ ] Do something\n- [x] Done thing\n- [X] Also done";
        let items = parse_checklist(text);
        assert_eq!(items.len(), 3);
        assert!(!items[0].completed);
        assert!(items[1].completed);
        assert!(items[2].completed);
        assert_eq!(items[0].description, "Do something");
        assert_eq!(items[1].description, "Done thing");
    }

    #[test]
    fn test_parse_checklist_empty() {
        let items = parse_checklist("No checklist items here");
        assert!(items.is_empty());
    }

    #[test]
    fn test_parse_checklist_ignores_malformed() {
        let text = "- [ ] valid\n- broken line\n- [ ] also valid";
        let items = parse_checklist(text);
        assert_eq!(items.len(), 2);
    }
}
