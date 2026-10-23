//! FileSync 后端入口：注册插件、状态与命令

mod commands;
mod compare;
mod csv_export;
mod dup_scan;
mod dup_trash;
mod scanner;
mod state;
mod sync;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state::CompareState::new())
        .invoke_handler(tauri::generate_handler![
            commands::start_compare,
            commands::stop_compare,
            commands::start_dup_scan,
            commands::stop_dup_scan,
            commands::delete_dup_files,
            commands::stop_dup_delete,
            commands::undo_delete_dup,
            commands::purge_trash,
            commands::list_delete_batches,
            commands::open_trash_dir,
            commands::export_dup_csv,
            commands::apply_sync_action,
            commands::stop_sync,
            commands::undo_sync,
            commands::stop_undo,
            commands::export_csv,
            commands::open_in_explorer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
