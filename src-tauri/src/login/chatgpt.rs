//! Signing in to the ChatGPT plan: the PKCE OAuth flow OpenAI documents for
//! open-source tools (`auth.openai.com`). The browser part runs once; the token
//! is then refreshed in place until its refresh token stops working.
//!
//! The only file this module reads is the cache it keeps in the app data dir,
//! `<app data>/oauth/chatgpt.json` (owner-only):
//!
//! ```json
//! {
//!   "clientId": "oaiapp_…",
//!   "subject": "user-…",
//!   "accessToken": "eyJ…",
//!   "refreshToken": "…",
//!   "expiresAt": 1767225600000,
//!   "tokenType": "Bearer",
//!   "scope": "openid offline_access chatgpt.tokens.use.direct",
//!   "accountLabel": "me@example.com"
//! }
//! ```
//!
//! The `id_token` the token endpoint returns is read the way the CLI tools read
//! it: the payload is base64url-decoded for the account's subject and email, and
//! the signature is *not* verified. That is the standing trade-off for this
//! shape of client — the token came straight from the endpoint over TLS and is
//! only used to name the account — so the issuer, audience, expiry and nonce are
//! checked instead.

use super::*;
use crate::accounts::commands::{oauth_token_path, write_token_file};
use crate::accounts::registry::AccountDraft;
use crate::accounts::types::{AccountKind, ModelRoute, ModelSource, OAuthToken, Protocol, Secret};
use crate::util::now_millis;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE_URL: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
/// The API the tokens are issued for, where requests on the plan go.
const RESOURCE: &str = "https://api.openai.com/v1";
const MODELS_URL: &str = "https://api.openai.com/v1/models";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope that lets The Pair use the plan; without it a sign-in only names the account.
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
/// The first-time registration entry point: never saved, never spent for a token.
const REGISTER: &str = "dynamic_agent_client";
const CALLBACK_PATH: &str = "/auth/callback";
/// The callback port tried first; any free port works after that.
const CALLBACK_PORT: u16 = 1455;
/// Where users see and limit what The Pair uses of their plan.
const USAGE_URL: &str = "https://chatgpt.com/settings/usage";
/// Refresh errors that mean the refresh token cannot be used again.
const UNUSABLE: [&str; 6] = [
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
];
const APP_NAME: &str = "The Pair";

/// The cached sign-in: the client OpenAI registered for this machine plus the
/// tokens, which rotate on every refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedToken {
    /// Reused on later sign-ins so the registration is not repeated.
    client_id: String,
    /// The account the ID token named, so a sign-in cannot switch accounts.
    #[serde(default)]
    subject: String,
    #[serde(flatten)]
    token: OAuthToken,
}

impl CachedToken {
    fn has_plan_scope(&self) -> bool {
        self.token
            .scope
            .as_deref()
            .map(|scope| scope.split_whitespace().any(|part| part == PLAN_SCOPE))
            .unwrap_or(false)
    }
}

/// The endpoints a sign-in talks to. Tests point them at a local server.
struct Flow<'a> {
    authorize: &'a str,
    token: &'a str,
    models: &'a str,
    /// Whether to open the system browser; tests drive the callback themselves.
    open_browser: bool,
}

impl Flow<'static> {
    fn live() -> Self {
        Flow {
            authorize: AUTHORIZE_URL,
            token: TOKEN_URL,
            models: MODELS_URL,
            open_browser: true,
        }
    }
}

/// The fresh secrets of one sign-in attempt.
struct Attempt {
    state: String,
    nonce: String,
    verifier: String,
}

impl Attempt {
    fn new() -> Self {
        Self {
            state: random_token(),
            nonce: random_token(),
            verifier: random_token(),
        }
    }
}

/// What the browser brings back from a sign-in that went through.
#[derive(Debug, PartialEq)]
struct Callback {
    code: String,
    client_id: Option<String>,
    scope: Option<String>,
}

pub async fn login(app_dir: &Path) -> Result<LoginOutcome, String> {
    sign_in(app_dir, &Flow::live()).await
}

/// Serializes every refresh of the saved sign-in. OpenAI rotates the refresh
/// token on each use, so two callers spending the same one — two pairs on the
/// plan, or a turn and the Sign-in button — would leave the loser holding a
/// token the server now calls reused, which drops the whole sign-in.
static RENEW_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Renews the saved sign-in without a browser, for a turn about to use it.
///
/// Answers with the token to use: the saved one when it is not close to expiry
/// (another turn may have renewed it while this one waited), otherwise the one
/// the refresh came back with, already written to the cache.
pub async fn renew(app_dir: &Path) -> Result<OAuthToken, String> {
    renew_at(app_dir, TOKEN_URL).await
}

async fn renew_at(app_dir: &Path, token_url: &str) -> Result<OAuthToken, String> {
    let _renewing = RENEW_LOCK.lock().await;
    let cached = load_token(app_dir)
        .ok_or_else(|| "there is no saved ChatGPT sign-in; sign in again".to_string())?;
    if !cached.token.access_token.is_empty()
        && !cached
            .token
            .expires_within(now_millis(), OAuthToken::REFRESH_MARGIN_MS)
    {
        return Ok(cached.token);
    }
    let http = http_client()?;
    let refreshed = refresh(&http, app_dir, token_url, &cached).await?;
    save_token(app_dir, &refreshed)?;
    Ok(refreshed.token)
}

async fn sign_in(app_dir: &Path, flow: &Flow<'_>) -> Result<LoginOutcome, String> {
    let http = http_client()?;
    // Held across the refresh and the write that follows it, and released before
    // the browser half: a turn renewing the same sign-in must not spend the
    // refresh token this one is spending.
    let renewing = RENEW_LOCK.lock().await;
    let saved = load_token(app_dir);
    if let Some(cached) = saved.as_ref().filter(|cached| renewable(cached)) {
        match refresh(&http, app_dir, flow.token, cached).await {
            // OpenAI rotates refresh tokens, so the pair that came back is the
            // only one that still works: it is written before anything else
            // gets the chance to spend it.
            Ok(refreshed) => {
                save_token(app_dir, &refreshed)?;
                drop(renewing);
                return finish(&http, flow, refreshed).await;
            }
            // A token that is still inside its lifetime survives a failed
            // refresh: only an expired one has to send the user to the browser.
            Err(_) if !cached.token.is_expired(now_millis()) => {
                drop(renewing);
                return finish(&http, flow, cached.clone()).await;
            }
            Err(error) => return Err(error),
        }
    }
    drop(renewing);
    let (listener, redirect_uri) = bind_callback(CALLBACK_PORT).await?;
    let attempt = Attempt::new();
    browser_flow(
        &http,
        app_dir,
        flow,
        &listener,
        &redirect_uri,
        &attempt,
        saved.as_ref(),
    )
    .await
}

/// The browser half: authorize, wait for the callback, spend the code.
async fn browser_flow(
    http: &reqwest::Client,
    app_dir: &Path,
    flow: &Flow<'_>,
    listener: &TcpListener,
    redirect_uri: &str,
    attempt: &Attempt,
    saved: Option<&CachedToken>,
) -> Result<LoginOutcome, String> {
    let url = authorize_url(flow.authorize, attempt, redirect_uri, saved)?;
    if flow.open_browser {
        open_browser(&url).map_err(|error| format!("{}; open {} yourself", error, url))?;
    }
    let callback = tokio::time::timeout(
        super::BROWSER_TIMEOUT,
        wait_for_callback(listener, &attempt.state),
    )
    .await
    .map_err(|_| {
        "the browser didn't come back within 5 minutes; try signing in again".to_string()
    })??;
    let client_id = issued_client(
        saved
            .map(|cached| cached.client_id.as_str())
            .unwrap_or_default(),
        callback.client_id.as_deref(),
    )?;

    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id.as_str()),
        ("code", callback.code.as_str()),
        ("code_verifier", attempt.verifier.as_str()),
        ("redirect_uri", redirect_uri),
        ("resource", RESOURCE),
    ];
    let response = http
        .post(flow.token)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form_body(&form))
        .send()
        .await
        .map_err(|error| format!("could not reach OpenAI's token endpoint: {}", error))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!(
            "OpenAI turned the sign-in away (HTTP {} {}); try again",
            status,
            oauth_error(&body)
        ));
    }
    let id_token = body
        .get("id_token")
        .and_then(Value::as_str)
        .ok_or("the token response has no id_token")?;
    let (subject, email) = identity(id_token, &client_id, &attempt.nonce, now_secs())?;
    // The registration belongs to the account that made it: a sign-in as
    // anybody else has to start from a clean cache, or it would quietly take
    // the saved registration over.
    if let Some(saved) = saved {
        if !saved.subject.is_empty() && saved.subject != subject {
            let who = saved
                .token
                .account_label
                .as_deref()
                .filter(|label| !label.is_empty())
                .unwrap_or("the account saved here");
            return Err(format!(
                "that is a different ChatGPT account than {}; to switch accounts, delete {} and sign in again",
                who,
                token_path(app_dir).display()
            ));
        }
    }
    let scope = callback
        .scope
        .as_deref()
        .or_else(|| body.get("scope").and_then(Value::as_str))
        .unwrap_or_default();
    ensure_plan_scope(scope, &email)?;

    // The callback names the scopes the account granted; the token response may
    // leave them out, and the saved sign-in needs them to stay reusable.
    let mut granted = fresh_token();
    if !scope.is_empty() {
        granted.scope = Some(scope.to_string());
    }
    let cached = CachedToken {
        client_id,
        subject,
        token: take_tokens(&granted, &body, &email)?,
    };
    save_token(app_dir, &cached)?;
    finish(http, flow, cached).await
}

/// Builds the outcome and lists the plan's models, which is the only call that
/// may fail without failing the sign-in.
async fn finish(
    http: &reqwest::Client,
    flow: &Flow<'_>,
    cached: CachedToken,
) -> Result<LoginOutcome, String> {
    let mut notes = vec![format!(
        "Requests on your ChatGPT plan count toward its usage. Manage usage: {}",
        USAGE_URL
    )];
    let models = match fetch_models(http, &cached.token.access_token, flow.models).await {
        Ok(models) if models.is_empty() => {
            notes.push("Your ChatGPT plan listed no models; add them by hand.".to_string());
            Vec::new()
        }
        Ok(models) => models,
        Err(message) => {
            notes.push(format!(
                "Could not list your ChatGPT plan's models: {}",
                message
            ));
            Vec::new()
        }
    };
    Ok(LoginOutcome {
        protocol: Protocol::Responses,
        base_url: RESOURCE.to_string(),
        vendor: Some("openai".to_string()),
        display_name: Some(format!("{} plan", APP_NAME)),
        account: account_draft(&cached),
        models,
        notes,
    })
}

fn account_draft(cached: &CachedToken) -> AccountDraft {
    let label = cached
        .token
        .account_label
        .clone()
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| format!("{} plan", APP_NAME));
    AccountDraft {
        kind: AccountKind::ChatGptOAuth,
        label,
        source: "oauth:chatgpt".to_string(),
        secret: Secret::OAuth(Box::new(cached.token.clone())),
        headers: Vec::new(),
        tool_names: None,
    }
}

/// Whether the saved sign-in must be renewed before it can be used again: it is
/// close enough to expiry, or it carries a refresh token worth spending. A cache
/// whose tokens were dropped has neither, so a sign-in starts in the browser.
fn renewable(cached: &CachedToken) -> bool {
    !cached.token.access_token.is_empty()
        && (cached.token.refresh_token.is_some()
            || cached
                .token
                .expires_within(now_millis(), OAuthToken::REFRESH_MARGIN_MS))
}

/// Trades the refresh token for new tokens. A refresh token the server calls
/// unusable is dropped from the cache — the registration and the scopes it was
/// granted stay — so the next sign-in starts in the browser instead of failing
/// the same way again.
async fn refresh(
    http: &reqwest::Client,
    app_dir: &Path,
    token_url: &str,
    cached: &CachedToken,
) -> Result<CachedToken, String> {
    let Some(refresh_token) = cached
        .token
        .refresh_token
        .as_deref()
        .filter(|token| !token.is_empty())
    else {
        return Err("the saved ChatGPT sign-in has no refresh token; sign in again".to_string());
    };
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", cached.client_id.as_str()),
        ("refresh_token", refresh_token),
        ("resource", RESOURCE),
    ];
    let response = http
        .post(token_url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form_body(&form))
        .send()
        .await
        .map_err(|error| format!("could not reach OpenAI's token endpoint: {}", error))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let code = oauth_error(&body);
        if UNUSABLE.iter().any(|unusable| *unusable == code) {
            drop_tokens(app_dir, cached);
        }
        return Err(format!(
            "could not refresh the ChatGPT sign-in (HTTP {} {}); sign in again",
            status, code
        ));
    }
    let email = cached.token.account_label.clone().unwrap_or_default();
    Ok(CachedToken {
        client_id: cached.client_id.clone(),
        subject: cached.subject.clone(),
        token: take_tokens(&cached.token, &body, &email)?,
    })
}

/// Writes the cache back without its tokens: the registration, the scopes it
/// was granted and the account it names stay, so the next sign-in reuses that
/// client — registering a new one for the same machine is the one thing a dead
/// refresh token must not cost.
fn drop_tokens(app_dir: &Path, cached: &CachedToken) {
    let spent = CachedToken {
        client_id: cached.client_id.clone(),
        subject: cached.subject.clone(),
        token: OAuthToken {
            access_token: String::new(),
            refresh_token: None,
            expires_at: 0,
            token_type: String::new(),
            ..cached.token.clone()
        },
    };
    let _ = save_token(app_dir, &spent);
}

/// Takes the tokens from a token-endpoint response. A refresh may leave out the
/// refresh token, the scope or the token type, which then stay as they were.
fn take_tokens(previous: &OAuthToken, body: &Value, email: &str) -> Result<OAuthToken, String> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or("the token response has no access_token")?
        .to_string();
    let expires_in = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(3600);
    Ok(OAuthToken {
        access_token,
        refresh_token: field_or(body, "refresh_token", previous.refresh_token.as_deref()),
        expires_at: now_millis() + expires_in.saturating_mul(1000),
        token_type: field_or(body, "token_type", Some(&previous.token_type))
            .unwrap_or_else(|| "Bearer".to_string()),
        scope: field_or(body, "scope", previous.scope.as_deref()),
        account_label: (!email.is_empty())
            .then(|| email.to_string())
            .or_else(|| previous.account_label.clone()),
    })
}

/// A non-blank string field of a token response, or what the saved token carries.
fn field_or(body: &Value, field: &str, previous: Option<&str>) -> Option<String> {
    body.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| previous.map(str::to_string))
}

/// The token a first sign-in starts from: nothing to carry over.
fn fresh_token() -> OAuthToken {
    OAuthToken {
        access_token: String::new(),
        refresh_token: None,
        expires_at: 0,
        token_type: String::new(),
        scope: None,
        account_label: None,
    }
}

/// A sign-in that didn't grant the plan scope can't be used for anything.
fn ensure_plan_scope(scope: &str, email: &str) -> Result<(), String> {
    if scope.split_whitespace().any(|part| part == PLAN_SCOPE) {
        return Ok(());
    }
    let who = if email.is_empty() { "ChatGPT" } else { email };
    Err(format!(
        "signed in as {}, but ChatGPT plan use wasn't allowed for {}; try again and allow it",
        who, APP_NAME
    ))
}

/// The authorize URL of an attempt: PKCE S256, the loopback redirect, and the
/// identity the account screen can prefill.
fn authorize_url(
    base: &str,
    attempt: &Attempt,
    redirect_uri: &str,
    saved: Option<&CachedToken>,
) -> Result<String, String> {
    // A registration that lost the plan scope is not reused: the account screen
    // has to come back so the scope can be granted again.
    let registered = saved
        .filter(|cached| !cached.client_id.is_empty() && cached.has_plan_scope())
        .map(|cached| cached.client_id.as_str());
    let client_id = registered.unwrap_or(REGISTER);
    let challenge = pkce_challenge(&attempt.verifier);
    let hint = saved
        .and_then(|cached| cached.token.account_label.clone())
        .filter(|label| !label.is_empty())
        .unwrap_or_default();
    let mut params: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPES),
        ("resource", RESOURCE),
        ("state", &attempt.state),
        ("nonce", &attempt.nonce),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge),
    ];
    if registered.is_some() {
        if !hint.is_empty() {
            params.push(("login_hint", &hint));
        }
    } else {
        params.push(("agent_name_hint", APP_NAME));
    }
    reqwest::Url::parse_with_params(base, &params)
        .map(|url| url.to_string())
        .map_err(|error| format!("could not build the ChatGPT authorize URL: {}", error))
}

/// Answers the browser until a request carries this attempt's callback.
async fn wait_for_callback(listener: &TcpListener, state: &str) -> Result<Callback, String> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| format!("lost the browser's answer: {}", error))?;
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        while !request.windows(4).any(|end| end == b"\r\n\r\n") && request.len() < 16_384 {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await {
                Ok(Ok(read)) if read > 0 => request.extend_from_slice(&chunk[..read]),
                _ => break,
            }
        }
        let target = String::from_utf8_lossy(&request)
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_string();
        let (status, page, outcome) = match parse_callback(&target, state) {
            Ok(None) => ("404 Not Found", "Nothing here.".to_string(), None),
            Ok(Some(callback)) => (
                "200 OK",
                format!("Signed in to {}. You can close this tab.", APP_NAME),
                Some(Ok(callback)),
            ),
            Err(message) => (
                "200 OK",
                format!(
                    "Sign-in didn't finish: {}. You can close this tab.",
                    escape(&message)
                ),
                Some(Err(message)),
            ),
        };
        let body = format!(
            "<!doctype html><meta charset=utf-8><title>{}</title><p>{}</p>",
            APP_NAME, page
        );
        let head = format!(
            "HTTP/1.1 {}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            status,
            body.len()
        );
        let _ = stream
            .write_all(format!("{}{}", head, body).as_bytes())
            .await;
        let _ = stream.shutdown().await;
        if let Some(outcome) = outcome {
            return outcome;
        }
    }
}

/// Reads a callback request's `target` (path and query). `Ok(None)` is a request
/// to ignore — another path, or a state from an older attempt — and an `Err`
/// ends the attempt.
fn parse_callback(target: &str, state: &str) -> Result<Option<Callback>, String> {
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{}", target)) else {
        return Ok(None);
    };
    if url.path() != CALLBACK_PATH {
        return Ok(None);
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if param("state").as_deref() != Some(state) {
        return Ok(None);
    }
    if let Some(error) = param("error") {
        return Err(match error.as_str() {
            "access_denied" => format!("ChatGPT plan use wasn't allowed for {}", APP_NAME),
            _ => format!(
                "{} {}",
                error,
                param("error_description").unwrap_or_default()
            )
            .trim()
            .to_string(),
        });
    }
    let code = param("code")
        .filter(|code| !code.is_empty())
        .ok_or("the browser came back without a code")?;
    Ok(Some(Callback {
        code,
        client_id: param("client_id"),
        scope: param("scope"),
    }))
}

/// The client ID after a callback: the one this sign-in registered, or the saved
/// one, which a callback must not replace.
fn issued_client(saved: &str, returned: Option<&str>) -> Result<String, String> {
    match (saved.is_empty(), returned) {
        (true, Some(issued)) if !issued.is_empty() && issued != REGISTER => Ok(issued.to_string()),
        (true, _) => Err(format!(
            "ChatGPT didn't return a client ID for {}, so its registration didn't finish; try again",
            APP_NAME
        )),
        (false, Some(other)) if other != saved => Err(format!(
            "ChatGPT returned a client ID ({}) other than the saved registration ({}); sign-in stopped",
            other, saved
        )),
        (false, _) => Ok(saved.to_string()),
    }
}

/// The ID token's subject and email, after checking its issuer, audience, expiry
/// and nonce. Its signature isn't checked — see the module docs.
fn identity(
    id_token: &str,
    client_id: &str,
    nonce: &str,
    now_secs: u64,
) -> Result<(String, String), String> {
    let claims = id_token
        .split('.')
        .nth(1)
        .and_then(|payload| base64url_decode(payload))
        .and_then(|json| serde_json::from_slice::<Value>(&json).ok())
        .ok_or("the ID token can't be read")?;
    let for_us = match claims.get("aud") {
        Some(Value::Array(audiences)) => audiences.iter().any(|audience| audience == client_id),
        Some(audience) => audience == client_id,
        None => false,
    };
    if claims.get("iss").and_then(Value::as_str) != Some(ISSUER) {
        return Err(format!(
            "the ID token is from {}, not {}",
            claims.get("iss").cloned().unwrap_or(Value::Null),
            ISSUER
        ));
    }
    if !for_us {
        return Err("the ID token is for another client".to_string());
    }
    if claims
        .get("exp")
        .and_then(Value::as_u64)
        .map(|expiry| expiry <= now_secs)
        .unwrap_or(true)
    {
        return Err("the ID token has expired".to_string());
    }
    if claims.get("nonce").and_then(Value::as_str) != Some(nonce) {
        return Err("the ID token's nonce doesn't match this sign-in".to_string());
    }
    let subject = claims
        .get("sub")
        .and_then(Value::as_str)
        .filter(|subject| !subject.is_empty())
        .ok_or("the ID token names no account")?
        .to_string();
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((subject, email))
}

/// The models the plan offers, in the server's order.
async fn fetch_models(
    http: &reqwest::Client,
    access_token: &str,
    models_url: &str,
) -> Result<Vec<ModelRoute>, String> {
    let response = http
        .get(models_url)
        .header("authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!(
            "HTTP {}: {}",
            status,
            truncate(&body.to_string(), 200)
        ));
    }
    Ok(models_from(&body))
}

/// Reads the plan's catalog: `models` entries with `slug` and `visibility`, and
/// when given `context_window`, `supported_reasoning_levels`,
/// `default_reasoning_level` and `input_modalities`. A plain OpenAI list (`data`
/// with `id`s) is read too, so a plain API key on the same endpoint still lists.
fn models_from(catalog: &Value) -> Vec<ModelRoute> {
    let mut models: Vec<ModelRoute> = Vec::new();
    let listed = catalog
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|model| model.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter(|model| takes_text(model));
    for model in listed {
        let Some(id) = model
            .get("slug")
            .and_then(Value::as_str)
            .filter(|slug| !slug.is_empty())
        else {
            continue;
        };
        if models.iter().any(|known| known.id.eq_ignore_ascii_case(id)) {
            continue;
        }
        models.push(ModelRoute {
            id: id.to_string(),
            display_name: id.to_string(),
            context_window: model.get("context_window").and_then(Value::as_u64),
            max_output: None,
            efforts: model
                .get("supported_reasoning_levels")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|level| level.get("effort").and_then(Value::as_str))
                .map(str::to_string)
                .collect(),
            default_effort: model
                .get("default_reasoning_level")
                .and_then(Value::as_str)
                .map(str::to_string),
            price: None,
            source: ModelSource::Endpoint,
        });
    }
    if models.is_empty() {
        models = catalog
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|model| model.get("id").and_then(Value::as_str))
            .map(|id| ModelRoute {
                id: id.to_string(),
                display_name: id.to_string(),
                context_window: None,
                max_output: None,
                efforts: Vec::new(),
                default_effort: None,
                price: None,
                source: ModelSource::Endpoint,
            })
            .collect();
    }
    models
}

/// The Pair sends text only, so a model that declares modalities without text is
/// not offered.
fn takes_text(model: &Value) -> bool {
    model
        .get("input_modalities")
        .and_then(Value::as_array)
        .map(|modalities| modalities.iter().any(|modality| modality == "text"))
        .unwrap_or(true)
}

fn token_path(app_dir: &Path) -> std::path::PathBuf {
    oauth_token_path(app_dir, "chatgpt")
}

fn save_token(app_dir: &Path, cached: &CachedToken) -> Result<(), String> {
    let bytes = serde_json::to_vec(cached)
        .map_err(|error| format!("could not serialize the ChatGPT token: {}", error))?;
    write_token_file(&token_path(app_dir), &bytes)
}

/// An unreadable or corrupt cache is treated as absent: signing in again is
/// always possible.
fn load_token(app_dir: &Path) -> Option<CachedToken> {
    let bytes = std::fs::read(token_path(app_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Listens for the browser's answer on 127.0.0.1, preferring `port` and falling
/// back to an ephemeral one; the port actually bound goes into `redirect_uri`.
async fn bind_callback(port: u16) -> Result<(TcpListener, String), String> {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(_) => TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| format!("could not listen for the browser's answer: {}", error))?,
    };
    let bound = listener
        .local_addr()
        .map_err(|error| format!("could not read the callback port: {}", error))?;
    Ok((
        listener,
        format!("http://127.0.0.1:{}{}", bound.port(), CALLBACK_PATH),
    ))
}

/// Opens the system browser through the opener plugin, which knows the command
/// on every platform The Pair builds for: `open` and `xdg-open` are not on
/// Windows. A browser that can't be opened is reported, not swallowed, so the
/// caller can put the URL in front of the user instead of waiting for a
/// callback that will never come.
fn open_browser(url: &str) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("there is no sign-in URL to open".to_string());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|error| error.to_string())
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("could not build an HTTP client: {}", error))
}

/// The machine-readable code of an OAuth error body: `{"error": "code"}` or
/// `{"error": {"code": "code"}}`.
fn oauth_error(body: &Value) -> String {
    body.get("error")
        .and_then(|error| {
            error
                .as_str()
                .or_else(|| error.get("code").and_then(Value::as_str))
        })
        .unwrap_or_default()
        .to_string()
}

fn now_secs() -> u64 {
    now_millis() / 1000
}

/// Percent-encodes one form value: everything but the unreserved set.
fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

/// An `application/x-www-form-urlencoded` body, which is how the token endpoint
/// takes its parameters.
fn form_body(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(name, value)| format!("{}={}", form_encode(name), form_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn truncate(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push('…');
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// Random bytes for the verifier, state and nonce. `uuid` v4 draws from the
/// platform's random source, so the pieces are chained to reach any length.
fn random_bytes(count: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(count);
    while bytes.len() < count {
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    bytes.truncate(count);
    bytes
}

/// 32 random bytes in base64url: 43 characters, as a PKCE verifier needs.
fn random_token() -> String {
    base64url_encode(&random_bytes(32))
}

/// The PKCE S256 challenge for `verifier`.
fn pkce_challenge(verifier: &str) -> String {
    base64url_encode(&sha256(verifier.as_bytes()))
}

const BASE64URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let triple = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        let sextets = [
            (triple >> 18) as usize & 0x3f,
            (triple >> 12) as usize & 0x3f,
            (triple >> 6) as usize & 0x3f,
            triple as usize & 0x3f,
        ];
        for (index, sextet) in sextets.iter().enumerate() {
            if index >= chunk.len() + 1 {
                break;
            }
            out.push(BASE64URL_ALPHABET[*sextet] as char);
        }
    }
    out
}

/// Base64url without padding, ignoring any `=` a token still carries.
fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for symbol in text.chars() {
        if symbol == '=' {
            continue;
        }
        let value = match symbol {
            'A'..='Z' => u32::from(symbol) - u32::from('A'),
            'a'..='z' => u32::from(symbol) - u32::from('a') + 26,
            '0'..='9' => u32::from(symbol) - u32::from('0') + 52,
            '-' => 62,
            '_' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    // A trailing group of six bits carries no byte, so the input was truncated.
    (bits < 6).then_some(out)
}

/// SHA-256, for the PKCE challenge. Hand-rolled: the crate takes no hashing
/// dependency and this flow needs one fixed-size digest.
fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "the-pair-chatgpt-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ))
    }

    fn cached(label: &str, expires_in_ms: u64) -> CachedToken {
        cached_with(label, Some("refresh-1"), expires_in_ms)
    }

    fn cached_with(label: &str, refresh: Option<&str>, expires_in_ms: u64) -> CachedToken {
        CachedToken {
            client_id: "oaiapp_1".into(),
            subject: "user-1".into(),
            token: OAuthToken {
                access_token: "access-1".into(),
                refresh_token: refresh.map(str::to_string),
                expires_at: now_millis() + expires_in_ms,
                token_type: "Bearer".into(),
                scope: Some(format!("openid offline_access {}", PLAN_SCOPE)),
                account_label: Some(label.into()),
            },
        }
    }

    fn jwt(claims: &str) -> String {
        format!(
            "{}.{}.sig",
            base64url_encode(b"{\"alg\":\"none\"}"),
            base64url_encode(claims.as_bytes())
        )
    }

    /// An HTTP server on a free port that answers each request with the next
    /// `(status, body)` pair and records the bodies it was sent.
    async fn stub(replies: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        tokio::spawn(async move {
            for (status, reply) in replies {
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
                    let text = String::from_utf8_lossy(&request).to_string();
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
                        recorder.lock().unwrap().push(text[end + 4..].to_string());
                        break;
                    }
                }
                let response = format!(
                    "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    status,
                    reply.len(),
                    reply
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (url, seen)
    }

    fn test_flow<'a>(token: &'a str, models: &'a str) -> Flow<'a> {
        Flow {
            authorize: "https://auth.openai.test/api/accounts/authorize",
            token,
            models,
            open_browser: false,
        }
    }

    #[test]
    fn the_pkce_challenge_matches_rfc_7636() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(
            sha256(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
        assert_eq!(
            base64url_encode(&sha256(b"abc")),
            "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0"
        );
        assert_eq!(random_token().len(), 43);
        for (plain, encoded) in [("", ""), ("f", "Zg"), ("fo", "Zm8"), ("foo", "Zm9v")] {
            assert_eq!(base64url_encode(plain.as_bytes()), encoded);
        }
        assert_eq!(base64url_decode("YWJj"), Some(b"abc".to_vec()));
        assert_eq!(base64url_decode("YWJ"), Some(b"ab".to_vec()));
        assert_eq!(base64url_decode("YQ=="), Some(b"a".to_vec()));
        assert_eq!(base64url_decode("Y"), None, "a lone symbol is not a byte");
        assert_eq!(base64url_decode("!!"), None);
    }

    #[test]
    fn the_authorize_url_carries_the_pkce_and_identity_params() {
        let attempt = Attempt {
            state: "s".into(),
            nonce: "n".into(),
            verifier: "v".into(),
        };
        let params = |saved: Option<&CachedToken>| -> Vec<(String, String)> {
            reqwest::Url::parse(
                &authorize_url(
                    "https://auth.openai.test/api/accounts/authorize",
                    &attempt,
                    "http://127.0.0.1:1455/auth/callback",
                    saved,
                )
                .unwrap(),
            )
            .unwrap()
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
        };
        let get = |params: &[(String, String)], key: &str| {
            params
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        };

        let first = params(None);
        assert_eq!(get(&first, "response_type").as_deref(), Some("code"));
        assert_eq!(get(&first, "client_id").as_deref(), Some(REGISTER));
        assert_eq!(get(&first, "agent_name_hint").as_deref(), Some(APP_NAME));
        assert_eq!(get(&first, "login_hint"), None);
        assert_eq!(
            get(&first, "code_challenge").as_deref(),
            Some(pkce_challenge("v").as_str())
        );
        assert_eq!(
            get(&first, "code_challenge_method").as_deref(),
            Some("S256")
        );
        assert_eq!(get(&first, "state").as_deref(), Some("s"));
        assert_eq!(get(&first, "nonce").as_deref(), Some("n"));
        assert_eq!(get(&first, "resource").as_deref(), Some(RESOURCE));
        assert_eq!(
            get(&first, "redirect_uri").as_deref(),
            Some("http://127.0.0.1:1455/auth/callback")
        );
        assert!(get(&first, "scope").unwrap().contains(PLAN_SCOPE));

        let registered = cached("me@example.com", 3_600_000);
        let again = params(Some(&registered));
        assert_eq!(get(&again, "client_id").as_deref(), Some("oaiapp_1"));
        assert_eq!(get(&again, "agent_name_hint"), None);
        assert_eq!(get(&again, "login_hint").as_deref(), Some("me@example.com"));

        let declined = CachedToken {
            token: OAuthToken {
                scope: Some("openid".into()),
                ..registered.token.clone()
            },
            ..registered.clone()
        };
        assert_eq!(
            get(&params(Some(&declined)), "client_id").as_deref(),
            Some(REGISTER),
            "a registration that lost the plan scope is not reused"
        );
    }

    #[test]
    fn only_this_attempts_callback_ends_the_wait() {
        assert_eq!(
            parse_callback(
                "/auth/callback?code=c1&state=s1&client_id=oaiapp_9&scope=openid+chatgpt.tokens.use.direct",
                "s1"
            ),
            Ok(Some(Callback {
                code: "c1".into(),
                client_id: Some("oaiapp_9".into()),
                scope: Some("openid chatgpt.tokens.use.direct".into())
            }))
        );
        assert_eq!(
            parse_callback("/auth/callback?code=c1&state=old", "s1"),
            Ok(None)
        );
        assert_eq!(parse_callback("/favicon.ico", "s1"), Ok(None));
        assert_eq!(
            parse_callback("/callback?code=c1&state=s1", "s1"),
            Ok(None),
            "the path must match exactly"
        );
        assert!(
            parse_callback("/auth/callback?error=access_denied&state=s1", "s1")
                .unwrap_err()
                .contains("wasn't allowed")
        );
        assert!(parse_callback("/auth/callback?state=s1", "s1").is_err());
    }

    #[tokio::test]
    async fn the_callback_server_answers_the_browser_until_the_callback_comes() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = tokio::spawn(async move {
            let mut pages = Vec::new();
            for target in [
                "/favicon.ico",
                "/auth/callback?code=c&state=old",
                "/auth/callback?code=c&state=s&client_id=oaiapp_1",
            ] {
                let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .unwrap();
                stream
                    .write_all(
                        format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", target).as_bytes(),
                    )
                    .await
                    .unwrap();
                let mut page = String::new();
                stream.read_to_string(&mut page).await.unwrap();
                pages.push(page);
            }
            pages
        });

        let callback = wait_for_callback(&listener, "s").await.unwrap();
        assert_eq!(
            (callback.code.as_str(), callback.client_id.as_deref()),
            ("c", Some("oaiapp_1"))
        );
        let pages = browser.await.unwrap();
        assert!(
            pages[0].starts_with("HTTP/1.1 404") && pages[1].starts_with("HTTP/1.1 404"),
            "{:?}",
            pages
        );
        assert!(
            pages[2].starts_with("HTTP/1.1 200") && pages[2].contains("Signed in to The Pair"),
            "{}",
            pages[2]
        );
    }

    #[tokio::test]
    async fn the_callback_port_falls_back_when_1455_is_taken() {
        let taken = tokio::net::TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
            .await
            .unwrap();
        let (listener, redirect_uri) = bind_callback(CALLBACK_PORT).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_ne!(port, CALLBACK_PORT, "the busy port must not be reused");
        assert_eq!(
            redirect_uri,
            format!("http://127.0.0.1:{}{}", port, CALLBACK_PATH)
        );
        drop(taken);
    }

    #[test]
    fn the_id_token_must_be_for_this_sign_in() {
        let claims = |aud: Value, nonce: &str, exp: u64| {
            jwt(&serde_json::json!({
                "iss": ISSUER,
                "aud": aud,
                "nonce": nonce,
                "exp": exp,
                "sub": "user-1",
                "email": "a@b.c"
            })
            .to_string())
        };
        let identified = identity(
            &claims("oaiapp_1".into(), "n", 2_000),
            "oaiapp_1",
            "n",
            1_000,
        )
        .unwrap();
        assert_eq!(identified, ("user-1".to_string(), "a@b.c".to_string()));
        assert!(
            identity(
                &claims(serde_json::json!(["x", "oaiapp_1"]), "n", 2_000),
                "oaiapp_1",
                "n",
                1_000
            )
            .is_ok(),
            "an audience list may name the client"
        );
        assert!(identity(
            &claims("oaiapp_2".into(), "n", 2_000),
            "oaiapp_1",
            "n",
            1_000
        )
        .is_err());
        assert!(identity(
            &claims("oaiapp_1".into(), "other", 2_000),
            "oaiapp_1",
            "n",
            1_000
        )
        .is_err());
        assert!(identity(
            &claims("oaiapp_1".into(), "n", 1_000),
            "oaiapp_1",
            "n",
            1_000
        )
        .is_err());
        assert!(
            identity(
                &jwt(
                    &serde_json::json!({"aud": "oaiapp_1", "nonce": "n", "exp": 9_999, "sub": "u"})
                        .to_string()
                ),
                "oaiapp_1",
                "n",
                1_000
            )
            .is_err(),
            "another issuer is refused"
        );
        assert!(identity("not-a-jwt", "oaiapp_1", "n", 1_000).is_err());
    }

    #[test]
    fn a_sign_in_without_plan_scope_is_refused() {
        assert!(ensure_plan_scope(&format!("openid {}", PLAN_SCOPE), "a@b.c").is_ok());
        let refused = ensure_plan_scope("openid email", "a@b.c").unwrap_err();
        assert!(
            refused.contains("a@b.c") && refused.contains("plan use"),
            "{}",
            refused
        );
        assert!(ensure_plan_scope("", "").unwrap_err().contains("ChatGPT"));
    }

    #[test]
    fn the_token_expiry_respects_the_refresh_margin() {
        let fresh = cached_with(
            "me@example.com",
            None,
            OAuthToken::REFRESH_MARGIN_MS + 1_000,
        );
        assert!(
            !renewable(&fresh),
            "a fresh token is not renewed on its own"
        );
        let expiring = cached("me@example.com", OAuthToken::REFRESH_MARGIN_MS - 1_000);
        assert!(renewable(&expiring));
        let expired = cached_with("me@example.com", None, 0);
        assert!(renewable(&expired), "an expired token is always renewed");
        let dropped = CachedToken {
            token: OAuthToken {
                account_label: Some("me@example.com".into()),
                ..fresh_token()
            },
            ..expired.clone()
        };
        assert!(
            !renewable(&dropped),
            "a cache whose tokens were dropped sends the user to the browser"
        );
    }

    #[test]
    fn a_refresh_without_a_new_refresh_token_keeps_the_old_one() {
        let saved = cached("me@example.com", 0);
        let taken = take_tokens(
            &saved.token,
            &serde_json::json!({"access_token": "new", "expires_in": 60}),
            "me@example.com",
        )
        .unwrap();
        assert_eq!(taken.access_token, "new");
        assert_eq!(taken.refresh_token, Some("refresh-1".into()));
        assert_eq!(taken.token_type, "Bearer");
        assert_eq!(taken.account_label.as_deref(), Some("me@example.com"));
        assert_eq!(
            taken.scope.as_deref(),
            saved.token.scope.as_deref(),
            "the plan scope survives a refresh that leaves it out"
        );
        assert!(
            taken.expires_at > now_millis(),
            "expires_at is absolute, not a duration"
        );
        assert!(take_tokens(&fresh_token(), &serde_json::json!({}), "").is_err());
    }

    #[test]
    fn the_catalog_lists_plan_models_with_what_they_take() {
        let catalog = serde_json::json!({"models": [
            {"slug": "gpt-a", "visibility": "list", "context_window": 272000, "default_reasoning_level": "medium",
             "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}], "input_modalities": ["text", "image"]},
            {"slug": "hidden", "visibility": "hide"},
            {"slug": "gpt-a", "visibility": "list"},
            {"slug": "image-only", "visibility": "list", "input_modalities": ["image"]},
            {"slug": "gpt-b", "visibility": "list"}
        ]});
        let models = models_from(&catalog);
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["gpt-a", "gpt-b"],
            "hidden, duplicate and non-text entries are dropped"
        );
        assert_eq!(models[0].context_window, Some(272_000));
        assert_eq!(models[0].default_effort.as_deref(), Some("medium"));
        assert_eq!(models[0].efforts, ["low", "high"]);
        assert_eq!(models[0].source, ModelSource::Endpoint);
        assert!(models[1].efforts.is_empty());
        assert_eq!(
            models_from(&serde_json::json!({"data": [{"id": "gpt-c"}]}))[0].id,
            "gpt-c"
        );
        assert!(models_from(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn a_missing_token_cache_starts_a_fresh_sign_in() {
        let dir = scratch("missing");
        assert!(load_token(&dir).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_token_cache_is_ignored() {
        let dir = scratch("corrupt");
        fs::create_dir_all(dir.join("oauth")).unwrap();
        fs::write(dir.join("oauth/chatgpt.json"), "{ not json").unwrap();
        assert!(load_token(&dir).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_saved_registration_is_reused_on_the_next_sign_in() {
        let dir = scratch("reuse");
        let saved = CachedToken {
            token: OAuthToken {
                refresh_token: None,
                ..cached("me@example.com", 3_600_000).token
            },
            ..cached("me@example.com", 3_600_000)
        };
        save_token(&dir, &saved).unwrap();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let redirect_uri = format!("http://127.0.0.1:{}{}", port, CALLBACK_PATH);
        let attempt = Attempt {
            state: "state-1".into(),
            nonce: "nonce-1".into(),
            verifier: "verifier-1".into(),
        };
        let id_token = jwt(&serde_json::json!({
            "iss": ISSUER,
            "aud": "oaiapp_1",
            "nonce": "nonce-1",
            "exp": now_secs() + 600,
            "sub": "user-1",
            "email": "me@example.com"
        })
        .to_string());
        let (token, bodies) = stub(vec![(
            200,
            format!(
                r#"{{"access_token": "access-2", "expires_in": 3600, "id_token": "{}"}}"#,
                id_token
            ),
        )])
        .await;
        let (models, _) = stub(vec![(200, r#"{"data": []}"#.to_string())]).await;
        let flow = test_flow(&token, &models);
        let http = http_client().unwrap();
        tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(
                    format!(
                        "GET {}?code=code-2&state=state-1&scope=openid+chatgpt.tokens.use.direct HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                        CALLBACK_PATH
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut page = String::new();
            stream.read_to_string(&mut page).await.unwrap();
        });

        let outcome = browser_flow(
            &http,
            &dir,
            &flow,
            &listener,
            &redirect_uri,
            &attempt,
            Some(&saved),
        )
        .await
        .unwrap();

        let cached = load_token(&dir).unwrap();
        assert_eq!(cached.client_id, "oaiapp_1", "the registration is kept");
        assert_eq!(cached.subject, "user-1");
        assert!(
            bodies.lock().unwrap()[0].contains("client_id=oaiapp_1"),
            "the saved client spends the code"
        );
        assert_eq!(outcome.account.label, "me@example.com");
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn an_expiring_token_is_refreshed_before_the_browser() {
        let dir = scratch("refresh");
        let saved = cached("me@example.com", OAuthToken::REFRESH_MARGIN_MS - 1_000);
        save_token(&dir, &saved).unwrap();
        let (token, bodies) = stub(vec![(
            200,
            r#"{"access_token": "new-access", "expires_in": 3600}"#.to_string(),
        )])
        .await;
        let (models, _) = stub(vec![(200, r#"{"data": [{"id": "gpt-a"}]}"#.to_string())]).await;

        let outcome = sign_in(&dir, &test_flow(&token, &models)).await.unwrap();

        assert_eq!(outcome.models[0].id, "gpt-a");
        match &outcome.account.secret {
            Secret::OAuth(token) => {
                assert_eq!(token.access_token, "new-access");
                assert_eq!(token.refresh_token.as_deref(), Some("refresh-1"));
                assert_eq!(token.scope.as_deref(), saved.token.scope.as_deref());
            }
            other => panic!("expected an OAuth secret, got {:?}", other),
        }
        let form = bodies.lock().unwrap()[0].clone();
        for field in [
            "grant_type=refresh_token",
            "client_id=oaiapp_1",
            "refresh_token=refresh-1",
            "resource=https%3A%2F%2Fapi.openai.com%2Fv1",
        ] {
            assert!(form.contains(field), "{}", form);
        }
        let written = load_token(&dir).expect("the refreshed token is written back");
        assert_eq!(written.token.access_token, "new-access");
        assert_eq!(written.client_id, "oaiapp_1");
        assert_eq!(written.subject, "user-1");
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_turn_renews_an_expiring_sign_in_without_a_browser() {
        let dir = scratch("renew");
        save_token(&dir, &cached("me@example.com", 0)).unwrap();
        let (token, bodies) = stub(vec![(
            200,
            r#"{"access_token": "new-access", "refresh_token": "refresh-2", "expires_in": 3600}"#
                .to_string(),
        )])
        .await;

        let renewed = renew_at(&dir, &token).await.unwrap();

        assert_eq!(renewed.access_token, "new-access");
        assert!(!renewed.is_expired(now_millis()));
        assert!(bodies.lock().unwrap()[0].contains("refresh_token=refresh-1"));
        let written = load_token(&dir).expect("the renewed token is written back");
        assert_eq!(written.token.refresh_token.as_deref(), Some("refresh-2"));

        // A second turn arriving just behind finds the renewed token and spends
        // nothing: the stub has no second reply to give.
        let again = renew_at(&dir, &token).await.unwrap();
        assert_eq!(again.access_token, "new-access");
        assert_eq!(bodies.lock().unwrap().len(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn renewing_without_a_saved_sign_in_is_a_readable_error() {
        let dir = scratch("renew-none");
        let error = renew_at(&dir, "http://127.0.0.1:9/unreachable")
            .await
            .unwrap_err();
        assert!(error.contains("sign in again"), "{}", error);
    }

    #[tokio::test]
    async fn a_rotated_refresh_token_reaches_the_disk() {
        let dir = scratch("refresh-rotated");
        let saved = cached("me@example.com", 0);
        save_token(&dir, &saved).unwrap();
        let (token, _) = stub(vec![(
            200,
            r#"{"access_token": "new-access", "refresh_token": "refresh-2", "expires_in": 3600}"#
                .to_string(),
        )])
        .await;
        let (models, _) = stub(vec![(200, r#"{"data": []}"#.to_string())]).await;

        sign_in(&dir, &test_flow(&token, &models)).await.unwrap();

        let written = load_token(&dir).expect("the refresh is persisted");
        assert_eq!(
            written.token.refresh_token.as_deref(),
            Some("refresh-2"),
            "OpenAI rotates the refresh token, so the spent one must not stay on disk"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_failed_refresh_returns_a_readable_error() {
        let dir = scratch("refresh-failed");
        let saved = cached("me@example.com", 0);
        save_token(&dir, &saved).unwrap();
        let (token, _) = stub(vec![(400, r#"{"error": "invalid_grant"}"#.to_string())]).await;

        let error = sign_in(&dir, &test_flow(&token, "http://127.0.0.1:9/unreachable"))
            .await
            .unwrap_err();

        assert!(
            error.contains("invalid_grant") && error.contains("sign in again"),
            "{}",
            error
        );
        let kept = load_token(&dir).expect("the registration survives");
        assert_eq!(kept.client_id, "oaiapp_1");
        assert_eq!(kept.subject, "user-1");
        assert_eq!(kept.token.account_label.as_deref(), Some("me@example.com"));
        assert_eq!(
            kept.token.scope.as_deref(),
            saved.token.scope.as_deref(),
            "the granted scopes stay, so the registration is reused as it was"
        );
        assert!(
            kept.token.access_token.is_empty() && kept.token.refresh_token.is_none(),
            "the spent token is gone: {:?}",
            kept.token
        );
        assert!(
            !renewable(&kept),
            "the next sign-in goes to the browser instead of refreshing again"
        );
        assert!(
            kept.has_plan_scope(),
            "the next authorize URL reuses the registration instead of making a new one"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_refresh_without_a_refresh_token_is_refused() {
        let dir = scratch("no-refresh");
        let saved = CachedToken {
            token: OAuthToken {
                refresh_token: None,
                ..cached("me@example.com", 0).token
            },
            ..cached("me@example.com", 0)
        };
        save_token(&dir, &saved).unwrap();
        let http = http_client().unwrap();

        let error = refresh(&http, &dir, "http://127.0.0.1:9/unreachable", &saved)
            .await
            .unwrap_err();

        assert!(error.contains("no refresh token"), "{}", error);
        assert!(
            load_token(&dir).is_some(),
            "a transient refusal keeps the cache"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn signs_in_through_the_browser_and_caches_the_token() {
        let dir = scratch("signin");
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let redirect_uri = format!("http://127.0.0.1:{}{}", port, CALLBACK_PATH);
        let attempt = Attempt {
            state: "state-1".into(),
            nonce: "nonce-1".into(),
            verifier: "verifier-1".into(),
        };
        let id_token = jwt(&serde_json::json!({
            "iss": ISSUER,
            "aud": "oaiapp_1",
            "nonce": "nonce-1",
            "exp": now_secs() + 600,
            "sub": "user-1",
            "email": "me@example.com"
        })
        .to_string());
        let (token, bodies) = stub(vec![(
            200,
            format!(
                r#"{{"access_token": "access-1", "refresh_token": "refresh-1", "expires_in": 3600, "id_token": "{}"}}"#,
                id_token
            ),
        )])
        .await;
        let (models, _) = stub(vec![(
            200,
            r#"{"models": [{"slug": "gpt-5-codex", "visibility": "list", "context_window": 272000,
                 "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}],
                 "input_modalities": ["text", "image"]}]}"#
                .to_string(),
        )])
        .await;
        let flow = test_flow(&token, &models);
        let http = http_client().unwrap();
        let browser = tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(
                    format!(
                        "GET {}?code=code-1&state=state-1&client_id=oaiapp_1&scope=openid+chatgpt.tokens.use.direct HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                        CALLBACK_PATH
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut page = String::new();
            stream.read_to_string(&mut page).await.unwrap();
            page
        });

        let outcome = browser_flow(&http, &dir, &flow, &listener, &redirect_uri, &attempt, None)
            .await
            .unwrap();

        let page = browser.await.unwrap();
        assert!(page.starts_with("HTTP/1.1 200"), "{}", page);
        assert_eq!(outcome.protocol, Protocol::Responses);
        assert_eq!(outcome.base_url, RESOURCE);
        assert_eq!(outcome.account.kind, AccountKind::ChatGptOAuth);
        assert_eq!(outcome.account.source, "oauth:chatgpt");
        assert_eq!(outcome.account.label, "me@example.com");
        assert_eq!(outcome.models.len(), 1);
        assert_eq!(outcome.models[0].id, "gpt-5-codex");
        assert_eq!(outcome.models[0].context_window, Some(272_000));
        assert_eq!(outcome.models[0].efforts, ["low", "high"]);
        assert!(outcome.notes.iter().any(|note| note.contains("usage")));
        match &outcome.account.secret {
            Secret::OAuth(token) => assert_eq!(token.access_token, "access-1"),
            other => panic!("expected an OAuth secret, got {:?}", other),
        }
        let form = bodies.lock().unwrap()[0].clone();
        for field in [
            "grant_type=authorization_code",
            "client_id=oaiapp_1",
            "code=code-1",
            "code_verifier=verifier-1",
            "redirect_uri=http%3A%2F%2F127.0.0.1",
        ] {
            assert!(form.contains(field), "{}", form);
        }
        let saved = load_token(&dir).unwrap();
        assert_eq!(saved.client_id, "oaiapp_1");
        assert_eq!(saved.subject, "user-1");
        assert!(saved.has_plan_scope());
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_callback_that_denies_the_plan_scope_ends_the_sign_in() {
        let dir = scratch("denied");
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let redirect_uri = format!("http://127.0.0.1:{}{}", port, CALLBACK_PATH);
        let attempt = Attempt {
            state: "state-1".into(),
            nonce: "nonce-1".into(),
            verifier: "verifier-1".into(),
        };
        let id_token = jwt(&serde_json::json!({
            "iss": ISSUER,
            "aud": "oaiapp_1",
            "nonce": "nonce-1",
            "exp": now_secs() + 600,
            "sub": "user-1",
            "email": "me@example.com"
        })
        .to_string());
        let (token, _) = stub(vec![(
            200,
            format!(
                r#"{{"access_token": "access-1", "expires_in": 3600, "id_token": "{}"}}"#,
                id_token
            ),
        )])
        .await;
        let flow = test_flow(&token, "http://127.0.0.1:9/unreachable");
        let http = http_client().unwrap();
        tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(
                    format!(
                        "GET {}?code=code-1&state=state-1&client_id=oaiapp_1&scope=openid+email HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                        CALLBACK_PATH
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut page = String::new();
            stream.read_to_string(&mut page).await.unwrap();
            page
        });

        let error = browser_flow(&http, &dir, &flow, &listener, &redirect_uri, &attempt, None)
            .await
            .unwrap_err();

        assert!(error.contains("plan use"), "{}", error);
        assert!(
            load_token(&dir).is_none(),
            "nothing is cached for a refused sign-in"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn another_account_cannot_take_the_saved_registration_over() {
        let dir = scratch("switch");
        let saved = cached("me@example.com", 3_600_000);
        save_token(&dir, &saved).unwrap();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let redirect_uri = format!("http://127.0.0.1:{}{}", port, CALLBACK_PATH);
        let attempt = Attempt {
            state: "state-1".into(),
            nonce: "nonce-1".into(),
            verifier: "verifier-1".into(),
        };
        let id_token = jwt(&serde_json::json!({
            "iss": ISSUER,
            "aud": "oaiapp_1",
            "nonce": "nonce-1",
            "exp": now_secs() + 600,
            "sub": "user-2",
            "email": "other@example.com"
        })
        .to_string());
        let (token, _) = stub(vec![(
            200,
            format!(
                r#"{{"access_token": "access-2", "expires_in": 3600, "id_token": "{}"}}"#,
                id_token
            ),
        )])
        .await;
        let flow = test_flow(&token, "http://127.0.0.1:9/unreachable");
        let http = http_client().unwrap();
        tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(
                    format!(
                        "GET {}?code=code-2&state=state-1&client_id=oaiapp_1&scope=openid+chatgpt.tokens.use.direct HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                        CALLBACK_PATH
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            let mut page = String::new();
            stream.read_to_string(&mut page).await.unwrap();
            page
        });

        let error = browser_flow(
            &http,
            &dir,
            &flow,
            &listener,
            &redirect_uri,
            &attempt,
            Some(&saved),
        )
        .await
        .unwrap_err();

        assert!(
            error.contains("different ChatGPT account") && error.contains("me@example.com"),
            "{}",
            error
        );
        let kept = load_token(&dir).expect("the saved sign-in stays as it was");
        assert_eq!(kept.subject, "user-1");
        assert_eq!(kept.token.access_token, "access-1");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_blank_url_is_never_handed_to_the_opener() {
        // The opener shells out to the platform's browser command; a blank URL
        // would only raise a confusing error window.
        assert!(open_browser("").is_err());
        assert!(open_browser("   ").is_err());
    }
}
