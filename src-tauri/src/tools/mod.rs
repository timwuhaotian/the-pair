//! The tools an agent can call, and what each role is allowed to call.
//!
//! A tool is a JSON Schema the model is offered plus a runner. The schema uses
//! The Pair's own tool names; a gateway that renames them (Muse Code) is mapped
//! by the transport, so nothing here knows about wire names.
//!
//! The mentor is read-only by construction: its [`ToolPolicy`] simply does not
//! offer the writing tools, so there is no permission check to get wrong.

pub mod bash;
pub mod edit;
pub mod files;
pub mod search;
pub mod todo;

use serde_json::{json, Value};
use std::path::Path;

/// What a role may do with the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPolicy {
    /// Plan and review: read, search, and keep a todo list.
    ReadOnly,
    /// Carry the work out: everything, including writing files and commands.
    Full,
}

impl ToolPolicy {
    /// The tools this policy offers.
    pub fn tools(self) -> &'static [&'static str] {
        match self {
            ToolPolicy::ReadOnly => &["read", "grep", "glob", "todo_write"],
            ToolPolicy::Full => &[
                "read",
                "write",
                "edit",
                "bash",
                "grep",
                "glob",
                "todo_write",
            ],
        }
    }

    pub fn allows(self, name: &str) -> bool {
        self.tools().contains(&name)
    }
}

/// Everything a tool needs to run: where the work happens, and what this agent
/// may do there.
pub struct ToolContext<'a> {
    pub directory: &'a Path,
    pub policy: ToolPolicy,
}

/// The schemas this role is offered, in the order the model sees them.
pub fn schemas(policy: ToolPolicy) -> Vec<Value> {
    policy
        .tools()
        .iter()
        .filter_map(|name| schema(name))
        .collect()
}

/// One tool's schema, in The Pair's own name.
pub fn schema(name: &str) -> Option<Value> {
    let function = match name {
        "read" => json!({
            "name": "read",
            "description": "Read a file from the workspace. Returns its contents with line numbers.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path relative to the workspace root."}
                },
                "required": ["path"]
            }
        }),
        "write" => json!({
            "name": "write",
            "description": "Write a file, creating or replacing it. Parent directories are created.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path relative to the workspace root."},
                    "content": {"type": "string", "description": "The whole file content."}
                },
                "required": ["path", "content"]
            }
        }),
        "edit" => json!({
            "name": "edit",
            "description": "Replace one exact stretch of a file with new text. The old text must appear exactly once.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path relative to the workspace root."},
                    "old_string": {"type": "string", "description": "The exact text to replace."},
                    "new_string": {"type": "string", "description": "The text to put in its place."}
                },
                "required": ["path", "old_string", "new_string"]
            }
        }),
        "bash" => json!({
            "name": "bash",
            "description": "Run a shell command in the workspace and read its output.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The command line to run."}
                },
                "required": ["command"]
            }
        }),
        "grep" => json!({
            "name": "grep",
            "description": "Search file contents for a pattern. Returns matching lines with their paths.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "The text or regular expression to look for."},
                    "path": {"type": "string", "description": "Directory to search in, relative to the workspace root. Defaults to the whole workspace."}
                },
                "required": ["pattern"]
            }
        }),
        "glob" => json!({
            "name": "glob",
            "description": "List files whose path matches a pattern, e.g. src/**/*.rs.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "The glob pattern to match."},
                    "path": {"type": "string", "description": "Directory to search in, relative to the workspace root. Defaults to the whole workspace."}
                },
                "required": ["pattern"]
            }
        }),
        "todo_write" => json!({
            "name": "todo_write",
            "description": "Replace the task list with the given items, so progress is visible.",
            "parameters": {
                "type": "object",
                "properties": {
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": {"type": "string"},
                                "status": {"type": "string", "enum": ["pending", "in_progress", "done"]}
                            },
                            "required": ["content", "status"]
                        },
                        "description": "The whole list, in order."
                    }
                },
                "required": ["todos"]
            }
        }),
        _ => return None,
    };
    Some(json!({"type": "function", "function": function}))
}

/// Runs one tool call and returns the text the model reads.
///
/// A tool never panics on a bad call: a missing argument or an unreadable path
/// is an error string the model can act on.
pub async fn run(ctx: &ToolContext<'_>, name: &str, arguments: &str) -> Result<String, String> {
    if schema(name).is_none() {
        return Err(format!("unknown tool `{}`", name));
    }
    if !ctx.policy.allows(name) {
        return Err(format!(
            "the tool `{}` is not available to this agent",
            name
        ));
    }
    let parsed: Value = serde_json::from_str(arguments)
        .map_err(|error| format!("the arguments for `{}` are not valid JSON: {}", name, error))?;
    let args = parsed
        .as_object()
        .ok_or_else(|| format!("the arguments for `{}` must be a JSON object", name))?;

    match name {
        "read" => files::read(ctx, args).await,
        "write" => files::write(ctx, args).await,
        "edit" => edit::edit(ctx, args).await,
        "bash" => bash::run(ctx, args).await,
        "grep" => search::grep(ctx, args).await,
        "glob" => search::glob(ctx, args).await,
        "todo_write" => todo::write(args),
        // `schema(name)` already rejected anything else, so this is unreachable
        // through `run`; it keeps a new policy entry from silently doing nothing.
        other => Err(format!("unknown tool `{}`", other)),
    }
}

/// A one-line label for the console: what the call is about, not its result.
pub fn summary(name: &str, arguments: &str) -> String {
    let Ok(parsed) = serde_json::from_str::<Value>(arguments) else {
        return name.to_string();
    };
    let field = |key: &str| {
        parsed
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default()
    };
    match name {
        "read" | "write" | "edit" => {
            let path = field("path");
            if path.is_empty() {
                name.to_string()
            } else {
                format!("{} {}", name, path)
            }
        }
        "bash" => {
            let command = field("command");
            if command.is_empty() {
                name.to_string()
            } else {
                format!("bash {}", truncate_line(&command, 80))
            }
        }
        "grep" | "glob" => {
            let pattern = field("pattern");
            if pattern.is_empty() {
                name.to_string()
            } else {
                format!("{} {:?}", name, pattern)
            }
        }
        "todo_write" => {
            let count = parsed
                .get("todos")
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or(0);
            format!("todo_write ({} items)", count)
        }
        other => other.to_string(),
    }
}

/// One line, cut on a character boundary so a long command stays readable.
fn truncate_line(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(tools: &[Value]) -> Vec<String> {
        tools
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn the_mentor_is_offered_read_only_tools() {
        let policy = ToolPolicy::ReadOnly;
        assert_eq!(policy.tools(), &["read", "grep", "glob", "todo_write"]);
        assert!(policy.allows("read"));
        assert!(policy.allows("todo_write"));
        assert!(!policy.allows("write"));
        assert!(!policy.allows("edit"));
        assert!(!policy.allows("bash"));
    }

    #[test]
    fn the_executor_is_offered_every_tool() {
        let policy = ToolPolicy::Full;
        for tool in [
            "read",
            "write",
            "edit",
            "bash",
            "grep",
            "glob",
            "todo_write",
        ] {
            assert!(policy.allows(tool), "{} missing", tool);
        }
    }

    #[test]
    fn schemas_cover_exactly_what_the_policy_allows() {
        assert_eq!(
            names(&schemas(ToolPolicy::ReadOnly)),
            ToolPolicy::ReadOnly.tools()
        );
        assert_eq!(names(&schemas(ToolPolicy::Full)), ToolPolicy::Full.tools());
    }

    #[test]
    fn every_schema_is_a_function_with_a_name_and_parameters() {
        for tool in ToolPolicy::Full.tools() {
            let schema = schema(tool).expect(tool);
            assert_eq!(schema["type"], "function");
            assert_eq!(schema["function"]["name"], *tool);
            assert!(schema["function"]["parameters"].is_object(), "{}", tool);
            assert!(
                schema["function"]["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "{} has no description",
                tool
            );
        }
        assert!(schema("nope").is_none());
    }

    #[tokio::test]
    async fn a_tool_the_policy_does_not_allow_is_refused() {
        let ctx = ToolContext {
            directory: Path::new("."),
            policy: ToolPolicy::ReadOnly,
        };
        let error = run(&ctx, "bash", r#"{"command":"ls"}"#).await.unwrap_err();
        assert!(error.contains("not available"), "{}", error);
    }

    #[tokio::test]
    async fn arguments_that_are_not_json_are_a_readable_error() {
        let ctx = ToolContext {
            directory: Path::new("."),
            policy: ToolPolicy::Full,
        };
        let error = run(&ctx, "read", "not json").await.unwrap_err();
        assert!(error.contains("not valid JSON"), "{}", error);
        let error = run(&ctx, "read", "[1,2]").await.unwrap_err();
        assert!(error.contains("must be a JSON object"), "{}", error);
    }

    #[tokio::test]
    async fn an_unknown_tool_is_a_readable_error() {
        let ctx = ToolContext {
            directory: Path::new("."),
            policy: ToolPolicy::Full,
        };
        let error = run(&ctx, "nope", "{}").await.unwrap_err();
        assert!(error.contains("unknown tool"), "{}", error);
    }

    #[test]
    fn summaries_name_the_call_without_its_result() {
        assert_eq!(
            summary("read", r#"{"path":"src/main.rs"}"#),
            "read src/main.rs"
        );
        assert_eq!(
            summary("bash", &format!(r#"{{"command":"{}"}}"#, "x".repeat(200))),
            format!("bash {}…", "x".repeat(80))
        );
        assert_eq!(
            summary("grep", r#"{"pattern":"fn main"}"#),
            "grep \"fn main\""
        );
        assert_eq!(summary("read", "not json"), "read");
        assert_eq!(summary("weird", "{}"), "weird");
    }
}
