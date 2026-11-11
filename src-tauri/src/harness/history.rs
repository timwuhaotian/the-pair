//! The conversation one agent keeps across its turns.
//!
//! Every (pair, role) owns one history in OpenAI chat form: the user turn the
//! agent answered, the assistant's reply with the tool calls it asked for, and
//! every tool result. It is replayed whole on each request, so a turn sees
//! everything the earlier ones said — what a CLI agent gets from its own session
//! file, without depending on one existing.
//!
//! A turn only ever appends: a failed attempt's partial output is discarded by
//! the transport, so the history holds complete exchanges only. What a turn
//! leaves behind is cut the same way [`bounded`] cuts the replay, so a pair's
//! stored conversation cannot grow with one tool-heavy turn's results.

use crate::llm::Reply;
use serde_json::{json, Value};

/// Messages one history may hold. A pair's whole run — plan, implement, review,
/// repair — stays well under this; the cap only catches a pair that runs for
/// days without finishing, and a context window the agent would otherwise
/// overrun.
pub const MAX_HISTORY_MESSAGES: usize = 400;

/// Text one history may hold. One tool result may be tens of kilobytes, so the
/// message count alone does not bound what is replayed — or what the pair stores.
const MAX_HISTORY_BYTES: usize = 400 * 1024;

/// A history cut down to what the next turn replays.
pub struct Bounded {
    /// The turns to send, oldest first.
    pub messages: Vec<Value>,
    /// What to tell the agent about the turns that were dropped, when any were.
    /// It rides on the system prompt rather than in the history: an extra
    /// message there would either repeat a `user` turn or strand a `tool` result,
    /// and some gateways reject both.
    pub notice: Option<String>,
}

/// The user turn one agent answers.
pub fn user(text: &str) -> Value {
    json!({"role": "user", "content": text})
}

/// The assistant turn that answered, with the tool calls it asked for — or
/// `None` when the reply carried neither text nor calls, which is not a turn any
/// provider accepts back.
///
/// Each key is written only when it has something in it: some providers reject
/// an empty `content` string, and OpenAI's chat endpoint rejects an empty
/// `tool_calls` array, so a plain reply must not carry one.
pub fn assistant(reply: &Reply) -> Option<Value> {
    let tool_calls: Vec<Value> = reply
        .tool_calls
        .iter()
        .map(|call| {
            json!({
                "type": "function",
                "id": call.id,
                "function": {"name": call.name, "arguments": call.arguments},
            })
        })
        .collect();
    let says_something = !reply.text.trim().is_empty();
    if !says_something && tool_calls.is_empty() {
        return None;
    }
    let mut message = serde_json::Map::new();
    message.insert("role".into(), json!("assistant"));
    if says_something {
        message.insert("content".into(), json!(reply.text));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    Some(Value::Object(message))
}

/// A tool's answer, addressed to the call that asked for it.
pub fn tool_result(id: &str, output: &str) -> Value {
    json!({"role": "tool", "tool_call_id": id, "content": output})
}

/// What stands in for the answer of a call the turn stopped before running.
const NEVER_RAN: &str = "This call did not finish: the turn was stopped before it returned.";

/// Answers every tool call the last assistant turn made that has no result yet.
///
/// A turn that fails or is stopped part-way through its calls leaves an
/// assistant turn asking for more than was answered, and every provider refuses
/// a history like that. The calls that did run keep their real results.
pub fn close_open_calls(messages: &mut Vec<Value>) {
    let Some(asked_at) = messages
        .iter()
        .rposition(|message| role_of(message) == "assistant")
    else {
        return;
    };
    let answered: Vec<&str> = messages[asked_at + 1..]
        .iter()
        .filter_map(|message| message.get("tool_call_id").and_then(Value::as_str))
        .collect();
    let open: Vec<String> = messages[asked_at]
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|call| call.get("id").and_then(Value::as_str))
        .filter(|id| !answered.contains(id))
        .map(str::to_string)
        .collect();
    for id in open {
        messages.push(tool_result(&id, NEVER_RAN));
    }
}

/// The history as the next turn replays it: at most [`MAX_HISTORY_MESSAGES`]
/// messages and [`MAX_HISTORY_BYTES`] of text, plus a notice when anything was
/// dropped.
///
/// The cut lands on the oldest message of the window that is not a `tool`
/// result, so a result is never replayed without the assistant call it answers.
pub fn bounded(history: &[Value]) -> Bounded {
    let cut = window_start(history);
    if cut == 0 {
        return Bounded {
            messages: history.to_vec(),
            notice: None,
        };
    }

    Bounded {
        messages: history[cut..].to_vec(),
        notice: Some(NOTICE.to_string()),
    }
}

/// The history a turn leaves behind, cut at the same place [`bounded`] cuts.
///
/// `bounded` keeps the next request inside the context window; this keeps what
/// the pair stores — which its snapshot fsyncs after every turn — from growing
/// with one tool-heavy turn's results. The window is the same, so what is stored
/// is what the next turn would replay.
pub fn stored(history: &[Value]) -> Vec<Value> {
    history[window_start(history)..].to_vec()
}

/// The message the window starts at, or 0 when nothing has to be dropped.
///
/// The message count alone does not bound a history: one tool result may be
/// tens of kilobytes, and hundreds of those would overrun any context window —
/// and bloat the snapshot the pair writes after every turn.
fn window_start(history: &[Value]) -> usize {
    let mut window = 0usize;
    let mut bytes = 0usize;
    for (index, message) in history.iter().enumerate().rev() {
        bytes += size_of(message);
        if bytes > MAX_HISTORY_BYTES {
            window = index + 1;
            break;
        }
    }
    if history.len() <= MAX_HISTORY_MESSAGES && window == 0 {
        return 0;
    }
    window = window.max(history.len().saturating_sub(MAX_HISTORY_MESSAGES));
    if window == 0 {
        return 0;
    }

    (window..history.len())
        .find(|index| role_of(&history[*index]) != "tool")
        .unwrap_or(window)
}

/// What stands in for the turns the window dropped. It rides on the system
/// prompt rather than in the history: an extra message there would either repeat
/// a `user` turn or strand a `tool` result, and some gateways reject both.
const NOTICE: &str = "The earlier part of this conversation was dropped to stay within your context \
                      window. The task, the workspace and the pair's state are unchanged; the turns \
                      below are the current ones.";

fn role_of(message: &Value) -> &str {
    message.get("role").and_then(Value::as_str).unwrap_or("")
}

/// A message's rough share of a request: its own text plus the JSON around it.
fn size_of(message: &Value) -> usize {
    message.to_string().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(index: usize) -> Vec<Value> {
        vec![
            user(&format!("task {}", index)),
            assistant(&Reply {
                text: "on it".into(),
                tool_calls: Vec::new(),
                ..Reply::default()
            })
            .expect("a reply with text is a turn"),
        ]
    }

    fn history_of(turns: usize) -> Vec<Value> {
        (0..turns).flat_map(turn).collect()
    }

    #[test]
    fn a_short_history_is_replayed_untouched() {
        let history = history_of(3);
        let bounded = bounded(&history);
        assert_eq!(bounded.messages, history);
        assert!(bounded.notice.is_none());
    }

    #[test]
    fn an_empty_history_needs_no_notice() {
        let bounded = bounded(&[]);
        assert!(bounded.messages.is_empty());
        assert!(bounded.notice.is_none());
    }

    #[test]
    fn an_over_long_history_keeps_its_newest_turns_and_says_so() {
        let history = history_of(MAX_HISTORY_MESSAGES / 2 + 5);
        let bounded = bounded(&history);

        assert_eq!(bounded.messages.len(), MAX_HISTORY_MESSAGES);
        assert!(bounded
            .notice
            .is_some_and(|notice| notice.contains("dropped")));
        // The newest turns survive, in order.
        assert_eq!(*bounded.messages.last().unwrap(), *history.last().unwrap());
        assert_eq!(bounded.messages[1], history[11]);
    }

    #[test]
    fn the_cut_never_lands_on_a_tool_result() {
        // Turns of user → assistant(calls) → tool result, sized past the cap and
        // phased so the window's first message *is* a tool result.
        let mut history: Vec<Value> = Vec::new();
        for index in 0..140 {
            history.push(user(&format!("task {}", index)));
            history.extend(assistant(&Reply {
                text: String::new(),
                tool_calls: vec![crate::llm::ToolCall {
                    id: format!("call-{}", index),
                    name: "read".into(),
                    arguments: "{}".into(),
                }],
                ..Reply::default()
            }));
            history.push(tool_result(&format!("call-{}", index), "done"));
        }
        let window = history.len() - MAX_HISTORY_MESSAGES;
        assert_eq!(
            role_of(&history[window]),
            "tool",
            "the window starts on one"
        );

        let bounded = bounded(&history);
        // The window started on an orphaned tool result, so the cut moved
        // forward to the next turn boundary and dropped the call with it.
        assert_eq!(role_of(&bounded.messages[0]), "user");
    }

    #[test]
    fn a_history_of_fat_tool_results_is_bounded_by_bytes_too() {
        // Few messages, but each one is a tool result near the tools' own cap:
        // the message count alone would let this through.
        let mut history: Vec<Value> = Vec::new();
        for index in 0..40 {
            history.push(user(&format!("turn {}", index)));
            history.push(tool_result(
                &format!("call-{}", index),
                &"y".repeat(30 * 1024),
            ));
        }

        let bounded = bounded(&history);
        let kept_bytes: usize = bounded.messages.iter().map(size_of).sum();
        assert!(
            kept_bytes < MAX_HISTORY_BYTES + 30 * 1024,
            "{} bytes replayed",
            kept_bytes
        );
        assert!(bounded.notice.is_some(), "the drop is announced");
        assert_ne!(
            role_of(&bounded.messages[0]),
            "tool",
            "the tail starts on a turn"
        );
    }

    #[test]
    fn a_tool_result_names_the_call_it_answers() {
        let message = tool_result("call-1", "written");
        assert_eq!(message["role"], "tool");
        assert_eq!(message["tool_call_id"], "call-1");
        assert_eq!(message["content"], "written");
    }

    #[test]
    fn what_a_turn_stores_is_cut_where_the_replay_is() {
        let history = history_of(MAX_HISTORY_MESSAGES / 2 + 5);
        assert_eq!(stored(&history), bounded(&history).messages);
        assert_eq!(stored(&history).len(), MAX_HISTORY_MESSAGES);
    }

    #[test]
    fn a_stored_history_of_fat_tool_results_is_capped_by_bytes_too() {
        // One tool-heavy turn appends its results whole: the replay window keeps
        // the request small, but the pair's stored conversation — which its
        // snapshot fsyncs after every turn — has to be capped as well.
        let mut history: Vec<Value> = Vec::new();
        for index in 0..40 {
            history.push(user(&format!("turn {}", index)));
            history.push(tool_result(
                &format!("call-{}", index),
                &"y".repeat(30 * 1024),
            ));
        }

        let stored = stored(&history);
        let kept_bytes: usize = stored.iter().map(size_of).sum();
        assert!(
            kept_bytes < MAX_HISTORY_BYTES + 30 * 1024,
            "{} bytes stored",
            kept_bytes
        );
        assert_ne!(role_of(&stored[0]), "tool", "the tail starts on a turn");
        assert_eq!(stored, bounded(&history).messages);
    }

    #[test]
    fn a_history_that_fits_is_stored_whole() {
        let history = history_of(3);
        assert_eq!(stored(&history), history);
        assert!(stored(&[]).is_empty());
    }

    #[test]
    fn an_assistant_turn_omits_content_when_it_only_called_tools() {
        let reply = Reply {
            tool_calls: vec![crate::llm::ToolCall {
                id: "call-1".into(),
                name: "read".into(),
                arguments: "{}".into(),
            }],
            ..Reply::default()
        };
        let message = assistant(&reply).expect("a reply with calls is a turn");
        assert!(message.get("content").is_none());
        assert_eq!(message["tool_calls"][0]["function"]["name"], "read");
    }

    #[test]
    fn calls_a_stopped_turn_never_answered_get_a_result() {
        let two_calls = assistant(&Reply {
            tool_calls: ["call-1", "call-2"]
                .iter()
                .map(|id| crate::llm::ToolCall {
                    id: (*id).into(),
                    name: "bash".into(),
                    arguments: "{}".into(),
                })
                .collect(),
            ..Reply::default()
        })
        .unwrap();
        let mut history = vec![
            user("do it"),
            two_calls,
            tool_result("call-1", "edited a.rs"),
        ];

        close_open_calls(&mut history);

        assert_eq!(history.len(), 4);
        assert_eq!(history[2]["content"], "edited a.rs", "a real result stays");
        assert_eq!(history[3]["tool_call_id"], "call-2");
        assert!(history[3]["content"]
            .as_str()
            .unwrap()
            .contains("did not finish"));

        // Nothing open, nothing added — and a history with no assistant turn at
        // all is left alone.
        close_open_calls(&mut history);
        assert_eq!(history.len(), 4);
        let mut bare = vec![user("hi")];
        close_open_calls(&mut bare);
        assert_eq!(bare.len(), 1);
    }

    #[test]
    fn a_plain_reply_carries_no_tool_calls_key() {
        // OpenAI's chat endpoint rejects `"tool_calls": []`, and the stored form
        // is replayed to it verbatim.
        let message = assistant(&Reply {
            text: "done".into(),
            ..Reply::default()
        })
        .expect("a reply with text is a turn");
        assert_eq!(message["content"], "done");
        assert!(message.get("tool_calls").is_none(), "{}", message);
    }

    #[test]
    fn a_reply_with_neither_text_nor_calls_is_not_a_turn() {
        assert!(assistant(&Reply::default()).is_none());
        assert!(assistant(&Reply {
            text: "  \n".into(),
            ..Reply::default()
        })
        .is_none());
    }
}
