//! Running one turn: resolve the selection, stream the reply, run the tools the
//! model asks for, and fall back when the first model cannot finish.

use super::history;
use super::ModelSelection;
use crate::accounts::store::load as load_providers;
use crate::accounts::types::{
    Account, AccountKind, AccountStatus, ModelRoute, OAuthToken, Provider, Secret,
};
use crate::llm::{CancelResult, Client, ClientConfig, Delta, ProviderError, Usage};
use crate::tools::{self, ToolPolicy};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// How long one model call may take in all, stream included. A reasoning model
/// on a long context can sit well past a minute before its first token, so this
/// is generous; the turn's own idle timeout is the real guard.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Tool calls one turn may make. A real task runs tens; this only catches a
/// model that keeps calling tools without ever answering.
const MAX_TOOL_CALLS_PER_TURN: usize = 200;

/// What one turn is asked to do.
pub struct TurnRequest {
    pub selection: ModelSelection,
    /// The pair's own reasoning effort for this role. It wins over the route's
    /// default, which wins over nothing at all: a model that takes an effort
    /// runs at the one the pair was given.
    pub effort: Option<String>,
    /// The agent's system prompt (mentor planning, executor implementing).
    pub system: String,
    /// Prior turns in OpenAI chat form, oldest first, ending with the user turn
    /// this one answers.
    pub history: Vec<Value>,
    /// Keys the provider's prompt cache, so a session's requests share one.
    pub cache_key: String,
    /// Set by a pause, kill or a superseded run: the stream stops where it is.
    pub cancel: Arc<AtomicBool>,
    /// The pair's workspace. Tools read and write here, and nowhere else.
    pub directory: String,
    /// What this agent may do in the workspace.
    pub policy: ToolPolicy,
}

/// One tool call the model made, as the console shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub ok: bool,
    /// The tool's answer, truncated for display.
    pub output: String,
}

/// What came back, and from which selection it came.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnOutcome {
    pub text: String,
    pub reasoning: String,
    pub usage: Usage,
    /// The provider's own session id, where the protocol has one.
    pub session_id: Option<String>,
    /// The selection that actually answered — the backup when it stepped in.
    pub answered_by: ModelSelection,
    /// Set when the primary failed and the backup took over.
    pub fell_back: bool,
    /// Every tool call the turn made, in order.
    pub tool_calls: Vec<ToolCallRecord>,
    /// The task list the agent left behind.
    pub todos: Vec<tools::todo::Todo>,
    /// The conversation after this turn: what was replayed, plus the user turn
    /// and everything the agent did about it. Store it and replay it next time.
    pub history: Vec<Value>,
}

/// What a tool did, as it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolEvent<'a> {
    Started {
        id: &'a str,
        name: &'a str,
        summary: &'a str,
    },
    Finished {
        id: &'a str,
        name: &'a str,
        ok: bool,
        output: &'a str,
    },
}

/// A turn that could not finish, and the selection it was running when it gave
/// up.
///
/// The caller marks that selection's account expired when the provider refused
/// the credential, so a backup that was the one refused is the one that gets
/// marked — and a primary that failed first is not blamed for it.
#[derive(Debug)]
pub struct TurnFailure {
    pub error: ProviderError,
    pub selection: ModelSelection,
    /// The conversation up to where the turn stopped, when a tool had already
    /// run: its calls and results are the only record of what the turn changed
    /// in the workspace, so the caller stores it and the retry replays it. Empty
    /// when nothing ran, because then there is nothing a retry could repeat.
    pub history: Vec<Value>,
}

/// One attempt that could not finish, with what it had done by then.
struct Stopped {
    error: ProviderError,
    /// See [`TurnFailure::history`].
    history: Vec<Value>,
}

/// How often a turn waiting on a model call or a tool looks at its cancel flag.
const CANCEL_POLL: Duration = Duration::from_millis(100);

/// Resolves once `cancel` is raised.
///
/// A stream only reports back between chunks and a tool only when it returns, so
/// a turn waiting for its first token, sleeping out a retry or inside a
/// five-minute `bash` call would not see a pause until then. Racing the wait
/// against this stops it where it is.
async fn raised(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

/// Runs one turn against `selection`, then against its backup when the first
/// cannot finish.
///
/// `on_delta` sees every streamed chunk and `on_tool` every tool call. Both must
/// be `Send`: the turn runs on its own task, so a callback holding a `!Send`
/// type would keep the whole future off the runtime's worker threads.
///
/// The failure names the selection it came from, which is what makes it wide; a
/// turn that could not finish is not on a hot path.
#[allow(clippy::result_large_err)]
pub async fn run_turn(
    app_dir: &Path,
    request: &TurnRequest,
    on_delta: &mut (dyn FnMut(Delta<'_>) -> CancelResult + Send),
    on_tool: &mut (dyn FnMut(ToolEvent<'_>) -> CancelResult + Send),
) -> Result<TurnOutcome, TurnFailure> {
    let providers = providers_for(app_dir, &request.selection).await;
    let worked = AtomicBool::new(false);
    let failed = |stopped: Stopped, selection: &ModelSelection| TurnFailure {
        error: stopped.error,
        selection: selection.clone(),
        history: stopped.history,
    };
    match attempt(
        &providers,
        request,
        &request.selection,
        &worked,
        on_delta,
        on_tool,
    )
    .await
    {
        Ok(outcome) => Ok(outcome),
        Err(primary) => {
            // A tool has already run, so the primary's work is real: the backup
            // cannot redo it, and answering from a history without the primary's
            // exchange would hide what already changed in the workspace.
            if worked.load(Ordering::SeqCst) {
                return Err(failed(primary, &request.selection));
            }
            let Some(backup) = request.selection.backup.as_deref() else {
                return Err(failed(primary, &request.selection));
            };
            // The backup only ever sees a cancellation: a transient failure was
            // already retried inside `attempt`.
            if request.cancel.load(Ordering::SeqCst) {
                return Err(TurnFailure {
                    error: ProviderError::cancelled(primary.error.attempts),
                    selection: request.selection.clone(),
                    history: Vec::new(),
                });
            }
            let providers = providers_for(app_dir, backup).await;
            match attempt(&providers, request, backup, &worked, on_delta, on_tool).await {
                Ok(mut outcome) => {
                    outcome.fell_back = true;
                    Ok(outcome)
                }
                Err(stopped) => Err(TurnFailure {
                    error: ProviderError {
                        // The backup's refusal is the one that ended the turn, so
                        // its kind and status are what the caller acts on; the
                        // primary's message still says what failed first.
                        status: stopped.error.status,
                        kind: stopped.error.kind,
                        message: format!(
                            "{} (the backup model {} also failed: {})",
                            primary.error.message, backup.model, stopped.error.message
                        ),
                        attempts: primary.error.attempts + stopped.error.attempts,
                        retryable: stopped.error.retryable,
                        advice: stopped.error.advice,
                    },
                    selection: backup.clone(),
                    history: stopped.history,
                }),
            }
        }
    }
}

/// The registry as a turn on `selection` should see it: read after a sign-in
/// that is about to expire has been renewed.
async fn providers_for(app_dir: &Path, selection: &ModelSelection) -> Vec<Provider> {
    renew_expiring_sign_in(app_dir, selection).await;
    load_providers(app_dir)
}

/// Renews a ChatGPT sign-in that is expired or about to be, before a turn uses
/// it.
///
/// The access token is short-lived and the sign-in keeps a refresh token for
/// exactly this, so a pair that runs past the expiry must not stop and send a
/// human to the Sign-in button. A renewal that fails changes nothing here: the
/// turn runs on the token it has and reports the sign-in as expired only when
/// it really is.
async fn renew_expiring_sign_in(app_dir: &Path, selection: &ModelSelection) {
    let renewable = load_providers(app_dir)
        .iter()
        .find(|provider| provider.id == selection.provider_id)
        .and_then(|provider| {
            provider
                .accounts
                .iter()
                .find(|account| account.id == selection.account_id)
        })
        .is_some_and(|account| {
            account.kind == AccountKind::ChatGptOAuth
                && matches!(&account.secret, Secret::OAuth(token)
                if token.refresh_token.as_deref().is_some_and(|refresh| !refresh.is_empty())
                    && token.expires_within(
                        crate::util::now_millis(),
                        OAuthToken::REFRESH_MARGIN_MS,
                    ))
        });
    if !renewable {
        return;
    }
    match crate::login::chatgpt::renew(app_dir).await {
        Ok(token) => store_renewed_token(app_dir, selection, token),
        Err(error) => eprintln!(
            "[harness] Could not renew the ChatGPT sign-in before the turn: {}",
            error
        ),
    }
}

/// Writes a renewed token onto the account the turn is about to use. The
/// account's status is re-evaluated with it, so one a failed turn had marked
/// expired is offered again.
fn store_renewed_token(app_dir: &Path, selection: &ModelSelection, token: OAuthToken) {
    use crate::accounts::store::{global_lock, save as save_providers};
    let _guard = global_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut providers = load_providers(app_dir);
    let replaced = crate::accounts::registry::replace_secret(
        &mut providers,
        &selection.provider_id,
        &selection.account_id,
        Secret::OAuth(Box::new(token)),
    );
    if replaced {
        if let Err(error) = save_providers(app_dir, &providers) {
            eprintln!(
                "[harness] Could not store the renewed ChatGPT sign-in: {}",
                error
            );
        }
    }
}

async fn attempt(
    providers: &[Provider],
    request: &TurnRequest,
    selection: &ModelSelection,
    // Set once this attempt has run a tool: whatever happens next, the turn's
    // work is done and no other model may be asked to carry on from it.
    worked: &AtomicBool,
    on_delta: &mut (dyn FnMut(Delta<'_>) -> CancelResult + Send),
    on_tool: &mut (dyn FnMut(ToolEvent<'_>) -> CancelResult + Send),
) -> Result<TurnOutcome, Stopped> {
    // Nothing has run yet, so there is no conversation to hand back.
    let before_work = |error: ProviderError| Stopped {
        error,
        history: Vec::new(),
    };
    let resolved = resolve(providers, selection, request.effort.as_deref()).map_err(before_work)?;
    // A client that cannot be built — no request timeout, no TLS backend — will
    // never answer, and no retry heals it.
    let client = Client::new(resolved.config)
        .map_err(ProviderError::unusable)
        .map_err(before_work)?;

    let mut messages = Vec::with_capacity(request.history.len() + 1);
    let carries_system = !request.system.trim().is_empty();
    let replayed = history::bounded(&request.history);
    let mut system = request.system.clone();
    if let Some(notice) = replayed.notice {
        system = match system.trim().is_empty() {
            true => notice,
            false => format!("{}\n\n{}", system.trim_end(), notice),
        };
    }
    if carries_system {
        messages.push(json!({"role": "system", "content": system}));
    }
    messages.extend(replayed.messages.iter().cloned());

    let exchange = converse(
        &client,
        request,
        selection,
        carries_system,
        &mut messages,
        worked,
        on_delta,
        on_tool,
    )
    .await;
    exchange.map_err(|error| {
        // The tools this attempt ran are in `messages` and nowhere else. A call
        // the turn stopped before answering gets a result saying so: a history
        // that ends on an unanswered call is refused by every provider.
        let history = match worked.load(Ordering::SeqCst) {
            true => {
                history::close_open_calls(&mut messages);
                carried_history(&messages, carries_system)
            }
            false => Vec::new(),
        };
        Stopped { error, history }
    })
}

/// The model/tool loop of one attempt: ask, run what it asks for, ask again,
/// until it answers without calling anything. `messages` is the conversation it
/// extends, which the caller keeps when the loop fails part-way.
#[allow(clippy::too_many_arguments)]
async fn converse(
    client: &Client,
    request: &TurnRequest,
    selection: &ModelSelection,
    carries_system: bool,
    messages: &mut Vec<Value>,
    worked: &AtomicBool,
    on_delta: &mut (dyn FnMut(Delta<'_>) -> CancelResult + Send),
    on_tool: &mut (dyn FnMut(ToolEvent<'_>) -> CancelResult + Send),
) -> Result<TurnOutcome, ProviderError> {
    let tool_schemas = tools::schemas(request.policy);
    let workspace = std::path::Path::new(&request.directory);
    let tool_context = tools::ToolContext {
        directory: workspace,
        policy: request.policy,
    };
    tools::todo::clear();

    let cancel = request.cancel.as_ref();
    let mut reasoning = String::new();
    // The last thing the model said alongside its tool calls. A model that
    // reports next to its final call and then closes with an empty message has
    // still answered, and this is the answer.
    let mut said = String::new();
    let mut records: Vec<ToolCallRecord> = Vec::new();
    let mut calls_made = 0usize;
    let mut session_id = None;
    // Every request of the loop is billed, not only the one that ended it: a
    // turn that ran ten tool rounds spent ten requests' worth of tokens.
    let mut usage = crate::llm::Usage::default();

    loop {
        // Where this request's reasoning starts: a discarded attempt takes back
        // what it streamed, never what earlier requests of the turn said.
        let request_start = reasoning.len();
        // Records the turn's own output, then forwards the chunk: the caller's
        // stream is what the console shows, so it must see every delta.
        let mut observed = |delta: Delta<'_>| -> CancelResult {
            if cancel.load(Ordering::SeqCst) {
                return CancelResult::Cancel;
            }
            match delta {
                Delta::Reasoning(chunk) => reasoning.push_str(chunk),
                // The reply's text arrives whole on the reply itself.
                Delta::Text(_) | Delta::Tool { .. } | Delta::Retry { .. } => {}
                // The attempt that streamed this is being thrown away, so what
                // it delivered is not part of the reply.
                Delta::Discard => reasoning.truncate(request_start),
            }
            on_delta(delta)
        };

        let reply = tokio::select! {
            biased;
            reply = client.complete(messages, &tool_schemas, &request.cache_key, &mut observed) => reply?,
            _ = raised(cancel) => return Err(ProviderError::cancelled(1)),
        };

        usage.prompt_tokens += reply.usage.prompt_tokens;
        usage.completion_tokens += reply.usage.completion_tokens;
        usage.cached_tokens += reply.usage.cached_tokens;
        usage.reasoning_tokens += reply.usage.reasoning_tokens;
        if session_id.is_none() {
            session_id = reply.session_id.clone();
        }
        // A reply with neither text nor calls is not a turn a provider accepts
        // back, so it is not stored.
        messages.extend(history::assistant(&reply));

        if reply.tool_calls.is_empty() {
            let text = match reply.text.trim().is_empty() {
                true => said,
                false => reply.text,
            };
            return Ok(TurnOutcome {
                text,
                reasoning,
                usage,
                session_id,
                answered_by: selection.clone(),
                fell_back: false,
                tool_calls: records,
                todos: tools::todo::snapshot(),
                history: carried_history(messages, carries_system),
            });
        }
        if !reply.text.trim().is_empty() {
            said = reply.text;
        }

        calls_made += reply.tool_calls.len();
        if calls_made > MAX_TOOL_CALLS_PER_TURN {
            return Err(ProviderError::bad_response(format!(
                "the model asked for more than {} tool calls in one turn; the turn was stopped",
                MAX_TOOL_CALLS_PER_TURN
            )));
        }

        for call in reply.tool_calls {
            let summary = tools::summary(&call.name, &call.arguments);
            if matches!(
                on_tool(ToolEvent::Started {
                    id: &call.id,
                    name: &call.name,
                    summary: &summary,
                }),
                CancelResult::Cancel
            ) {
                return Err(ProviderError::cancelled(1));
            }

            // The call is about to run, whatever it answers and whether or not
            // it is stopped half-way: this attempt has now touched the
            // workspace, so no other model may be asked to carry on.
            worked.store(true, Ordering::SeqCst);
            // Dropping the call is what stops it: `bash` ends its command's
            // whole process tree when its future goes.
            let result = tokio::select! {
                biased;
                result = tools::run(&tool_context, &call.name, &call.arguments) => result,
                _ = raised(cancel) => return Err(ProviderError::cancelled(1)),
            };
            let (ok, output) = match &result {
                Ok(output) => (true, output.clone()),
                Err(error) => (false, error.clone()),
            };
            records.push(ToolCallRecord {
                id: call.id.clone(),
                name: call.name.clone(),
                summary,
                ok,
                output: display_output(&output),
            });
            messages.push(history::tool_result(&call.id, &output));
            if matches!(
                on_tool(ToolEvent::Finished {
                    id: &call.id,
                    name: &call.name,
                    ok,
                    output: &output,
                }),
                CancelResult::Cancel
            ) {
                return Err(ProviderError::cancelled(1));
            }
        }
    }
}

/// The conversation a turn leaves behind: everything it replayed, plus its own
/// exchange, capped the way the replay is — and without the system prompt, which
/// only ever rides on the request.
fn carried_history(messages: &[Value], carries_system: bool) -> Vec<Value> {
    let exchange = match carries_system {
        true => &messages[1..],
        false => messages,
    };
    history::stored(exchange)
}

/// A tool's answer as the console shows it: the first lines, cut on a character
/// boundary, with a marker when more was said.
fn display_output(output: &str) -> String {
    const LINES: usize = 12;
    const CHARS: usize = 600;
    let mut text: String = output.lines().take(LINES).collect::<Vec<_>>().join("\n");
    let remaining = output.lines().count().saturating_sub(LINES);
    if remaining > 0 {
        text.push_str(&format!("\n\n… {} more lines", remaining));
    }
    if text.chars().count() > CHARS {
        let cut: String = text.chars().take(CHARS).collect();
        format!("{}…", cut)
    } else {
        text
    }
}

/// A selection plus everything the transport needs, resolved from the store.
#[derive(Debug)]
struct Resolved {
    config: ClientConfig,
}

fn resolve(
    providers: &[Provider],
    selection: &ModelSelection,
    // The pair's own effort for this role, which wins over the route's.
    effort: Option<&str>,
) -> Result<Resolved, ProviderError> {
    if !selection.is_usable() {
        return Err(ProviderError::bad_response(
            "this agent has no model selected".to_string(),
        ));
    }
    let provider = providers
        .iter()
        .find(|provider| provider.id == selection.provider_id)
        .ok_or_else(|| {
            ProviderError::bad_response(format!(
                "the model endpoint `{}` is no longer in the registry; pick a model again",
                selection.provider_id
            ))
        })?;
    let account = provider
        .accounts
        .iter()
        .find(|account| account.id == selection.account_id)
        .ok_or_else(|| {
            ProviderError::bad_response(format!(
                "the account for {} is no longer in the registry; pick a model again",
                provider.display_name
            ))
        })?;

    let api_key = api_key_of(account)?;
    let model = route_of(provider, selection);

    Ok(Resolved {
        config: ClientConfig {
            protocol: provider.protocol,
            base_url: provider.base_url.clone(),
            api_key,
            headers: account.headers.clone(),
            tool_names: account.tool_names.clone(),
            model: selection.model.clone(),
            max_output_tokens: model.and_then(|route| route.max_output),
            reasoning_effort: effort
                .or(selection.effort.as_deref())
                .or(model.and_then(|route| route.default_effort.as_deref()))
                .map(str::to_string),
            timeout: REQUEST_TIMEOUT,
        },
    })
}

/// The bearer value for an account, or why it cannot be used right now.
fn api_key_of(account: &Account) -> Result<String, ProviderError> {
    match &account.secret {
        Secret::ApiKey(key) if !key.trim().is_empty() => Ok(key.trim().to_string()),
        Secret::Env(name) => std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::auth(format!(
                    "{} is not set in this shell, so {} cannot be used",
                    name, account.label
                ))
            }),
        Secret::OAuth(token) => {
            if token.is_expired(crate::util::now_millis()) {
                return Err(ProviderError::auth(format!(
                    "the {} sign-in expired; sign in again from the Models section",
                    account.label
                )));
            }
            Ok(token.access_token.clone())
        }
        _ => Err(ProviderError::auth(format!(
            "{} has no credential; add one from the Models section",
            account.label
        ))),
    }
}

/// The route for the selected model, when the registry lists it.
fn route_of<'a>(provider: &'a Provider, selection: &ModelSelection) -> Option<&'a ModelRoute> {
    provider
        .models
        .iter()
        .find(|route| route.id.eq_ignore_ascii_case(&selection.model))
}

/// Marks an account expired so the Models section shows "sign in again".
pub fn mark_account_expired(app_dir: &Path, provider_id: &str, account_id: &str) -> bool {
    use crate::accounts::store::{global_lock, save as save_providers};
    let _guard = global_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut providers = load_providers(app_dir);
    let mut changed = false;
    for provider in providers.iter_mut() {
        if provider.id != provider_id {
            continue;
        }
        for account in provider.accounts.iter_mut() {
            if account.id == account_id && account.status != AccountStatus::AuthExpired {
                account.status = AccountStatus::AuthExpired;
                changed = true;
            }
        }
    }
    if changed {
        let _ = save_providers(app_dir, &providers);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::store::save as save_providers;
    use crate::accounts::types::{
        AccountKind, HeaderPair, ModelPrice, ModelSource, OAuthToken, Protocol,
    };
    use crate::llm::ErrorKind;

    fn app_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("the-pair-harness-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn provider() -> Provider {
        Provider {
            id: "anthropic-1".into(),
            protocol: Protocol::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            vendor: "anthropic".into(),
            display_name: "Anthropic".into(),
            accounts: vec![Account {
                id: "anthropic-1-a".into(),
                kind: AccountKind::ApiKey,
                label: "claude".into(),
                source: "claude".into(),
                status: AccountStatus::Ready,
                headers: vec![HeaderPair {
                    name: "x-test".into(),
                    value: "1".into(),
                }],
                tool_names: None,
                secret: Secret::ApiKey("sk-ant-test".into()),
            }],
            models: vec![ModelRoute {
                id: "claude-sonnet-4-5".into(),
                display_name: "Claude Sonnet 4.5".into(),
                context_window: Some(200_000),
                max_output: Some(64_000),
                efforts: vec![],
                default_effort: Some("medium".into()),
                price: Some(ModelPrice {
                    input_per_million: 3.0,
                    output_per_million: 15.0,
                }),
                source: ModelSource::Preset,
            }],
        }
    }

    fn selection() -> ModelSelection {
        ModelSelection {
            provider_id: "anthropic-1".into(),
            account_id: "anthropic-1-a".into(),
            model: "claude-sonnet-4-5".into(),
            effort: Some("high".into()),
            backup: None,
        }
    }

    #[test]
    fn resolves_the_route_into_a_client_config() {
        let providers = vec![provider()];
        let resolved = resolve(&providers, &selection(), None).unwrap();
        assert_eq!(resolved.config.protocol, Protocol::Anthropic);
        assert_eq!(resolved.config.api_key, "sk-ant-test");
        assert_eq!(resolved.config.model, "claude-sonnet-4-5");
        // A named effort wins over the route's default.
        assert_eq!(resolved.config.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(resolved.config.max_output_tokens, Some(64_000));
        assert_eq!(resolved.config.headers.len(), 1);
    }

    #[test]
    fn the_route_default_effort_fills_in_when_none_is_named() {
        let providers = vec![provider()];
        let mut request = selection();
        request.effort = None;
        let resolved = resolve(&providers, &request, None).unwrap();
        assert_eq!(resolved.config.reasoning_effort.as_deref(), Some("medium"));
    }

    #[test]
    fn a_model_the_registry_does_not_list_still_resolves() {
        let providers = vec![provider()];
        let mut request = selection();
        request.model = "claude-future-9".into();
        let resolved = resolve(&providers, &request, None).unwrap();
        assert_eq!(resolved.config.model, "claude-future-9");
        assert_eq!(resolved.config.max_output_tokens, None);
        assert_eq!(resolved.config.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn the_pairs_own_effort_wins_over_the_route_default() {
        let providers = vec![provider()];
        // The pair was given "low"; the route would have spent its own default,
        // and the selection carries the route's.
        let resolved = resolve(&providers, &selection(), Some("low")).unwrap();
        assert_eq!(resolved.config.reasoning_effort.as_deref(), Some("low"));

        // Nothing named anywhere leaves it unset: a model that takes no effort
        // is not sent one.
        let mut bare = provider();
        bare.models[0].default_effort = None;
        let mut request = selection();
        request.effort = None;
        let resolved = resolve(&[bare], &request, None).unwrap();
        assert_eq!(resolved.config.reasoning_effort, None);
    }

    #[test]
    fn an_unknown_provider_is_a_readable_error() {
        let providers = vec![provider()];
        let mut request = selection();
        request.provider_id = "gone".into();
        let error = resolve(&providers, &request, None).unwrap_err();
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(error.message.contains("gone"), "{}", error.message);
    }

    #[test]
    fn an_unknown_account_is_a_readable_error() {
        let providers = vec![provider()];
        let mut request = selection();
        request.account_id = "gone".into();
        let error = resolve(&providers, &request, None).unwrap_err();
        assert!(error.message.contains("no longer in the registry"));
    }

    #[test]
    fn an_env_account_without_its_variable_fails_as_auth() {
        let _guard = crate::test_env::lock_env();
        std::env::remove_var("THE_PAIR_HARNESS_MISSING");
        let mut providers = vec![provider()];
        providers[0].accounts[0].secret = Secret::Env("THE_PAIR_HARNESS_MISSING".into());
        let error = resolve(&providers, &selection(), None).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Auth);
        assert!(error.message.contains("THE_PAIR_HARNESS_MISSING"));
    }

    #[test]
    fn an_expired_oauth_account_says_to_sign_in_again() {
        let mut providers = vec![provider()];
        providers[0].accounts[0].kind = AccountKind::ChatGptOAuth;
        providers[0].accounts[0].secret = Secret::OAuth(Box::new(OAuthToken {
            access_token: "old".into(),
            refresh_token: Some("r".into()),
            expires_at: 1,
            token_type: "Bearer".into(),
            scope: None,
            account_label: Some("me@example.com".into()),
        }));
        let error = resolve(&providers, &selection(), None).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Auth);
        assert!(error.message.contains("sign in again"), "{}", error.message);
    }

    #[test]
    fn a_selection_with_no_account_is_rejected_before_any_request() {
        let mut request = selection();
        request.account_id = String::new();
        let error = resolve(&[provider()], &request, None).unwrap_err();
        assert!(error.message.contains("no model selected"));
    }

    #[test]
    fn an_expired_account_is_marked_in_the_store() {
        let dir = app_dir();
        save_providers(&dir, &[provider()]).unwrap();
        assert!(mark_account_expired(&dir, "anthropic-1", "anthropic-1-a"));

        let stored = load_providers(&dir);
        assert_eq!(stored[0].accounts[0].status, AccountStatus::AuthExpired);
        // Idempotent: a second call changes nothing and reports false.
        assert!(!mark_account_expired(&dir, "anthropic-1", "anthropic-1-a"));
        assert!(!mark_account_expired(&dir, "nope", "anthropic-1-a"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

// ---------------------------------------------------------------------------
// End-to-end: a whole turn against a local server speaking a real protocol
// ---------------------------------------------------------------------------

#[cfg(test)]
mod wire_tests {
    use super::*;
    use crate::accounts::store::save as save_providers;
    use crate::accounts::types::{
        AccountKind, AccountStatus, ModelRoute, ModelSource, Protocol, Secret,
    };
    use serde_json::json;
    use std::sync::{Arc as StdArc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::llm::ErrorKind;

    /// A local HTTP server answering every request with the next canned reply.
    /// Returns its base URL and the requests it was asked.
    async fn serve(replies: Vec<String>) -> (String, StdArc<StdMutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let asked: StdArc<StdMutex<Vec<String>>> = StdArc::new(StdMutex::new(Vec::new()));
        let recorder = StdArc::clone(&asked);
        tokio::spawn(async move {
            let mut replies = replies.into_iter();
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 8_192];
                let body = loop {
                    let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break String::new(),
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
                            Ok(0) | Err(_) => break String::new(),
                            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                        }
                        continue;
                    }
                    break String::from_utf8_lossy(&buffer[end + 4..end + 4 + length]).to_string();
                };
                recorder.lock().unwrap().push(body);
                if let Some(reply) = replies.next() {
                    let _ = socket.write_all(reply.as_bytes()).await;
                }
                let _ = socket.shutdown().await;
            }
        });
        (url, asked)
    }

    fn http(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    fn sse(events: &[&str]) -> String {
        let mut out = String::new();
        for event in events {
            out.push_str(&format!("data: {}\n\n", event));
        }
        out.push_str("data: [DONE]\n\n");
        out
    }

    /// One OpenAI Chat Completions stream with text and a usage chunk.
    fn openai_reply() -> String {
        http(&sse(&[
            r#"{"choices":[{"delta":{"content":"Plan:"}}]}"#,
            r#"{"choices":[{"delta":{"content":" do the thing."}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            r#"{"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":34}}"#,
        ]))
    }

    fn app_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("the-pair-wire-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A registry with one endpoint per (base_url, key) pair, so a test can
    /// point a selection at its own local server.
    fn registry(dir: &std::path::Path, entries: &[(&str, &str, Protocol)]) {
        let providers: Vec<Provider> = entries
            .iter()
            .map(|(base_url, key, protocol)| Provider {
                id: format!("wire-{}", short(base_url)),
                protocol: *protocol,
                base_url: (*base_url).to_string(),
                vendor: "test".into(),
                display_name: "Test".into(),
                accounts: vec![Account {
                    id: format!("wire-{}-a", short(base_url)),
                    kind: AccountKind::ApiKey,
                    label: "test".into(),
                    source: "manual".into(),
                    status: AccountStatus::Ready,
                    headers: vec![],
                    tool_names: None,
                    secret: Secret::ApiKey((*key).to_string()),
                }],
                models: vec![ModelRoute {
                    id: "test-model".into(),
                    display_name: "Test Model".into(),
                    context_window: None,
                    max_output: None,
                    efforts: vec![],
                    default_effort: None,
                    price: None,
                    source: ModelSource::Manual,
                }],
            })
            .collect();
        save_providers(dir, &providers).unwrap();
    }

    /// A stable per-endpoint suffix: the port, which is unique per test server.
    fn short(base_url: &str) -> String {
        base_url
            .rsplit(':')
            .next()
            .filter(|port| port.chars().all(|ch| ch.is_ascii_digit()))
            .unwrap_or("0")
            .to_string()
    }

    fn selection_for(base_url: &str) -> ModelSelection {
        ModelSelection {
            provider_id: format!("wire-{}", short(base_url)),
            account_id: format!("wire-{}-a", short(base_url)),
            model: "test-model".into(),
            effort: None,
            backup: None,
        }
    }

    /// A real workspace, so a tool call has somewhere to run.
    fn workspace() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("the-pair-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn request(
        selection: ModelSelection,
        cancel: StdArc<AtomicBool>,
        directory: &std::path::Path,
    ) -> TurnRequest {
        TurnRequest {
            selection,
            effort: None,
            system: "You are the mentor.".into(),
            history: vec![json!({"role": "user", "content": "plan it"})],
            cache_key: "pair-1-mentor".into(),
            cancel,
            directory: directory.display().to_string(),
            policy: ToolPolicy::Full,
        }
    }

    /// A request whose history is what an earlier turn of the same run
    /// left behind, so the replay itself is under test.
    fn request_with_history(
        selection: ModelSelection,
        cancel: StdArc<AtomicBool>,
        directory: &std::path::Path,
        history: Vec<serde_json::Value>,
    ) -> TurnRequest {
        TurnRequest {
            history,
            ..request(selection, cancel, directory)
        }
    }

    /// A tool-event sink that records what happened.
    fn tool_log() -> StdArc<StdMutex<Vec<(String, bool)>>> {
        StdArc::new(StdMutex::new(Vec::new()))
    }

    fn record_tools(
        log: &StdArc<StdMutex<Vec<(String, bool)>>>,
    ) -> impl FnMut(ToolEvent<'_>) -> CancelResult + '_ {
        let log = StdArc::clone(log);
        move |event| {
            if let ToolEvent::Finished { name, ok, .. } = event {
                log.lock().unwrap().push((name.to_string(), ok));
            }
            CancelResult::Continue
        }
    }

    #[tokio::test]
    async fn the_turns_history_is_replayed_and_returned_for_the_next_one() {
        let dir = app_dir();
        let (url, asked) = serve(vec![openai_reply()]).await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let workspace = workspace();
        let tools = tool_log();

        // What the mentor's first turn of the run left behind, then the
        // turn it is about to answer — the shape the caller stores.
        let mut prior = vec![
            json!({"role": "user", "content": "build the parser"}),
            json!({"role": "assistant", "content": "Step 1: read the file"}),
        ];
        prior.push(json!({"role": "user", "content": "plan it"}));
        let outcome = run_turn(
            &dir,
            &request_with_history(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
                prior.clone(),
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the turn completes");

        // The request replayed the earlier turns and ended on this turn's user
        // message — the system prompt never leaks into the stored history.
        let body = asked.lock().unwrap().remove(0);
        assert!(body.contains("build the parser"), "{}", body);
        assert!(body.contains("Step 1: read the file"), "{}", body);
        assert!(body.contains("plan it"), "{}", body);

        // What comes back is the whole conversation, ready to store and replay.
        assert_eq!(&outcome.history[..prior.len()], &prior[..]);
        assert_eq!(
            outcome.history[prior.len()]["content"],
            "Plan: do the thing."
        );
        assert!(
            outcome
                .history
                .iter()
                .all(|message| message["role"] != "system"),
            "the system prompt rides on the request only"
        );
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_tool_turn_keeps_its_calls_and_results_in_the_history() {
        let workspace = workspace();
        let dir = app_dir();
        let (url, _) = serve(vec![
            tool_call_reply(
                "write",
                r#"{"path":"hello.txt","content":"kept\n"}"#,
                "call-1",
            ),
            openai_reply(),
        ])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let tools = tool_log();

        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the turn completes");

        // The exchange is stored the way the wire format needs it: the call, its
        // result, then the reply that used it.
        let history = &outcome.history;
        assert_eq!(history[0]["role"], "user");
        assert_eq!(history[1]["role"], "assistant");
        assert_eq!(history[1]["tool_calls"][0]["id"], "call-1");
        assert_eq!(history[2]["role"], "tool");
        assert_eq!(history[2]["tool_call_id"], "call-1");
        assert!(history[2]["content"]
            .as_str()
            .unwrap()
            .contains("hello.txt"));
        assert_eq!(history[3]["content"], "Plan: do the thing.");
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn an_over_long_history_is_replayed_from_its_newest_turns() {
        let dir = app_dir();
        let (url, asked) = serve(vec![openai_reply()]).await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let workspace = workspace();
        let tools = tool_log();

        // Two turns' worth of messages past the cap, oldest first.
        let mut prior: Vec<serde_json::Value> = Vec::new();
        for index in 0..(crate::harness::history::MAX_HISTORY_MESSAGES / 2 + 2) {
            prior.push(json!({"role": "user", "content": format!("turn {}", index)}));
            prior.push(json!({"role": "assistant", "content": format!("answer {}", index)}));
        }
        let oldest = prior[0]["content"].as_str().unwrap().to_string();
        let outcome = run_turn(
            &dir,
            &request_with_history(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
                prior.clone(),
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the turn completes");

        let body = asked.lock().unwrap().remove(0);
        assert!(!body.contains(&oldest), "the oldest turns were dropped");
        assert!(
            body.contains("was dropped to stay within your context window"),
            "a notice stands in for them"
        );
        // What the turn stores is the bounded window, so the pair's
        // conversation does not grow without end — and no notice is kept in it.
        assert_eq!(
            outcome.history.len(),
            crate::harness::history::MAX_HISTORY_MESSAGES
        );
        assert!(outcome
            .history
            .iter()
            .all(|message| message["role"] != "system"));
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_whole_turn_streams_its_text_and_reports_the_usage() {
        let dir = app_dir();
        let (url, asked) = serve(vec![openai_reply()]).await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);

        let workspace = workspace();
        let mut chunks = Vec::new();
        let tools = tool_log();
        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |delta| {
                if let Delta::Text(text) = delta {
                    chunks.push(text.to_string());
                }
                CancelResult::Continue
            },
            &mut record_tools(&tools),
        )
        .await
        .expect("the turn completes");
        assert!(tools.lock().unwrap().is_empty(), "no tools were called");

        assert_eq!(outcome.text, "Plan: do the thing.");
        assert_eq!(outcome.usage.prompt_tokens, 120);
        assert_eq!(outcome.usage.completion_tokens, 34);
        assert_eq!(
            chunks,
            vec!["Plan:".to_string(), " do the thing.".to_string()]
        );
        assert!(!outcome.fell_back);

        // The system prompt went as the system message, and the credential as
        // the bearer token.
        let body = asked.lock().unwrap().remove(0);
        assert!(body.contains("\"role\":\"system\""), "{}", body);
        assert!(body.contains("plan it"), "{}", body);
        assert_eq!(
            outcome.answered_by.model, "test-model",
            "the outcome names the model that answered"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_cancelled_turn_stops_where_it_is() {
        // A reply that never ends: the only way out is the cancel flag.
        let (url, _) = serve(vec![http(
            "data: {\"choices\":[{\"delta\":{\"content\":\"first\"}}]}\n\n",
        )])
        .await;
        let dir = app_dir();
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let workspace = workspace();
        let tools = tool_log();

        let cancel = StdArc::new(AtomicBool::new(false));
        let trigger = StdArc::clone(&cancel);
        let failure = run_turn(
            &dir,
            &request(selection_for(&url), cancel, &workspace),
            &mut move |_| {
                trigger.store(true, Ordering::SeqCst);
                CancelResult::Continue
            },
            &mut record_tools(&tools),
        )
        .await
        .expect_err("a cancelled turn is an error");

        let error = failure.error;
        assert!(error.is_cancelled(), "{:?}", error);
        assert!(!error.retryable, "a cancellation is not retried");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn the_backup_answers_when_the_primary_is_refused() {
        let dir = app_dir();
        let (primary, _) = serve(vec![
            "HTTP/1.1 401 Unauthorized\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}"
                .to_string(),
        ])
        .await;
        let (backup, _) = serve(vec![openai_reply()]).await;
        let workspace = workspace();
        let tools = tool_log();
        registry(
            &dir,
            &[
                (&primary, "sk-bad", Protocol::OpenAiChat),
                (&backup, "sk-good", Protocol::OpenAiChat),
            ],
        );

        let selection = ModelSelection {
            backup: Some(Box::new(selection_for(&backup))),
            ..selection_for(&primary)
        };
        let outcome = run_turn(
            &dir,
            &request(selection, StdArc::new(AtomicBool::new(false)), &workspace),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the backup takes over");

        assert_eq!(outcome.text, "Plan: do the thing.");
        assert!(outcome.fell_back);
        assert_eq!(
            outcome.answered_by.provider_id,
            format!("wire-{}", short(&backup))
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_failure_with_no_backup_is_reported_as_it_happened() {
        let dir = app_dir();
        let (url, _) = serve(vec![
            "HTTP/1.1 401 Unauthorized\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}"
                .to_string(),
        ])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let workspace = workspace();
        let tools = tool_log();

        let failure = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect_err("a refused credential with no backup fails the turn");

        let error = failure.error;
        assert_eq!(error.kind, ErrorKind::Auth);
        assert!(!error.is_cancelled());
        assert!(
            error.message.contains("401"),
            "the provider's status reaches the user: {}",
            error.message
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_rate_limit_is_retried_before_it_reaches_the_caller() {
        let dir = app_dir();
        // The transport retries a 429, so the same refusal is served twice and
        // the second attempt succeeds.
        let (url, asked) = serve(vec![
            "HTTP/1.1 429 Too Many Requests\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}"
                .to_string(),
            openai_reply(),
        ])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let workspace = workspace();
        let tools = tool_log();

        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the retry succeeds");

        assert_eq!(outcome.text, "Plan: do the thing.");
        assert_eq!(asked.lock().unwrap().len(), 2, "the 429 was retried once");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// One OpenAI stream that asks for a tool call.
    ///
    /// Built with `serde_json` rather than `format!`: the arguments are
    /// themselves JSON, and splicing them into a JSON string by hand leaves the
    /// quotes unbalanced.
    fn tool_call_reply(name: &str, arguments: &str, id: &str) -> String {
        let call = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": id,
                        "function": {"name": name, "arguments": arguments}
                    }]
                }
            }]
        });
        let end = json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]});
        http(&sse(&[&call.to_string(), &end.to_string()]))
    }

    #[tokio::test]
    async fn the_executor_runs_the_tools_the_model_asks_for() {
        let workspace = workspace();
        let dir = app_dir();
        let (url, asked) = serve(vec![
            tool_call_reply(
                "write",
                r#"{"path":"hello.txt","content":"written by the model\n"}"#,
                "call-1",
            ),
            tool_call_reply("bash", r#"{"command":"cat hello.txt"}"#, "call-2"),
            openai_reply(),
        ])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let tools = tool_log();

        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the turn completes");

        // The file the model wrote is really on disk, in the workspace.
        let written = std::fs::read_to_string(workspace.join("hello.txt")).unwrap();
        assert_eq!(written, "written by the model\n");

        let log = tools.lock().unwrap();
        assert_eq!(log.len(), 2, "both calls ran: {:?}", log);
        assert_eq!(log[0], ("write".to_string(), true));
        assert_eq!(log[1], ("bash".to_string(), true));

        // The tool results went back to the model as tool turns, so the third
        // request saw the whole exchange.
        let bodies: Vec<String> = asked.lock().unwrap().clone();
        assert_eq!(bodies.len(), 3, "one request per round");
        assert!(bodies[1].contains("\"role\":\"tool\""), "{}", bodies[1]);
        assert!(bodies[1].contains("written by the model"), "{}", bodies[1]);
        assert!(bodies[2].contains("written by the model"), "{}", bodies[2]);

        assert_eq!(outcome.tool_calls.len(), 2);
        assert!(outcome.tool_calls.iter().all(|call| call.ok));
        assert_eq!(outcome.tool_calls[0].summary, "write hello.txt");
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn the_usage_of_every_request_of_the_loop_is_reported() {
        let workspace = workspace();
        let dir = app_dir();
        // Three rounds, each with its own usage: the turn is billed for all
        // three, not only the one that ended it.
        let with_usage = |prompt: u64, completion: u64| {
            let call = json!({
                "choices": [{"delta": {"tool_calls": [{"index": 0, "id": "c",
                    "function": {"name": "read", "arguments": "{\"path\":\"missing.txt\"}"}}]}}]}
            );
            let end = json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]});
            let usage = json!({"choices": [], "usage": {"prompt_tokens": prompt,
                                                        "completion_tokens": completion}});
            http(&sse(&[
                &call.to_string(),
                &end.to_string(),
                &usage.to_string(),
            ]))
        };
        let (url, _) = serve(vec![
            with_usage(100, 10),
            with_usage(200, 20),
            openai_reply(),
        ])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let tools = tool_log();

        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the turn completes");

        assert_eq!(outcome.usage.prompt_tokens, 420);
        assert_eq!(outcome.usage.completion_tokens, 64);
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_read_only_agent_is_refused_the_tools_it_may_not_use() {
        let workspace = workspace();
        let dir = app_dir();
        let (url, _) = serve(vec![
            tool_call_reply("bash", r#"{"command":"echo nope"}"#, "call-1"),
            openai_reply(),
        ])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let tools = tool_log();

        let mut request = request(
            selection_for(&url),
            StdArc::new(AtomicBool::new(false)),
            &workspace,
        );
        request.policy = ToolPolicy::ReadOnly;

        let outcome = run_turn(
            &dir,
            &request,
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("a refused tool does not end the turn");

        // The refusal is the tool's answer, so the model can carry on.
        let log = tools.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert!(!log[0].1, "the call failed");
        assert!(outcome.tool_calls[0].output.contains("not available"));
        assert_eq!(outcome.text, "Plan: do the thing.");
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_gateway_that_renames_a_tool_sends_the_wire_name() {
        let workspace = workspace();
        let dir = app_dir();
        let (url, asked) = serve(vec![
            tool_call_reply("bash", r#"{"command":"echo hi"}"#, "call-1"),
            openai_reply(),
        ])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        // The account carries Muse Code's rename: bash → run_command.
        {
            use crate::accounts::store::{global_lock, save as save_providers};
            let _guard = global_lock().lock().unwrap();
            let mut providers = load_providers(&dir);
            providers[0].accounts[0].tool_names =
                Some([("bash".to_string(), "run_command".to_string())].into());
            save_providers(&dir, &providers).unwrap();
        }
        let tools = tool_log();

        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the turn completes");

        // The request named the gateway's own tool, and the call came back as
        // ours, so the tool that ran was bash.
        let bodies: Vec<String> = asked.lock().unwrap().clone();
        assert!(bodies[0].contains("run_command"), "{}", bodies[0]);
        assert!(!bodies[0].contains("\"bash\""), "{}", bodies[0]);
        assert_eq!(tools.lock().unwrap()[0].0, "bash");
        assert_eq!(outcome.tool_calls[0].name, "bash");
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_model_that_never_stops_calling_tools_is_stopped() {
        let workspace = workspace();
        let dir = app_dir();
        // One more reply than the turn may spend: the last one is never asked for.
        let replies: Vec<String> = (0..=MAX_TOOL_CALLS_PER_TURN)
            .map(|index| {
                tool_call_reply(
                    "read",
                    &format!(r#"{{"path":"missing-{}.txt"}}"#, index),
                    &format!("call-{}", index),
                )
            })
            .collect();
        let (url, _) = serve(replies).await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);

        let failure = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tool_log()),
        )
        .await
        .expect_err("the runaway is stopped");

        // The reply that went over the limit asked for a call nobody ran: it is
        // answered in the history handed back, so a retry can replay it.
        let last = failure.history.last().expect("the tools ran");
        assert_eq!(last["role"], "tool");
        assert!(last["content"].as_str().unwrap().contains("did not finish"));
        let error = failure.error;
        assert_eq!(error.kind, ErrorKind::BadResponse);
        assert!(
            error.message.contains("tool calls in one turn"),
            "{}",
            error.message
        );
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn an_anthropic_turn_uses_the_messages_protocol() {
        let dir = app_dir();
        let (url, asked) = serve(vec![http(&format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: {}\n\n",
            r#"{"type":"message_start","message":{"usage":{"input_tokens":88,"output_tokens":0}}}"#,
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Reviewed."}}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":12}}"#,
            r#"{"type":"message_stop"}"#,
        ))])
        .await;
        registry(&dir, &[(&url, "sk-ant", Protocol::Anthropic)]);
        let workspace = workspace();
        let tools = tool_log();

        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect("the anthropic turn completes");

        assert_eq!(outcome.text, "Reviewed.");
        assert_eq!(outcome.usage.prompt_tokens, 88);
        assert_eq!(outcome.usage.completion_tokens, 12);
        let body = asked.lock().unwrap().remove(0);
        assert!(
            body.contains("\"system\""),
            "the system prompt is top-level: {}",
            body
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_discarded_attempts_output_is_not_part_of_the_reply() {
        let dir = app_dir();
        // The first attempt streams reasoning, then its stream is torn: the
        // transport retries and tells the turn to drop what it showed before the
        // retry's deltas arrive.
        let torn = http(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"half a \"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thought\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":\n\n",
        );
        let (url, _) = serve(vec![torn, openai_reply()]).await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let workspace = workspace();
        let tools = tool_log();

        let mut discarded = 0usize;
        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |delta| {
                if matches!(delta, Delta::Discard) {
                    discarded += 1;
                }
                CancelResult::Continue
            },
            &mut record_tools(&tools),
        )
        .await
        .expect("the retry answers");

        assert_eq!(discarded, 1, "the failed attempt was discarded");
        assert_eq!(outcome.text, "Plan: do the thing.");
        assert!(
            outcome.reasoning.is_empty(),
            "nothing the discarded attempt reasoned survives: {:?}",
            outcome.reasoning
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_turn_that_ran_a_tool_is_not_handed_to_the_backup() {
        let dir = app_dir();
        // The primary writes to the workspace and is then refused. A backup
        // answering from a history without that exchange would hide a change
        // that already happened, so the fallback stops here.
        let (primary, _) = serve(vec![
            tool_call_reply(
                "write",
                r#"{"path":"hello.txt","content":"kept\n"}"#,
                "call-1",
            ),
            "HTTP/1.1 401 Unauthorized\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}"
                .to_string(),
        ])
        .await;
        let (backup, _) = serve(vec![openai_reply()]).await;
        let workspace = workspace();
        let tools = tool_log();
        registry(
            &dir,
            &[
                (&primary, "sk-bad", Protocol::OpenAiChat),
                (&backup, "sk-good", Protocol::OpenAiChat),
            ],
        );

        let selection = ModelSelection {
            backup: Some(Box::new(selection_for(&backup))),
            ..selection_for(&primary)
        };
        let failure = run_turn(
            &dir,
            &request(selection, StdArc::new(AtomicBool::new(false)), &workspace),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect_err("the primary's work is not handed to another model");

        assert_eq!(failure.error.kind, ErrorKind::Auth);
        assert_eq!(
            failure.selection.provider_id,
            format!("wire-{}", short(&primary)),
            "the primary is the model that failed"
        );
        // What the primary wrote stays written.
        assert_eq!(
            std::fs::read_to_string(workspace.join("hello.txt")).unwrap(),
            "kept\n"
        );
        // And the failure hands back the conversation that wrote it: a retry
        // replaying a history without this exchange would write it again.
        let roles: Vec<&str> = failure
            .history
            .iter()
            .map(|message| message["role"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            roles,
            ["user", "assistant", "tool"],
            "{:?}",
            failure.history
        );
        assert_eq!(failure.history[2]["tool_call_id"], "call-1");
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A reply that says something and calls a tool in the same breath.
    fn text_and_tool_call_reply(text: &str, name: &str, arguments: &str, id: &str) -> String {
        let said = json!({"choices": [{"delta": {"content": text}}]});
        let call = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": id,
                        "function": {"name": name, "arguments": arguments}
                    }]
                }
            }]
        });
        let end = json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]});
        http(&sse(&[
            &said.to_string(),
            &call.to_string(),
            &end.to_string(),
        ]))
    }

    #[tokio::test]
    async fn a_report_written_beside_the_last_tool_call_is_the_turns_answer() {
        let workspace = workspace();
        let dir = app_dir();
        // The model reports next to its final call, then closes with an empty
        // message: the work and its report both exist.
        let (url, _) = serve(vec![
            text_and_tool_call_reply(
                "Done: wrote hello.txt.",
                "write",
                r#"{"path":"hello.txt","content":"hi\n"}"#,
                "call-1",
            ),
            http(&sse(&[
                r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            ])),
        ])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);

        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace,
            ),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tool_log()),
        )
        .await
        .expect("an empty closing message does not fail the turn");

        assert_eq!(outcome.text, "Done: wrote hello.txt.");
        // The empty closing message is not a turn any provider takes back, so
        // the stored conversation ends on the tool result.
        let last = outcome.history.last().unwrap();
        assert_eq!(last["role"], "tool", "{:?}", outcome.history);
        assert!(outcome
            .history
            .iter()
            .all(|message| message.get("tool_calls") != Some(&json!([]))));
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_plain_reply_is_stored_without_a_tool_calls_key() {
        let dir = app_dir();
        let (url, _) = serve(vec![openai_reply()]).await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);

        let outcome = run_turn(
            &dir,
            &request(
                selection_for(&url),
                StdArc::new(AtomicBool::new(false)),
                &workspace(),
            ),
            &mut |_| CancelResult::Continue,
            &mut |_| CancelResult::Continue,
        )
        .await
        .unwrap();

        let reply = outcome.history.last().unwrap();
        assert_eq!(reply["role"], "assistant");
        assert!(reply.get("tool_calls").is_none(), "{}", reply);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_turn_inside_a_tool_call_stops_when_it_is_cancelled() {
        let workspace = workspace();
        let dir = app_dir();
        let (url, _) = serve(vec![tool_call_reply(
            "bash",
            r#"{"command":"sleep 30","timeout":60}"#,
            "call-1",
        )])
        .await;
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let cancel = StdArc::new(AtomicBool::new(false));
        let pause = StdArc::clone(&cancel);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            pause.store(true, Ordering::SeqCst);
        });

        let started = std::time::Instant::now();
        let failure = run_turn(
            &dir,
            &request(selection_for(&url), cancel, &workspace),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tool_log()),
        )
        .await
        .expect_err("the pause stops the turn");

        assert!(failure.error.is_cancelled(), "{:?}", failure.error);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the turn sat out the command: {:?}",
            started.elapsed()
        );
        // The call that was cut short is answered, so the history is replayable.
        let last = failure.history.last().expect("the call is on record");
        assert_eq!(last["tool_call_id"], "call-1");
        assert!(last["content"].as_str().unwrap().contains("did not finish"));
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_turn_waiting_for_its_first_token_stops_when_it_is_cancelled() {
        let dir = app_dir();
        // A server that takes the request and never answers it.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        registry(&dir, &[(&url, "sk-test", Protocol::OpenAiChat)]);
        let cancel = StdArc::new(AtomicBool::new(false));
        let pause = StdArc::clone(&cancel);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            pause.store(true, Ordering::SeqCst);
        });

        let started = std::time::Instant::now();
        let failure = run_turn(
            &dir,
            &request(selection_for(&url), cancel, &workspace()),
            &mut |_| CancelResult::Continue,
            &mut |_| CancelResult::Continue,
        )
        .await
        .expect_err("the pause stops the turn");

        assert!(failure.error.is_cancelled(), "{:?}", failure.error);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(failure.history.is_empty(), "nothing ran");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn the_refusal_that_ended_the_turn_names_the_account_to_mark() {
        let dir = app_dir();
        // Both models are refused. The failure the turn reports is the backup's,
        // so the backup's credential is the one that gets marked expired — the
        // primary's is not blamed for a refusal it did not give.
        let refusal = || {
            "HTTP/1.1 401 Unauthorized\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}"
                .to_string()
        };
        let (primary, _) = serve(vec![refusal()]).await;
        let (backup, _) = serve(vec![refusal()]).await;
        let workspace = workspace();
        let tools = tool_log();
        registry(
            &dir,
            &[
                (&primary, "sk-bad", Protocol::OpenAiChat),
                (&backup, "sk-also-bad", Protocol::OpenAiChat),
            ],
        );

        let selection = ModelSelection {
            backup: Some(Box::new(selection_for(&backup))),
            ..selection_for(&primary)
        };
        let failure = run_turn(
            &dir,
            &request(selection, StdArc::new(AtomicBool::new(false)), &workspace),
            &mut |_| CancelResult::Continue,
            &mut record_tools(&tools),
        )
        .await
        .expect_err("both refusals fail the turn");

        assert_eq!(failure.error.kind, ErrorKind::Auth);
        assert_eq!(
            failure.selection.account_id,
            format!("wire-{}-a", short(&backup)),
            "the backup is the model that was refused last"
        );
        assert!(mark_account_expired(
            &dir,
            &failure.selection.provider_id,
            &failure.selection.account_id,
        ));

        let stored = crate::accounts::store::load(&dir);
        let status_of = |id: &str| {
            stored
                .iter()
                .find(|provider| provider.id == id)
                .and_then(|provider| provider.accounts.first())
                .map(|account| account.status)
        };
        assert_eq!(
            status_of(&format!("wire-{}", short(&backup))),
            Some(AccountStatus::AuthExpired)
        );
        assert_eq!(
            status_of(&format!("wire-{}", short(&primary))),
            Some(AccountStatus::Ready),
            "a credential the provider accepted is left alone"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
