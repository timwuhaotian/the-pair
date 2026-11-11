//! Signing in to subscription-style providers from inside the app.
//!
//! ChatGPT and Command Code need a browser round-trip; opencode-go and Muse
//! Code only need a key. Every flow ends in the same [`LoginOutcome`] shape so
//! the caller can upsert it into the registry without knowing the service.

pub mod chatgpt;
pub mod commandcode;
pub mod muse;
pub mod opencode_go;

use crate::accounts::registry::AccountDraft;
use crate::accounts::types::{ModelRoute, Protocol};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginService {
    ChatGpt,
    CommandCode,
    OpencodeGo,
    Muse,
}

impl LoginService {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "chatgpt" => Some(LoginService::ChatGpt),
            "commandcode" => Some(LoginService::CommandCode),
            "opencode-go" => Some(LoginService::OpencodeGo),
            "muse" => Some(LoginService::Muse),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            LoginService::ChatGpt => "chatgpt",
            LoginService::CommandCode => "commandcode",
            LoginService::OpencodeGo => "opencode-go",
            LoginService::Muse => "muse",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            LoginService::ChatGpt => "ChatGPT",
            LoginService::CommandCode => "Command Code",
            LoginService::OpencodeGo => "OpenCode Go",
            LoginService::Muse => "Muse Code",
        }
    }

    /// Services that open a browser and wait for a redirect.
    pub fn needs_browser(self) -> bool {
        matches!(self, LoginService::ChatGpt | LoginService::CommandCode)
    }

    /// Services that take a key pasted into the app. Command Code signs in
    /// through the browser, but its flow ends by offering a pasted key instead,
    /// so the UI has to offer that field for it as well.
    pub fn needs_key(self) -> bool {
        matches!(
            self,
            LoginService::OpencodeGo | LoginService::Muse | LoginService::CommandCode
        )
    }
}

/// What a finished flow wants written into the registry.
#[derive(Debug, Clone, PartialEq)]
pub struct LoginOutcome {
    pub protocol: Protocol,
    pub base_url: String,
    pub vendor: Option<String>,
    pub display_name: Option<String>,
    pub account: AccountDraft,
    pub models: Vec<ModelRoute>,
    pub notes: Vec<String>,
}

/// How long a browser flow waits before giving up.
pub const BROWSER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Runs `service` to completion.
///
/// `api_key` is required for the key-only services and, for Command Code, is
/// what takes the place of its browser round-trip. `app_dir` is the app data
/// dir, where OAuth tokens are cached.
pub async fn launch(
    service: LoginService,
    app_dir: &Path,
    api_key: Option<String>,
) -> Result<LoginOutcome, String> {
    match service {
        LoginService::ChatGpt => chatgpt::login(app_dir).await,
        LoginService::CommandCode => match api_key.filter(|key| !key.trim().is_empty()) {
            // A pasted key skips the browser round-trip entirely.
            Some(key) => commandcode::login_with_key(app_dir, key.trim()).await,
            None => commandcode::login(app_dir).await,
        },
        LoginService::OpencodeGo => {
            let key = api_key
                .filter(|key| !key.trim().is_empty())
                .ok_or_else(|| {
                    "Paste your OpenCode Go API key to finish signing in.".to_string()
                })?;
            opencode_go::login(key.trim())
        }
        LoginService::Muse => {
            let key = api_key
                .filter(|key| !key.trim().is_empty())
                .ok_or_else(|| "Paste your Muse Code API key to finish signing in.".to_string())?;
            muse::login(key.trim())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [LoginService; 4] = [
        LoginService::ChatGpt,
        LoginService::CommandCode,
        LoginService::OpencodeGo,
        LoginService::Muse,
    ];

    #[test]
    fn the_ids_round_trip() {
        for service in ALL {
            assert_eq!(LoginService::from_id(service.id()), Some(service));
        }
        assert_eq!(LoginService::from_id("openai"), None);
        assert_eq!(LoginService::from_id(""), None);
    }

    #[test]
    fn every_service_says_how_it_authenticates() {
        for service in ALL {
            assert!(
                service.needs_browser() || service.needs_key(),
                "{} has no way in",
                service.id()
            );
        }
        assert!(LoginService::ChatGpt.needs_browser());
        assert!(!LoginService::ChatGpt.needs_key());
        assert!(LoginService::CommandCode.needs_browser());
        assert!(!LoginService::OpencodeGo.needs_browser());
        assert!(LoginService::Muse.needs_key());
        assert!(!LoginService::Muse.needs_browser());
    }

    #[test]
    fn command_code_takes_a_pasted_key_as_well() {
        // Its browser flow ends with "Try again, or paste an API key instead",
        // which the UI can only offer when the service says it takes a key.
        assert!(LoginService::CommandCode.needs_key());
        assert!(LoginService::CommandCode.needs_browser());
        assert!(!LoginService::OpencodeGo.needs_browser());
        assert!(LoginService::OpencodeGo.needs_key());
    }
}
