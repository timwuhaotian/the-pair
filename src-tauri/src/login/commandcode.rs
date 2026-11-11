//! Command Code sign-in: the subscription behind `api.commandcode.ai`.
//!
//! The studio's CLI login page hands a fresh API key back to a loopback
//! callback, the key is checked against `/alpha/whoami`, and the subscription's
//! model catalog is pulled from `/provider/v1/models?view=client`. The key has
//! no refresh token, so it is stored as an [`OAuthToken`] whose access token is
//! the key and whose expiry is "until revoked".
//!
//! Two entry points, both ending in the same [`LoginOutcome`]:
//!
//! - [`login`] opens the browser and waits for the callback;
//! - [`login_with_key`] takes a pasted key, for a browser that can't come back.
//!
//! The callback arrives as either a JSON post:
//!
//! ```json
//! {"apiKey":"cc-...","state":"<state>","userId":"u","userName":"tim","keyName":"cli"}
//! ```
//!
//! or a form post (which is redirected to the completion page):
//!
//! ```text
//! apiKey=cc-...&state=<state>&userId=u&userName=tim&keyName=cli
//! ```
//!
//! The catalog is a plain model list:
//!
//! ```json
//! {"data":[{"id":"claude-sonnet-5-5","context_length":1000000,
//!           "inputModalities":["text","image"],"reasoningEfforts":["low","high"]}]}
//! ```

use super::LoginOutcome;
use crate::accounts::commands::{oauth_token_path, write_token_file};
use crate::accounts::registry::AccountDraft;
use crate::accounts::types::{AccountKind, ModelRoute, ModelSource, OAuthToken, Protocol, Secret};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Where the CLI's gateway answers.
const BASE_URL: &str = "https://api.commandcode.ai";
/// Where the account behind the subscription lives.
const STUDIO: &str = "https://commandcode.ai";
const WHOAMI_PATH: &str = "/alpha/whoami";
/// The subscription's model catalog, readable without a key.
const CATALOG_PATH: &str = "/provider/v1/models?view=client";
const CALLBACK_PATH: &str = "/callback";
const COMPLETE_PATH: &str = "/callback/complete";
/// How long the login waits for the browser to come back.
const SIGN_IN_WAIT: Duration = Duration::from_secs(300);
/// The largest callback body, mirroring the CLI's cap.
const MAX_CALLBACK_BODY: usize = 10_000;
/// The CLI version the gateway expects on `/alpha/whoami`.
const CLI_VERSION: &str = "1.88.5";
/// Provenance recorded on the account and its cached token file.
const ACCOUNT_SOURCE: &str = "oauth:commandcode";

/// Signs in through the browser: opens the studio's CLI login page, waits for
/// the key on the loopback callback, then verifies it and lists the models.
pub async fn login(app_dir: &Path) -> Result<LoginOutcome, String> {
    let state = random_state();
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| format!("could not listen for the browser's answer: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("could not read the callback port: {error}"))?
        .port();
    let url = auth_url(port, &state)?;
    open_browser(url.as_str())
        .map_err(|error| format!("could not open your browser ({error}); open {url} yourself"))?;
    let waited = tokio::time::timeout(SIGN_IN_WAIT, wait_for_callback(&listener, &state)).await;
    drop(listener);
    let callback = match waited {
        Ok(Ok(callback)) => callback,
        Ok(Err(reason)) => {
            return Err(format!(
                "Command Code sign-in did not complete ({reason}). Try again, or paste an API key instead."
            ));
        }
        Err(_) => {
            return Err(
                "The browser did not come back in time. Try again, or paste an API key instead."
                    .to_string(),
            );
        }
    };
    finish(app_dir, callback.api_key, callback.user_name).await
}

/// Signs in with a pasted key, for a browser that can't open or come back.
///
/// A second entry point beside [`login`]: `login::launch` can only run the
/// browser flow (it takes no key), so a caller that already holds a key calls
/// this directly instead of re-opening the studio page.
pub async fn login_with_key(app_dir: &Path, key: &str) -> Result<LoginOutcome, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Paste your Command Code API key to finish signing in.".to_string());
    }
    finish(app_dir, key.to_string(), String::new()).await
}

/// Verifies the key, lists the subscription's models, and caches the token.
async fn finish(
    app_dir: &Path,
    api_key: String,
    fallback_name: String,
) -> Result<LoginOutcome, String> {
    let http = http_client();
    let who = whoami(&http, &api_key).await?;
    let name = if fallback_name.is_empty() {
        whoami_name(&who)
    } else {
        fallback_name
    };
    let entries = catalog(&http).await?;
    let models = catalog_models(&entries);
    if models.is_empty() {
        return Err("Command Code listed no models.".to_string());
    }

    let token = OAuthToken {
        access_token: api_key,
        refresh_token: None,
        expires_at: u64::MAX,
        token_type: "Bearer".to_string(),
        scope: None,
        account_label: (!name.is_empty()).then(|| name.clone()),
    };
    let mut notes = vec![
        "Requests through this account count toward your Command Code subscription usage."
            .to_string(),
    ];
    match serde_json::to_vec(&token) {
        Ok(bytes) => {
            if let Err(error) = write_token_file(&oauth_token_path(app_dir, "commandcode"), &bytes)
            {
                notes.push(format!("Could not cache the Command Code token: {error}"));
            }
        }
        Err(error) => notes.push(format!(
            "Could not serialize the Command Code token: {error}"
        )),
    }

    let label = if name.is_empty() {
        "Command Code".to_string()
    } else {
        name
    };
    Ok(LoginOutcome {
        protocol: Protocol::CommandCode,
        base_url: BASE_URL.to_string(),
        vendor: None,
        display_name: Some("Command Code".to_string()),
        account: AccountDraft {
            kind: AccountKind::CommandCodeOAuth,
            label,
            source: ACCOUNT_SOURCE.to_string(),
            secret: Secret::OAuth(Box::new(token)),
            headers: Vec::new(),
            tool_names: None,
        },
        models,
        notes,
    })
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("cli")
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// The account `key` names, or why it names none.
async fn whoami(http: &reqwest::Client, key: &str) -> Result<Value, String> {
    let response = http
        .get(format!("{BASE_URL}{WHOAMI_PATH}"))
        .header("x-command-code-version", CLI_VERSION)
        .header("x-cli-environment", "production")
        .header("authorization", format!("Bearer {key}"))
        .send()
        .await
        .map_err(|error| format!("could not reach Command Code: {error}"))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("could not read Command Code's answer: {error}"))?;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(
            "Command Code turned the key away (HTTP 401). Try again, or paste an API key instead."
                .to_string(),
        );
    }
    if !status.is_success() {
        return Err(format!(
            "Command Code turned the sign-in away (HTTP {status}). Try again, or paste a key instead."
        ));
    }
    Ok(body)
}

fn whoami_name(who: &Value) -> String {
    who.get("user")
        .and_then(|user| user.get("userName").or_else(|| user.get("name")))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The subscription's model catalog entries, in the server's order.
async fn catalog(http: &reqwest::Client) -> Result<Vec<Value>, String> {
    let response = http
        .get(format!("{BASE_URL}{CATALOG_PATH}"))
        .send()
        .await
        .map_err(|error| format!("could not reach Command Code: {error}"))?;
    let response = response
        .error_for_status()
        .map_err(|error| format!("Command Code refused to list its models: {error}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("could not read Command Code's catalog: {error}"))?;
    let list: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Command Code's catalog is not valid JSON: {error}"))?;
    list.get("data")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "Command Code's catalog has no models.".to_string())
}

/// The catalog entries as models: every chat-capable entry answers on the API.
fn catalog_models(entries: &[Value]) -> Vec<ModelRoute> {
    entries
        .iter()
        .filter(|entry| accepts_text(entry))
        .filter_map(|entry| {
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())?;
            Some(ModelRoute {
                id: id.to_string(),
                display_name: catalog_display_name(entry, id),
                context_window: entry.get("context_length").and_then(Value::as_u64),
                max_output: None,
                efforts: catalog_efforts(entry),
                default_effort: None,
                price: None,
                source: ModelSource::Endpoint,
            })
        })
        .collect()
}

/// Keeps chat-capable models: an entry that lists modalities but omits `text`
/// (e.g. an image-only entry) is not usable for a coding conversation.
fn accepts_text(entry: &Value) -> bool {
    match entry.get("inputModalities").and_then(Value::as_array) {
        Some(modalities) if !modalities.is_empty() => modalities
            .iter()
            .any(|modality| modality.as_str() == Some("text")),
        _ => true,
    }
}

fn catalog_efforts(entry: &Value) -> Vec<String> {
    entry
        .get("reasoningEfforts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

fn catalog_display_name(entry: &Value, id: &str) -> String {
    entry
        .get("display")
        .and_then(|display| display.get("label"))
        .and_then(Value::as_str)
        .filter(|label| !label.is_empty())
        .unwrap_or(id)
        .to_string()
}

/// The sign-in page: the callback and state the studio posts the key back with.
fn auth_url(port: u16, state: &str) -> Result<reqwest::Url, String> {
    let callback = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    reqwest::Url::parse_with_params(
        &format!("{STUDIO}/studio/auth/cli"),
        [
            ("callback", callback.as_str()),
            ("state", state),
            ("mode", "redirect"),
        ],
    )
    .map_err(|error| format!("could not build the sign-in URL: {error}"))
}

/// Opens the system browser through the opener plugin, which knows the command
/// on every platform The Pair builds for: `open` and `xdg-open` are not on
/// Windows. The plugin's launcher is also detached, so waiting for the callback
/// never waits on the browser command as well.
fn open_browser(url: &str) -> Result<(), String> {
    if url.trim().is_empty() {
        return Err("there is no sign-in URL to open".to_string());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|error| error.to_string())
}

/// 32 random bytes as base64url, the same state shape the CLI uses. Two v4
/// UUIDs are concatenated: each is 16 bytes drawn from the OS CSPRNG.
fn random_state() -> String {
    let mut bytes = [0u8; 32];
    let (first, second) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    bytes[..16].copy_from_slice(first.as_bytes());
    bytes[16..].copy_from_slice(second.as_bytes());
    base64url_encode(&bytes)
}

/// Base64url without padding, using the URL-safe alphabet `A-Za-z0-9-_`.
fn base64url_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let group = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(group >> 18) as usize & 63] as char);
        out.push(ALPHABET[(group >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(group >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[group as usize & 63] as char);
        }
    }
    out
}

/// What the browser brings back from a sign-in that went through.
#[derive(Debug, PartialEq)]
struct Callback {
    api_key: String,
    user_name: String,
}

/// Answers the loopback server until a request carries this attempt's callback:
/// the key as JSON, or as a form that lands on the completion page.
async fn wait_for_callback(listener: &TcpListener, state: &str) -> Result<Callback, String> {
    let mut pending: Option<Callback> = None;
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| format!("the callback listener failed: {error}"))?;
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        while !has_head(&request) && request.len() < 16_384 {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await {
                Ok(Ok(read)) if read > 0 => request.extend_from_slice(&chunk[..read]),
                _ => break,
            }
        }
        let head_end = request
            .windows(4)
            .position(|end| end == b"\r\n\r\n")
            .map(|at| at + 4)
            .unwrap_or(request.len());
        let head = String::from_utf8_lossy(&request[..head_end]).into_owned();
        let mut body = request[head_end..].to_vec();
        let mut lines = head.lines();
        let mut request_line = lines.next().unwrap_or_default().split_whitespace();
        let (method, target) = (
            request_line.next().unwrap_or_default(),
            request_line.next().unwrap_or("/"),
        );
        let header = |name: &str| {
            lines.clone().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim().eq_ignore_ascii_case(name)).then(|| value.trim().to_string())
            })
        };
        let origin = header("origin").unwrap_or_default();
        let private = header("access-control-request-private-network").is_some_and(|v| v == "true");
        let over_cap = method == "POST"
            && header("content-length")
                .and_then(|len| len.parse::<usize>().ok())
                .is_some_and(|len| len > MAX_CALLBACK_BODY);
        while !over_cap
            && method == "POST"
            && body.len()
                < header("content-length")
                    .and_then(|len| len.parse::<usize>().ok())
                    .unwrap_or(0)
                    .min(MAX_CALLBACK_BODY + 1)
        {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await {
                Ok(Ok(read)) if read > 0 => body.extend_from_slice(&chunk[..read]),
                _ => break,
            }
        }
        let (reply, outcome) = if over_cap || body.len() > MAX_CALLBACK_BODY {
            (
                Reply::json(413, r#"{"success":false,"error":"Payload too large"}"#),
                None,
            )
        } else {
            route(
                method,
                target,
                &header("content-type").unwrap_or_default(),
                &body,
                state,
                &mut pending,
            )
        };
        let head = reply.head(&origin, private);
        let _ = stream.write_all(head.as_bytes()).await;
        let _ = stream.write_all(reply.body.as_bytes()).await;
        let _ = stream.shutdown().await;
        if let Some(outcome) = outcome {
            return outcome;
        }
    }
}

/// Whether the bytes hold a complete head: headers closed by a blank line.
fn has_head(request: &[u8]) -> bool {
    request.windows(4).any(|end| end == b"\r\n\r\n")
}

/// One callback answer: status, body, and where a form lands next.
struct Reply {
    status: u16,
    content_type: &'static str,
    body: String,
    location: Option<String>,
}

impl Reply {
    fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_string(),
            location: None,
        }
    }

    fn page(status: u16, body: String) -> Self {
        Self {
            status,
            content_type: "text/html; charset=utf-8",
            body,
            location: None,
        }
    }

    /// The head with the CORS headers the studio page needs to post here.
    fn head(&self, origin: &str, private: bool) -> String {
        let reason = match self.status {
            200 => "OK",
            204 => "No Content",
            303 => "See Other",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Content Too Large",
            415 => "Unsupported Media Type",
            _ => "Error",
        };
        let allowed = ["https://commandcode.ai", "https://staging.commandcode.ai"];
        let origin = if allowed.contains(&origin) {
            origin
        } else {
            "http://localhost:3000"
        };
        let location = self
            .location
            .as_deref()
            .map_or(String::new(), |to| format!("location: {to}\r\n"));
        let private = if private {
            "access-control-allow-private-network: true\r\n"
        } else {
            ""
        };
        format!(
            "HTTP/1.1 {} {reason}\r\n\
             access-control-allow-origin: {origin}\r\n\
             access-control-allow-methods: GET, POST, OPTIONS\r\n\
             access-control-allow-headers: Content-Type\r\n\
             {location}\
             {private}\
             content-type: {}\r\n\
             content-length: {}\r\n\
             connection: close\r\n\r\n",
            self.status,
            self.content_type,
            self.body.len(),
        )
    }
}

/// Routes one callback request. `Ok(None)` keeps listening; otherwise the
/// attempt ends.
fn route(
    method: &str,
    target: &str,
    content_type: &str,
    body: &[u8],
    state: &str,
    pending: &mut Option<Callback>,
) -> (Reply, Option<Result<Callback, String>>) {
    if method == "OPTIONS" {
        return (Reply::json(204, ""), None);
    }
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}"));
    let path = url.as_ref().map(|url| url.path()).unwrap_or_default();
    if method == "GET" && path == COMPLETE_PATH {
        let here = url
            .as_ref()
            .ok()
            .and_then(|url| pair(&url_query_pairs(url), "state"));
        if here.as_deref() != Some(state) {
            return (
                Reply::page(
                    403,
                    error_page("Invalid state token", "Restart login from your terminal."),
                ),
                None,
            );
        }
        return match pending.take() {
            Some(callback) => {
                let page = format!(
                    "<!doctype html><meta charset=utf-8><title>The Pair</title>\
                     <p>Signed in to Command Code as {}. You can close this tab.</p>",
                    escape(&callback.user_name)
                );
                (Reply::page(200, page), Some(Ok(callback)))
            }
            None => (
                Reply::page(
                    404,
                    error_page(
                        "Return to your terminal",
                        "Restart login from your terminal if you reached it directly.",
                    ),
                ),
                None,
            ),
        };
    }
    if path != CALLBACK_PATH {
        return (
            Reply::json(404, r#"{"success":false,"error":"Not found"}"#),
            None,
        );
    }
    if method == "GET" {
        return (
            Reply::page(
                405,
                error_page(
                    "Return to your terminal",
                    "This page completes login automatically during sign-in.",
                ),
            ),
            None,
        );
    }
    if method != "POST" {
        return (
            Reply::json(
                405,
                r#"{"success":false,"error":"Method not allowed. Use POST."}"#,
            ),
            None,
        );
    }
    let kind = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if kind == "application/json" {
        return json_callback(body, state);
    }
    if kind == "application/x-www-form-urlencoded" {
        return form_callback(&String::from_utf8_lossy(body), state, pending);
    }
    (
        Reply::json(
            415,
            r#"{"success":false,"error":"Unsupported content type"}"#,
        ),
        None,
    )
}

/// A JSON post: the key, or the denial.
fn json_callback(body: &[u8], state: &str) -> (Reply, Option<Result<Callback, String>>) {
    let posted: Value = serde_json::from_slice(body).unwrap_or_default();
    if posted.get("error").is_some() {
        return denied(
            posted["state"].as_str().unwrap_or_default(),
            posted["error"].as_str().unwrap_or_default(),
            posted["error_description"].as_str().unwrap_or_default(),
            state,
        );
    }
    let fields = ["apiKey", "state", "userId", "userName", "keyName"]
        .into_iter()
        .map(|name| posted[name].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    let [api_key, here, _, user_name, _] = fields[..] else {
        return (
            Reply::json(
                400,
                r#"{"success":false,"error":"Missing required fields"}"#,
            ),
            None,
        );
    };
    if api_key.is_empty() {
        return (
            Reply::json(
                400,
                r#"{"success":false,"error":"Missing required fields"}"#,
            ),
            None,
        );
    }
    if here != state {
        return (
            Reply::json(403, r#"{"success":false,"error":"Invalid state token"}"#),
            None,
        );
    }
    (
        Reply::json(200, r#"{"success":true}"#),
        Some(Ok(Callback {
            api_key: api_key.to_string(),
            user_name: user_name.to_string(),
        })),
    )
}

/// A form post: the key lands on the completion page, which ends the attempt.
fn form_callback(
    body: &str,
    state: &str,
    pending: &mut Option<Callback>,
) -> (Reply, Option<Result<Callback, String>>) {
    let pairs = url_query_pairs_raw(body);
    if let Some(error) = pair(&pairs, "error") {
        let (reply, outcome) = denied(
            pair(&pairs, "state").as_deref().unwrap_or_default(),
            &error,
            pair(&pairs, "error_description")
                .as_deref()
                .unwrap_or_default(),
            state,
        );
        let reply = match outcome {
            Some(Err(_)) => Reply::page(
                200,
                error_page(
                    "Authorization denied",
                    "Return to your terminal and run login again, or paste a key.",
                ),
            ),
            _ => Reply::page(
                reply.status,
                error_page("Invalid state token", "Restart login from your terminal."),
            ),
        };
        return (reply, outcome);
    }
    let field = |name: &str| pair(&pairs, name).unwrap_or_default();
    let (api_key, here, user_name) = (field("apiKey"), field("state"), field("userName"));
    if api_key.is_empty()
        || field("userId").is_empty()
        || user_name.is_empty()
        || field("keyName").is_empty()
    {
        return (
            Reply::page(
                400,
                error_page(
                    "Invalid request",
                    "Required parameters are missing. Restart login, or paste a key.",
                ),
            ),
            None,
        );
    }
    if here != state {
        return (
            Reply::page(
                403,
                error_page("Invalid state token", "Restart login from your terminal."),
            ),
            None,
        );
    }
    *pending = Some(Callback { api_key, user_name });
    (
        Reply {
            status: 303,
            content_type: "text/html; charset=utf-8",
            body: String::new(),
            location: Some(format!("{COMPLETE_PATH}?state={state}")),
        },
        None,
    )
}

/// A denial post: 403 on a foreign state, else the attempt ends with its reason.
fn denied(
    here: &str,
    error: &str,
    description: &str,
    state: &str,
) -> (Reply, Option<Result<Callback, String>>) {
    if here != state {
        return (
            Reply::json(403, r#"{"success":false,"error":"Invalid state token"}"#),
            None,
        );
    }
    let message = match error {
        "access_denied" => "Command Code authorization was denied".to_string(),
        _ => format!("{error} {description}").trim().to_string(),
    };
    (Reply::json(200, r#"{"success":true}"#), Some(Err(message)))
}

/// A query string's decoded pairs: the form posts are encoded the same way.
fn url_query_pairs_raw(query: &str) -> Vec<(String, String)> {
    reqwest::Url::parse(&format!("http://127.0.0.1/x?{query}"))
        .map(|url| {
            url.query_pairs()
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn url_query_pairs(url: &reqwest::Url) -> Vec<(String, String)> {
    url.query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

fn pair(pairs: &[(String, String)], name: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
}

fn error_page(heading: &str, message: &str) -> String {
    format!(
        "<!doctype html><meta charset=utf-8><title>The Pair</title><h1>{}</h1><p>{}</p>",
        escape(heading),
        escape(message)
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn routed(
        method: &str,
        target: &str,
        content_type: &str,
        body: &str,
        pending: &mut Option<Callback>,
    ) -> (Reply, Option<Result<Callback, String>>) {
        route(
            method,
            target,
            content_type,
            body.as_bytes(),
            "state-1",
            pending,
        )
    }

    #[test]
    fn a_json_post_with_the_key_ends_the_attempt() {
        let mut pending = None;
        let (reply, outcome) = routed(
            "POST",
            "/callback",
            "application/json",
            r#"{"apiKey":"cc-key","state":"state-1","userId":"u","userName":"tim","keyName":"cli"}"#,
            &mut pending,
        );
        assert_eq!(
            (reply.status, reply.body.as_str()),
            (200, r#"{"success":true}"#)
        );
        assert_eq!(
            outcome.unwrap().unwrap(),
            Callback {
                api_key: "cc-key".to_string(),
                user_name: "tim".to_string(),
            }
        );
    }

    #[test]
    fn a_form_post_lands_on_the_completion_page_which_ends_it() {
        let mut pending = None;
        let (reply, outcome) = routed(
            "POST",
            "/callback",
            "application/x-www-form-urlencoded",
            "apiKey=cc-key&state=state-1&userId=u&userName=tim&keyName=cli",
            &mut pending,
        );
        assert_eq!(reply.status, 303);
        assert_eq!(
            reply.location.as_deref(),
            Some("/callback/complete?state=state-1")
        );
        assert!(outcome.is_none() && pending.is_some());

        let (reply, outcome) = routed(
            "GET",
            "/callback/complete?state=state-1",
            "",
            "",
            &mut pending,
        );
        assert_eq!(reply.status, 200);
        assert!(reply.body.contains("tim"), "{}", reply.body);
        assert_eq!(outcome.unwrap().unwrap().api_key, "cc-key");
    }

    #[test]
    fn a_foreign_state_ends_nothing() {
        let mut pending = None;
        let (reply, outcome) = routed(
            "POST",
            "/callback",
            "application/json",
            r#"{"apiKey":"cc-key","state":"older","userId":"u","userName":"tim","keyName":"cli"}"#,
            &mut pending,
        );
        assert_eq!(reply.status, 403);
        assert!(outcome.is_none());

        let (reply, outcome) = routed(
            "GET",
            "/callback/complete?state=older",
            "",
            "",
            &mut pending,
        );
        assert_eq!(reply.status, 403);
        assert!(outcome.is_none());
    }

    #[test]
    fn a_denial_ends_the_attempt_with_its_reason() {
        let mut pending = None;
        let (reply, outcome) = routed(
            "POST",
            "/callback",
            "application/json",
            r#"{"error":"access_denied","error_description":"","state":"state-1"}"#,
            &mut pending,
        );
        assert_eq!(reply.status, 200);
        assert_eq!(
            outcome.unwrap().unwrap_err(),
            "Command Code authorization was denied"
        );
    }

    #[test]
    fn malformed_and_unsupported_requests_are_answered() {
        let mut pending = None;
        for (method, target, content_type, status) in [
            ("OPTIONS", "/callback", "", 204),
            ("GET", "/favicon.ico", "", 404),
            ("GET", "/callback", "", 405),
            ("POST", "/callback", "text/plain", 415),
            ("POST", "/callback", "application/json", 400),
            (
                "POST",
                "/callback",
                "application/x-www-form-urlencoded",
                400,
            ),
        ] {
            let (reply, outcome) = routed(method, target, content_type, "{}", &mut pending);
            assert_eq!(reply.status, status, "{method} {target}");
            assert!(outcome.is_none());
        }
        assert!(pending.is_none());
    }

    #[test]
    fn every_reply_carries_cors_and_a_length() {
        let head = Reply::json(200, r#"{"success":true}"#).head("https://commandcode.ai", true);
        assert!(
            head.contains("access-control-allow-origin: https://commandcode.ai"),
            "{head}"
        );
        assert!(
            head.contains("access-control-allow-private-network: true"),
            "{head}"
        );
        assert!(head.contains("content-length: 16"), "{head}");
        let head = Reply::json(200, "{}").head("https://rogue.example", false);
        assert!(
            head.contains("access-control-allow-origin: http://localhost:3000"),
            "{head}"
        );
    }

    #[test]
    fn the_auth_url_names_the_callback_state_and_mode() {
        let url = auth_url(43210, "state-1").unwrap();
        assert_eq!(
            url.as_str(),
            "https://commandcode.ai/studio/auth/cli?callback=http%3A%2F%2F127.0.0.1%3A43210%2Fcallback&state=state-1&mode=redirect"
        );
    }

    #[test]
    fn state_is_43_url_safe_chars_and_unique() {
        let first = random_state();
        let second = random_state();
        assert_eq!(first.len(), 43);
        assert_eq!(second.len(), 43);
        assert_ne!(first, second);
        assert!(first
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'));
    }

    #[test]
    fn base64url_encode_matches_the_standard() {
        assert_eq!(base64url_encode(b"hello"), "aGVsbG8");
        assert_eq!(base64url_encode(&[0xff, 0xff, 0xff]), "____");
        assert_eq!(base64url_encode(&[0xff]), "_w");
        assert_eq!(base64url_encode(&[0xff, 0xff]), "__8");
        assert_eq!(base64url_encode(b""), "");
    }

    #[test]
    fn the_catalog_becomes_models() {
        let entries = vec![
            json!({"id": "claude-sonnet-5-5", "context_length": 1000000,
                "display": {"label": "Claude Sonnet 5.5"},
                "inputModalities": ["text", "image"],
                "reasoningEfforts": ["low", "medium", "high", "xhigh", "max"]}),
            json!({"id": "muse-spark", "context_length": 1000000,
                "inputModalities": ["text"], "reasoningEfforts": []}),
            json!({"id": "image-only", "inputModalities": ["image"]}),
            json!({"context_length": 1}),
        ];
        let models = catalog_models(&entries);
        assert_eq!(models.len(), 2, "the image-only and id-less entries drop");
        assert_eq!(models[0].id, "claude-sonnet-5-5");
        assert_eq!(models[0].display_name, "Claude Sonnet 5.5");
        assert_eq!(models[0].context_window, Some(1_000_000));
        assert_eq!(models[0].efforts, ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(models[0].source, ModelSource::Endpoint);
        assert_eq!(models[1].id, "muse-spark");
        assert_eq!(models[1].display_name, "muse-spark", "falls back to the id");
        assert!(models[1].efforts.is_empty());
    }

    #[test]
    fn whoami_name_prefers_user_name_then_name() {
        assert_eq!(
            whoami_name(&json!({"user": {"userName": "tim", "name": "Tim"}})),
            "tim"
        );
        assert_eq!(whoami_name(&json!({"user": {"name": "Tim"}})), "Tim");
        assert_eq!(whoami_name(&json!({})), "");
    }

    #[tokio::test]
    async fn the_json_callback_round_trip_over_a_socket() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { wait_for_callback(&listener, "state-1").await });

        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let body = r#"{"apiKey":"cc-key","state":"state-1","userId":"u","userName":"tim","keyName":"cli"}"#;
        stream
            .write_all(
                format!(
                    "POST /callback HTTP/1.1\r\nhost: 127.0.0.1\r\n\
                     content-type: application/json\r\ncontent-length: {}\r\n\
                     connection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
            .await
            .expect("the server should answer")
            .unwrap();
        let text = String::from_utf8_lossy(&response);
        assert!(text.contains("200"), "{text}");
        assert!(text.contains(r#"{"success":true}"#), "{text}");

        let callback = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("the server should finish")
            .unwrap()
            .unwrap();
        assert_eq!(callback.api_key, "cc-key");
        assert_eq!(callback.user_name, "tim");
    }

    #[tokio::test]
    async fn the_form_callback_round_trip_over_a_socket() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { wait_for_callback(&listener, "state-1").await });

        let mut post = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let body = "apiKey=cc-key&state=state-1&userId=u&userName=tim&keyName=cli";
        post.write_all(
            format!(
                "POST /callback HTTP/1.1\r\nhost: 127.0.0.1\r\n\
                 content-type: application/x-www-form-urlencoded\r\ncontent-length: {}\r\n\
                 connection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut post_response = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), post.read_to_end(&mut post_response))
            .await
            .expect("the server should answer the form")
            .unwrap();
        let post_text = String::from_utf8_lossy(&post_response);
        assert!(post_text.contains("303"), "{post_text}");
        assert!(
            post_text.contains("/callback/complete?state=state-1"),
            "{post_text}"
        );

        let mut get = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        get.write_all(
            b"GET /callback/complete?state=state-1 HTTP/1.1\r\n\
              host: 127.0.0.1\r\nconnection: close\r\n\r\n",
        )
        .await
        .unwrap();
        let mut get_response = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), get.read_to_end(&mut get_response))
            .await
            .expect("the server should answer the redirect")
            .unwrap();
        let get_text = String::from_utf8_lossy(&get_response);
        assert!(get_text.contains("200"), "{get_text}");
        assert!(get_text.contains("tim"), "{get_text}");

        let callback = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("the server should finish")
            .unwrap()
            .unwrap();
        assert_eq!(callback.api_key, "cc-key");
    }

    #[tokio::test]
    async fn login_with_key_rejects_an_empty_key() {
        let root =
            std::env::temp_dir().join(format!("the-pair-commandcode-{}", uuid::Uuid::new_v4()));
        let outcome = login_with_key(&root, "   ").await;
        assert!(outcome.is_err());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_blank_url_is_never_handed_to_the_opener() {
        // The opener shells out to the platform's browser command; a blank URL
        // would only raise a confusing error window.
        assert!(open_browser("").is_err());
        assert!(open_browser("   ").is_err());
    }
}
