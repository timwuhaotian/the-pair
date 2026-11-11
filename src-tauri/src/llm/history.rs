//! History read in its stored OpenAI chat form: the system message, message
//! text, and tool-call arguments.
//!
//! Every protocol module keeps history the same way — the OpenAI chat shape The
//! Pair stores — and converts a copy of it per request, so the bytes sent for
//! earlier messages never change. OpenAI Chat Completions sends the stored form
//! unchanged; the other protocols read it through these accessors.

use serde_json::Value;

/// The system prompt: the first message, which is always the system message.
pub(super) fn system_text(messages: &[Value]) -> (&str, &[Value]) {
    match messages.first() {
        Some(first) if first["role"] == "system" => (
            first["content"].as_str().unwrap_or_default(),
            &messages[1..],
        ),
        _ => ("", messages),
    }
}

/// A message's text, whether `content` is a string or a list of parts.
pub(super) fn content_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Tool-call arguments as a JSON object, or an empty object when they aren't one.
pub(super) fn arguments_object(arguments: &str) -> Value {
    match serde_json::from_str::<Value>(arguments) {
        Ok(value @ Value::Object(_)) => value,
        _ => Value::Object(Default::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_system_message_is_split_off_the_history() {
        let messages = vec![
            json!({"role": "system", "content": "be brief"}),
            json!({"role": "user", "content": "hi"}),
        ];
        let (system, history) = system_text(&messages);
        assert_eq!(system, "be brief");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["role"], "user");
    }

    #[test]
    fn history_without_a_system_message_stays_whole() {
        let messages = vec![json!({"role": "user", "content": "hi"})];
        let (system, history) = system_text(&messages);
        assert_eq!(system, "");
        assert_eq!(history.len(), 1);
        let (system, history) = system_text(&[]);
        assert_eq!(system, "");
        assert!(history.is_empty());
    }

    #[test]
    fn content_text_reads_strings_and_part_lists() {
        assert_eq!(content_text(&json!({"content": "plain"})), "plain");
        assert_eq!(
            content_text(
                &json!({"content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]})
            ),
            "a\nb"
        );
        assert_eq!(content_text(&json!({"content": 12})), "");
    }

    #[test]
    fn arguments_are_read_as_an_object_or_replaced() {
        assert_eq!(
            arguments_object(r#"{"path":"a.rs"}"#),
            json!({"path": "a.rs"})
        );
        assert_eq!(arguments_object("not json"), json!({}));
        assert_eq!(arguments_object("[1,2]"), json!({}));
    }
}
