# Đặc tả dữ liệu và persistence

## 1. Tổng quan

Dự án hiện không dùng DBMS như PostgreSQL, MySQL hay SQLite làm application database. Persistence được xây trên file hệ điều hành và một số định dạng backup riêng.

Ba khu vực dữ liệu chính:

- file store nội bộ của ứng dụng: `~/.codex-quota-monitor/accounts.json`
- file auth vận hành của Codex runtime: `~/.codex/auth.json`
- payload backup/import-export: slim text và full encrypted file

## 2. File store chính của ứng dụng

### 2.1 Vị trí

Config directory hiện tại:

- Windows: `%USERPROFILE%\\.codex-quota-monitor`
- path logic theo code: thư mục home + `.codex-quota-monitor`

Legacy directory được hỗ trợ migrate:

- `%USERPROFILE%\\.codex-switcher`

### 2.2 File chính

Tên file store:

- `accounts.json`

### 2.3 Cấu trúc logic

Root object là `AccountsStore`:

| Field | Kiểu | Ý nghĩa |
| --- | --- | --- |
| `version` | `u32` | Version schema nội bộ của store |
| `accounts` | `StoredAccount[]` | Danh sách account đã lưu |
| `active_account_id` | `string \| null` | Account đang active trong app |

`StoredAccount` gồm:

| Field | Kiểu | Ý nghĩa |
| --- | --- | --- |
| `id` | `string` | UUID/account id nội bộ |
| `name` | `string` | Tên hiển thị trong UI |
| `email` | `string \| null` | Email suy ra từ token nếu có |
| `plan_type` | `string \| null` | Team/Plus/Pro... nếu parse được |
| `auth_mode` | `api_key \| chat_gpt` | Loại xác thực |
| `auth_data` | object | Token hoặc API key thực tế |
| `created_at` | RFC3339 string | Thời điểm tạo account |
| `last_used_at` | RFC3339 string/null | Thời điểm gần nhất được active |

`auth_data` hiện có 2 shape:

- API key: `key`
- ChatGPT: `id_token`, `access_token`, `refresh_token`, `account_id`

## 3. File auth của Codex runtime

### 3.1 Vị trí

Path đích khi switch account:

- `~/.codex/auth.json`

Path này có thể bị override bằng `CODEX_HOME`.

### 3.2 Vai trò

Đây không phải source of truth cho danh sách account của ứng dụng. Nó là output vận hành để Codex CLI, Codex app hoặc extension đọc account đang active.

Phân chia trách nhiệm:

- `accounts.json`: lưu mọi account mà app quản lý
- `auth.json`: phản ánh account đang được publish ra runtime bên ngoài

### 3.3 Dữ liệu ghi ra

Backend chuyển `StoredAccount` sang `AuthDotJson` rồi ghi trực tiếp ra file. Với ChatGPT account, file này chứa token runtime thật.

## 4. Migration dữ liệu legacy

`storage.rs` hiện hỗ trợ tự migrate khi:

- file mới `~/.codex-quota-monitor/accounts.json` chưa tồn tại
- file cũ `~/.codex-switcher/accounts.json` có tồn tại

Hành vi hiện tại:

- copy nguyên file legacy sang path mới
- không có bước transform schema phức tạp
- không có rollback rõ ràng nếu copy thất bại giữa chừng

## 5. Slim export/import format

### 5.1 Mục tiêu

Slim format phục vụ chia sẻ hoặc backup nhanh với payload gọn, chỉ giữ token tối thiểu đủ để tái tạo account.

### 5.2 Quy ước hiện tại

Prefix mới:

- `cqm1.`

Prefix legacy vẫn chấp nhận khi import:

- `css1.`

### 5.3 Nội dung dữ liệu

Slim payload không chứa toàn bộ `StoredAccount`. Thực tế nó chỉ giữ:

- metadata account cần thiết
- API key nếu là `api_key`
- refresh token nếu là `chat_gpt`

Với ChatGPT account, import slim sẽ gọi flow tạo lại account từ refresh token.

### 5.4 Kỹ thuật encode

- JSON payload
- nén `zlib`
- mã hóa biểu diễn bằng `URL-safe base64`
- backend chặn payload giải nén quá `2 MiB`

### 5.5 Semantics import

Khi import:

- account trùng sẽ bị skip
- import chạy đồng thời với concurrency `6`
- kết quả trả `ImportAccountsSummary`

## 6. Full encrypted backup format

### 6.1 Mục tiêu

Full backup dùng cho export/import toàn bộ cấu hình account đầy đủ hơn slim payload.

### 6.2 Header và tương thích

Magic hiện tại:

- `CQMF`

Magic legacy vẫn chấp nhận:

- `CSWF`

Version format hiện thấy trong code:

- `1`

### 6.3 Thông số mã hóa

| Thành phần | Giá trị |
| --- | --- |
| KDF | `PBKDF2-HMAC-SHA256` |
| Iterations | `210000` |
| Salt length | `16 bytes` |
| Nonce length | `24 bytes` |
| Cipher | `XChaCha20Poly1305` |

### 6.4 Giới hạn

- file import tối đa `8 MiB`

### 6.5 Điểm cần chú ý bảo trì

Code hiện có `FULL_PRESET_PASSPHRASE` hardcode trong backend. Điều này giúp import/export hoạt động mà không hỏi người dùng mật khẩu, nhưng về mặt bảo mật đây là một shared secret nhúng trong binary.

## 7. Dữ liệu runtime không persistence

Không phải dữ liệu nào trong app cũng được lưu xuống đĩa.

Ba nhóm trạng thái chỉ tồn tại trong memory:

- ring buffer log `AppLogState`
- snapshot runtime `RuntimeState`
- pending OAuth flow trong `PENDING_OAUTH`

Hệ quả:

- restart app sẽ mất log runtime cũ
- không có audit trail lịch sử switch
- không có resume state cho OAuth flow đang dang dở

## 8. Tính nhất quán và chiến lược ghi file

Theo code hiện tại, thao tác lưu file chủ yếu dùng `fs::write` trực tiếp. Tài liệu source chưa cho thấy:

- atomic write qua temp file + rename
- file lock đa tiến trình
- checksum hoặc integrity record riêng cho `accounts.json`

Rủi ro bảo trì:

- crash hoặc power loss trong lúc ghi có thể để lại file hỏng
- nếu có hai tiến trình cùng ghi, chưa có cơ chế tránh lost update

## 9. Dữ liệu không có trong hệ thống

Hiện chưa có bằng chứng về:

- lịch sử usage theo thời gian
- audit log thao tác người dùng
- danh sách workspace/editor mapping bền vững
- cấu hình feature flag
- user preferences riêng cho updater/runtime policy

Điều này có nghĩa ứng dụng đang thiên về utility hiện thời, chưa phải sản phẩm quản trị dài hạn.

## 10. Khuyến nghị nâng cấp persistence

Nếu hệ thống tiếp tục mở rộng, roadmap hợp lý là:

1. Giữ `accounts.json` cho hiện tại nhưng bổ sung atomic write và checksum.
2. Tách secret store khỏi metadata store nếu cần tăng mức bảo mật.
3. Thêm schema versioning rõ ràng hơn cho backup format.
4. Bổ sung migration framework nội bộ nếu tiếp tục rebrand hoặc đổi cấu trúc account.
5. Cân nhắc SQLite khi bắt đầu cần history, audit, policy state hoặc cache phức tạp.
