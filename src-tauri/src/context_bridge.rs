use serde::{Deserialize, Serialize};

/// A single item in the mentor's plan checklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub description: String,
    pub completed: bool,
}

/// Parses a checklist from mentor output text.
/// Looks for lines matching "- [ ] item" or "- [x] item".
pub fn parse_checklist(text: &str) -> Vec<PlanItem> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if let Some(desc) = trimmed
                .strip_prefix("- [x]")
                .or_else(|| trimmed.strip_prefix("- [X]"))
            {
                let desc = desc.trim();
                if !desc.is_empty() {
                    return Some(PlanItem {
                        description: desc.to_string(),
                        completed: true,
                    });
                }
            } else if let Some(desc) = trimmed.strip_prefix("- [ ]") {
                let desc = desc.trim();
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
