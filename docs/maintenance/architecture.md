# Kiến trúc hệ thống

## 1. Tổng quan

`Codex Quota Monitor` là một desktop application dùng `Tauri 2` để đóng gói:

- frontend React 19 + TypeScript + Vite
- backend Rust chạy trong process Tauri
- cơ chế gọi lệnh nội bộ bằng `invoke`
- logic theo dõi usage, quản lý nhiều tài khoản, switch account và restart runtime Codex

Ứng dụng không có backend service riêng của dự án. Thay vào đó, backend Rust gọi trực tiếp:

- OpenAI OAuth endpoints
- ChatGPT backend API
- OpenAI API
- GitHub release endpoint cho updater
- process list và process launcher của hệ điều hành

## 2. Sơ đồ thành phần

```mermaid
flowchart LR
  UI[React UI\nsrc/App.tsx + components] --> Hooks[Hooks\nuseAccounts / useAppUpdate]
  Hooks --> TauriInvoke[Tauri invoke]
  TauriInvoke --> Commands[commands/*]

  Commands --> Auth[auth/*]
  Commands --> Runtime[runtime.rs]
  Commands --> UsageApi[api/usage.rs]
  Commands --> Logs[app_logging.rs]

  Auth --> AccountsJson[~/.codex-quota-monitor/accounts.json]
  Auth --> CodexAuth[~/.codex/auth.json]
  Auth --> OAuth[auth.openai.com]

  UsageApi --> ChatGPT[chatgpt.com/backend-api]
  UsageApi --> OpenAI[api.openai.com]

  Runtime --> OS[Windows/macOS/Linux process APIs]
  UI --> Updater[@tauri-apps/plugin-updater]
  Updater --> GitHub[GitHub Releases / latest.json]
```

## 3. Thành phần frontend

### 3.1 App shell

`src/App.tsx` là orchestration layer chính của frontend. File này đang nắm nhiều trách nhiệm:

- layout tổng thể của app
- state UI mức cao
- orchestration các thao tác account, switch, warm-up, import/export, updater
- runtime log sync từ backend
- active session panel
- update card và actions menu

Điều này khiến `App.tsx` đóng vai trò controller lớn hơn là chỉ view composition.

### 3.2 Hooks chính

`src/hooks/useAccounts.ts`

- load danh sách account bằng `list_accounts`
- refresh usage đồng thời cho nhiều account
- gọi các command account / usage / oauth
- cập nhật active account optimistic sau `switch_account`

`src/hooks/useAppUpdate.ts`

- tự check update lúc startup khi không ở dev mode
- ưu tiên dùng `@tauri-apps/plugin-updater`
- fallback sang GitHub REST API nếu updater artifact chưa sẵn sàng
- cầm `pendingUpdateRef` để thực hiện `downloadAndInstall` và `relaunch`

### 3.3 Component UI

`src/components/AccountCard.tsx`

- card cho active account và other accounts
- hiển thị usage bar, plan badge, rename inline, switch/warm-up/refresh/delete

`src/components/AddAccountModal.tsx`

- thêm account qua OAuth hoặc import file `auth.json`

`src/components/UsageBar.tsx`

- hiển thị 2 cửa sổ rate limit chính: `5h` và `Weekly`

`src/components/LogPanel.tsx`

- panel log runtime ở cột phải
- chỉ hiển thị trên breakpoint `xl`

## 4. Thành phần backend

### 4.1 Entrypoint và plugin

`src-tauri/src/lib.rs`

- khởi tạo Tauri builder
- đăng ký plugin `opener`, `dialog`, `process`, `updater`
- inject state `AppLogState`
- khai báo toàn bộ Tauri command dùng bởi frontend

### 4.2 Command layer

`src-tauri/src/commands/account.rs`

- list account, active account
- import account từ `auth.json`
- switch account
- delete / rename
- export/import slim text
- export/import full encrypted file

`src-tauri/src/commands/oauth.rs`

- start login
- complete login
- cancel login

`src-tauri/src/commands/usage.rs`

- get usage cho 1 account
- refresh usage toàn bộ
- warm-up 1 account hoặc tất cả

`src-tauri/src/commands/process.rs`

- trả về snapshot runtime để UI biết có thể switch hay không

### 4.3 Auth layer

`src-tauri/src/auth/storage.rs`

- quản lý file `accounts.json`
- migrate legacy từ `~/.codex-switcher/accounts.json`
- add/remove/set active/touch/update metadata

`src-tauri/src/auth/codex_auth.rs`

- ghi credentials sang `~/.codex/auth.json`
- đọc/import `auth.json` hiện có
- parse JWT claims để lấy email và plan type

`src-tauri/src/auth/oauth_server.rs`

- tự dựng local callback server bằng `tiny_http`
- dùng PKCE cho OAuth flow
- exchange code lấy token
- tạo `StoredAccount` mới từ token nhận được

`src-tauri/src/auth/token_refresh.rs`

- refresh access token khi gần hết hạn
- đồng bộ `~/.codex/auth.json` nếu account đang active
- tạo lại account ChatGPT từ refresh token khi slim import

### 4.4 Usage API layer

`src-tauri/src/api/usage.rs`

- gọi `chatgpt.com/backend-api/wham/usage` để lấy usage
- gọi `chatgpt.com/backend-api/codex/responses` để warm-up account ChatGPT
- gọi `api.openai.com/v1/responses` để warm-up account API key
- tự refresh token và retry khi ChatGPT trả `401`

### 4.5 Runtime orchestration

`src-tauri/src/runtime.rs`

- chụp process snapshot theo OS
- classify process thành:
  - standalone Codex CLI blocking
  - VS Code extension runtime
  - Antigravity extension runtime
  - VS Code window/process
  - Antigravity window/process
  - Codex app
- terminate process theo PID
- đợi process cũ thoát
- relaunch runtime tương ứng

Đây là module có tính platform heuristic cao nhất trong hệ thống.

### 4.6 Logging nội bộ

`src-tauri/src/app_logging.rs`

- giữ ring buffer log trong memory
- phát event `app-log` sang frontend
- cho phép frontend lấy log gần nhất hoặc clear log

## 5. Persistence và trạng thái hệ thống

Ứng dụng hiện dùng ba lớp trạng thái chính:

1. `accounts.json` của chính app
2. `auth.json` của Codex CLI/App/Extension
3. trạng thái runtime snapshot từ process list

Điểm quan trọng là `accounts.json` là nguồn sự thật cho danh sách account, còn `~/.codex/auth.json` là output vận hành được ghi lại mỗi khi switch.

## 6. Updater và release architecture

`src-tauri/tauri.conf.json`

- bật `createUpdaterArtifacts`
- cấu hình updater endpoint tới `releases/latest/download/latest.json`
- nhúng public key cho updater

`.github/workflows/build.yml`

- production release chỉ trigger trên `push` vào `main`
- hiện chỉ build Windows x64
- version release lấy từ `package.json`
- validate version alignment giữa `package.json`, `Cargo.toml`, `tauri.conf.json`

## 7. Điểm nối quan trọng giữa các lớp

- UI gọi command qua `invoke`
- command layer gọi auth/runtime/api/storage trực tiếp, không có service layer trung gian rõ ràng
- `switch_account` là điểm tụ của nhiều concern:
  - auth write
  - state update
  - runtime inspect
  - process terminate
  - background relaunch
  - app logging
- updater cũng là cross-cutting concern giữa UI, Tauri plugin, GitHub releases và CI/CD

## 8. Kết luận kiến trúc

Kiến trúc hiện tại thiên về một desktop utility gọn, ít abstraction, tập trung delivery. Cách này giúp tốc độ phát triển nhanh ở giai đoạn đầu, nhưng đã bắt đầu lộ dấu hiệu coupling cao tại `App.tsx`, `account.rs` và `runtime.rs`. Đây là ba điểm cần đặc biệt chú ý trước khi thêm những feature lớn hơn như multi-workspace support, richer policy engine hoặc cross-platform release đầy đủ.
