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
        self.extension_pids.len() + self.vscode_pids.len() + self.codex_app_pids.len()
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

pub(crate) fn relaunch_codex_app(executable_path: Option<&str>) -> bool {
    #[cfg(windows)]
    {
        let candidates = codex_app_launch_candidates(executable_path);
        return relaunch_with_retries(&candidates, &[], is_codex_app_process);
    }

    #[cfg(not(windows))]
    {
        executable_path.is_some_and(|path| launch_detached(path, &[]))
    }
}

fn classify_processes(processes: Vec<ProcessRecord>) -> RuntimeState {
    let mut state = RuntimeState::default();

    for process in processes {
        if is_switcher_process(&process) || is_runtime_inspector_process(&process) {
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
            if state.codex_app_launch_path.is_none() {
                state.codex_app_launch_path = process.launch_target();
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

fn is_switcher_process(process: &ProcessRecord) -> bool {
    let haystack = process.combined_haystack();
    haystack.contains("codex-switcher") || haystack.contains("codex switcher")
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

fn is_vscode_process(process: &ProcessRecord) -> bool {
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

    let looks_like_app_path = haystack.contains("/codex.app/")
        || haystack.contains("\\program files\\codex")
        || haystack.contains("appdata\\local\\programs\\codex")
        || haystack.contains("/applications/codex.app/")
        || haystack.contains("/opt/codex/")
        || launch_target.contains("/codex.app/")
        || launch_target.contains("\\program files\\codex")
        || launch_target.contains("appdata\\local\\programs\\codex");

    looks_like_app_path
        && !haystack.contains("app-server")
        && !haystack.contains("@openai/codex")
        && !haystack.contains("node_modules/@openai/codex")
        && !haystack.contains("\\node_modules\\@openai\\codex")
}

fn is_standalone_codex_cli(process: &ProcessRecord) -> bool {
    if is_switcher_process(process)
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
) -> bool {
    if candidates.is_empty() {
        return false;
    }

    for candidate in candidates {
        if !launch_detached(candidate, args) {
            continue;
        }

        if wait_for_matching_process(matcher, Duration::from_millis(1500)) {
            return true;
        }

        return true;
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
    fn powershell_runtime_inspector_is_not_classified_as_cli() {
        let state = classify_processes(vec![record(
            707,
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
