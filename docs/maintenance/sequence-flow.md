# Luồng xử lý chính

## 1. Thêm account bằng OAuth

```mermaid
sequenceDiagram
  participant U as User
  participant UI as React UI
  participant CMD as Tauri OAuth commands
  participant OAUTH as auth.openai.com
  participant CB as Local callback server
  participant STORE as accounts.json
  participant AUTH as ~/.codex/auth.json

  U->>UI: Nhập tên account và bấm Add
  UI->>CMD: start_login(account_name)
  CMD-->>UI: OAuthLoginInfo(auth_url, callback_port)
  UI->>OAUTH: Mở browser tới auth_url
  OAUTH->>CB: Redirect về local callback
  UI->>CMD: complete_login()
  CMD->>OAUTH: Exchange code lấy token
  CMD->>STORE: add_account + set_active_account + touch_account
  CMD->>AUTH: switch_to_account()
  CMD-->>UI: AccountInfo
  UI->>UI: cập nhật danh sách account và active account
```

## 2. Import account từ `auth.json`

```mermaid
sequenceDiagram
  participant U as User
  participant UI as AddAccountModal
  participant CMD as account commands
  participant CA as codex_auth.rs
  participant STORE as accounts.json

  U->>UI: Chọn file auth.json và tên account
  UI->>CMD: add_account_from_file(path, name)
  CMD->>CA: import_from_auth_json(path, name)
  CA-->>CMD: StoredAccount
  CMD->>STORE: add_account()
  CMD-->>UI: AccountInfo
  UI->>UI: refresh danh sách account
```

## 3. Refresh usage cho một account

```mermaid
sequenceDiagram
  participant U as User
  participant UI as AccountCard/App
  participant CMD as usage commands
  participant API as api/usage.rs
  participant CHATGPT as ChatGPT backend
  participant REFRESH as token_refresh.rs

  U->>UI: Bấm Refresh
  UI->>CMD: get_usage(account_id)
  CMD->>API: get_account_usage(account)
  API->>CHATGPT: GET usage
  alt token hết hạn
    CHATGPT-->>API: 401
    API->>REFRESH: refresh_access_token_if_needed()
    API->>CHATGPT: retry GET usage
  end
  API-->>CMD: UsageInfo
  CMD-->>UI: UsageInfo
  UI->>UI: cập nhật progress bar và reset time
```

## 4. Switch account và restart runtime

```mermaid
sequenceDiagram
  participant U as User
  participant UI as App/useAccounts
  participant CMD as switch_account
  participant RT as runtime.rs
  participant STORE as accounts.json
  participant AUTH as ~/.codex/auth.json
  participant OS as OS processes

  U->>UI: Bấm Switch & Restart
  UI->>CMD: switch_account(account_id)
  CMD->>RT: inspect_runtime_state()
  alt có standalone CLI
    RT-->>CMD: blocking_cli_pids != empty
    CMD-->>UI: Err("đóng standalone Codex CLI trước khi switch")
    UI->>UI: hiện lỗi, không switch
  else không có CLI blocker
    CMD->>AUTH: ghi auth.json mới
    CMD->>STORE: set_active_account + touch_account
    CMD->>OS: đóng extension worker / editor / codex app nếu cần
    CMD-->>UI: SwitchAccountResult
    par background relaunch
      CMD->>RT: wait_for_processes_to_exit()
      RT->>OS: relaunch_vscode() nếu có VS Code extension runtime
      RT->>OS: relaunch_antigravity() nếu có Antigravity extension runtime
      RT->>OS: relaunch_codex_app() nếu có Codex app
    and UI sync
      UI->>UI: optimistic active account update
      UI->>UI: loadAccounts() và refresh runtime panel nền
    end
  end
```

## 5. Slim import nhiều account

```mermaid
sequenceDiagram
  participant U as User
  participant UI as App
  participant CMD as import_accounts_slim_text
  participant PARSE as slim parser
  participant REFRESH as token_refresh.rs
  participant STORE as accounts.json

  U->>UI: Paste slim payload và bấm Import
  UI->>CMD: import_accounts_slim_text(payload)
  CMD->>PARSE: validate prefix + base64 + zlib + JSON
  loop từng account trong payload
    alt account API key
      CMD->>STORE: add_account()
    else account ChatGPT
      CMD->>REFRESH: create_chatgpt_account_from_refresh_token()
      REFRESH-->>CMD: StoredAccount
      CMD->>STORE: add_account()
    end
  end
  CMD-->>UI: ImportAccountsSummary
  UI->>UI: toast kết quả và reload danh sách
```

## 6. Check update và cài bản mới

```mermaid
sequenceDiagram
  participant UI as useAppUpdate
  participant UPD as Tauri updater plugin
  participant GH as GitHub latest.json / Releases
  participant U as User
  participant APP as Current app

  UI->>UPD: check()
  alt updater artifact hợp lệ
    UPD->>GH: lấy latest.json
    GH-->>UPD: metadata release
    UPD-->>UI: update available
  else fallback
    UI->>GH: GitHub release API
    GH-->>UI: latest release metadata
  end

  U->>UI: Bấm Download & Install
  UI->>UPD: downloadAndInstall()
  UPD->>GH: tải installer/update artifact
  GH-->>UPD: gói update đã ký
  UPD-->>UI: install completed
  UI->>APP: relaunch()
```

## 7. Luồng production release

```mermaid
sequenceDiagram
  participant DEV as Developer
  participant GH as GitHub
  participant CI as build.yml
  participant TAG as Release tag
  participant REL as GitHub Release

  DEV->>GH: Push code vào main
  GH->>CI: Trigger workflow Build & Release
  CI->>CI: đọc version từ package.json
  CI->>CI: validate version đồng bộ với Cargo.toml và tauri.conf.json
  alt tag đã tồn tại
    CI-->>GH: skip release
  else tag chưa tồn tại
    CI->>CI: build Windows installer và updater artifacts
    CI->>TAG: tạo tag vX.Y.Z
    CI->>REL: publish release + assets + latest.json
  end
```

## 8. Ghi chú vận hành

Hai điểm ảnh hưởng mạnh đến bảo trì:

- `switch_account` trả về trước khi mọi runtime reopen hoàn tất, nên UI và background relaunch không hoàn toàn đồng bộ
- `runtime.rs` dựa trên heuristic process classification, nên cùng một luồng có thể khác nhau giữa môi trường cài đặt khác nhau
