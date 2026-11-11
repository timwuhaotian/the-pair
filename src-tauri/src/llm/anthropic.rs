//! Anthropic Messages: `POST {base_url}/v1/messages`, one SSE stream back. The
//! same wire format is spoken by MiniMax and other Anthropic-compatible
//! providers, whose model names [`ClaudeVersion`] declines to parse.
//!
//! History is kept in The Pair's stored OpenAI chat form and converted per
//! request, so the bytes sent for earlier messages never change: the system
//! prompt moves to a top-level `system` field, every other turn becomes a list
//! of content blocks, consecutive user turns merge, and the last block carries
//! a `cache_control` breakpoint. An assistant turn's tool calls become `tool_use`
//! blocks and a `tool` message becomes a `tool_result` block in a user turn.
//!
//! Tools go out as a flat list — `name`, `description`, `input_schema`, with
//! none of OpenAI's `type:"function"` wrapper — and a call comes back as a
//! `tool_use` content block whose input streams as `input_json_delta` pieces:
//!
//! ```text
//! → POST /v1/messages        x-api-key: sk-ant-…  anthropic-version: 2023-06-01
//! → {"model":"claude-sonnet-4-5","max_tokens":8192,"stream":true,
//! →  "system":[{"type":"text","text":"You are the mentor."}],
//! →  "tools":[{"name":"bash","description":"Run a command",
//! →            "input_schema":{"type":"object"}}],
//! →  "messages":[{"role":"user","content":[{"type":"text","text":"review this"}]}]}
//! ← data: {"type":"message_start","message":{"usage":{"input_tokens":12}}}
//! ← data: {"type":"content_block_delta","index":0,
//!          "delta":{"type":"text_delta","text":"Looks right."}}
//! ← data: {"type":"content_block_start","index":1,
//!          "content_block":{"type":"tool_use","id":"toolu_1","name":"bash"}}
//! ← data: {"type":"content_block_delta","index":1,
//!          "delta":{"type":"input_json_delta","partial_json":"{\"command\":"}}
//! ← data: {"type":"content_block_delta","index":1,
//!          "delta":{"type":"input_json_delta","partial_json":"\"ls\"}"}}
//! ← data: {"type":"content_block_stop","index":1}
//! ← data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},
//!          "usage":{"output_tokens":7}}
//! ← data: {"type":"message_stop"}
//! ```
//!
//! Usage arrives in two pieces: `message_start` reports the input tokens, cache
//! reads and writes included, and `message_delta` the output tokens — so later
//! values replace earlier ones for the providers that send placeholders first.
//! Thinking is split by model family: older Claude models take an explicit token
//! budget, everything newer decides adaptively and takes an effort instead.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::history::{arguments_object, content_text, system_text};
use super::tokens::cap_output;
use super::{
    emit, our_tool_name, truncate, wire_tool_name, CancelResult, ClientConfig, Delta, ErrorKind,
    Failure, Prepared, Reply, StreamParser, ToolCall,
};
use crate::accounts::types::normalize_base_url;

/// The API version every request names; Anthropic turns a request away without
/// it.
const VERSION: &str = "2023-06-01";
/// The output cap for a compatible provider whose model name gives no family.
const FALLBACK_MAX_TOKENS: u64 = 64_000;
/// The beta header that names the interleaved-thinking feature, which a
/// budgeted thinking stream asks for.
const INTERLEAVED_BETA: &str = "interleaved-thinking-2025-05-14";
/// The budget an effort Anthropic does not know takes: a route can be given one
/// by hand, and an enabled thinking block with no budget is turned away.
const FALLBACK_BUDGET_TOKENS: u64 = 4_096;
/// The most tool calls one reply keeps, so a provider that floods or replays
/// blocks cannot grow it without bound.
const MAX_STREAMED_CALLS: usize = 128;

pub(super) fn prepare(
    config: &ClientConfig,
    messages: &[Value],
    tools: &[Value],
    _cache_key: &str,
) -> Result<Prepared, Failure> {
    let claude = ClaudeVersion::parse(&config.model);
    let (system, history) = system_text(messages);
    let mut body = Map::new();
    body.insert("model".into(), config.model.clone().into());
    let max_tokens = max_tokens(config, claude);
    body.insert("max_tokens".into(), max_tokens.into());
    body.insert("stream".into(), true.into());
    if !system.is_empty() {
        body.insert(
            "system".into(),
            json!([{"type": "text", "text": system, "cache_control": {"type": "ephemeral"}}]),
        );
    }
    body.insert("messages".into(), convert_messages(config, history).into());
    if !tools.is_empty() {
        body.insert("tools".into(), tools_array(config, tools).into());
    }
    let budget = claude.is_some_and(|version| version.budget_thinking());
    for (key, value) in thinking_params(config.reasoning_effort.as_deref(), budget, max_tokens) {
        body.insert(key, value);
    }
    let mut headers = vec![
        ("x-api-key".to_string(), config.api_key.clone()),
        ("anthropic-version".to_string(), VERSION.to_string()),
        ("content-type".to_string(), "application/json".to_string()),
    ];
    if budget && config.reasoning_effort.is_some() {
        headers.push(("anthropic-beta".to_string(), INTERLEAVED_BETA.to_string()));
    }
    Ok(Prepared {
        url: format!("{}/v1/messages", normalize_base_url(&config.base_url)),
        body: serde_json::to_vec(&body)
            .map_err(|error| Failure::fatal(format!("could not serialize the request: {error}")))?,
        headers,
    })
}

/// The tools as Anthropic takes them: a flat name, a description and the JSON
/// Schema the input is validated against, with no `type:"function"` wrapper. An
/// account that renames a tool names it the way the gateway expects.
fn tools_array(config: &ClientConfig, tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            let function = &tool["function"];
            json!({
                "name": wire_tool_name(config, function["name"].as_str().unwrap_or_default()),
                "description": function
                    .get("description")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new())),
                "input_schema": function.get("parameters").cloned().unwrap_or_else(|| json!({})),
            })
        })
        .collect()
}

/// Recognizes Claude model names, whose thinking rules differ from compatible
/// providers'.
#[derive(Debug, Clone, Copy)]
struct ClaudeVersion {
    family: Family,
    major: u32,
    minor: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Family {
    Opus,
    Sonnet,
    Haiku,
    Other,
}

impl ClaudeVersion {
    fn parse(model: &str) -> Option<Self> {
        let name = model.to_ascii_lowercase();
        let rest = name.rsplit('/').next()?.strip_prefix("claude-")?;
        let family = ["opus", "sonnet", "haiku"]
            .iter()
            .zip([Family::Opus, Family::Sonnet, Family::Haiku])
            .find_map(|(word, family)| rest.contains(word).then_some(family));
        let numbers: Vec<u32> = rest
            .split(['-', '.'])
            .filter_map(|part| part.parse().ok())
            .filter(|number| *number < 100)
            .collect();
        let major = *numbers.first()?;
        Some(Self {
            family: family.unwrap_or(Family::Other),
            major,
            minor: numbers.get(1).copied().unwrap_or(0),
        })
    }

    /// Older Claude models take a token budget; newer ones decide adaptively.
    fn budget_thinking(self) -> bool {
        self.major < 4 || (self.major == 4 && self.minor <= 5 && self.family != Family::Other)
    }

    /// The largest output the family accepts, which shrinks the older it is.
    fn max_tokens(self) -> u64 {
        match (self.family, self.major, self.minor) {
            (_, 5.., _) | (Family::Opus, 4, 6..) | (Family::Sonnet, 4, 6..) => 128_000,
            (Family::Opus, 4, 0..=1) => 32_000,
            (_, 4, _) => 64_000,
            // Claude 3.7 was the first of the 3.x line to take a 64K output.
            (Family::Opus | Family::Sonnet, 3, 7..) => 64_000,
            (Family::Haiku, 3, _) => 4_096,
            _ => 8_192,
        }
    }
}

/// The output cap: the family's own limit, narrowed by the route's, and never
/// above what any provider accepts.
fn max_tokens(config: &ClientConfig, claude: Option<ClaudeVersion>) -> u64 {
    let max = match claude {
        Some(version) => config
            .max_output_tokens
            .map_or(version.max_tokens(), |n| n.min(version.max_tokens())),
        None => config.max_output_tokens.unwrap_or(FALLBACK_MAX_TOKENS),
    };
    cap_output(max)
}

/// Budget tokens for the older Claude models, adaptive thinking plus an effort
/// for everything else, including Anthropic-compatible providers.
fn thinking_params(effort: Option<&str>, budget: bool, max_tokens: u64) -> Vec<(String, Value)> {
    let Some(effort) = effort else {
        return vec![("thinking".into(), json!({"type": "disabled"}))];
    };
    if budget {
        // Anthropic's minimum explicit budget is 1024; below that, disable
        // thinking rather than consume the whole reply or send an invalid one.
        if max_tokens <= 1_024 {
            return vec![("thinking".into(), json!({"type": "disabled"}))];
        }
        let tokens = match effort {
            "low" => 1_024,
            "medium" => 4_096,
            "high" => 32_000,
            // An effort the route was given by hand, which Anthropic does not know:
            // it is not one the API takes either, so it names a fixed budget and
            // no `output_config`, rather than an enabled block with no budget at
            // all, which the API turns away.
            _ => FALLBACK_BUDGET_TOKENS,
        };
        return vec![(
            "thinking".to_string(),
            json!({"type": "enabled", "budget_tokens": tokens.min(max_tokens - 1)}),
        )];
    }
    vec![
        (
            "thinking".to_string(),
            json!({"type": "adaptive", "display": "summarized"}),
        ),
        ("output_config".into(), json!({"effort": effort})),
    ]
}

/// Converts OpenAI-format history: consecutive user turns merge into one,
/// assistant tool calls become `tool_use` blocks, a tool result joins the
/// trailing user turn as a `tool_result` block, and the last block carries a
/// cache breakpoint.
fn convert_messages(config: &ClientConfig, history: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for message in history {
        match message["role"].as_str() {
            Some("assistant") => {
                let blocks = assistant_blocks(config, message);
                if !blocks.is_empty() {
                    out.push(json!({"role": "assistant", "content": blocks}));
                }
            }
            Some("system") => {
                push_user(
                    &mut out,
                    vec![json!({"type": "text", "text": format!(
                        "<system>{}</system>",
                        content_text(message)
                    )})],
                );
            }
            Some("tool") => {
                let blocks = tool_result_blocks(message);
                if !blocks.is_empty() {
                    push_user(&mut out, blocks);
                }
            }
            _ => {
                let blocks = user_blocks(message);
                if !blocks.is_empty() {
                    push_user(&mut out, blocks);
                }
            }
        }
    }
    if let Some(last) = out.last_mut() {
        if let Some(blocks) = last["content"].as_array_mut() {
            if let Some(block) = blocks.last_mut() {
                block["cache_control"] = json!({"type": "ephemeral"});
            }
        }
    }
    out
}

/// Appends to the trailing user turn when there is one, and starts one when
/// there is not.
fn push_user(out: &mut Vec<Value>, blocks: Vec<Value>) {
    if let Some(last) = out.last_mut().filter(|m| m["role"] == "user") {
        if let Some(content) = last["content"].as_array_mut() {
            content.extend(blocks);
            return;
        }
    }
    out.push(json!({"role": "user", "content": blocks}));
}

/// A user turn's blocks: its text, and an inline image as the base64 source
/// Anthropic takes. A part that is neither is dropped.
fn user_blocks(message: &Value) -> Vec<Value> {
    match &message["content"] {
        Value::String(text) if !text.is_empty() => vec![json!({"type": "text", "text": text})],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part["type"].as_str() {
                Some("text") => part["text"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                    .map(|text| json!({"type": "text", "text": text})),
                Some("image_url") => image_block(part["image_url"]["url"].as_str()?),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn image_block(url: &str) -> Option<Value> {
    let rest = url.strip_prefix("data:")?;
    let (media_type, data) = rest.split_once(";base64,")?;
    Some(json!({
        "type": "image",
        "source": {"type": "base64", "media_type": media_type, "data": data}
    }))
}

/// A tool result as Anthropic takes it: a `tool_result` block naming the call
/// it answers. One that names no call cannot be matched to a `tool_use` block,
/// so it is left out of the request instead.
fn tool_result_blocks(message: &Value) -> Vec<Value> {
    let id = message["tool_call_id"].as_str().unwrap_or_default();
    if id.is_empty() {
        return Vec::new();
    }
    vec![json!({
        "type": "tool_result",
        "tool_use_id": id,
        "content": content_text(message),
    })]
}

/// An assistant turn's blocks: its text, then its tool calls as `tool_use`
/// blocks. A turn with neither carries no content Anthropic accepts, so it is
/// left out of the request instead.
fn assistant_blocks(config: &ClientConfig, message: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();
    if let Some(text) = message["content"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
    {
        blocks.push(json!({"type": "text", "text": text}));
    }
    for call in message["tool_calls"].as_array().into_iter().flatten() {
        let name = call["function"]["name"].as_str().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        blocks.push(json!({
            "type": "tool_use",
            "id": call["id"].as_str().unwrap_or_default(),
            "name": wire_tool_name(config, name),
            "input": arguments_object(call["function"]["arguments"].as_str().unwrap_or_default()),
        }));
    }
    blocks
}

/// Reads one Messages stream. The events name their own delta type, so text and
/// thinking need no per-block state; the running input usage, whether
/// `message_stop` arrived and the tool blocks being streamed are the only things
/// worth remembering.
pub(super) struct Parser {
    /// The account the request went out on: a call comes back under the wire name
    /// the account maps our own to, and is mapped back from it.
    config: ClientConfig,
    input: InputUsage,
    message_stop: bool,
    /// Each tool block's index to the slot in [`Reply::tool_calls`] it fills.
    /// A delta names that slot, the call's position in the reply, so the pieces of
    /// one call are told from another's whatever the blocks around them are.
    blocks: BTreeMap<u64, usize>,
}

impl Parser {
    pub(super) fn new(config: &ClientConfig) -> Self {
        Self {
            config: config.clone(),
            input: InputUsage::default(),
            message_stop: false,
            blocks: BTreeMap::new(),
        }
    }

    /// The slot the tool block `index` fills, opened under our own name when the
    /// block has not been seen yet.
    fn open_tool(&mut self, reply: &mut Reply, index: u64, id: &str, wire: &str) -> Option<usize> {
        if let Some(slot) = self.blocks.get(&index) {
            return Some(*slot);
        }
        if id.is_empty() || reply.tool_calls.len() >= MAX_STREAMED_CALLS {
            return None;
        }
        let slot = reply.tool_calls.len();
        reply.tool_calls.push(ToolCall {
            id: id.to_string(),
            name: our_tool_name(&self.config, wire),
            arguments: String::new(),
        });
        self.blocks.insert(index, slot);
        Some(slot)
    }
}

/// Input token counts. Anthropic reports them in `message_start`; MiniMax sends
/// placeholders there and the real counts in `message_delta`, so later values
/// replace earlier ones.
#[derive(Default)]
struct InputUsage {
    fresh: u64,
    written: u64,
    read: u64,
}

impl InputUsage {
    fn update(&mut self, usage: &Value, reply: &mut Reply) {
        if !usage.is_object() {
            return;
        }
        if let Some(n) = usage["input_tokens"].as_u64() {
            self.fresh = n;
        }
        if let Some(n) = usage["cache_creation_input_tokens"].as_u64() {
            self.written = n;
        }
        if let Some(n) = usage["cache_read_input_tokens"].as_u64() {
            self.read = n;
        }
        reply.usage.prompt_tokens = self.fresh + self.written + self.read;
        reply.usage.cached_tokens = self.read;
    }
}

impl StreamParser for Parser {
    fn event(
        &mut self,
        event: &Value,
        reply: &mut Reply,
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<(), Failure> {
        match event["type"].as_str() {
            Some("message_start") => self.input.update(&event["message"]["usage"], reply),
            Some("content_block_start") => {
                let block = &event["content_block"];
                if block["type"] == "tool_use" {
                    let index = event["index"].as_u64().unwrap_or_default();
                    let id = block["id"].as_str().unwrap_or_default();
                    let name = block["name"].as_str().unwrap_or_default();
                    self.open_tool(reply, index, id, name);
                }
            }
            Some("content_block_delta") => match event["delta"]["type"].as_str() {
                Some("text_delta") => {
                    let text = event["delta"]["text"].as_str().unwrap_or_default();
                    reply.text.push_str(text);
                    emit(on_delta, Delta::Text(text))?;
                }
                Some("thinking_delta") => {
                    let text = event["delta"]["thinking"].as_str().unwrap_or_default();
                    reply.reasoning.push_str(text);
                    emit(on_delta, Delta::Reasoning(text))?;
                }
                // The pieces of one call's arguments are appended as they arrive,
                // valid JSON or not: what the model sent is what the tool layer
                // reads.
                Some("input_json_delta") => {
                    let index = event["index"].as_u64().unwrap_or_default();
                    let piece = event["delta"]["partial_json"].as_str().unwrap_or_default();
                    let Some(slot) = self.blocks.get(&index).copied() else {
                        return Ok(());
                    };
                    let call = &mut reply.tool_calls[slot];
                    call.arguments.push_str(piece);
                    emit(
                        on_delta,
                        Delta::Tool {
                            index: slot,
                            id: &call.id,
                            name: &call.name,
                            arguments: piece,
                        },
                    )?;
                }
                _ => {}
            },
            // A block that stops takes its index with it, so a delta that arrives
            // after it belongs to no call.
            Some("content_block_stop") => {
                let index = event["index"].as_u64().unwrap_or_default();
                self.blocks.remove(&index);
            }
            Some("message_delta") => {
                if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                    reply.finish_reason = Some(
                        match reason {
                            "max_tokens" => "length",
                            "tool_use" => "tool_calls",
                            "end_turn" | "stop_sequence" => "stop",
                            other => other,
                        }
                        .to_string(),
                    );
                }
                if let Some(output) = event["usage"]["output_tokens"].as_u64() {
                    reply.usage.completion_tokens = output;
                }
                self.input.update(&event["usage"], reply);
            }
            Some("message_stop") => self.message_stop = true,
            Some("error") => {
                let error = &event["error"];
                let message = format!(
                    "provider error mid-stream: {}",
                    truncate(&error.to_string(), 2000)
                );
                return Err(match error["type"].as_str() {
                    Some("authentication_error") => {
                        Failure::of(ErrorKind::Auth, None, false, message)
                    }
                    Some("invalid_request_error" | "permission_error" | "not_found_error") => {
                        Failure::fatal(message)
                    }
                    _ => Failure::transient(message),
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self, reply: &mut Reply) -> Result<(), Failure> {
        if !self.message_stop {
            return Err(Failure::of(
                ErrorKind::BadResponse,
                None,
                true,
                "Anthropic stream ended without message_stop".into(),
            ));
        }
        reply.repair_call_arguments();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Client;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn config(model: &str) -> ClientConfig {
        ClientConfig {
            protocol: crate::accounts::types::Protocol::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: "sk-ant-test".into(),
            headers: Vec::new(),
            tool_names: None,
            model: model.into(),
            max_output_tokens: None,
            reasoning_effort: None,
            timeout: Duration::from_secs(30),
        }
    }

    fn messages() -> Vec<Value> {
        vec![json!({"role": "user", "content": "hi"})]
    }

    /// A client pointed at the local test server.
    fn client(base_url: &str) -> Client {
        let mut local = config("claude-sonnet-4-5");
        local.base_url = base_url.into();
        Client::new(local).unwrap()
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

    /// A `message_start`, whose usage names the input tokens.
    fn usage_in(usage: Value) -> Value {
        json!({"type": "message_start", "message": {"usage": usage}})
    }

    /// A content-block delta: the text field is named after the delta's type.
    fn text_at(index: u64, text: &str) -> Value {
        json!({"type": "content_block_delta", "index": index,
               "delta": {"type": "text_delta", "text": text}})
    }

    fn thinking_at(index: u64, text: &str) -> Value {
        json!({"type": "content_block_delta", "index": index,
               "delta": {"type": "thinking_delta", "thinking": text}})
    }

    /// A `message_delta`: the stop reason and the usage travel together.
    fn message_delta(reason: &str, usage: Value) -> Value {
        json!({"type": "message_delta", "delta": {"stop_reason": reason}, "usage": usage})
    }

    /// A `content_block_start` opening a tool call.
    fn tool_at(index: u64, id: &str, name: &str) -> Value {
        json!({"type": "content_block_start", "index": index,
               "content_block": {"type": "tool_use", "id": id, "name": name}})
    }

    /// A piece of a tool call's arguments.
    fn input_at(index: u64, piece: &str) -> Value {
        json!({"type": "content_block_delta", "index": index,
               "delta": {"type": "input_json_delta", "partial_json": piece}})
    }

    /// A `content_block_stop` closing the block at `index`.
    fn block_stop(index: u64) -> Value {
        json!({"type": "content_block_stop", "index": index})
    }

    /// Drains `events` through a parser, collecting the streamed text.
    fn drain(events: &[Value]) -> Result<Reply, Failure> {
        drain_as(&config("claude-sonnet-4-5"), events)
    }

    /// Drains `events` through a parser on `config`, collecting the streamed
    /// text and the pieces of every tool call.
    fn drain_as(config: &ClientConfig, events: &[Value]) -> Result<Reply, Failure> {
        let mut parser = Parser::new(config);
        let mut reply = Reply::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut pieces: Vec<(usize, String, String, String)> = Vec::new();
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
                    } => pieces.push((
                        index,
                        id.to_string(),
                        name.to_string(),
                        arguments.to_string(),
                    )),
                    Delta::Retry { .. } => {}
                    Delta::Discard => {}
                }
                CancelResult::Continue
            };
            parser.event(event, &mut reply, &mut on_delta)?;
        }
        assert_eq!(reply.text, text);
        assert_eq!(reply.reasoning, reasoning);
        // Every piece named the call it belongs to, and the pieces of one call
        // add up to the arguments it was left with.
        let mut joined: Vec<String> = vec![String::new(); reply.tool_calls.len()];
        for (index, id, name, _) in &pieces {
            let call = &reply.tool_calls[*index];
            assert_eq!(&call.id, id);
            assert_eq!(&call.name, name);
        }
        for (index, _, _, piece) in &pieces {
            joined[*index].push_str(piece);
        }
        for (call, arguments) in reply.tool_calls.iter().zip(&joined) {
            assert_eq!(&call.arguments, arguments);
        }
        parser.finish(&mut reply)?;
        Ok(reply)
    }

    #[test]
    fn claude_versions_pick_thinking_and_limits() {
        let opus = ClaudeVersion::parse("claude-opus-5-5").unwrap();
        assert!(!opus.budget_thinking());
        assert_eq!(opus.max_tokens(), 128_000);
        let sonnet = ClaudeVersion::parse("claude-sonnet-4-5-20250929").unwrap();
        assert!(sonnet.budget_thinking());
        assert_eq!(sonnet.max_tokens(), 64_000);
        let old_sonnet = ClaudeVersion::parse("claude-3-7-sonnet-20250219").unwrap();
        assert_eq!(
            old_sonnet.max_tokens(),
            64_000,
            "Claude 3.7 takes a 64K output"
        );
        let older = ClaudeVersion::parse("claude-3-5-sonnet-20241022").unwrap();
        assert_eq!(older.max_tokens(), 8_192);
        let opus = ClaudeVersion::parse("claude-3-opus-20240229").unwrap();
        assert_eq!(opus.max_tokens(), 8_192);
        let haiku = ClaudeVersion::parse("claude-3-haiku-20240307").unwrap();
        assert_eq!(haiku.max_tokens(), 4_096);
        assert!(ClaudeVersion::parse("MiniMax-M3.1-Flash").is_none());
    }

    #[test]
    fn the_request_names_the_version_and_moves_the_system_prompt_up() {
        let prepared = prepare(
            &config("claude-sonnet-4-5"),
            &[
                json!({"role": "system", "content": "be brief"}),
                json!({"role": "user", "content": "hi"}),
            ],
            &[],
            "k",
        )
        .unwrap();
        assert_eq!(prepared.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(header_of(&prepared, "anthropic-version").unwrap(), VERSION);
        assert_eq!(
            header_of(&prepared, "content-type").unwrap(),
            "application/json"
        );
        assert_eq!(header_of(&prepared, "x-api-key").unwrap(), "sk-ant-test");
        let body = body_of(&prepared);
        assert_eq!(body["model"], "claude-sonnet-4-5");
        assert_eq!(body["stream"], true);
        assert_eq!(
            body["system"],
            json!([{"type": "text", "text": "be brief", "cache_control": {"type": "ephemeral"}}])
        );
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hi");
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"],
            json!({"type": "ephemeral"}),
            "the last block carries the cache breakpoint"
        );
        assert_eq!(
            body["thinking"],
            json!({"type": "disabled"}),
            "no effort names thinking off"
        );
        assert!(body.get("tools").is_none(), "no tools, no tool list");
    }

    #[test]
    fn the_tools_go_out_in_anthropics_own_shape() {
        let tools = vec![
            json!({"type": "function", "function": {
                "name": "bash", "description": "Run a command",
                "parameters": {"type": "object",
                               "properties": {"command": {"type": "string"}}}}}),
            json!({"type": "function", "function": {"name": "read"}}),
        ];
        let body =
            body_of(&prepare(&config("claude-sonnet-4-5"), &messages(), &tools, "k").unwrap());
        assert_eq!(
            body["tools"],
            json!([
                {"name": "bash", "description": "Run a command",
                 "input_schema": {"type": "object",
                                  "properties": {"command": {"type": "string"}}}},
                {"name": "read", "description": "", "input_schema": {}},
            ])
        );
    }

    #[test]
    fn a_renamed_tool_goes_out_under_the_gateways_own_name() {
        let mut named = config("claude-sonnet-4-5");
        named.tool_names = Some(BTreeMap::from([(
            "bash".to_string(),
            "run_command".to_string(),
        )]));
        let tools = vec![json!({"type": "function", "function": {
            "name": "bash", "description": "Run a command",
            "parameters": {"type": "object"}}})];
        let body = body_of(&prepare(&named, &messages(), &tools, "k").unwrap());
        assert_eq!(body["tools"][0]["name"], "run_command");
        assert_eq!(body["tools"][0]["input_schema"], json!({"type": "object"}));
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_is_trimmed() {
        let mut with_slash = config("claude-sonnet-4-5");
        with_slash.base_url = "https://gateway.test/".into();
        let prepared = prepare(&with_slash, &messages(), &[], "k").unwrap();
        assert_eq!(prepared.url, "https://gateway.test/v1/messages");
    }

    #[test]
    fn max_tokens_comes_from_the_family_the_route_or_the_fallback() {
        // The cap `prepare` would send, with the version parsed as it parses it.
        let cap = |model: &str| {
            let config = config(model);
            max_tokens(&config, ClaudeVersion::parse(&config.model))
        };
        assert_eq!(cap("claude-sonnet-4-5"), 64_000);
        assert_eq!(
            cap("claude-3-haiku-20240307"),
            4_096,
            "the family's own limit is the ceiling"
        );
        let mut capped = config("claude-3-haiku-20240307");
        capped.max_output_tokens = Some(64_000);
        assert_eq!(
            max_tokens(&capped, ClaudeVersion::parse(&capped.model)),
            4_096
        );
        let mut smaller = config("claude-sonnet-4-5");
        smaller.max_output_tokens = Some(8_192);
        assert_eq!(
            max_tokens(&smaller, ClaudeVersion::parse(&smaller.model)),
            8_192
        );
        let mut huge = config("MiniMax-M3.1-Flash");
        huge.max_output_tokens = Some(1_000_000);
        assert_eq!(
            max_tokens(&huge, ClaudeVersion::parse(&huge.model)),
            131_072,
            "no provider accepts more than the ceiling"
        );
        assert_eq!(
            max_tokens(&config("MiniMax-M3.1-Flash"), None),
            64_000,
            "a name that is no Claude gets the fallback"
        );
    }

    #[test]
    fn thinking_splits_by_model_family() {
        let mut model = config("claude-opus-5-5");
        model.reasoning_effort = Some("high".into());
        let prepared = prepare(&model, &messages(), &[], "k").unwrap();
        let body = body_of(&prepared);
        assert_eq!(
            body["thinking"],
            json!({"type": "adaptive", "display": "summarized"}),
            "a new Claude decides adaptively"
        );
        assert_eq!(body["output_config"], json!({"effort": "high"}));
        assert!(
            header_of(&prepared, "anthropic-beta").is_none(),
            "an adaptive model needs no beta header"
        );

        let mut older = config("claude-sonnet-4-5");
        older.reasoning_effort = Some("medium".into());
        let prepared = prepare(&older, &messages(), &[], "k").unwrap();
        let body = body_of(&prepared);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 4_096);
        assert_eq!(
            header_of(&prepared, "anthropic-beta").unwrap(),
            INTERLEAVED_BETA,
            "a budgeted model asks for interleaved thinking"
        );
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn history_merges_consecutive_user_turns_under_one_breakpoint() {
        let history = vec![
            json!({"role": "user", "content": "fix it"}),
            json!({"role": "assistant", "content": "on it"}),
            json!({"role": "user", "content": "and this"}),
            json!({"role": "user", "content": "and that"}),
        ];
        let out = convert_messages(&config("claude-sonnet-4-5"), &history);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2]["role"], "user");
        let blocks = out[2]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].get("cache_control").is_none());
        assert_eq!(blocks[1]["cache_control"], json!({"type": "ephemeral"}));
    }

    #[test]
    fn a_prompt_and_its_pasted_image_are_one_user_turn() {
        let history = vec![
            json!({"role": "user", "content": "what is [Image #1]"}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "[Image #1]:"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
            ]}),
        ];
        let out = convert_messages(&config("claude-sonnet-4-5"), &history);
        assert_eq!(out.len(), 1);
        let blocks = out[0]["content"].as_array().unwrap();
        assert_eq!(blocks[1], json!({"type": "text", "text": "[Image #1]:"}));
        assert_eq!(
            blocks[2]["source"],
            json!({"type": "base64", "media_type": "image/png", "data": "AAAA"})
        );
    }

    #[test]
    fn a_mid_history_system_message_is_wrapped_as_text() {
        let history = vec![
            json!({"role": "user", "content": "hi"}),
            json!({"role": "system", "content": "remember"}),
        ];
        let out = convert_messages(&config("claude-sonnet-4-5"), &history);
        assert_eq!(out.len(), 1, "the system text joins the user turn");
        assert_eq!(out[0]["content"][1]["text"], "<system>remember</system>");
    }

    #[test]
    fn history_becomes_tool_use_and_tool_result_blocks() {
        let mut named = config("claude-sonnet-4-5");
        named.tool_names = Some(BTreeMap::from([(
            "bash".to_string(),
            "run_command".to_string(),
        )]));
        let out = convert_messages(
            &named,
            &[
                json!({"role": "user", "content": "list the files"}),
                json!({"role": "assistant", "content": "on it", "tool_calls": [
                    {"id": "toolu_1", "type": "function", "function": {
                        "name": "bash", "arguments": "{\"command\":\"ls\"}"}}]}),
                json!({"role": "tool", "tool_call_id": "toolu_1", "content": "a.rs"}),
                json!({"role": "tool", "tool_call_id": "toolu_2", "content": "b.rs"}),
                json!({"role": "user", "content": "thanks"}),
            ],
        );
        assert_eq!(out.len(), 3, "{out:?}");
        assert_eq!(
            out[1],
            json!({"role": "assistant", "content": [
                {"type": "text", "text": "on it"},
                {"type": "tool_use", "id": "toolu_1", "name": "run_command",
                 "input": {"command": "ls"}},
            ]}),
            "the call goes out under the name the gateway expects"
        );
        assert_eq!(
            out[2],
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "a.rs"},
                {"type": "tool_result", "tool_use_id": "toolu_2", "content": "b.rs"},
                {"type": "text", "text": "thanks", "cache_control": {"type": "ephemeral"}},
            ]}),
            "the results join one user turn"
        );
    }

    #[test]
    fn an_assistant_turn_of_tool_calls_alone_is_kept() {
        let out = convert_messages(
            &config("claude-sonnet-4-5"),
            &[
                json!({"role": "assistant", "content": "  ", "tool_calls": [
                    {"id": "toolu_1", "function": {"name": "read", "arguments": "{}"}}]}),
                json!({"role": "user", "content": "and now"}),
            ],
        );
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(out[0]["content"][0]["type"], "tool_use");
    }

    #[test]
    fn a_tool_call_with_no_name_is_left_out() {
        let out = convert_messages(
            &config("claude-sonnet-4-5"),
            &[
                json!({"role": "assistant", "content": "on it", "tool_calls": [
                    {"id": "toolu_1", "function": {"arguments": "{}"}}]}),
                json!({"role": "user", "content": "and now"}),
            ],
        );
        assert_eq!(out.len(), 2, "{out:?}");
        assert_eq!(
            out[0]["content"],
            json!([{"type": "text", "text": "on it"}])
        );
    }

    #[test]
    fn a_tool_result_that_names_no_call_is_left_out() {
        let out = convert_messages(
            &config("claude-sonnet-4-5"),
            &[
                json!({"role": "user", "content": "go"}),
                json!({"role": "tool", "content": "orphan"}),
            ],
        );
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn whitespace_only_assistant_text_is_skipped() {
        let history = vec![
            json!({"role": "assistant", "content": "  \n  "}),
            json!({"role": "user", "content": "hi"}),
        ];
        let out = convert_messages(&config("claude-sonnet-4-5"), &history);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0]["role"], "user");
    }

    #[test]
    fn stream_assembles_thinking_text_and_usage() {
        let reply = drain(&[
            usage_in(json!({"input_tokens": 5, "cache_read_input_tokens": 100})),
            thinking_at(0, "plan"),
            text_at(1, "Reading."),
            message_delta("end_turn", json!({"output_tokens": 9})),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        assert_eq!(reply.text, "Reading.");
        assert_eq!(reply.reasoning, "plan");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            (
                reply.usage.prompt_tokens,
                reply.usage.cached_tokens,
                reply.usage.completion_tokens
            ),
            (105, 100, 9)
        );
    }

    #[test]
    fn stop_reasons_are_normalized() {
        let reason = |stop| {
            drain(&[
                text_at(0, "x"),
                message_delta(stop, json!({})),
                json!({"type": "message_stop"}),
            ])
            .unwrap()
            .finish_reason
        };
        assert_eq!(reason("max_tokens").as_deref(), Some("length"));
        assert_eq!(reason("tool_use").as_deref(), Some("tool_calls"));
        assert_eq!(reason("stop_sequence").as_deref(), Some("stop"));
        assert_eq!(reason("refusal").as_deref(), Some("refusal"));
    }

    #[test]
    fn a_tool_call_streams_into_the_reply() {
        let reply = drain(&[
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "text", "text": ""}}),
            text_at(0, "On it."),
            tool_at(1, "toolu_1", "bash"),
            input_at(1, "{\"command\":"),
            input_at(1, "\"ls\"}"),
            block_stop(1),
            message_delta("tool_use", json!({"output_tokens": 9})),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        assert_eq!(reply.text, "On it.", "the text and the call both survive");
        assert_eq!(reply.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "toolu_1".into(),
                name: "bash".into(),
                arguments: "{\"command\":\"ls\"}".into(),
            }]
        );
    }

    #[test]
    fn two_tool_calls_stream_side_by_side() {
        let reply = drain(&[
            tool_at(0, "toolu_1", "read"),
            input_at(0, "{\"path\":\"a.rs\"}"),
            block_stop(0),
            tool_at(1, "toolu_2", "bash"),
            input_at(1, "{\"command\":"),
            input_at(1, "\"ls -l\"}"),
            block_stop(1),
            message_delta("tool_use", json!({})),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        let names: Vec<&str> = reply
            .tool_calls
            .iter()
            .map(|call| call.name.as_str())
            .collect();
        assert_eq!(names, vec!["read", "bash"]);
        assert_eq!(reply.tool_calls[1].arguments, "{\"command\":\"ls -l\"}");
    }

    #[test]
    fn a_renamed_tool_comes_back_under_our_own_name() {
        let mut named = config("claude-sonnet-4-5");
        named.tool_names = Some(BTreeMap::from([(
            "bash".to_string(),
            "run_command".to_string(),
        )]));
        let reply = drain_as(
            &named,
            &[
                tool_at(0, "toolu_1", "run_command"),
                input_at(0, "{\"command\":\"ls\"}"),
                block_stop(0),
                message_delta("tool_use", json!({})),
                json!({"type": "message_stop"}),
            ],
        )
        .unwrap();
        assert_eq!(reply.tool_calls[0].name, "bash");
        assert_eq!(reply.tool_calls[0].arguments, "{\"command\":\"ls\"}");
    }

    #[test]
    fn a_repeated_block_start_reuses_the_call_it_opened() {
        let reply = drain(&[
            tool_at(0, "toolu_1", "bash"),
            input_at(0, "{\"command\":"),
            tool_at(0, "toolu_1", "bash"),
            input_at(0, "\"ls\"}"),
            message_delta("tool_use", json!({})),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].arguments, "{\"command\":\"ls\"}");
    }

    #[test]
    fn torn_arguments_are_repaired_so_the_history_stays_replayable() {
        let reply = drain(&[
            tool_at(0, "toolu_1", "bash"),
            input_at(0, "{\"command\":"),
            block_stop(0),
            message_delta("tool_use", json!({})),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        assert_eq!(
            reply.tool_calls[0].arguments, "{}",
            "a torn input is not sent back as it arrived"
        );
    }

    #[test]
    fn a_block_of_another_kind_and_an_unknown_delta_are_read_past() {
        let reply = drain(&[
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "text", "text": ""}}),
            thinking_at(0, "plan"),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "signature_delta", "signature": "abc"}}),
            block_stop(0),
            text_at(1, "Done."),
            block_stop(1),
            message_delta("end_turn", json!({})),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        assert_eq!(reply.text, "Done.");
        assert_eq!(reply.reasoning, "plan");
        assert!(reply.tool_calls.is_empty());
    }

    #[test]
    fn a_tool_piece_without_its_block_is_dropped() {
        let reply = drain(&[
            input_at(3, "{\"command\":\"ls\"}"),
            tool_at(4, "", "bash"),
            input_at(4, "{\"command\":\"ls\"}"),
            block_stop(4),
            message_delta("tool_use", json!({})),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        assert!(reply.tool_calls.is_empty(), "{:?}", reply.tool_calls);
    }

    #[test]
    fn a_tool_piece_after_its_block_stopped_is_dropped() {
        let reply = drain(&[
            tool_at(0, "toolu_1", "bash"),
            input_at(0, "{\"command\":\"ls\"}"),
            block_stop(0),
            input_at(0, "{\"command\":\"rm -rf\"}"),
            message_delta("tool_use", json!({})),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        assert_eq!(reply.tool_calls[0].arguments, "{\"command\":\"ls\"}");
    }

    #[test]
    fn a_flood_of_tool_blocks_stops_at_the_cap() {
        let mut events: Vec<Value> = (0..MAX_STREAMED_CALLS + 8)
            .map(|index| tool_at(index as u64, "toolu_flood", "bash"))
            .collect();
        events.push(json!({"type": "message_stop"}));
        let reply = drain(&events).unwrap();
        assert_eq!(reply.tool_calls.len(), MAX_STREAMED_CALLS);
    }

    #[test]
    fn a_cancelled_tool_delta_stops_the_stream() {
        let mut parser = Parser::new(&config("claude-sonnet-4-5"));
        let mut reply = Reply::default();
        let mut keep = |_: Delta| CancelResult::Continue;
        parser
            .event(&tool_at(0, "toolu_1", "bash"), &mut reply, &mut keep)
            .unwrap();
        let error = parser
            .event(
                &input_at(0, "{\"command\":"),
                &mut reply,
                &mut |_: Delta| CancelResult::Cancel,
            )
            .unwrap_err();
        assert!(error.cancelled);
        assert_eq!(
            reply.tool_calls[0].arguments, "{\"command\":",
            "the piece landed before the cancel"
        );
    }

    #[test]
    fn message_stop_is_required() {
        let mut parser = Parser::new(&config("claude-sonnet-4-5"));
        let mut reply = Reply::default();
        let mut on_delta = |_: Delta| CancelResult::Continue;
        parser
            .event(
                &json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}),
                &mut reply,
                &mut on_delta,
            )
            .unwrap();
        assert!(parser.finish(&mut reply).unwrap_err().retryable);
        parser
            .event(&json!({"type": "message_stop"}), &mut reply, &mut on_delta)
            .unwrap();
        parser.finish(&mut reply).unwrap();
    }

    #[test]
    fn thinking_budget_leaves_visible_output_even_at_tiny_limits() {
        let budget = |max| thinking_params(Some("high"), true, max);
        assert_eq!(budget(1)[0].1, json!({"type": "disabled"}));
        assert_eq!(budget(1_024)[0].1, json!({"type": "disabled"}));
        assert_eq!(budget(1_025)[0].1["budget_tokens"], 1_024);
        assert_eq!(budget(2_000)[0].1["budget_tokens"], 1_999);
        assert_eq!(budget(64_000)[0].1["budget_tokens"], 32_000);
    }

    #[test]
    fn an_unknown_effort_names_a_fixed_budget_rather_than_no_budget() {
        let params = thinking_params(Some("turbo"), true, 8_192);
        assert_eq!(
            params[0].1,
            json!({"type": "enabled", "budget_tokens": 4_096})
        );
        assert_eq!(
            params.len(),
            1,
            "an effort the API does not know is not asked for separately"
        );
    }

    #[test]
    fn minimax_usage_in_message_delta_wins() {
        let reply = drain(&[
            usage_in(json!({"input_tokens": 0, "cache_read_input_tokens": 2200})),
            message_delta(
                "end_turn",
                json!({"input_tokens": 2, "output_tokens": 2, "cache_read_input_tokens": 2198}),
            ),
            json!({"type": "message_stop"}),
        ])
        .unwrap();
        assert_eq!(
            (reply.usage.prompt_tokens, reply.usage.cached_tokens),
            (2200, 2198)
        );
    }

    #[test]
    fn mid_stream_errors_retry_only_when_temporary() {
        let error = |kind| json!({"type": "error", "error": {"type": kind}});
        let refused = drain(&[error("authentication_error")]).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::Auth);
        assert!(!refused.retryable);
        assert!(refused.message.contains("authentication_error"));
        let invalid = drain(&[error("invalid_request_error")]).unwrap_err();
        assert!(!invalid.retryable);
        let overloaded = drain(&[error("overloaded_error")]).unwrap_err();
        assert!(overloaded.retryable, "an overload heals on a retry");
    }

    #[test]
    fn a_cancelled_delta_stops_the_stream() {
        let mut parser = Parser::new(&config("claude-sonnet-4-5"));
        let mut reply = Reply::default();
        let mut on_delta = |_: Delta| CancelResult::Cancel;
        let error = parser
            .event(&text_at(0, "hi"), &mut reply, &mut on_delta)
            .unwrap_err();
        assert!(error.cancelled);
        assert_eq!(reply.text, "hi");
    }

    // -----------------------------------------------------------------------
    // Wire tests: a local server feeds real SSE through the request layer.
    // -----------------------------------------------------------------------

    /// A local server that answers each connection with the next canned reply
    /// and closes, recording when each request arrived and the body it carried.
    async fn serve(
        replies: Vec<String>,
    ) -> (String, Arc<Mutex<Vec<Instant>>>, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let arrivals: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
        let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_arrivals = Arc::clone(&arrivals);
        let seen_bodies = Arc::clone(&bodies);
        tokio::spawn(async move {
            let mut replies = replies.into_iter();
            while let Ok((mut socket, _)) = listener.accept().await {
                seen_arrivals.lock().unwrap().push(Instant::now());
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 8_192];
                loop {
                    let head_end = buffer.windows(4).position(|window| window == b"\r\n\r\n");
                    if let Some(head_end) = head_end {
                        let head =
                            String::from_utf8_lossy(&buffer[..head_end]).to_ascii_lowercase();
                        let length = head
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if buffer.len() >= head_end + 4 + length {
                            seen_bodies
                                .lock()
                                .unwrap()
                                .push(String::from_utf8_lossy(&buffer[head_end + 4..]).to_string());
                            break;
                        }
                    }
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                    }
                }
                if let Some(reply) = replies.next() {
                    let _ = socket.write_all(reply.as_bytes()).await;
                }
                let _ = socket.shutdown().await;
            }
        });
        (url, arrivals, bodies)
    }

    /// A canned HTTP reply: the status line, a JSON content type and the body.
    fn http(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{body}"
        )
    }

    fn sse(events: &[Value]) -> String {
        let mut out = String::from(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
        );
        for event in events {
            out.push_str(&format!("data: {}\n\n", event));
        }
        out
    }

    #[tokio::test]
    async fn a_streamed_turn_comes_back_whole_with_its_usage() {
        let events = [
            usage_in(json!({
                "input_tokens": 12,
                "cache_read_input_tokens": 100,
                "cache_creation_input_tokens": 8
            })),
            thinking_at(0, "check the tests"),
            text_at(1, "Looks"),
            text_at(1, " right."),
            message_delta("end_turn", json!({"output_tokens": 7})),
            json!({"type": "message_stop"}),
        ];
        let (url, arrivals, bodies) = serve(vec![sse(&events)]).await;
        let client = client(&url);
        let mut streamed = String::new();
        let reply = client
            .complete(&messages(), &[], "k", &mut |delta| {
                if let Delta::Text(text) = delta {
                    streamed.push_str(text);
                }
                CancelResult::Continue
            })
            .await
            .unwrap();

        assert_eq!(arrivals.lock().unwrap().len(), 1);
        assert_eq!(reply.text, "Looks right.");
        assert_eq!(reply.reasoning, "check the tests");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            (
                reply.usage.prompt_tokens,
                reply.usage.cached_tokens,
                reply.usage.completion_tokens
            ),
            (120, 100, 7),
            "usage comes from the two usage events"
        );
        assert_eq!(streamed, "Looks right.", "deltas arrive as they stream");

        let body: Value = serde_json::from_slice(bodies.lock().unwrap()[0].as_bytes()).unwrap();
        assert_eq!(body["model"], "claude-sonnet-4-5");
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["max_tokens"], 64_000);
    }

    #[tokio::test]
    async fn a_rate_limit_then_a_server_error_are_retried_with_backoff() {
        let limited = http(
            "429 Too Many Requests",
            r#"{"error":{"message":"slow down"}}"#,
        );
        let broken = http(
            "500 Internal Server Error",
            r#"{"error":{"message":"boom"}}"#,
        );
        let events = [
            usage_in(json!({"input_tokens": 3})),
            text_at(0, "ok"),
            message_delta("end_turn", json!({"output_tokens": 1})),
            json!({"type": "message_stop"}),
        ];
        let (url, arrivals, _) = serve(vec![limited, broken, sse(&events)]).await;
        let client = client(&url);
        let mut retries = Vec::new();
        let reply = client
            .complete(&messages(), &[], "k", &mut |delta| {
                if let Delta::Retry { attempt, .. } = delta {
                    retries.push(attempt);
                }
                CancelResult::Continue
            })
            .await
            .unwrap();

        assert_eq!(reply.text, "ok");
        let arrivals = arrivals.lock().unwrap();
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
        let rejected = http(
            "400 Bad Request",
            r#"{"error":{"message":"unknown parameter"}}"#,
        );
        let (url, arrivals, _) = serve(vec![rejected, sse(&[])]).await;
        let client = client(&url);
        let error = client
            .complete(&messages(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();

        assert_eq!(arrivals.lock().unwrap().len(), 1, "a 400 is not retried");
        assert_eq!(error.kind, ErrorKind::Other);
        assert_eq!(error.attempts, 1);
        assert!(
            error.message.contains("unknown parameter"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn a_stream_without_message_stop_is_retried() {
        let torn = sse(&[usage_in(json!({"input_tokens": 3})), text_at(0, "half")]);
        let events = [
            usage_in(json!({"input_tokens": 3})),
            text_at(0, "whole"),
            message_delta("end_turn", json!({"output_tokens": 1})),
            json!({"type": "message_stop"}),
        ];
        let (url, arrivals, _) = serve(vec![torn, sse(&events)]).await;
        let client = client(&url);
        let reply = client
            .complete(&messages(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();

        assert_eq!(reply.text, "whole", "the torn stream is discarded whole");
        assert_eq!(arrivals.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_streamed_tool_call_comes_back_over_the_wire() {
        let tools = vec![json!({"type": "function", "function": {
            "name": "bash", "description": "Run a command",
            "parameters": {"type": "object",
                           "properties": {"command": {"type": "string"}}}}})];
        let events = [
            usage_in(json!({"input_tokens": 12})),
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "text", "text": ""}}),
            text_at(0, "On it."),
            tool_at(1, "toolu_1", "bash"),
            input_at(1, "{\"command\":"),
            input_at(1, "\"ls\"}"),
            block_stop(1),
            message_delta("tool_use", json!({"output_tokens": 7})),
            json!({"type": "message_stop"}),
        ];
        let (url, arrivals, bodies) = serve(vec![sse(&events)]).await;
        let client = client(&url);
        let mut pieces: Vec<(usize, String)> = Vec::new();
        let reply = client
            .complete(&messages(), &tools, "k", &mut |delta| {
                if let Delta::Tool {
                    index,
                    name,
                    arguments,
                    ..
                } = delta
                {
                    pieces.push((index, format!("{name}({arguments})")));
                }
                CancelResult::Continue
            })
            .await
            .unwrap();

        assert_eq!(arrivals.lock().unwrap().len(), 1);
        assert_eq!(reply.text, "On it.");
        assert_eq!(reply.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "toolu_1".into(),
                name: "bash".into(),
                arguments: "{\"command\":\"ls\"}".into(),
            }]
        );
        assert_eq!(
            pieces,
            vec![
                (0, "bash({\"command\":)".to_string()),
                (0, "bash(\"ls\"})".to_string()),
            ],
            "the pieces arrive as they stream"
        );

        let body: Value = serde_json::from_slice(bodies.lock().unwrap()[0].as_bytes()).unwrap();
        assert_eq!(
            body["tools"],
            json!([{"name": "bash", "description": "Run a command",
                    "input_schema": {"type": "object",
                                     "properties": {"command": {"type": "string"}}}}]),
            "the tools ride along in Anthropic's own shape"
        );
    }

    #[tokio::test]
    async fn a_tool_call_the_stream_tore_comes_back_runnable() {
        let events = [
            usage_in(json!({"input_tokens": 12})),
            tool_at(0, "toolu_1", "bash"),
            input_at(0, "{\"command\":"),
            message_delta("tool_use", json!({"output_tokens": 7})),
            json!({"type": "message_stop"}),
        ];
        let (url, arrivals, _) = serve(vec![sse(&events)]).await;
        let reply = client(&url)
            .complete(&messages(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();
        assert_eq!(arrivals.lock().unwrap().len(), 1);
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "toolu_1".into(),
                name: "bash".into(),
                arguments: "{}".into(),
            }],
            "torn arguments are repaired, so the stored call replays without a 400"
        );
    }
}
