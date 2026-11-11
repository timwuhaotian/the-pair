//! OpenAI Chat Completions: `POST {base}/chat/completions`, one SSE stream back.
//!
//! The request carries the history in its stored OpenAI chat form unchanged, so
//! the cached prefix keeps hitting across turns:
//!
//! ```json
//! {"model":"gpt-5","messages":[{"role":"user","content":"hi"}],
//!  "stream":true,"stream_options":{"include_usage":true},
//!  "prompt_cache_key":"pair-1-mentor","max_completion_tokens":4096,
//!  "reasoning_effort":"medium",
//!  "tools":[{"type":"function","function":{"name":"bash",
//!            "description":"Run a shell command","parameters":{"type":"object"}}}]}
//! ```
//!
//! and the reply streams as `chat.completion.chunk` events:
//!
//! ```text
//! data: {"choices":[{"delta":{"role":"assistant","content":"Hel"}}]}
//! data: {"choices":[{"delta":{"reasoning_content":"check the tests"}}]}
//! data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
//!            "function":{"name":"bash","arguments":"{\"command\":"}}]}}]}
//! data: {"choices":[{"delta":{"tool_calls":[{"index":0,
//!            "function":{"arguments":"\"ls\""}}]}}]}
//! data: {"choices":[{"delta":{"content":"lo"},"finish_reason":"tool_calls"}],
//!        "usage":{"prompt_tokens":12,"completion_tokens":3,
//!                 "completion_tokens_details":{"reasoning_tokens":1},
//!                 "prompt_tokens_details":{"cached_tokens":8}}}
//! data: [DONE]
//! ```
//!
//! Usage arrives either on the final chunk or on a chunk of its own with no
//! `choices`, which `include_usage` asks for. A tool call streams as pieces on
//! `choices[0].delta.tool_calls[]`: its id and its name ride on the first piece
//! of the call and its arguments across all of them, so the pieces are gathered
//! by `index` into [`Reply::tool_calls`] — under The Pair's own name for the
//! tool, never the gateway's.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

use super::tokens::cap_output;
use super::{
    arguments_valid, emit, our_tool_name, truncate, wire_tool_name, CancelResult, ClientConfig,
    Delta, ErrorKind, Failure, Prepared, Reply, StreamParser, ToolCall, Usage,
};
use crate::accounts::types::normalize_base_url;

/// The fields a provider may use for streamed reasoning text, in the order they
/// are tried.
pub(super) const REASONING_KEYS: [&str; 3] =
    ["reasoning_content", "reasoning_details", "reasoning"];

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    messages: &'a [Value],
    stream: bool,
    stream_options: StreamOptions,
    prompt_cache_key: &'a str,
    // Left out entirely when the turn has no tools: an empty array declares
    // nothing, and some gateways refuse a request that carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<WireTool<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// One tool as the gateway takes it: a function whose name is the gateway's own
/// where the account renames ours.
#[derive(Serialize)]
struct WireTool<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    function: WireFunction<'a>,
}

#[derive(Serialize)]
struct WireFunction<'a> {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    parameters: Value,
}

pub(super) fn prepare(
    config: &ClientConfig,
    messages: &[Value],
    tools: &[Value],
    cache_key: &str,
) -> Result<Prepared, Failure> {
    // A copy, so the stored history stays byte-for-byte as it was: a call some
    // provider once streamed torn is sent back as the empty object, since an API
    // that validates its calls would otherwise turn every request away.
    let mut messages = messages.to_vec();
    drop_empty_assistant_parts(&mut messages);
    repair_call_arguments(&mut messages);
    let max_output = config.max_output_tokens.map(cap_output);
    // The reasoning models take `max_completion_tokens`; every other model,
    // including most OpenAI-compatible gateways, still takes `max_tokens`.
    let completion_style = config.model.starts_with("gpt-5")
        || config
            .model
            .strip_prefix('o')
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    let request = Request {
        model: &config.model,
        messages: &messages,
        stream: true,
        stream_options: StreamOptions {
            include_usage: true,
        },
        prompt_cache_key: cache_key,
        tools: wire_tools(config, tools),
        max_tokens: max_output.filter(|_| !completion_style),
        max_completion_tokens: max_output.filter(|_| completion_style),
        reasoning_effort: config.reasoning_effort.as_deref(),
    };
    Ok(Prepared {
        url: format!("{}/chat/completions", normalize_base_url(&config.base_url)),
        body: serde_json::to_vec(&request)
            .map_err(|error| Failure::fatal(format!("could not serialize the request: {error}")))?,
        headers: vec![
            ("authorization".into(), format!("Bearer {}", config.api_key)),
            ("content-type".into(), "application/json".into()),
        ],
    })
}

/// An assistant turn stored with `"tool_calls": []` loses the key on the copy
/// being sent, and one left with neither text nor calls is not sent at all: the
/// chat endpoint rejects both, and a history stored before the harness stopped
/// writing them would otherwise fail every later request the same way.
fn drop_empty_assistant_parts(messages: &mut Vec<Value>) {
    for message in messages.iter_mut() {
        if message["role"] != "assistant" {
            continue;
        }
        let no_calls = message
            .get("tool_calls")
            .is_some_and(|calls| calls.as_array().map_or(true, Vec::is_empty));
        if no_calls {
            if let Some(fields) = message.as_object_mut() {
                fields.remove("tool_calls");
            }
        }
    }
    messages.retain(|message| {
        message["role"] != "assistant"
            || message.get("tool_calls").is_some()
            || !super::history::content_text(message).trim().is_empty()
    });
}

/// Stored arguments that aren't valid JSON become `{}` on the copy being sent: an
/// API that validates its calls would reject the whole request, and a call some
/// provider once streamed torn would then fail every request the same way.
fn repair_call_arguments(messages: &mut [Value]) {
    for call in messages
        .iter_mut()
        .filter_map(|message| message.get_mut("tool_calls"))
        .filter_map(Value::as_array_mut)
        .flatten()
    {
        let Some(function) = call.get_mut("function").and_then(Value::as_object_mut) else {
            continue;
        };
        let torn = function
            .get("arguments")
            .and_then(Value::as_str)
            .is_some_and(|arguments| !arguments_valid(arguments));
        if torn {
            function.insert("arguments".into(), json!("{}"));
        }
    }
}

/// The tools in the wire form, or nothing when the turn has none.
fn wire_tools<'a>(config: &ClientConfig, tools: &'a [Value]) -> Option<Vec<WireTool<'a>>> {
    if tools.is_empty() {
        return None;
    }
    Some(
        tools
            .iter()
            .map(|tool| {
                // The tool's spec, whether it arrived wrapped in a function
                // object or bare.
                let spec = tool.get("function").unwrap_or(tool);
                WireTool {
                    kind: "function",
                    function: WireFunction {
                        name: wire_tool_name(config, spec["name"].as_str().unwrap_or_default()),
                        description: spec["description"].as_str(),
                        parameters: parameters_of(spec),
                    },
                }
            })
            .collect(),
    )
}

/// The tool's JSON Schema, which the API refuses a call without: a tool that
/// names none takes an empty object's.
fn parameters_of(spec: &Value) -> Value {
    match spec.get("parameters") {
        Some(parameters) if parameters.is_object() => parameters.clone(),
        _ => json!({"type": "object", "properties": {}}),
    }
}

/// The most streamed tool calls one reply keeps, so a provider that floods or
/// replays indices cannot grow it without bound.
const MAX_STREAMED_CALLS: usize = 128;

/// One tool call being read: its id and its name ride on the first piece of the
/// call, and its arguments arrive across every piece.
#[derive(Default)]
struct Pending {
    id: String,
    name: String,
    arguments: String,
}

/// Reads one Chat Completions stream. Chat Completions has no session of its
/// own, so [`Reply::session_id`] stays empty for this protocol.
pub(super) struct Parser {
    /// Whether the stream said `[DONE]`.
    done: bool,
    /// The account's tool-name overrides, so a call that came back under the
    /// gateway's name is read back as The Pair's own.
    config: ClientConfig,
    /// The tool calls read so far, by the index each one streams under.
    calls: BTreeMap<usize, Pending>,
}

impl Parser {
    pub(super) fn new(config: &ClientConfig) -> Self {
        Self {
            done: false,
            config: config.clone(),
            calls: BTreeMap::new(),
        }
    }
}

impl StreamParser for Parser {
    fn event(
        &mut self,
        event: &Value,
        reply: &mut Reply,
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<(), Failure> {
        if let Some(error) = event.get("error").filter(|e| !e.is_null()) {
            let code = error.get("code").and_then(Value::as_u64).map(|c| c as u16);
            return Err(Failure::from_status(
                code,
                format!(
                    "provider error mid-stream: {}",
                    truncate(&error.to_string(), 2000)
                ),
            ));
        }
        // Usage may arrive on its own chunk, before or after the text ends.
        if let Some(usage) = event.get("usage").filter(|u| u.is_object()) {
            reply.usage = parse_usage(usage);
        }
        let Some(choice) = event.get("choices").and_then(|c| c.get(0)) else {
            return Ok(());
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            reply.finish_reason = Some(reason.to_string());
        }
        let Some(delta) = choice.get("delta") else {
            return Ok(());
        };
        if delta
            .get("refusal")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
        {
            return Err(Failure::fatal("provider refused the request".into()));
        }
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            reply.text.push_str(text);
            emit(on_delta, Delta::Text(text))?;
        }
        for key in REASONING_KEYS {
            if let Some(text) = delta.get(key).and_then(Value::as_str) {
                reply.reasoning.push_str(text);
                emit(on_delta, Delta::Reasoning(text))?;
                break;
            }
        }
        if let Some(pieces) = delta.get("tool_calls").and_then(Value::as_array) {
            for piece in pieces {
                let index = piece["index"].as_u64().unwrap_or_default() as usize;
                // A flood of calls stops at the cap: an index the stream never
                // named before is dropped rather than growing the reply.
                if !self.calls.contains_key(&index) && self.calls.len() >= MAX_STREAMED_CALLS {
                    continue;
                }
                let pending = self.calls.entry(index).or_default();
                // The id and the name ride on the first piece; a later piece
                // that repeats them names the same call again.
                if let Some(id) = piece["id"].as_str().filter(|id| !id.is_empty()) {
                    if pending.id.is_empty() {
                        pending.id = id.to_string();
                    }
                }
                let function = &piece["function"];
                if let Some(name) = function["name"].as_str().filter(|name| !name.is_empty()) {
                    if pending.name.is_empty() {
                        pending.name = our_tool_name(&self.config, name);
                    }
                }
                // The arguments are raw JSON text, valid or not: what the model
                // sent is what the tool layer reads, and [`Parser::finish`] repairs
                // a call the stream tore.
                let arguments = function["arguments"].as_str().unwrap_or_default();
                pending.arguments.push_str(arguments);
                emit(
                    on_delta,
                    Delta::Tool {
                        index,
                        id: &pending.id,
                        name: &pending.name,
                        arguments,
                    },
                )?;
            }
        }
        Ok(())
    }

    fn done(&mut self) {
        self.done = true;
    }

    fn finish(&mut self, reply: &mut Reply) -> Result<(), Failure> {
        if !self.done && reply.finish_reason.is_none() {
            return Err(Failure::of(
                ErrorKind::BadResponse,
                None,
                true,
                "OpenAI stream ended without finish_reason or [DONE]".into(),
            ));
        }
        // Gathered in index order, which is the order the model called them in.
        reply.tool_calls = self
            .calls
            .values()
            .map(|pending| ToolCall {
                id: pending.id.clone(),
                name: pending.name.clone(),
                arguments: pending.arguments.clone(),
            })
            .collect();
        // A call whose name never arrived cannot be run, and an API that validates
        // its calls turns the whole request away for a `function.name` that is
        // empty, on every turn after it: only a named call is kept.
        let streamed = reply.tool_calls.len();
        reply.tool_calls.retain(|call| !call.name.is_empty());
        let nameless = streamed - reply.tool_calls.len();
        if nameless > 0
            && reply.tool_calls.is_empty()
            && reply.text.trim().is_empty()
            && reply.reasoning.is_empty()
        {
            return Err(Failure::of(
                ErrorKind::BadResponse,
                None,
                true,
                format!("provider streamed {nameless} tool call(s) without names"),
            ));
        }
        reply.repair_call_arguments();
        Ok(())
    }
}

/// Token counts from a usage object: the reasoning models report cached prompt
/// tokens and reasoning tokens in nested details objects, and some gateways use
/// flatter names for the same numbers.
fn parse_usage(usage: &Value) -> Usage {
    let num = |v: Option<&Value>| v.and_then(Value::as_u64).unwrap_or(0);
    let cached = num(usage.pointer("/prompt_tokens_details/cached_tokens"))
        .max(num(usage.get("prompt_cache_hit_tokens")))
        .max(num(usage.get("cached_tokens")));
    Usage {
        prompt_tokens: num(usage.get("prompt_tokens")),
        completion_tokens: num(usage.get("completion_tokens")),
        cached_tokens: cached,
        reasoning_tokens: num(usage.pointer("/completion_tokens_details/reasoning_tokens"))
            .max(num(usage.get("reasoning_tokens"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::types::Protocol;
    use crate::llm::Client;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn config(model: &str, max_output: Option<u64>) -> ClientConfig {
        ClientConfig {
            protocol: Protocol::OpenAiChat,
            base_url: "https://api.openai.com/v1".into(),
            api_key: "sk-test".into(),
            headers: Vec::new(),
            tool_names: None,
            model: model.into(),
            max_output_tokens: max_output,
            reasoning_effort: None,
            timeout: Duration::from_secs(30),
        }
    }

    fn user() -> Vec<Value> {
        vec![json!({"role": "user", "content": "hi"})]
    }

    fn body_of(prepared: &Prepared) -> Value {
        serde_json::from_slice(&prepared.body).unwrap()
    }

    fn header_of<'a>(prepared: &'a Prepared, name: &str) -> Option<&'a String> {
        prepared
            .headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    #[test]
    fn the_request_carries_the_stream_and_cache_key() {
        let prepared = prepare(&config("gpt-4o", None), &user(), &[], "pair-1-mentor").unwrap();
        assert_eq!(prepared.url, "https://api.openai.com/v1/chat/completions");
        assert_eq!(
            header_of(&prepared, "authorization").unwrap(),
            "Bearer sk-test"
        );
        assert_eq!(
            header_of(&prepared, "content-type").unwrap(),
            "application/json"
        );
        let body = body_of(&prepared);
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["prompt_cache_key"], "pair-1-mentor");
        assert_eq!(body["messages"], serde_json::json!(user()));
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn the_request_carries_the_tools_it_was_given() {
        let prepared = prepare(&config("gpt-4o", None), &user(), &tools(), "k").unwrap();
        let body = body_of(&prepared);
        let sent = body["tools"].as_array().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0]["type"], "function");
        assert_eq!(sent[0]["function"]["name"], "bash");
        assert_eq!(sent[0]["function"]["description"], "Run a shell command");
        assert_eq!(
            sent[0]["function"]["parameters"],
            json!({"type": "object", "properties": {"command": {"type": "string"}},
                   "required": ["command"]})
        );
        assert_eq!(sent[1]["function"]["name"], "read");
    }

    #[test]
    fn a_stored_call_the_stream_tore_is_repaired_on_the_way_out() {
        let history = vec![
            json!({"role": "user", "content": "list the files"}),
            json!({"role": "assistant", "content": "on it", "tool_calls": [
                {"id": "call_1", "type": "function", "function": {
                    "name": "bash", "arguments": "{\"command\":"}}]}),
            json!({"role": "tool", "tool_call_id": "call_1", "content": "a.rs"}),
            json!({"role": "user", "content": "and this"}),
        ];
        let prepared = prepare(&config("gpt-4o", None), &history, &[], "k").unwrap();
        let body = body_of(&prepared);
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["arguments"], "{}",
            "a torn call goes back as the empty object, not as it was stored"
        );
        assert_eq!(
            history[1]["tool_calls"][0]["function"]["arguments"], "{\"command\":",
            "the stored history itself is left alone"
        );
    }

    #[test]
    fn an_empty_stored_tool_call_list_is_not_sent_back() {
        let history = vec![
            json!({"role": "user", "content": "plan it"}),
            json!({"role": "assistant", "content": "the plan", "tool_calls": []}),
            json!({"role": "user", "content": "review it"}),
            json!({"role": "assistant", "tool_calls": []}),
            json!({"role": "user", "content": "again"}),
        ];
        let prepared = prepare(&config("gpt-4o", None), &history, &[], "k").unwrap();
        let sent = body_of(&prepared)["messages"].as_array().unwrap().clone();
        assert_eq!(sent.len(), 4, "the turn with nothing in it is left out");
        assert_eq!(sent[1]["content"], "the plan");
        assert!(sent[1].get("tool_calls").is_none(), "{}", sent[1]);
        assert!(sent
            .iter()
            .all(|message| message.get("tool_calls").is_none()));
        assert_eq!(
            history[1]["tool_calls"],
            json!([]),
            "the stored copy is left alone"
        );
    }

    #[test]
    fn an_empty_tool_list_leaves_the_tools_out_of_the_request() {
        let body = body_of(&prepare(&config("gpt-4o", None), &user(), &[], "k").unwrap());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn a_tool_without_a_schema_or_description_still_sends() {
        let bare = vec![json!({"type": "function", "function": {"name": "bash"}})];
        let body = body_of(&prepare(&config("gpt-4o", None), &user(), &bare, "k").unwrap());
        let function = &body["tools"][0]["function"];
        assert_eq!(function["name"], "bash");
        assert!(function.get("description").is_none());
        assert_eq!(
            function["parameters"],
            json!({"type": "object", "properties": {}})
        );
    }

    #[test]
    fn a_renamed_tool_is_sent_under_the_gateways_name() {
        let mut renamed = config("gpt-4o", None);
        renamed.tool_names = Some(BTreeMap::from([(
            "bash".to_string(),
            "run_command".to_string(),
        )]));
        let body = body_of(&prepare(&renamed, &user(), &tools(), "k").unwrap());
        assert_eq!(body["tools"][0]["function"]["name"], "run_command");
        assert_eq!(body["tools"][1]["function"]["name"], "read");
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_is_trimmed() {
        let mut with_slash = config("gpt-4o", None);
        with_slash.base_url = "https://gateway.test/v1/".into();
        let prepared = prepare(&with_slash, &user(), &[], "k").unwrap();
        assert_eq!(prepared.url, "https://gateway.test/v1/chat/completions");
    }

    #[test]
    fn the_output_cap_uses_max_tokens_or_max_completion_tokens() {
        let plain = prepare(&config("gpt-4o", Some(4_096)), &user(), &[], "k").unwrap();
        let plain = body_of(&plain);
        assert_eq!(plain["max_tokens"], 4_096);
        assert!(plain.get("max_completion_tokens").is_none());

        for model in ["gpt-5", "gpt-5-mini", "o3", "o4-mini"] {
            let body = body_of(&prepare(&config(model, Some(4_096)), &user(), &[], "k").unwrap());
            assert_eq!(body["max_completion_tokens"], 4_096, "{model}");
            assert!(body.get("max_tokens").is_none(), "{model}");
        }
        // A model that merely starts with those letters is not a reasoning model.
        let prepared = prepare(&config("openrouter-auto", Some(4_096)), &user(), &[], "k").unwrap();
        let body = body_of(&prepared);
        assert_eq!(body["max_tokens"], 4_096);
    }

    #[test]
    fn the_reasoning_effort_is_passed_through_when_the_route_names_one() {
        let mut effort = config("gpt-5", None);
        effort.reasoning_effort = Some("high".into());
        let body = body_of(&prepare(&effort, &user(), &[], "k").unwrap());
        assert_eq!(body["reasoning_effort"], "high");
    }

    fn parser() -> Parser {
        Parser::new(&config("gpt-4o", None))
    }

    /// One streamed piece, owned: a delta's borrows don't outlive its chunk.
    #[derive(Debug, PartialEq)]
    enum Piece {
        Text(String),
        Reasoning(String),
        Tool {
            index: usize,
            id: String,
            name: String,
            arguments: String,
        },
    }

    /// A drained stream: the reply it built and every delta it emitted.
    #[derive(Debug)]
    struct Drained {
        reply: Reply,
        pieces: Vec<Piece>,
    }

    /// Drains `events` through a parser, collecting the reply and the deltas.
    fn drain(events: &[Value], done: bool) -> Result<Drained, Failure> {
        let mut parser = parser();
        let mut reply = Reply::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut pieces = Vec::new();
        for event in events {
            let mut on_delta = |delta: Delta| {
                match delta {
                    Delta::Text(chunk) => {
                        text.push_str(chunk);
                        pieces.push(Piece::Text(chunk.to_string()));
                    }
                    Delta::Reasoning(chunk) => {
                        reasoning.push_str(chunk);
                        pieces.push(Piece::Reasoning(chunk.to_string()));
                    }
                    Delta::Tool {
                        index,
                        id,
                        name,
                        arguments,
                    } => pieces.push(Piece::Tool {
                        index,
                        id: id.to_string(),
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    }),
                    Delta::Retry { .. } => {}
                    Delta::Discard => {}
                }
                CancelResult::Continue
            };
            parser.event(event, &mut reply, &mut on_delta)?;
        }
        assert_eq!(reply.text, text);
        assert_eq!(reply.reasoning, reasoning);
        if done {
            parser.done();
        }
        parser.finish(&mut reply)?;
        Ok(Drained { reply, pieces })
    }

    /// The tools a turn sends, in the stored OpenAI form.
    fn tools() -> Vec<Value> {
        vec![
            json!({"type": "function", "function": {
                "name": "bash",
                "description": "Run a shell command",
                "parameters": {"type": "object",
                               "properties": {"command": {"type": "string"}},
                               "required": ["command"]}}}),
            json!({"type": "function", "function": {
                "name": "read",
                "description": "Read a file",
                "parameters": {"type": "object",
                               "properties": {"path": {"type": "string"}}}}}),
        ]
    }

    #[test]
    fn a_chunk_stream_collects_text_reasoning_and_usage() {
        let drained = drain(
            &[
                json!({"choices":[{"index":0,"delta":{"role":"assistant"}}]}),
                json!({"choices":[{"index":0,"delta":{"content":"Hel"}}]}),
                json!({"choices":[{"index":0,"delta":{"reasoning_content":"check the tests"}}]}),
                json!({"choices":[{"index":0,"delta":{"content":"lo"},"finish_reason":"stop"}],
                       "usage":{"prompt_tokens":12,"completion_tokens":3,
                                "completion_tokens_details":{"reasoning_tokens":1},
                                "prompt_tokens_details":{"cached_tokens":8}}}),
            ],
            true,
        )
        .unwrap();
        let reply = &drained.reply;
        assert_eq!(reply.text, "Hello");
        assert_eq!(reply.reasoning, "check the tests");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(reply.session_id, None);
        assert!(
            reply.tool_calls.is_empty(),
            "a reply with no calls has none"
        );
        let usage = reply.usage;
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 3);
        assert_eq!(usage.reasoning_tokens, 1);
        assert_eq!(usage.cached_tokens, 8);
    }

    #[test]
    fn a_usage_chunk_of_its_own_fills_the_usage() {
        let drained = drain(
            &[
                json!({"choices":[{"index":0,"delta":{"content":"hi"}}]}),
                json!({"usage":{"prompt_tokens":9,"completion_tokens":2,
                                "prompt_cache_hit_tokens":4}}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
            ],
            true,
        )
        .unwrap();
        let reply = &drained.reply;
        assert_eq!(reply.text, "hi");
        assert_eq!(reply.usage.prompt_tokens, 9);
        assert_eq!(reply.usage.completion_tokens, 2);
        assert_eq!(reply.usage.cached_tokens, 4);
    }

    #[test]
    fn a_flat_reasoning_token_count_is_read_too() {
        let drained = drain(
            &[
                json!({"choices":[{"delta":{"content":"hi"}}]}),
                json!({"usage":{"prompt_tokens":9,"completion_tokens":4,"reasoning_tokens":2}}),
                json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
            ],
            true,
        )
        .unwrap();
        assert_eq!(drained.reply.usage.reasoning_tokens, 2);
    }

    #[test]
    fn every_reasoning_field_the_providers_use_is_read() {
        for key in REASONING_KEYS {
            let drained = drain(
                &[
                    json!({"choices":[{"delta":{key: "why"}}]}),
                    json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}),
                ],
                true,
            )
            .unwrap();
            assert_eq!(drained.reply.reasoning, "why", "{key}");
        }
    }

    #[test]
    fn a_repeated_id_or_name_on_one_index_keeps_the_first() {
        let drained = drain(
            &[
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
                    "function":{"name":"bash","arguments":"{\"a\":1}"}}]}}]}),
                // The same call said again under another id and name: the first
                // naming of it stands.
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_9",
                    "function":{"name":"read"}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            true,
        )
        .unwrap();
        assert_eq!(
            drained.reply.tool_calls,
            vec![ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: r#"{"a":1}"#.into(),
            }]
        );
    }

    #[test]
    fn arguments_that_are_not_json_are_repaired_so_they_replay() {
        let drained = drain(
            &[
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
                    "function":{"name":"bash","arguments":"{\"command\":"}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,
                    "function":{"arguments":" \"ls\""}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            true,
        )
        .unwrap();
        assert_eq!(
            drained.reply.tool_calls[0].arguments, "{}",
            "a torn call is repaired, so it replays instead of failing every request"
        );
    }

    #[test]
    fn a_call_whose_name_never_arrived_is_dropped() {
        let drained = drain(
            &[
                json!({"choices":[{"delta":{"content":"listing now"}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
                    "function":{"arguments":"{}"}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            true,
        )
        .unwrap();
        assert!(
            drained.reply.tool_calls.is_empty(),
            "{:?}",
            drained.reply.tool_calls
        );
        assert_eq!(drained.reply.text, "listing now");

        // A stream of nothing but nameless calls has nothing to run and nothing
        // to show, so it is retried rather than committed as an empty turn.
        let error = drain(
            &[
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
                    "function":{"arguments":"{}"}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            true,
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(error.retryable);
        assert!(error.message.contains("without names"), "{}", error.message);
    }

    #[test]
    fn a_flood_of_streamed_calls_stops_at_the_cap() {
        let mut events: Vec<Value> = (0..MAX_STREAMED_CALLS + 8)
            .map(|index| {
                json!({"choices":[{"delta":{"tool_calls":[{"index":index,
                    "id":format!("call_{index}"),
                    "function":{"name":"bash","arguments":"{}"}}]}}]})
            })
            .collect();
        events.push(json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}));
        let drained = drain(&events, true).unwrap();
        assert_eq!(drained.reply.tool_calls.len(), MAX_STREAMED_CALLS);
    }

    #[test]
    fn text_and_a_tool_call_survive_in_one_reply() {
        let drained = drain(
            &[
                json!({"choices":[{"delta":{"content":"Running it."}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
                    "function":{"name":"bash","arguments":"{\"command\":\"ls\"}"}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}],
                       "usage":{"prompt_tokens":7,"completion_tokens":4}}),
            ],
            true,
        )
        .unwrap();
        assert_eq!(drained.reply.text, "Running it.");
        assert_eq!(drained.reply.usage.prompt_tokens, 7);
        assert_eq!(
            drained.reply.tool_calls,
            vec![ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }]
        );
    }

    #[test]
    fn a_refusal_is_fatal() {
        let error = drain(
            &[json!({"choices":[{"delta":{"refusal":"I can't help with that"}}]})],
            true,
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Other);
        assert!(!error.retryable);
        assert!(error.message.contains("refused"));
    }

    #[test]
    fn a_mid_stream_error_names_its_status() {
        let event = json!({"error": {"code": 429, "message": "slow down"}});
        let error = drain(&[event], true).unwrap_err();
        assert_eq!(error.status, Some(429));
        assert_eq!(error.kind, ErrorKind::RateLimited);
        assert!(error.retryable);
        assert!(error.message.contains("slow down"));
    }

    #[test]
    fn a_stream_that_ends_without_a_reason_or_done_is_retried() {
        let error = drain(&[json!({"choices":[{"delta":{"content":"hi"}}]})], false).unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(error.retryable);
        assert!(error.message.contains("finish_reason"));
    }

    #[test]
    fn a_finish_reason_alone_ends_the_stream() {
        let drained = drain(
            &[json!({"choices":[{"delta":{"content":"hi"},"finish_reason":"length"}]})],
            false,
        )
        .unwrap();
        assert_eq!(drained.reply.finish_reason.as_deref(), Some("length"));
    }

    #[test]
    fn a_cancelled_delta_stops_the_stream() {
        let mut parser = parser();
        let mut reply = Reply::default();
        let mut on_delta = |_: Delta| CancelResult::Cancel;
        let error = parser
            .event(
                &json!({"choices":[{"delta":{"content":"hi"}}]}),
                &mut reply,
                &mut on_delta,
            )
            .unwrap_err();
        assert!(error.cancelled);
        assert_eq!(reply.text, "hi");
    }

    #[test]
    fn streamed_tool_calls_are_gathered_by_index() {
        let drained = drain(
            &[
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
                    "type":"function",
                    "function":{"name":"bash","arguments":r#"{"command":"#}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call_2",
                    "type":"function",
                    "function":{"name":"read","arguments":r#"{"path":"#}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,
                    "function":{"arguments":r#""ls"}"#}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":1,
                    "function":{"arguments":r#""a.rs"}"#}}]}}]}),
                json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            true,
        )
        .unwrap();
        assert_eq!(
            drained.reply.tool_calls,
            vec![
                ToolCall {
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: r#"{"command":"ls"}"#.into(),
                },
                ToolCall {
                    id: "call_2".into(),
                    name: "read".into(),
                    arguments: r#"{"path":"a.rs"}"#.into(),
                },
            ],
            "the calls come back whole, in the order the model made them"
        );
        // One delta per piece, each carrying that piece's own arguments and the
        // id and name its call started with.
        assert_eq!(
            drained.pieces,
            vec![
                Piece::Tool {
                    index: 0,
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: r#"{"command":"#.into(),
                },
                Piece::Tool {
                    index: 1,
                    id: "call_2".into(),
                    name: "read".into(),
                    arguments: r#"{"path":"#.into(),
                },
                Piece::Tool {
                    index: 0,
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: r#""ls"}"#.into(),
                },
                Piece::Tool {
                    index: 1,
                    id: "call_2".into(),
                    name: "read".into(),
                    arguments: r#""a.rs"}"#.into(),
                },
            ]
        );
    }

    // -----------------------------------------------------------------------
    // Wire tests: a local server feeds real SSE through the request layer.
    // -----------------------------------------------------------------------

    /// One request the test server answered.
    #[derive(Debug)]
    struct Asked {
        body: String,
    }

    /// A local HTTP server: answers each connection with the next canned reply
    /// and closes, recording what each request carried and when it arrived.
    async fn serve(replies: Vec<String>) -> (String, Arc<Mutex<Vec<Asked>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let asked: Arc<Mutex<Vec<Asked>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&asked);
        tokio::spawn(async move {
            let mut replies = replies.into_iter();
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4_096];
                loop {
                    let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                        }
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buffer[..end]).to_ascii_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if buffer.len() < end + 4 + length {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                        }
                        continue;
                    }
                    recorder.lock().unwrap().push(Asked {
                        body: String::from_utf8_lossy(&buffer[end + 4..end + 4 + length])
                            .to_string(),
                    });
                    break;
                }
                if let Some(reply) = replies.next() {
                    let _ = socket.write_all(reply.as_bytes()).await;
                }
                let _ = socket.shutdown().await;
            }
        });
        (url, asked)
    }

    /// A canned SSE reply: the events, then the stream's end marker.
    fn sse(events: &[Value]) -> String {
        let mut body = String::new();
        for event in events {
            body.push_str(&format!("data: {event}\n\n"));
        }
        body.push_str("data: [DONE]\n\n");
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
             connection: close\r\n\r\n{body}"
        )
    }

    fn bodies(asked: &Arc<Mutex<Vec<Asked>>>) -> Vec<String> {
        asked
            .lock()
            .unwrap()
            .iter()
            .map(|ask| ask.body.clone())
            .collect()
    }

    /// A client pointed at the local test server.
    fn client(base_url: &str) -> Client {
        let mut local = config("gpt-4o", None);
        local.base_url = base_url.into();
        Client::new(local).unwrap()
    }

    #[tokio::test]
    async fn a_streamed_tool_call_comes_back_whole() {
        let events = [
            json!({"choices":[{"delta":{"role":"assistant","content":"Running it."}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
                "type":"function",
                "function":{"name":"bash","arguments":r#"{"command":"#}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,
                "function":{"arguments":r#""ls"}"#}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}],
                   "usage":{"prompt_tokens":7,"completion_tokens":4}}),
        ];
        let (url, asked) = serve(vec![sse(&events)]).await;
        let mut pieces: Vec<(usize, String, String, String)> = Vec::new();
        let reply = client(&url)
            .complete(&user(), &tools(), "pair-1-mentor", &mut |delta| {
                if let Delta::Tool {
                    index,
                    id,
                    name,
                    arguments,
                } = delta
                {
                    pieces.push((
                        index,
                        id.to_string(),
                        name.to_string(),
                        arguments.to_string(),
                    ));
                }
                CancelResult::Continue
            })
            .await
            .unwrap();

        assert_eq!(
            reply.text, "Running it.",
            "the text and the call both survive"
        );
        assert_eq!(reply.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }]
        );
        assert_eq!(
            pieces,
            vec![
                (0, "call_1".into(), "bash".into(), r#"{"command":"#.into()),
                (0, "call_1".into(), "bash".into(), r#""ls"}"#.into()),
            ],
            "each piece arrives as it streams"
        );
        let body: Value = serde_json::from_str(&bodies(&asked)[0]).unwrap();
        assert_eq!(body["prompt_cache_key"], "pair-1-mentor");
        assert_eq!(body["tools"][0]["function"]["name"], "bash");
        assert_eq!(body["tools"][1]["function"]["name"], "read");
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["required"],
            json!(["command"])
        );
    }

    #[tokio::test]
    async fn a_renamed_tool_travels_under_the_wire_name_both_ways() {
        let events = [
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1",
                "function":{"name":"run_command",
                             "arguments":r#"{"command":"ls"}"#}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        ];
        let (url, asked) = serve(vec![sse(&events)]).await;
        let mut renamed = config("gpt-4o", None);
        renamed.base_url = url;
        renamed.tool_names = Some(BTreeMap::from([(
            "bash".to_string(),
            "run_command".to_string(),
        )]));
        let reply = Client::new(renamed)
            .unwrap()
            .complete(&user(), &tools(), "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();

        let body: Value = serde_json::from_str(&bodies(&asked)[0]).unwrap();
        assert_eq!(
            body["tools"][0]["function"]["name"], "run_command",
            "the gateway is asked for the name it knows"
        );
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }],
            "the call comes back under The Pair's own name"
        );
    }
}
