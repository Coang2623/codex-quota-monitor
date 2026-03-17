import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAccounts } from "./hooks/useAccounts";
import { useAppUpdate } from "./hooks/useAppUpdate";
import { AccountCard, AddAccountModal, LogPanel } from "./components";
import type {
  AppLogEntry,
  AppLogLevel,
  CodexProcessInfo,
  SwitchAccountResult,
} from "./types";
import "./App.css";

const MAX_LOG_ENTRIES = 250;

function createClientLogEntry(
  scope: string,
  message: string,
  level: AppLogLevel = "info"
): AppLogEntry {
  return {
    id: `frontend-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    timestamp_ms: Date.now(),
    level,
    scope,
    message,
    source: "frontend",
  };
}

function normalizeBackendLogEntry(entry: AppLogEntry): AppLogEntry {
  return {
    ...entry,
    source: "backend",
  };
}

function appendLogEntry(entries: AppLogEntry[], entry: AppLogEntry): AppLogEntry[] {
  const next = [...entries, entry];
  return next.length > MAX_LOG_ENTRIES ? next.slice(next.length - MAX_LOG_ENTRIES) : next;
}

function pluralize(count: number, singular: string, plural = `${singular}s`) {
  return `${count} ${count === 1 ? singular : plural}`;
}

function formatRelativeReleaseDate(value: string | null): string | null {
  if (!value) return null;
  const publishedAt = new Date(value);
  if (Number.isNaN(publishedAt.getTime())) {
    return null;
  }

  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(publishedAt);
}

function App() {
  const {
    accounts,
    loading,
    error,
    refreshUsage,
    refreshSingleUsage,
    warmupAccount,
    warmupAllAccounts,
    switchAccount,
    deleteAccount,
    renameAccount,
    importFromFile,
    exportAccountsSlimText,
    importAccountsSlimText,
    exportAccountsFullEncryptedFile,
    importAccountsFullEncryptedFile,
    startOAuthLogin,
    completeOAuthLogin,
    cancelOAuthLogin,
  } = useAccounts();
  const { updateInfo, isCheckingForUpdates, isInstallingUpdate, checkForUpdates, installUpdate } =
    useAppUpdate();

  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [isConfigModalOpen, setIsConfigModalOpen] = useState(false);
  const [configModalMode, setConfigModalMode] = useState<"slim_export" | "slim_import">(
    "slim_export"
  );
  const [configPayload, setConfigPayload] = useState("");
  const [configModalError, setConfigModalError] = useState<string | null>(null);
  const [configCopied, setConfigCopied] = useState(false);
  const [switchingId, setSwitchingId] = useState<string | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const [processInfo, setProcessInfo] = useState<CodexProcessInfo | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isExportingSlim, setIsExportingSlim] = useState(false);
  const [isImportingSlim, setIsImportingSlim] = useState(false);
  const [isExportingFull, setIsExportingFull] = useState(false);
  const [isImportingFull, setIsImportingFull] = useState(false);
  const [isWarmingAll, setIsWarmingAll] = useState(false);
  const [warmingUpId, setWarmingUpId] = useState<string | null>(null);
  const [refreshSuccess, setRefreshSuccess] = useState(false);
  const [warmupToast, setWarmupToast] = useState<{
    message: string;
    isError: boolean;
  } | null>(null);
  const [maskedAccounts, setMaskedAccounts] = useState<Set<string>>(new Set());
  const [otherAccountsSort, setOtherAccountsSort] = useState<
    "deadline_asc" | "deadline_desc" | "remaining_desc" | "remaining_asc"
  >("deadline_asc");
  const [logEntries, setLogEntries] = useState<AppLogEntry[]>([]);
  const [isActionsMenuOpen, setIsActionsMenuOpen] = useState(false);
  const [dismissedUpdateVersion, setDismissedUpdateVersion] = useState<string | null>(null);
  const actionsMenuRef = useRef<HTMLDivElement | null>(null);
  const processSnapshotRef = useRef<string | null>(null);
  const switchResetTimerRef = useRef<number | null>(null);
  const updateSnapshotRef = useRef<string | null>(null);
  const updateLifecycleRef = useRef<string | null>(null);

  const appendLog = useCallback(
    (scope: string, message: string, level: AppLogLevel = "info") => {
      setLogEntries((prev) => appendLogEntry(prev, createClientLogEntry(scope, message, level)));
    },
    []
  );

  const clearSwitchResetTimer = useCallback(() => {
    if (switchResetTimerRef.current !== null) {
      window.clearTimeout(switchResetTimerRef.current);
      switchResetTimerRef.current = null;
    }
  }, []);

  const toggleMask = (accountId: string) => {
    setMaskedAccounts((prev) => {
      const next = new Set(prev);
      if (next.has(accountId)) {
        next.delete(accountId);
      } else {
        next.add(accountId);
      }
      return next;
    });
  };

  const allMasked =
    accounts.length > 0 && accounts.every((account) => maskedAccounts.has(account.id));

  const toggleMaskAll = () => {
    setMaskedAccounts((prev) => {
      const shouldMaskAll = !accounts.every((account) => prev.has(account.id));
      if (shouldMaskAll) {
        return new Set(accounts.map((account) => account.id));
      }
      return new Set();
    });
  };

  const checkProcesses = useCallback(async () => {
    try {
      const info = await invoke<CodexProcessInfo>("check_codex_processes");
      setProcessInfo(info);
    } catch (err) {
      console.error("Failed to check processes:", err);
      appendLog("runtime", `Process check failed: ${formatWarmupError(err)}`, "error");
    }
  }, [appendLog]);

  useEffect(() => {
    if (!updateInfo.checked_at) return;

    const snapshot = [
      updateInfo.status,
      updateInfo.current_version,
      updateInfo.latest_version,
      updateInfo.error,
      updateInfo.source,
      updateInfo.checked_at,
    ].join(":");

    if (updateSnapshotRef.current === snapshot) {
      return;
    }
    updateSnapshotRef.current = snapshot;

    if (updateInfo.status === "available") {
      appendLog(
        "update",
        updateInfo.can_download_and_install
          ? `Update available: v${updateInfo.current_version} -> v${updateInfo.latest_version}`
          : `Release available: v${updateInfo.current_version} -> v${updateInfo.latest_version} (manual install fallback)`,
        updateInfo.can_download_and_install ? "warn" : "info"
      );
      return;
    }

    if (updateInfo.status === "up_to_date") {
      appendLog(
        "update",
        `App is up to date at v${updateInfo.current_version}`,
        "success"
      );
      if (updateInfo.source === "manual") {
        showWarmupToast(`Already using the latest version (v${updateInfo.current_version}).`);
      }
      return;
    }

    if (updateInfo.status === "error") {
      appendLog(
        "update",
        `Update check failed: ${updateInfo.error ?? "Unknown error"}`,
        "error"
      );
      if (updateInfo.source === "manual") {
        showWarmupToast(
          `Update check failed: ${updateInfo.error ?? "Unknown error"}`,
          true
        );
      }
    }
  }, [appendLog, updateInfo]);

  useEffect(() => {
    if (updateLifecycleRef.current === updateInfo.status) {
      return;
    }
    updateLifecycleRef.current = updateInfo.status;

    if (updateInfo.status === "downloading") {
      appendLog("update", "Downloading update package", "info");
      return;
    }

    if (updateInfo.status === "installing") {
      appendLog("update", "Installing update package", "warn");
      return;
    }

    if (updateInfo.status === "relaunching") {
      appendLog("update", "Update installed. Restarting application", "success");
    }
  }, [appendLog, updateInfo.status]);

  const handleClearLogs = useCallback(async () => {
    try {
      await invoke("clear_logs");
      setLogEntries([]);
    } catch (err) {
      console.error("Failed to clear logs:", err);
      appendLog("log", `Clear failed: ${formatWarmupError(err)}`, "error");
    }
  }, [appendLog]);

  // Check processes on mount and periodically
  useEffect(() => {
    checkProcesses();
    const interval = setInterval(checkProcesses, 3000); // Check every 3 seconds
    return () => clearInterval(interval);
  }, [checkProcesses]);

  useEffect(() => {
    let mounted = true;
    let unlisten: UnlistenFn | null = null;

    invoke<AppLogEntry[]>("get_recent_logs")
      .then((entries) => {
        if (!mounted) return;
        setLogEntries(entries.map(normalizeBackendLogEntry));
      })
      .catch((err) => {
        console.error("Failed to load recent logs:", err);
        if (mounted) {
          appendLog("log", `Initial log sync failed: ${formatWarmupError(err)}`, "error");
        }
      });

    listen<AppLogEntry>("app-log", (event) => {
      if (!mounted) return;
      setLogEntries((prev) => appendLogEntry(prev, normalizeBackendLogEntry(event.payload)));
    })
      .then((dispose) => {
        unlisten = dispose;
      })
      .catch((err) => {
        console.error("Failed to subscribe to app logs:", err);
        if (mounted) {
          appendLog("log", `Live log subscription failed: ${formatWarmupError(err)}`, "error");
        }
      });

    return () => {
      mounted = false;
      if (unlisten) {
        void unlisten();
      }
    };
  }, [appendLog]);

  useEffect(() => {
    if (!processInfo) return;

    const snapshot = [
      processInfo.count,
      processInfo.background_count,
      processInfo.can_switch,
      processInfo.vscode_window_count,
      processInfo.vscode_extension_count,
      processInfo.antigravity_window_count,
      processInfo.antigravity_extension_count,
      processInfo.codex_app_count,
    ].join(":");
    if (processSnapshotRef.current === snapshot) {
      return;
    }
    processSnapshotRef.current = snapshot;

    if (processInfo.count > 0) {
      appendLog(
        "runtime",
        `Blocking CLI detected: ${processInfo.count} standalone process(es)`,
        "warn"
      );
      return;
    }

    if (processInfo.background_count > 0) {
      const activeSessions = [
        processInfo.vscode_extension_count > 0 ? "VS Code" : null,
        processInfo.antigravity_extension_count > 0 ? "Antigravity" : null,
        processInfo.codex_app_count > 0 ? "Codex app" : null,
      ].filter(Boolean);
      appendLog(
        "runtime",
        `Detected active session(s): ${activeSessions.join(", ")}`,
        "info"
      );
      return;
    }

    appendLog("runtime", "No Codex runtime detected", "success");
  }, [appendLog, processInfo]);

  useEffect(() => {
    if (!isActionsMenuOpen) return;

    const handleClickOutside = (event: MouseEvent) => {
      if (!actionsMenuRef.current) return;
      if (!actionsMenuRef.current.contains(event.target as Node)) {
        setIsActionsMenuOpen(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [isActionsMenuOpen]);

  useEffect(() => {
    return () => {
      clearSwitchResetTimer();
    };
  }, [clearSwitchResetTimer]);

  const handleSwitch = async (accountId: string) => {
    const target = accounts.find((account) => account.id === accountId);
    try {
      clearSwitchResetTimer();
      setSwitchingId(accountId);
      switchResetTimerRef.current = window.setTimeout(() => {
        setSwitchingId((current) => {
          if (current !== accountId) {
            return current;
          }
          return null;
        });
        switchResetTimerRef.current = null;
      }, 90000);
      appendLog(
        "switch",
        `Switch started${target ? ` for ${target.name}` : ""}`,
        "info"
      );
      const result = await switchAccount(accountId);
      clearSwitchResetTimer();
      setSwitchingId((current) => (current === accountId ? null : current));
      appendLog("switch", formatSwitchOutcome(result), "success");
      showWarmupToast(formatSwitchOutcome(result));
      void checkProcesses();
    } catch (err) {
      console.error("Failed to switch account:", err);
      appendLog("switch", `Switch failed: ${formatWarmupError(err)}`, "error");
      showWarmupToast(`Switch failed: ${formatWarmupError(err)}`, true);
    } finally {
      clearSwitchResetTimer();
      setSwitchingId((current) => (current === accountId ? null : current));
    }
  };

  const handleDelete = async (accountId: string) => {
    if (deleteConfirmId !== accountId) {
      setDeleteConfirmId(accountId);
      setTimeout(() => setDeleteConfirmId(null), 3000);
      return;
    }

    try {
      await deleteAccount(accountId);
      setDeleteConfirmId(null);
    } catch (err) {
      console.error("Failed to delete account:", err);
    }
  };

  const handleRefresh = async () => {
    setIsRefreshing(true);
    setRefreshSuccess(false);
    try {
      appendLog("usage", "Refreshing usage for all accounts", "info");
      await refreshUsage();
      appendLog("usage", "Usage refresh completed", "success");
      setRefreshSuccess(true);
      setTimeout(() => setRefreshSuccess(false), 2000);
    } catch (err) {
      appendLog("usage", `Refresh failed: ${formatWarmupError(err)}`, "error");
    } finally {
      setIsRefreshing(false);
    }
  };

  const showWarmupToast = (message: string, isError = false) => {
    setWarmupToast({ message, isError });
    setTimeout(() => setWarmupToast(null), 2500);
  };

  const formatWarmupError = (err: unknown) => {
    if (!err) return "Unknown error";
    if (err instanceof Error && err.message) return err.message;
    if (typeof err === "string") return err;
    try {
      return JSON.stringify(err);
    } catch {
      return "Unknown error";
    }
  };

  const formatSwitchOutcome = (result: SwitchAccountResult) => {
    const parts: string[] = [];

    if (result.closed_vscode_windows > 0) {
      parts.push(
        result.restarted_vscode
          ? `closed VS Code (${result.closed_vscode_windows} process${
              result.closed_vscode_windows === 1 ? "" : "es"
            }) and requested reopen`
          : `closed VS Code (${result.closed_vscode_windows} process${
              result.closed_vscode_windows === 1 ? "" : "es"
            })`
      );
    }
    else if (result.restarted_vscode) {
      parts.push("requested VS Code reopen");
    }

    if (result.closed_antigravity_windows > 0) {
      parts.push(
        result.restarted_antigravity
          ? `closed Antigravity (${result.closed_antigravity_windows} process${
              result.closed_antigravity_windows === 1 ? "" : "es"
            }) and requested reopen`
          : `closed Antigravity (${result.closed_antigravity_windows} process${
              result.closed_antigravity_windows === 1 ? "" : "es"
            })`
      );
    }
    else if (result.restarted_antigravity) {
      parts.push("requested Antigravity reopen");
    }

    if (result.closed_extension_processes > 0) {
      parts.push(`closed ${result.closed_extension_processes} Codex extension worker${
        result.closed_extension_processes === 1 ? "" : "s"
      }`);
    }

    if (result.closed_codex_apps > 0) {
      parts.push(
        result.restarted_codex_app
          ? `closed Codex app (${result.closed_codex_apps}) and requested reopen`
          : `closed Codex app (${result.closed_codex_apps})`
      );
    }

    return parts.length > 0 ? `Switched account and ${parts.join(", ")}.` : "Switched account.";
  };

  const handleWarmupAccount = async (accountId: string, accountName: string) => {
    try {
      setWarmingUpId(accountId);
      appendLog("warmup", `Warm-up started for ${accountName}`, "info");
      await warmupAccount(accountId);
      appendLog("warmup", `Warm-up sent for ${accountName}`, "success");
      showWarmupToast(`Warm-up sent for ${accountName}`);
    } catch (err) {
      console.error("Failed to warm up account:", err);
      appendLog("warmup", `Warm-up failed for ${accountName}: ${formatWarmupError(err)}`, "error");
      showWarmupToast(
        `Warm-up failed for ${accountName}: ${formatWarmupError(err)}`,
        true
      );
    } finally {
      setWarmingUpId(null);
    }
  };

  const handleWarmupAll = async () => {
    try {
      setIsWarmingAll(true);
      appendLog("warmup", "Warm-up all started", "info");
      const summary = await warmupAllAccounts();
      if (summary.total_accounts === 0) {
        appendLog("warmup", "Warm-up all skipped because no accounts are available", "warn");
        showWarmupToast("No accounts available for warm-up", true);
        return;
      }

      if (summary.failed_account_ids.length === 0) {
        appendLog(
          "warmup",
          `Warm-up all completed for ${summary.warmed_accounts} account(s)`,
          "success"
        );
        showWarmupToast(
          `Warm-up sent for all ${summary.warmed_accounts} account${
            summary.warmed_accounts === 1 ? "" : "s"
          }`
        );
      } else {
        appendLog(
          "warmup",
          `Warm-up all partial failure: ${summary.failed_account_ids.length} account(s) failed`,
          "warn"
        );
        showWarmupToast(
          `Warmed ${summary.warmed_accounts}/${summary.total_accounts}. Failed: ${summary.failed_account_ids.length}`,
          true
        );
      }
    } catch (err) {
      console.error("Failed to warm up all accounts:", err);
      appendLog("warmup", `Warm-up all failed: ${formatWarmupError(err)}`, "error");
      showWarmupToast(`Warm-up all failed: ${formatWarmupError(err)}`, true);
    } finally {
      setIsWarmingAll(false);
    }
  };

  const handleExportSlimText = async () => {
    setConfigModalMode("slim_export");
    setConfigModalError(null);
    setConfigPayload("");
    setConfigCopied(false);
    setIsConfigModalOpen(true);

    try {
      setIsExportingSlim(true);
      const payload = await exportAccountsSlimText();
      setConfigPayload(payload);
      showWarmupToast(`Slim text exported (${accounts.length} accounts).`);
    } catch (err) {
      console.error("Failed to export slim text:", err);
      const message = err instanceof Error ? err.message : String(err);
      setConfigModalError(message);
      showWarmupToast("Slim export failed", true);
    } finally {
      setIsExportingSlim(false);
    }
  };

  const openImportSlimTextModal = () => {
    setConfigModalMode("slim_import");
    setConfigModalError(null);
    setConfigPayload("");
    setConfigCopied(false);
    setIsConfigModalOpen(true);
  };

  const handleImportSlimText = async () => {
    if (!configPayload.trim()) {
      setConfigModalError("Please paste the slim text string first.");
      return;
    }

    try {
      setIsImportingSlim(true);
      setConfigModalError(null);
      const summary = await importAccountsSlimText(configPayload);
      setMaskedAccounts(new Set());
      setIsConfigModalOpen(false);
      showWarmupToast(
        `Imported ${summary.imported_count}, skipped ${summary.skipped_count} (total ${summary.total_in_payload})`
      );
    } catch (err) {
      console.error("Failed to import slim text:", err);
      const message = err instanceof Error ? err.message : String(err);
      setConfigModalError(message);
      showWarmupToast("Slim import failed", true);
    } finally {
      setIsImportingSlim(false);
    }
  };

  const handleExportFullFile = async () => {
    try {
      setIsExportingFull(true);
      const selected = await save({
        title: "Export Full Encrypted Account Config",
        defaultPath: "codex-quota-monitor-full.cqm",
        filters: [
          {
            name: "Codex Quota Monitor Backup",
            extensions: ["cqm", "cswf"],
          },
        ],
      });

      if (!selected) return;

      await exportAccountsFullEncryptedFile(selected);
      showWarmupToast("Full encrypted file exported.");
    } catch (err) {
      console.error("Failed to export full encrypted file:", err);
      showWarmupToast("Full export failed", true);
    } finally {
      setIsExportingFull(false);
    }
  };

  const handleImportFullFile = async () => {
    try {
      setIsImportingFull(true);
      const selected = await open({
        multiple: false,
        title: "Import Full Encrypted Account Config",
        filters: [
          {
            name: "Codex Quota Monitor Backup",
            extensions: ["cqm"],
          },
        ],
      });

      if (!selected || Array.isArray(selected)) return;

      const summary = await importAccountsFullEncryptedFile(selected);
      setMaskedAccounts(new Set());
      showWarmupToast(
        `Imported ${summary.imported_count}, skipped ${summary.skipped_count} (total ${summary.total_in_payload})`
      );
    } catch (err) {
      console.error("Failed to import full encrypted file:", err);
      showWarmupToast("Full import failed", true);
    } finally {
      setIsImportingFull(false);
    }
  };

  const handleCheckForUpdates = async () => {
    try {
      await checkForUpdates("manual");
    } catch {
      // The hook already stores and reports the error state.
    }
  };

  const handleInstallUpdate = async () => {
    try {
      appendLog("update", "User approved update download and install", "warn");
      await installUpdate();
    } catch (err) {
      appendLog("update", `Install failed: ${formatWarmupError(err)}`, "error");
      showWarmupToast(`Update install failed: ${formatWarmupError(err)}`, true);
    }
  };

  const handleOpenReleasePage = async () => {
    if (!updateInfo.release_url) return;

    try {
      await openUrl(updateInfo.release_url);
      appendLog("update", `Opened release page ${updateInfo.release_url}`, "info");
    } catch (err) {
      appendLog("update", `Failed to open release page: ${formatWarmupError(err)}`, "error");
      showWarmupToast("Failed to open release page", true);
    }
  };

  const activeAccount = accounts.find((a) => a.is_active);
  const otherAccounts = accounts.filter((a) => !a.is_active);
  const blockingProcessCount = processInfo?.count ?? 0;
  const restartableRuntimeCount = processInfo?.background_count ?? 0;
  const hasBlockingProcesses = blockingProcessCount > 0;
  const hasRestartableRuntimes = restartableRuntimeCount > 0;
  const activeSessionCards = processInfo
    ? [
        {
          key: "cli",
          label: "Standalone CLI",
          status: hasBlockingProcesses ? "Blocking" : "Clear",
          detail: hasBlockingProcesses
            ? `${pluralize(blockingProcessCount, "process")} still running`
            : "No standalone CLI detected",
          tone: hasBlockingProcesses ? "amber" : "slate",
        },
        {
          key: "vscode",
          label: "VS Code",
          status:
            processInfo.vscode_extension_count > 0
              ? "Active"
              : processInfo.vscode_window_count > 0
                ? "Open"
                : "Closed",
          detail:
            processInfo.vscode_extension_count > 0
              ? `${pluralize(processInfo.vscode_window_count, "window")} • ${pluralize(processInfo.vscode_extension_count, "Codex worker")}`
              : processInfo.vscode_window_count > 0
                ? `${pluralize(processInfo.vscode_window_count, "window")} open • no Codex worker`
                : "No VS Code session",
          tone:
            processInfo.vscode_extension_count > 0
              ? "blue"
              : processInfo.vscode_window_count > 0
                ? "slate"
                : "slate",
        },
        {
          key: "antigravity",
          label: "Antigravity",
          status:
            processInfo.antigravity_extension_count > 0
              ? "Active"
              : processInfo.antigravity_window_count > 0
                ? "Open"
                : "Closed",
          detail:
            processInfo.antigravity_extension_count > 0
              ? `${pluralize(processInfo.antigravity_window_count, "window")} • ${pluralize(processInfo.antigravity_extension_count, "Codex worker")}`
              : processInfo.antigravity_window_count > 0
                ? `${pluralize(processInfo.antigravity_window_count, "window")} open • no Codex worker`
                : "No Antigravity session",
          tone:
            processInfo.antigravity_extension_count > 0
              ? "blue"
              : processInfo.antigravity_window_count > 0
                ? "slate"
                : "slate",
        },
        {
          key: "codex-app",
          label: "Codex app",
          status: processInfo.codex_app_count > 0 ? "Active" : "Closed",
          detail:
            processInfo.codex_app_count > 0
              ? `${pluralize(processInfo.codex_app_count, "process")} running`
              : "Codex app is closed",
          tone: processInfo.codex_app_count > 0 ? "blue" : "slate",
        },
      ]
    : [];
  const activeSessionKindCount = activeSessionCards.filter(
    (card) => card.status === "Active" || card.status === "Blocking"
  ).length;
  const activeSessionSummary = hasBlockingProcesses
    ? `${pluralize(blockingProcessCount, "CLI process")} blocking switching`
    : hasRestartableRuntimes
      ? `${pluralize(activeSessionKindCount, "active session")} detected`
      : "No active Codex session detected";
  const showUpdateCard =
    ["available", "downloading", "installing", "relaunching"].includes(updateInfo.status) &&
    updateInfo.latest_version !== null &&
    updateInfo.latest_version !== dismissedUpdateVersion;
  const publishedUpdateDate = formatRelativeReleaseDate(updateInfo.published_at);
  const updateSummary = (() => {
    if (updateInfo.status === "available") {
      return updateInfo.can_download_and_install
        ? `Version ${updateInfo.latest_version} is ready to download and install.`
        : `Version ${updateInfo.latest_version} is available, but this build needs manual install.`;
    }
    if (updateInfo.status === "checking") {
      return "Checking GitHub releases for a new production build.";
    }
    if (updateInfo.status === "downloading") {
      return updateInfo.download_percent !== null
        ? `Downloading update package: ${updateInfo.download_percent}%`
        : "Downloading update package.";
    }
    if (updateInfo.status === "installing") {
      return "Installing the downloaded update package.";
    }
    if (updateInfo.status === "relaunching") {
      return "Update installed. Restarting the app.";
    }
    if (updateInfo.status === "up_to_date") {
      return `You are already on the latest version (${updateInfo.current_version}).`;
    }
    if (updateInfo.status === "error") {
      return "Unable to verify the latest GitHub release right now.";
    }
    return "Update checks run automatically when the app starts.";
  })();

  const sortedOtherAccounts = useMemo(() => {
    const getResetDeadline = (resetAt: number | null | undefined) =>
      resetAt ?? Number.POSITIVE_INFINITY;

    const getRemainingPercent = (usedPercent: number | null | undefined) => {
      if (usedPercent === null || usedPercent === undefined) {
        return Number.NEGATIVE_INFINITY;
      }
      return Math.max(0, 100 - usedPercent);
    };

    return [...otherAccounts].sort((a, b) => {
      if (otherAccountsSort === "deadline_asc" || otherAccountsSort === "deadline_desc") {
        const deadlineDiff =
          getResetDeadline(a.usage?.primary_resets_at) -
          getResetDeadline(b.usage?.primary_resets_at);
        if (deadlineDiff !== 0) {
          return otherAccountsSort === "deadline_asc" ? deadlineDiff : -deadlineDiff;
        }
        const remainingDiff =
          getRemainingPercent(b.usage?.primary_used_percent) -
          getRemainingPercent(a.usage?.primary_used_percent);
        if (remainingDiff !== 0) return remainingDiff;
        return a.name.localeCompare(b.name);
      }

      const remainingDiff =
        getRemainingPercent(b.usage?.primary_used_percent) -
        getRemainingPercent(a.usage?.primary_used_percent);
      if (otherAccountsSort === "remaining_desc" && remainingDiff !== 0) {
        return remainingDiff;
      }
      if (otherAccountsSort === "remaining_asc" && remainingDiff !== 0) {
        return -remainingDiff;
      }
      const deadlineDiff =
        getResetDeadline(a.usage?.primary_resets_at) -
        getResetDeadline(b.usage?.primary_resets_at);
      if (deadlineDiff !== 0) return deadlineDiff;
      return a.name.localeCompare(b.name);
    });
  }, [otherAccounts, otherAccountsSort]);

  return (
    <div className="min-h-screen bg-gray-50 xl:grid xl:grid-cols-[minmax(0,1fr)_320px]">
      <div className="min-w-0">
      {/* Header */}
      <header className="sticky top-0 z-40 bg-white border-b border-gray-200">
        <div className="max-w-5xl mx-auto px-6 py-4">
          <div className="grid grid-cols-1 gap-3 md:grid-cols-[minmax(0,1fr)_max-content] md:items-center md:gap-4">
            <div className="flex items-center gap-3 min-w-0 flex-1">
              <div className="h-10 w-10 rounded-xl bg-gray-900 flex items-center justify-center text-white font-black text-xs tracking-tight">
                QM
              </div>
              <div className="min-w-0">
                <h1 className="text-xl font-bold text-gray-900 tracking-tight">
                  Codex Quota Monitor
                </h1>
                <p className="text-xs text-gray-500">
                  Quota monitoring and multi-account control for Codex
                </p>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2 shrink-0 md:ml-4 md:w-max md:flex-nowrap md:justify-end">
              <button
                onClick={toggleMaskAll}
                className="h-10 px-4 py-2 text-sm font-medium rounded-lg bg-gray-100 hover:bg-gray-200 text-gray-700 transition-colors shrink-0 whitespace-nowrap"
                title={allMasked ? "Show all account names and emails" : "Hide all account names and emails"}
              >
                <span className="flex items-center gap-2">
                  {allMasked ? (
                    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth={2}
                        d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21"
                      />
                    </svg>
                  ) : (
                    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                    </svg>
                  )}
                  {allMasked ? "Show All" : "Hide All"}
                </span>
              </button>
              <button
                onClick={handleRefresh}
                disabled={isRefreshing}
                className="h-10 px-4 py-2 text-sm font-medium rounded-lg bg-gray-100 hover:bg-gray-200 text-gray-700 transition-colors disabled:opacity-50 shrink-0 whitespace-nowrap"
              >
                {isRefreshing ? "↻ Refreshing..." : "↻ Refresh All"}
              </button>
              <button
                onClick={handleWarmupAll}
                disabled={isWarmingAll || accounts.length === 0}
                className="h-10 px-4 py-2 text-sm font-medium rounded-lg bg-gray-100 hover:bg-gray-200 text-gray-700 transition-colors disabled:opacity-50 shrink-0 whitespace-nowrap"
                title="Send minimal traffic using all accounts"
              >
                {isWarmingAll ? (
                  <span className="flex items-center gap-2">
                    <span className="animate-pulse">⚡</span> Warming...
                  </span>
                ) : (
                  <span className="flex items-center gap-2">
                    <span>⚡</span> Warm-up All
                  </span>
                )}
              </button>

              <div className="relative" ref={actionsMenuRef}>
                <button
                  onClick={() => setIsActionsMenuOpen((prev) => !prev)}
                  className="h-10 px-4 py-2 text-sm font-medium rounded-lg bg-gray-900 hover:bg-gray-800 text-white transition-colors shrink-0 whitespace-nowrap"
                >
                  Account ▾
                </button>
                {isActionsMenuOpen && (
                  <div className="absolute right-0 mt-2 w-56 rounded-xl border border-gray-200 bg-white shadow-xl p-2 z-50">
                    <button
                      onClick={() => {
                        setIsActionsMenuOpen(false);
                        setIsAddModalOpen(true);
                      }}
                      className="w-full text-left px-3 py-2 text-sm rounded-lg hover:bg-gray-100 text-gray-700"
                    >
                      + Add Account
                    </button>
                    <button
                      onClick={() => {
                        setIsActionsMenuOpen(false);
                        void handleCheckForUpdates();
                      }}
                      disabled={isCheckingForUpdates}
                      className="w-full text-left px-3 py-2 text-sm rounded-lg hover:bg-gray-100 text-gray-700 disabled:opacity-50"
                    >
                      {isCheckingForUpdates ? "Checking updates..." : "Check for Updates"}
                    </button>
                    <button
                      onClick={() => {
                        setIsActionsMenuOpen(false);
                        void handleExportSlimText();
                      }}
                      disabled={isExportingSlim}
                      className="w-full text-left px-3 py-2 text-sm rounded-lg hover:bg-gray-100 text-gray-700 disabled:opacity-50"
                    >
                      {isExportingSlim ? "Exporting..." : "Export Slim Text"}
                    </button>
                    <button
                      onClick={() => {
                        setIsActionsMenuOpen(false);
                        openImportSlimTextModal();
                      }}
                      disabled={isImportingSlim}
                      className="w-full text-left px-3 py-2 text-sm rounded-lg hover:bg-gray-100 text-gray-700 disabled:opacity-50"
                    >
                      {isImportingSlim ? "Importing..." : "Import Slim Text"}
                    </button>
                    <button
                      onClick={() => {
                        setIsActionsMenuOpen(false);
                        void handleExportFullFile();
                      }}
                      disabled={isExportingFull}
                      className="w-full text-left px-3 py-2 text-sm rounded-lg hover:bg-gray-100 text-gray-700 disabled:opacity-50"
                    >
                      {isExportingFull ? "Exporting..." : "Export Full Encrypted File"}
                    </button>
                    <button
                      onClick={() => {
                        setIsActionsMenuOpen(false);
                        void handleImportFullFile();
                      }}
                      disabled={isImportingFull}
                      className="w-full text-left px-3 py-2 text-sm rounded-lg hover:bg-gray-100 text-gray-700 disabled:opacity-50"
                    >
                      {isImportingFull ? "Importing..." : "Import Full Encrypted File"}
                    </button>
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>
      </header>

      {/* Main Content */}
      <main className="max-w-5xl mx-auto px-6 py-8">
        <section className="mb-6 rounded-2xl border border-gray-200 bg-white p-5 shadow-sm">
          <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
            <div>
              <p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-gray-500">
                App Update
              </p>
              <h2 className="mt-1 text-lg font-semibold text-gray-900">{updateSummary}</h2>
              <p className="mt-1 text-sm text-gray-500">
                Auto-check runs on startup against the latest stable GitHub release.
              </p>
            </div>
            <div className="flex items-center gap-2 self-start">
              <span
                className={`inline-flex items-center gap-2 rounded-full border px-3 py-1 text-xs font-medium ${
                  updateInfo.status === "available" ||
                  updateInfo.status === "downloading" ||
                  updateInfo.status === "installing" ||
                  updateInfo.status === "relaunching"
                    ? "border-blue-200 bg-blue-50 text-blue-700"
                    : updateInfo.status === "error"
                      ? "border-red-200 bg-red-50 text-red-700"
                      : updateInfo.status === "up_to_date"
                        ? "border-green-200 bg-green-50 text-green-700"
                        : "border-gray-200 bg-gray-50 text-gray-700"
                }`}
              >
                <span
                  className={`inline-block h-1.5 w-1.5 rounded-full ${
                    updateInfo.status === "available" ||
                    updateInfo.status === "downloading" ||
                    updateInfo.status === "installing" ||
                    updateInfo.status === "relaunching"
                      ? "bg-blue-500"
                      : updateInfo.status === "error"
                        ? "bg-red-500"
                        : updateInfo.status === "up_to_date"
                          ? "bg-green-500"
                          : "bg-gray-400"
                  }`}
                ></span>
                {updateInfo.status === "available"
                  ? "Update available"
                  : updateInfo.status === "downloading"
                    ? "Downloading"
                    : updateInfo.status === "installing"
                      ? "Installing"
                      : updateInfo.status === "relaunching"
                        ? "Restarting"
                  : updateInfo.status === "error"
                    ? "Check failed"
                    : updateInfo.status === "up_to_date"
                      ? "Up to date"
                      : updateInfo.status === "checking"
                        ? "Checking"
                        : "Idle"}
              </span>
              <button
                onClick={() => void handleCheckForUpdates()}
                disabled={isCheckingForUpdates || isInstallingUpdate}
                className="h-9 px-3 py-2 text-sm font-medium rounded-lg bg-gray-100 hover:bg-gray-200 text-gray-700 transition-colors disabled:opacity-50"
              >
                {isCheckingForUpdates ? "Checking..." : "Check now"}
              </button>
            </div>
          </div>

          {showUpdateCard && (
            <div className="mt-4 rounded-xl border border-blue-200 bg-blue-50/70 p-4">
              <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                <div>
                  <p className="text-sm font-semibold text-blue-900">
                    v{updateInfo.latest_version} is available
                  </p>
                  <p className="mt-1 text-sm text-blue-800">
                    Current version: v{updateInfo.current_version}
                    {publishedUpdateDate ? ` • Published ${publishedUpdateDate}` : ""}
                  </p>
                  {updateInfo.release_name && (
                    <p className="mt-1 text-xs uppercase tracking-[0.14em] text-blue-700/80">
                      {updateInfo.release_name}
                    </p>
                  )}
                  {updateInfo.error && (
                    <p className="mt-2 text-sm text-blue-800">{updateInfo.error}</p>
                  )}
                  {updateInfo.body && (
                    <p className="mt-2 line-clamp-3 text-sm text-blue-800/90 whitespace-pre-line">
                      {updateInfo.body}
                    </p>
                  )}
                  {updateInfo.status === "downloading" && (
                    <div className="mt-3">
                      <div className="h-2 w-full overflow-hidden rounded-full bg-blue-100">
                        <div
                          className="h-full rounded-full bg-blue-600 transition-[width]"
                          style={{
                            width: `${updateInfo.download_percent ?? 15}%`,
                          }}
                        ></div>
                      </div>
                      <p className="mt-2 text-xs text-blue-800">
                        {updateInfo.download_percent !== null
                          ? `Downloaded ${updateInfo.download_percent}%`
                          : "Downloading updater package"}
                      </p>
                    </div>
                  )}
                </div>
                <div className="flex flex-wrap gap-2">
                  {updateInfo.can_download_and_install ? (
                    <button
                      onClick={() => void handleInstallUpdate()}
                      disabled={isInstallingUpdate}
                      className="px-3 py-2 text-sm font-medium rounded-lg bg-blue-600 hover:bg-blue-700 text-white transition-colors disabled:opacity-50"
                    >
                      {updateInfo.status === "downloading"
                        ? "Downloading..."
                        : updateInfo.status === "installing"
                          ? "Installing..."
                          : updateInfo.status === "relaunching"
                            ? "Restarting..."
                            : "Download & Install"}
                    </button>
                  ) : (
                    <button
                      onClick={() => void handleOpenReleasePage()}
                      className="px-3 py-2 text-sm font-medium rounded-lg bg-blue-600 hover:bg-blue-700 text-white transition-colors"
                    >
                      Open release page
                    </button>
                  )}
                  <button
                    onClick={() => void handleOpenReleasePage()}
                    className="px-3 py-2 text-sm font-medium rounded-lg bg-white hover:bg-blue-100 text-blue-700 border border-blue-200 transition-colors"
                  >
                    View release
                  </button>
                  <button
                    onClick={() => setDismissedUpdateVersion(updateInfo.latest_version)}
                    className="px-3 py-2 text-sm font-medium rounded-lg bg-white hover:bg-blue-100 text-blue-700 border border-blue-200 transition-colors"
                  >
                    Dismiss
                  </button>
                </div>
              </div>
            </div>
          )}

          {updateInfo.status === "error" && updateInfo.error && updateInfo.source === "manual" && (
            <div className="mt-4 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
              {updateInfo.error}
            </div>
          )}
        </section>

        {processInfo && (
          <section className="mb-6 rounded-2xl border border-gray-200 bg-white p-5 shadow-sm">
            <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
              <div>
                <p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-gray-500">
                  Active Session
                </p>
                <h2 className="mt-1 text-lg font-semibold text-gray-900">{activeSessionSummary}</h2>
                <p className="mt-1 text-sm text-gray-500">
                  Runtime detail for safe switching and restart decisions.
                </p>
              </div>
              <span
                className={`inline-flex items-center gap-2 self-start rounded-full border px-3 py-1 text-xs font-medium ${
                  hasBlockingProcesses
                    ? "border-amber-200 bg-amber-50 text-amber-700"
                    : hasRestartableRuntimes
                      ? "border-blue-200 bg-blue-50 text-blue-700"
                      : "border-green-200 bg-green-50 text-green-700"
                }`}
              >
                <span
                  className={`inline-block h-1.5 w-1.5 rounded-full ${
                    hasBlockingProcesses
                      ? "bg-amber-500"
                      : hasRestartableRuntimes
                        ? "bg-blue-500"
                        : "bg-green-500"
                  }`}
                ></span>
                {hasBlockingProcesses
                  ? "Switch blocked"
                  : hasRestartableRuntimes
                    ? "Switch allowed"
                    : "Idle"}
              </span>
            </div>

            <div className="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
              {activeSessionCards.map((card) => (
                <div
                  key={card.key}
                  className={`rounded-xl border p-4 ${
                    card.tone === "amber"
                      ? "border-amber-200 bg-amber-50/60"
                      : card.tone === "blue"
                        ? "border-blue-200 bg-blue-50/60"
                        : "border-gray-200 bg-gray-50"
                  }`}
                >
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-sm font-semibold text-gray-900">{card.label}</p>
                    <span
                      className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${
                        card.tone === "amber"
                          ? "bg-amber-100 text-amber-700"
                          : card.tone === "blue"
                            ? "bg-blue-100 text-blue-700"
                            : "bg-white text-gray-600"
                      }`}
                    >
                      {card.status}
                    </span>
                  </div>
                  <p className="mt-2 text-sm text-gray-600">{card.detail}</p>
                </div>
              ))}
            </div>
          </section>
        )}

        {loading && accounts.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20">
            <div className="animate-spin h-10 w-10 border-2 border-gray-900 border-t-transparent rounded-full mb-4"></div>
            <p className="text-gray-500">Loading accounts...</p>
          </div>
        ) : error ? (
          <div className="text-center py-20">
            <div className="text-red-600 mb-2">Failed to load accounts</div>
            <p className="text-sm text-gray-500">{error}</p>
          </div>
        ) : accounts.length === 0 ? (
          <div className="text-center py-20">
            <div className="h-16 w-16 rounded-2xl bg-gray-100 flex items-center justify-center mx-auto mb-4">
              <span className="text-3xl">👤</span>
            </div>
            <h2 className="text-xl font-semibold text-gray-900 mb-2">
              No accounts yet
            </h2>
            <p className="text-gray-500 mb-6">
              Add your first Codex account to get started
            </p>
            <button
              onClick={() => setIsAddModalOpen(true)}
              className="px-6 py-3 text-sm font-medium rounded-lg bg-gray-900 hover:bg-gray-800 text-white transition-colors"
            >
              Add Account
            </button>
          </div>
        ) : (
          <div className="space-y-8">
            {/* Active Account */}
            {activeAccount && (
              <section>
                <h2 className="text-sm font-medium text-gray-500 uppercase tracking-wider mb-4">
                  Active Account
                </h2>
                <AccountCard
                  account={activeAccount}
                  onSwitch={() => { }}
                  onWarmup={() =>
                    handleWarmupAccount(activeAccount.id, activeAccount.name)
                  }
                  onDelete={() => handleDelete(activeAccount.id)}
                  onRefresh={() => refreshSingleUsage(activeAccount.id)}
                  onRename={(newName) => renameAccount(activeAccount.id, newName)}
                  switching={switchingId === activeAccount.id}
                  switchDisabled={hasBlockingProcesses}
                  warmingUp={isWarmingAll || warmingUpId === activeAccount.id}
                  masked={maskedAccounts.has(activeAccount.id)}
                  onToggleMask={() => toggleMask(activeAccount.id)}
                />
              </section>
            )}

            {/* Other Accounts */}
            {otherAccounts.length > 0 && (
              <section>
                <div className="flex items-center justify-between gap-3 mb-4">
                  <h2 className="text-sm font-medium text-gray-500 uppercase tracking-wider">
                    Other Accounts ({otherAccounts.length})
                  </h2>
                  <div className="flex items-center gap-2">
                    <label htmlFor="other-accounts-sort" className="text-xs text-gray-500">
                      Sort
                    </label>
                    <div className="relative">
                      <select
                        id="other-accounts-sort"
                        value={otherAccountsSort}
                        onChange={(e) =>
                          setOtherAccountsSort(
                            e.target.value as
                              | "deadline_asc"
                              | "deadline_desc"
                              | "remaining_desc"
                              | "remaining_asc"
                          )
                        }
                        className="appearance-none font-sans text-xs sm:text-sm font-medium pl-3 pr-9 py-2 rounded-xl border border-gray-300 bg-gradient-to-b from-white to-gray-50 text-gray-700 shadow-sm hover:border-gray-400 hover:shadow focus:outline-none focus:ring-2 focus:ring-gray-300 focus:border-gray-400 transition-all"
                      >
                        <option value="deadline_asc">Reset: earliest to latest</option>
                        <option value="deadline_desc">Reset: latest to earliest</option>
                        <option value="remaining_desc">
                          % remaining: highest to lowest
                        </option>
                        <option value="remaining_asc">
                          % remaining: lowest to highest
                        </option>
                      </select>
                      <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center text-gray-500">
                        <svg
                          className="h-4 w-4"
                          viewBox="0 0 20 20"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="2"
                        >
                          <path d="M6 8l4 4 4-4" strokeLinecap="round" strokeLinejoin="round" />
                        </svg>
                      </span>
    </div>
                  </div>
                </div>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  {sortedOtherAccounts.map((account) => (
                    <AccountCard
                      key={account.id}
                      account={account}
                      onSwitch={() => handleSwitch(account.id)}
                      onWarmup={() => handleWarmupAccount(account.id, account.name)}
                      onDelete={() => handleDelete(account.id)}
                      onRefresh={() => refreshSingleUsage(account.id)}
                      onRename={(newName) => renameAccount(account.id, newName)}
                      switching={switchingId === account.id}
                      switchDisabled={hasBlockingProcesses}
                      warmingUp={isWarmingAll || warmingUpId === account.id}
                      masked={maskedAccounts.has(account.id)}
                      onToggleMask={() => toggleMask(account.id)}
                    />
                  ))}
                </div>
              </section>
            )}
          </div>
        )}
      </main>

      {/* Refresh Success Toast */}
      {refreshSuccess && (
        <div className="fixed bottom-6 left-1/2 -translate-x-1/2 px-4 py-3 bg-green-600 text-white rounded-lg shadow-lg text-sm flex items-center gap-2">
          <span>✓</span> Usage refreshed successfully
        </div>
      )}

      {/* Warm-up Toast */}
      {warmupToast && (
        <div
          className={`fixed bottom-20 left-1/2 -translate-x-1/2 px-4 py-3 rounded-lg shadow-lg text-sm ${
            warmupToast.isError
              ? "bg-red-600 text-white"
              : "bg-amber-100 text-amber-900 border border-amber-300"
          }`}
        >
          {warmupToast.message}
        </div>
      )}

      {/* Delete Confirmation Toast */}
      {deleteConfirmId && (
        <div className="fixed bottom-6 left-1/2 -translate-x-1/2 px-4 py-3 bg-red-600 text-white rounded-lg shadow-lg text-sm">
          Click delete again to confirm removal
        </div>
      )}

      {/* Add Account Modal */}
      <AddAccountModal
        isOpen={isAddModalOpen}
        onClose={() => setIsAddModalOpen(false)}
        onImportFile={importFromFile}
        onStartOAuth={startOAuthLogin}
        onCompleteOAuth={completeOAuthLogin}
        onCancelOAuth={cancelOAuthLogin}
      />

      {/* Import/Export Config Modal */}
      {isConfigModalOpen && (
        <div className="fixed inset-0 bg-black/40 flex items-center justify-center z-50">
          <div className="bg-white border border-gray-200 rounded-2xl w-full max-w-2xl mx-4 shadow-xl">
            <div className="flex items-center justify-between p-5 border-b border-gray-100">
              <h2 className="text-lg font-semibold text-gray-900">
                {configModalMode === "slim_export" ? "Export Slim Text" : "Import Slim Text"}
              </h2>
              <button
                onClick={() => setIsConfigModalOpen(false)}
                className="text-gray-400 hover:text-gray-600 transition-colors"
              >
                ✕
              </button>
            </div>
            <div className="p-5 space-y-4">
              {configModalMode === "slim_import" ? (
                <p className="text-sm text-amber-700 bg-amber-50 border border-amber-200 rounded-lg px-3 py-2">
                  Existing accounts are kept. Only missing accounts are imported.
                </p>
              ) : (
                <p className="text-sm text-gray-500">
                  This slim string contains account secrets. Keep it private.
                </p>
              )}
              <textarea
                value={configPayload}
                onChange={(e) => setConfigPayload(e.target.value)}
                readOnly={configModalMode === "slim_export"}
                placeholder={
                  configModalMode === "slim_export"
                    ? isExportingSlim
                      ? "Generating..."
                      : "Export string will appear here"
                    : "Paste config string here"
                }
                className="w-full h-48 px-4 py-3 bg-gray-50 border border-gray-200 rounded-lg text-sm text-gray-800 placeholder-gray-400 focus:outline-none focus:border-gray-400 focus:ring-1 focus:ring-gray-400 font-mono"
              />
              {configModalError && (
                <div className="p-3 bg-red-50 border border-red-200 rounded-lg text-red-600 text-sm">
                  {configModalError}
                </div>
              )}
            </div>
            <div className="flex gap-3 p-5 border-t border-gray-100">
              <button
                onClick={() => setIsConfigModalOpen(false)}
                className="px-4 py-2.5 text-sm font-medium rounded-lg bg-gray-100 hover:bg-gray-200 text-gray-700 transition-colors"
              >
                Close
              </button>
              {configModalMode === "slim_export" ? (
                <button
                  onClick={async () => {
                    if (!configPayload) return;
                    try {
                      await navigator.clipboard.writeText(configPayload);
                      setConfigCopied(true);
                      setTimeout(() => setConfigCopied(false), 1500);
                    } catch {
                      setConfigModalError("Clipboard unavailable. Please copy manually.");
                    }
                  }}
                  disabled={!configPayload || isExportingSlim}
                  className="px-4 py-2.5 text-sm font-medium rounded-lg bg-gray-900 hover:bg-gray-800 text-white transition-colors disabled:opacity-50"
                >
                  {configCopied ? "Copied" : "Copy String"}
                </button>
              ) : (
                <button
                  onClick={handleImportSlimText}
                  disabled={isImportingSlim}
                  className="px-4 py-2.5 text-sm font-medium rounded-lg bg-gray-900 hover:bg-gray-800 text-white transition-colors disabled:opacity-50"
                >
                  {isImportingSlim ? "Importing..." : "Import Missing Accounts"}
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      </div>
      <LogPanel entries={logEntries} onClear={handleClearLogs} />
    </div>
  );
}

export default App;
