//! Process detection commands

use crate::runtime::inspect_runtime_state;

/// Information about running Codex processes
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodexProcessInfo {
    /// Number of blocking standalone Codex CLI processes
    pub count: usize,
    /// Number of restartable runtimes such as VS Code, Antigravity, extension workers, or Codex app
    pub background_count: usize,
    /// Whether switching is allowed (no standalone CLI processes running)
    pub can_switch: bool,
    /// Process IDs of blocking standalone Codex CLI processes
    pub pids: Vec<u32>,
    /// Number of VS Code windows currently open
    pub vscode_window_count: usize,
    /// Number of VS Code Codex extension workers currently running
    pub vscode_extension_count: usize,
    /// Number of Antigravity windows currently open
    pub antigravity_window_count: usize,
    /// Number of Antigravity Codex extension workers currently running
    pub antigravity_extension_count: usize,
    /// Number of Codex app processes currently running
    pub codex_app_count: usize,
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
        vscode_window_count: runtime_state.vscode_window_count,
        vscode_extension_count: runtime_state.vscode_extension_pids.len(),
        antigravity_window_count: runtime_state.antigravity_window_count,
        antigravity_extension_count: runtime_state.antigravity_extension_pids.len(),
        codex_app_count: runtime_state.codex_app_pids.len(),
    })
}
