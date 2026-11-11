//! The agent's own task list: what it is doing now, and what comes next.
//!
//! `todo_write` replaces the whole list rather than appending to it, so the
//! model can only ever describe the list it means to keep — there is no partial
//! update to get wrong. A rejected call leaves the previous list alone, so a
//! half-written list is never what the human is shown.
//!
//! The list lives in this process's memory for the current turn: the harness
//! reads it back with [`snapshot`] to show the pair's progress, and calls
//! [`clear`] when a turn starts. Nothing is written to the workspace, so the
//! tool cannot damage the pair's files.

use std::sync::Mutex;

use serde::Serialize;
use serde_json::{Map, Value};

/// How far along one item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    /// Not started yet.
    Pending,
    /// The item being worked on right now.
    InProgress,
    /// Finished.
    Done,
}

impl TodoStatus {
    /// The checkbox the rendered list shows for this status.
    fn marker(self) -> &'static str {
        match self {
            TodoStatus::Pending => "[ ]",
            TodoStatus::InProgress => "[~]",
            TodoStatus::Done => "[x]",
        }
    }

    /// The status a string names, if it names one.
    fn parse(text: &str) -> Option<Self> {
        match text {
            "pending" => Some(TodoStatus::Pending),
            "in_progress" => Some(TodoStatus::InProgress),
            "done" => Some(TodoStatus::Done),
            _ => None,
        }
    }
}

/// One item on the list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Todo {
    /// What has to happen, trimmed of surrounding whitespace.
    pub content: String,
    pub status: TodoStatus,
}

/// Items spelled out one by one in the reply; the rest is counted instead, so a
/// long list cannot flood the model's context with text it just wrote.
const MAX_RENDERED: usize = 50;

/// The list as it stands, shared by every pair running in this process.
///
/// A poisoned lock is recovered rather than propagated: a panicking caller must
/// not leave every later turn unable to write its list.
fn list() -> &'static Mutex<Vec<Todo>> {
    static LIST: Mutex<Vec<Todo>> = Mutex::new(Vec::new());
    &LIST
}

/// The list as it stands, for the harness to show the human.
pub fn snapshot() -> Vec<Todo> {
    list()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Empties the list; the harness calls this when a turn starts.
pub fn clear() {
    list()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

/// Replaces the task list with `args["todos"]` and returns the list as rendered.
///
/// Each item needs a non-empty `content`; `status` is one of `pending`,
/// `in_progress` or `done`, and an item without one counts as pending.
pub fn write(args: &Map<String, Value>) -> Result<String, String> {
    let items = args.get("todos").and_then(Value::as_array).ok_or_else(|| {
        "todo_write needs `todos`: an array of {content, status} items.".to_string()
    })?;

    let mut todos = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        todos.push(parse_item(item, index + 1)?);
    }

    let open = todos
        .iter()
        .filter(|todo| todo.status != TodoStatus::Done)
        .count();
    let rendered = render(&todos, open);
    *list()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = todos;
    Ok(rendered)
}

/// One item of the call, with its 1-based position named in any complaint.
fn parse_item(item: &Value, index: usize) -> Result<Todo, String> {
    let object = item.as_object().ok_or_else(|| {
        format!(
            "todo item {} must be an object with `content` and `status`.",
            index
        )
    })?;
    let content = object
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .ok_or_else(|| format!("todo item {} needs a non-empty `content`.", index))?;
    let status = match object.get("status") {
        None | Some(Value::Null) => TodoStatus::Pending,
        Some(Value::String(text)) => TodoStatus::parse(text).ok_or_else(|| {
            format!(
                "todo item {} has an unknown status `{}`; use pending, in_progress or done.",
                index, text
            )
        })?,
        Some(_) => {
            return Err(format!(
                "the `status` of todo item {} must be a string.",
                index
            ))
        }
    };
    Ok(Todo {
        content: content.to_string(),
        status,
    })
}

/// The list as the model reads it back: a one-line count, then each item's
/// checkbox and content, so the call confirms exactly what is now on the list.
fn render(todos: &[Todo], open: usize) -> String {
    if todos.is_empty() {
        return "Task list cleared.".to_string();
    }
    let mut out = format!(
        "Task list updated: {} open, {} done.",
        open,
        todos.len() - open
    );
    for todo in todos.iter().take(MAX_RENDERED) {
        out.push('\n');
        out.push_str(todo.status.marker());
        out.push(' ');
        out.push_str(&todo.content);
    }
    let hidden = todos.len().saturating_sub(MAX_RENDERED);
    if hidden > 0 {
        out.push_str(&format!("\n… {} more", hidden));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The list is process-wide, so these tests take turns.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn call(value: Value) -> Result<String, String> {
        write(value.as_object().expect("an object"))
    }

    #[test]
    fn a_list_replaces_the_previous_one_and_is_rendered_back() {
        let _guard = lock();
        clear();
        call(json!({"todos": [{"content": "read the contract", "status": "done"}]})).unwrap();
        let rendered = call(json!({"todos": [
            {"content": "write the tool", "status": "in_progress"},
            {"content": "test it", "status": "pending"},
            {"content": "read the contract", "status": "done"}
        ]}))
        .unwrap();
        assert_eq!(
            rendered,
            "Task list updated: 2 open, 1 done.\n[~] write the tool\n[ ] test it\n[x] read the contract"
        );
        assert_eq!(
            snapshot(),
            vec![
                Todo {
                    content: "write the tool".to_string(),
                    status: TodoStatus::InProgress
                },
                Todo {
                    content: "test it".to_string(),
                    status: TodoStatus::Pending
                },
                Todo {
                    content: "read the contract".to_string(),
                    status: TodoStatus::Done
                }
            ]
        );
    }

    #[test]
    fn an_item_without_a_status_counts_as_pending() {
        let _guard = lock();
        clear();
        let rendered = call(json!({"todos": [{"content": "  pad me  "}]})).unwrap();
        assert_eq!(rendered, "Task list updated: 1 open, 0 done.\n[ ] pad me");
        assert_eq!(snapshot()[0].status, TodoStatus::Pending);
    }

    #[test]
    fn an_unknown_status_is_refused_and_keeps_the_old_list() {
        let _guard = lock();
        clear();
        call(json!({"todos": [{"content": "keep me", "status": "done"}]})).unwrap();
        let error =
            call(json!({"todos": [{"content": "nope", "status": "completed"}]})).unwrap_err();
        assert!(error.contains("unknown status `completed`"), "{}", error);
        assert!(error.contains("in_progress"), "{}", error);
        assert_eq!(snapshot().len(), 1);
        assert_eq!(snapshot()[0].content, "keep me");
    }

    #[test]
    fn content_that_is_empty_or_blank_is_refused() {
        let _guard = lock();
        clear();
        for content in ["", "   "] {
            let error =
                call(json!({"todos": [{"content": content, "status": "pending"}]})).unwrap_err();
            assert!(error.contains("non-empty `content`"), "{}", error);
        }
        let error = call(json!({"todos": [{"content": "fine"}, {"content": ""}]})).unwrap_err();
        assert!(error.contains("todo item 2"), "{}", error);
        assert!(snapshot().is_empty());
    }

    #[test]
    fn todos_that_are_not_an_array_are_refused() {
        let _guard = lock();
        clear();
        for value in [
            json!("one item"),
            json!({"content": "one item"}),
            json!(3),
            json!(null),
        ] {
            let error = call(json!({"todos": value})).unwrap_err();
            assert!(error.contains("needs `todos`"), "{}", error);
        }
        let error = call(json!({})).unwrap_err();
        assert!(error.contains("needs `todos`"), "{}", error);
    }

    #[test]
    fn an_item_that_is_not_an_object_or_has_the_wrong_types_is_refused() {
        let _guard = lock();
        clear();
        let error = call(json!({"todos": ["just text"]})).unwrap_err();
        assert!(error.contains("todo item 1 must be an object"), "{}", error);

        let error = call(json!({"todos": [{"content": 7}]})).unwrap_err();
        assert!(error.contains("non-empty `content`"), "{}", error);

        let error = call(json!({"todos": [{"content": "ok", "status": true}]})).unwrap_err();
        assert!(
            error.contains("`status` of todo item 1 must be a string"),
            "{}",
            error
        );
        assert!(snapshot().is_empty());
    }

    #[test]
    fn snapshot_returns_a_copy_and_clear_empties_the_list() {
        let _guard = lock();
        clear();
        call(json!({"todos": [{"content": "one", "status": "pending"}]})).unwrap();
        let mut taken = snapshot();
        taken.clear();
        assert_eq!(snapshot().len(), 1);

        clear();
        assert!(snapshot().is_empty());
    }

    #[test]
    fn an_empty_list_clears_the_list() {
        let _guard = lock();
        clear();
        call(json!({"todos": [{"content": "one", "status": "in_progress"}]})).unwrap();
        let rendered = call(json!({"todos": []})).unwrap();
        assert_eq!(rendered, "Task list cleared.");
        assert!(snapshot().is_empty());
    }

    #[test]
    fn a_long_list_is_summarised_rather_than_spelled_out() {
        let _guard = lock();
        clear();
        let items: Vec<Value> = (0..MAX_RENDERED + 3)
            .map(|i| json!({"content": format!("item {}", i), "status": "pending"}))
            .collect();
        let rendered = call(json!({"todos": items})).unwrap();
        assert!(
            rendered.starts_with("Task list updated: 53 open, 0 done.\n[ ] item 0\n"),
            "{}",
            rendered
        );
        assert!(rendered.ends_with("\n… 3 more"), "{}", rendered);
        assert_eq!(rendered.lines().count(), MAX_RENDERED + 2);
        assert_eq!(snapshot().len(), MAX_RENDERED + 3);
    }
}
