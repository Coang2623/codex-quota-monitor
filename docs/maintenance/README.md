# Tài liệu bảo trì Codex Quota Monitor

## Mục tiêu

Bộ tài liệu này được tạo từ source code hiện tại của dự án để phục vụ:

- bảo trì hằng ngày
- phân tích tác động khi sửa lỗi
- lập kế hoạch tách module và nâng cấp sau này
- onboarding nhanh cho người mới tham gia dự án

Phạm vi tài liệu bám theo mã nguồn đang có trong repo ở phiên bản `0.1.10`.

## Bản đồ tài liệu

- `architecture.md`: kiến trúc tổng thể, thành phần chính, dependency và điểm nối giữa frontend, backend, runtime, updater
- `api-spec.md`: bề mặt API nội bộ của ứng dụng, gồm Tauri command và external API mà backend gọi ra ngoài
- `db-spec.md`: đặc tả persistence và dữ liệu lưu trữ; dự án không có DBMS, nhưng có file store, auth file, backup format và migration
- `feature-spec.md`: đặc tả các capability nghiệp vụ đang có trong ứng dụng
- `sequence-flow.md`: luồng chi tiết cho các flow chính như OAuth, switch account, refresh usage, updater
- `screen-spec.md`: đặc tả UI hiện tại của app desktop
- `design-analysis.md`: phân tích thiết kế hiện tại theo góc nhìn bảo trì và nâng cấp

## Cách đọc khuyến nghị

1. Đọc `architecture.md` để nắm khung hệ thống.
2. Đọc `feature-spec.md` để hiểu ứng dụng đang làm gì.
3. Đọc `sequence-flow.md` để lần theo các luồng chính.
4. Đọc `design-analysis.md` trước khi bắt đầu refactor hoặc thêm feature lớn.

## Giới hạn bằng chứng

- Ứng dụng hiện là desktop app Tauri 2 với frontend React và backend Rust, không có backend service riêng của dự án.
- Persistence hiện tại là file JSON và file backup mã hóa, không có PostgreSQL, SQLite application DB hay migration framework trong repo.
- Các tài liệu này chỉ mô tả những gì có thể truy vết được từ source hiện tại như `src/`, `src-tauri/`, `package.json`, `Cargo.toml`, `tauri.conf.json`, workflow release và README.
