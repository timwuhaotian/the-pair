mod acceptance;
mod config_paths;
mod file_cache;
mod git_tracker;
mod message_broker;
mod model_catalog;
mod pair_manager;
mod path_env;
mod process_spawner;
mod provider_adapter;
mod provider_registry;
mod report_generator;
mod resource_monitor;
mod session_snapshot;
mod skill_discovery;
mod stubs;
mod types;
mod util;
mod worktree_manager;

use message_broker::MessageBroker;
use pair_manager::PairManager;
use process_spawner::ProcessSpawner;
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
fn git_get_file_diff(directory: String, file_path: String, status: String) -> Result<String, String> {
    git_tracker::GitTracker::get_file_diff(&directory, &file_path, &status)
}

fn setup_menu(app: &AppHandle) -> tauri::Result<()> {
    let app_menu = SubmenuBuilder::new(app, "The Pair")
        .text("check_updates", "Check for Updates...")
        .separator()
        .text("quit", "Quit The Pair")
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

    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &edit_menu])
        .build()?;
    app.set_menu(menu)?;

    app.on_menu_event(move |app_handle, event| match event.id().0.as_str() {
        "check_updates" => {
            log::info!("Menu: Check for updates triggered");
            let _ = app_handle.emit("app:update:check", ());
            log::info!("Emitting app:update:check event");
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            path_env::apply_fallback_path();
            std::thread::spawn(path_env::refresh_path_from_login_shell);
            setup_menu(app.handle())?;

            let broker = app.state::<Mutex<MessageBroker>>();
            let mut broker = broker.lock().unwrap();
            broker.set_app_handle(app.handle().clone());
            Ok(())
        })
        .manage(Mutex::new(PairManager::new()))
        .manage(Mutex::new(MessageBroker::new()))
        .manage(ProcessSpawner::new())
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
            stubs::pair_retry_turn,
            stubs::pair_get_messages,
            stubs::pair_get_state,
            stubs::config_get_models,
            stubs::config_get_cached_models,
            stubs::config_refresh_models,
            stubs::config_get_providers,
            stubs::config_read,
            stubs::config_open_file,
            file_cache::file_list_files,
            file_cache::file_parse_mentions,
            file_cache::file_read_content,
            session_snapshot::session_save_snapshot,
            session_snapshot::load_all_pairs,
            session_snapshot::list_recoverable_sessions,
            session_snapshot::delete_recoverable_session,
            session_snapshot::restore_session,
            skill_discovery::discover_skills,
            app_restart,
            git_get_file_diff
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
