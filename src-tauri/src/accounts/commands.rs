//! Tauri commands for the model registry.
//!
//! Every command resolves the app data dir, takes the store lock, does a
//! read-modify-write and returns frontend views. Views never carry secrets —
//! see [`ProviderView`].

use super::registry::{upsert_drafts, upsert_manual, AccountDraft, ProviderDraft};
use super::store::{
    global_lock, load as load_providers, oauth_path, save as save_providers, write_private,
};
use super::types::{
    AccountKind, AccountStatus, HeaderPair, ModelRoute, ModelSource, Protocol, Provider,
    ProviderView, Secret,
};
use crate::login::{self, LoginService};
use crate::sources::{self, ImportCandidate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

// ---------------------------------------------------------------------------
// Frontend payloads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateProviderView {
    pub protocol: Protocol,
    pub base_url: String,
    pub vendor: String,
    pub display_name: String,
    pub accounts: Vec<CandidateAccountView>,
    pub models: Vec<ModelRoute>,
    /// True when this endpoint is already in the registry.
    pub already_imported: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateAccountView {
    pub kind: AccountKind,
    pub label: String,
    pub source: String,
    pub status: AccountStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<HeaderPair>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateView {
    pub source_id: String,
    pub source_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub providers: Vec<CandidateProviderView>,
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Accounts this scan would add or refresh.
    pub account_count: usize,
    /// Providers this scan would create.
    pub new_provider_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProviderInput {
    pub protocol: Protocol,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAccountKeyInput {
    pub provider_id: String,
    pub account_id: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginServiceView {
    pub id: String,
    pub display_name: String,
    pub needs_browser: bool,
    pub needs_key: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRefInput {
    pub provider_id: String,
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatusView {
    /// `waiting`, `done`, `failed`, `cancelled` or `unknown`.
    pub state: String,
    pub service: String,
    /// Milliseconds since the flow started, so the UI can show real progress.
    pub waiting_ms: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub providers: Option<Vec<ProviderView>>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn accounts_list(app: AppHandle) -> Result<Vec<ProviderView>, String> {
    let dir = app_data_dir(&app)?;
    let _guard = global_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    Ok(views(&load_providers(&dir)))
}

/// Dry run: what a scan would import, without writing anything.
#[tauri::command]
pub fn accounts_scan(
    app: AppHandle,
    source_ids: Option<Vec<String>>,
) -> Result<Vec<CandidateView>, String> {
    let dir = app_data_dir(&app)?;
    let candidates = match source_ids {
        Some(ids) if !ids.is_empty() => sources::scan(&ids),
        _ => sources::scan_detected(),
    };
    let existing = load_providers(&dir);
    Ok(candidates
        .iter()
        .map(|candidate| candidate_view(candidate, &existing))
        .collect())
}

/// Scans the named sources and writes what it finds.
#[tauri::command]
pub fn accounts_commit_import(
    app: AppHandle,
    source_ids: Vec<String>,
) -> Result<Vec<ProviderView>, String> {
    let dir = app_data_dir(&app)?;
    let _guard = global_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut providers = load_providers(&dir);
    let candidates = sources::scan(&source_ids);
    let drafts: Vec<ProviderDraft> = candidates
        .into_iter()
        .flat_map(|candidate| candidate.drafts)
        .collect();
    upsert_drafts(&mut providers, &drafts);
    save_providers(&dir, &providers)?;
    Ok(views(&providers))
}

/// Adds a hand-written provider (any protocol, any endpoint).
#[tauri::command]
pub fn accounts_add_provider(
    app: AppHandle,
    input: AddProviderInput,
) -> Result<Vec<ProviderView>, String> {
    let dir = app_data_dir(&app)?;
    let _guard = global_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut providers = load_providers(&dir);
    let secret = if input.api_key.trim().is_empty() {
        Secret::None
    } else {
        Secret::ApiKey(input.api_key.trim().to_string())
    };
    let models = input
        .models
        .iter()
        .map(|id| ModelRoute {
            id: id.trim().to_string(),
            display_name: id.trim().to_string(),
            context_window: None,
            max_output: None,
            efforts: Vec::new(),
            default_effort: None,
            price: None,
            source: ModelSource::Manual,
        })
        .filter(|model| !model.id.is_empty())
        .collect();
    // A URL that normalizes to nothing (`/`, `///`) cannot address an endpoint,
    // and the command must not panic over one.
    upsert_manual(
        &mut providers,
        input.protocol,
        &input.base_url,
        input.label.as_deref().unwrap_or("manual"),
        secret,
        models,
    )?;
    save_providers(&dir, &providers)?;
    Ok(views(&providers))
}

/// Pastes a new key onto an existing account (also clears an expired OAuth
/// account, which the user may have resolved out of band).
#[tauri::command]
pub fn accounts_set_account_key(
    app: AppHandle,
    input: SetAccountKeyInput,
) -> Result<Vec<ProviderView>, String> {
    let dir = app_data_dir(&app)?;
    let _guard = global_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut providers = load_providers(&dir);
    let secret = Secret::ApiKey(input.api_key.trim().to_string());
    let updated = super::registry::replace_secret(
        &mut providers,
        &input.provider_id,
        &input.account_id,
        secret,
    );
    if !updated {
        return Err(format!(
            "No account {} on provider {}",
            input.account_id, input.provider_id
        ));
    }
    save_providers(&dir, &providers)?;
    Ok(views(&providers))
}

#[tauri::command]
pub fn accounts_delete_account(
    app: AppHandle,
    input: AccountRefInput,
) -> Result<Vec<ProviderView>, String> {
    let dir = app_data_dir(&app)?;
    let providers = delete_account(&dir, &input.provider_id, &input.account_id)?;
    Ok(views(&providers))
}

/// Removes an account from the registry, along with the sign-in it cached.
fn delete_account(
    dir: &Path,
    provider_id: &str,
    account_id: &str,
) -> Result<Vec<Provider>, String> {
    let _guard = global_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut providers = load_providers(dir);
    let mut removed_sources: Vec<String> = Vec::new();
    if let Some(provider) = providers
        .iter_mut()
        .find(|provider| provider.id == provider_id)
    {
        provider.accounts.retain(|account| {
            let removed = account.id == account_id;
            if removed {
                removed_sources.push(account.source.clone());
            }
            !removed
        });
    }
    // A provider with no accounts left is not selectable, so it goes too.
    providers.retain(|provider| !provider.accounts.is_empty());
    save_providers(dir, &providers)?;
    for source in &removed_sources {
        forget_sign_in(dir, &providers, source);
    }
    Ok(providers)
}

/// Deletes the token file a sign-in flow cached for a deleted account.
///
/// The sign-in services keep their token in `oauth/<service>.json`, outside the
/// registry. Left behind, the secret outlives the account the user just
/// deleted, and the next sign-in finds it, refreshes it and brings the same
/// account back without ever opening the browser. The file stays while any
/// other account still comes from that sign-in.
fn forget_sign_in(dir: &Path, remaining: &[Provider], source: &str) {
    let Some(service) = source.strip_prefix("oauth:") else {
        return;
    };
    // The service names the file, so it must be a plain name and nothing else.
    if service.is_empty()
        || !service
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return;
    }
    let still_used = remaining
        .iter()
        .flat_map(|provider| provider.accounts.iter())
        .any(|account| account.source == source);
    if still_used {
        return;
    }
    let path = oauth_path(dir, service);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!(
            "[accounts] Could not delete the cached sign-in at {}: {}",
            path.display(),
            error
        ),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferRouteInput {
    /// `mentor` or `executor`.
    pub role: String,
    pub model_id: String,
    pub provider_id: String,
}

/// Records the endpoint a model was picked under for a role.
///
/// The picker hands the pair a bare model id, and two endpoints can list the
/// same one — a ChatGPT plan and an OpenAI API key, say. This is how the choice
/// between them reaches the turn that has to make it.
#[tauri::command]
pub fn accounts_prefer_route(app: AppHandle, input: PreferRouteInput) -> Result<(), String> {
    let dir = app_data_dir(&app)?;
    crate::harness::prefer_route(&dir, &input.role, &input.model_id, &input.provider_id)
}

/// The sign-in services the UI offers, so it can label the button correctly.
#[tauri::command]
pub fn accounts_login_services() -> Vec<LoginServiceView> {
    [
        LoginService::ChatGpt,
        LoginService::CommandCode,
        LoginService::OpencodeGo,
        LoginService::Muse,
    ]
    .into_iter()
    .map(|service| LoginServiceView {
        id: service.id().to_string(),
        display_name: service.display_name().to_string(),
        needs_browser: service.needs_browser(),
        needs_key: service.needs_key(),
    })
    .collect()
}

/// Starts a sign-in flow and returns a session id to poll.
#[tauri::command]
pub async fn accounts_launch_oauth(
    app: AppHandle,
    service: String,
    api_key: Option<String>,
) -> Result<String, String> {
    let Some(service) = LoginService::from_id(&service) else {
        return Err(format!("Unknown sign-in service `{}`", service));
    };
    let dir = app_data_dir(&app)?;
    let session_id = next_session_id(service);
    let outcome: Arc<Mutex<Option<Result<Vec<ProviderView>, String>>>> = Arc::new(Mutex::new(None));
    let cancelled = Arc::new(AtomicBool::new(false));

    let task_outcome = Arc::clone(&outcome);
    let task_cancelled = Arc::clone(&cancelled);
    let task_dir = dir.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let result = tokio::time::timeout(login::BROWSER_TIMEOUT, async {
            if task_cancelled.load(Ordering::SeqCst) {
                return Err("Sign-in cancelled.".to_string());
            }
            login::launch(service, &task_dir, api_key).await
        })
        .await
        .unwrap_or_else(|_| Err("Sign-in timed out after 5 minutes.".to_string()));

        let stored = result.and_then(|outcome| {
            let _guard = global_lock()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut providers = load_providers(&task_dir);
            upsert_drafts(
                &mut providers,
                &[ProviderDraft {
                    protocol: outcome.protocol,
                    base_url: outcome.base_url,
                    vendor: outcome.vendor,
                    display_name: outcome.display_name,
                    accounts: vec![outcome.account],
                    models: outcome.models,
                }],
            );
            save_providers(&task_dir, &providers).map(|()| views(&providers))
        });
        *task_outcome
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(stored);
    });

    sessions()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            session_id.clone(),
            OAuthSession {
                service,
                started_at: Instant::now(),
                outcome,
                cancelled,
                handle,
            },
        );
    Ok(session_id)
}

#[tauri::command]
pub fn accounts_oauth_status(session_id: String) -> OAuthStatusView {
    let (view, finished) = {
        let sessions = sessions().lock().unwrap_or_else(|error| error.into_inner());
        let Some(session) = sessions.get(&session_id) else {
            return OAuthStatusView {
                state: "unknown".into(),
                service: String::new(),
                waiting_ms: 0,
                message: Some("This sign-in session is gone.".into()),
                providers: None,
            };
        };
        let waiting_ms = session.started_at.elapsed().as_millis();
        if session.cancelled.load(Ordering::SeqCst) {
            (
                OAuthStatusView {
                    state: "cancelled".into(),
                    service: session.service.id().into(),
                    waiting_ms,
                    message: None,
                    providers: None,
                },
                true,
            )
        } else {
            let outcome = session
                .outcome
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (
                status_view(outcome.as_ref(), session.service.id(), waiting_ms),
                outcome.is_some(),
            )
        }
    };
    if finished {
        // A terminal state has been reported, so the session has nothing left to
        // say: dropping it keeps the map from growing one entry per sign-in.
        sessions()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&session_id);
    }
    view
}

/// The reply for one poll of `session_id`.
///
/// The outcome is cloned, never taken: a poll response the UI never sees (a
/// webview reload, a remounted modal) must not leave the session reporting
/// `waiting` forever.
fn status_view(
    outcome: Option<&Result<Vec<ProviderView>, String>>,
    service: &str,
    waiting_ms: u128,
) -> OAuthStatusView {
    let base = OAuthStatusView {
        state: String::new(),
        service: service.to_string(),
        waiting_ms,
        message: None,
        providers: None,
    };
    match outcome {
        None => OAuthStatusView {
            state: "waiting".into(),
            ..base
        },
        Some(Err(message)) => OAuthStatusView {
            state: "failed".into(),
            message: Some(message.clone()),
            ..base
        },
        Some(Ok(providers)) => OAuthStatusView {
            state: "done".into(),
            providers: Some(providers.clone()),
            ..base
        },
    }
}

#[tauri::command]
pub fn accounts_cancel_oauth(session_id: String) -> Result<(), String> {
    let mut sessions = sessions().lock().unwrap_or_else(|error| error.into_inner());
    let Some(session) = sessions.remove(&session_id) else {
        return Err(format!("Unknown sign-in session `{}`", session_id));
    };
    session.cancelled.store(true, Ordering::SeqCst);
    session.handle.abort();
    Ok(())
}

// ---------------------------------------------------------------------------
// OAuth session bookkeeping
// ---------------------------------------------------------------------------

struct OAuthSession {
    service: LoginService,
    started_at: Instant,
    outcome: Arc<Mutex<Option<Result<Vec<ProviderView>, String>>>>,
    cancelled: Arc<AtomicBool>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

static SESSIONS: OnceLock<Mutex<HashMap<String, OAuthSession>>> = OnceLock::new();

/// Two sign-ins launched inside the same millisecond must not overwrite each
/// other's session, so the clock is only part of the id.
static SESSION_SEQ: AtomicU32 = AtomicU32::new(0);

fn sessions() -> &'static Mutex<HashMap<String, OAuthSession>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_session_id(service: LoginService) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = SESSION_SEQ.fetch_add(1, Ordering::SeqCst);
    format!("{}-{}-{}", service.id(), millis, seq)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn app_data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data dir: {}", error))
}

fn views(providers: &[Provider]) -> Vec<ProviderView> {
    let mut views: Vec<ProviderView> = providers.iter().map(Provider::to_view).collect();
    views.sort_by(|a, b| {
        a.ready_account_count
            .cmp(&b.ready_account_count)
            .reverse()
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    views
}

fn candidate_view(candidate: &ImportCandidate, existing: &[Provider]) -> CandidateView {
    let mut new_provider_count = 0;
    let mut account_count = 0;
    let providers = candidate
        .drafts
        .iter()
        .map(|draft| {
            let base_url = super::types::normalize_base_url(&draft.base_url);
            // Same comparison `provider_id` makes: case-insensitive, so a
            // config that spells the host in capitals is not offered as new.
            let already_imported = existing.iter().any(|provider| {
                provider.protocol == draft.protocol
                    && super::types::normalize_base_url(&provider.base_url).to_lowercase()
                        == base_url.to_lowercase()
            });
            if !already_imported {
                new_provider_count += 1;
            }
            account_count += draft.accounts.len();
            CandidateProviderView {
                protocol: draft.protocol,
                base_url: draft.base_url.clone(),
                vendor: draft
                    .vendor
                    .clone()
                    .unwrap_or_else(|| super::registry::infer_vendor(&draft.base_url)),
                display_name: draft.display_name.clone().unwrap_or_else(|| {
                    super::registry::prettify_vendor(&draft.vendor.clone().unwrap_or_default())
                }),
                accounts: draft
                    .accounts
                    .iter()
                    .map(|account| CandidateAccountView {
                        kind: account.kind,
                        label: account.label.clone(),
                        source: account.source.clone(),
                        status: account_status_of(account),
                        masked_secret: account.secret.masked(),
                        headers: account
                            .headers
                            .iter()
                            .map(super::types::mask_header)
                            .collect(),
                    })
                    .collect(),
                models: draft.models.clone(),
                already_imported,
            }
        })
        .collect();

    CandidateView {
        source_id: candidate.source_id.clone(),
        source_name: candidate.source_name.clone(),
        config_path: candidate
            .config_path
            .as_ref()
            .map(|path| path.display().to_string()),
        providers,
        notes: candidate.notes.clone(),
        error: candidate.error.clone(),
        account_count,
        new_provider_count,
    }
}

fn account_status_of(account: &AccountDraft) -> AccountStatus {
    if account.secret.is_present() {
        AccountStatus::Ready
    } else {
        AccountStatus::NeedsKey
    }
}

/// Exposed for the login flows: where a service caches its token.
pub fn oauth_token_path(dir: &Path, service: &str) -> std::path::PathBuf {
    oauth_path(dir, service)
}

/// Exposed for the login flows: atomically writing a 0600 token file.
pub fn write_token_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_private(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oauth_provider(account_id: &str, source: &str) -> Provider {
        Provider {
            id: "openai-1".into(),
            protocol: Protocol::Responses,
            base_url: "https://api.openai.com/v1".into(),
            vendor: "openai".into(),
            display_name: "The Pair plan".into(),
            accounts: vec![crate::accounts::types::Account {
                id: account_id.into(),
                kind: AccountKind::ChatGptOAuth,
                label: "me@example.com".into(),
                source: source.into(),
                status: crate::accounts::types::AccountStatus::Ready,
                headers: vec![],
                tool_names: None,
                secret: Secret::OAuth(Box::new(crate::accounts::types::OAuthToken {
                    access_token: "access".into(),
                    refresh_token: Some("refresh".into()),
                    expires_at: u64::MAX,
                    token_type: "Bearer".into(),
                    scope: None,
                    account_label: None,
                })),
            }],
            models: vec![],
        }
    }

    #[test]
    fn deleting_a_signed_in_account_deletes_its_cached_token() {
        let _guard = crate::test_env::lock_env();
        let dir = std::env::temp_dir().join(format!("the-pair-delete-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        save_providers(&dir, &[oauth_provider("openai-1-a", "oauth:chatgpt")]).unwrap();
        let token = oauth_path(&dir, "chatgpt");
        let other = oauth_path(&dir, "commandcode");
        write_private(&token, b"{\"refreshToken\":\"refresh\"}").unwrap();
        write_private(&other, b"{}").unwrap();

        let remaining = delete_account(&dir, "openai-1", "openai-1-a").unwrap();

        assert!(remaining.is_empty(), "the emptied endpoint goes too");
        assert!(!token.exists(), "the secret outlived the account");
        assert!(other.exists(), "another service's sign-in is not touched");
        // Deleting something that is not there changes nothing and is not an error.
        assert!(delete_account(&dir, "openai-1", "openai-1-a").is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_sign_in_another_account_still_uses_is_kept() {
        let _guard = crate::test_env::lock_env();
        let dir = std::env::temp_dir().join(format!("the-pair-delete-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut provider = oauth_provider("openai-1-a", "oauth:chatgpt");
        let mut second = provider.accounts[0].clone();
        second.id = "openai-1-b".into();
        provider.accounts.push(second);
        let mut keyed = provider.accounts[0].clone();
        keyed.id = "openai-1-c".into();
        keyed.source = "manual".into();
        provider.accounts.push(keyed);
        save_providers(&dir, &[provider]).unwrap();
        let token = oauth_path(&dir, "chatgpt");
        write_private(&token, b"{}").unwrap();

        // A pasted key has no cached sign-in to forget.
        delete_account(&dir, "openai-1", "openai-1-c").unwrap();
        assert!(token.exists());
        delete_account(&dir, "openai-1", "openai-1-a").unwrap();
        assert!(token.exists(), "the other account still signs in with it");
        delete_account(&dir, "openai-1", "openai-1-b").unwrap();
        assert!(!token.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn candidate_view_marks_already_imported_endpoints() {
        let existing = vec![Provider {
            id: "anthropic-1".into(),
            protocol: Protocol::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            vendor: "anthropic".into(),
            display_name: "Anthropic".into(),
            accounts: vec![],
            models: vec![],
        }];
        let candidate = ImportCandidate {
            source_id: "claude".into(),
            source_name: "Claude Code".into(),
            config_path: None,
            drafts: vec![
                ProviderDraft {
                    protocol: Protocol::Anthropic,
                    base_url: "https://API.anthropic.com/".into(),
                    vendor: None,
                    display_name: Some("Claude".into()),
                    accounts: vec![AccountDraft {
                        kind: AccountKind::ApiKey,
                        label: "claude".into(),
                        source: "claude".into(),
                        secret: Secret::ApiKey("sk-ant-1".into()),
                        headers: vec![HeaderPair {
                            name: "Authorization".into(),
                            value: "Bearer sk-ant-1".into(),
                        }],
                        tool_names: None,
                    }],
                    models: vec![],
                },
                ProviderDraft {
                    protocol: Protocol::OpenAiChat,
                    base_url: "https://api.x.ai/v1".into(),
                    vendor: None,
                    display_name: None,
                    accounts: vec![],
                    models: vec![],
                },
            ],
            notes: vec!["note".into()],
            error: None,
        };

        let view = candidate_view(&candidate, &existing);
        assert_eq!(view.providers.len(), 2);
        assert!(
            view.providers[0].already_imported,
            "trailing slash must match"
        );
        assert!(!view.providers[1].already_imported);
        assert_eq!(view.new_provider_count, 1);
        assert_eq!(view.account_count, 1);
        // The key itself must not reach the view.
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("sk-ant-1"), "secret leaked: {}", json);
        assert!(json.contains("maskedSecret"));
        // Nor may a header copied from the source config.
        assert!(
            !json.contains("Bearer sk-ant-1"),
            "header secret leaked: {}",
            json
        );
    }

    #[test]
    fn a_lost_poll_response_does_not_stick_the_session_in_waiting() {
        let outcome: Result<Vec<ProviderView>, String> = Ok(vec![]);
        assert_eq!(status_view(None, "chatgpt", 0).state, "waiting");

        let first = status_view(Some(&outcome), "chatgpt", 500);
        assert_eq!(first.state, "done");
        assert_eq!(first.service, "chatgpt");
        // Polling again — or a retry after a dropped response — must report the
        // same terminal state, not a fresh `waiting`.
        let second = status_view(Some(&outcome), "chatgpt", 900);
        assert_eq!(second.state, "done");
        assert_eq!(second.waiting_ms, 900);

        let failure: Result<Vec<ProviderView>, String> = Err("timed out".into());
        let failed = status_view(Some(&failure), "muse", 1);
        assert_eq!(failed.state, "failed");
        assert_eq!(failed.message.as_deref(), Some("timed out"));
        assert_eq!(status_view(Some(&failure), "muse", 2).state, "failed");
    }

    #[test]
    fn every_sign_in_service_advertises_how_it_authenticates() {
        let services = accounts_login_services();
        assert_eq!(services.len(), 4);
        let chatgpt = services.iter().find(|s| s.id == "chatgpt").unwrap();
        assert!(chatgpt.needs_browser);
        assert!(!chatgpt.needs_key);
        assert_eq!(chatgpt.display_name, "ChatGPT");
        let muse = services.iter().find(|s| s.id == "muse").unwrap();
        assert!(!muse.needs_browser);
        assert!(muse.needs_key);
    }

    #[test]
    fn session_ids_stay_unique_inside_one_millisecond() {
        let first = next_session_id(LoginService::ChatGpt);
        let second = next_session_id(LoginService::ChatGpt);
        assert_ne!(first, second, "two sign-ins must not share a session");
        assert!(first.starts_with("chatgpt-"));
        assert_ne!(next_session_id(LoginService::Muse), first);
    }

    #[test]
    fn views_put_ready_providers_first() {
        let ready = Provider {
            id: "a".into(),
            protocol: Protocol::OpenAiChat,
            base_url: "https://a.example/v1".into(),
            vendor: "a".into(),
            display_name: "Aaa".into(),
            accounts: vec![crate::accounts::types::Account {
                id: "a-1".into(),
                kind: AccountKind::ApiKey,
                label: "l".into(),
                source: "manual".into(),
                status: AccountStatus::Ready,
                headers: vec![],
                tool_names: None,
                secret: Secret::ApiKey("k".into()),
            }],
            models: vec![],
        };
        let empty = Provider {
            id: "b".into(),
            display_name: "Bbb".into(),
            ..ready.clone()
        };
        let ordered = views(&[empty, ready]);
        assert_eq!(ordered[0].display_name, "Aaa");
    }
}
