//! Process detection commands

use crate::runtime::inspect_runtime_state;

/// Information about running Codex processes
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodexProcessInfo {
    /// Number of blocking standalone Codex CLI processes
    pub count: usize,
    /// Number of restartable runtimes such as VS Code, extension workers, or Codex app
    pub background_count: usize,
    /// Whether switching is allowed (no standalone CLI processes running)
    pub can_switch: bool,
    /// Process IDs of blocking standalone Codex CLI processes
    pub pids: Vec<u32>,
}

/// Check for running Codex processes
#[tauri::command]
pub async fn check_codex_processes() -> Result<CodexProcessInfo, String> {
    let runtime_state = inspect_runtime_state().map_err(|e| e.to_string())?;
    let count = runtime_state.blocking_cli_pids.len();

    Ok(CodexProcessInfo {
        count,
        background_count: runtime_state.restartable_process_count(),
        can_switch: count == 0,
        pids: runtime_state.blocking_cli_pids,
    })
}
