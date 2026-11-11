mod acceptance;
mod accounts;
mod config_paths;
mod context_bridge;
mod file_cache;
mod git_tracker;
mod harness;
mod intelligence_store;
mod llm;
mod login;
mod message_broker;
mod pair_manager;
mod path_env;
mod quality_gate;
mod recent_activity;
mod report_generator;
mod session_snapshot;
mod skill_discovery;
mod smart_pause;
mod sources;
mod stubs;
#[cfg(test)]
mod test_env;
mod tools;
mod types;
mod util;
mod worktree_manager;

use harness::driver::TurnDriver;
use message_broker::MessageBroker;
use pair_manager::PairManager;
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, SubmenuBuilder},
    AppHandle, Emitter, Manager,
};

#[tauri::command]
fn app_restart(app: AppHandle) {
    app.restart();
}

#[tauri::command]
fn show_main_window(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
async fn git_get_file_diff(
    directory: String,
    file_path: String,
    status: String,
) -> Result<String, String> {
    // Off the main thread: diffs (and untracked-file reads) can be slow.
    tauri::async_runtime::spawn_blocking(move || {
        git_tracker::GitTracker::get_file_diff(&directory, &file_path, &status)
    })
    .await
    .map_err(|e| format!("Diff task failed: {}", e))?
}

#[tauri::command]
async fn git_commit_changes(
    directory: String,
    message: String,
) -> Result<git_tracker::CommitOutcome, String> {
    // Off the main thread: add/commit can take a lock and walk the tree.
    tauri::async_runtime::spawn_blocking(move || git_tracker::commit_changes(&directory, &message))
        .await
        .map_err(|e| format!("Commit task failed: {}", e))?
}

#[tauri::command]
async fn git_push_changes(directory: String) -> Result<git_tracker::PushOutcome, String> {
    // Off the main thread: network round-trips must not block the UI.
    tauri::async_runtime::spawn_blocking(move || git_tracker::push_changes(&directory, "origin"))
        .await
        .map_err(|e| format!("Push task failed: {}", e))?
}

fn setup_menu(app: &AppHandle) -> tauri::Result<()> {
    // App menu: standard macOS items plus update check. About/Settings are
    // native; custom ids emit frontend events the renderer subscribes to.
    let app_menu = SubmenuBuilder::new(app, "The Pair")
        .about(None)
        .separator()
        .text("check_updates", "Check for Updates...")
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    // File: pair lifecycle. Shortcuts are handled by the renderer's global
    // key handler so they work identically with and without the menu bar.
    let file_menu = SubmenuBuilder::new(app, "File")
        .text("menu:new-pair", "New Pair...")
        .text("menu:new-task", "New Task...")
        .separator()
        .close_window()
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .separator()
        .select_all()
        .build()?;

    // View: shell navigation + appearance.
    let view_menu = SubmenuBuilder::new(app, "View")
        .text("menu:go-pairs", "Go to Pairs")
        .text("menu:go-activity", "Go to Activity")
        .text("menu:go-models", "Go to Models")
        .text("menu:go-insights", "Go to Insights")
        .separator()
        .text("menu:toggle-sidebar", "Toggle Sidebar")
        .text("menu:toggle-theme", "Toggle Theme")
        .separator()
        .text("menu:palette", "Command Palette...")
        .separator()
        .fullscreen()
        .build()?;

    // Window: standard window management.
    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .text("menu:back", "Back to Pairs")
        .build()?;

    // Help: shortcuts reference + updates.
    let help_menu = SubmenuBuilder::new(app, "Help")
        .text("menu:shortcuts", "Keyboard Shortcuts")
        .text("check_updates", "Check for Updates...")
        .build()?;

    let menu = MenuBuilder::new(app)
        .items(&[
            &app_menu,
            &file_menu,
            &edit_menu,
            &view_menu,
            &window_menu,
            &help_menu,
        ])
        .build()?;
    app.set_menu(menu)?;

    app.on_menu_event(move |app_handle, event| match event.id().0.as_str() {
        "check_updates" => {
            log::info!("Menu: Check for updates triggered");
            let _ = app_handle.emit("app:update:check", ());
            log::info!("Emitting app:update:check event");
        }
        // Custom ids re-dispatch as frontend events; App.tsx subscribes and
        // routes them through the same handlers as the palette + shortcuts.
        "menu:new-pair" => {
            let _ = app_handle.emit("app:menu:new-pair", ());
        }
        "menu:new-task" => {
            let _ = app_handle.emit("app:menu:new-task", ());
        }
        "menu:pause" => {
            let _ = app_handle.emit("app:menu:pause", ());
        }
        "menu:resume" => {
            let _ = app_handle.emit("app:menu:resume", ());
        }
        "menu:back" => {
            let _ = app_handle.emit("app:menu:back", ());
        }
        "menu:toggle-sidebar" => {
            let _ = app_handle.emit("app:menu:toggle-sidebar", ());
        }
        "menu:toggle-theme" => {
            let _ = app_handle.emit("app:menu:toggle-theme", ());
        }
        "menu:go-pairs" => {
            let _ = app_handle.emit("app:menu:go-pairs", ());
        }
        "menu:go-activity" => {
            let _ = app_handle.emit("app:menu:go-activity", ());
        }
        "menu:go-models" => {
            let _ = app_handle.emit("app:menu:go-models", ());
        }
        "menu:go-insights" => {
            let _ = app_handle.emit("app:menu:go-insights", ());
        }
        "menu:shortcuts" => {
            let _ = app_handle.emit("app:menu:shortcuts", ());
        }
        "menu:palette" => {
            let _ = app_handle.emit("app:menu:palette", ());
        }
        "quit" => {
            app_handle.exit(0);
        }
        _ => {}
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Resolve PATH before anything else runs: the first model detection must
    // see the login-shell PATH (CLIs only on it were reported missing and
    // cached that way), and changing the environment is only sound while this
    // is still the only thread. The login-shell capture is bounded.
    path_env::apply_fallback_path();
    // A launchd-started app inherits no profile, so the API keys a user exports
    // in `.zshrc` are absent from the process. Env-backed accounts resolve their
    // key at turn time, so the whole login environment has to be here too. That
    // capture carries PATH with it, so the shell — and the user's whole profile
    // — runs once before the window appears, not twice. Only when it brought no
    // PATH back is the shell asked for PATH alone.
    match sources::env::load_login_shell_env() {
        Some(shell_path) => path_env::apply_login_shell_path(&shell_path),
        None => path_env::refresh_path_from_login_shell(),
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            setup_menu(app.handle())?;

            let broker = app.state::<Mutex<MessageBroker>>();
            let mut broker = broker.lock().unwrap_or_else(|e| e.into_inner());
            broker.set_app_handle(app.handle().clone());

            // One-time backfill of historical run outcomes into the
            // intelligence store. Runs off the main thread and only inserts
            // when the DB is empty; failures degrade silently.
            let backfill_handle = app.handle().clone();
            std::thread::spawn(move || intelligence_store::backfill_if_empty(&backfill_handle));

            Ok(())
        })
        .manage(Mutex::new(PairManager::new()))
        .manage(Mutex::new(MessageBroker::new()))
        .manage(TurnDriver::new())
        .invoke_handler(tauri::generate_handler![
            pair_manager::pair_create,
            pair_manager::pair_list,
            pair_manager::pair_delete,
            pair_manager::pair_pause,
            pair_manager::pair_resume,
            pair_manager::pair_assign_task,
            pair_manager::pair_update_models,
            pair_manager::repo_check_state,
            pair_manager::repo_list_branches,
            pair_manager::kill_process,
            pair_manager::pair_retry_turn,
            accounts::commands::accounts_login_services,
            accounts::commands::accounts_list,
            accounts::commands::accounts_scan,
            accounts::commands::accounts_commit_import,
            accounts::commands::accounts_add_provider,
            accounts::commands::accounts_set_account_key,
            accounts::commands::accounts_delete_account,
            accounts::commands::accounts_prefer_route,
            accounts::commands::accounts_launch_oauth,
            accounts::commands::accounts_oauth_status,
            accounts::commands::accounts_cancel_oauth,
            stubs::pair_get_messages,
            stubs::pair_get_state,
            file_cache::file_list_files,
            file_cache::file_parse_mentions,
            file_cache::file_read_content,
            session_snapshot::session_save_snapshot,
            session_snapshot::load_all_pairs,
            session_snapshot::list_recoverable_sessions,
            session_snapshot::delete_recoverable_session,
            session_snapshot::restore_session,
            recent_activity::get_recent_activities,
            intelligence_store::get_insights_summary,
            intelligence_store::get_recommendation,
            intelligence_store::record_intervention,
            skill_discovery::discover_skills,
            skill_discovery::skill_read_content,
            skill_discovery::skill_refresh,
            app_restart,
            show_main_window,
            git_get_file_diff,
            git_commit_changes,
            git_push_changes
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
