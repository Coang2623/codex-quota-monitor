//! Account management Tauri commands

use crate::auth::{
    add_account, create_chatgpt_account_from_refresh_token, get_active_account,
    import_from_auth_json, load_accounts, remove_account, save_accounts, set_active_account,
    switch_to_account, touch_account,
};
use crate::types::{AccountInfo, AccountsStore, AuthData, ImportAccountsSummary, StoredAccount};

use anyhow::Context;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use futures::{stream, StreamExt};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
#[allow(dead_code)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const SLIM_EXPORT_PREFIX: &str = "cqm1.";
const LEGACY_SLIM_EXPORT_PREFIX: &str = "css1.";
const SLIM_FORMAT_VERSION: u8 = 1;
const SLIM_AUTH_API_KEY: u8 = 0;
const SLIM_AUTH_CHATGPT: u8 = 1;

const FULL_FILE_MAGIC: &[u8; 4] = b"CQMF";
const LEGACY_FULL_FILE_MAGIC: &[u8; 4] = b"CSWF";
const FULL_FILE_VERSION: u8 = 1;
const FULL_SALT_LEN: usize = 16;
const FULL_NONCE_LEN: usize = 24;
const FULL_KDF_ITERATIONS: u32 = 210_000;
const FULL_PRESET_PASSPHRASE: &str = "gT7kQ9mV2xN4pL8sR1dH6zW3cB5yF0uJ_aE7nK2tP9vM4rX1";

const MAX_IMPORT_JSON_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMPORT_FILE_BYTES: u64 = 8 * 1024 * 1024;
const SLIM_IMPORT_CONCURRENCY: usize = 6;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SlimPayload {
    #[serde(rename = "v")]
    version: u8,
    #[serde(rename = "a", skip_serializing_if = "Option::is_none")]
    active_name: Option<String>,
    #[serde(rename = "c")]
    accounts: Vec<SlimAccountPayload>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SlimAccountPayload {
    #[serde(rename = "n")]
    name: String,
    #[serde(rename = "t")]
    auth_type: u8,
    #[serde(rename = "k", skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SwitchAccountResult {
    pub closed_extension_processes: usize,
    pub closed_vscode_windows: usize,
    pub restarted_vscode: bool,
    pub closed_antigravity_windows: usize,
    pub restarted_antigravity: bool,
    pub closed_codex_apps: usize,
    pub restarted_codex_app: bool,
}

/// List all accounts with their info
#[tauri::command]
pub async fn list_accounts() -> Result<Vec<AccountInfo>, String> {
    let store = load_accounts().map_err(|e| e.to_string())?;
    let active_id = store.active_account_id.as_deref();

    let accounts: Vec<AccountInfo> = store
        .accounts
        .iter()
        .map(|a| AccountInfo::from_stored(a, active_id))
        .collect();

    Ok(accounts)
}

/// Get the currently active account
#[tauri::command]
pub async fn get_active_account_info() -> Result<Option<AccountInfo>, String> {
    let store = load_accounts().map_err(|e| e.to_string())?;
    let active_id = store.active_account_id.as_deref();

    if let Some(active) = get_active_account().map_err(|e| e.to_string())? {
        Ok(Some(AccountInfo::from_stored(&active, active_id)))
    } else {
        Ok(None)
    }
}

/// Add an account from an auth.json file
#[tauri::command]
pub async fn add_account_from_file(path: String, name: String) -> Result<AccountInfo, String> {
    // Import from the file
    let account = import_from_auth_json(&path, name).map_err(|e| e.to_string())?;

    // Add to storage
    let stored = add_account(account).map_err(|e| e.to_string())?;

    let store = load_accounts().map_err(|e| e.to_string())?;
    let active_id = store.active_account_id.as_deref();

    Ok(AccountInfo::from_stored(&stored, active_id))
}

/// Switch to a different account
#[tauri::command]
pub async fn switch_account(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<SwitchAccountResult, String> {
    let store = load_accounts().map_err(|e| e.to_string())?;

    // Find the account
    let account = store
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| format!("Account not found: {account_id}"))?;

    crate::app_logging::info(
        &app,
        "switch",
        format!("Switch requested for account `{}`", account.name),
    );

    let runtime_state = crate::runtime::inspect_runtime_state().map_err(|e| e.to_string())?;
    crate::app_logging::info(
        &app,
        "switch",
        format!(
            "Runtime snapshot: blockers={}, extension_workers={}, vscode_extension_workers={}, antigravity_extension_workers={}, vscode_windows={}, antigravity_windows={}, codex_apps={}",
            runtime_state.blocking_cli_pids.len(),
            runtime_state.extension_pids.len(),
            runtime_state.vscode_extension_pids.len(),
            runtime_state.antigravity_extension_pids.len(),
            runtime_state.vscode_pids.len(),
            runtime_state.antigravity_pids.len(),
            runtime_state.codex_app_pids.len(),
        ),
    );
    if !runtime_state.blocking_cli_pids.is_empty() {
        let count = runtime_state.blocking_cli_pids.len();
        crate::app_logging::warn(
            &app,
            "switch",
            format!("Switch blocked by {count} standalone Codex CLI process(es)"),
        );
        return Err(format!(
            "Cannot switch while {count} standalone Codex CLI process{} still running. Close the CLI first, then try again.",
            if count == 1 { "" } else { "es" }
        ));
    }

    // Write to ~/.codex/auth.json
    switch_to_account(account).map_err(|e| e.to_string())?;
    crate::app_logging::info(&app, "switch", "Updated ~/.codex/auth.json");

    // Update the active account in our store
    set_active_account(&account_id).map_err(|e| e.to_string())?;

    // Update last_used_at
    touch_account(&account_id).map_err(|e| e.to_string())?;
    crate::app_logging::info(
        &app,
        "switch",
        format!("Active account set to `{}`", account.name),
    );

    let should_restart_vscode =
        !runtime_state.vscode_pids.is_empty() && !runtime_state.vscode_extension_pids.is_empty();
    let should_restart_antigravity = !runtime_state.antigravity_pids.is_empty()
        && !runtime_state.antigravity_extension_pids.is_empty();
    let mut extension_pids_to_terminate = Vec::new();
    if !should_restart_vscode {
        extension_pids_to_terminate.extend(runtime_state.vscode_extension_pids.iter().copied());
    }
    if !should_restart_antigravity {
        extension_pids_to_terminate.extend(runtime_state.antigravity_extension_pids.iter().copied());
    }
    extension_pids_to_terminate.sort_unstable();
    extension_pids_to_terminate.dedup();
    let closed_extension_processes =
        crate::runtime::terminate_pids(&extension_pids_to_terminate);
    let closed_vscode_windows = if should_restart_vscode {
        crate::runtime::terminate_pids(&runtime_state.vscode_pids)
    } else {
        0
    };
    let closed_antigravity_windows = if should_restart_antigravity {
        crate::runtime::terminate_pids(&runtime_state.antigravity_pids)
    } else {
        0
    };
    let closed_codex_apps = crate::runtime::terminate_pids(&runtime_state.codex_app_pids);
    crate::app_logging::info(
        &app,
        "switch",
        format!(
            "Closed runtimes: extension_workers={}, vscode_windows={}, antigravity_windows={}, codex_apps={}",
            closed_extension_processes,
            closed_vscode_windows,
            closed_antigravity_windows,
            closed_codex_apps
        ),
    );
    if should_restart_vscode {
        crate::app_logging::info(
            &app,
            "switch",
            "VS Code was closed so it can reopen cleanly and pick up the new auth state",
        );
    }
    if should_restart_antigravity {
        crate::app_logging::info(
            &app,
            "switch",
            "Antigravity was closed so it can reopen cleanly and pick up the new auth state",
        );
    }
    let mut vscode_shutdown_pids = runtime_state.vscode_pids.clone();
    vscode_shutdown_pids.extend(runtime_state.vscode_extension_pids.iter().copied());
    let mut antigravity_shutdown_pids = runtime_state.antigravity_pids.clone();
    antigravity_shutdown_pids.extend(runtime_state.antigravity_extension_pids.iter().copied());
    let restarted_vscode = should_restart_vscode;
    let restarted_antigravity = should_restart_antigravity;
    let restarted_codex_app = closed_codex_apps > 0;

    if should_restart_vscode || should_restart_antigravity || closed_codex_apps > 0 {
        crate::app_logging::info(
            &app,
            "switch",
            "Runtime restart was scheduled in the background so the UI can update immediately",
        );

        if should_restart_vscode {
            let app_for_vscode_restart = app.clone();
            let vscode_launch_path = runtime_state.vscode_launch_path.clone();
            let vscode_shutdown_pids_for_restart = vscode_shutdown_pids.clone();

            std::thread::spawn(move || {
                crate::runtime::wait_for_pids_to_exit(
                    &vscode_shutdown_pids_for_restart,
                    Duration::from_secs(5),
                );
                let launch_target = vscode_launch_path.as_deref().unwrap_or("Code.exe");
                let restarted = crate::runtime::relaunch_vscode(
                    vscode_launch_path.as_deref(),
                    &vscode_shutdown_pids_for_restart,
                );

                if restarted {
                    crate::app_logging::info(
                        &app_for_vscode_restart,
                        "switch",
                        format!("VS Code reopened using `{launch_target}`"),
                    );
                } else {
                    crate::app_logging::warn(
                        &app_for_vscode_restart,
                        "switch",
                        format!("VS Code reopen failed for `{launch_target}`"),
                    );
                }
            });
        }

        if should_restart_antigravity {
            let app_for_antigravity_restart = app.clone();
            let antigravity_launch_path = runtime_state.antigravity_launch_path.clone();
            let antigravity_shutdown_pids_for_restart = antigravity_shutdown_pids.clone();

            std::thread::spawn(move || {
                crate::runtime::wait_for_pids_to_exit(
                    &antigravity_shutdown_pids_for_restart,
                    Duration::from_secs(5),
                );
                let launch_target = antigravity_launch_path
                    .as_deref()
                    .unwrap_or("Antigravity.exe");
                let restarted = crate::runtime::relaunch_antigravity(
                    antigravity_launch_path.as_deref(),
                    &antigravity_shutdown_pids_for_restart,
                );

                if restarted {
                    crate::app_logging::info(
                        &app_for_antigravity_restart,
                        "switch",
                        format!("Antigravity reopened using `{launch_target}`"),
                    );
                } else {
                    crate::app_logging::warn(
                        &app_for_antigravity_restart,
                        "switch",
                        format!("Antigravity reopen failed for `{launch_target}`"),
                    );
                }
            });
        }

        if closed_codex_apps > 0 {
            let app_for_codex_restart = app.clone();
            let codex_app_launch_path = runtime_state.codex_app_launch_path.clone();
            let codex_app_pids = runtime_state.codex_app_pids.clone();

            std::thread::spawn(move || {
                crate::runtime::wait_for_pids_to_exit(&codex_app_pids, Duration::from_secs(8));
                let lingering_codex_app_pids =
                    crate::runtime::current_codex_app_pids().unwrap_or_default();

                if !lingering_codex_app_pids.is_empty() {
                    let additionally_closed =
                        crate::runtime::terminate_pids(&lingering_codex_app_pids);
                    crate::app_logging::warn(
                        &app_for_codex_restart,
                        "switch",
                        format!(
                            "Codex app still had {} lingering process(es); forced shutdown closed {} more",
                            lingering_codex_app_pids.len(),
                            additionally_closed
                        ),
                    );
                    crate::runtime::wait_for_pids_to_exit(
                        &lingering_codex_app_pids,
                        Duration::from_secs(4),
                    );
                }

                let remaining_codex_app_pids =
                    crate::runtime::current_codex_app_pids().unwrap_or_default();
                if !remaining_codex_app_pids.is_empty() {
                    crate::app_logging::warn(
                        &app_for_codex_restart,
                        "switch",
                        format!(
                            "Codex app still reports {} running process(es) before relaunch; waiting for the runtime lock to clear",
                            remaining_codex_app_pids.len()
                        ),
                    );
                }

                std::thread::sleep(Duration::from_millis(1500));
                let launch_target = codex_app_launch_path
                    .as_deref()
                    .unwrap_or("(missing path)");
                let restarted = crate::runtime::relaunch_codex_app(
                    codex_app_launch_path.as_deref(),
                    &remaining_codex_app_pids,
                );

                if restarted {
                    crate::app_logging::info(
                        &app_for_codex_restart,
                        "switch",
                        format!("Codex app reopened using `{launch_target}`"),
                    );
                } else {
                    crate::app_logging::warn(
                        &app_for_codex_restart,
                        "switch",
                        format!("Codex app reopen failed for `{launch_target}`"),
                    );
                }
            });
        }
    }

    Ok(SwitchAccountResult {
        closed_extension_processes,
        closed_vscode_windows,
        restarted_vscode,
        closed_antigravity_windows,
        restarted_antigravity,
        closed_codex_apps,
        restarted_codex_app,
    })
}

// is not enough — we need a full window reload.

/// Remove an account
#[tauri::command]
pub async fn delete_account(account_id: String) -> Result<(), String> {
    remove_account(&account_id).map_err(|e| e.to_string())?;
    Ok(())
}

/// Rename an account
#[tauri::command]
pub async fn rename_account(account_id: String, new_name: String) -> Result<(), String> {
    crate::auth::storage::update_account_metadata(&account_id, Some(new_name), None, None)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Export minimal account config as a compact text string.
/// For ChatGPT accounts, only refresh token is exported.
#[tauri::command]
pub async fn export_accounts_slim_text() -> Result<String, String> {
    let store = load_accounts().map_err(|e| e.to_string())?;
    encode_slim_payload_from_store(&store).map_err(|e| e.to_string())
}

/// Import minimal account config from a compact text string, skipping existing accounts.
#[tauri::command]
pub async fn import_accounts_slim_text(payload: String) -> Result<ImportAccountsSummary, String> {
    let slim_payload = decode_slim_payload(&payload).map_err(|e| format!("{e:#}"))?;
    let total_in_payload = slim_payload.accounts.len();

    let current = load_accounts().map_err(|e| e.to_string())?;
    let existing_names: HashSet<String> = current.accounts.iter().map(|a| a.name.clone()).collect();

    let imported = build_store_from_slim_payload(slim_payload, &existing_names)
        .await
        .map_err(|e| {
            format!(
                "{e:#}\nHint: Slim import needs network access to refresh ChatGPT tokens. You can use Full encrypted file import when offline."
            )
        })?;
    validate_imported_store(&imported).map_err(|e| format!("{e:#}"))?;

    let (merged, summary) = merge_accounts_store(current, imported);
    save_accounts(&merged).map_err(|e| e.to_string())?;
    Ok(ImportAccountsSummary {
        total_in_payload,
        imported_count: summary.imported_count,
        skipped_count: total_in_payload.saturating_sub(summary.imported_count),
    })
}

/// Export full account config as an encrypted file.
#[tauri::command]
pub async fn export_accounts_full_encrypted_file(path: String) -> Result<(), String> {
    let store = load_accounts().map_err(|e| e.to_string())?;
    let encrypted =
        encode_full_encrypted_store(&store, FULL_PRESET_PASSPHRASE).map_err(|e| e.to_string())?;
    write_encrypted_file(&path, &encrypted).map_err(|e| e.to_string())?;
    Ok(())
}

/// Import full account config from an encrypted file, skipping existing accounts.
#[tauri::command]
pub async fn import_accounts_full_encrypted_file(
    path: String,
) -> Result<ImportAccountsSummary, String> {
    let encrypted = read_encrypted_file(&path).map_err(|e| e.to_string())?;
    let imported = decode_full_encrypted_store(&encrypted, FULL_PRESET_PASSPHRASE)
        .map_err(|e| e.to_string())?;
    validate_imported_store(&imported).map_err(|e| e.to_string())?;

    let current = load_accounts().map_err(|e| e.to_string())?;
    let (merged, summary) = merge_accounts_store(current, imported);
    save_accounts(&merged).map_err(|e| e.to_string())?;
    Ok(summary)
}


/// Reload all open VSCode windows so extensions (including Codex) restart
/// and read the updated `~/.codex/auth.json`.
///
/// Uses `code --open-url vscode://command/workbench.action.reloadWindow`
/// which reliably triggers a window reload across platforms.
/// Falls back to OS-native URI handlers if `code` CLI is not found.
#[allow(dead_code)]
fn reload_vscode_windows() {
    let reload_uri = "vscode://command/workbench.action.reloadWindow";

    #[cfg(unix)]
    {
        // Try `code --open-url` first (works when VS Code is in PATH)
        let code_result = std::process::Command::new("code")
            .args(["--open-url", reload_uri])
            .output();

        if code_result.is_err() || !code_result.as_ref().unwrap().status.success() {
            // Fallback to OS URI handler
            #[cfg(target_os = "macos")]
            let cmd = "open";
            #[cfg(not(target_os = "macos"))]
            let cmd = "xdg-open";

            let _ = std::process::Command::new(cmd).arg(reload_uri).output();
        }
    }

    #[cfg(windows)]
    {
        // Try `code --open-url` first — this is the most reliable method on Windows.
        // Look for `code.cmd` in common locations.
        let code_cmd = find_vscode_cli();
        let mut reloaded = false;

        if let Some(code_path) = code_cmd {
            if let Ok(output) = std::process::Command::new(&code_path)
                .creation_flags(CREATE_NO_WINDOW)
                .args(["--open-url", reload_uri])
                .output()
            {
                reloaded = output.status.success();
            }
        }

        if !reloaded {
            // Fallback: use cmd.exe start to open the URI via Windows protocol handler
            let _ = std::process::Command::new("cmd")
                .creation_flags(CREATE_NO_WINDOW)
                .args(["/C", "start", "", reload_uri])
                .output();
        }
    }
}

/// Find the VS Code CLI (`code` / `code.cmd`) on the system.
#[cfg(windows)]
#[allow(dead_code)]
fn find_vscode_cli() -> Option<String> {
    // Check if `code` is directly in PATH
    if let Ok(output) = std::process::Command::new("where.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .arg("code.cmd")
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = stdout.lines().next() {
                let path = first_line.trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
    }

    // Check well-known install locations
    let candidates = [
        r"D:\Apps\Microsoft VS Code\bin\code.cmd",
        r"C:\Program Files\Microsoft VS Code\bin\code.cmd",
        r"C:\Users\nguye\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd",
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    None
}

fn encode_slim_payload_from_store(store: &AccountsStore) -> anyhow::Result<String> {
    let active_name = store.active_account_id.as_ref().and_then(|active_id| {
        store
            .accounts
            .iter()
            .find(|account| account.id == *active_id)
            .map(|account| account.name.clone())
    });

    let slim_accounts = store
        .accounts
        .iter()
        .map(|account| match &account.auth_data {
            AuthData::ApiKey { key } => SlimAccountPayload {
                name: account.name.clone(),
                auth_type: SLIM_AUTH_API_KEY,
                api_key: Some(key.clone()),
                refresh_token: None,
            },
            AuthData::ChatGPT { refresh_token, .. } => SlimAccountPayload {
                name: account.name.clone(),
                auth_type: SLIM_AUTH_CHATGPT,
                api_key: None,
                refresh_token: Some(refresh_token.clone()),
            },
        })
        .collect();

    let payload = SlimPayload {
        version: SLIM_FORMAT_VERSION,
        active_name,
        accounts: slim_accounts,
    };

    let json = serde_json::to_vec(&payload).context("Failed to serialize slim payload")?;
    let compressed = compress_bytes(&json).context("Failed to compress slim payload")?;

    Ok(format!(
        "{SLIM_EXPORT_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(compressed)
    ))
}

fn decode_slim_payload(payload: &str) -> anyhow::Result<SlimPayload> {
    let normalized: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    if normalized.is_empty() {
        anyhow::bail!("Import string is empty");
    }

    let encoded = normalized
        .strip_prefix(SLIM_EXPORT_PREFIX)
        .or_else(|| normalized.strip_prefix(LEGACY_SLIM_EXPORT_PREFIX))
        .unwrap_or(&normalized);

    let compressed = URL_SAFE_NO_PAD
        .decode(encoded)
        .context("Invalid slim import string (base64 decode failed)")?;

    let decompressed = decompress_bytes_with_limit(&compressed, MAX_IMPORT_JSON_BYTES)
        .context("Invalid slim import string (decompression failed)")?;

    let parsed: SlimPayload = serde_json::from_slice(&decompressed)
        .context("Invalid slim import string (JSON parse failed)")?;

    validate_slim_payload(&parsed)?;
    Ok(parsed)
}

fn validate_slim_payload(payload: &SlimPayload) -> anyhow::Result<()> {
    if payload.version != SLIM_FORMAT_VERSION {
        anyhow::bail!("Unsupported slim payload version: {}", payload.version);
    }

    let mut names = HashSet::new();

    for account in &payload.accounts {
        if account.name.trim().is_empty() {
            anyhow::bail!("Slim import contains an account with empty name");
        }

        if !names.insert(account.name.clone()) {
            anyhow::bail!(
                "Slim import contains duplicate account name: {}",
                account.name
            );
        }

        match account.auth_type {
            SLIM_AUTH_API_KEY => {
                if account
                    .api_key
                    .as_ref()
                    .map_or(true, |key| key.trim().is_empty())
                {
                    anyhow::bail!("API key is missing for account {}", account.name);
                }
            }
            SLIM_AUTH_CHATGPT => {
                if account
                    .refresh_token
                    .as_ref()
                    .map_or(true, |token| token.trim().is_empty())
                {
                    anyhow::bail!("Refresh token is missing for account {}", account.name);
                }
            }
            _ => {
                anyhow::bail!(
                    "Unsupported auth type {} for account {}",
                    account.auth_type,
                    account.name
                );
            }
        }
    }

    if let Some(active_name) = &payload.active_name {
        if !names.contains(active_name) {
            anyhow::bail!("Slim import references missing active account: {active_name}");
        }
    }

    Ok(())
}

async fn build_store_from_slim_payload(
    payload: SlimPayload,
    existing_names: &HashSet<String>,
) -> anyhow::Result<AccountsStore> {
    let active_name = payload.active_name;
    let import_candidates: Vec<SlimAccountPayload> = payload
        .accounts
        .into_iter()
        .filter(|entry| !existing_names.contains(&entry.name))
        .collect();

    let accounts = restore_slim_accounts(import_candidates).await?;
    let mut active_account_id = None;

    if let Some(active) = active_name {
        active_account_id = accounts
            .iter()
            .find(|account| account.name == active)
            .map(|account| account.id.clone());
    }

    if active_account_id.is_none() {
        active_account_id = accounts.first().map(|a| a.id.clone());
    }

    Ok(AccountsStore {
        version: 1,
        accounts,
        active_account_id,
    })
}

async fn restore_slim_accounts(
    entries: Vec<SlimAccountPayload>,
) -> anyhow::Result<Vec<StoredAccount>> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let mut restored = Vec::with_capacity(entries.len());
    let mut tasks = stream::iter(entries.into_iter().map(|entry| async move {
        let account_name = entry.name;
        let account = match entry.auth_type {
            SLIM_AUTH_API_KEY => StoredAccount::new_api_key(
                account_name.clone(),
                entry.api_key.context("API key payload is missing")?,
            ),
            SLIM_AUTH_CHATGPT => {
                let refresh_token = entry
                    .refresh_token
                    .context("Refresh token payload is missing")?;
                create_chatgpt_account_from_refresh_token(account_name.clone(), refresh_token)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to restore ChatGPT account `{account_name}` from refresh token"
                        )
                    })?
            }
            _ => anyhow::bail!("Unsupported auth type in slim payload"),
        };
        Ok::<StoredAccount, anyhow::Error>(account)
    }))
    .buffered(SLIM_IMPORT_CONCURRENCY);

    while let Some(account_result) = tasks.next().await {
        restored.push(account_result?);
    }

    Ok(restored)
}

fn encode_full_encrypted_store(store: &AccountsStore, passphrase: &str) -> anyhow::Result<Vec<u8>> {
    let json = serde_json::to_vec(store).context("Failed to serialize account store")?;
    let compressed = compress_bytes(&json).context("Failed to compress account store")?;

    let mut salt = [0u8; FULL_SALT_LEN];
    rand::rng().fill_bytes(&mut salt);

    let mut nonce = [0u8; FULL_NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);

    let key = derive_encryption_key(passphrase, &salt);
    let cipher = XChaCha20Poly1305::new((&key).into());
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), compressed.as_slice())
        .map_err(|_| anyhow::anyhow!("Failed to encrypt account store"))?;

    let mut out = Vec::with_capacity(4 + 1 + FULL_SALT_LEN + FULL_NONCE_LEN + ciphertext.len());
    out.extend_from_slice(FULL_FILE_MAGIC);
    out.push(FULL_FILE_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);

    Ok(out)
}

fn decode_full_encrypted_store(
    file_bytes: &[u8],
    passphrase: &str,
) -> anyhow::Result<AccountsStore> {
    if file_bytes.len() as u64 > MAX_IMPORT_FILE_BYTES {
        anyhow::bail!("Encrypted file is too large");
    }

    let header_len = 4 + 1 + FULL_SALT_LEN + FULL_NONCE_LEN;
    if file_bytes.len() <= header_len {
        anyhow::bail!("Encrypted file is invalid or truncated");
    }

    let magic = &file_bytes[..4];
    if magic != FULL_FILE_MAGIC && magic != LEGACY_FULL_FILE_MAGIC {
        anyhow::bail!("Encrypted file header is invalid");
    }

    let version = file_bytes[4];
    if version != FULL_FILE_VERSION {
        anyhow::bail!("Unsupported encrypted file version: {version}");
    }

    let salt_start = 5;
    let nonce_start = salt_start + FULL_SALT_LEN;
    let ciphertext_start = nonce_start + FULL_NONCE_LEN;

    let salt = &file_bytes[salt_start..nonce_start];
    let nonce = &file_bytes[nonce_start..ciphertext_start];
    let ciphertext = &file_bytes[ciphertext_start..];

    let key = derive_encryption_key(passphrase, salt);
    let cipher = XChaCha20Poly1305::new((&key).into());
    let compressed = cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| {
            anyhow::anyhow!("Failed to decrypt file (wrong passphrase or corrupted file)")
        })?;

    let json = decompress_bytes_with_limit(&compressed, MAX_IMPORT_JSON_BYTES)
        .context("Failed to decompress decrypted payload")?;

    let store: AccountsStore =
        serde_json::from_slice(&json).context("Failed to parse decrypted account payload")?;

    Ok(store)
}

fn derive_encryption_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, FULL_KDF_ITERATIONS, &mut key);
    key
}

fn compress_bytes(input: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(input)?;
    encoder.finish().context("Failed to finalize compression")
}

fn decompress_bytes_with_limit(input: &[u8], max_bytes: u64) -> anyhow::Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(input);
    let mut limited = decoder.by_ref().take(max_bytes + 1);
    let mut decompressed = Vec::new();
    limited.read_to_end(&mut decompressed)?;

    if decompressed.len() as u64 > max_bytes {
        anyhow::bail!("Import data is too large");
    }

    Ok(decompressed)
}

fn write_encrypted_file(path: &str, bytes: &[u8]) -> anyhow::Result<()> {
    fs::write(path, bytes).with_context(|| format!("Failed to write file: {path}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to set file permissions: {path}"))?;
    }

    Ok(())
}

fn read_encrypted_file(path: &str) -> anyhow::Result<Vec<u8>> {
    let metadata =
        fs::metadata(path).with_context(|| format!("Failed to read file metadata: {path}"))?;
    if metadata.len() > MAX_IMPORT_FILE_BYTES {
        anyhow::bail!("Encrypted file is too large");
    }

    fs::read(path).with_context(|| format!("Failed to read file: {path}"))
}

fn validate_imported_store(store: &AccountsStore) -> anyhow::Result<()> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();

    for account in &store.accounts {
        if account.id.trim().is_empty() {
            anyhow::bail!("Import contains an account with empty id");
        }
        if account.name.trim().is_empty() {
            anyhow::bail!("Import contains an account with empty name");
        }
        if !ids.insert(account.id.clone()) {
            anyhow::bail!("Import contains duplicate account id: {}", account.id);
        }
        if !names.insert(account.name.clone()) {
            anyhow::bail!("Import contains duplicate account name: {}", account.name);
        }
    }

    if let Some(active_id) = &store.active_account_id {
        if !ids.contains(active_id) {
            anyhow::bail!("Import references a missing active account: {active_id}");
        }
    }

    Ok(())
}

fn merge_accounts_store(
    mut current: AccountsStore,
    imported: AccountsStore,
) -> (AccountsStore, ImportAccountsSummary) {
    let imported_version = imported.version;
    let imported_active_id = imported.active_account_id;
    let total_in_payload = imported.accounts.len();
    let mut imported_count = 0usize;
    let mut existing_ids: HashSet<String> = current.accounts.iter().map(|a| a.id.clone()).collect();
    let mut existing_names: HashSet<String> =
        current.accounts.iter().map(|a| a.name.clone()).collect();

    for account in imported.accounts {
        if existing_ids.contains(&account.id) || existing_names.contains(&account.name) {
            continue;
        }
        existing_ids.insert(account.id.clone());
        existing_names.insert(account.name.clone());
        current.accounts.push(account);
        imported_count += 1;
    }

    current.version = current.version.max(imported_version).max(1);

    let current_active_is_valid = current
        .active_account_id
        .as_ref()
        .is_some_and(|id| current.accounts.iter().any(|a| &a.id == id));

    if !current_active_is_valid {
        if let Some(imported_active) = imported_active_id {
            if current.accounts.iter().any(|a| a.id == imported_active) {
                current.active_account_id = Some(imported_active);
            } else {
                current.active_account_id = current.accounts.first().map(|a| a.id.clone());
            }
        } else {
            current.active_account_id = current.accounts.first().map(|a| a.id.clone());
        }
    }

    (
        current,
        ImportAccountsSummary {
            total_in_payload,
            imported_count,
            skipped_count: total_in_payload.saturating_sub(imported_count),
        },
    )
}
