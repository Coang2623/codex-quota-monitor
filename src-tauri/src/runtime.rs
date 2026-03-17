use anyhow::Context;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeState {
    pub(crate) blocking_cli_pids: Vec<u32>,
    pub(crate) extension_pids: Vec<u32>,
    pub(crate) vscode_pids: Vec<u32>,
    pub(crate) vscode_launch_path: Option<String>,
    pub(crate) codex_app_pids: Vec<u32>,
    pub(crate) codex_app_launch_path: Option<String>,
}

impl RuntimeState {
    pub(crate) fn restartable_process_count(&self) -> usize {
        usize::from(!self.extension_pids.is_empty())
            + usize::from(!self.vscode_pids.is_empty())
            + usize::from(!self.codex_app_pids.is_empty())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProcessRecord {
    pid: u32,
    name: String,
    executable_path: Option<String>,
    command_line: String,
}

pub(crate) fn inspect_runtime_state() -> anyhow::Result<RuntimeState> {
    let processes = list_processes()?;
    Ok(classify_processes(processes))
}

pub(crate) fn current_codex_app_pids() -> anyhow::Result<Vec<u32>> {
    Ok(inspect_runtime_state()?.codex_app_pids)
}

pub(crate) fn terminate_pids(pids: &[u32]) -> usize {
    pids.iter().filter(|pid| terminate_pid(**pid)).count()
}

pub(crate) fn wait_for_pids_to_exit(pids: &[u32], timeout: Duration) {
    if pids.is_empty() {
        return;
    }

    let tracked: HashSet<u32> = pids.iter().copied().collect();
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let still_running = list_processes()
            .map(|processes| processes.into_iter().any(|process| tracked.contains(&process.pid)))
            .unwrap_or(false);

        if !still_running {
            return;
        }

        sleep(Duration::from_millis(150));
    }
}

pub(crate) fn reload_vscode_windows(executable_path: Option<&str>) -> bool {
    const RELOAD_URI: &str = "vscode://command/workbench.action.reloadWindow";
    const OPEN_SIDEBAR_URI: &str = "vscode://command/chatgpt.openSidebar";

    #[cfg(windows)]
    {
        if dispatch_vscode_command_uri(executable_path, RELOAD_URI) {
            sleep(Duration::from_millis(1200));
            let _ = dispatch_vscode_command_uri(executable_path, OPEN_SIDEBAR_URI);
            return true;
        }

        return false;
    }

    #[cfg(not(windows))]
    {
        if let Some(path) = executable_path {
            if launch_detached(path, &["--open-url", RELOAD_URI]) {
                sleep(Duration::from_millis(1200));
                let _ = launch_detached(path, &["--open-url", OPEN_SIDEBAR_URI]);
                return true;
            }
        }

        if launch_detached("code", &["--open-url", RELOAD_URI]) {
            sleep(Duration::from_millis(1200));
            let _ = launch_detached("code", &["--open-url", OPEN_SIDEBAR_URI]);
            return true;
        }

        false
    }
}

pub(crate) fn relaunch_vscode(executable_path: Option<&str>, existing_pids: &[u32]) -> bool {
    #[cfg(windows)]
    {
        let executable_candidates = vscode_launch_candidates(executable_path);
        let cli_candidates = vscode_cli_candidates(executable_path);

        return relaunch_vscode_with_retries(
            &executable_candidates,
            &cli_candidates,
            existing_pids,
        );
    }

    #[cfg(not(windows))]
    {
        if let Some(path) = executable_path {
            if launch_detached(path, &[]) {
                return true;
            }
        }

        launch_detached("code", &[])
    }
}

pub(crate) fn relaunch_codex_app(executable_path: Option<&str>, existing_pids: &[u32]) -> bool {
    #[cfg(windows)]
    {
        let store_app_ids = codex_store_app_ids(executable_path);
        if relaunch_store_app_with_retries(&store_app_ids, is_codex_app_process, existing_pids) {
            return true;
        }

        let candidates = codex_app_launch_candidates(executable_path);
        return relaunch_with_retries(&candidates, &[], is_codex_app_process, existing_pids);
    }

    #[cfg(not(windows))]
    {
        executable_path.is_some_and(|path| launch_detached(path, &[]))
    }
}

fn classify_processes(processes: Vec<ProcessRecord>) -> RuntimeState {
    let mut state = RuntimeState::default();

    for process in processes {
        if is_monitor_process(&process) || is_runtime_inspector_process(&process) {
            continue;
        }

        if is_cursor_process(&process) {
            continue;
        }

        if is_extension_runtime(&process) {
            state.extension_pids.push(process.pid);
            continue;
        }

        if is_vscode_process(&process) {
            state.vscode_pids.push(process.pid);
            if state.vscode_launch_path.is_none() {
                state.vscode_launch_path = process.launch_target().or(Some(String::from("code")));
            }
            continue;
        }

        if is_codex_app_process(&process) {
            state.codex_app_pids.push(process.pid);
            if let Some(candidate) = process.launch_target() {
                let should_replace = state
                    .codex_app_launch_path
                    .as_deref()
                    .is_none_or(|existing| {
                        !is_primary_codex_app_launch_target(existing)
                            && is_primary_codex_app_launch_target(&candidate)
                    });

                if should_replace {
                    state.codex_app_launch_path = Some(candidate);
                }
            }
            continue;
        }

        if is_standalone_codex_cli(&process) {
            state.blocking_cli_pids.push(process.pid);
        }
    }

    dedupe(&mut state.blocking_cli_pids);
    dedupe(&mut state.extension_pids);
    dedupe(&mut state.vscode_pids);
    dedupe(&mut state.codex_app_pids);

    state
}

fn list_processes() -> anyhow::Result<Vec<ProcessRecord>> {
    #[cfg(unix)]
    {
        let output = Command::new("ps")
            .args(["-eo", "pid=,comm=,command="])
            .output()
            .context("Failed to inspect running processes via ps")?;

        return parse_unix_ps_output(&output.stdout);
    }

    #[cfg(windows)]
    {
        let output = Command::new("powershell")
            .creation_flags(CREATE_NO_WINDOW)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and ($_.Name -in @('Code.exe','codex.exe','Codex.exe','node.exe') -or $_.CommandLine -match 'codex|@openai/codex|openai.chatgpt|vscode|Code.app|Codex.app') } | ForEach-Object { \"$($_.ProcessId)`t$($_.Name)`t$($_.ExecutablePath)`t$($_.CommandLine)\" }",
            ])
            .output()
            .context("Failed to inspect running processes via PowerShell")?;

        return parse_windows_cim_output(&output.stdout);
    }
}

#[cfg(unix)]
fn parse_unix_ps_output(stdout: &[u8]) -> anyhow::Result<Vec<ProcessRecord>> {
    let stdout = String::from_utf8(stdout.to_vec()).context("ps output was not valid UTF-8")?;
    let mut processes = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(pid_str) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };

        processes.push(ProcessRecord {
            pid,
            name: name.to_string(),
            executable_path: None,
            command_line: parts.collect::<Vec<_>>().join(" "),
        });
    }

    Ok(processes)
}

fn parse_windows_cim_output(stdout: &[u8]) -> anyhow::Result<Vec<ProcessRecord>> {
    let stdout =
        String::from_utf8(stdout.to_vec()).context("PowerShell output was not valid UTF-8")?;
    let mut processes = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.splitn(4, '\t');
        let Some(pid_str) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        let executable_path = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let command_line = parts.next().unwrap_or("").trim().to_string();

        let Ok(pid) = pid_str.trim().parse::<u32>() else {
            continue;
        };

        processes.push(ProcessRecord {
            pid,
            name: name.trim().to_string(),
            executable_path,
            command_line,
        });
    }

    Ok(processes)
}

fn is_monitor_process(process: &ProcessRecord) -> bool {
    let haystack = process.combined_haystack();
    haystack.contains("codex-quota-monitor") || haystack.contains("codex quota monitor")
}

fn is_runtime_inspector_process(process: &ProcessRecord) -> bool {
    let name = process.name.to_ascii_lowercase();
    if name != "powershell.exe" && name != "powershell" && name != "pwsh.exe" && name != "pwsh" {
        return false;
    }

    let haystack = process.combined_haystack();
    haystack.contains("get-ciminstance win32_process")
        && haystack.contains("where-object")
        && haystack.contains("@openai/codex")
}

fn is_extension_runtime(process: &ProcessRecord) -> bool {
    let haystack = process.combined_haystack();
    let inside_editor_extension = haystack.contains(".vscode")
        || haystack.contains(".antigravity")
        || haystack.contains("openai.chatgpt")
        || haystack.contains("vscode-server");

    inside_editor_extension && (haystack.contains("app-server") || looks_like_codex_binary(process))
}

fn is_cursor_process(process: &ProcessRecord) -> bool {
    let name = process.name.to_ascii_lowercase();
    let haystack = process.combined_haystack();
    let launch_target = process.launch_target_lower().unwrap_or_default();

    name == "cursor"
        || name == "cursor.exe"
        || file_name_lower(&launch_target).as_deref() == Some("cursor")
        || file_name_lower(&launch_target).as_deref() == Some("cursor.exe")
        || haystack.contains("\\cursor\\")
        || haystack.contains("/cursor.app/")
        || haystack.contains("appdata\\local\\programs\\cursor")
        || haystack.contains("\\program files\\cursor\\")
        || haystack.contains("cursor.exe")
            && (haystack.contains("--vscode-window-config=") || haystack.contains("--user-data-dir="))
}

fn is_vscode_process(process: &ProcessRecord) -> bool {
    if is_cursor_process(process) {
        return false;
    }

    let name = process.name.to_ascii_lowercase();
    let haystack = process.combined_haystack();
    let launch_target = process.launch_target_lower().unwrap_or_default();

    name == "code"
        || name == "code.exe"
        || file_name_lower(&launch_target).as_deref() == Some("code")
        || file_name_lower(&launch_target).as_deref() == Some("code.exe")
        || haystack.contains("microsoft vs code")
        || haystack.contains("/visual studio code.app/")
        || haystack.contains("\\visual studio code\\")
        || haystack.contains("--vscode-window-config=")
        || haystack.contains("vscode-webview")
        || haystack.contains("microsoft.visualstudiocode")
}

fn is_codex_app_process(process: &ProcessRecord) -> bool {
    if is_extension_runtime(process) || is_vscode_process(process) {
        return false;
    }

    let haystack = process.combined_haystack();
    let launch_target = process.launch_target_lower().unwrap_or_default();
    let has_codex_app_identity = haystack.contains("/codex.app/")
        || haystack.contains("\\program files\\codex\\")
        || haystack.contains("\\program files\\openai codex\\")
        || haystack.contains("appdata\\local\\programs\\codex")
        || haystack.contains("appdata\\local\\programs\\openai codex")
        || haystack.contains("\\windowsapps\\openai.codex_")
        || haystack.contains("/applications/codex.app/")
        || haystack.contains("/opt/codex/")
        || haystack.contains("--app-user-model-id=com.openai.codex")
        || haystack.contains("--annotation=_productname=codex")
        || haystack.contains("--user-data-dir=\"c:\\users\\")
            && haystack.contains("appdata\\roaming\\codex")
        || launch_target.contains("/codex.app/")
        || launch_target.contains("\\program files\\codex\\")
        || launch_target.contains("\\program files\\openai codex\\")
        || launch_target.contains("appdata\\local\\programs\\codex")
        || launch_target.contains("appdata\\local\\programs\\openai codex")
        || launch_target.contains("\\windowsapps\\openai.codex_");
    let looks_like_app_runtime = haystack.contains("--type=")
        || haystack.contains("--utility-sub-type=")
        || haystack.contains("--user-data-dir=")
        || haystack.contains("--app-path=")
        || haystack.contains("crashpad-handler")
        || haystack.contains("gpu-process")
        || haystack.contains("renderer")
        || haystack.contains("app-server")
        || is_primary_codex_app_launch_target(&launch_target);

    has_codex_app_identity
        && looks_like_app_runtime
        && !haystack.contains("@openai/codex")
        && !haystack.contains("node_modules/@openai/codex")
        && !haystack.contains("\\node_modules\\@openai\\codex")
}

fn is_standalone_codex_cli(process: &ProcessRecord) -> bool {
    if is_monitor_process(process)
        || is_extension_runtime(process)
        || is_vscode_process(process)
        || is_codex_app_process(process)
    {
        return false;
    }

    let haystack = process.combined_haystack();
    looks_like_codex_binary(process)
        || haystack.contains("@openai/codex")
        || haystack.contains("node_modules/@openai/codex")
        || haystack.contains("\\node_modules\\@openai\\codex")
        || haystack.contains("codex.js")
}

fn looks_like_codex_binary(process: &ProcessRecord) -> bool {
    let name = process.name.to_ascii_lowercase();
    if name == "codex" || name == "codex.exe" {
        return true;
    }

    process
        .launch_target_lower()
        .as_deref()
        .and_then(file_name_lower)
        .is_some_and(|file_name| file_name == "codex" || file_name == "codex.exe")
}

fn is_primary_codex_app_launch_target(path: &str) -> bool {
    let normalized = path.to_ascii_lowercase();
    let looks_like_codex_executable = file_name_lower(path)
        .as_deref()
        .is_some_and(|file_name| file_name == "codex" || file_name == "codex.exe");

    looks_like_codex_executable
        && !normalized.contains("\\app\\resources\\codex.exe")
        && !normalized.contains("/app/resources/codex")
        && !normalized.contains(" app-server")
}

fn terminate_pid(pid: u32) -> bool {
    #[cfg(unix)]
    {
        return Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
    }

    #[cfg(windows)]
    {
        return Command::new("taskkill")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["/F", "/PID", &pid.to_string()])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
    }
}

fn launch_detached(program: &str, args: &[&str]) -> bool {
    #[cfg(unix)]
    {
        Command::new(program).args(args).spawn().is_ok()
    }

    #[cfg(windows)]
    {
        let escaped_program = escape_powershell_single_quoted(program);
        let command = if args.is_empty() {
            format!("Start-Process -FilePath '{escaped_program}'")
        } else {
            let escaped_args = args
                .iter()
                .map(|arg| format!("'{}'", escape_powershell_single_quoted(arg)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Start-Process -FilePath '{escaped_program}' -ArgumentList {}", escaped_args)
        };

        Command::new("powershell")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["-NoProfile", "-NonInteractive", "-Command", &command])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

#[cfg(windows)]
fn escape_powershell_single_quoted(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(windows)]
fn launch_windows_store_app(app_id: &str) -> bool {
    let escaped_app_id = escape_powershell_single_quoted(app_id);
    let command = format!(
        "Start-Process -FilePath 'explorer.exe' -ArgumentList 'shell:AppsFolder\\{escaped_app_id}'"
    );

    Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn launch_uri_detached(uri: &str) -> bool {
    Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "Start-Process -FilePath '{}'",
                escape_powershell_single_quoted(uri)
            ),
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn wait_for_matching_process(matcher: fn(&ProcessRecord) -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let has_match = list_processes()
            .map(|processes| processes.into_iter().any(|process| matcher(&process)))
            .unwrap_or(false);

        if has_match {
            return true;
        }

        sleep(Duration::from_millis(200));
    }

    false
}

#[cfg(windows)]
fn relaunch_vscode_with_retries(
    executable_candidates: &[String],
    cli_candidates: &[String],
    existing_pids: &[u32],
) -> bool {
    let mut dispatched_launch = false;

    for candidate in cli_candidates {
        if !run_vscode_cli(candidate, &[]) {
            continue;
        }

        dispatched_launch = true;
        if confirm_vscode_relaunch(existing_pids) {
            return true;
        }
    }

    for candidate in executable_candidates {
        if !launch_detached(candidate, &[]) {
            continue;
        }

        dispatched_launch = true;
        if confirm_vscode_relaunch(existing_pids) {
            return true;
        }
    }

    dispatched_launch
}

#[cfg(windows)]
fn confirm_vscode_relaunch(existing_pids: &[u32]) -> bool {
    if wait_for_new_matching_process(
        is_vscode_process,
        existing_pids,
        Duration::from_millis(1500),
    ) {
        return true;
    }

    sleep(Duration::from_millis(350));

    list_processes()
        .map(|processes| processes.into_iter().any(|process| is_vscode_process(&process)))
        .unwrap_or(false)
}

#[cfg(windows)]
fn relaunch_with_retries(
    candidates: &[String],
    args: &[&str],
    matcher: fn(&ProcessRecord) -> bool,
    existing_pids: &[u32],
) -> bool {
    if candidates.is_empty() {
        return false;
    }

    for attempt in 0..3 {
        let known_existing = known_matching_pids(matcher, existing_pids);

        for candidate in candidates {
            if !launch_detached(candidate, args) {
                continue;
            }

            if wait_for_new_matching_process(
                matcher,
                &known_existing,
                Duration::from_millis(2500 + (attempt * 1000) as u64),
            ) {
                return true;
            }
        }

        sleep(Duration::from_millis(1200 + (attempt * 600) as u64));
    }

    false
}

#[cfg(windows)]
fn relaunch_store_app_with_retries(
    app_ids: &[String],
    matcher: fn(&ProcessRecord) -> bool,
    existing_pids: &[u32],
) -> bool {
    if app_ids.is_empty() {
        return false;
    }

    for attempt in 0..3 {
        let known_existing = known_matching_pids(matcher, existing_pids);

        for app_id in app_ids {
            if !launch_windows_store_app(app_id) {
                continue;
            }

            if wait_for_new_matching_process(
                matcher,
                &known_existing,
                Duration::from_millis(3500 + (attempt * 1500) as u64),
            ) {
                return true;
            }
        }

        sleep(Duration::from_millis(1600 + (attempt * 800) as u64));
    }

    false
}

#[cfg(windows)]
fn wait_for_new_matching_process(
    matcher: fn(&ProcessRecord) -> bool,
    existing_pids: &[u32],
    timeout: Duration,
) -> bool {
    let existing: HashSet<u32> = existing_pids.iter().copied().collect();
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let has_new_match = list_processes()
            .map(|processes| {
                processes
                    .into_iter()
                    .any(|process| !existing.contains(&process.pid) && matcher(&process))
            })
            .unwrap_or(false);

        if has_new_match {
            return true;
        }

        sleep(Duration::from_millis(200));
    }

    false
}

#[cfg(windows)]
fn known_matching_pids(
    matcher: fn(&ProcessRecord) -> bool,
    seed_pids: &[u32],
) -> Vec<u32> {
    let mut pids = seed_pids.to_vec();

    if let Ok(processes) = list_processes() {
        pids.extend(
            processes
                .into_iter()
                .filter(|process| matcher(process))
                .map(|process| process.pid),
        );
    }

    dedupe(&mut pids);
    pids
}

#[cfg(windows)]
#[allow(dead_code)]
fn vscode_launch_candidates(executable_path: Option<&str>) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(path) = executable_path {
        if let Some(executable) = vscode_executable_candidate(path) {
            push_launch_candidate(&mut candidates, &executable);
        }
    }

    for path in lookup_windows_command("Code.exe") {
        push_launch_candidate(&mut candidates, &path);
    }

    for path in known_vscode_paths() {
        push_launch_candidate(&mut candidates, &path);
    }

    candidates
}

#[cfg(windows)]
fn dispatch_vscode_command_uri(executable_path: Option<&str>, uri: &str) -> bool {
    let cli_candidates = vscode_cli_candidates(executable_path);

    for candidate in &cli_candidates {
        if run_vscode_cli(candidate, &["--open-url", uri]) {
            return true;
        }
    }

    launch_uri_detached(uri)
}

#[cfg(windows)]
fn run_vscode_cli(program: &str, args: &[&str]) -> bool {
    let escaped_program = escape_powershell_single_quoted(program);
    let escaped_args = args
        .iter()
        .map(|arg| format!("'{}'", escape_powershell_single_quoted(arg)))
        .collect::<Vec<_>>()
        .join(" ");
    let command = if escaped_args.is_empty() {
        format!("& '{escaped_program}'")
    } else {
        format!("& '{escaped_program}' {escaped_args}")
    };

    Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn vscode_cli_candidates(executable_path: Option<&str>) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(path) = executable_path {
        if let Some(cli_path) = vscode_cli_candidate(path) {
            push_launch_candidate(&mut candidates, &cli_path);
        }
    }

    for path in lookup_windows_command("code.cmd") {
        push_launch_candidate(&mut candidates, &path);
    }

    for path in known_vscode_cli_paths() {
        push_launch_candidate(&mut candidates, &path);
    }

    candidates
}

#[cfg(windows)]
fn codex_app_launch_candidates(executable_path: Option<&str>) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(path) = executable_path {
        push_launch_candidate(&mut candidates, path);
    }

    for path in known_codex_app_paths() {
        push_launch_candidate(&mut candidates, &path);
    }

    candidates
}

#[cfg(windows)]
fn codex_store_app_ids(executable_path: Option<&str>) -> Vec<String> {
    let mut app_ids = Vec::new();

    if let Some(path) = executable_path {
        if let Some(app_id) = codex_store_app_id_from_path(path) {
            push_store_app_id(&mut app_ids, &app_id);
        }
    }

    for app_id in lookup_codex_store_app_ids() {
        push_store_app_id(&mut app_ids, &app_id);
    }

    app_ids
}

#[cfg(windows)]
fn push_store_app_id(app_ids: &mut Vec<String>, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    let normalized = trimmed.to_string();
    if !app_ids
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(&normalized))
    {
        app_ids.push(normalized);
    }
}

#[cfg(windows)]
fn lookup_codex_store_app_ids() -> Vec<String> {
    let Ok(output) = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-StartApps | Where-Object { $_.Name -eq 'Codex' -or $_.AppID -match '^OpenAI\\.Codex_.*!App$' } | Select-Object -ExpandProperty AppID",
        ])
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(windows)]
fn codex_store_app_id_from_path(path: &str) -> Option<String> {
    let normalized = path.replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    let marker = "\\windowsapps\\";
    let start = lower.find(marker)? + marker.len();
    let package_full_name = normalized[start..].split('\\').next()?;
    let (package_prefix, publisher_id) = package_full_name.rsplit_once("__")?;
    let package_name = package_prefix.split('_').next()?;

    if package_name.is_empty() || publisher_id.is_empty() {
        return None;
    }

    Some(format!("{package_name}_{publisher_id}!App"))
}

#[cfg(windows)]
fn push_launch_candidate(candidates: &mut Vec<String>, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    let normalized = trimmed.to_string();
    let is_named_command = !normalized.contains('\\') && !normalized.contains('/');
    if !is_named_command && !Path::new(&normalized).exists() {
        return;
    }

    if !candidates.iter().any(|candidate| candidate.eq_ignore_ascii_case(&normalized)) {
        candidates.push(normalized);
    }
}

#[cfg(windows)]
fn lookup_windows_command(command: &str) -> Vec<String> {
    let Ok(output) = Command::new("where.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .arg(command)
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(windows)]
#[allow(dead_code)]
fn vscode_executable_candidate(path: &str) -> Option<String> {
    let candidate = Path::new(path);
    let file_name = candidate.file_name()?.to_str()?.to_ascii_lowercase();

    if file_name == "code.exe" && candidate.exists() {
        return Some(candidate.to_string_lossy().into_owned());
    }

    if file_name == "code.cmd" {
        let install_dir = candidate.parent()?.parent()?;
        let executable = install_dir.join("Code.exe");
        if executable.exists() {
            return Some(executable.to_string_lossy().into_owned());
        }
    }

    None
}

#[cfg(windows)]
fn vscode_cli_candidate(path: &str) -> Option<String> {
    let candidate = Path::new(path);
    let file_name = candidate.file_name()?.to_str()?.to_ascii_lowercase();

    if file_name == "code.cmd" && candidate.exists() {
        return Some(candidate.to_string_lossy().into_owned());
    }

    if file_name == "code.exe" {
        let cli = candidate.parent()?.join("bin").join("code.cmd");
        if cli.exists() {
            return Some(cli.to_string_lossy().into_owned());
        }
    }

    None
}

#[cfg(windows)]
#[allow(dead_code)]
fn known_vscode_paths() -> Vec<String> {
    let mut paths = vec![
        String::from(r"D:\Apps\Microsoft VS Code\Code.exe"),
        String::from(r"C:\Program Files\Microsoft VS Code\Code.exe"),
        String::from(r"C:\Program Files (x86)\Microsoft VS Code\Code.exe"),
    ];

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let base = Path::new(&local_app_data).join("Programs").join("Microsoft VS Code");
        paths.push(base.join("Code.exe").to_string_lossy().into_owned());
    }

    paths
}

#[cfg(windows)]
fn known_vscode_cli_paths() -> Vec<String> {
    let mut paths = vec![
        String::from(r"D:\Apps\Microsoft VS Code\bin\code.cmd"),
        String::from(r"C:\Program Files\Microsoft VS Code\bin\code.cmd"),
        String::from(r"C:\Program Files (x86)\Microsoft VS Code\bin\code.cmd"),
    ];

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let base = Path::new(&local_app_data)
            .join("Programs")
            .join("Microsoft VS Code")
            .join("bin")
            .join("code.cmd");
        paths.push(base.to_string_lossy().into_owned());
    }

    paths
}

#[cfg(windows)]
fn known_codex_app_paths() -> Vec<String> {
    let mut paths = vec![
        String::from(r"C:\Program Files\Codex\Codex.exe"),
        String::from(r"C:\Program Files\OpenAI Codex\Codex.exe"),
        String::from(r"C:\Program Files (x86)\Codex\Codex.exe"),
        String::from(r"C:\Program Files (x86)\OpenAI Codex\Codex.exe"),
    ];

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let programs = Path::new(&local_app_data).join("Programs");
        paths.push(programs.join("Codex").join("Codex.exe").to_string_lossy().into_owned());
        paths.push(
            programs
                .join("OpenAI Codex")
                .join("Codex.exe")
                .to_string_lossy()
                .into_owned(),
        );
    }

    paths
}

fn dedupe(pids: &mut Vec<u32>) {
    let mut seen = HashSet::new();
    pids.retain(|pid| seen.insert(*pid));
}

fn first_command_token(command_line: &str) -> Option<String> {
    let command_line = command_line.trim();
    if command_line.is_empty() {
        return None;
    }

    if let Some(rest) = command_line.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }

    command_line
        .split_whitespace()
        .next()
        .map(ToOwned::to_owned)
}

fn file_name_lower(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
}

impl ProcessRecord {
    fn combined_haystack(&self) -> String {
        format!(
            "{} {} {}",
            self.name.to_ascii_lowercase(),
            self.executable_path
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase(),
            self.command_line.to_ascii_lowercase()
        )
    }

    fn launch_target(&self) -> Option<String> {
        self.executable_path
            .clone()
            .or_else(|| first_command_token(&self.command_line))
    }

    fn launch_target_lower(&self) -> Option<String> {
        self.launch_target().map(|target| target.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_processes, ProcessRecord};

    fn record(pid: u32, name: &str, executable_path: Option<&str>, command_line: &str) -> ProcessRecord {
        ProcessRecord {
            pid,
            name: name.to_string(),
            executable_path: executable_path.map(ToOwned::to_owned),
            command_line: command_line.to_string(),
        }
    }

    #[test]
    fn classifies_node_wrapped_codex_cli_as_blocking() {
        let state = classify_processes(vec![record(
            101,
            "node.exe",
            Some(r"C:\Program Files\nodejs\node.exe"),
            r#""C:\Program Files\nodejs\node.exe" "C:\Users\me\AppData\Roaming\npm\node_modules\@openai\codex\bin\codex.js""#,
        )]);

        assert_eq!(state.blocking_cli_pids, vec![101]);
        assert!(state.extension_pids.is_empty());
        assert!(state.vscode_pids.is_empty());
    }

    #[test]
    fn classifies_extension_worker_as_restartable() {
        let state = classify_processes(vec![record(
            202,
            "codex.exe",
            Some(r"C:\Users\me\.vscode\extensions\openai.chatgpt\bin\windows-x86_64\codex.exe"),
            r#"C:\Users\me\.vscode\extensions\openai.chatgpt\bin\windows-x86_64\codex.exe app-server --analytics-default-enabled"#,
        )]);

        assert_eq!(state.extension_pids, vec![202]);
        assert!(state.blocking_cli_pids.is_empty());
    }

    #[test]
    fn classifies_vscode_window_and_captures_launch_path() {
        let state = classify_processes(vec![record(
            303,
            "Code.exe",
            Some(r"D:\Apps\Microsoft VS Code\Code.exe"),
            r#""D:\Apps\Microsoft VS Code\Code.exe" --type=renderer --vscode-window-config=vscode:abc"#,
        )]);

        assert_eq!(state.vscode_pids, vec![303]);
        assert_eq!(
            state.vscode_launch_path.as_deref(),
            Some(r"D:\Apps\Microsoft VS Code\Code.exe")
        );
        assert_eq!(state.restartable_process_count(), 1);
    }

    #[test]
    fn classifies_codex_app_as_restartable() {
        let state = classify_processes(vec![record(
            404,
            "Codex",
            Some("/Applications/Codex.app/Contents/MacOS/Codex"),
            r#""/Applications/Codex.app/Contents/MacOS/Codex""#,
        )]);

        assert_eq!(state.codex_app_pids, vec![404]);
        assert_eq!(
            state.codex_app_launch_path.as_deref(),
            Some("/Applications/Codex.app/Contents/MacOS/Codex")
        );
    }

    #[test]
    fn windows_store_codex_app_and_app_server_are_not_cli() {
        let state = classify_processes(vec![
            record(
                808,
                "codex.exe",
                Some(
                    r"C:\Program Files\WindowsApps\OpenAI.Codex_26.313.5234.0_x64__2p2nqsd0c76g0\app\resources\codex.exe",
                ),
                r#""C:\Program Files\WindowsApps\OpenAI.Codex_26.313.5234.0_x64__2p2nqsd0c76g0\app\resources\codex.exe" app-server --analytics-default-enabled"#,
            ),
            record(
                809,
                "Codex.exe",
                Some(
                    r"C:\Program Files\WindowsApps\OpenAI.Codex_26.313.5234.0_x64__2p2nqsd0c76g0\app\Codex.exe",
                ),
                r#""C:\Program Files\WindowsApps\OpenAI.Codex_26.313.5234.0_x64__2p2nqsd0c76g0\app\Codex.exe" --type=renderer --user-data-dir="C:\Users\me\AppData\Roaming\Codex" --app-user-model-id=com.openai.codex"#,
            ),
        ]);

        assert!(state.blocking_cli_pids.is_empty());
        assert_eq!(state.codex_app_pids, vec![808, 809]);
        assert_eq!(
            state.codex_app_launch_path.as_deref(),
            Some(
                r"C:\Program Files\WindowsApps\OpenAI.Codex_26.313.5234.0_x64__2p2nqsd0c76g0\app\Codex.exe"
            )
        );
        assert_eq!(state.restartable_process_count(), 1);
    }

    #[test]
    fn derives_windows_store_codex_app_id_from_launch_path() {
        let app_id = super::codex_store_app_id_from_path(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.313.5234.0_x64__2p2nqsd0c76g0\app\Codex.exe",
        );

        assert_eq!(app_id.as_deref(), Some("OpenAI.Codex_2p2nqsd0c76g0!App"));
    }

    #[test]
    fn mixed_cli_and_vscode_keeps_cli_blocking() {
        let state = classify_processes(vec![
            record(
                505,
                "node",
                Some("/usr/bin/node"),
                "/usr/bin/node /usr/local/lib/node_modules/@openai/codex/bin/codex.js",
            ),
            record(
                506,
                "Code",
                Some("/Applications/Visual Studio Code.app/Contents/MacOS/Electron"),
                r#""/Applications/Visual Studio Code.app/Contents/MacOS/Electron" --vscode-window-config=vscode:def"#,
            ),
        ]);

        assert_eq!(state.blocking_cli_pids, vec![505]);
        assert_eq!(state.vscode_pids, vec![506]);
    }

    #[test]
    fn vscode_process_is_not_classified_as_cli() {
        let state = classify_processes(vec![record(
            606,
            "Code.exe",
            Some(r"D:\Apps\Microsoft VS Code\Code.exe"),
            r#""D:\Apps\Microsoft VS Code\Code.exe" --type=renderer --vscode-window-config=vscode:ghi"#,
        )]);

        assert!(state.blocking_cli_pids.is_empty());
        assert_eq!(state.vscode_pids, vec![606]);
    }

    #[test]
    fn cursor_process_is_ignored() {
        let state = classify_processes(vec![record(
            707,
            "Cursor.exe",
            Some(r"C:\Users\me\AppData\Local\Programs\Cursor\Cursor.exe"),
            r#""C:\Users\me\AppData\Local\Programs\Cursor\Cursor.exe" --type=renderer --vscode-window-config=vscode:cursor"#,
        )]);

        assert!(state.blocking_cli_pids.is_empty());
        assert!(state.extension_pids.is_empty());
        assert!(state.vscode_pids.is_empty());
        assert!(state.codex_app_pids.is_empty());
        assert_eq!(state.restartable_process_count(), 0);
    }

    #[test]
    fn powershell_runtime_inspector_is_not_classified_as_cli() {
        let state = classify_processes(vec![record(
            808,
            "powershell.exe",
            Some(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            r#""C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and ($_.Name -in @('Code.exe','codex.exe','Codex.exe','node.exe') -or $_.CommandLine -match 'codex|@openai/codex|openai.chatgpt|vscode|Code.app|Codex.app') }""#,
        )]);

        assert!(state.blocking_cli_pids.is_empty());
        assert!(state.extension_pids.is_empty());
        assert!(state.vscode_pids.is_empty());
        assert!(state.codex_app_pids.is_empty());
    }
}
