//! Codex Quota Monitor - quota monitoring and multi-account management for Codex

pub mod api;
pub mod app_logging;
pub mod auth;
pub mod commands;
pub mod runtime;
pub mod types;

use app_logging::{clear_logs, get_recent_logs};
use commands::{
    add_account_from_file, cancel_login, check_codex_processes, complete_login, delete_account,
    export_accounts_full_encrypted_file, export_accounts_slim_text, get_active_account_info,
    get_usage, import_accounts_full_encrypted_file, import_accounts_slim_text, list_accounts,
    refresh_all_accounts_usage, rename_account, start_login, switch_account, warmup_account,
    warmup_all_accounts,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(app_logging::AppLogState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            app_logging::info(app.handle(), "app", "Codex Quota Monitor started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Account management
            list_accounts,
            get_active_account_info,
            add_account_from_file,
            switch_account,
            delete_account,
            rename_account,
            export_accounts_slim_text,
            import_accounts_slim_text,
            export_accounts_full_encrypted_file,
            import_accounts_full_encrypted_file,
            // OAuth
            start_login,
            complete_login,
            cancel_login,
            // Usage
            get_usage,
            refresh_all_accounts_usage,
            warmup_account,
            warmup_all_accounts,
            // Process detection
            check_codex_processes,
            // App logs
            get_recent_logs,
            clear_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
