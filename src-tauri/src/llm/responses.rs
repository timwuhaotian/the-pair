//! OpenAI Responses: one stateless streamed turn on `POST {base}/responses`.
//! The same wire format is spoken by Codex and by a ChatGPT plan, which only
//! differ in the credential.
//!
//! The request carries `store: false`, so the server keeps nothing between
//! turns and each one resends the whole conversation as `input` items. The
//! system prompt travels as `instructions`, which this API takes outside
//! `input`:
//!
//! ```text
//! → {"model":"gpt-5","stream":true,"store":false,"instructions":"You are …",
//! →  "input":[{"role":"user","content":"hi"}],
//! →  "prompt_cache_key":"pair-1-mentor","max_output_tokens":4096,
//! →  "reasoning":{"effort":"high","summary":"auto"}}
//! ← data: {"type":"response.reasoning_summary_text.delta","delta":"check"}
//! ← data: {"type":"response.output_text.delta","delta":"Hello"}
//! ← data: {"type":"response.completed","response":{"id":"resp_1",
//! →  "usage":{"input_tokens":12,"output_tokens":3}}}
//! ← data: [DONE]
//! ```
//!
//! The stream ends on `[DONE]`, and the terminal `response.completed` event
//! carries both the usage and the response's id, which this protocol keeps as
//! the session id.
//!
//! Tools are declared in the flat shape this API takes — a function tool names
//! its own `name` and `parameters`, with no `function` wrapper — and come back
//! as `function_call` output items, streamed in pieces:
//!
//! ```text
//! →  "tools":[{"type":"function","name":"bash","description":"…",
//! →            "parameters":{"type":"object","properties":{…}}}]
//! ← data: {"type":"response.output_item.added","output_index":0,
//! →        "item":{"id":"fc_1","type":"function_call","call_id":"call_1",
//! →                 "name":"bash","arguments":""}}
//! ← data: {"type":"response.function_call_arguments.delta","item_id":"fc_1",
//! →        "delta":"{\"command\":"}
//! ← data: {"type":"response.output_item.done","output_index":0,"item":{…}}
//! ← data: {"type":"response.completed","response":{"id":"resp_1",
//! →  "output":[{"type":"function_call","call_id":"call_1","name":"bash",
//! →             "arguments":"{\"command\":\"ls\"}"}],
//! →  "usage":{"input_tokens":12,"output_tokens":3}}}
//! ```
//!
//! A call's pieces land on `reply.tool_calls` at the position its item events
//! opened it in, and the terminal event's copy of the output only fills in what
//! those pieces never carried, so one call is never counted twice. A call is
//! matched by its `call_id`, by its own `id`, or — for a gateway that names
//! neither — by the output position it was opened at, so a call the item events
//! carried only by position still gets its arguments. The reasoning the API
//! reports arrives as summary text on `reply.reasoning`, or as plain reasoning
//! text on the servers outside OpenAI's own that send it that way.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::history::{content_text, system_text};
use super::tokens::cap_output;
use super::{
    emit, our_tool_name, truncate, wire_tool_name, CancelResult, ClientConfig, Delta, ErrorKind,
    Failure, Prepared, Reply, StreamParser, ToolCall, Usage,
};
use crate::accounts::types::normalize_base_url;

pub(super) fn prepare(
    config: &ClientConfig,
    messages: &[Value],
    tools: &[Value],
    cache_key: &str,
) -> Result<Prepared, Failure> {
    let (system, history) = system_text(messages);
    let mut body = Map::new();
    body.insert("model".into(), config.model.clone().into());
    body.insert("stream".into(), true.into());
    body.insert("store".into(), false.into());
    if !system.is_empty() {
        body.insert("instructions".into(), system.into());
    }
    body.insert("input".into(), convert_input(config, history).into());
    // A route with no tools says nothing about them, rather than declaring an
    // empty list a provider may refuse.
    if !tools.is_empty() {
        body.insert("tools".into(), tools_of(config, tools).into());
    }
    body.insert("prompt_cache_key".into(), cache_key.into());
    if let Some(max) = config.max_output_tokens {
        body.insert("max_output_tokens".into(), cap_output(max).into());
    }
    // Only a route that names an effort asks for reasoning; without one the
    // summary never arrives, and nothing on this protocol keeps the encrypted
    // content the API would send back with it.
    if let Some(effort) = &config.reasoning_effort {
        body.insert(
            "reasoning".into(),
            json!({"effort": effort, "summary": "auto"}),
        );
    }
    Ok(Prepared {
        url: format!("{}/responses", normalize_base_url(&config.base_url)),
        body: serde_json::to_vec(&body)
            .map_err(|error| Failure::fatal(format!("could not serialize the request: {error}")))?,
        headers: vec![
            ("authorization".into(), format!("Bearer {}", config.api_key)),
            ("content-type".into(), "application/json".into()),
        ],
    })
}

/// History in its stored chat form becomes `input` items: the system message
/// went out as `instructions`, so only the rest arrives here. A message with no
/// role or no text has nothing to send and is left out. The calls an assistant
/// made become `function_call` items and their results
/// `function_call_output` items, which this API keeps beside the messages
/// rather than inside them.
fn convert_input(config: &ClientConfig, history: &[Value]) -> Vec<Value> {
    let mut items = Vec::new();
    for message in history {
        match message["role"].as_str() {
            Some("assistant") => {
                let content = content_text(message);
                if !content.is_empty() {
                    items.push(json!({"role": "assistant", "content": content}));
                }
                items.extend(call_items(config, message));
            }
            Some("tool") => items.push(json!({
                "type": "function_call_output",
                "call_id": message["tool_call_id"].as_str().unwrap_or_default(),
                "output": content_text(message),
            })),
            Some(role) => {
                let content = content_text(message);
                if !content.is_empty() {
                    items.push(json!({"role": role, "content": content}));
                }
            }
            None => {}
        }
    }
    items
}

/// The calls one assistant message made, as `function_call` items. Each name
/// goes out the way the gateway spells it, and the arguments travel as the JSON
/// text they were stored as.
fn call_items(config: &ClientConfig, message: &Value) -> Vec<Value> {
    message["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|call| {
            json!({
                "type": "function_call",
                "call_id": call["id"].as_str().unwrap_or_default(),
                "name": wire_tool_name(
                    config,
                    call["function"]["name"].as_str().unwrap_or_default(),
                ),
                "arguments": call["function"]["arguments"].as_str().unwrap_or_default(),
            })
        })
        .collect()
}

/// Tool definitions as this API takes them: flat, with the name and the schema
/// on the tool itself. They arrive in the chat-completions form, nested under
/// `function`; one already in this API's shape is read as it is.
fn tools_of(config: &ClientConfig, tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let function = tool
                .get("function")
                .filter(|f| f.is_object())
                .unwrap_or(tool);
            let name = function["name"].as_str()?;
            let mut wire = Map::new();
            wire.insert("type".into(), "function".into());
            wire.insert("name".into(), wire_tool_name(config, name).into());
            if let Some(description) = function["description"].as_str() {
                wire.insert("description".into(), description.into());
            }
            if let Some(parameters) = function.get("parameters").filter(|p| p.is_object()) {
                wire.insert("parameters".into(), parameters.clone());
            }
            Some(wire.into())
        })
        .collect()
}

/// Reads one Responses stream. The stream has no end marker of its own beyond
/// `[DONE]`, so the terminal `response.completed` event is what
/// [`Parser::finish`] requires before a reply counts as whole.
pub(super) struct Parser {
    config: ClientConfig,
    /// Whether a terminal response event arrived.
    terminal: bool,
    /// Every id a call may be named by — the item's own `id` and its `call_id`
    /// — pointing at the call's position in `reply.tool_calls`.
    at: BTreeMap<String, usize>,
    /// Each output position to the call it opened, which is the only name a call
    /// that carries no id at all can be matched by.
    at_index: BTreeMap<u64, usize>,
}

impl Parser {
    pub(super) fn new(config: &ClientConfig) -> Self {
        Self {
            config: config.clone(),
            terminal: false,
            at: BTreeMap::new(),
            at_index: BTreeMap::new(),
        }
    }

    /// Where a call the stream already opened sits, by either of the ids the
    /// stream may name it with, or by the output position it was opened at.
    fn find(&self, item: &Value, output_index: u64) -> Option<usize> {
        ["call_id", "id"]
            .into_iter()
            .filter_map(|key| item[key].as_str())
            .find_map(|key| self.at.get(key).copied())
            .or_else(|| self.at_index.get(&output_index).copied())
    }

    /// Opens a tool call the stream just started, and returns the position it
    /// took. The call takes the next position in the reply, so its index is where
    /// every later piece of it lands. The caller announces it, once whatever the
    /// item already carried has been filled in.
    fn open(&mut self, item: &Value, output_index: u64, reply: &mut Reply) -> Option<usize> {
        if item["type"].as_str() != Some("function_call") {
            return None;
        }
        let call_id = item["call_id"].as_str().unwrap_or_default();
        let item_id = item["id"].as_str().unwrap_or_default();
        let opened = self.at.contains_key(call_id)
            || self.at.contains_key(item_id)
            || self.at_index.contains_key(&output_index);
        if opened {
            return None;
        }
        let name = our_tool_name(&self.config, item["name"].as_str().unwrap_or_default());
        let index = reply.tool_calls.len();
        // The arguments start empty and are filled by the deltas that stream
        // them: a gateway that puts the whole call on the item that opens it and
        // streams the deltas as well would otherwise count every piece twice.
        reply.tool_calls.push(ToolCall {
            id: call_id.to_string(),
            name,
            arguments: String::new(),
        });
        for key in [call_id, item_id] {
            if !key.is_empty() {
                self.at.insert(key.to_string(), index);
            }
        }
        self.at_index.insert(output_index, index);
        Some(index)
    }

    /// Hands the caller the call at `index`, so the console can name it once
    /// instead of on every piece of it.
    fn announce(
        reply: &Reply,
        index: usize,
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<(), Failure> {
        let call = &reply.tool_calls[index];
        emit(
            on_delta,
            Delta::Tool {
                index,
                id: &call.id,
                name: &call.name,
                arguments: &call.arguments,
            },
        )
    }
}

impl StreamParser for Parser {
    fn event(
        &mut self,
        event: &Value,
        reply: &mut Reply,
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<(), Failure> {
        // The output position every item event names, which is the only name a
        // call that carries no id at all can be matched by.
        let output_index = event["output_index"].as_u64().unwrap_or_default();
        match event["type"].as_str() {
            Some("response.refusal.delta") => {
                if event["delta"].as_str().is_some_and(|text| !text.is_empty()) {
                    return Err(Failure::fatal("provider refused the request".into()));
                }
            }
            Some("response.output_text.delta") => {
                let text = event["delta"].as_str().unwrap_or_default();
                reply.text.push_str(text);
                emit(on_delta, Delta::Text(text))?;
            }
            Some("response.output_item.added") => {
                if let Some(index) = self.open(&event["item"], output_index, reply) {
                    Self::announce(reply, index, on_delta)?;
                }
            }
            Some("response.function_call_arguments.delta") => {
                let index = event["item_id"]
                    .as_str()
                    .and_then(|id| self.at.get(id).copied())
                    .or_else(|| self.at_index.get(&output_index).copied());
                // A delta for a call the item events never opened has nowhere to
                // land; the terminal event's copy of the output carries it.
                if let Some(index) = index {
                    let arguments = event["delta"].as_str().unwrap_or_default();
                    if !arguments.is_empty() {
                        reply.tool_calls[index].arguments.push_str(arguments);
                        let call = &reply.tool_calls[index];
                        emit(
                            on_delta,
                            Delta::Tool {
                                index,
                                id: &call.id,
                                name: &call.name,
                                arguments,
                            },
                        )?;
                    }
                }
            }
            Some("response.output_item.done") => {
                if let Some(index) = self.find(&event["item"], output_index) {
                    fill(&mut reply.tool_calls[index], &self.config, &event["item"]);
                }
            }
            // A server outside OpenAI's own reports its reasoning as plain text
            // rather than as a summary, so both names are read.
            Some("response.reasoning_summary_text.delta" | "response.reasoning_text.delta") => {
                let text = event["delta"].as_str().unwrap_or_default();
                reply.reasoning.push_str(text);
                emit(on_delta, Delta::Reasoning(text))?;
            }
            Some("response.completed" | "response.incomplete") => {
                self.terminal = true;
                let response = &event["response"];
                reply.session_id = response["id"].as_str().map(str::to_string);
                reply.usage = usage_of(&response["usage"]);
                // The terminal event repeats the whole output, which is the only
                // place a call the item events never carried shows up. Each item
                // takes its own position in that output, so a call the stream
                // opened is filled rather than opened again.
                for (output_index, item) in response["output"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    if item["type"].as_str() != Some("function_call") {
                        continue;
                    }
                    let output_index = output_index as u64;
                    match self.find(item, output_index) {
                        Some(index) => fill(&mut reply.tool_calls[index], &self.config, item),
                        None => {
                            // A call that only the terminal event carries is opened
                            // here and filled from the same item, which is the only
                            // place its arguments ever arrive.
                            if let Some(index) = self.open(item, output_index, reply) {
                                fill(&mut reply.tool_calls[index], &self.config, item);
                                Self::announce(reply, index, on_delta)?;
                            }
                        }
                    }
                }
                let incomplete = event["type"] == "response.incomplete";
                let reason = response["incomplete_details"]["reason"].as_str();
                // Running out of output is a whole reply; every other reason is
                // a turn that stopped for a cause of its own.
                if incomplete && reason != Some("max_output_tokens") {
                    let message = format!(
                        "response incomplete: {}",
                        reason.unwrap_or("unknown reason")
                    );
                    return Err(if reason == Some("content_filter") {
                        Failure::fatal(message)
                    } else {
                        Failure::transient(message)
                    });
                }
                reply.finish_reason = Some(
                    if incomplete {
                        "length"
                    } else if reply.tool_calls.is_empty() {
                        "stop"
                    } else {
                        // A turn that asked for tools ended for the tools, however
                        // the server words it.
                        "tool_calls"
                    }
                    .to_string(),
                );
            }
            Some("response.failed" | "error") => {
                let error = if event["type"] == "error" {
                    event.clone()
                } else {
                    event["response"]["error"].clone()
                };
                let message = format!(
                    "provider error mid-stream: {}",
                    truncate(&error.to_string(), 2000)
                );
                let code = error["code"].as_str().unwrap_or_default();
                let kind = if code.contains("rate_limit") {
                    ErrorKind::RateLimited
                } else if code.contains("usage_limit") || code.contains("quota") {
                    ErrorKind::Quota
                } else {
                    ErrorKind::Other
                };
                let retry = code.contains("rate_limit")
                    || code.contains("server_error")
                    || code.ends_with("_unavailable")
                    || code.is_empty();
                return Err(if retry {
                    Failure::of(kind, None, true, message)
                } else {
                    Failure::of(kind, None, false, message)
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self, reply: &mut Reply) -> Result<(), Failure> {
        if self.terminal {
            reply.repair_call_arguments();
            Ok(())
        } else {
            Err(Failure::of(
                ErrorKind::BadResponse,
                None,
                true,
                "Responses stream ended without a terminal response event".into(),
            ))
        }
    }
}

/// Fills a call in from the complete item that repeats it: a gateway that
/// opens a call without naming it, or sends no argument deltas at all, still
/// reports the whole call on the item that closes it.
fn fill(call: &mut ToolCall, config: &ClientConfig, item: &Value) {
    if call.name.is_empty() {
        call.name = our_tool_name(config, item["name"].as_str().unwrap_or_default());
    }
    if call.arguments.is_empty() {
        call.arguments = item["arguments"].as_str().unwrap_or_default().to_string();
    }
}

/// Token counts from the terminal event's `usage` object: cached prompt tokens
/// and reasoning tokens arrive in nested details objects.
fn usage_of(usage: &Value) -> Usage {
    let num = |pointer: &str| usage.pointer(pointer).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        prompt_tokens: num("/input_tokens"),
        completion_tokens: num("/output_tokens"),
        cached_tokens: num("/input_tokens_details/cached_tokens"),
        reasoning_tokens: num("/output_tokens_details/reasoning_tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::types::Protocol;
    use crate::llm::Client;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn config(base_url: &str) -> ClientConfig {
        ClientConfig {
            protocol: Protocol::Responses,
            base_url: base_url.into(),
            api_key: "sk-test".into(),
            headers: Vec::new(),
            tool_names: None,
            model: "gpt-5".into(),
            max_output_tokens: None,
            reasoning_effort: None,
            timeout: Duration::from_secs(10),
        }
    }

    fn user() -> Vec<Value> {
        vec![json!({"role": "user", "content": "hi"})]
    }

    /// Two tool definitions in the chat-completions form the caller passes them
    /// in, one of which a gateway renames.
    fn tools() -> Vec<Value> {
        vec![
            json!({"type": "function", "function": {
                "name": "bash", "description": "run a command",
                "parameters": {"type": "object",
                               "properties": {"command": {"type": "string"}},
                               "required": ["command"]}}}),
            json!({"type": "function", "function": {
                "name": "read", "description": "read a file",
                "parameters": {"type": "object",
                               "properties": {"path": {"type": "string"}}}}}),
        ]
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

    /// Drains `events` through a parser, collecting what the stream carried.
    fn drain(events: &[Value]) -> Result<Reply, Failure> {
        drain_tools(events).map(|(reply, _)| reply)
    }

    /// The same drain, with every `Delta::Tool` the parser fired, so a test can
    /// check the pieces beside the reply they were assembled into.
    fn drain_tools(events: &[Value]) -> Result<(Reply, Vec<Piece>), Failure> {
        let mut parser = Parser::new(&config("https://api.openai.com/v1"));
        let mut reply = Reply::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut pieces = Vec::new();
        for event in events {
            let mut on_delta = |delta: Delta| {
                match delta {
                    Delta::Text(chunk) => text.push_str(chunk),
                    Delta::Reasoning(chunk) => reasoning.push_str(chunk),
                    Delta::Tool {
                        index,
                        id,
                        name,
                        arguments,
                    } => pieces.push(Piece {
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
        parser.finish(&mut reply)?;
        Ok((reply, pieces))
    }

    /// One `Delta::Tool` as a test saw it.
    #[derive(Debug, PartialEq)]
    struct Piece {
        index: usize,
        id: String,
        name: String,
        arguments: String,
    }

    fn completed(usage: Value) -> Value {
        json!({"type": "response.completed", "response": {"id": "resp_1", "usage": usage}})
    }

    /// The terminal event, whose `output` repeats the calls the stream carried.
    fn completed_with(output: Vec<Value>, usage: Value) -> Value {
        json!({"type": "response.completed",
               "response": {"id": "resp_1", "output": output, "usage": usage}})
    }

    /// The events one streamed call arrives as: its item opens it at
    /// `output_index`, the argument deltas fill it, and the item that closes it
    /// repeats the whole call.
    fn call_events(output_index: usize, call_id: &str, name: &str, pieces: &[&str]) -> Vec<Value> {
        let item_id = format!("fc_{output_index}");
        let mut events = vec![json!({
            "type": "response.output_item.added", "output_index": output_index,
            "item": {"id": item_id, "type": "function_call", "call_id": call_id,
                     "name": name, "arguments": ""}
        })];
        for piece in pieces {
            events.push(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": item_id, "output_index": output_index, "delta": piece
            }));
        }
        events.push(json!({
            "type": "response.output_item.done", "output_index": output_index,
            "item": {"id": item_id, "type": "function_call", "call_id": call_id,
                     "name": name, "arguments": pieces.concat()}
        }));
        events
    }

    /// One `function_call` output item as the terminal event reports it.
    fn output_call(call_id: &str, name: &str, arguments: &str) -> Value {
        json!({"type": "function_call", "call_id": call_id, "name": name,
               "arguments": arguments})
    }

    #[test]
    fn the_request_is_stateless_and_carries_the_system_as_instructions() {
        let prepared = prepare(
            &config("https://api.openai.com/v1"),
            &[
                json!({"role": "system", "content": "be brief"}),
                json!({"role": "user", "content": "hi"}),
            ],
            &[],
            "pair-1-mentor",
        )
        .unwrap();
        assert_eq!(prepared.url, "https://api.openai.com/v1/responses");
        assert_eq!(
            header_of(&prepared, "authorization").unwrap(),
            "Bearer sk-test"
        );
        assert_eq!(
            header_of(&prepared, "content-type").unwrap(),
            "application/json"
        );
        let body = body_of(&prepared);
        assert_eq!(body["model"], "gpt-5");
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["instructions"], "be brief");
        assert_eq!(body["prompt_cache_key"], "pair-1-mentor");
        assert_eq!(
            body["input"],
            json!([{"role": "user", "content": "hi"}]),
            "the system message is not an input item"
        );
        assert!(body.get("reasoning").is_none());
        assert!(body.get("max_output_tokens").is_none());
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_is_trimmed() {
        let prepared = prepare(&config("https://gateway.test/v1/"), &user(), &[], "k").unwrap();
        assert_eq!(prepared.url, "https://gateway.test/v1/responses");
    }

    #[test]
    fn the_output_cap_is_clamped_to_what_providers_accept() {
        let mut capped = config("https://api.openai.com/v1");
        capped.max_output_tokens = Some(200_000);
        let body = body_of(&prepare(&capped, &user(), &[], "k").unwrap());
        assert_eq!(body["max_output_tokens"], 131_072);

        capped.max_output_tokens = Some(4_096);
        let body = body_of(&prepare(&capped, &user(), &[], "k").unwrap());
        assert_eq!(body["max_output_tokens"], 4_096);
    }

    #[test]
    fn a_named_effort_asks_for_a_reasoning_summary() {
        let mut thinking = config("https://api.openai.com/v1");
        thinking.reasoning_effort = Some("high".into());
        let body = body_of(&prepare(&thinking, &user(), &[], "k").unwrap());
        assert_eq!(
            body["reasoning"],
            json!({"effort": "high", "summary": "auto"})
        );
    }

    #[test]
    fn history_becomes_input_items() {
        let prepared = prepare(
            &config("https://api.openai.com/v1"),
            &[
                json!({"role": "user", "content": "hi"}),
                json!({"role": "assistant", "content": "ok"}),
                json!({"role": "user", "content": "thanks"}),
            ],
            &[],
            "k",
        )
        .unwrap();
        assert_eq!(
            body_of(&prepared)["input"],
            json!([
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "ok"},
                {"role": "user", "content": "thanks"},
            ])
        );
    }

    #[test]
    fn a_message_with_no_role_or_no_text_is_left_out() {
        let prepared = prepare(
            &config("https://api.openai.com/v1"),
            &[
                json!({"content": "no role"}),
                json!({"role": "user", "content": ""}),
                json!({"role": "assistant", "content": [{"type": "text", "text": "kept"}]}),
                json!({"role": "user", "content": "hi"}),
            ],
            &[],
            "k",
        )
        .unwrap();
        assert_eq!(
            body_of(&prepared)["input"],
            json!([
                {"role": "assistant", "content": "kept"},
                {"role": "user", "content": "hi"},
            ])
        );
    }

    #[test]
    fn the_request_declares_each_tool_in_the_flat_shape() {
        let prepared =
            prepare(&config("https://api.openai.com/v1"), &user(), &tools(), "k").unwrap();
        assert_eq!(
            body_of(&prepared)["tools"],
            json!([
                {"type": "function", "name": "bash", "description": "run a command",
                 "parameters": {"type": "object",
                                "properties": {"command": {"type": "string"}},
                                "required": ["command"]}},
                {"type": "function", "name": "read", "description": "read a file",
                 "parameters": {"type": "object",
                                "properties": {"path": {"type": "string"}}}},
            ]),
            "a function tool on this API is flat, with no `function` wrapper"
        );
    }

    #[test]
    fn an_empty_tool_list_sends_no_tools_key() {
        let prepared = prepare(&config("https://api.openai.com/v1"), &user(), &[], "k").unwrap();
        assert!(body_of(&prepared).get("tools").is_none());
    }

    #[test]
    fn history_calls_and_results_become_function_call_items() {
        let mut renamed = config("https://api.openai.com/v1");
        renamed.tool_names = Some(BTreeMap::from([(
            "bash".to_string(),
            "run_command".to_string(),
        )]));
        let prepared = prepare(
            &renamed,
            &[
                json!({"role": "user", "content": "list the files"}),
                json!({"role": "assistant", "content": "on it", "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {
                        "name": "bash", "arguments": r#"{"command":"ls"}"#}}]}),
                json!({"role": "tool", "tool_call_id": "call_1", "content": "a.rs\nb.rs"}),
                json!({"role": "assistant", "tool_calls": [
                    {"id": "call_2", "type": "function", "function": {
                        "name": "bash", "arguments": r#"{"command":"pwd"}"#}}]}),
                json!({"role": "tool", "tool_call_id": "call_2", "content": "/repo"}),
            ],
            &[],
            "k",
        )
        .unwrap();
        assert_eq!(
            body_of(&prepared)["input"],
            json!([
                {"role": "user", "content": "list the files"},
                {"role": "assistant", "content": "on it"},
                {"type": "function_call", "call_id": "call_1", "name": "run_command",
                 "arguments": r#"{"command":"ls"}"#},
                {"type": "function_call_output", "call_id": "call_1",
                 "output": "a.rs\nb.rs"},
                {"type": "function_call", "call_id": "call_2", "name": "run_command",
                 "arguments": r#"{"command":"pwd"}"#},
                {"type": "function_call_output", "call_id": "call_2", "output": "/repo"},
            ]),
            "the calls sit beside the messages, named as the gateway spells them"
        );
    }

    #[test]
    fn a_stream_collects_text_reasoning_usage_and_the_response_id() {
        let reply = drain(&[
            json!({"type": "response.created", "response": {"id": "resp_1"}}),
            json!({"type": "response.in_progress", "response": {"id": "resp_1"}}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "check the tests"}),
            json!({"type": "response.output_text.delta", "delta": "Hel"}),
            json!({"type": "response.output_text.delta", "delta": "lo"}),
            completed(
                json!({"input_tokens": 12, "input_tokens_details": {"cached_tokens": 8},
                             "output_tokens": 3,
                             "output_tokens_details": {"reasoning_tokens": 1}}),
            ),
        ])
        .unwrap();
        assert_eq!(reply.text, "Hello");
        assert_eq!(reply.reasoning, "check the tests");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(reply.session_id.as_deref(), Some("resp_1"));
        let usage = reply.usage;
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 3);
        assert_eq!(usage.cached_tokens, 8);
        assert_eq!(usage.reasoning_tokens, 1);
    }

    #[test]
    fn a_streamed_tool_call_lands_on_the_reply() {
        let mut events = call_events(0, "call_1", "bash", &[r#"{"command":"#, r#""ls"}"#]);
        events.push(completed_with(
            vec![output_call("call_1", "bash", r#"{"command":"ls"}"#)],
            json!({"input_tokens": 7}),
        ));
        let (reply, pieces) = drain_tools(&events).unwrap();
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }]
        );
        assert_eq!(reply.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            pieces,
            vec![
                Piece {
                    index: 0,
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: "".into(),
                },
                Piece {
                    index: 0,
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: r#"{"command":"#.into(),
                },
                Piece {
                    index: 0,
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: r#""ls"}"#.into(),
                },
            ],
            "each piece arrives as its own delta, at the call's position"
        );
    }

    #[test]
    fn torn_argument_pieces_are_repaired_so_the_call_replays() {
        let mut events = call_events(0, "call_1", "bash", &[r#"{"command":"#, "oops"]);
        events.push(completed_with(
            vec![output_call("call_1", "bash", r#"{"command":"oops""#)],
            json!({"input_tokens": 2}),
        ));
        let (reply, pieces) = drain_tools(&events).unwrap();
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(
            reply.tool_calls[0].arguments, "{}",
            "a call the stream tore is repaired rather than stored broken"
        );
        assert_eq!(pieces.len(), 3);
    }

    #[test]
    fn a_call_the_gateway_fills_on_its_first_event_is_not_counted_twice() {
        // A gateway that puts the whole arguments on the item that opens the call
        // and streams the deltas as well: the deltas are what count.
        let mut events = vec![json!({
            "type": "response.output_item.added", "output_index": 0,
            "item": {"id": "fc_0", "type": "function_call", "call_id": "call_1",
                     "name": "bash", "arguments": r#"{"command":"ls"}"#}
        })];
        events.extend(call_events(0, "call_1", "bash", &[r#"{"command":"ls"}"#]));
        events.push(completed_with(
            vec![output_call("call_1", "bash", r#"{"command":"ls"}"#)],
            json!({"input_tokens": 3}),
        ));
        let (reply, _) = drain_tools(&events).unwrap();
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }],
            "the call lands once, not twice"
        );
    }

    #[test]
    fn a_call_with_no_id_is_keyed_by_its_output_position() {
        let mut events = vec![
            json!({"type": "response.output_item.added", "output_index": 0,
                   "item": {"type": "function_call", "name": "bash"}}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 0,
                   "delta": r#"{"command":"#}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 0,
                   "delta": r#""ls"}"#}),
            json!({"type": "response.output_item.done", "output_index": 0,
                   "item": {"type": "function_call", "name": "bash",
                            "arguments": r#"{"command":"ls"}"#}}),
        ];
        events.push(completed_with(
            vec![json!({"type": "function_call", "name": "bash",
                       "arguments": r#"{"command":"ls"}"#})],
            json!({"input_tokens": 3}),
        ));
        let (reply, pieces) = drain_tools(&events).unwrap();
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "".into(),
                name: "bash".into(),
                arguments: r#"{"command":"ls"}"#.into(),
            }],
            "a call the item events opened is filled by the terminal event, not opened again"
        );
        assert_eq!(pieces.len(), 3, "the open and the two argument pieces");
    }

    #[test]
    fn reasoning_arrives_as_plain_text_on_servers_outside_openais_own() {
        let reply = drain(&[
            json!({"type": "response.reasoning_text.delta", "delta": "reading"}),
            json!({"type": "response.output_text.delta", "delta": "ok"}),
            completed(json!({"input_tokens": 4})),
        ])
        .unwrap();
        assert_eq!(reply.reasoning, "reading");
        assert_eq!(reply.text, "ok");
    }

    #[test]
    fn text_and_a_tool_call_survive_in_the_same_reply() {
        let mut events = vec![json!({"type": "response.output_text.delta",
                                    "delta": "listing now"})];
        events.extend(call_events(1, "call_1", "bash", &[r#"{"command":"ls"}"#]));
        events.push(completed_with(
            vec![
                json!({"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "listing now"}]}),
                output_call("call_1", "bash", r#"{"command":"ls"}"#),
            ],
            json!({"input_tokens": 4}),
        ));
        let (reply, pieces) = drain_tools(&events).unwrap();
        assert_eq!(reply.text, "listing now");
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].arguments, r#"{"command":"ls"}"#);
        assert_eq!(pieces.len(), 2, "the open and the one argument piece");
        assert_eq!(
            pieces[0].index, 0,
            "the index is the call's place in the reply, not in the output"
        );
    }

    #[test]
    fn reasoning_and_calls_alternate_without_mixing() {
        let mut events = vec![
            json!({"type": "response.output_item.added", "output_index": 0,
                   "item": {"id": "rs_1", "type": "reasoning", "summary": []}}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "plan"}),
            json!({"type": "response.output_item.done", "output_index": 0,
                   "item": {"id": "rs_1", "type": "reasoning", "summary": []}}),
        ];
        events.extend(call_events(
            1,
            "call_1",
            "read",
            &[r#"{"path":"#, r#""a.rs"}"#],
        ));
        events.push(completed_with(
            vec![
                json!({"type": "reasoning", "id": "rs_1", "summary": []}),
                output_call("call_1", "read", r#"{"path":"a.rs"}"#),
            ],
            json!({"input_tokens": 6}),
        ));
        let (reply, pieces) = drain_tools(&events).unwrap();
        assert_eq!(reply.reasoning, "plan");
        assert_eq!(
            reply.tool_calls.len(),
            1,
            "the reasoning item is not a call"
        );
        assert_eq!(reply.tool_calls[0].id, "call_1");
        assert_eq!(reply.tool_calls[0].name, "read");
        assert_eq!(reply.tool_calls[0].arguments, r#"{"path":"a.rs"}"#);
        assert_eq!(pieces.len(), 3);
        for piece in &pieces {
            assert_eq!(piece.index, 0, "one call, one position");
            assert_eq!(piece.name, "read");
        }
    }

    #[test]
    fn the_terminal_events_copy_of_a_call_is_not_counted_twice() {
        // The item events carried the whole call, and the terminal event
        // repeats it.
        let mut events = call_events(0, "call_1", "bash", &[r#"{"command":"ls"}"#]);
        events.push(completed_with(
            vec![output_call("call_1", "bash", r#"{"command":"ls"}"#)],
            json!({"input_tokens": 3}),
        ));
        let (reply, _) = drain_tools(&events).unwrap();
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].arguments, r#"{"command":"ls"}"#);

        // A call the item events never carried only shows up there.
        let (fallback, pieces) = drain_tools(&[completed_with(
            vec![
                json!({"type": "message", "role": "assistant", "content": []}),
                output_call("call_9", "read", r#"{"path":"a.rs"}"#),
            ],
            json!({"input_tokens": 3}),
        )])
        .unwrap();
        assert_eq!(
            fallback.tool_calls,
            vec![ToolCall {
                id: "call_9".into(),
                name: "read".into(),
                arguments: r#"{"path":"a.rs"}"#.into(),
            }]
        );
        assert_eq!(pieces.len(), 1, "the whole call arrives as one delta");
        assert_eq!(pieces[0].arguments, r#"{"path":"a.rs"}"#);
    }

    #[test]
    fn a_refusal_delta_is_fatal() {
        let error =
            drain(&[json!({"type": "response.refusal.delta", "delta": "can't help"})]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Other);
        assert!(!error.retryable);
        assert!(error.message.contains("refused"));

        // An empty delta is a refusal that never arrived.
        let ignored = drain(&[
            json!({"type": "response.refusal.delta", "delta": ""}),
            completed(json!({"input_tokens": 1})),
        ])
        .unwrap();
        assert_eq!(ignored.text, "");
    }

    #[test]
    fn incomplete_reasons_are_classified() {
        let filtered = drain(&[
            json!({"type": "response.incomplete", "response": {"incomplete_details": {"reason": "content_filter"}}}),
        ])
        .unwrap_err();
        assert!(!filtered.retryable);

        let broken = drain(&[
            json!({"type": "response.incomplete", "response": {"incomplete_details": {"reason": "internal_error"}}}),
        ])
        .unwrap_err();
        assert!(broken.retryable);

        let truncated = drain(&[
            json!({"type": "response.incomplete", "response": {"incomplete_details": {"reason": "max_output_tokens"}}}),
        ])
        .unwrap();
        assert_eq!(truncated.finish_reason.as_deref(), Some("length"));

        let unexplained =
            drain(&[json!({"type": "response.incomplete", "response": {}})]).unwrap_err();
        assert!(unexplained.retryable);
        assert!(unexplained.message.contains("unknown reason"));
    }

    #[test]
    fn a_stream_without_a_terminal_event_is_retried() {
        let error =
            drain(&[json!({"type": "response.output_text.delta", "delta": "hi"})]).unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(error.retryable);
        assert!(error.message.contains("terminal"));
    }

    #[test]
    fn mid_stream_errors_retry_only_when_temporary() {
        let failed =
            |code: &str| json!({"type": "response.failed", "response": {"error": {"code": code}}});
        let unavailable = drain(&[failed("subscription_sharing_usage_unavailable")]).unwrap_err();
        assert_eq!(unavailable.kind, ErrorKind::Other);
        assert!(unavailable.retryable);

        let spent = drain(&[failed("subscription_sharing_usage_limit_exceeded")]).unwrap_err();
        assert_eq!(spent.kind, ErrorKind::Quota);
        assert!(!spent.retryable);

        let limited = drain(&[failed("rate_limit_exceeded")]).unwrap_err();
        assert_eq!(limited.kind, ErrorKind::RateLimited);
        assert!(limited.retryable);

        // A bare `error` event carries the code on the event itself.
        let bare = drain(&[json!({"type": "error", "code": "rate_limit", "message": "slow"})])
            .unwrap_err();
        assert_eq!(bare.kind, ErrorKind::RateLimited);
        assert!(bare.retryable);

        for code in ["server_error", ""] {
            let temporary = drain(&[failed(code)]).unwrap_err();
            assert_eq!(temporary.kind, ErrorKind::Other, "{code}");
            assert!(temporary.retryable, "{code}");
        }
    }

    // -----------------------------------------------------------------------
    // Wire tests: a local server feeds real SSE through the request layer.
    // -----------------------------------------------------------------------

    /// One request the test server answered.
    #[derive(Debug)]
    struct Asked {
        at: Instant,
        head: String,
        body: String,
    }

    /// A local HTTP server: answers each connection with the next canned reply
    /// and closes, and beyond those just reads and closes, recording what each
    /// request carried and when it arrived.
    async fn serve(replies: Vec<String>) -> (String, Arc<Mutex<Vec<Asked>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let asked: Arc<Mutex<Vec<Asked>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&asked);
        tokio::spawn(async move {
            let mut replies = replies.into_iter();
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 8_192];
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
                        at: Instant::now(),
                        head,
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

    fn sse(events: &[Value]) -> String {
        let mut body = String::new();
        for event in events {
            body.push_str(&format!("data: {}\n\n", event));
        }
        body.push_str("data: [DONE]\n\n");
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{}",
            body
        )
    }

    fn failure_reply(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {}\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{}",
            status, body
        )
    }

    fn arrivals(asked: &Arc<Mutex<Vec<Asked>>>) -> Vec<Instant> {
        asked.lock().unwrap().iter().map(|ask| ask.at).collect()
    }

    fn bodies(asked: &Arc<Mutex<Vec<Asked>>>) -> Vec<String> {
        asked
            .lock()
            .unwrap()
            .iter()
            .map(|ask| ask.body.clone())
            .collect()
    }

    #[tokio::test]
    async fn a_streamed_turn_comes_back_whole() {
        let events = [
            json!({"type": "response.reasoning_summary_text.delta", "delta": "thinking"}),
            json!({"type": "response.output_text.delta", "delta": "Hello"}),
            json!({"type": "response.output_text.delta", "delta": " world"}),
            completed(json!({"input_tokens": 9})),
        ];
        let (url, asked) = serve(vec![sse(&events)]).await;
        let client = Client::new(config(&url)).unwrap();
        let mut streamed = String::new();
        let reply = client
            .complete(&user(), &[], "pair-1-mentor", &mut |delta| {
                if let Delta::Text(text) = delta {
                    streamed.push_str(text);
                }
                CancelResult::Continue
            })
            .await
            .unwrap();

        assert_eq!(arrivals(&asked).len(), 1);
        assert_eq!(streamed, "Hello world", "deltas arrive as they stream");
        assert_eq!(reply.text, "Hello world");
        assert_eq!(reply.reasoning, "thinking");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(reply.session_id.as_deref(), Some("resp_1"));

        let body: Value = serde_json::from_str(&bodies(&asked)[0]).unwrap();
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["prompt_cache_key"], json!("pair-1-mentor"));
        assert_eq!(body["input"][0], json!({"role": "user", "content": "hi"}));
        assert!(body.get("tools").is_none(), "no tools, no tools key");
        let head = &asked.lock().unwrap()[0].head;
        assert!(head.contains("authorization: bearer sk-test"), "{head}");
    }

    #[tokio::test]
    async fn a_gateway_that_renames_a_tool_sends_the_wire_name_and_gets_ours_back() {
        let mut events = vec![json!({"type": "response.output_text.delta",
                                    "delta": "listing now"})];
        events.extend(call_events(
            0,
            "call_1",
            "run_command",
            &[r#"{"command":"#, r#""ls"}"#],
        ));
        events.push(completed_with(
            vec![output_call("call_1", "run_command", r#"{"command":"ls"}"#)],
            json!({"input_tokens": 11}),
        ));
        let (url, asked) = serve(vec![sse(&events)]).await;
        let mut renamed = config(&url);
        renamed.tool_names = Some(BTreeMap::from([(
            "bash".to_string(),
            "run_command".to_string(),
        )]));
        let client = Client::new(renamed).unwrap();
        let mut pieces: Vec<Piece> = Vec::new();
        let reply = client
            .complete(&user(), &tools(), "pair-1-mentor", &mut |delta| {
                if let Delta::Tool {
                    index,
                    id,
                    name,
                    arguments,
                } = delta
                {
                    pieces.push(Piece {
                        index,
                        id: id.to_string(),
                        name: name.to_string(),
                        arguments: arguments.to_string(),
                    });
                }
                CancelResult::Continue
            })
            .await
            .unwrap();

        // What went out declares both tools, the renamed one under the name the
        // gateway expects.
        let body: Value = serde_json::from_str(&bodies(&asked)[0]).unwrap();
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "run_command");
        assert_eq!(body["tools"][0]["description"], "run a command");
        assert_eq!(body["tools"][0]["parameters"]["type"], "object");
        assert_eq!(body["tools"][1]["name"], "read");
        // What came back is the text, and the call under our own name.
        assert_eq!(reply.text, "listing now");
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
                Piece {
                    index: 0,
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: "".into(),
                },
                Piece {
                    index: 0,
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: r#"{"command":"#.into(),
                },
                Piece {
                    index: 0,
                    id: "call_1".into(),
                    name: "bash".into(),
                    arguments: r#""ls"}"#.into(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn the_completed_event_carries_the_usage() {
        let events = [completed(json!({"input_tokens": 120,
                                        "input_tokens_details": {"cached_tokens": 64},
                                        "output_tokens": 30,
                                        "output_tokens_details": {"reasoning_tokens": 12}}))];
        let (url, asked) = serve(vec![sse(&events)]).await;
        let client = Client::new(config(&url)).unwrap();
        let reply = client
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();
        let usage = reply.usage;
        assert_eq!(usage.prompt_tokens, 120);
        assert_eq!(usage.completion_tokens, 30);
        assert_eq!(usage.cached_tokens, 64);
        assert_eq!(usage.reasoning_tokens, 12);
        assert_eq!(arrivals(&asked).len(), 1);
    }

    #[tokio::test]
    async fn a_rate_limit_then_a_server_error_are_retried_with_backoff() {
        let replies = vec![
            failure_reply(
                "429 Too Many Requests",
                r#"{"error":{"message":"slow down"}}"#,
            ),
            failure_reply(
                "500 Internal Server Error",
                r#"{"error":{"message":"boom"}}"#,
            ),
            sse(&[
                json!({"type": "response.output_text.delta", "delta": "ok"}),
                completed(json!({"input_tokens": 3})),
            ]),
        ];
        let (url, asked) = serve(replies).await;
        let client = Client::new(config(&url)).unwrap();
        let mut retries = Vec::new();
        let reply = client
            .complete(&user(), &[], "k", &mut |delta| {
                if let Delta::Retry { attempt, .. } = delta {
                    retries.push(attempt);
                }
                CancelResult::Continue
            })
            .await
            .unwrap();

        assert_eq!(reply.text, "ok");
        let arrivals = arrivals(&asked);
        assert_eq!(arrivals.len(), 3, "both failures were retried");
        assert_eq!(retries, vec![1, 2]);
        assert!(
            arrivals[1].duration_since(arrivals[0]) >= Duration::from_millis(400),
            "the first retry waits out its backoff: {:?}",
            arrivals[1].duration_since(arrivals[0])
        );
        assert!(
            arrivals[2].duration_since(arrivals[1]) >= Duration::from_millis(800),
            "the backoff doubles: {:?}",
            arrivals[2].duration_since(arrivals[1])
        );
    }

    #[tokio::test]
    async fn a_bad_request_fails_at_once() {
        let replies = vec![
            failure_reply(
                "400 Bad Request",
                r#"{"error":{"message":"unknown parameter"}}"#,
            ),
            sse(&[]),
        ];
        let (url, asked) = serve(replies).await;
        let client = Client::new(config(&url)).unwrap();
        let error = client
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();

        assert_eq!(arrivals(&asked).len(), 1, "a 400 is not retried");
        assert_eq!(error.kind, ErrorKind::Other);
        assert_eq!(error.attempts, 1);
        assert!(
            error.message.contains("unknown parameter"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn a_stream_that_stops_short_is_sent_again() {
        let torn = sse(&[json!({"type": "response.output_text.delta", "delta": "half"})]);
        let whole = sse(&[
            json!({"type": "response.output_text.delta", "delta": "whole"}),
            completed(json!({"input_tokens": 5})),
        ]);
        let (url, asked) = serve(vec![torn, whole]).await;
        let client = Client::new(config(&url)).unwrap();
        let reply = client
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();

        assert_eq!(reply.text, "whole", "the torn stream is discarded whole");
        assert_eq!(arrivals(&asked).len(), 2);
    }
}
