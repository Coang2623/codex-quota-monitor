# Phân tích thiết kế và định hướng bảo trì

## 1. Tóm tắt đánh giá

Thiết kế hiện tại phù hợp với một desktop utility được phát triển nhanh để giải quyết bài toán thật của người dùng:

- quản lý nhiều account
- theo dõi quota
- đổi account và restart runtime xung quanh Codex

Điểm mạnh của hướng này là delivery nhanh, ít tầng abstraction và tương đối dễ lần theo flow bằng source. Điểm yếu là khi số feature tăng, coupling xuất hiện rất rõ ở một vài file trung tâm.

## 2. Điểm mạnh hiện tại

### 2.1 Kiến trúc đủ nhỏ để reverse-engineer nhanh

Hệ thống chỉ có vài trục chính:

- React UI
- Tauri command layer
- auth/storage/runtime/api modules

Điều này giúp người mới có thể hiểu toàn hệ thống trong thời gian ngắn hơn so với một kiến trúc phân tán.

### 2.2 Persistence đơn giản

Việc dùng `accounts.json` giúp:

- dễ backup
- dễ migrate
- dễ inspect khi debug

Đối với utility desktop, đây là lựa chọn hợp lý ở giai đoạn đầu.

### 2.3 Runtime classifier đã có test

So với nhiều phần còn lại, `runtime.rs` đã có bộ test tốt hơn mặt bằng chung của repo. Đây là tín hiệu tích cực vì runtime detection là vùng logic nhiều heuristic nhất.

### 2.4 Tách được một số concern quan trọng

Mặc dù chưa triệt để, codebase đã có các module riêng cho:

- OAuth
- token refresh
- usage API
- updater hook
- runtime log

Các mảnh này tạo nền tốt cho refactor tiếp theo.

## 3. Điểm nóng kiến trúc

### 3.1 `src/App.tsx` là god component ở frontend

Biểu hiện:

- giữ quá nhiều state UI mức cao
- chứa nhiều handler nghiệp vụ
- điều phối account, runtime, update, modal, log cùng một nơi

Rủi ro:

- khó test
- dễ tạo regression khi thêm feature nhỏ
- khó tách trách nhiệm giữa view và orchestration

### 3.2 `commands/account.rs` gom quá nhiều trách nhiệm

File này hiện chứa đồng thời:

- command account cơ bản
- switch runtime orchestration
- slim backup/import
- full encrypted backup/import
- helper mã hóa/giải mã

Rủi ro:

- bất kỳ thay đổi nào ở account lifecycle dễ chạm vào nhiều concern khác nhau
- khó tách test theo module
- code review khó khoanh vùng

### 3.3 `runtime.rs` vừa là classifier, vừa là launcher, vừa là policy engine

Hiện module này đang làm ba việc khác bản chất:

- đọc process snapshot
- phân loại editor/app/runtime
- terminate và relaunch process

Rủi ro:

- heuristic platform-specific tăng nhanh
- khó mở rộng thêm editor mới
- khó viết test cho launch policy mà không phụ thuộc classifier hiện hữu

### 3.4 Contract lỗi chưa chuẩn hóa

Backend chủ yếu trả `Result<T, String>`.

Hệ quả:

- frontend không phân biệt tốt loại lỗi
- khó map sang UX message nhất quán
- khó tự động retry theo loại lỗi

### 3.5 Test coverage chưa cân bằng

Theo source hiện tại:

- có test đáng kể ở `runtime.rs`
- hầu như không thấy test frontend
- chưa thấy test cho backup/import/export
- chưa thấy test cho OAuth command, updater flow, storage migration

Rủi ro:

- các flow đang nhạy cảm nhất lại phụ thuộc nhiều vào manual testing

## 4. Rủi ro kỹ thuật nổi bật

### 4.1 Hardcoded passphrase cho full backup

Đây là rủi ro bảo mật rõ ràng nhất đọc được trực tiếp từ code.

Tác động:

- ai có binary/source đều có thể suy ra khóa preset
- mã hóa hiện tại nghiêng về obfuscation hơn là bảo vệ bí mật thực sự

### 4.2 Ghi file chưa thấy atomic write

Nếu `accounts.json` bị ghi giữa chừng hoặc có hai luồng cùng sửa, repo chưa cho thấy cơ chế bảo vệ mạnh.

### 4.3 Restart runtime dựa trên heuristic cài đặt

Đây là bản chất khó tránh của sản phẩm, nhưng nó tạo một class bug riêng:

- detect nhầm runtime
- reopen sai executable
- reopen trễ hoặc sớm quá
- editor đang mở nhưng không nên restart

### 4.4 UI và background relaunch không hoàn toàn đồng bộ

`switch_account` phải trả kết quả cho UI trong khi reopen editor/app diễn ra bất đồng bộ. Điều này tạo ra khoảng “trạng thái trung gian” dễ gây khó hiểu cho người dùng và cho người bảo trì.

## 5. Đề xuất tái cấu trúc theo pha

### 5.1 Pha 1: làm sạch ranh giới module

Mục tiêu:

- giảm rủi ro chỉnh sửa hàng ngày

Việc nên làm:

1. Tách `App.tsx` thành:
   - page shell
   - `UpdateSection`
   - `ActiveSessionSection`
   - `AccountsSection`
   - `ImportExportModal`
2. Tách `account.rs` thành:
   - account CRUD commands
   - backup/import service
   - switch orchestration service
3. Chuẩn hóa log scope và error mapping

### 5.2 Pha 2: chuẩn hóa runtime platform layer

Mục tiêu:

- hỗ trợ thêm editor/app mà không phình `runtime.rs`

Việc nên làm:

1. Tạo model chung `EditorFamily` hoặc `RuntimeTarget`.
2. Tách classifier khỏi launcher.
3. Tách policy “có nên restart không” khỏi code terminate/relaunch.
4. Định nghĩa contract test theo fixture process list cho từng editor.

### 5.3 Pha 3: tăng độ tin cậy persistence và bảo mật

Việc nên làm:

1. Atomic write cho `accounts.json`.
2. Tách secret store khỏi metadata store nếu cần.
3. Bỏ hardcoded preset passphrase hoặc ít nhất cho phép user-supplied password.
4. Thêm schema version rõ cho backup/import.

### 5.4 Pha 4: tăng khả năng quan sát và test

Việc nên làm:

1. Thêm test cho storage migration, slim import, full backup.
2. Thêm test cho `switch_account` bằng runtime snapshot mock.
3. Thêm frontend component test cho các section trọng yếu.
4. Nếu còn đi xa hơn, thêm e2e smoke test cho build Windows.

## 6. Khuyến nghị cho maintainer mới

Khi cần sửa code, thứ tự đọc source hợp lý là:

1. `docs/maintenance/architecture.md`
2. `src/App.tsx`
3. `src/hooks/useAccounts.ts`
4. `src-tauri/src/commands/account.rs`
5. `src-tauri/src/runtime.rs`
6. `src-tauri/src/auth/storage.rs`
7. `src-tauri/src/api/usage.rs`

Khi cần debug bug switch/restart:

1. xem `Active Session`
2. xem `Runtime Log`
3. kiểm tra `runtime.rs`
4. kiểm tra `switch_account`
5. xác thực `~/.codex/auth.json` đã đổi chưa

## 7. Quyết định thiết kế nên giữ

Một số quyết định hiện tại vẫn đáng giữ ở trung hạn:

- Tauri desktop utility thay vì tách server riêng
- file store đơn giản cho account metadata
- runtime log trực tiếp trong UI
- release tự động từ `main`
- updater artifact qua GitHub Releases

Không nên refactor chỉ để “đẹp kiến trúc” nếu chưa có bài toán cụ thể hơn.

## 8. Kết luận

Codebase này đã vượt qua giai đoạn prototype ngắn hạn, nhưng chưa đến mức “maintenance-friendly” nếu tiếp tục thêm nhiều feature mà không tái cấu trúc. Ba vùng cần theo dõi sát nhất trong mọi thay đổi sau này là:

- `App.tsx`
- `commands/account.rs`
- `runtime.rs`

Nếu giữ được nguyên tắc tách dần ba vùng này theo service boundary rõ ràng, dự án có thể nâng cấp tiếp mà không phải viết lại từ đầu.
