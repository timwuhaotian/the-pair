//! Commands the renderer calls directly on the broker.
//!
//! The model-cache and provider-probe commands that used to live here were the
//! CLI's: they detected installed agents and listed the models their CLIs
//! expose. The registry (`accounts`) replaced them — a pair's models come from
//! `accounts_list`, and its credentials from the `accounts_*` commands.

use crate::message_broker::MessageBroker;
use crate::types::PairState;

#[tauri::command]
pub fn pair_get_messages(
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    pair_id: String,
) -> Result<Vec<crate::types::Message>, String> {
    let broker = broker.lock().unwrap_or_else(|e| e.into_inner());
    let state = broker
        .get_state(&pair_id)
        .ok_or_else(|| format!("No broker state found for pair {}", pair_id))?;
    Ok(state.messages)
}

#[tauri::command]
pub fn pair_get_state(
    broker: tauri::State<'_, std::sync::Mutex<MessageBroker>>,
    pair_id: String,
) -> Result<Option<PairState>, String> {
    let broker = broker.lock().unwrap_or_else(|e| e.into_inner());
    Ok(broker.get_state(&pair_id))
}
