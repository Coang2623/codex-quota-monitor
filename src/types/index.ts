// Types matching the Rust backend

export type AuthMode = "api_key" | "chat_gpt";

export interface AccountInfo {
  id: string;
  name: string;
  email: string | null;
  plan_type: string | null;
  auth_mode: AuthMode;
  is_active: boolean;
  created_at: string;
  last_used_at: string | null;
}

export interface UsageInfo {
  account_id: string;
  plan_type: string | null;
  primary_used_percent: number | null;
  primary_window_minutes: number | null;
  primary_resets_at: number | null;
  secondary_used_percent: number | null;
  secondary_window_minutes: number | null;
  secondary_resets_at: number | null;
  has_credits: boolean | null;
  unlimited_credits: boolean | null;
  credits_balance: string | null;
  error: string | null;
}

export interface OAuthLoginInfo {
  auth_url: string;
  callback_port: number;
}

export interface AccountWithUsage extends AccountInfo {
  usage?: UsageInfo;
  usageLoading?: boolean;
}

export interface CodexProcessInfo {
  count: number;
  background_count: number;
  can_switch: boolean;
  pids: number[];
  vscode_window_count: number;
  vscode_extension_count: number;
  antigravity_window_count: number;
  antigravity_extension_count: number;
  codex_app_count: number;
}

export type AppLogLevel = "info" | "success" | "warn" | "error";

export interface AppLogEntry {
  id: string | number;
  timestamp_ms: number;
  level: AppLogLevel | string;
  scope: string;
  message: string;
  source?: "backend" | "frontend";
}

export interface WarmupSummary {
  total_accounts: number;
  warmed_accounts: number;
  failed_account_ids: string[];
}

export interface ImportAccountsSummary {
  total_in_payload: number;
  imported_count: number;
  skipped_count: number;
}

export interface SwitchAccountResult {
  closed_extension_processes: number;
  closed_vscode_windows: number;
  restarted_vscode: boolean;
  closed_antigravity_windows: number;
  restarted_antigravity: boolean;
  closed_codex_apps: number;
  restarted_codex_app: boolean;
}

export type AppUpdateStatus =
  | "idle"
  | "checking"
  | "available"
  | "up_to_date"
  | "downloading"
  | "installing"
  | "relaunching"
  | "error";
export type AppUpdateCheckSource = "auto" | "manual" | null;

export interface AppUpdateInfo {
  status: AppUpdateStatus;
  current_version: string | null;
  latest_version: string | null;
  release_name: string | null;
  release_url: string | null;
  published_at: string | null;
  body: string | null;
  error: string | null;
  checked_at: number | null;
  source: AppUpdateCheckSource;
  can_download_and_install: boolean;
  downloaded_bytes: number;
  content_length: number | null;
  download_percent: number | null;
}
