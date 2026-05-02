use crate::session_snapshot::{ensure_snapshot_dir, read_json, SessionSnapshotRecord, INDEX_FILE_NAME};
use crate::types::{ActivityType, MessageSender, MessageType, RecentActivity};
use std::fs;
use tauri::AppHandle;

fn activity_type_for_message(msg_type: &MessageType) -> ActivityType {
    match msg_type {
        MessageType::Result => ActivityType::Result,
        MessageType::Acceptance => ActivityType::Acceptance,
        MessageType::Handoff => ActivityType::Handoff,
        _ => ActivityType::StatusChange,
    }
}

fn role_string_for_sender(sender: &MessageSender) -> String {
    match sender {
        MessageSender::Mentor => "mentor".to_string(),
        MessageSender::Executor => "executor".to_string(),
        MessageSender::Human => "human".to_string(),
    }
}

fn extract_activities_from_messages(
    pair_id: &str,
    pair_name: &str,
    messages: &[crate::types::Message],
) -> Vec<RecentActivity> {
    messages
        .iter()
        .filter(|msg| {
            matches!(
                msg.msg_type,
                MessageType::Result | MessageType::Acceptance | MessageType::Handoff
            )
        })
        .map(|msg| RecentActivity {
            pair_id: pair_id.to_string(),
            pair_name: pair_name.to_string(),
            activity_type: activity_type_for_message(&msg.msg_type),
            description: msg.content.chars().take(80).collect(),
            timestamp: msg.timestamp,
            role: Some(role_string_for_sender(&msg.from)),
        })
        .collect()
}

#[tauri::command]
pub fn get_recent_activities(app: AppHandle, limit: Option<usize>) -> Result<Vec<RecentActivity>, String> {
    let limit = limit.unwrap_or(10);
    let dir = ensure_snapshot_dir(&app)?;

    let entries =
        fs::read_dir(&dir).map_err(|e| format!("Failed to read snapshot dir: {}", e))?;

    let mut activities: Vec<RecentActivity> = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE_NAME) {
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let snapshot: SessionSnapshotRecord = match read_json(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let pair_id = &snapshot.pair_id;
        let pair_name = &snapshot.name;

        // Extract status change events for terminal states
        if matches!(
            snapshot.status,
            crate::types::PairStatus::Finished
                | crate::types::PairStatus::Paused
                | crate::types::PairStatus::Error
        ) {
            let status_label = match snapshot.status {
                crate::types::PairStatus::Finished => "Finished",
                crate::types::PairStatus::Paused => "Paused",
                crate::types::PairStatus::Error => "Error",
                _ => unreachable!(),
            };

            activities.push(RecentActivity {
                pair_id: pair_id.clone(),
                pair_name: pair_name.clone(),
                activity_type: ActivityType::StatusChange,
                description: format!("Pair {}", status_label),
                timestamp: snapshot
                    .current_run_finished_at
                    .unwrap_or(snapshot.saved_at),
                role: None,
            });
        }

        // Extract meaningful messages from run history
        for run in &snapshot.run_history {
            for msg in &run.messages {
                if matches!(
                    msg.msg_type,
                    MessageType::Result | MessageType::Acceptance | MessageType::Handoff
                ) {
                    activities.push(RecentActivity {
                        pair_id: pair_id.clone(),
                        pair_name: pair_name.clone(),
                        activity_type: activity_type_for_message(&msg.msg_type),
                        description: msg.content.chars().take(80).collect(),
                        timestamp: msg.timestamp,
                        role: Some(role_string_for_sender(&msg.from)),
                    });
                }
            }
        }

        // Fallback to snapshot.messages if no run history
        if snapshot.run_history.is_empty() {
            activities.extend(extract_activities_from_messages(
                pair_id,
                pair_name,
                &snapshot.messages,
            ));
        }
    }

    // Sort by timestamp descending, take limit
    activities.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(activities.into_iter().take(limit).collect())
}
