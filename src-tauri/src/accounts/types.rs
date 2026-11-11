//! Core types for the direct-connection model registry.
//!
//! A *provider* is a wire-compatible API endpoint, identified by
//! `(protocol, base_url)`. Several coding agents often configure the same
//! endpoint (opencode and kimi can both point at `api.moonshot.cn`), so scans
//! merge into one provider that carries every *account* discovered for it.
//!
//! Secrets never cross IPC: the frontend sees [`AccountView`], which only
//! reports whether a secret exists and a masked preview of it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The wire protocol used to talk to a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    /// OpenAI Chat Completions (`POST {base}/chat/completions`).
    #[serde(rename = "openaiChat")]
    OpenAiChat,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "responses")]
    Responses,
    /// Command Code's subscription gateway: NDJSON lines with CLI headers.
    #[serde(rename = "commandCode")]
    CommandCode,
}

impl Protocol {
    pub fn slug(self) -> &'static str {
        match self {
            Protocol::OpenAiChat => "openai",
            Protocol::Anthropic => "anthropic",
            Protocol::Responses => "responses",
            Protocol::CommandCode => "commandcode",
        }
    }

    /// The endpoint used when a config names no URL.
    pub fn default_base_url(self) -> &'static str {
        match self {
            Protocol::OpenAiChat | Protocol::Responses => "https://api.openai.com/v1",
            Protocol::Anthropic => "https://api.anthropic.com",
            Protocol::CommandCode => "https://api.commandcode.ai",
        }
    }
}

/// How an account authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountKind {
    /// A pasted or imported API key.
    #[serde(rename = "apiKey")]
    ApiKey,
    /// ChatGPT subscription via PKCE OAuth.
    #[serde(rename = "chatgptOauth")]
    ChatGptOAuth,
    /// Command Code subscription via its browser hand-off.
    #[serde(rename = "commandCodeOauth")]
    CommandCodeOAuth,
    /// opencode-go: an API key plus a fixed extra header.
    #[serde(rename = "opencodeGo")]
    OpencodeGo,
    /// The name of an environment variable holding the key.
    #[serde(rename = "env")]
    Env,
}

/// Whether the account can be used right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountStatus {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "needsKey")]
    NeedsKey,
    #[serde(rename = "authExpired")]
    AuthExpired,
    #[serde(rename = "rateLimited")]
    RateLimited,
}

/// A stored credential. Serialized into the credential file, never to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Secret {
    None,
    ApiKey(String),
    /// Resolved from the process environment at request time.
    Env(String),
    OAuth(Box<OAuthToken>),
}

impl Secret {
    /// A short, non-reversible preview safe to show in the UI.
    pub fn masked(&self) -> Option<String> {
        match self {
            Secret::None => None,
            Secret::Env(name) => Some(format!("${}", name)),
            Secret::ApiKey(key) => Some(mask_secret(key)),
            Secret::OAuth(token) => Some(mask_secret(&token.access_token)),
        }
    }

    pub fn is_present(&self) -> bool {
        match self {
            Secret::None => false,
            Secret::Env(name) => std::env::var(name)
                .map(|value| !value.is_empty())
                .unwrap_or(false),
            // A blank key is not a credential: it would fail on the first
            // request while showing as ready in the UI.
            Secret::ApiKey(key) => !key.trim().is_empty(),
            Secret::OAuth(token) => !token.access_token.trim().is_empty(),
        }
    }
}

/// `sk-ant…9f3c` — enough to recognize a key, not enough to use it.
///
/// Sliced by characters, never by bytes: an imported config can hold a key with
/// multi-byte characters, and a byte slice there panics.
pub fn mask_secret(secret: &str) -> String {
    let trimmed = secret.trim();
    let count = trimmed.chars().count();
    if count <= 8 {
        return "••••••••".to_string();
    }
    let head: String = trimmed.chars().take(6).collect();
    let tail: String = trimmed.chars().skip(count - 4).collect();
    format!("{}…{}", head, tail)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthToken {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix milliseconds.
    pub expires_at: u64,
    #[serde(default = "default_token_type")]
    pub token_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Human label from the identity endpoint, when the service exposes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

impl OAuthToken {
    /// Refresh this much earlier than expiry so in-flight requests survive.
    pub const REFRESH_MARGIN_MS: u64 = 300_000;

    pub fn expires_within(&self, now_millis: u64, window_ms: u64) -> bool {
        self.expires_at <= now_millis.saturating_add(window_ms)
    }

    pub fn is_expired(&self, now_millis: u64) -> bool {
        self.expires_within(now_millis, 0)
    }
}

/// Whether a header carries a credential rather than metadata.
///
/// Source configs copy gateway headers verbatim, and `Authorization: Bearer
/// sk-…` is a live secret: it must not cross IPC inside a view.
pub fn header_is_secret(name: &str) -> bool {
    let lowered = name.trim().to_ascii_lowercase();
    lowered == "authorization"
        || ["key", "token", "secret", "cookie", "password"]
            .iter()
            .any(|hint| lowered.contains(hint))
}

/// A header pair safe to show in the UI.
pub fn mask_header(header: &HeaderPair) -> HeaderPair {
    if header_is_secret(&header.name) {
        HeaderPair {
            name: header.name.clone(),
            value: mask_secret(&header.value),
        }
    } else {
        header.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

/// One credential on a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub kind: AccountKind,
    pub label: String,
    /// Which importer or flow produced it (`kimi`, `opencode`, `manual`,
    /// `oauth:chatgpt`, `env`).
    pub source: String,
    pub status: AccountStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<HeaderPair>,
    /// Tool-name overrides for gateways that rename them (muse code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_names: Option<BTreeMap<String, String>>,
    pub secret: Secret,
}

impl Account {
    /// Accounts are re-derived from the same source on every scan, so the id
    /// must be stable across runs: rotating a key updates the account instead
    /// of orphaning it.
    pub fn make_id(provider_id: &str, kind: AccountKind, source: &str, label: &str) -> String {
        let fingerprint = format!("{}|{}|{}", kind_slug(kind), source, label);
        format!("{}-{}", provider_id, short_hash(&fingerprint))
    }
}

fn kind_slug(kind: AccountKind) -> &'static str {
    match kind {
        AccountKind::ApiKey => "api-key",
        AccountKind::ChatGptOAuth => "chatgpt-oauth",
        AccountKind::CommandCodeOAuth => "commandcode-oauth",
        AccountKind::OpencodeGo => "opencode-go",
        AccountKind::Env => "env",
    }
}

/// Where a model entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelSource {
    #[serde(rename = "preset")]
    Preset,
    #[serde(rename = "imported")]
    Imported,
    #[serde(rename = "endpoint")]
    Endpoint,
    #[serde(rename = "manual")]
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPrice {
    pub input_per_million: f64,
    pub output_per_million: f64,
}

/// A selectable model on a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoute {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<ModelPrice>,
    pub source: ModelSource,
}

/// A wire-compatible endpoint plus everything discovered for it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub vendor: String,
    pub display_name: String,
    pub accounts: Vec<Account>,
    pub models: Vec<ModelRoute>,
}

impl Provider {
    pub fn ready_account_count(&self) -> usize {
        self.accounts
            .iter()
            .filter(|account| account.status == AccountStatus::Ready)
            .count()
    }
}

// ---------------------------------------------------------------------------
// Frontend views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub vendor: String,
    pub display_name: String,
    pub accounts: Vec<AccountView>,
    pub models: Vec<ModelRoute>,
    pub ready_account_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub kind: AccountKind,
    pub label: String,
    pub source: String,
    pub status: AccountStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<HeaderPair>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_names: Option<BTreeMap<String, String>>,
    pub has_secret: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_secret: Option<String>,
}

impl Provider {
    pub fn to_view(&self) -> ProviderView {
        ProviderView {
            id: self.id.clone(),
            protocol: self.protocol,
            base_url: self.base_url.clone(),
            vendor: self.vendor.clone(),
            display_name: self.display_name.clone(),
            ready_account_count: self.ready_account_count(),
            accounts: self
                .accounts
                .iter()
                .map(|account| AccountView {
                    id: account.id.clone(),
                    kind: account.kind,
                    label: account.label.clone(),
                    source: account.source.clone(),
                    status: account.status,
                    headers: account.headers.iter().map(mask_header).collect(),
                    tool_names: account.tool_names.clone(),
                    has_secret: account.secret.is_present(),
                    masked_secret: account.secret.masked(),
                })
                .collect(),
            models: self.models.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Identity helpers
// ---------------------------------------------------------------------------

/// Strip trailing slashes and surrounding whitespace so two spellings of the
/// same endpoint compare equal.
///
/// An explicit default port goes too: `https://host:443` and `http://host` name
/// the same endpoint as the bare host, and leaving them in would defeat the
/// `(protocol, base_url)` dedupe by minting a second provider.
pub fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_slash = trimmed.trim_end_matches('/');
    if without_slash.is_empty() {
        return String::new();
    }
    strip_default_port(without_slash).unwrap_or_else(|| without_slash.to_string())
}

/// `https://api.anthropic.com:443/v1` → `https://api.anthropic.com/v1`.
///
/// Only the authority is touched, and only when something is left of it: a
/// userinfo or a path holding the same text is not a port.
fn strip_default_port(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "https" => ":443",
        "http" => ":80",
        _ => return None,
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    let authority = authority.strip_suffix(default_port)?;
    if authority.is_empty() {
        return None;
    }
    Some(format!("{}://{}{}", scheme, authority, path))
}

/// FNV-1a, 64-bit, rendered as 16 lowercase hex chars.
///
/// All 64 bits are kept: truncating to 32 makes a collision likely enough to
/// silently merge two distinct endpoints into one provider, and these ids are
/// persisted inside pair selections. `DefaultHasher` makes no stability promise
/// across Rust versions, so the hash is hand-rolled.
pub fn short_hash(input: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{:016x}", hash)
}

pub fn provider_id(protocol: Protocol, base_url: &str) -> String {
    format!(
        "{}-{}",
        protocol.slug(),
        short_hash(&normalize_base_url(base_url).to_lowercase())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_trailing_slashes_and_whitespace() {
        assert_eq!(
            normalize_base_url("  https://api.anthropic.com/  "),
            "https://api.anthropic.com"
        );
        assert_eq!(
            normalize_base_url("https://api.x.ai/v1///"),
            "https://api.x.ai/v1"
        );
        assert_eq!(normalize_base_url("   "), "");
        // An explicit default port names the same endpoint as the bare host.
        assert_eq!(
            normalize_base_url("https://api.anthropic.com:443"),
            "https://api.anthropic.com"
        );
        assert_eq!(
            normalize_base_url("http://llm.internal.example:80/v1/"),
            "http://llm.internal.example/v1"
        );
        // A non-default port is part of the endpoint, so it stays.
        assert_eq!(
            normalize_base_url("https://api.anthropic.com:8443"),
            "https://api.anthropic.com:8443"
        );
        assert_eq!(
            provider_id(Protocol::Anthropic, "https://api.anthropic.com:443"),
            provider_id(Protocol::Anthropic, "https://api.anthropic.com")
        );
    }

    #[test]
    fn provider_id_ignores_url_spelling() {
        let a = provider_id(Protocol::Anthropic, "https://api.anthropic.com");
        let b = provider_id(Protocol::Anthropic, "https://api.anthropic.com/");
        assert_eq!(a, b);
        assert_ne!(
            a,
            provider_id(Protocol::OpenAiChat, "https://api.anthropic.com")
        );
        assert_eq!(
            a,
            format!("anthropic-{}", short_hash("https://api.anthropic.com"))
        );
    }

    #[test]
    fn account_id_is_stable_for_the_same_source_and_label() {
        let first = Account::make_id(
            "anthropic-1",
            AccountKind::ApiKey,
            "kimi",
            "api.moonshot.cn",
        );
        let second = Account::make_id(
            "anthropic-1",
            AccountKind::ApiKey,
            "kimi",
            "api.moonshot.cn",
        );
        assert_eq!(first, second);
        assert_ne!(
            first,
            Account::make_id(
                "anthropic-1",
                AccountKind::ApiKey,
                "opencode",
                "api.moonshot.cn",
            )
        );
    }

    #[test]
    fn short_hash_is_deterministic_and_sixteen_chars() {
        assert_eq!(short_hash("abc"), short_hash("abc"));
        assert_eq!(short_hash("abc").len(), 16);
        assert_ne!(short_hash("abc"), short_hash("abd"));
    }

    #[test]
    fn masks_secrets_without_exposing_them() {
        assert_eq!(mask_secret("sk-ant-api03-abcdefghijklmnop"), "sk-ant…mnop");
        assert_eq!(mask_secret("short"), "••••••••");
        // A key with multi-byte characters: byte slicing would panic here.
        assert_eq!(mask_secret("αβγδεζηθικλμνξ"), "αβγδεζ…λμνξ");
        assert_eq!(mask_secret("密钥密钥"), "••••••••");
        assert_eq!(
            Secret::Env("ANTHROPIC_API_KEY".into()).masked().unwrap(),
            "$ANTHROPIC_API_KEY"
        );
        assert_eq!(Secret::None.masked(), None);
        assert!(!Secret::ApiKey("   ".into()).is_present());
        assert!(Secret::ApiKey("sk-1".into()).is_present());
    }

    #[test]
    fn env_secret_requires_a_live_variable() {
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_ENV_SECRET");
        std::env::remove_var("THE_PAIR_TEST_ENV_SECRET");
        assert!(!Secret::Env("THE_PAIR_TEST_ENV_SECRET".into()).is_present());

        std::env::set_var("THE_PAIR_TEST_ENV_SECRET", "x");
        assert!(Secret::Env("THE_PAIR_TEST_ENV_SECRET".into()).is_present());
        // Blank counts as absent, matching the importers' `live_env`.
        std::env::set_var("THE_PAIR_TEST_ENV_SECRET", "");
        assert!(!Secret::Env("THE_PAIR_TEST_ENV_SECRET".into()).is_present());

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_ENV_SECRET", value),
            None => std::env::remove_var("THE_PAIR_TEST_ENV_SECRET"),
        }
    }

    #[test]
    fn oauth_expiry_respects_the_refresh_margin() {
        let token = OAuthToken {
            access_token: "a".into(),
            refresh_token: None,
            expires_at: 1_000_000,
            token_type: "Bearer".into(),
            scope: None,
            account_label: None,
        };
        assert!(token.expires_within(1_000_000, OAuthToken::REFRESH_MARGIN_MS));
        assert!(!token.expires_within(
            1_000_000 - OAuthToken::REFRESH_MARGIN_MS - 1,
            OAuthToken::REFRESH_MARGIN_MS
        ));
        assert!(token.is_expired(1_000_001));
        assert!(!token.is_expired(999_999));
    }

    #[test]
    fn provider_view_hides_secrets() {
        let provider = Provider {
            id: "anthropic-1".into(),
            protocol: Protocol::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            vendor: "anthropic".into(),
            display_name: "Anthropic".into(),
            accounts: vec![Account {
                id: "anthropic-1-1".into(),
                kind: AccountKind::ApiKey,
                label: "claude settings".into(),
                source: "claude".into(),
                status: AccountStatus::Ready,
                headers: vec![HeaderPair {
                    name: "Authorization".into(),
                    value: "Bearer sk-ant-secret-value".into(),
                }],
                tool_names: None,
                secret: Secret::ApiKey("sk-ant-super-secret-value".into()),
            }],
            models: vec![],
        };
        let json = serde_json::to_string(&provider.to_view()).unwrap();
        assert!(!json.contains("super-secret"), "secret leaked: {}", json);
        assert!(
            !json.contains("secret-value"),
            "header secret leaked: {}",
            json
        );
        assert!(json.contains("maskedSecret"));
        assert_eq!(provider.ready_account_count(), 1);
    }

    #[test]
    fn auth_like_headers_are_masked_before_they_reach_the_ui() {
        assert!(header_is_secret("Authorization"));
        assert!(header_is_secret(" x-api-key "));
        assert!(header_is_secret("X-Goog-Api-Key"));
        assert!(header_is_secret("X-Session-Token"));
        assert!(!header_is_secret("User-Agent"));
        assert!(!header_is_secret("Content-Type"));
        let masked = mask_header(&HeaderPair {
            name: "Authorization".into(),
            value: "Bearer sk-ant-secret-value".into(),
        });
        assert_eq!(masked.name, "Authorization");
        assert!(!masked.value.contains("secret-value"));
        let plain = mask_header(&HeaderPair {
            name: "User-Agent".into(),
            value: "the-pair".into(),
        });
        assert_eq!(plain.value, "the-pair");
    }

    #[test]
    fn enums_travel_over_ipc_as_camel_case() {
        // The renderer matches these strings literally; a rename here is a
        // breaking change for the UI, so it is pinned.
        assert_eq!(
            serde_json::to_string(&Protocol::OpenAiChat).unwrap(),
            "\"openaiChat\""
        );
        assert_eq!(
            serde_json::to_string(&Protocol::CommandCode).unwrap(),
            "\"commandCode\""
        );
        assert_eq!(
            serde_json::to_string(&AccountKind::ChatGptOAuth).unwrap(),
            "\"chatgptOauth\""
        );
        assert_eq!(
            serde_json::to_string(&AccountKind::CommandCodeOAuth).unwrap(),
            "\"commandCodeOauth\""
        );
        assert_eq!(
            serde_json::to_string(&AccountKind::OpencodeGo).unwrap(),
            "\"opencodeGo\""
        );
        assert_eq!(
            serde_json::to_string(&AccountStatus::NeedsKey).unwrap(),
            "\"needsKey\""
        );
        assert_eq!(
            serde_json::to_string(&AccountStatus::AuthExpired).unwrap(),
            "\"authExpired\""
        );
        assert_eq!(
            serde_json::to_string(&ModelSource::Imported).unwrap(),
            "\"imported\""
        );
    }
}
