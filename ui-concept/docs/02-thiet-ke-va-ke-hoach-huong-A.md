# VietKaraStudio – Thiết kế Hướng A & Kế hoạch áp dụng

> Tài liệu đi kèm bản mẫu tương tác tại [`../prototype/index.html`](../prototype/index.html).
> Các câu hỏi ở bước 2 chưa được chốt, tôi tạm dùng giả định mặc định: **ưu tiên người làm thường xuyên, màn hình ≥ 1366×768, giao diện tối, giữ menu chữ, giữ Tách nhạc & Cài đặt thành 2 trang cùng phong cách**.

## 1. Cấu trúc màn hình & Luồng tương tác (Hướng A – Studio Editor)

1. **Menubar + Topbar**:
   - Menubar giữ các nhóm `Tệp / Chỉnh sửa / Đồng bộ / Công cụ / Hiển thị / Trợ giúp` gọn cho phím tắt và lệnh ít dùng.
   - Topbar chuyển trực tiếp 3 trang (`Karaoke` · `Tách nhạc lấy beat` · `Cài đặt model`), hiển thị tên project + trạng thái lưu, và 3 nút thao tác chính (`Nhận lời & căn từ`, `Gõ nhịp`, `Xuất video`).
2. **Trang Karaoke (4 vùng cố định)**:
   - **Bảng trái (`Lời & nguồn`)**: gom 2 tab *Lời bài hát* (chọn câu nhảy playhead, khóa câu, sửa toàn bộ lời) và *Nguồn media* (nạp bản hát, beat gốc, tách giọng bè).
   - **Giữa (`Preview + Transport`)**: khung xem trước 16:9 / 9:16 cập nhật trực tiếp tiêu đề, logo, đếm ngược, màu chữ, cỡ chữ; thanh phát bên dưới có chọn nguồn nghe (`Beat gốc / Bản có giọng / Vocal`) và nút bật/tắt `Bè`.
   - **Bảng phải (`Inspector` tab dọc icon)**: tách rõ 7 nhóm (`Từ đang chọn`, `Kiểu lời`, `Song ca`, `Tiêu đề`, `Nền & tỉ lệ`, `Logo & Đếm ngược`, `Preset`) thay vì lồng tab + `<details>` nhiều tầng như bản cũ.
   - **Đáy (`Timeline` toàn chiều rộng)**: 3 track thẳng hàng (`Bản hát quy đổi`, `Beat gốc`, `Lời từng từ` tô màu theo vai), hỗ trợ zoom, bật/tắt sóng nhạc, theo nhạc, liên kết mép, thanh gõ nhịp Space.
3. **Trang `Tách nhạc lấy beat` & `Cài đặt model`**: dùng chung design tokens và cấu trúc card gọn, bỏ kiểu landing page cũ.
4. **Trạng thái thể hiện trong prototype**:
   - *Chưa mở dữ liệu*, *Có dữ liệu*, *Đang xử lý* (thanh tiến độ ở statusbar), *Lỗi* (banner lỗi + nút đóng).
   - Các thao tác AI / tách nhạc / dựng video được gắn nhãn `mô phỏng`.

## 2. Chi tiết có thể khó triển khai với công nghệ hiện tại (HTML/CSS/JS thuần)

1. **Quản lý trạng thái đồng bộ giữa Timeline – Bảng Lời – Inspector – Preview**: hiện tại `app.js` (~76 KB) và `studio.js` (~45 KB) thao tác DOM trực tiếp theo `id`. Khi chuyển sang Inspector theo ngữ cảnh và đồng bộ chọn từ giữa timeline và bảng phải, việc cập nhật thủ công từng phần tử rất dễ sót trạng thái nếu không tách một hàm `render()` hoặc pub/sub nhẹ.
2. **Timeline toàn chiều rộng với nhiều từ trên bài dài (300–500 từ)**: tạo hàng trăm thẻ `<div>` có kéo/giãn mép trực tiếp trong DOM dễ giật khi zoom/cuộn nhanh. Nên giữ cơ chế vẽ canvas cho sóng nhạc (hoặc virtualize khối từ nằm ngoài khung nhìn).
3. **Chia nhỏ `index.html` (~59 KB)**: không có bundler/framework nên việc tách component HTML phải dựa vào template literal JS hoặc giữ khung HTML tĩnh và tách JS theo module.

## 3. Kế hoạch áp dụng vào dự án thật theo từng giai đoạn

> **Đã duyệt triển khai (16/11/2026).** Tiến độ:
>
> | Giai đoạn | Trạng thái |
> |---|---|
> | 1 — Tokens & CSS | ✅ Xong: `static/tokens.css`, `static/studio-a.css` (nạp sau `style.css`), đã kiểm tra 1366×768 & 1920×1080 |
> | 2 — Khung 4 vùng | ✅ Xong: bỏ rail trái, chuyển điều hướng 3 trang (`Karaoke`, `Tách nhạc lấy beat`, `Cài đặt model`) lên Topbar, sửa cuộn dọc Inspector & tràn ngang, thêm null-check trong `static/branding.js` |
> | 3 — Inspector | ✅ Xong: thanh tab dọc `56px` bên trái Inspector với vạch nhấn vàng `--c-accent`, giữ nguyên toàn bộ `id` và `data-inspector-tab` |
> | 4 — Tách nhạc & Cài đặt | ✅ Xong: đồng bộ bề mặt, viền, chữ và nút trên `#separationPage` & `#settingsPage` theo `tokens.css` |
>
> Cách làm Giai đoạn 1: không sửa trực tiếp `style.css` (566 dòng, nhiều lớp `:root` chồng nhau) mà dùng lớp `studio-a.css` cuối cùng, để dễ so sánh/hoàn tác; chỉ gỡ dần phần cũ khi lớp mới đã ổn định.

### Giai đoạn 1 — Chuẩn hóa Design Tokens & CSS (rủi ro thấp)
- **Tái sử dụng**: giữ nguyên toàn bộ `app.py`, `worker.py`, các endpoint Flask và handler JS hiện có.
- **Thay đổi**: đưa bảng màu, font, khoảng cách vào `tokens.css` (tách từ `style.css` 103 KB); chuẩn hóa kích thước nút, input, bỏ bớt màu trang trí của các nhóm `<details>`.
- **Rủi ro**: CSS cũ có 33 khối `@media` và nhiều selector lồng nhau; cần kiểm tra lại trên cửa sổ `1366×768` và `1920×1080` trong WebView2.

### Giai đoạn 2 — Tái cấu trúc khung 4 vùng & chuyển điều hướng lên Topbar (rủi ro trung bình)
- **Tái sử dụng**: giữ nguyên logic `layout_resize.js`, `timeline_edit.js`, `tapping.js`, `tap_timing.js`, `backing_vocals.js`.
- **Thay đổi**:
  - Bỏ rail trái chuyển trang, đưa `Karaoke / Tách nhạc / Cài đặt` lên Topbar.
  - Đưa Timeline ra toàn chiều rộng đáy (`grid-column: 1 / 4`), giữ lại các ID phần tử DOM cũ để không gãy `app.js` và các bài test JS (`tests/*.test.js`).
  - Gom bảng *Từ đang chọn* vào làm tab đầu tiên của Inspector.
- **Rủi ro**: `layout_resize.js` đang tính chiều cao/rộng theo cấu trúc 3 cột cũ → cần viết lại hàm tính kích thước kéo giãn.

### Giai đoạn 3 — Tổ chức lại Inspector & xóa trùng lặp (rủi ro trung bình)
- **Tái sử dụng**: toàn bộ input/select hiện có (giữ nguyên `id` để binding backend không đổi).
- **Thay đổi**:
  - Thay 8 tab ngang + `<details>` bằng thanh tab dọc icon ở mép trái Inspector.
  - Loại bỏ chỗ trùng lặp giữa tab `Kiểu lời` và các tab `Nền / Logo / Đếm ngược`; bỏ phần ẩn `#mvSettings` hoặc chuyển thành tùy chọn thử nghiệm rõ ràng.
- **Rủi ro**: các sự kiện `change`/`input` gắn vào `id` cũ phải được giữ nguyên khi di chuyển vị trí DOM.

### Giai đoạn 4 — Đồng bộ phong cách trang Tách nhạc & Cài đặt (rủi ro thấp)
- **Tái sử dụng**: giữ nguyên toàn bộ logic gọi API YouTube, Mel-RoFormer, UVR Karaoke, kiểm tra SHA-256 và cập nhật bản vá (`updates.js`).
- **Thay đổi**: thay layout trang Tách nhạc từ dạng landing page sang card 2 bước gọn gàng.

## 4. Các điểm cần chốt sau khi anh/chị xem thử bản mẫu

1. **Chiều cao mặc định của Timeline** (hiện đặt 38% chiều cao cửa sổ) và có cần cho kéo giãn tự do không?
2. **Tab dọc bên phải**: bản mới đã thêm icon SVG kèm chữ ngắn (`Chọn`, `Chữ`, `Song ca`, `Tiêu đề`, `Nền`, `Logo`, `Preset`); anh/chị thấy vừa mắt chưa hay muốn thu gọn lại?
3. **Nút trên Topbar**: ngoài `Nhận lời & căn từ`, `Gõ nhịp`, `Xuất video`, có cần đưa thêm nút nào ra ngoài không?
4. **Màu vai song ca** (hiện dùng Tím / Hồng / Xanh dương) có phù hợp mắt nhìn khi làm việc lâu không?

## 5. Lịch sử chỉnh sửa bản mẫu

- **Lần 1 (16/11/2026) — Nâng cấp hệ thống icon & nút bấm studio:**
  - Thay toàn bộ ký tự Unicode đơn giản bằng **SVG icon đồng bộ** qua sprite nội bộ (`#i-*` trong `prototype/index.html` + helper `ico()` trong `prototype/js/app.js`).
  - Nâng cấp bề mặt `.btn`, `.btn.primary`, `.btn.accent-soft`, `.icon-btn`, `.play-circle` với nền gradient nhẹ và viền sáng cạnh trên; gom các nút toolbar trên Timeline thành `.btn-group`.
  - Mở rộng thanh tab dọc Inspector lên `54px` (`prototype/css/components.css`) với icon + nhãn ngắn và vạch nhấn trạng thái đang chọn.
  - **Áp dụng về dự án thật:** có thể đưa trực tiếp SVG sprite vào `static/index.html` và CSS nút mới vào `static/style.css` ở Giai đoạn 1–3 mà không đổi logic JS/Flask.
