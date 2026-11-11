//! The model-to-model harness: one turn of one agent, end to end.
//!
//! A turn resolves its model from the credential store, streams the reply, runs
//! the tools the model asks for, and falls back to the pair's backup when the
//! first model cannot finish. [`turn`] is that turn; [`driver`] is the pair loop
//! around it — the same acceptance, plan-gate, iteration-budget and snapshot
//! bookkeeping every transport used to share, now with no process to spawn.
//!
//! Everything a turn needs is resolved here from the credential store, so a
//! caller never handles a secret: it names a [`ModelSelection`] and gets text
//! back.
//!
//! The conversation each agent carries across its turns lives in
//! [`history`]: the caller owns it, stores it with the pair and replays it on
//! the next turn.

pub mod driver;
pub mod history;
pub mod resolve;
pub mod turn;

pub use resolve::{keep_or_resolve, prefer_route, preferred_selection, selection_for_role};
pub use turn::{mark_account_expired, run_turn, ToolEvent, TurnRequest};

use serde::{Deserialize, Serialize};

/// One model an agent talks to, and the account it authenticates with.
///
/// Ids are the registry's own (`accounts::types::provider_id`), so a selection
/// survives a re-scan as long as the endpoint and the account's label are
/// unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    pub provider_id: String,
    pub account_id: String,
    pub model: String,
    /// The route's reasoning effort, where the model takes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Used when this selection cannot complete the turn at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<Box<ModelSelection>>,
}

impl ModelSelection {
    /// A selection with no account cannot talk to anything.
    pub fn is_usable(&self) -> bool {
        !self.provider_id.is_empty() && !self.account_id.is_empty() && !self.model.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_round_trips_through_json() {
        let selection = ModelSelection {
            provider_id: "anthropic-1".into(),
            account_id: "anthropic-1-a".into(),
            model: "claude-sonnet-4-5".into(),
            effort: Some("high".into()),
            backup: Some(Box::new(ModelSelection {
                provider_id: "openai-1".into(),
                account_id: "openai-1-b".into(),
                model: "gpt-5.1".into(),
                effort: None,
                backup: None,
            })),
        };
        let json = serde_json::to_string(&selection).unwrap();
        assert!(json.contains("providerId"), "{}", json);
        let back: ModelSelection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, selection);
        assert!(back.backup.unwrap().backup.is_none());
    }

    #[test]
    fn a_backup_is_optional_in_older_payloads() {
        let selection: ModelSelection =
            serde_json::from_str(r#"{"providerId":"p","accountId":"a","model":"m"}"#).unwrap();
        assert_eq!(selection.effort, None);
        assert!(selection.backup.is_none());
        assert!(selection.is_usable());
    }

    #[test]
    fn an_empty_selection_is_not_usable() {
        let selection = ModelSelection {
            provider_id: String::new(),
            account_id: "a".into(),
            model: "m".into(),
            effort: None,
            backup: None,
        };
        assert!(!selection.is_usable());
    }
}
