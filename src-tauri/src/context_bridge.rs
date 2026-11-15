use serde::{Deserialize, Serialize};

/// A single item in the mentor's plan checklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub description: String,
    pub completed: bool,
}

/// Parses a checklist from mentor output text.
/// Accepts `-`, `*` and ordered (`1.`) bullets with `[ ]` / `[x]` (case-insensitive).
pub fn parse_checklist(text: &str) -> Vec<PlanItem> {
    text.lines().filter_map(parse_checklist_line).collect()
}

fn parse_checklist_line(line: &str) -> Option<PlanItem> {
    let trimmed = line.trim();
    // Strip the bullet: `- `, `* `, or `1. ` (any `N. ` ordered marker).
    let after_bullet = if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        rest
    } else if let Some(dot) = trimmed.find(". ") {
        let (number, _) = trimmed.split_at(dot);
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        &trimmed[dot + 2..]
    } else {
        return None;
    };
    let after_bullet = after_bullet.trim_start();
    let bracketed = after_bullet.strip_prefix('[')?;
    let end = bracketed.find(']')?;
    let marker = &bracketed[..end];
    // `[]` is malformed (empty marker); `[ ]` is unchecked, `[x]`/`[X]` done.
    let completed = if marker.eq_ignore_ascii_case("x") {
        true
    } else if !marker.is_empty() && marker.chars().all(|c| c == ' ' || c == '\t') {
        false
    } else {
        return None;
    };
    let desc = after_bullet[end + 2..].trim();
    if desc.is_empty() {
        return None;
    }
    Some(PlanItem {
        description: desc.to_string(),
        completed,
    })
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

    #[test]
    fn test_parse_checklist_star_and_ordered_bullets() {
        let text = "* [ ] star task\n* [X] star done\n1. [ ] ordered task\n2. [x] ordered done";
        let items = parse_checklist(text);
        assert_eq!(items.len(), 4);
        assert!(!items[0].completed);
        assert!(items[1].completed);
        assert!(!items[2].completed);
        assert!(items[3].completed);
        assert_eq!(items[0].description, "star task");
        assert_eq!(items[2].description, "ordered task");
    }

    #[test]
    fn test_parse_checklist_rejects_bad_markers() {
        let text = "- [y] nope\n- [] nope\n1. no box here\nx. [ ] bad number\n- [ ]\n- [ ] ok";
        let items = parse_checklist(text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].description, "ok");
    }
}
