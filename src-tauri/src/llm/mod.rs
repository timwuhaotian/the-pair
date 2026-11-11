//! Provider clients: one streaming request with retries, in each provider's
//! wire format. History is kept in OpenAI chat form and converted the same way
//! on every request, so the bytes sent for earlier messages never change.
//!
//! One `complete` streams one reply. For OpenAI Chat Completions:
//!
//! ```text
//! POST https://api.openai.com/v1/chat/completions
//! authorization: Bearer sk-…
//! {"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"stream":true,
//!  "stream_options":{"include_usage":true},"prompt_cache_key":"pair-1-mentor"}
//!
//! data: {"choices":[{"delta":{"content":"Hel"}}]}
//! data: {"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}],
//!        "usage":{"prompt_tokens":12,"completion_tokens":3}}
//! data: [DONE]
//! ```
//!
//! Every attempt first waits out a 429 cooldown shared by every client on the
//! same account, so two pairs on it pace themselves together instead of each
//! burning its own retries on the same cap. A failed attempt's partial output is
//! discarded, so history only ever holds complete replies.
//!
//! # Protocol modules
//!
//! Each module in this directory implements the same two halves:
//!
//! - `prepare(&ClientConfig, &[Value], &str) -> Result<Prepared, Failure>` builds
//!   the request for one turn;
//! - `Parser::new(&ClientConfig)` returns a [`StreamParser`] that reads the
//!   stream back into a [`Reply`].
//!
//! Every protocol but Command Code answers an SSE event stream; Command Code
//! answers NDJSON, one bare JSON object per line, which the same line loop reads.

mod anthropic;
mod commandcode;
mod error;
mod history;
mod openai;
mod responses;
mod tokens;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::accounts::types::{HeaderPair, Protocol};

use error::{
    backoff, body_excerpt, classify, error_chain, excerpt, transport_failure, CANCELLED,
    LONGEST_WAIT,
};
pub use error::{ErrorKind, ProviderError};

/// Requests one `complete` may send in all.
const MAX_ATTEMPTS: u32 = 10;

/// Requests that never reach a response head stop early: a refused port or an
/// unresolvable host won't heal in the time ten attempts take.
const TRANSPORT_ATTEMPTS: u32 = 3;

/// Requests one `complete` may send in all, however many of its waits were
/// shared with other pairs, so peers pushing the cooldown back can't keep it
/// alive forever.
const HARD_ATTEMPTS: u32 = 2 * MAX_ATTEMPTS;

/// The longest one request may spend waiting between attempts in all, every
/// backoff and every shared 429 cooldown together. A limit that hasn't cleared
/// by then isn't going to, and pairs share one client, so their waits add up to
/// a turn that looks stopped.
const MAX_WAIT: Duration = Duration::from_secs(180);

/// How long a connection may take to open before the request fails.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Body bytes kept from a response, enough for an error message's excerpt.
const EXCERPT_BYTES: usize = 8_192;

/// How The Pair names itself to the servers it calls.
const USER_AGENT: &str = concat!("the-pair/", env!("CARGO_PKG_VERSION"));

/// One (protocol, base_url, account) triple, with everything a turn needs that
/// the caller resolved before the request: the credential, the model, and the
/// route's limits.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub protocol: Protocol,
    /// Endpoint root, without a trailing slash (`https://api.openai.com/v1`).
    pub base_url: String,
    /// The resolved credential, never the name of an environment variable.
    pub api_key: String,
    /// Extra headers the account needs (an opencode-go session header, say).
    pub headers: Vec<HeaderPair>,
    /// Tool-name overrides for gateways that rename them, where the account
    /// carries any (Muse Code). Our name → the name the gateway expects.
    pub tool_names: Option<BTreeMap<String, String>>,
    pub model: String,
    /// The route's own output limit, when it names one.
    pub max_output_tokens: Option<u64>,
    /// The reasoning effort the route selected, where the model takes one.
    pub reasoning_effort: Option<String>,
    /// How long one request may take in all, stream included.
    pub timeout: Duration,
}

/// One tool call the model asked for. `arguments` is the raw JSON text, which
/// the tool layer parses; a provider that streams it arrives in pieces.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    /// The Pair's own name for the tool, never the wire name.
    pub name: String,
    pub arguments: String,
}

/// What a streamed chunk carried.
pub enum Delta<'a> {
    Text(&'a str),
    Reasoning(&'a str),
    /// A piece of a tool call's arguments. `index` is the call's position in the
    /// reply, so the pieces of one call can be told from another's; the rest of
    /// the call arrives whole in [`Reply::tool_calls`], which is what the tool
    /// loop runs.
    Tool {
        index: usize,
        #[allow(dead_code)]
        id: &'a str,
        name: &'a str,
        #[allow(dead_code)]
        arguments: &'a str,
    },
    Retry {
        attempt: u32,
        wait: Duration,
        reason: &'a str,
    },
    /// The attempt that was streaming is being thrown away, so everything it
    /// delivered is not part of the reply: the caller drops what it showed before
    /// the retry's deltas arrive, and a discarded attempt is never shown twice.
    Discard,
}

/// What the caller answers a delta with. A paused or killed turn cancels, and
/// the stream stops where it is instead of being read to the end.
pub enum CancelResult {
    Continue,
    Cancel,
}

/// Token counts as the provider reported them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// Prompt tokens served from the provider's cache.
    pub cached_tokens: u64,
    /// Tokens the model spent reasoning, where it reports them.
    pub reasoning_tokens: u64,
}

/// One completed reply. A failed attempt's partial output is discarded, so a
/// reply always holds a whole turn.
#[derive(Debug, Default, Clone)]
pub struct Reply {
    pub text: String,
    pub reasoning: String,
    /// Normalized to the provider's own word: `stop`, `length`, `tool_calls`.
    pub finish_reason: Option<String>,
    pub usage: Usage,
    /// The provider's session, where the protocol has one.
    pub session_id: Option<String>,
    /// Tool calls the model asked for, in the order it made them.
    pub tool_calls: Vec<ToolCall>,
}

impl Reply {
    /// Tool-call arguments that aren't valid JSON become `{}`: whatever a provider
    /// streamed, the stored history must stay replayable, or the call is sent back
    /// on every later request and fails each one the same way. Every parser calls
    /// this as its stream finishes.
    pub fn repair_call_arguments(&mut self) {
        for call in &mut self.tool_calls {
            if !arguments_valid(&call.arguments) {
                call.arguments = "{}".into();
            }
        }
    }
}

/// The 429 deadline every request on one account waits out. Every client built
/// for the same endpoint and credential holds the same one — see
/// [`cooldown_for`] — so a rate limit paces two pairs on that account together
/// instead of each burning its own retries on the same cap.
type Cooldown = Arc<Mutex<Option<Instant>>>;

/// The cooldown of the account `config` names: one per (protocol, endpoint,
/// credential), for as long as it is in use or still counting down.
///
/// A turn builds a fresh [`Client`] for every attempt, so a cooldown owned by
/// the client would be new each time — shared with nothing, and gone before the
/// next turn on the account could observe the wait a 429 had just named.
fn cooldown_for(config: &ClientConfig) -> Cooldown {
    use std::collections::hash_map::DefaultHasher;
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};

    static COOLDOWNS: Mutex<Option<HashMap<u64, Cooldown>>> = Mutex::new(None);

    let mut hasher = DefaultHasher::new();
    (config.protocol as u8).hash(&mut hasher);
    config
        .base_url
        .trim()
        .trim_end_matches('/')
        .to_ascii_lowercase()
        .hash(&mut hasher);
    config.api_key.hash(&mut hasher);
    let key = hasher.finish();

    let mut guard = COOLDOWNS.lock().unwrap_or_else(|e| e.into_inner());
    let cooldowns = guard.get_or_insert_with(HashMap::new);
    // An entry nobody holds and whose wait has passed says nothing any more. A
    // refreshed sign-in is a new credential, so without this the map would grow
    // by one entry per refresh.
    let now = Instant::now();
    cooldowns.retain(|other, cooldown| {
        *other == key
            || Arc::strong_count(cooldown) > 1
            || cooldown_deadline(cooldown).is_some_and(|deadline| deadline > now)
    });
    Arc::clone(cooldowns.entry(key).or_default())
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    config: ClientConfig,
    cooldown: Cooldown,
    /// How long one request may wait between attempts in all.
    wait_budget: Duration,
}

/// The bytes of a chunked event stream, drained one `\n`-terminated line at a
/// time. Bytes already scanned with no newline in them are not scanned again, so
/// a multi-megabyte line arriving in many chunks costs one pass, not one per
/// chunk.
#[derive(Default)]
struct SseLines {
    buffer: Vec<u8>,
    scan: usize,
}

impl SseLines {
    fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    /// The next complete line, without its newline, or `None` when no newline
    /// arrived yet.
    fn next_line(&mut self) -> Option<Vec<u8>> {
        let Some(rel) = self.buffer[self.scan..].iter().position(|&b| b == b'\n') else {
            self.scan = self.buffer.len();
            return None;
        };
        let end = self.scan + rel;
        let line: Vec<u8> = self.buffer.drain(..=end).collect();
        self.scan = 0;
        Some(line)
    }

    /// What arrived after the last newline: checked for a torn tail once the
    /// stream ends.
    fn rest(&mut self) -> &[u8] {
        self.scan = self.buffer.len();
        &self.buffer
    }
}

/// A request ready to send: where, what, and with which headers.
struct Prepared {
    url: String,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
}

/// The state a protocol keeps while it reads one streamed reply.
/// A protocol's event-stream reader. `Send` because a turn runs on its own
/// task, so the parser lives across awaits on a worker thread.
trait StreamParser: Send {
    fn event(
        &mut self,
        event: &Value,
        reply: &mut Reply,
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<(), Failure>;
    fn done(&mut self) {}
    fn finish(&mut self, reply: &mut Reply) -> Result<(), Failure>;
}

/// One failed attempt, classified where the response was read.
#[derive(Debug)]
struct Failure {
    kind: ErrorKind,
    status: Option<u16>,
    retryable: bool,
    retry_after: Option<Duration>,
    /// A retry budget smaller than `MAX_ATTEMPTS`, for failures that won't heal
    /// in time.
    max_attempts: Option<u32>,
    /// Whether the caller cancelled the stream, which is never retried.
    cancelled: bool,
    message: String,
}

impl Failure {
    pub(super) fn of(
        kind: ErrorKind,
        status: Option<u16>,
        retryable: bool,
        message: String,
    ) -> Self {
        Self {
            kind,
            status,
            retryable,
            retry_after: None,
            max_attempts: None,
            cancelled: false,
            message,
        }
    }

    /// A rejection no retry can heal.
    pub(super) fn fatal(message: String) -> Self {
        Self::of(ErrorKind::Other, None, false, message)
    }

    /// A failure that a retry can heal.
    pub(super) fn transient(message: String) -> Self {
        Self::of(ErrorKind::Other, None, true, message)
    }

    /// A failure read off an HTTP status, mid-stream.
    pub(super) fn from_status(status: Option<u16>, message: String) -> Self {
        let retryable = status
            .and_then(|status| reqwest::StatusCode::from_u16(status).ok())
            .is_some_and(error::retryable_status);
        Self::of(error::kind_for_status(status), status, retryable, message)
    }

    /// The caller cancelled the stream: nothing is wrong with the provider.
    pub(super) fn cancelled() -> Self {
        Self {
            cancelled: true,
            ..Self::of(ErrorKind::Other, None, false, CANCELLED.to_string())
        }
    }
}

/// Hands one delta to the caller and turns a cancellation into a failure.
/// The name a tool is sent under: the account's own mapping, when the gateway
/// renames it (Muse Code refuses `bash` and answers `run_command`).
pub(super) fn wire_tool_name(config: &ClientConfig, name: &str) -> String {
    config
        .tool_names
        .as_ref()
        .and_then(|names| names.get(name))
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

/// The name a tool call came back under, mapped back to The Pair's own.
pub(super) fn our_tool_name(config: &ClientConfig, name: &str) -> String {
    let Some(names) = config.tool_names.as_ref() else {
        return name.to_string();
    };
    names
        .iter()
        .find(|(_, wire)| wire.as_str() == name)
        .map(|(ours, _)| ours.clone())
        .unwrap_or_else(|| name.to_string())
}

fn emit(
    on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    delta: Delta,
) -> Result<(), Failure> {
    match on_delta(delta) {
        CancelResult::Continue => Ok(()),
        CancelResult::Cancel => Err(Failure::cancelled()),
    }
}

impl Client {
    /// Builds a client for one (protocol, base_url, account) triple.
    pub fn new(config: ClientConfig) -> Result<Self, String> {
        if config.base_url.trim().is_empty() {
            return Err("the provider has no base URL".to_string());
        }
        if config.timeout.is_zero() {
            return Err("the provider has no request timeout".to_string());
        }
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(config.timeout)
            .build()
            .map_err(|error| format!("could not build an HTTP client: {}", error))?;
        Ok(Self {
            http,
            cooldown: cooldown_for(&config),
            config,
            wait_budget: MAX_WAIT,
        })
    }

    /// Sends one request, retrying transient failures. `on_delta` sees every
    /// streamed chunk and every retry; answering [`CancelResult::Cancel`] stops
    /// the stream where it is and returns a cancelled error.
    pub async fn complete(
        &self,
        messages: &[Value],
        tools: &[Value],
        cache_key: &str,
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<Reply, ProviderError> {
        let prepared = self.prepare(messages, tools, cache_key)?;
        let mut attempt = 1u32;
        let mut burns = 0u32;
        let mut mine: Option<Instant> = None;
        let mut waited = Duration::ZERO;
        loop {
            let (slept, shared) = self.wait_cooldown(mine).await;
            waited += slept;
            let failure = match self.attempt(&prepared, on_delta).await {
                Ok(reply) => return Ok(reply),
                Err(failure) => failure,
            };
            if failure.cancelled {
                return Err(ProviderError::cancelled(attempt));
            }
            if !shared {
                burns += 1;
            }
            let cap = failure.max_attempts.unwrap_or(MAX_ATTEMPTS);
            let spent = if failure.max_attempts.is_some() {
                attempt
            } else {
                burns
            };
            if !failure.retryable
                || spent >= cap
                || attempt >= HARD_ATTEMPTS
                || waited >= self.wait_budget
            {
                // Even a final 429 paces the next call on this client: the
                // server just named a wait, and a clone starting now should
                // observe it. The wait is capped, so a gateway that names an
                // hour cannot hold every later call on this client for an hour.
                if failure.status == Some(429) {
                    self.set_cooldown(published_wait(failure.retry_after, attempt));
                }
                return Err(ProviderError {
                    status: failure.status,
                    kind: failure.kind,
                    message: failure.message,
                    attempts: attempt,
                    retryable: failure.retryable,
                    advice: None,
                });
            }
            let wait = failure.retry_after.unwrap_or_else(|| backoff(attempt));
            // The attempt being left behind delivered output that is not part of
            // the reply, so the caller is told to drop it before the retry's
            // deltas arrive.
            for delta in [
                Delta::Discard,
                Delta::Retry {
                    attempt,
                    wait,
                    reason: &failure.message,
                },
            ] {
                if emit(on_delta, delta).is_err() {
                    return Err(ProviderError::cancelled(attempt));
                }
            }
            if failure.status == Some(429) {
                let deadline = self.set_cooldown(wait);
                mine = Some(deadline);
            } else {
                tokio::time::sleep(wait).await;
                // The backoff is a wait like any other, so the budget that bounds
                // every wait between attempts bounds this one too.
                waited += wait;
            }
            attempt += 1;
        }
    }

    /// Builds the request for this client's protocol.
    fn prepare(
        &self,
        messages: &[Value],
        tools: &[Value],
        cache_key: &str,
    ) -> Result<Prepared, ProviderError> {
        match self.config.protocol {
            Protocol::OpenAiChat => openai::prepare(&self.config, messages, tools, cache_key),
            Protocol::Anthropic => anthropic::prepare(&self.config, messages, tools, cache_key),
            Protocol::Responses => responses::prepare(&self.config, messages, tools, cache_key),
            Protocol::CommandCode => commandcode::prepare(&self.config, messages, tools, cache_key),
        }
        .map_err(|failure| ProviderError {
            status: failure.status,
            kind: failure.kind,
            message: failure.message,
            attempts: 0,
            retryable: failure.retryable,
            advice: None,
        })
    }

    /// Sleeps until the shared 429 cooldown passes. Returns how long it waited,
    /// and whether this call actually waited out a deadline another pair set;
    /// such an attempt doesn't burn this call's budget.
    async fn wait_cooldown(&self, mine: Option<Instant>) -> (Duration, bool) {
        let Some(shared) = cooldown_deadline(&self.cooldown) else {
            return (Duration::ZERO, false);
        };
        let deadline = mine.filter(|mine| *mine > shared).unwrap_or(shared);
        let now = Instant::now();
        if deadline <= now {
            return (Duration::ZERO, false);
        }
        let wait = deadline - now;
        tokio::time::sleep(wait).await;
        (wait, mine.is_none_or(|mine| deadline > mine))
    }

    /// Names a 429 cooldown every request on this client waits out, and returns
    /// the deadline it set.
    fn set_cooldown(&self, wait: Duration) -> Instant {
        let deadline = Instant::now() + wait;
        let mut guard = match self.cooldown.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = Some(deadline);
        deadline
    }

    /// One request: send it, read the stream back, or classify the failure.
    async fn attempt(
        &self,
        prepared: &Prepared,
        on_delta: &mut (dyn FnMut(Delta) -> CancelResult + Send),
    ) -> Result<Reply, Failure> {
        let mut headers: Vec<(&str, &str)> = prepared
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        headers.extend(
            self.config
                .headers
                .iter()
                .map(|header| (header.name.as_str(), header.value.as_str())),
        );
        let mut request = self.http.post(&prepared.url).body(prepared.body.clone());
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let mut response = request.send().await.map_err(transport_failure)?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = retry_after(&response);
            let text = response.text().await.unwrap_or_default();
            return Err(classify(status, retry_after, &text));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split(';')
                    .next()
                    .unwrap_or(value)
                    .trim()
                    .to_ascii_lowercase()
            });
        // Command Code answers NDJSON under its own content type, not an event
        // stream, so only the other protocols are held to the event-stream gate.
        let ndjson = self.config.protocol == Protocol::CommandCode;
        if !ndjson
            && content_type
                .as_deref()
                .is_some_and(|content_type| content_type != "text/event-stream")
        {
            let body = body_excerpt(&mut response).await;
            let message = format!(
                "HTTP {status}: content-type is {ct}, not an event stream: {}",
                excerpt(&body),
                ct = content_type.as_deref().unwrap_or_default()
            );
            return Err(Failure::of(
                ErrorKind::BadResponse,
                Some(status.as_u16()),
                false,
                message,
            ));
        }

        let mut parser: Box<dyn StreamParser> = match self.config.protocol {
            Protocol::OpenAiChat => Box::new(openai::Parser::new(&self.config)),
            Protocol::Anthropic => Box::new(anthropic::Parser::new(&self.config)),
            Protocol::Responses => Box::new(responses::Parser::new(&self.config)),
            Protocol::CommandCode => Box::new(commandcode::Parser::new(&self.config)),
        };
        let mut reply = Reply::default();
        let mut lines = SseLines::default();
        let mut head: Vec<u8> = Vec::new();
        let mut saw_data = false;
        let interrupted = |error: reqwest::Error| {
            Failure::of(
                ErrorKind::Transport,
                None,
                true,
                format!("stream interrupted: {}", error_chain(&error)),
            )
        };
        'stream: while let Some(chunk) = response.chunk().await.map_err(interrupted)? {
            if head.len() < EXCERPT_BYTES {
                let take = (EXCERPT_BYTES - head.len()).min(chunk.len());
                head.extend_from_slice(&chunk[..take]);
            }
            lines.push(&chunk);
            while let Some(line) = lines.next_line() {
                let line = String::from_utf8_lossy(&line);
                if ndjson {
                    // One whole event per line, with no `data:` prefix: an empty
                    // line is padding, and a line that isn't JSON is a torn or
                    // misrouted answer rather than a stream to keep reading.
                    let data = line.trim();
                    if data.is_empty() {
                        continue;
                    }
                    saw_data = true;
                    let event = parse_ndjson_line(data)?;
                    parser.event(&event, &mut reply, on_delta)?;
                    continue;
                }
                let Some(data) = line.trim().strip_prefix("data:") else {
                    continue;
                };
                saw_data = true;
                let data = data.trim();
                let Some(event) = parse_sse_data(data)? else {
                    parser.done();
                    break 'stream;
                };
                parser.event(&event, &mut reply, on_delta)?;
            }
        }
        if !saw_data {
            let noted = content_type.map_or(String::new(), |content_type| {
                format!(" (content-type {content_type})")
            });
            let message = format!(
                "HTTP {status}: the stream carried no events{noted}: {}",
                excerpt(&String::from_utf8_lossy(&head))
            );
            return Err(Failure::of(
                ErrorKind::BadResponse,
                Some(status.as_u16()),
                false,
                message,
            ));
        }
        ensure_stream_consumed(lines.rest())?;
        parser.finish(&mut reply)?;
        validate_reply(reply)
    }
}

/// The cooldown every request on this client waits out.
fn cooldown_deadline(cooldown: &Cooldown) -> Option<Instant> {
    let guard = match cooldown.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard
}

/// The wait a `retry-after` header names, in seconds. The HTTP-date form some
/// servers send is ignored: waiting out a date would outlive the turn.
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// One `data:` line: the event it carries, or `None` for `[DONE]`.
fn parse_sse_data(data: &str) -> Result<Option<Value>, Failure> {
    if data == "[DONE]" {
        return Ok(None);
    }
    serde_json::from_str(data)
        .map(Some)
        .map_err(|error| Failure::transient(format!("malformed SSE JSON: {error}")))
}

/// One NDJSON line: the whole event it carries.
fn parse_ndjson_line(data: &str) -> Result<Value, Failure> {
    serde_json::from_str(data).map_err(|error| {
        Failure::of(
            ErrorKind::BadResponse,
            None,
            true,
            format!("malformed NDJSON: {error}"),
        )
    })
}

/// The wait a 429 publishes as the cooldown every request on this client waits
/// out: the server's own, capped at the longest wait any one request would sit
/// through, so a gateway that names an hour doesn't stop the client for an hour.
fn published_wait(retry_after: Option<Duration>, attempt: u32) -> Duration {
    retry_after
        .unwrap_or_else(|| backoff(attempt))
        .min(LONGEST_WAIT)
}

/// A stream that ends mid-line is torn, not finished: the tail is retried.
fn ensure_stream_consumed(buffer: &[u8]) -> Result<(), Failure> {
    if buffer.iter().all(|b| b.is_ascii_whitespace()) {
        Ok(())
    } else {
        Err(Failure::transient(
            "stream ended with unconsumed data".into(),
        ))
    }
}

/// Only normal termination and an output-limit reply are usable; in particular,
/// a content filter must not turn a partial or reasoning-only attempt into a
/// committed turn.
fn validate_reply(reply: Reply) -> Result<Reply, Failure> {
    if let Some(reason) = reply.finish_reason.as_deref() {
        if !matches!(reason, "stop" | "length" | "tool_calls") {
            return Err(Failure::fatal(format!(
                "response terminated abnormally: {reason}"
            )));
        }
    }
    if reply.text.trim().is_empty() && reply.finish_reason.is_none() {
        return Err(Failure::transient(match reply.reasoning.is_empty() {
            true => "empty response without a termination reason".into(),
            false => "reasoning-only response without a termination reason".into(),
        }));
    }
    Ok(reply)
}

/// A string cut at `max` bytes, on a character boundary.
fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Whether tool-call arguments are JSON an API accepts back. A provider can end
/// a streamed call without ever sending arguments, and the empty string is not
/// valid JSON.
pub(super) fn arguments_valid(arguments: &str) -> bool {
    serde_json::from_str::<Value>(arguments).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    fn config(base_url: &str) -> ClientConfig {
        ClientConfig {
            protocol: Protocol::OpenAiChat,
            base_url: base_url.into(),
            api_key: "sk-test".into(),
            headers: Vec::new(),
            tool_names: None,
            model: "gpt-4o".into(),
            max_output_tokens: None,
            reasoning_effort: None,
            timeout: Duration::from_secs(10),
        }
    }

    fn client(base_url: &str) -> Client {
        Client::new(config(base_url)).unwrap()
    }

    /// A client for the Command Code gateway, whose answers are NDJSON.
    fn commandcode(base_url: &str) -> Client {
        let mut local = config(base_url);
        local.protocol = Protocol::CommandCode;
        Client::new(local).unwrap()
    }

    fn user() -> Vec<Value> {
        vec![serde_json::json!({"role": "user", "content": "hi"})]
    }

    fn response(head: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\n{head}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len(),
            head = head
        )
    }

    fn sse(events: &[&str]) -> String {
        let mut body = String::new();
        for event in events {
            body.push_str("data: ");
            body.push_str(event);
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        response("content-type: text/event-stream", &body)
    }

    /// One streamed chunk that ends the turn normally.
    fn sse_ok() -> String {
        sse(&[r#"{"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}"#])
    }

    /// A Command Code answer: NDJSON under its own content type, one whole event
    /// per line, with no `data:` prefix.
    fn ndjson(events: &[&str]) -> String {
        let mut body = String::new();
        for event in events {
            body.push_str(event);
            body.push('\n');
        }
        response("content-type: application/x-ndjson", &body)
    }

    /// A Command Code answer that ends the turn normally.
    fn ndjson_ok() -> String {
        ndjson(&[
            r#"{"type":"text-delta","text":"reading "}"#,
            r#"{"type":"finish","finishReason":"stop","totalUsage":{"inputTokens":2,"outputTokens":3}}"#,
        ])
    }

    /// A stream that stops without `[DONE]` and without a finish reason.
    fn unfinished() -> String {
        response(
            "content-type: text/event-stream",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
        )
    }

    /// A stream whose last line is cut off mid-JSON.
    fn torn() -> String {
        let mut out = unfinished();
        out.push_str("data: {\"choices\"");
        out
    }

    /// A canned non-2xx response.
    fn failed(status: &str, headers: &[(&str, &str)], body: &str) -> String {
        let mut head = format!("HTTP/1.1 {status}\r\n");
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        format!(
            "{head}content-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    /// A rate limit that names the wait it wants.
    fn limited(retry_after: u64) -> String {
        failed(
            "429 Too Many Requests",
            &[
                ("content-type", "application/json"),
                ("retry-after", &retry_after.to_string()),
            ],
            r#"{"error":{"message":"slow down"}}"#,
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
    async fn a_streaming_reply_collects_the_text_and_the_usage() {
        let events = [
            serde_json::json!({"choices":[{"delta":{"content":"Hel"}}]}),
            serde_json::json!({"choices":[{"delta":{"reasoning_content":"check"}}]}),
            serde_json::json!({"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}],
                               "usage":{"prompt_tokens":12,"completion_tokens":3,
                                        "completion_tokens_details":{"reasoning_tokens":1},
                                        "prompt_tokens_details":{"cached_tokens":8}}}),
        ];
        let encoded: Vec<String> = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect();
        let chunks: Vec<&str> = encoded.iter().map(String::as_str).collect();
        let (url, asked) = serve(vec![sse(&chunks)]).await;
        let mut text = String::new();
        let mut reasoning = String::new();
        let reply = client(&url)
            .complete(&user(), &[], "pair-1-mentor", &mut |delta| {
                match delta {
                    Delta::Text(chunk) => text.push_str(chunk),
                    Delta::Reasoning(chunk) => reasoning.push_str(chunk),
                    Delta::Tool { .. } => {}
                    Delta::Retry { .. } => {}
                    Delta::Discard => {}
                }
                CancelResult::Continue
            })
            .await
            .unwrap();
        assert_eq!(reply.text, "Hello");
        assert_eq!(reply.reasoning, "check");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(reply.session_id, None);
        assert_eq!(reply.usage.prompt_tokens, 12);
        assert_eq!(reply.usage.completion_tokens, 3);
        assert_eq!(reply.usage.reasoning_tokens, 1);
        assert_eq!(reply.usage.cached_tokens, 8);
        assert_eq!(arrivals(&asked).len(), 1);
        let body: Value = serde_json::from_str(&bodies(&asked)[0]).unwrap();
        assert_eq!(body["prompt_cache_key"], "pair-1-mentor");
        assert_eq!(body["model"], "gpt-4o");
    }

    #[tokio::test]
    async fn the_accounts_headers_reach_the_request() {
        let (url, asked) = serve(vec![sse_ok()]).await;
        let mut with_headers = config(&url);
        with_headers.headers = vec![HeaderPair {
            name: "x-session".into(),
            value: "pair-1".into(),
        }];
        Client::new(with_headers)
            .unwrap()
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();
        let head = &asked.lock().unwrap()[0].head;
        assert!(head.contains("x-session: pair-1"), "{head}");
        assert!(head.contains("authorization: bearer sk-test"), "{head}");
    }

    #[tokio::test]
    async fn a_rate_limit_is_retried_after_the_wait_the_server_named() {
        let (url, asked) = serve(vec![limited(1), sse_ok()]).await;
        let mut retries: Vec<(u32, Duration, String)> = Vec::new();
        let reply = client(&url)
            .complete(&user(), &[], "k", &mut |delta| {
                if let Delta::Retry {
                    attempt,
                    wait,
                    reason,
                } = delta
                {
                    retries.push((attempt, wait, reason.to_string()));
                }
                CancelResult::Continue
            })
            .await
            .unwrap();
        assert_eq!(reply.text, "ok");
        assert_eq!(retries.len(), 1);
        let (attempt, wait, reason) = &retries[0];
        assert_eq!(*attempt, 1);
        assert_eq!(*wait, Duration::from_secs(1));
        assert!(reason.contains("429"), "{reason}");
        let arrivals = arrivals(&asked);
        assert_eq!(arrivals.len(), 2);
        assert!(
            arrivals[1].duration_since(arrivals[0]) >= Duration::from_millis(900),
            "the cooldown paces the retry"
        );
    }

    #[tokio::test]
    async fn a_server_error_is_retried_with_backoff() {
        let broken = failed(
            "500 Internal Server Error",
            &[("content-type", "application/json")],
            r#"{"error":{"message":"boom"}}"#,
        );
        let (url, asked) = serve(vec![broken, sse_ok()]).await;
        let reply = client(&url)
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();
        assert_eq!(reply.text, "ok");
        let arrivals = arrivals(&asked);
        assert_eq!(arrivals.len(), 2);
        assert!(
            arrivals[1].duration_since(arrivals[0]) >= Duration::from_millis(400),
            "the backoff paces the retry"
        );
    }

    #[tokio::test]
    async fn an_auth_error_is_not_retried() {
        let bad_key = failed(
            "401 Unauthorized",
            &[("content-type", "application/json")],
            r#"{"error":{"message":"bad key"}}"#,
        );
        let (url, asked) = serve(vec![bad_key, sse_ok()]).await;
        let error = client(&url)
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Auth);
        assert_eq!(error.status, Some(401));
        assert!(!error.retryable);
        assert_eq!(error.attempts, 1);
        assert!(error.message.contains("bad key"));
        assert_eq!(arrivals(&asked).len(), 1);
    }

    #[tokio::test]
    async fn a_spent_quota_is_not_retried() {
        let spent = failed(
            "429 Too Many Requests",
            &[("content-type", "application/json")],
            r#"{"error":{"message":"Usage limit reached. Your limit will reset at 10:00"}}"#,
        );
        let (url, asked) = serve(vec![spent, sse_ok()]).await;
        let error = client(&url)
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Quota);
        assert!(!error.retryable);
        assert_eq!(error.attempts, 1);
        assert!(error.message.contains("reset at 10:00"));
        assert_eq!(arrivals(&asked).len(), 1);
    }

    #[tokio::test]
    async fn a_cancelled_stream_stops_where_it_is() {
        let chunks = [
            r#"{"choices":[{"delta":{"content":"Hel"}}]}"#,
            r#"{"choices":[{"delta":{"content":"lo"}}]}"#,
        ];
        let (url, asked) = serve(vec![sse(&chunks)]).await;
        let mut seen = String::new();
        let error = client(&url)
            .complete(&user(), &[], "k", &mut |delta| {
                if let Delta::Text(chunk) = delta {
                    seen.push_str(chunk);
                }
                CancelResult::Cancel
            })
            .await
            .unwrap_err();
        assert!(error.is_cancelled());
        assert_eq!(error.attempts, 1);
        assert_eq!(seen, "Hel");
        assert_eq!(arrivals(&asked).len(), 1);
    }

    #[tokio::test]
    async fn a_cancelled_retry_stops_the_request() {
        let (url, asked) = serve(vec![limited(1), sse_ok()]).await;
        let error = client(&url)
            .complete(&user(), &[], "k", &mut |delta| {
                if let Delta::Retry { .. } = delta {
                    return CancelResult::Cancel;
                }
                CancelResult::Continue
            })
            .await
            .unwrap_err();
        assert!(error.is_cancelled());
        assert_eq!(arrivals(&asked).len(), 1);
    }

    #[tokio::test]
    async fn a_shared_cooldown_paces_concurrent_completes() {
        let (url, asked) = serve(vec![
            limited(1),
            sse(&[r#"{"choices":[{"delta":{"content":"ok"}}]}"#]),
            sse_ok(),
        ])
        .await;
        let first = client(&url);
        let second = first.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let mut sent = Some(tx);
        let mut on_delta = |delta: Delta| {
            if let Delta::Retry { .. } = delta {
                if let Some(tx) = sent.take() {
                    let _ = tx.send(());
                }
            }
            CancelResult::Continue
        };
        let a_message = user();
        let a = first.complete(&a_message, &[], "k", &mut on_delta);
        let b_message = user();
        let b = async {
            let _ = rx.await;
            second
                .complete(&b_message, &[], "k", &mut |_| CancelResult::Continue)
                .await
        };
        let (a, b) = tokio::join!(a, b);
        a.unwrap();
        b.unwrap();
        let arrivals = arrivals(&asked);
        assert_eq!(
            arrivals.len(),
            3,
            "both completes finish on the one rate limit"
        );
        assert!(
            arrivals[1].duration_since(arrivals[0]) >= Duration::from_millis(900),
            "the second waits out the cooldown"
        );
    }

    #[test]
    fn clients_built_apart_for_one_account_share_its_cooldown() {
        // A turn builds a client per attempt, so the cooldown has to belong to
        // the account, not to the client that happened to see the 429.
        let url = "http://cooldown-shared.invalid/v1";
        let first = client(url);
        let deadline = first.set_cooldown(Duration::from_secs(30));
        let second = client(&format!("{}/", url.to_ascii_uppercase()));
        assert_eq!(
            cooldown_deadline(&second.cooldown),
            Some(deadline),
            "the same endpoint and credential, however the URL is spelled"
        );

        // Another credential on that endpoint, another endpoint and another
        // protocol are other accounts.
        let mut other_key = config(url);
        other_key.api_key = "sk-someone-else".into();
        assert_eq!(
            cooldown_deadline(&Client::new(other_key).unwrap().cooldown),
            None
        );
        assert_eq!(
            cooldown_deadline(&client("http://cooldown-other.invalid/v1").cooldown),
            None
        );
        assert_eq!(cooldown_deadline(&commandcode(url).cooldown), None);

        // The wait outlives the clients that saw it: the next turn on the
        // account still finds it.
        drop(first);
        drop(second);
        assert_eq!(cooldown_deadline(&client(url).cooldown), Some(deadline));
    }

    #[tokio::test]
    async fn a_request_gives_up_once_it_has_waited_out_its_budget() {
        let (url, asked) = serve(vec![limited(1), limited(1), limited(1), sse_ok()]).await;
        let mut patient = client(&url);
        patient.wait_budget = Duration::from_millis(1_500);
        let error = patient
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::RateLimited);
        assert_eq!(error.attempts, 3, "the waits, not the retries, ran out");
        assert_eq!(
            arrivals(&asked).len(),
            3,
            "the answer that came later is never asked for"
        );
    }

    #[tokio::test]
    async fn a_server_that_closes_without_responding_is_not_hammered() {
        let (url, asked) = serve(Vec::new()).await;
        let error = client(&url)
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Transport);
        assert!(
            error.attempts <= TRANSPORT_ATTEMPTS,
            "{} attempts",
            error.attempts
        );
        assert_eq!(arrivals(&asked).len() as u32, error.attempts);
    }

    #[tokio::test]
    async fn a_refused_connection_stops_early() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        drop(listener);
        let error = client(&url)
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Transport);
        assert!(
            error.attempts <= TRANSPORT_ATTEMPTS,
            "{} attempts",
            error.attempts
        );
        assert!(
            error.message.to_ascii_lowercase().contains("refused"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn an_unusable_url_fails_at_once() {
        let error = client("not a url")
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();
        assert_eq!(
            (error.kind, error.attempts),
            (ErrorKind::Transport, 1),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn an_html_success_body_fails_at_once_with_an_excerpt() {
        let html = "<html><head><title>Gateway</title></head><body>\
                    <h1>Upstream maintenance</h1><p>Try again soon.</p></body></html>";
        let (url, asked) = serve(vec![response(
            "content-type: text/html; charset=utf-8",
            html,
        )])
        .await;
        let error = client(&url)
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert_eq!(error.attempts, 1);
        assert_eq!(arrivals(&asked).len(), 1);
        assert!(error.message.contains("text/html"), "{}", error.message);
        assert!(
            error.message.contains("Upstream maintenance"),
            "{}",
            error.message
        );
        assert!(!error.message.contains("<h1>"), "{}", error.message);
    }

    #[tokio::test]
    async fn a_stream_that_ends_without_a_reason_or_done_is_retried() {
        let (url, asked) = serve(vec![unfinished(), sse_ok()]).await;
        let reply = client(&url)
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();
        assert_eq!(reply.text, "ok");
        assert_eq!(arrivals(&asked).len(), 2);
    }

    #[tokio::test]
    async fn a_torn_stream_tail_is_retried() {
        let (url, asked) = serve(vec![torn(), sse_ok()]).await;
        let reply = client(&url)
            .complete(&user(), &[], "k", &mut |_| CancelResult::Continue)
            .await
            .unwrap();
        assert_eq!(reply.text, "ok");
        assert_eq!(arrivals(&asked).len(), 2);
    }

    #[tokio::test]
    async fn a_retry_tells_the_caller_to_drop_what_the_attempt_streamed() {
        let (url, asked) = serve(vec![torn(), sse_ok()]).await;
        let mut seen: Vec<String> = Vec::new();
        let reply = client(&url)
            .complete(&user(), &[], "k", &mut |delta| {
                match delta {
                    Delta::Text(chunk) => seen.push(format!("text:{chunk}")),
                    Delta::Discard => seen.push("discard".to_string()),
                    Delta::Retry { .. } => seen.push("retry".to_string()),
                    _ => {}
                }
                CancelResult::Continue
            })
            .await
            .unwrap();
        assert_eq!(reply.text, "ok");
        assert_eq!(
            seen,
            vec![
                "text:ok".to_string(),
                "discard".to_string(),
                "retry".to_string(),
                "text:ok".to_string(),
            ],
            "the attempt left behind is dropped before the retry streams again"
        );
        assert_eq!(arrivals(&asked).len(), 2);
    }

    #[tokio::test]
    async fn a_command_code_stream_of_ndjson_lines_comes_back_whole() {
        let events = [
            r#"{"type":"text-delta","text":"reading "}"#,
            r#"{"type":"tool-input-start","id":"c1","toolName":"read"}"#,
            r#"{"type":"tool-input-delta","id":"c1","delta":"{\"path\":"}"#,
            r#"{"type":"tool-call","toolCallId":"c1","toolName":"read","input":{"path":"a"}}"#,
            r#"{"type":"finish","finishReason":"tool-calls","totalUsage":{"inputTokens":50,"outputTokens":7}}"#,
        ];
        let (url, asked) = serve(vec![ndjson(&events)]).await;
        let mut text = String::new();
        let reply = commandcode(&url)
            .complete(&user(), &[], "pair-1-mentor", &mut |delta| {
                if let Delta::Text(chunk) = delta {
                    text.push_str(chunk);
                }
                CancelResult::Continue
            })
            .await
            .unwrap();
        assert_eq!(text, "reading ");
        assert_eq!(reply.text, "reading ");
        assert_eq!(reply.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(reply.tool_calls.len(), 1, "{reply:?}");
        assert_eq!(reply.tool_calls[0].arguments, r#"{"path":"a"}"#);
        assert_eq!(reply.usage.prompt_tokens, 50);
        assert_eq!(reply.usage.completion_tokens, 7);
        assert_eq!(arrivals(&asked).len(), 1);
    }

    #[tokio::test]
    async fn a_malformed_ndjson_line_is_retried() {
        let (url, asked) = serve(vec![ndjson(&[r#"{"type":"text-delta""#]), ndjson_ok()]).await;
        let mut reasons: Vec<String> = Vec::new();
        let reply = commandcode(&url)
            .complete(&user(), &[], "k", &mut |delta| {
                if let Delta::Retry { reason, .. } = delta {
                    reasons.push(reason.to_string());
                }
                CancelResult::Continue
            })
            .await
            .unwrap();
        assert_eq!(reply.text, "reading ");
        assert_eq!(arrivals(&asked).len(), 2);
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("malformed NDJSON"), "{}", reasons[0]);
    }

    #[test]
    fn torn_call_arguments_are_repaired_before_they_are_stored() {
        let mut reply = Reply {
            tool_calls: vec![
                ToolCall {
                    id: "c1".into(),
                    name: "bash".into(),
                    arguments: r#"{"command":"#.into(),
                },
                ToolCall {
                    id: "c2".into(),
                    name: "read".into(),
                    arguments: String::new(),
                },
                ToolCall {
                    id: "c3".into(),
                    name: "write".into(),
                    arguments: r#"{"path":"a"}"#.into(),
                },
            ],
            ..Reply::default()
        };
        reply.repair_call_arguments();
        assert_eq!(reply.tool_calls[0].arguments, "{}");
        assert_eq!(reply.tool_calls[1].arguments, "{}");
        assert_eq!(
            reply.tool_calls[2].arguments, r#"{"path":"a"}"#,
            "a whole call is left as it arrived"
        );
    }

    #[test]
    fn a_published_cooldown_is_capped_at_the_longest_wait() {
        assert_eq!(
            published_wait(Some(Duration::from_secs(3_600)), 1),
            LONGEST_WAIT,
            "a gateway that names an hour doesn't hold the client for an hour"
        );
        assert_eq!(
            published_wait(Some(Duration::from_secs(5)), 1),
            Duration::from_secs(5)
        );
        assert!(published_wait(None, 1) <= LONGEST_WAIT);
    }

    #[test]
    fn a_client_needs_a_base_url_and_a_timeout() {
        let mut empty = config("");
        assert!(Client::new(empty.clone()).is_err());
        empty.timeout = Duration::ZERO;
        assert!(Client::new(empty).is_err());
        assert!(Client::new(config("http://127.0.0.1:1")).is_ok());
    }

    #[test]
    fn sse_lines_split_chunks_and_keep_unfinished_tails() {
        let mut lines = SseLines::default();
        assert!(lines.next_line().is_none());
        lines.push(b"data: one\ndata: t");
        assert_eq!(
            lines.next_line().as_deref(),
            Some(b"data: one\n".as_slice())
        );
        assert!(lines.next_line().is_none());
        assert_eq!(lines.rest(), b"data: t");
        lines.push(b"wo\n\n");
        assert_eq!(
            lines.next_line().as_deref(),
            Some(b"data: two\n".as_slice())
        );
        assert_eq!(lines.next_line().as_deref(), Some(b"\n".as_slice()));
        assert!(lines.next_line().is_none());
        assert!(lines.rest().is_empty());
    }

    #[test]
    fn sse_lines_remember_how_far_they_scanned() {
        let mut lines = SseLines::default();
        lines.push(&vec![b'x'; 10_000]);
        assert!(lines.next_line().is_none());
        assert_eq!(lines.scan, 10_000);
        lines.push(b"\n");
        let line = lines.next_line().expect("the newline ends the long line");
        assert_eq!(line.len(), 10_001);
        assert_eq!(lines.scan, 0);
    }

    #[test]
    fn malformed_json_and_unconsumed_stream_tails_are_transient() {
        assert!(parse_sse_data("{not json}").unwrap_err().retryable);
        assert!(parse_sse_data("[DONE]").unwrap().is_none());
        assert!(ensure_stream_consumed(b" \r\n").is_ok());
        assert!(
            ensure_stream_consumed(br#"data: {"unfinished":"#)
                .unwrap_err()
                .retryable
        );
    }

    #[test]
    fn empty_replies_follow_their_termination_reason() {
        let empty = |reason: Option<&str>| Reply {
            finish_reason: reason.map(str::to_string),
            ..Reply::default()
        };
        assert!(validate_reply(empty(Some("stop"))).is_ok());
        let reasoning_only = Reply {
            reasoning: "plan".into(),
            finish_reason: Some("length".into()),
            ..Reply::default()
        };
        assert!(validate_reply(reasoning_only).is_ok());
        assert_eq!(
            validate_reply(empty(Some("length")))
                .unwrap()
                .finish_reason
                .as_deref(),
            Some("length")
        );
        assert!(validate_reply(empty(None)).unwrap_err().retryable);
        assert!(
            !validate_reply(empty(Some("content_filter")))
                .unwrap_err()
                .retryable
        );
        assert!(
            !validate_reply(empty(Some("refusal")))
                .unwrap_err()
                .retryable
        );
    }

    #[test]
    fn a_long_string_is_cut_on_a_character_boundary() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("aéb", 3), "aé");
        assert_eq!(truncate("aéb", 2), "a");
    }
}
