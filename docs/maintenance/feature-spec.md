# Đặc tả chức năng hiện tại

## 1. Mục tiêu sản phẩm

`Codex Quota Monitor` là desktop utility giúp người dùng làm việc với nhiều account Codex/OpenAI trong cùng một máy, đồng thời theo dõi quota và giảm thao tác thủ công khi đổi account.

## 2. Nhóm chức năng cốt lõi

### 2.1 Quản lý nhiều account

Người dùng có thể:

- xem danh sách account đã lưu
- biết account nào đang active
- đổi tên account
- xóa account
- chọn account active mới

Nguồn dữ liệu:

- `accounts.json` trong thư mục cấu hình của ứng dụng

Ràng buộc hiện tại:

- tên account phải duy nhất
- account đầu tiên được thêm sẽ tự thành active nếu chưa có active account

### 2.2 Thêm account bằng OAuth

Ứng dụng hỗ trợ login ChatGPT/OpenAI qua OAuth:

- người dùng nhập tên account
- app mở trình duyệt tới OpenAI auth
- local callback server nhận authorization code
- backend exchange token
- account mới được lưu và set active ngay

Đặc điểm:

- dùng PKCE
- có thể hủy flow đang chờ
- timeout chờ callback khoảng 5 phút

### 2.3 Import account từ `auth.json`

Người dùng có thể nhập credentials từ file `auth.json` hiện có của Codex.

Use case:

- đang dùng Codex CLI/App/Extension sẵn
- muốn đưa account hiện tại vào app mà không phải OAuth lại

### 2.4 Switch account và publish sang Codex runtime

Đây là chức năng trọng tâm của sản phẩm.

Khi người dùng bấm `Switch & Restart`, backend sẽ:

- kiểm tra runtime hiện tại
- chặn switch nếu thấy standalone `Codex CLI` đang chạy
- ghi account mới sang `~/.codex/auth.json`
- cập nhật active account nội bộ
- đóng các runtime liên quan nếu cần
- mở lại editor/app tương thích theo best effort

Runtime đang được hỗ trợ trong code hiện tại:

- standalone `Codex CLI` như blocker
- `VS Code`
- `Antigravity`
- `Codex app`

Rule hiện tại:

- `VS Code` chỉ restart nếu thật sự có Codex extension worker của VS Code
- `Antigravity` chỉ restart nếu thật sự có Codex extension worker của Antigravity
- `Codex app` được xem là runtime riêng và có nhánh reopen riêng

### 2.5 Phát hiện active session

UI có mục `Active Session` để cho biết runtime nào đang mở:

- số CLI blocker
- số cửa sổ VS Code
- số extension worker của VS Code
- số cửa sổ Antigravity
- số extension worker của Antigravity
- số Codex app đang chạy

Mục tiêu của feature này:

- giúp người dùng hiểu app sẽ restart gì
- giải thích vì sao switch bị block hay không bị block

### 2.6 Xem usage/quota

Với account ChatGPT, ứng dụng có thể đọc quota hiện tại và hiển thị:

- cửa sổ giới hạn ngắn hạn `5h`
- cửa sổ giới hạn tuần `7d`
- thời điểm reset
- trạng thái credits nếu backend trả về

Với account API key:

- warm-up dùng được
- usage quota hiện không có dữ liệu thật trong app

### 2.7 Refresh usage đồng loạt

Người dùng có thể:

- refresh usage cho một account
- refresh usage cho toàn bộ account

Đặc điểm:

- backend refresh song song
- frontend có auto refresh theo chu kỳ

### 2.8 Warm-up request

Ứng dụng hỗ trợ gửi request nhẹ để “làm nóng” account:

- 1 account
- toàn bộ account

Mục đích thực tế:

- kiểm tra account còn hoạt động
- giảm độ trễ cho lần dùng đầu sau khi đổi account

### 2.9 Export/import cấu hình

Có hai mode backup chính:

#### Slim text

- payload gọn
- phù hợp chia sẻ nhanh hoặc lưu tay
- dùng refresh token/API key tối thiểu

#### Full encrypted file

- đầy đủ hơn
- xuất ra file mã hóa
- phù hợp backup cấu hình nhiều account

Tương thích legacy:

- import được payload slim cũ
- import được backup full cũ

### 2.10 Migration dữ liệu từ brand cũ

App hiện hỗ trợ tự migrate dữ liệu từ brand cũ:

- từ `~/.codex-switcher/accounts.json`
- sang `~/.codex-quota-monitor/accounts.json`

Điều này giúp rebrand mà không làm mất account cũ.

### 2.11 Runtime log trong ứng dụng

UI có panel log bên phải để theo dõi:

- detect runtime
- switch account
- restart editor/app
- check/update progress
- lỗi vận hành từ backend/frontend

Log này là log live trong memory, không phải log file bền vững.

### 2.12 Auto check for update và in-app update

Ứng dụng có updater với hai lớp:

- tự check phiên bản mới khi mở app
- cho phép `Download & Install` nếu updater artifact hợp lệ

Fallback:

- nếu updater plugin không lấy được gói hợp lệ, UI vẫn có thể mở trang release thủ công

### 2.13 CI/CD release production

Repo có production workflow để:

- trigger khi push vào `main`
- validate version đồng bộ giữa JS/Rust/Tauri config
- build Windows release
- tạo GitHub release với release note theo tag
- phát hành updater artifact như `latest.json`

## 3. Nhóm chức năng chưa thấy trong code

Theo source hiện tại, chưa có bằng chứng cho các nhóm sau:

- quản lý workspace riêng theo account
- scheduler tự switch account khi quota thấp
- hard policy “đừng restart khi đang gen câu trả lời”
- lịch sử usage theo ngày
- xác thực đa người dùng trong app
- đồng bộ cloud cho danh sách account

## 4. Ràng buộc sản phẩm hiện tại

Một số ràng buộc không nên bỏ qua khi bảo trì:

- runtime detection dựa trên heuristic process inspection
- `Codex CLI` standalone luôn là blocker
- restart editor/app là best effort, phụ thuộc hệ điều hành và cách cài runtime
- usage data phụ thuộc API ngoài, không do app sở hữu

## 5. Định hướng mở rộng hợp lý

Nếu tiếp tục phát triển, các hướng mở rộng phù hợp với nền hiện tại là:

1. thêm policy engine cho restart confirmation và busy detection
2. tách runtime support thành adapter riêng cho từng editor/app
3. thêm telemetry nội bộ hoặc persistent log
4. thêm usage history và dashboard xu hướng
5. thêm thiết lập cá nhân cho updater, auto refresh và switch policy
