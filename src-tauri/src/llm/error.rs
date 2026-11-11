//! How a failed provider request is classified: the kinds callers act on, and
//! whether waiting can heal it.
//!
//! The same status can mean opposite things. A 429 whose body says "usage limit
//! reached" is spent until the plan resets, while a 429 that says "slow down"
//! heals in a second, so classification reads the body and not just the status.

use std::fmt;
use std::time::Duration;

use reqwest::StatusCode;

use super::{truncate, Failure, EXCERPT_BYTES};

/// Why a provider request failed, so callers never have to re-classify message
/// text to decide what to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The conversation no longer fits the context window.
    Overflow,
    /// The credentials were rejected.
    Auth,
    /// A usage allowance ran out and resets on its own schedule.
    Quota,
    /// Requests came too fast.
    RateLimited,
    /// The provider refused an image or other media.
    Media,
    /// The request never reached a usable response: DNS, connect, TLS.
    Transport,
    /// The response wasn't the event stream the protocol speaks.
    BadResponse,
    /// Anything else.
    Other,
}

/// A provider request that failed after every retry, classified once where the
/// response was read. Its `Display` is the message the user sees.
#[derive(Debug)]
pub struct ProviderError {
    /// The HTTP status, when the failure had one.
    pub status: Option<u16>,
    /// What kind of failure this was.
    pub kind: ErrorKind,
    /// The server's or the transport's message, as shown to the user.
    pub message: String,
    /// Requests made, including the failed one.
    pub attempts: u32,
    /// Whether the failure was transient — a 429 or a 5xx — rather than a
    /// rejection another attempt cannot heal. `attempts` says how many were
    /// already spent before giving up.
    pub retryable: bool,
    /// Guidance the caller appends after the attempt count.
    pub advice: Option<String>,
}

impl ProviderError {
    /// What a stream the caller cancelled reports: nothing was wrong with the
    /// provider, so the failure is never retried and never advises anything.
    pub(crate) fn cancelled(attempts: u32) -> Self {
        Self {
            status: None,
            kind: ErrorKind::Other,
            message: CANCELLED.to_string(),
            attempts,
            retryable: false,
            advice: None,
        }
    }

    /// A route that could not produce a request at all — no base URL, no request
    /// timeout — so nothing was ever sent and no retry can heal it.
    pub fn unusable(message: String) -> Self {
        Self {
            status: None,
            kind: ErrorKind::Transport,
            message,
            attempts: 0,
            retryable: false,
            advice: None,
        }
    }

    /// The credentials were rejected, or cannot be used as they stand.
    pub fn auth(message: String) -> Self {
        Self {
            status: None,
            kind: ErrorKind::Auth,
            message,
            attempts: 1,
            retryable: false,
            advice: None,
        }
    }

    /// The caller's own request was wrong before it left: a selection the
    /// registry no longer backs, say. Retrying cannot help.
    pub fn bad_response(message: String) -> Self {
        Self {
            status: None,
            kind: ErrorKind::BadResponse,
            message,
            attempts: 0,
            retryable: false,
            advice: None,
        }
    }

    /// Whether this error is a cancellation rather than a provider failure.
    pub fn is_cancelled(&self) -> bool {
        self.kind == ErrorKind::Other && self.message == CANCELLED
    }
}

/// The message a cancelled stream reports, so the caller can tell it apart from
/// a provider failure without a variant of its own.
pub(crate) const CANCELLED: &str = "cancelled: the caller stopped the stream";

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (after {} attempt(s)){}",
            self.message,
            self.attempts,
            self.advice
                .as_deref()
                .map(|advice| format!(" {}", advice))
                .unwrap_or_default()
        )
    }
}

impl std::error::Error for ProviderError {}

/// Statuses another attempt can heal.
pub(super) fn retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 425 | 429 | 500..=599)
}

/// The kind of a failure that carries an HTTP status and nothing else to go on.
pub(super) fn kind_for_status(status: Option<u16>) -> ErrorKind {
    match status {
        Some(401 | 403) => ErrorKind::Auth,
        Some(402) => ErrorKind::Quota,
        Some(429) => ErrorKind::RateLimited,
        _ => ErrorKind::Other,
    }
}

/// An error with every error beneath it joined by `: ` — reqwest's own `Display`
/// drops the source, which is the part that says whether it was DNS, TLS or a
/// refused connection.
pub(super) fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut chain = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        chain.push_str(": ");
        chain.push_str(&error.to_string());
        source = error.source();
    }
    chain
}

/// A request that never produced a response. Builder and URL mistakes and
/// rejected certificates can't succeed on a retry, so they fail at once;
/// connection-level failures get `TRANSPORT_ATTEMPTS`; timeouts stay as
/// transient as ever.
pub(super) fn transport_failure(error: reqwest::Error) -> Failure {
    let chain = error_chain(&error);
    let fatal = error.is_builder()
        || error.is_request()
        || chain.to_ascii_lowercase().contains("certificate");
    Failure {
        max_attempts: (!fatal && !error.is_timeout()).then_some(super::TRANSPORT_ATTEMPTS),
        ..Failure::of(
            ErrorKind::Transport,
            None,
            !fatal,
            format!("request failed: {chain}"),
        )
    }
}

/// The first bytes of a response body, enough to say what a misrouted endpoint
/// sent.
pub(super) async fn body_excerpt(response: &mut reqwest::Response) -> String {
    let mut bytes = Vec::new();
    while bytes.len() < EXCERPT_BYTES {
        match response.chunk().await {
            Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
            Ok(None) | Err(_) => break,
        }
    }
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// A body as one short line for an error message: HTML tags stripped,
/// whitespace folded.
pub(super) fn excerpt(body: &str) -> String {
    let mut text = String::with_capacity(body.len());
    let mut tag = false;
    for c in body.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => text.push(c),
            _ => {}
        }
    }
    truncate(&text.split_whitespace().collect::<Vec<_>>().join(" "), 500).to_string()
}

/// A server asking for a longer wait than this isn't worth sitting through.
pub(super) const LONGEST_WAIT: Duration = Duration::from_secs(60);

/// Words in a 429 body that say a usage allowance ran out, not that requests came
/// too fast. The last two are the Chinese wording some gateways send for a
/// spent plan.
const QUOTA_WORDS: [&str; 8] = [
    "usage limit",
    "budget_exceeded",
    "budget has been exceeded",
    "reset at",
    "exceeded your current",
    "usage_limit_exceeded",
    "用量上限",
    "使用上限",
];

/// Words that say which allowance a quota belongs to: the plan that ran out, or
/// when it comes back. The bare word is deliberately absent — gateways write it
/// for a per-minute limit ("Quota exceeded for quota metric … per-minute") that
/// heals in seconds — so a quota only spends the allowance beside one of these.
const QUOTA_SCOPE: [&str; 6] = [
    "reset",
    "plan",
    "subscription",
    "billing",
    "monthly",
    "credit",
];

/// Words in a rejection body that say the conversation no longer fits the
/// context window.
const OVERFLOW_WORDS: [&str; 7] = [
    "context length",
    "context_length",
    "maximum context",
    "context window",
    "prompt is too long",
    "too many tokens",
    "input is too long",
];

/// Words in a rejection body that say the provider refused media.
const MEDIA_WORDS: [&str; 2] = ["image", "media"];

/// What a failed HTTP response means: retried, unless waiting won't help. A
/// spent quota (a 5-hour or monthly limit) resets in hours, so it fails at once
/// with the server's message.
pub(super) fn classify(status: StatusCode, retry_after: Option<Duration>, body: &str) -> Failure {
    let lower = body.to_lowercase();
    let quota = status == StatusCode::TOO_MANY_REQUESTS
        && (QUOTA_WORDS.iter().any(|w| lower.contains(w))
            || (lower.contains("quota") && QUOTA_SCOPE.iter().any(|w| lower.contains(w))));
    let too_long = retry_after.is_some_and(|wait| wait > LONGEST_WAIT);
    let kind = match status.as_u16() {
        429 if quota => ErrorKind::Quota,
        429 => ErrorKind::RateLimited,
        401 | 403 => ErrorKind::Auth,
        402 => ErrorKind::Quota,
        400 | 413 | 422 if OVERFLOW_WORDS.iter().any(|w| lower.contains(w)) => ErrorKind::Overflow,
        400 | 415 if MEDIA_WORDS.iter().any(|w| lower.contains(w)) => ErrorKind::Media,
        _ => ErrorKind::Other,
    };
    Failure {
        kind,
        status: Some(status.as_u16()),
        retryable: retryable_status(status) && !quota && !too_long,
        retry_after,
        max_attempts: None,
        message: format!("HTTP {status}: {}", truncate(body, 2000)),
        cancelled: false,
    }
}

/// 500 ms doubling to a 32 s cap, plus up to 25% jitter.
pub(super) fn backoff(attempt: u32) -> Duration {
    let base = (500u64 << (attempt - 1).min(6)).min(32_000);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let jitter = base * u64::from(nanos % 26) / 100;
    Duration::from_millis(base + jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure_of(status: u16, retry_after: Option<Duration>, body: &str) -> Failure {
        classify(StatusCode::from_u16(status).unwrap(), retry_after, body)
    }

    #[test]
    fn a_spent_quota_is_not_retried() {
        let body = r#"{"error":{"code":"1308","message":"Usage limit reached for 5 hour. Your limit will reset at 2026-10-22 16:06:02"}}"#;
        let failure = failure_of(429, None, body);
        assert!(!failure.retryable);
        assert_eq!(failure.kind, ErrorKind::Quota);
        assert!(failure.message.contains("reset at 2026-10-22 16:06:02"));
    }

    #[test]
    fn a_spent_chatgpt_plan_is_not_retried() {
        let body = r#"{"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"Try again later"}}"#;
        assert!(!failure_of(429, None, body).retryable);
    }

    #[test]
    fn a_spent_budget_is_not_retried() {
        let body = r#"{"error":{"type":"budget_exceeded","message":"Budget has been exceeded"}}"#;
        let failure = failure_of(429, None, body);
        assert_eq!(failure.kind, ErrorKind::Quota);
        assert!(!failure.retryable);
    }

    #[test]
    fn a_spent_chinese_token_plan_is_not_retried() {
        let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"已达到 Token Plan 用量上限：请升级 Token Plan 套餐或购买积分补充用量。 (2056)"}}"#;
        let failure = failure_of(429, None, body);
        assert_eq!(failure.kind, ErrorKind::Quota);
        assert!(!failure.retryable);
        let variant = r#"{"error":{"message":"已达到使用上限，请稍后再试"}}"#;
        assert!(!failure_of(429, None, variant).retryable);
    }

    #[test]
    fn a_per_minute_quota_is_still_a_rate_limit() {
        let body = r#"{"error":{"message":"Quota exceeded for quota metric `GenerateRequestsPerMinute`, limit 10, used 10"}}"#;
        let failure = failure_of(429, Some(Duration::from_secs(1)), body);
        assert_eq!(failure.kind, ErrorKind::RateLimited);
        assert!(failure.retryable, "a per-minute limit heals in seconds");
    }

    #[test]
    fn a_quota_that_names_the_plan_it_ran_out_is_not_retried() {
        let body = r#"{"error":{"message":"Quota exceeded for this subscription's monthly plan; it will reset at 10:00"}}"#;
        let failure = failure_of(429, None, body);
        assert_eq!(failure.kind, ErrorKind::Quota);
        assert!(!failure.retryable);
    }

    #[test]
    fn a_route_that_cannot_make_a_request_is_not_retryable() {
        let error = ProviderError::unusable("the provider has no base URL".to_string());
        assert_eq!(error.kind, ErrorKind::Transport);
        assert!(!error.retryable);
        assert_eq!(error.attempts, 0, "nothing was ever sent");
        assert!(error.to_string().contains("no base URL"));
    }

    #[test]
    fn rate_limits_and_server_errors_are_retried() {
        assert!(failure_of(429, Some(Duration::from_secs(5)), "slow down").retryable);
        assert!(failure_of(502, None, "bad gateway").retryable);
        assert!(!failure_of(400, None, "bad request").retryable);
    }

    #[test]
    fn a_long_retry_after_is_not_waited_out() {
        assert!(!failure_of(503, Some(Duration::from_secs(3600)), "down").retryable);
        assert!(failure_of(503, Some(Duration::from_secs(60)), "down").retryable);
    }

    #[test]
    fn throttle_text_is_not_overflow() {
        let failure = failure_of(429, None, "Too many tokens, please wait and try again");
        assert_eq!(failure.kind, ErrorKind::RateLimited);
        assert!(failure.retryable);
        assert_ne!(failure.kind, ErrorKind::Overflow);
    }

    #[test]
    fn statuses_classify_into_kinds() {
        assert_eq!(failure_of(401, None, "bad key").kind, ErrorKind::Auth);
        assert_eq!(failure_of(403, None, "no").kind, ErrorKind::Auth);
        assert_eq!(failure_of(402, None, "pay").kind, ErrorKind::Quota);
        assert_eq!(
            failure_of(429, None, "usage limit reached").kind,
            ErrorKind::Quota
        );
        assert_eq!(
            failure_of(
                400,
                None,
                "This model's maximum context length is 128000 tokens"
            )
            .kind,
            ErrorKind::Overflow
        );
        assert_eq!(
            failure_of(400, None, "the image is too large").kind,
            ErrorKind::Media
        );
        assert_eq!(
            failure_of(500, None, "prompt is too long").kind,
            ErrorKind::Other
        );
        assert_eq!(
            Failure::from_status(Some(429), "mid-stream".into()).kind,
            ErrorKind::RateLimited
        );
        assert_eq!(
            Failure::from_status(None, "mid-stream".into()).kind,
            ErrorKind::Other
        );
        assert!(Failure::from_status(Some(500), "mid-stream".into()).retryable);
    }

    #[test]
    fn provider_error_display_keeps_the_message_text() {
        let error = ProviderError {
            status: Some(429),
            kind: ErrorKind::RateLimited,
            message: "HTTP 429 Too Many Requests: slow down".into(),
            attempts: 3,
            retryable: true,
            advice: None,
        };
        assert_eq!(
            error.to_string(),
            "HTTP 429 Too Many Requests: slow down (after 3 attempt(s))"
        );
        let advised = ProviderError {
            advice: Some("check the key".into()),
            ..error
        };
        assert_eq!(
            advised.to_string(),
            "HTTP 429 Too Many Requests: slow down (after 3 attempt(s)) check the key"
        );
    }

    #[test]
    fn a_cancelled_stream_is_not_a_provider_failure() {
        let error = ProviderError::cancelled(2);
        assert!(error.is_cancelled());
        assert!(!error.retryable);
        assert_eq!(error.attempts, 2);
        assert_eq!(error.kind, ErrorKind::Other);
        assert!(error.to_string().contains("cancelled"));
        let other = ProviderError {
            status: None,
            kind: ErrorKind::Other,
            message: "something else".into(),
            attempts: 1,
            retryable: false,
            advice: None,
        };
        assert!(!other.is_cancelled());
    }

    #[test]
    fn an_html_body_becomes_a_one_line_excerpt() {
        let text = "<html>\n  <head><title>Gateway</title></head>\n  \
                    <body><h1>Down for maintenance</h1></body>\n</html>";
        assert_eq!(excerpt(text), "Gateway Down for maintenance");
        assert_eq!(excerpt(&"x".repeat(600)).len(), 500);
    }

    #[test]
    fn backoff_doubles_to_a_cap() {
        assert!(backoff(1) >= Duration::from_millis(500));
        assert!(backoff(1) <= Duration::from_millis(625));
        assert!(backoff(7) >= Duration::from_secs(32) && backoff(7) <= Duration::from_secs(40));
        assert!(backoff(10) <= Duration::from_secs(40));
    }

    #[test]
    fn error_chains_join_every_source() {
        let outer = std::io::Error::new(std::io::ErrorKind::Other, "outer");
        assert_eq!(error_chain(&outer), "outer");
    }
}
