use chrono::Utc;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

pub const APP_LOG_EVENT: &str = "app-log";
const MAX_LOG_ENTRIES: usize = 250;

#[derive(Debug, Clone, serde::Serialize)]
pub struct AppLogEntry {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub scope: String,
    pub message: String,
}

pub struct AppLogState {
    next_id: AtomicU64,
    entries: Mutex<VecDeque<AppLogEntry>>,
}

impl Default for AppLogState {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(0),
            entries: Mutex::new(VecDeque::with_capacity(MAX_LOG_ENTRIES)),
        }
    }
}

impl AppLogState {
    fn push(&self, level: &str, scope: &str, message: String) -> AppLogEntry {
        let entry = AppLogEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed) + 1,
            timestamp_ms: Utc::now().timestamp_millis(),
            level: level.to_string(),
            scope: scope.to_string(),
            message,
        };

        let mut entries = self.entries.lock().unwrap_or_else(|poison| poison.into_inner());
        entries.push_back(entry.clone());
        while entries.len() > MAX_LOG_ENTRIES {
            let _ = entries.pop_front();
        }

        entry
    }

    fn recent(&self) -> Vec<AppLogEntry> {
        self.entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clear();
    }
}

fn emit(app: &AppHandle, level: &str, scope: &str, message: String) {
    let state: State<'_, AppLogState> = app.state();
    let entry = state.push(level, scope, message);
    eprintln!(
        "[{}] [{}] {}",
        entry.level.to_uppercase(),
        entry.scope,
        entry.message
    );
    let _ = app.emit(APP_LOG_EVENT, &entry);
}

pub fn info(app: &AppHandle, scope: &str, message: impl Into<String>) {
    emit(app, "info", scope, message.into());
}

pub fn warn(app: &AppHandle, scope: &str, message: impl Into<String>) {
    emit(app, "warn", scope, message.into());
}

pub fn error(app: &AppHandle, scope: &str, message: impl Into<String>) {
    emit(app, "error", scope, message.into());
}

#[tauri::command]
pub async fn get_recent_logs(state: State<'_, AppLogState>) -> Result<Vec<AppLogEntry>, String> {
    Ok(state.recent())
}

#[tauri::command]
pub async fn clear_logs(state: State<'_, AppLogState>) -> Result<(), String> {
    state.clear();
    Ok(())
}
