use crate::session_snapshot::{
    ensure_snapshot_dir, read_json, SessionSnapshotRecord, INDEX_FILE_NAME,
};
use crate::types::{ActivityType, MessageSender, MessageType, RecentActivity};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;
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

/// Activities derived from one snapshot file, cached by (mtime, size) so the
/// dashboard's 10 s poll doesn't re-parse unchanged (possibly large) files.
struct CachedActivities {
    modified: Option<SystemTime>,
    len: u64,
    activities: Vec<RecentActivity>,
}

fn activity_cache() -> &'static Mutex<HashMap<PathBuf, CachedActivities>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedActivities>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn activities_from_snapshot(snapshot: &SessionSnapshotRecord) -> Vec<RecentActivity> {
    let pair_id = &snapshot.pair_id;
    let pair_name = &snapshot.name;
    let mut activities = Vec::new();

    // Extract status change events for terminal states
    let status_label = match snapshot.status {
        crate::types::PairStatus::Finished => Some("Finished"),
        crate::types::PairStatus::Paused => Some("Paused"),
        crate::types::PairStatus::Error => Some("Error"),
        _ => None,
    };
    if let Some(status_label) = status_label {
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
        activities.extend(extract_activities_from_messages(
            pair_id,
            pair_name,
            &run.messages,
        ));
    }

    // Fallback to snapshot.messages if no run history
    if snapshot.run_history.is_empty() {
        activities.extend(extract_activities_from_messages(
            pair_id,
            pair_name,
            &snapshot.messages,
        ));
    }

    activities
}

fn collect_recent_activities(dir: &Path, limit: usize) -> Result<Vec<RecentActivity>, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read snapshot dir: {}", e))?;

    let mut activities: Vec<RecentActivity> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut cache = activity_cache().lock().unwrap_or_else(|e| e.into_inner());

    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE_NAME) {
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().ok();
        let len = metadata.len();
        seen.push(path.clone());

        if let Some(cached) = cache.get(&path) {
            if cached.modified == modified && cached.len == len {
                activities.extend(cached.activities.iter().cloned());
                continue;
            }
        }

        let snapshot: SessionSnapshotRecord = match read_json(&path) {
            Ok(s) => s,
            Err(error) => {
                eprintln!(
                    "[recent_activity] Skipping snapshot {:?} that failed to load: {}",
                    path, error
                );
                cache.remove(&path);
                continue;
            }
        };

        let parsed = activities_from_snapshot(&snapshot);
        activities.extend(parsed.iter().cloned());
        cache.insert(
            path,
            CachedActivities {
                modified,
                len,
                activities: parsed,
            },
        );
    }

    // Forget deleted snapshots.
    cache.retain(|path, _| seen.contains(path));

    // Sort by timestamp descending, take limit
    activities.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(activities.into_iter().take(limit).collect())
}

#[tauri::command]
pub async fn get_recent_activities(
    app: AppHandle,
    limit: Option<usize>,
) -> Result<Vec<RecentActivity>, String> {
    let limit = limit.unwrap_or(10);
    let dir = ensure_snapshot_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || collect_recent_activities(&dir, limit))
        .await
        .map_err(|e| format!("Recent activity task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_recent_activities_skips_bad_files_and_uses_cache() {
        let dir =
            std::env::temp_dir().join(format!("the-pair-activity-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("broken.json"), "{not json").unwrap();
        fs::write(dir.join(INDEX_FILE_NAME), "[]").unwrap();

        let activities = collect_recent_activities(&dir, 10).expect("scan should succeed");
        assert!(activities.is_empty());
        // Second scan hits the (empty) cache path without error.
        assert!(collect_recent_activities(&dir, 10).unwrap().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
