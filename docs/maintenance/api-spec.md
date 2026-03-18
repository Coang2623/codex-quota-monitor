# Đặc tả API và contract nội bộ

## 1. Phạm vi

Tài liệu này mô tả hai lớp API đang tồn tại trong dự án:

- API nội bộ giữa frontend React và backend Rust thông qua `Tauri invoke`
- API ngoài hệ thống mà backend hoặc updater gọi tới

Ứng dụng không có REST API riêng. Toàn bộ integration nội bộ đi qua `invoke`, còn integration ra ngoài đi trực tiếp từ process desktop.

## 2. API nội bộ qua Tauri command

### 2.1 Account commands

| Command | Input chính | Output chính | Mục đích |
| --- | --- | --- | --- |
| `list_accounts` | không có | `AccountInfo[]` | Trả toàn bộ danh sách account trong `accounts.json` |
| `get_active_account_info` | không có | `AccountInfo \| null` | Trả account đang active trong store |
| `add_account_from_file` | `path`, `account_name` | `AccountInfo` | Import account từ file `auth.json` hiện có |
| `switch_account` | `account_id` | `SwitchAccountResult` | Ghi `~/.codex/auth.json`, cập nhật active account, đóng và mở lại runtime liên quan |
| `delete_account` | `account_id` | `()` | Xóa account khỏi store; không có soft delete |
| `rename_account` | `account_id`, `new_name` | `AccountInfo` | Đổi tên account trong store |
| `export_accounts_slim_text` | không có | `string` | Export payload gọn chứa token tối thiểu |
| `import_accounts_slim_text` | `payload` | `ImportAccountsSummary` | Import nhiều account từ slim payload |
| `export_accounts_full_encrypted_file` | không có | `Vec<u8>` | Export toàn bộ cấu hình account dạng file mã hóa |
| `import_accounts_full_encrypted_file` | `data` | `ImportAccountsSummary` | Import full backup mã hóa |

### 2.2 OAuth commands

| Command | Input chính | Output chính | Mục đích |
| --- | --- | --- | --- |
| `start_login` | `account_name` | `OAuthLoginInfo` | Tạo OAuth session, local callback server và auth URL |
| `complete_login` | không có | `AccountInfo` | Chờ callback, exchange token, lưu account mới và set active |
| `cancel_login` | không có | `()` | Hủy flow OAuth đang chờ |

### 2.3 Usage commands

| Command | Input chính | Output chính | Mục đích |
| --- | --- | --- | --- |
| `get_usage` | `account_id` | `UsageInfo` | Lấy usage của một account |
| `refresh_all_accounts_usage` | không có | `Vec<UsageInfo>` | Refresh usage đồng loạt cho toàn bộ account |
| `warmup_account` | `account_id` | `()` | Gửi request warm-up cho account |
| `warmup_all_accounts` | không có | `WarmupSummary` | Warm-up toàn bộ account |

### 2.4 Runtime và log commands

| Command | Input chính | Output chính | Mục đích |
| --- | --- | --- | --- |
| `check_codex_processes` | không có | `CodexProcessInfo` | Chụp snapshot runtime hiện tại để UI biết trạng thái switch |
| `get_recent_logs` | không có | `AppLogEntry[]` | Trả log runtime gần nhất đang giữ trong memory |
| `clear_logs` | không có | `()` | Xóa ring buffer log |

## 3. Contract dữ liệu chính

### 3.1 `AccountInfo`

Model account gửi lên UI gồm:

- `id`
- `name`
- `email`
- `plan_type`
- `auth_mode`
- `is_active`
- `created_at`
- `last_used_at`

Đây là DTO hiển thị, không chứa token bí mật.

### 3.2 `UsageInfo`

Payload usage hiện phản ánh trực tiếp 2 cửa sổ quota mà UI đang dùng:

- cụm primary: `primary_used_percent`, `primary_window_minutes`, `primary_resets_at`
- cụm secondary: `secondary_used_percent`, `secondary_window_minutes`, `secondary_resets_at`
- cụm credits: `has_credits`, `unlimited_credits`, `credits_balance`
- `error` để frontend hiển thị lỗi thay vì fail toàn màn hình

Account dùng API key hiện không có usage thật; backend trả `error` mô tả giới hạn này.

### 3.3 `CodexProcessInfo`

Contract này là snapshot rút gọn của `RuntimeState` để UI render `Active Session`:

- `count`: số standalone CLI blocker
- `background_count`: số runtime có thể restart được
- `can_switch`: `true` khi không có standalone CLI blocker
- `pids`: danh sách PID blocker
- `vscode_window_count`
- `vscode_extension_count`
- `antigravity_window_count`
- `antigravity_extension_count`
- `codex_app_count`

Điểm quan trọng: `count` không phải tổng số runtime đang mở, mà chỉ là số blocker CLI.

### 3.4 `SwitchAccountResult`

Kết quả switch được backend trả sau khi đã hoàn tất phần đồng bộ auth và lên lịch reopen:

- `closed_extension_processes`
- `closed_vscode_windows`
- `restarted_vscode`
- `closed_antigravity_windows`
- `restarted_antigravity`
- `closed_codex_apps`
- `restarted_codex_app`

Frontend dùng contract này để toast trạng thái restart thay vì tự suy luận.

### 3.5 `AppUpdateInfo`

Contract updater trên frontend phản ánh cả mode check lẫn mode download/install:

- metadata version: `current_version`, `latest_version`, `release_name`, `release_url`, `published_at`
- trạng thái: `status`, `source`, `checked_at`, `error`
- trạng thái tải: `can_download_and_install`, `downloaded_bytes`, `content_length`, `download_percent`

## 4. API ngoài hệ thống

### 4.1 OAuth của OpenAI

| Endpoint/host | Hướng gọi | Mục đích |
| --- | --- | --- |
| `https://auth.openai.com` | Backend Rust | Khởi tạo OAuth flow và exchange authorization code lấy token |
| local callback server `http://127.0.0.1:<port>` | Browser -> app | Nhận callback OAuth từ OpenAI |

Đặc điểm:

- dùng PKCE
- scope hiện có `openid profile email offline_access`
- default callback port là `1455`, có fallback port ngẫu nhiên nếu bận

### 4.2 ChatGPT backend API

| Endpoint/host | Hướng gọi | Mục đích |
| --- | --- | --- |
| `https://chatgpt.com/backend-api/wham/usage` | Backend Rust | Lấy usage quota cho account ChatGPT |
| `https://chatgpt.com/backend-api/codex/responses` | Backend Rust | Warm-up request cho account ChatGPT |

Đặc điểm:

- dùng bearer access token lấy từ ChatGPT auth
- nếu gặp `401`, backend sẽ thử refresh token rồi retry một lần

### 4.3 OpenAI API

| Endpoint/host | Hướng gọi | Mục đích |
| --- | --- | --- |
| `https://api.openai.com/v1/responses` | Backend Rust | Warm-up request cho account API key |

Đặc điểm:

- áp dụng cho account `api_key`
- backend set user agent `codex-cli/1.0.0`

### 4.4 GitHub release endpoint cho updater

| Endpoint/host | Hướng gọi | Mục đích |
| --- | --- | --- |
| `https://github.com/Coang2623/codex-quota-monitor/releases/latest/download/latest.json` | Tauri updater | Lấy metadata updater chính thức |
| GitHub Releases API | Frontend fallback | Check version mới khi updater artifact chưa sẵn sàng |

## 5. Giao tiếp realtime nội bộ

Ngoài `invoke`, backend còn phát event runtime log:

- event name: `app-log`
- nguồn dữ liệu: `AppLogState`
- consumer: `LogPanel` và state log trong `App.tsx`

Đây là event fire-and-forget, không có ack channel.

## 6. Quy ước lỗi

Hiện dự án không có một error envelope thống nhất. Mẫu phổ biến là:

- backend dùng `Result<T, String>`
- message lỗi được build trực tiếp từ error chain hoặc copy logic nghiệp vụ
- frontend hiển thị bằng toast, inline text hoặc log

Hệ quả bảo trì:

- khó phân loại lỗi theo mã
- khó i18n
- khó viết retry policy có cấu trúc

## 7. Nhận xét bảo trì

API nội bộ của dự án hiện còn gọn và trực tiếp, nhưng contract bắt đầu tăng nhanh ở ba vùng:

- `switch_account`
- `check_codex_processes`
- `useAppUpdate`

Nếu tiếp tục mở rộng sang thêm editor/runtime hoặc policy phức tạp hơn, nên cân nhắc:

- tách DTO riêng cho runtime/editor session
- chuẩn hóa error code
- gom nhóm command theo service boundary rõ hơn
