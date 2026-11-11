//! Command Code: the subscription behind the commandcode CLI's own gateway. Requests
//! go to `{base}/alpha/generate` in the CLI's wire format and come back as NDJSON —
//! one JSON object per line, not SSE — with `text-delta`, `reasoning-delta`,
//! `tool-input-*`/`tool-call` and `finish` events. History is kept in OpenAI chat
//! form and converted the same way on every request, so the bytes sent for earlier
//! messages never change.
//!
//! The gateway only answers its own CLI, so every request carries the CLI's headers
//! next to the bearer key:
//!
//! ```text
//! POST https://api.commandcode.ai/alpha/generate
//! authorization: Bearer cc-…
//! user-agent: cli
//! x-command-code-version: 1.88.5
//! ```
//!
//! ```json
//! {"config": {"date": "2026-11-11", "environment": "darwin"},
//!  "permissionMode": "auto-accept", "threadId": "0f3c…", "params": {
//!   "model": "claude-sonnet-5-5", "stream": true, "max_tokens": 64000,
//!   "system": [{"type": "text", "text": "…"}],
//!   "tools": [{"name": "bash", "description": "…", "input_schema": {…}}],
//!   "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]}}
//! ```
//!
//! and each line of the reply is a whole event, with no `data:` prefix:
//!
//! ```text
//! {"type":"text-delta","text":"reading "}
//! {"type":"tool-input-start","id":"c1","toolName":"bash"}
//! {"type":"tool-input-delta","id":"c1","delta":"{\"command\":"}
//! {"type":"tool-call","toolCallId":"c1","toolName":"bash","input":{"command":"ls"}}
//! {"type":"finish","finishReason":"tool-calls","totalUsage":{"inputTokens":50,"outputTokens":7}}
//! ```

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use super::history::{arguments_object, content_text, system_text};
use super::tokens::cap_output;
use super::{
    emit, our_tool_name, truncate, wire_tool_name, CancelResult, ClientConfig, Delta, ErrorKind,
    Failure, Prepared, Reply, StreamParser, ToolCall, Usage,
};
use crate::accounts::types::normalize_base_url;

/// The CLI version the gateway sees. The Pair speaks the CLI's protocol, so it says the
/// CLI's version the way the CLI does; the sign-in flow says it on its requests too.
const CLI_VERSION: &str = "1.88.5";

/// The output cap used when the route names none.
const DEFAULT_OUTPUT_TOKENS: u64 = 64_000;

/// What an image part becomes: this build sends text only, so an image rides along
/// as the URL it came from rather than as bytes the gateway would refuse.
const IMAGE_AS_TEXT: &str = "[image: ";

pub(super) fn prepare(
    config: &ClientConfig,
    messages: &[Value],
    tools: &[Value],
    cache_key: &str,
) -> Result<Prepared, Failure> {
    let (system, history) = system_text(messages);
    let mut wire = Vec::new();
    // Every tool call's wire name by its id, so the tool results below name it the same way.
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    for message in history {
        match message["role"].as_str() {
            Some("assistant") => {
                let mut content = Vec::new();
                for item in message["reasoning_details"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let text = detail_text(item);
                    if !text.is_empty() {
                        content.push(json!({"type": "reasoning", "text": text}));
                    }
                }
                let text = content_text(message);
                if !text.is_empty() {
                    content.push(json!({"type": "text", "text": text}));
                }
                for call in message["tool_calls"].as_array().into_iter().flatten() {
                    let id = call["id"].as_str().unwrap_or_default();
                    let name = call["function"]["name"].as_str().unwrap_or("unknown");
                    let wire_name = wire_tool_name(config, name);
                    names.insert(id.to_string(), wire_name.clone());
                    content.push(json!({
                        "type": "tool-call",
                        "toolCallId": id,
                        "toolName": wire_name,
                        "input": arguments_object(
                            call["function"]["arguments"].as_str().unwrap_or_default()
                        ),
                    }));
                }
                wire.push(json!({"role": "assistant", "content": content}));
            }
            Some("tool") => {
                let id = message["tool_call_id"].as_str().unwrap_or_default();
                let text = content_text(message);
                wire.push(json!({
                    "role": "tool",
                    "content": [{
                        "type": "tool-result",
                        "toolCallId": id,
                        "toolName": names.get(id).cloned().unwrap_or_else(|| "unknown".to_string()),
                        "output": {
                            "type": "text",
                            "value": if text.is_empty() { "(empty)".to_string() } else { text },
                        },
                    }],
                }));
            }
            _ => {
                let mut content = Vec::new();
                match &message["content"] {
                    Value::String(text) if !text.is_empty() => {
                        content.push(json!({"type": "text", "text": text}));
                    }
                    Value::Array(parts) => {
                        for part in parts {
                            match part["type"].as_str() {
                                Some("text")
                                    if part["text"]
                                        .as_str()
                                        .is_some_and(|text| !text.is_empty()) =>
                                {
                                    content.push(json!({"type": "text", "text": part["text"]}));
                                }
                                Some("image_url") => content.push(json!({
                                    "type": "text",
                                    "text": image_as_text(part["image_url"]["url"].as_str()),
                                })),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                if !content.is_empty() {
                    wire.push(json!({"role": "user", "content": content}));
                }
            }
        }
    }
    // The tools the way the CLI asks for them: a flat name, a description, and
    // the JSON Schema the gateway validates the input against. An account that
    // renames a tool names it the way the gateway expects.
    let tools: Vec<Value> = tools
        .iter()
        .map(|tool| {
            let function = &tool["function"];
            let name = function["name"].as_str().unwrap_or("unknown");
            json!({
                "name": wire_tool_name(config, name),
                "description": function
                    .get("description")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new())),
                "input_schema": function.get("parameters").cloned().unwrap_or(json!({})),
            })
        })
        .collect();
    let system: Vec<Value> = (!system.is_empty())
        .then(|| json!({"type": "text", "text": system}))
        .into_iter()
        .collect();
    let max_tokens = config
        .max_output_tokens
        .map_or(DEFAULT_OUTPUT_TOKENS, cap_output);
    let mut params = Map::new();
    params.insert("model".into(), config.model.clone().into());
    params.insert("messages".into(), wire.into());
    params.insert("tools".into(), tools.into());
    params.insert("system".into(), system.into());
    params.insert("max_tokens".into(), max_tokens.into());
    params.insert("stream".into(), true.into());
    if let Some(effort) = config
        .reasoning_effort
        .as_deref()
        .filter(|effort| !effort.is_empty())
    {
        params.insert("reasoning".into(), json!({"effort": effort}));
        params.insert("reasoning_effort".into(), effort.into());
    }
    let thread = thread_id(cache_key);
    let body = json!({
        "config": {
            "date": today(),
            "environment": environment(),
            "structure": [],
            "isGitRepo": false,
            "currentBranch": "",
            "mainBranch": "",
            "gitStatus": "",
            "recentCommits": [],
        },
        "memory": Value::Null,
        "taste": Value::Null,
        "skills": Value::Null,
        "permissionMode": "auto-accept",
        "threadId": thread,
        "params": params,
    });
    Ok(Prepared {
        url: format!("{}/alpha/generate", normalize_base_url(&config.base_url)),
        body: serde_json::to_vec(&body)
            .map_err(|error| Failure::fatal(format!("could not serialize the request: {error}")))?,
        headers: vec![
            ("authorization".into(), format!("Bearer {}", config.api_key)),
            ("content-type".into(), "application/json".into()),
            // The CLI's own headers, which the gateway expects on its own API.
            ("user-agent".into(), "cli".into()),
            ("x-command-code-version".into(), CLI_VERSION.into()),
            ("x-cli-environment".into(), "production".into()),
            ("x-project-slug".into(), "the-pair".into()),
            ("x-taste-learning".into(), "false".into()),
            ("x-session-id".into(), thread),
            ("traceparent".into(), traceparent()?),
        ],
    })
}

/// An image part as text: the URL it came from, cut short.
fn image_as_text(url: Option<&str>) -> String {
    match url {
        Some(url) => format!("{}{}]", IMAGE_AS_TEXT, truncate(url, 200)),
        None => format!("{IMAGE_AS_TEXT}]"),
    }
}

/// The replayable text of one structured reasoning item: scalar fields first, then summaries.
fn detail_text(item: &Value) -> String {
    let mut parts = Vec::new();
    for key in ["text", "thinking"] {
        if let Some(text) = item[key].as_str().filter(|text| !text.is_empty()) {
            parts.push(text);
        }
    }
    for part in item["summary"].as_array().into_iter().flatten() {
        if let Some(text) = part["text"].as_str().filter(|text| !text.is_empty()) {
            parts.push(text);
        }
    }
    parts.join("\n")
}

/// The thread and session id: the cache key hashed into a UUID, so a session's requests share
/// the gateway's thread the way the CLI's do.
fn thread_id(cache_key: &str) -> String {
    let digest = sha256(cache_key.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// A W3C traceparent: 24 random bytes as hex, split into trace and span ids.
fn traceparent() -> Result<String, Failure> {
    let mut bytes = [0u8; 24];
    let (first, second) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    bytes[..16].copy_from_slice(first.as_bytes());
    bytes[16..].copy_from_slice(&second.as_bytes()[..8]);
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("00-{}-{}-01", &hex[..32], &hex[32..]))
}

/// Today as `YYYY-MM-DD`, from days since the epoch (Howard Hinnant's civil date).
fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() / 86400);
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    format!(
        "{:04}-{:02}-{:02}",
        if month <= 2 { year + 1 } else { year },
        month,
        day
    )
}

/// The OS name the gateway knows: what the CLI sends on each platform.
fn environment() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        _ => "linux",
    }
}

/// SHA-256. Hand-rolled: the crate takes no hashing dependency and the thread id needs
/// one fixed-size digest.
fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4d748faa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfb,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb6, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((input.len() as u64) * 8).to_be_bytes());

    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (index, word) in block.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut digest = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// The most streamed tool calls one reply keeps, so a gateway that floods or
/// replays ids can't grow it without bound.
const MAX_STREAMED_CALLS: usize = 128;

/// The state one streamed reply is read into.
pub(super) struct Parser {
    /// The account the request went out on: a call comes back under the wire name
    /// the account maps our own to, and is mapped back from it.
    config: ClientConfig,
    /// Each tool call's id to the reply slot it was first given.
    calls: BTreeMap<String, usize>,
    /// Whether a terminating event arrived, which is the only proof the stream ended
    /// on purpose rather than being cut off.
    terminal: bool,
}

impl Parser {
    pub(super) fn new(config: &ClientConfig) -> Self {
        Self {
            config: config.clone(),
            calls: BTreeMap::new(),
            terminal: false,
        }
    }

    /// The slot of the call `id` names, opened under our own name when new.
    fn slot(&mut self, reply: &mut Reply, id: &str, wire: &str) -> Option<usize> {
        if let Some(slot) = self.calls.get(id) {
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
        self.calls.insert(id.to_string(), slot);
        Some(slot)
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
            Some("text-delta") => {
                let text = event["text"].as_str().unwrap_or_default();
                reply.text.push_str(text);
                emit(on_delta, Delta::Text(text))?;
            }
            Some("reasoning-delta") => {
                let text = event["text"].as_str().unwrap_or_default();
                reply.reasoning.push_str(text);
                emit(on_delta, Delta::Reasoning(text))?;
            }
            // A call opens on its first event and is named there: the deltas that
            // follow it carry only its id and a piece of the input.
            Some("tool-input-start") => {
                let id = event["id"].as_str().unwrap_or_default();
                let wire = event["toolName"].as_str().unwrap_or_default();
                self.slot(reply, id, wire);
            }
            Some("tool-input-delta") => {
                let id = event["id"].as_str().unwrap_or_default();
                let delta = event["delta"].as_str().unwrap_or_default();
                let Some(slot) = self.calls.get(id).copied() else {
                    return Ok(());
                };
                reply.tool_calls[slot].arguments.push_str(delta);
                emit(
                    on_delta,
                    Delta::Tool {
                        index: slot,
                        id,
                        name: &reply.tool_calls[slot].name,
                        arguments: delta,
                    },
                )?;
            }
            Some("tool-call") => {
                // The call carries its whole input: the deltas streamed it
                // already, so only what they haven't covered is appended, and a
                // call with no deltas lands whole.
                let id = event["toolCallId"].as_str().unwrap_or_default();
                let wire = event["toolName"].as_str().unwrap_or_default();
                let Some(slot) = self.slot(reply, id, wire) else {
                    return Ok(());
                };
                let full = event
                    .get("input")
                    .map(|input| serde_json::to_string(input).unwrap_or_default())
                    .unwrap_or_default();
                if reply.tool_calls[slot].arguments.is_empty() {
                    if !full.is_empty() {
                        emit(
                            on_delta,
                            Delta::Tool {
                                index: slot,
                                id,
                                name: &reply.tool_calls[slot].name,
                                arguments: &full,
                            },
                        )?;
                        reply.tool_calls[slot].arguments = full;
                    }
                } else if let Some(rest) = full
                    .strip_prefix(reply.tool_calls[slot].arguments.as_str())
                    .filter(|rest| !rest.is_empty())
                {
                    let rest = rest.to_string();
                    emit(
                        on_delta,
                        Delta::Tool {
                            index: slot,
                            id,
                            name: &reply.tool_calls[slot].name,
                            arguments: &rest,
                        },
                    )?;
                    reply.tool_calls[slot].arguments.push_str(&rest);
                }
            }
            Some("finish-step" | "finish") => {
                self.terminal = true;
                let reason = event["finishReason"]
                    .as_str()
                    .or_else(|| event["rawFinishReason"].as_str())
                    .unwrap_or("stop");
                reply.finish_reason = Some(map_finish(reason).to_string());
                let usage = match event["type"].as_str() {
                    Some("finish") => &event["totalUsage"],
                    _ => &event["usage"],
                };
                reply.usage = parse_usage(usage);
            }
            Some("error") => return Err(stream_error(event)),
            Some("abort") => {
                return Err(Failure::transient(
                    "Command Code aborted the request".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self, reply: &mut Reply) -> Result<(), Failure> {
        if self.terminal {
            // A call whose input never arrived, or arrived torn, is nothing the
            // tool layer can run: it goes back as the empty object the gateway
            // sends for a tool that takes no input.
            reply.repair_call_arguments();
            Ok(())
        } else {
            Err(Failure::of(
                ErrorKind::BadResponse,
                None,
                true,
                "Command Code stream ended without a finish event".into(),
            ))
        }
    }
}

/// A finish reason in OpenAI's values; anything else passes through for `validate_reply`.
fn map_finish(reason: &str) -> &str {
    if reason.contains("tool") {
        "tool_calls"
    } else if reason.contains("length") || reason.contains("max") {
        "length"
    } else if reason.contains("stop") || reason.contains("end") {
        "stop"
    } else {
        reason
    }
}

fn parse_usage(usage: &Value) -> Usage {
    let num = |value: Option<&Value>| value.and_then(Value::as_u64).unwrap_or(0);
    Usage {
        prompt_tokens: num(usage.get("inputTokens")),
        cached_tokens: num(usage.pointer("/inputTokenDetails/cacheReadTokens"))
            .max(num(usage.get("cachedInputTokens"))),
        completion_tokens: num(usage.get("outputTokens")),
        reasoning_tokens: num(usage.pointer("/outputTokenDetails/reasoningTokens"))
            .max(num(usage.get("reasoningTokens"))),
    }
}

/// Error markers that end the subscription's allowance: retrying can't heal them.
const TERMINAL_MARKERS: [&str; 3] = [
    "premium_credits_exhausted",
    "model_not_in_plan",
    "insufficient credits",
];

/// An `error` event's failure: the allowance markers and client errors stay fatal, a rate
/// limit and the rest may heal.
fn stream_error(event: &Value) -> Failure {
    let message = event["message"]
        .as_str()
        .or_else(|| event["error"]["message"].as_str())
        .or_else(|| event["error"].as_str())
        .unwrap_or("unknown error");
    let code = event["code"].as_str().unwrap_or_default();
    let status = event["statusCode"]
        .as_u64()
        .and_then(|code| u16::try_from(code).ok());
    let dump = event.to_string();
    if TERMINAL_MARKERS.iter().any(|marker| dump.contains(marker)) {
        return Failure::of(
            ErrorKind::Quota,
            status,
            false,
            format!("Command Code usage limit: {message}"),
        );
    }
    if status == Some(429) || code.contains("rate_limit") {
        return Failure::of(
            ErrorKind::RateLimited,
            status,
            true,
            format!("Command Code rate limit: {message}"),
        );
    }
    if let Some(status) = status {
        return Failure::from_status(Some(status), format!("Command Code error: {message}"));
    }
    match code {
        "invalid_request_error"
        | "authentication_error"
        | "permission_error"
        | "not_found_error" => Failure::fatal(format!("Command Code error: {message}")),
        _ => Failure::transient(format!("Command Code error: {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn config() -> ClientConfig {
        ClientConfig {
            protocol: crate::accounts::types::Protocol::CommandCode,
            base_url: "https://api.commandcode.ai".into(),
            api_key: "cc-key".into(),
            headers: Vec::new(),
            tool_names: None,
            model: "claude-sonnet-5-5".into(),
            max_output_tokens: None,
            reasoning_effort: Some("xhigh".into()),
            timeout: Duration::from_secs(300),
        }
    }

    fn prepared(messages: &[Value], tools: &[Value]) -> (String, Value, Vec<(String, String)>) {
        let prepared = prepare(&config(), messages, tools, "cache-key").unwrap();
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();
        (prepared.url, body, prepared.headers)
    }

    /// The header `name` carried, or an empty string.
    fn header(headers: &[(String, String)], name: &str) -> String {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    #[test]
    fn the_request_carries_the_cli_wire_format_and_headers() {
        let (url, body, headers) = prepared(
            &[
                json!({"role": "system", "content": "sys"}),
                json!({"role": "user", "content": "hi"}),
            ],
            &[],
        );
        assert_eq!(url, "https://api.commandcode.ai/alpha/generate");
        assert_eq!(body["params"]["model"], "claude-sonnet-5-5");
        assert_eq!(body["params"]["stream"], true);
        assert_eq!(body["params"]["max_tokens"], 64000);
        assert_eq!(body["params"]["reasoning_effort"], "xhigh");
        assert_eq!(
            body["params"]["system"],
            json!([{"type": "text", "text": "sys"}])
        );
        assert_eq!(
            body["params"]["messages"],
            json!([{"role": "user", "content": [{"type": "text", "text": "hi"}]}])
        );
        assert_eq!(body["params"]["tools"], json!([]), "no tools, no tool list");
        assert_eq!(body["permissionMode"], "auto-accept");
        assert!(
            body["config"].get("workingDir").is_none(),
            "the app's own directory is not the pair's, so none is named"
        );
        assert_eq!(header(&headers, "authorization"), "Bearer cc-key");
        assert_eq!(header(&headers, "user-agent"), "cli");
        assert_eq!(header(&headers, "x-command-code-version"), CLI_VERSION);
        assert_eq!(
            header(&headers, "x-session-id"),
            body["threadId"].as_str().unwrap()
        );
        assert!(
            header(&headers, "traceparent").starts_with("00-"),
            "{headers:?}"
        );
    }

    #[test]
    fn the_output_cap_is_the_routes_own_limit() {
        let mut bounded = config();
        bounded.max_output_tokens = Some(8_000);
        let prepared = prepare(
            &bounded,
            &[json!({"role": "user", "content": "hi"})],
            &[],
            "k",
        )
        .unwrap();
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();
        assert_eq!(body["params"]["max_tokens"], 8000);

        let mut huge = config();
        huge.max_output_tokens = Some(u64::MAX);
        let prepared =
            prepare(&huge, &[json!({"role": "user", "content": "hi"})], &[], "k").unwrap();
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();
        assert_eq!(body["params"]["max_tokens"], 131_072);
    }

    #[test]
    fn history_becomes_tool_calls_and_results() {
        let mut named = config();
        named.tool_names = Some(BTreeMap::from([(
            "read".to_string(),
            "read_file".to_string(),
        )]));
        let prepared = prepare(
            &named,
            &[
                json!({"role": "system", "content": "sys"}),
                json!({"role": "user", "content": "read it"}),
                json!({"role": "assistant", "content": "on it",
                    "reasoning_details": [{"type": "reasoning", "summary": [{"text": "plan"}]}],
                    "tool_calls": [{"id": "c1", "type": "function", "function": {
                        "name": "read", "arguments": "{\"path\":\"a\"}"}}]}),
                json!({"role": "tool", "tool_call_id": "c1", "content": "data"}),
            ],
            &[json!({"type": "function", "function": {
                "name": "read", "description": "Read", "parameters": {"type": "object"}}})],
            "k",
        )
        .unwrap();
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();
        let messages = body["params"]["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(
            messages[1]["content"],
            json!([
                {"type": "reasoning", "text": "plan"},
                {"type": "text", "text": "on it"},
                {"type": "tool-call", "toolCallId": "c1", "toolName": "read_file",
                 "input": {"path": "a"}},
            ])
        );
        assert_eq!(
            messages[2],
            json!({"role": "tool", "content": [
                {"type": "tool-result", "toolCallId": "c1", "toolName": "read_file",
                 "output": {"type": "text", "value": "data"}},
            ]})
        );
        assert_eq!(
            body["params"]["tools"],
            json!([{"name": "read_file", "description": "Read",
                    "input_schema": {"type": "object"}}]),
            "the tool goes out under the name the gateway expects"
        );
    }

    /// The tools the gateway is asked for, in the OpenAI chat shape.
    fn tools() -> Vec<Value> {
        vec![
            json!({"type": "function", "function": {
                "name": "bash", "description": "Run a command",
                "parameters": {"type": "object",
                               "properties": {"command": {"type": "string"}}}}}),
            json!({"type": "function", "function": {
                "name": "read", "description": "Read a file",
                "parameters": {"type": "object"}}}),
        ]
    }

    #[test]
    fn the_tools_go_out_under_the_gateways_own_shape() {
        let (_, body, _) = prepared(&[json!({"role": "user", "content": "go"})], &tools());
        assert_eq!(
            body["params"]["tools"],
            json!([
                {"name": "bash", "description": "Run a command",
                 "input_schema": {"type": "object",
                                  "properties": {"command": {"type": "string"}}}},
                {"name": "read", "description": "Read a file",
                 "input_schema": {"type": "object"}},
            ])
        );
    }

    #[test]
    fn an_image_part_rides_along_as_its_url() {
        let (_, body, _) = prepared(
            &[json!({"role": "user", "content": [
                {"type": "text", "text": "look"},
                {"type": "image_url", "image_url": {"url": "https://example/x.png"}},
            ]})],
            &[],
        );
        assert_eq!(
            body["params"]["messages"][0]["content"],
            json!([
                {"type": "text", "text": "look"},
                {"type": "text", "text": "[image: https://example/x.png]"},
            ])
        );
    }

    #[test]
    fn the_stream_assembles_text_reasoning_calls_and_usage() {
        let mut parser = Parser::new(&config());
        let mut reply = Reply::default();
        let mut sink = |_: Delta| CancelResult::Continue;
        let events = [
            json!({"type": "start"}),
            json!({"type": "reasoning-start", "id": "reasoning-0"}),
            json!({"type": "reasoning-delta", "id": "reasoning-0", "text": "hmm"}),
            json!({"type": "reasoning-end", "id": "reasoning-0"}),
            json!({"type": "text-start", "id": "txt-0"}),
            json!({"type": "text-delta", "id": "txt-0", "text": "reading "}),
            json!({"type": "tool-input-start", "id": "c1", "toolName": "read"}),
            json!({"type": "tool-input-delta", "id": "c1", "delta": "{\"path\":"}),
            json!({"type": "tool-input-end", "id": "c1"}),
            json!({"type": "tool-call", "toolCallId": "c1", "toolName": "read",
                   "input": {"path": "a"}}),
            json!({"type": "text-end", "id": "txt-0"}),
            json!({"type": "finish", "finishReason": "stop",
                "totalUsage": {"inputTokens": 50, "inputTokenDetails": {"cacheReadTokens": 5},
                "outputTokens": 7, "outputTokenDetails": {"reasoningTokens": 2}}}),
        ];
        for event in &events {
            parser.event(event, &mut reply, &mut sink).unwrap();
        }
        assert_eq!(reply.text, "reading ");
        assert_eq!(reply.reasoning, "hmm");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "c1".into(),
                name: "read".into(),
                arguments: "{\"path\":\"a\"}".into(),
            }]
        );
        assert_eq!(
            reply.usage,
            Usage {
                prompt_tokens: 50,
                completion_tokens: 7,
                cached_tokens: 5,
                reasoning_tokens: 2,
            }
        );
        parser.finish(&mut reply).unwrap();
    }

    #[test]
    fn a_tool_call_is_assembled_from_the_pieces_it_streamed() {
        let mut parser = Parser::new(&config());
        let mut reply = Reply::default();
        let mut pieces: Vec<(usize, String, String, String)> = Vec::new();
        let mut sink = |delta: Delta| {
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
        };
        let events = [
            json!({"type": "tool-input-start", "id": "c1", "toolName": "bash"}),
            json!({"type": "tool-input-delta", "id": "c1", "delta": "{\"command\":"}),
            json!({"type": "tool-input-delta", "id": "c1", "delta": "\"ls\"}"}),
            json!({"type": "tool-input-end", "id": "c1"}),
            json!({"type": "tool-call", "toolCallId": "c1", "toolName": "bash",
                   "input": {"command": "ls"}}),
            json!({"type": "finish", "finishReason": "tool-calls"}),
        ];
        for event in &events {
            parser.event(event, &mut reply, &mut sink).unwrap();
        }
        parser.finish(&mut reply).unwrap();
        assert_eq!(reply.text, "");
        assert_eq!(reply.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "c1".into(),
                name: "bash".into(),
                arguments: "{\"command\":\"ls\"}".into(),
            }]
        );
        assert_eq!(
            pieces,
            vec![
                (
                    0,
                    "c1".to_string(),
                    "bash".to_string(),
                    "{\"command\":".to_string()
                ),
                (
                    0,
                    "c1".to_string(),
                    "bash".to_string(),
                    "\"ls\"}".to_string()
                ),
            ],
            "each delta carries its piece, and the whole input repeats none of them"
        );
    }

    #[test]
    fn a_call_with_no_deltas_lands_whole() {
        let mut parser = Parser::new(&config());
        let mut reply = Reply::default();
        let mut pieces: Vec<String> = Vec::new();
        let mut sink = |delta: Delta| {
            if let Delta::Tool { arguments, .. } = delta {
                pieces.push(arguments.to_string());
            }
            CancelResult::Continue
        };
        parser
            .event(
                &json!({"type": "tool-call", "toolCallId": "c1", "toolName": "bash",
                        "input": {"command": "ls"}}),
                &mut reply,
                &mut sink,
            )
            .unwrap();
        parser
            .event(
                &json!({"type": "finish", "finishReason": "tool-calls"}),
                &mut reply,
                &mut sink,
            )
            .unwrap();
        parser.finish(&mut reply).unwrap();
        assert_eq!(reply.tool_calls[0].arguments, "{\"command\":\"ls\"}");
        // A delta for a call that was never opened has no slot to land in.
        parser
            .event(
                &json!({"type": "tool-input-delta", "id": "zz", "delta": "{}"}),
                &mut reply,
                &mut sink,
            )
            .unwrap();
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(pieces, vec!["{\"command\":\"ls\"}".to_string()]);
    }

    #[test]
    fn a_call_whose_input_never_arrives_lands_as_an_empty_object() {
        let mut parser = Parser::new(&config());
        let mut reply = Reply::default();
        let mut sink = |_: Delta| CancelResult::Continue;
        parser
            .event(
                &json!({"type": "tool-input-start", "id": "c1", "toolName": "bash"}),
                &mut reply,
                &mut sink,
            )
            .unwrap();
        parser
            .event(
                &json!({"type": "finish", "finishReason": "tool-calls"}),
                &mut reply,
                &mut sink,
            )
            .unwrap();
        parser.finish(&mut reply).unwrap();
        assert_eq!(reply.tool_calls[0].arguments, "{}");

        let mut torn = Parser::new(&config());
        let mut reply = Reply::default();
        torn.event(
            &json!({"type": "tool-input-start", "id": "c1", "toolName": "bash"}),
            &mut reply,
            &mut sink,
        )
        .unwrap();
        torn.event(
            &json!({"type": "tool-input-delta", "id": "c1", "delta": "{\"command\""}),
            &mut reply,
            &mut sink,
        )
        .unwrap();
        torn.event(
            &json!({"type": "finish", "finishReason": "tool-calls"}),
            &mut reply,
            &mut sink,
        )
        .unwrap();
        torn.finish(&mut reply).unwrap();
        assert_eq!(
            reply.tool_calls[0].arguments, "{}",
            "torn input is not runnable"
        );
    }

    #[test]
    fn mapped_tool_names_go_out_and_come_back() {
        let mut named = config();
        named.tool_names = Some(BTreeMap::from([(
            "bash".to_string(),
            "run_command".to_string(),
        )]));
        let prepared = prepare(
            &named,
            &[json!({"role": "user", "content": "go"})],
            &[json!({"function": {"name": "bash", "description": "R", "parameters": {}}})],
            "k",
        )
        .unwrap();
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();
        assert_eq!(body["params"]["tools"][0]["name"], "run_command");

        let mut parser = Parser::new(&named);
        let mut reply = Reply::default();
        let mut sink = |_: Delta| CancelResult::Continue;
        let events = [
            json!({"type": "tool-input-start", "id": "c9", "toolName": "run_command"}),
            json!({"type": "tool-input-delta", "id": "c9", "delta": "{\"command\":\"ls\"}"}),
            json!({"type": "tool-call", "toolCallId": "c9", "toolName": "run_command",
                   "input": {"command": "ls"}}),
            json!({"type": "finish", "finishReason": "tool-calls"}),
        ];
        for event in &events {
            parser.event(event, &mut reply, &mut sink).unwrap();
        }
        parser.finish(&mut reply).unwrap();
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "c9".into(),
                name: "bash".into(),
                arguments: "{\"command\":\"ls\"}".into(),
            }],
            "the call comes back under our own name"
        );
    }

    #[test]
    fn usage_comes_from_the_terminating_event_either_way() {
        let step = json!({"type": "finish-step", "finishReason": "tool-calls",
            "usage": {"inputTokens": 3, "cachedInputTokens": 1, "outputTokens": 4,
                      "reasoningTokens": 2}});
        let whole = json!({"type": "finish", "rawFinishReason": "end_turn",
            "totalUsage": {"inputTokens": 9, "outputTokens": 5}});
        for (event, reason, usage) in [
            (
                step,
                "tool_calls",
                Usage {
                    prompt_tokens: 3,
                    completion_tokens: 4,
                    cached_tokens: 1,
                    reasoning_tokens: 2,
                },
            ),
            (
                whole,
                "stop",
                Usage {
                    prompt_tokens: 9,
                    completion_tokens: 5,
                    cached_tokens: 0,
                    reasoning_tokens: 0,
                },
            ),
        ] {
            let mut parser = Parser::new(&config());
            let mut reply = Reply::default();
            let mut sink = |_: Delta| CancelResult::Continue;
            parser.event(&event, &mut reply, &mut sink).unwrap();
            assert_eq!(reply.finish_reason.as_deref(), Some(reason));
            assert_eq!(reply.usage, usage);
        }
    }

    #[test]
    fn an_error_event_is_fatal_only_when_the_allowance_ends() {
        let mut sink = |_: Delta| CancelResult::Continue;
        let mut error = |event: Value| {
            let mut parser = Parser::new(&config());
            parser
                .event(&event, &mut Reply::default(), &mut sink)
                .unwrap_err()
        };
        let fatal = error(json!({"type": "error", "message": "premium_credits_exhausted"}));
        assert!(
            !fatal.retryable && fatal.kind == ErrorKind::Quota,
            "{fatal:?}"
        );
        let fatal =
            error(json!({"type": "error", "code": "invalid_request_error", "message": "bad"}));
        assert!(!fatal.retryable, "{fatal:?}");
        let paced = error(json!({"type": "error", "statusCode": 429, "message": "slow"}));
        assert!(
            paced.retryable && paced.kind == ErrorKind::RateLimited,
            "{paced:?}"
        );
        assert!(error(json!({"type": "error", "message": "boom"})).retryable);
        assert!(error(json!({"type": "abort"})).retryable);
        assert!(
            Parser::new(&config())
                .finish(&mut Reply::default())
                .unwrap_err()
                .retryable
        );
    }

    #[test]
    fn a_cancelled_delta_ends_the_stream() {
        let mut parser = Parser::new(&config());
        let mut reply = Reply::default();
        let failure = parser
            .event(
                &json!({"type": "text-delta", "text": "hi"}),
                &mut reply,
                &mut |_| CancelResult::Cancel,
            )
            .unwrap_err();
        // The transport discards a cancelled attempt's whole reply, so what the
        // parser had already appended never reaches history.
        assert!(!failure.retryable, "{failure:?}");
    }

    #[test]
    fn the_thread_is_stable_per_session_and_today_is_a_date() {
        assert_eq!(thread_id("k"), thread_id("k"));
        assert_ne!(thread_id("k"), thread_id("j"));
        let id = thread_id("k");
        assert_eq!(id.len(), 36, "{id}");
        assert_eq!(&id[14..15], "4", "version 4 bits: {id}");
        assert!(
            matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
            "variant bits: {id}"
        );
        let date = today();
        assert_eq!(date.len(), 10, "{date}");
        assert!(date.starts_with("202"), "{date}");
        let environment = environment();
        assert!(
            ["darwin", "windows", "linux"].contains(&environment),
            "{environment}"
        );
    }

    // ---------------------------------------------------------------------------
    // The wire, over a socket
    // ---------------------------------------------------------------------------

    /// One canned answer: the status, the content type, and the body written one
    /// chunk at a time so a line can be split across two writes.
    struct Canned {
        status: u16,
        content_type: &'static str,
        chunks: Vec<String>,
    }

    impl Canned {
        fn status(status: u16) -> Self {
            Canned {
                status,
                content_type: "application/json",
                chunks: vec![r#"{"error":"slow down"}"#.to_string()],
            }
        }

        fn ndjson(chunks: Vec<String>) -> Self {
            Canned {
                status: 200,
                content_type: "application/x-ndjson",
                chunks,
            }
        }
    }

    /// A local server answering each connection with the next canned reply and
    /// closing, and counting the requests it saw and keeping their bodies.
    async fn serve(canned: Vec<Canned>) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(AtomicUsize::new(0));
        let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let counter = Arc::clone(&seen);
        let recorder = Arc::clone(&bodies);
        tokio::spawn(async move {
            for answer in canned {
                let Ok((mut stream, _)) = listener.accept().await else {
                    continue;
                };
                let mut request = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(read) = stream.read(&mut chunk).await else {
                        break;
                    };
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..read]);
                    let text = String::from_utf8_lossy(&request);
                    let Some(end) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let length = text
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        recorder.lock().unwrap().push(
                            String::from_utf8_lossy(&request[end + 4..end + 4 + length])
                                .to_string(),
                        );
                        break;
                    }
                }
                counter.fetch_add(1, Ordering::SeqCst);
                let body = answer.chunks.concat();
                let head = format!(
                    "HTTP/1.1 {} X\r\ncontent-type: {}\r\ncontent-length: {}\r\n\
                     connection: close\r\n\r\n",
                    answer.status,
                    answer.content_type,
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                for piece in &answer.chunks {
                    let _ = stream.write_all(piece.as_bytes()).await;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        });
        (url, seen, bodies)
    }

    /// Reads the NDJSON lines one reply streamed and hands each whole object to the
    /// parser: the line loop the transport runs for this protocol.
    async fn read_lines(
        response: &mut reqwest::Response,
        parser: &mut Parser,
        reply: &mut Reply,
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<(), Failure> {
        let mut pending: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            Failure::of(
                ErrorKind::Transport,
                None,
                true,
                format!("stream interrupted: {error}"),
            )
        })? {
            pending.extend_from_slice(&chunk);
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                let data = String::from_utf8_lossy(&line);
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                let event: Value = serde_json::from_str(data).map_err(|error| {
                    Failure::of(
                        ErrorKind::BadResponse,
                        None,
                        true,
                        format!("malformed NDJSON: {error}"),
                    )
                })?;
                parser.event(&event, reply, on_delta)?;
            }
        }
        if !pending.iter().all(|byte| byte.is_ascii_whitespace()) {
            return Err(Failure::of(
                ErrorKind::BadResponse,
                None,
                true,
                "stream ended with unconsumed data".into(),
            ));
        }
        parser.finish(reply)
    }

    /// One attempt against the local server, classified the way the transport does.
    async fn attempt(
        http: &reqwest::Client,
        url: &str,
        parser: &mut Parser,
        tools: &[Value],
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<Reply, Failure> {
        let prepared = prepare(
            &config(),
            &[json!({"role": "user", "content": "hi"})],
            tools,
            "k",
        )
        .unwrap();
        let mut request = http.post(url).body(prepared.body.clone());
        for (name, value) in &prepared.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let mut response = request
            .send()
            .await
            .map_err(|error| Failure::transient(format!("request failed: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(Failure::from_status(
                Some(status.as_u16()),
                format!("HTTP {status}: {text}"),
            ));
        }
        let mut reply = Reply::default();
        read_lines(&mut response, parser, &mut reply, on_delta).await?;
        Ok(reply)
    }

    /// Every delta a stream carried, as the (index, id, name, arguments) pieces.
    fn collect(delta: Delta, pieces: &mut Vec<(usize, String, String, String)>) -> CancelResult {
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
    }

    #[tokio::test]
    async fn the_ndjson_stream_comes_back_over_a_socket() {
        let lines = [
            r#"{"type":"start"}"#,
            r#"{"type":"reasoning-start","id":"r0"}"#,
            r#"{"type":"reasoning-delta","id":"r0","text":"hmm"}"#,
            r#"{"type":"reasoning-end","id":"r0"}"#,
            r#"{"type":"text-start","id":"t0"}"#,
            r#"{"type":"text-delta","id":"t0","text":"reading "}"#,
            r#"{"type":"tool-input-start","id":"c1","toolName":"read"}"#,
            r#"{"type":"tool-input-delta","id":"c1","delta":"{\"path\":"}"#,
            r#"{"type":"tool-input-end","id":"c1"}"#,
            r#"{"type":"tool-call","toolCallId":"c1","toolName":"read","input":{"path":"a"}}"#,
            concat!(
                r#"{"type":"finish","finishReason":"stop","totalUsage":{"inputTokens":50,"#,
                r#""inputTokenDetails":{"cacheReadTokens":5},"outputTokens":7,"#,
                r#""outputTokenDetails":{"reasoningTokens":2}}}"#
            ),
        ];
        // One object split across two writes, so the line buffer is exercised.
        let chunks = vec![
            format!("{}\n{}\n", lines[0], lines[1]),
            format!("{}\n{}\n", lines[2], lines[3]),
            format!("{}\n{}\n", lines[4], lines[5]),
            lines[6][..20].to_string(),
            format!("{}\n", &lines[6][20..]),
            format!("{}\n{}\n", lines[7], lines[8]),
            format!("{}\n", lines[9]),
            format!("{}\n", lines[10]),
        ];
        let (url, seen, _) = serve(vec![Canned::ndjson(chunks)]).await;
        let http = reqwest::Client::new();
        let mut parser = Parser::new(&config());
        let reply = attempt(&http, &url, &mut parser, &[], &mut |_: Delta| {
            CancelResult::Continue
        })
        .await
        .unwrap();

        assert_eq!(seen.load(Ordering::SeqCst), 1);
        assert_eq!(reply.text, "reading ");
        assert_eq!(reply.reasoning, "hmm");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            reply.tool_calls,
            vec![ToolCall {
                id: "c1".into(),
                name: "read".into(),
                arguments: "{\"path\":\"a\"}".into(),
            }]
        );
        assert_eq!(
            reply.usage,
            Usage {
                prompt_tokens: 50,
                completion_tokens: 7,
                cached_tokens: 5,
                reasoning_tokens: 2,
            }
        );
    }

    #[tokio::test]
    async fn text_and_several_tool_calls_come_back_over_the_socket() {
        let lines = [
            r#"{"type":"text-start","id":"t0"}"#,
            r#"{"type":"text-delta","id":"t0","text":"on it"}"#,
            r#"{"type":"tool-input-start","id":"c1","toolName":"bash"}"#,
            r#"{"type":"tool-input-delta","id":"c1","delta":"{\"command\":\"ls\"}"}"#,
            r#"{"type":"tool-input-start","id":"c2","toolName":"read"}"#,
            r#"{"type":"tool-input-delta","id":"c2","delta":"{\"path\":\"a\"}"}"#,
            r#"{"type":"tool-input-end","id":"c1"}"#,
            r#"{"type":"tool-call","toolCallId":"c1","toolName":"bash","input":{"command":"ls"}}"#,
            r#"{"type":"tool-call","toolCallId":"c2","toolName":"read","input":{"path":"a"}}"#,
            r#"{"type":"text-end","id":"t0"}"#,
            concat!(
                r#"{"type":"finish","finishReason":"tool-calls","totalUsage":{"#,
                r#""inputTokens":50,"outputTokens":7}}"#
            ),
        ];
        // One object split across two writes, so the line buffer is exercised.
        let chunks = vec![
            format!("{}\n{}\n", lines[0], lines[1]),
            format!("{}\n{}\n", lines[2], lines[3]),
            lines[4][..20].to_string(),
            format!("{}\n", &lines[4][20..]),
            format!("{}\n{}\n", lines[5], lines[6]),
            format!("{}\n{}\n", lines[7], lines[8]),
            format!("{}\n{}\n", lines[9], lines[10]),
        ];
        let (url, seen, bodies) = serve(vec![Canned::ndjson(chunks)]).await;
        let http = reqwest::Client::new();
        let mut parser = Parser::new(&config());
        let mut pieces = Vec::new();
        let reply = attempt(&http, &url, &mut parser, &tools(), &mut |delta| {
            collect(delta, &mut pieces)
        })
        .await
        .unwrap();

        assert_eq!(seen.load(Ordering::SeqCst), 1);
        let body: Value = serde_json::from_str(&bodies.lock().unwrap()[0]).unwrap();
        assert_eq!(
            body["params"]["tools"],
            json!([
                {"name": "bash", "description": "Run a command",
                 "input_schema": {"type": "object",
                                  "properties": {"command": {"type": "string"}}}},
                {"name": "read", "description": "Read a file",
                 "input_schema": {"type": "object"}},
            ]),
            "the tools the model was asked for ride along"
        );
        assert_eq!(reply.text, "on it", "the text and the calls both survive");
        assert_eq!(reply.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(
            reply.tool_calls,
            vec![
                ToolCall {
                    id: "c1".into(),
                    name: "bash".into(),
                    arguments: "{\"command\":\"ls\"}".into(),
                },
                ToolCall {
                    id: "c2".into(),
                    name: "read".into(),
                    arguments: "{\"path\":\"a\"}".into(),
                },
            ]
        );
        assert_eq!(
            pieces,
            vec![
                (
                    0,
                    "c1".to_string(),
                    "bash".to_string(),
                    "{\"command\":\"ls\"}".to_string()
                ),
                (
                    1,
                    "c2".to_string(),
                    "read".to_string(),
                    "{\"path\":\"a\"}".to_string()
                ),
            ],
            "each call's delta names the slot it belongs to"
        );
    }

    #[tokio::test]
    async fn a_rate_limited_answer_is_retried_with_backoff_and_then_succeeds() {
        let good = concat!(
            r#"{"type":"text-delta","text":"done"}"#,
            "\n",
            concat!(
                r#"{"type":"finish","finishReason":"stop","totalUsage":{"inputTokens":2,"#,
                r#""outputTokens":3}}"#
            ),
            "\n"
        );
        let (url, seen, _) = serve(vec![
            Canned::status(429),
            Canned::status(500),
            Canned::ndjson(vec![good.to_string()]),
        ])
        .await;
        let http = reqwest::Client::new();
        let mut parser = Parser::new(&config());
        let backoff = Duration::from_millis(25);
        let started = Instant::now();
        let mut reply = attempt(&http, &url, &mut parser, &[], &mut |_: Delta| {
            CancelResult::Continue
        })
        .await;
        while let Err(failure) = reply {
            assert!(failure.retryable, "{failure:?}");
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "the retry loop ran away"
            );
            tokio::time::sleep(backoff).await;
            reply = attempt(&http, &url, &mut parser, &[], &mut |_: Delta| {
                CancelResult::Continue
            })
            .await;
        }
        let reply = reply.unwrap();
        assert_eq!(reply.text, "done");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(
            reply.usage,
            Usage {
                prompt_tokens: 2,
                completion_tokens: 3,
                cached_tokens: 0,
                reasoning_tokens: 0,
            },
            "usage from the third attempt"
        );
        assert_eq!(seen.load(Ordering::SeqCst), 3);
        assert!(
            started.elapsed() >= backoff * 2,
            "two backoffs were waited out"
        );
    }

    #[tokio::test]
    async fn a_non_retryable_answer_is_not_retried() {
        let (url, seen, _) = serve(vec![Canned::status(400)]).await;
        let http = reqwest::Client::new();
        let mut parser = Parser::new(&config());
        let failure = attempt(&http, &url, &mut parser, &[], &mut |_: Delta| {
            CancelResult::Continue
        })
        .await
        .unwrap_err();
        assert!(!failure.retryable, "{failure:?}");
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_stream_with_no_finish_event_fails() {
        let (url, _, _) = serve(vec![Canned::ndjson(vec![
            r#"{"type":"text-delta","text":"half"}"#.to_string(),
        ])])
        .await;
        let http = reqwest::Client::new();
        let mut parser = Parser::new(&config());
        let failure = attempt(&http, &url, &mut parser, &[], &mut |_: Delta| {
            CancelResult::Continue
        })
        .await
        .unwrap_err();
        assert!(failure.retryable, "{failure:?}");
        assert_eq!(failure.kind, ErrorKind::BadResponse);
    }
}
