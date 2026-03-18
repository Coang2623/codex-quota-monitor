# Đặc tả màn hình hiện tại

## 1. Phạm vi

Ứng dụng hiện là single-window desktop app. Phần lớn UI nằm trong `src/App.tsx`, với một số phần tách thành component con.

Các màn hình thực tế:

- màn hình chính
- modal thêm account
- modal import/export slim text

Ngoài ra còn có panel log phụ ở cạnh phải trên màn hình rộng.

## 2. Màn hình chính

### 2.1 Header / toolbar trên cùng

Mục đích:

- hiển thị brand và tagline
- cung cấp action mức app

Các action hiện có:

- `Show All` / `Hide All`
- `Refresh All`
- `Warm-up All`
- menu `Account`

Menu `Account` hiện chứa:

- `Add Account`
- `Check for Updates`
- `Export Slim Text`
- `Import Slim Text`
- `Export Full Encrypted File`
- `Import Full Encrypted File`

Rule:

- nhiều nút có disabled state theo thao tác đang chạy
- `Show All` / `Hide All` áp dụng cho cơ chế mask dữ liệu nhạy cảm trong card account

### 2.2 Khối `App Update`

Vị trí:

- đầu phần nội dung chính, ngay dưới header

Nội dung:

- nhãn section `App Update`
- summary text như `Up to date`, `Checking`, `Update available`
- mô tả nguồn update từ GitHub release ổn định
- badge trạng thái
- nút `Check now`

Khi có bản mới:

- hiện card chi tiết phiên bản
- hiển thị version hiện tại, version mới, ngày publish, release title
- có thể hiển thị tóm tắt release body
- nếu đang download thì có progress bar

Hành động trong card update:

- `Download & Install` nếu updater artifact hợp lệ
- `Open release page` nếu chỉ fallback thủ công
- `View release`
- `Dismiss`

### 2.3 Khối `Active Session`

Vị trí:

- ngay dưới `App Update`

Mục đích:

- cho biết editor/runtime nào đang mở
- giải thích vì sao switch bị block hoặc được phép

Thành phần:

- tiêu đề `Active Session`
- summary text mức cao
- badge trạng thái:
  - `Switch blocked`
  - `Switch allowed`
  - `Idle`
- grid card chi tiết runtime

Các card runtime hiện có về mặt nghiệp vụ:

- standalone `Codex CLI`
- `VS Code`
- `Antigravity`
- `Codex app`

Mỗi card hiển thị:

- label runtime
- status badge
- detail text, ví dụ số cửa sổ logic, số extension worker, trạng thái restartable

Rule:

- blocker CLI dùng tone amber
- runtime restartable dùng tone blue
- trạng thái idle dùng tone xám/trung tính

### 2.4 Trạng thái rỗng / lỗi / loading

Khi chưa có account:

- icon placeholder
- tiêu đề `No accounts yet`
- nút `Add Account`

Khi đang load initial data:

- spinner trung tâm
- text `Loading accounts...`

Khi lỗi load:

- thông báo `Failed to load accounts`
- hiển thị message lỗi backend/frontend

### 2.5 Khối `Active Account`

Vị trí:

- dưới `Active Session`, chỉ hiện khi có account active

Mục đích:

- hiển thị account đang publish ra runtime

Nội dung card:

- chấm xanh active
- tên account
- email
- badge plan
- usage bar `5h` và `Weekly`
- dòng `Last updated`
- action row

Action trong card active:

- nút disabled `Active`
- `Warm-up`
- `Refresh`
- `Delete`

Card active có border xanh nhạt hơn card thường.

### 2.6 Khối `Other Accounts`

Vị trí:

- dưới `Active Account`

Nội dung:

- tiêu đề `Other Accounts (N)`
- dropdown sort
- grid/list các `AccountCard`

Hiện tại sort có nhiều mode, trong đó có các mode liên quan deadline và usage reset.

### 2.7 `AccountCard`

Component dùng chung cho active account và account thường.

Thông tin hiển thị:

- tên account, có thể click để rename inline khi không mask
- email
- badge plan
- usage bar
- thời gian cập nhật gần nhất

Action của account chưa active:

- `Switch & Restart`
- `Warm-up`
- `Refresh`
- `Delete`

Biến thể đặc biệt:

- nếu đang switch: nút chính thành `Switching...`
- nếu switch bị block: nút chính thành `CLI Running`
- nếu đang warm-up hoặc refresh: icon/action đổi trạng thái

### 2.8 Mask / privacy UI

Mỗi card có icon mắt để:

- ẩn/hiện tên account
- ẩn/hiện email

Khi bị mask:

- text bị blur
- rename inline bị khóa

## 3. Modal thêm account

Component:

- `AddAccountModal`

Tabs hiện có:

- `ChatGPT Login`
- `Import File`

### 3.1 Tab OAuth

Input chính:

- tên account

Hành vi:

- bấm action để bắt đầu OAuth
- mở trình duyệt ngoài
- chờ `complete_login`

### 3.2 Tab Import File

Input chính:

- path tới `auth.json`
- tên account

Hành vi:

- validate input
- gọi import backend

## 4. Modal slim import/export

Đây là modal text-based cho slim payload.

Hai mode:

- `Export Slim Text`
- `Import Slim Text`

Export:

- hiện payload text trong textarea

Import:

- cho phép paste payload
- nút `Import Missing Accounts`

Rule:

- modal này không dùng cho full encrypted file; full backup đi qua native file picker trực tiếp

## 5. Runtime Log panel

Component:

- `LogPanel`

Vị trí:

- cột bên phải trên màn hình rộng

Nội dung:

- tiêu đề `Runtime Log`
- mô tả ngắn `Live events from frontend and backend`
- nút `Clear`
- danh sách entry mới nhất

Mỗi entry hiện:

- level badge `INFO`, `WARN`, `ERROR`, `SUCCESS`
- timestamp
- scope như `RUNTIME`, `SWITCH`, `UPDATE`
- message

Đặc điểm:

- panel này là live log chứ không phải console đầy đủ
- giới hạn số dòng gần nhất
- hữu ích để debug hành vi switch/restart

## 6. Responsive behavior

Theo code hiện tại:

- nội dung chính tối đa cỡ `max-w-5xl`
- `Active Session` chuyển từ 1 cột sang nhiều cột tùy breakpoint
- `LogPanel` chỉ hiện ở breakpoint rộng
- toolbar đầu trang wrap xuống nhiều hàng khi chiều ngang hẹp

## 7. Phụ thuộc UI quan trọng

Các vùng UI có coupling cao với backend:

- `Active Session` phụ thuộc trực tiếp `CodexProcessInfo`
- card `Switch & Restart` phụ thuộc `switch_account` và `check_codex_processes`
- `App Update` phụ thuộc `useAppUpdate`
- log panel phụ thuộc event `app-log`

Khi refactor backend, bốn vùng này thường là nơi vỡ contract đầu tiên.
