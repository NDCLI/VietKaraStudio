# VietKaraStudio – Phân tích giao diện & đề xuất hướng thiết kế (Bước 1–2)

> Phân tích trên mã nguồn tại commit `f7f952f` (phiên bản 1.1.15).
> Mẫu trực quan so sánh các hướng: mở [`../concepts/index.html`](../concepts/index.html) trong trình duyệt.

## 1. Công nghệ giao diện (xác nhận từ mã nguồn)

| Hạng mục | Thực tế |
|---|---|
| Vỏ desktop | `pywebview` (Edge WebView2) mở trang local – `desktop_entry.py`, `launch.py` |
| Backend | Flask + waitress (`app.py` ~71 KB), worker xử lý nền (`worker.py`) |
| Frontend | HTML/CSS/JS thuần, không framework, không bundler. `static/index.html` ~59 KB (một file, mọi màn hình + 12 `<dialog>`), `style.css` ~103 KB, `app.js` ~76 KB, `studio.js` ~45 KB + 10 file JS phụ |
| Trạng thái | Biến toàn cục + thao tác DOM theo `id`; có test JS (`tests/*.test.js`) cho tapping, timeline_edit, audio monitor |
| Biến thiết kế | Có CSS custom properties nhưng ít và rải rác; 33 khối `@media` |

Mức độ hoàn thiện: **sản phẩm đang phát hành** (Setup Nuitka, bản vá tự động). Phần lớn tính năng có giao diện và logic thật.

## 2. Màn hình & tính năng

### Đã có trong mã nguồn (UI + handler)
- **Rail trái 3 trang**: Karaoke · Tách nhạc lấy beat · Cài đặt model.
- **Topbar**: tên project, trạng thái lưu, nút *Xuất video*; **menu chữ** Tệp / Chỉnh sửa / Đồng bộ / Công cụ / Hiển thị / Trợ giúp (dùng `<details>`).
- **Trang Karaoke** (3 cột + đáy):
  - Bảng trái: tab *Lời bài hát* (danh sách câu, sửa toàn bộ lời, nhận lời & tự căn, nhập SRT) và tab *Nguồn media* (bản có vocal/video, beat gốc, tách giọng bè, trích âm từ video).
  - Giữa: preview karaoke (16:9/9:16, toàn màn hình), thanh phát (nguồn nghe beat/bản hát/vocal, tốc độ 0.5–1.25×, nút Bè).
  - Bảng phải *Thuộc tính* 8 tab: Lựa chọn · Kiểu lời · Song ca · Tiêu đề · Nền · Logo · Đếm ngược · Preset. Bên trong lại là nhiều `<details>` nhóm màu.
  - Bảng *Từ đang chọn*: thời gian bắt đầu/kết thúc, vai, track, co giãn vùng chọn, căn lại/khóa/tách/gộp câu.
  - Panel *Mốc đồng bộ thủ công* bản hát ↔ beat (ẩn mặc định).
  - **Timeline** hai hàng (bản hát đã quy đổi / beat gốc) + hàng lời, zoom, theo nhạc, liên kết mép, sóng nhạc, undo/redo.
  - Chế độ **gõ nhịp Space** (tap/hold) với overlay trong timeline.
- **Trang Tách nhạc**: chọn file hoặc link YouTube → chọn model bè → *Tách beat không bè / có bè* → 3 thẻ kết quả nghe & tải WAV.
- **Trang Cài đặt**: danh sách model, cài model thủ công, cập nhật bản vá.
- **Hộp thoại**: Đồng bộ thủ công, Project (danh sách, dựng theo lô, phiên bản, sao lưu JSON/ZIP), Đề xuất đồng bộ, Gợi ý phân vai, Kết quả lô, Thư viện preset (16 preset), Xuất (âm thanh, định dạng, chất lượng, fps, độ phân giải, tỉ lệ, LUFS, thư mục, render thử 8s), Xem trước căn câu, Thiết lập AI, Nghe thử bè.
- **Footer**: phiên bản, trạng thái môi trường, thanh tiến độ tác vụ + Hủy.

### Placeholder / di sản
- `#mvSettings` (Kiểu Lyrics MV) bị ẩn cứng bằng `display:none!important` – tính năng chưa bật.
- `#alignLyricsBtn`, `#estimateBtn` là nút ẩn không nhãn (dùng làm proxy cho menu).
- Tab *Nền / Logo / Đếm ngược* trùng nội dung với các nhóm `<details>` trong *Kiểu lời* (cùng một khối, lọc bằng `data-property-section`).
- Tỉ lệ & độ phân giải xuất hiện ở 2 nơi (Cá nhân hóa và hộp thoại Xuất).

### Chỉ có trong tài liệu (chưa kiểm chứng UI riêng)
- README mô tả 315 font, chuyển mã VNI/TCVN3, đếm ngược sau nhạc dạo… – có logic nhưng không có màn hình riêng; tôi coi là thuộc tính trong inspector.

## 3. Hạn chế hiện tại (bố cục, điều hướng, thông tin)

1. **Quá tải mật độ**: một trang chứa gần như toàn bộ app; inspector có tab → nhóm gập → hàng chục control, nhãn dài kèm đơn vị.
2. **Điều hướng 3 tầng chồng nhau**: rail + menu chữ + tab trái + 8 tab phải + dialog. Cùng một lệnh có 2–3 lối vào (ví dụ “Nhận lời & tự căn” ở menu, tab lời, timeline).
3. **Không rõ luồng làm việc**: người dùng mới không thấy thứ tự *Nạp nguồn → Lời → Đồng bộ → Thiết kế → Xuất*; trạng thái “đã đủ điều kiện xuất chưa” chỉ lộ khi mở hộp thoại Xuất.
4. **Timeline bị chèn ép**: nằm dưới preview, chiều cao phụ thuộc kéo tay; đây lại là nơi làm việc lâu nhất (căn từ).
5. **Ngữ cảnh chọn lẫn lộn**: “Từ đang chọn” là panel riêng, trong khi tab “Lựa chọn” của inspector cũng tồn tại.
6. **Tác vụ nặng (AI, tách nhạc, render)** chỉ có 1 thanh ở footer; không có hàng đợi/lịch sử, không phân biệt lỗi rõ.
7. **Tách nhạc** là trang kiểu landing (tiêu đề lớn, slogan) – lệch phong cách với phần còn lại.
8. Trang trí (icon ký tự unicode, nhiều màu nhóm) nhiều hơn mức cần cho phần mềm làm việc.

## 4. Ba hướng thiết kế

Xem minh họa trong `concepts/index.html` (có thể bấm qua lại).

### A. Studio Editor (kiểu NLE – Premiere/Resolve tinh gọn)
Bố cục cố định 4 vùng: thư viện/lời (trái) · preview (giữa) · inspector theo ngữ cảnh (phải) · **timeline toàn chiều rộng** (đáy, cao ~40%). Menu chữ giữ lại; rail trái bỏ, thay bằng tab trang ở topbar.
- ✅ Gần cấu trúc hiện tại → tái sử dụng tối đa mã, người dùng cũ quen.
- ✅ Timeline rộng, phù hợp căn từ lâu.
- ❌ Vẫn nhiều thứ trên một màn; người mới chưa thấy luồng.

### B. Quy trình theo bước (Workflow stages)
Thanh bước trên cùng: **1 Nguồn · 2 Lời · 3 Đồng bộ · 4 Thiết kế · 5 Xuất**. Mỗi bước có bố cục riêng, chỉ hiện công cụ của bước đó, có checklist trạng thái.
- ✅ Dễ học nhất, giảm tải nhận thức, trạng thái sẵn sàng xuất luôn hiển thị.
- ❌ Người làm nhiều bài phải chuyển bước liên tục (chỉnh màu chữ khi đang căn từ); phải viết lại nhiều bố cục.

### C. Lời là trung tâm (Lyric-first)
Cột lời bài hát chiếm giữa màn hình như một trình soạn thảo: mỗi câu là một hàng có mini-timeline từng từ, vai, khóa. Preview thu nhỏ neo góc phải; timeline sóng nhạc chỉ hiện cho câu đang chọn.
- ✅ Tối ưu cho sửa lời/căn từ, đọc tiếng Việt tốt, thấy cả bài.
- ❌ Thiết kế hình ảnh (kiểu chữ, nền) bị lép vế; mini-timeline từng dòng khó làm mượt bằng DOM thuần với bài dài.

### ⭐ Đề xuất: A + “chế độ làm việc” mượn từ B
Giữ khung Studio Editor (A), thêm **thanh chế độ** ở topbar: *Đồng bộ* · *Thiết kế* · *Xuất* (như “Pages” của DaVinci). Cùng một khung, mỗi chế độ chỉ đổi nội dung bảng phải và chiều cao timeline; luôn có **chỉ báo sẵn sàng** (nguồn ✓ lời ✓ đồng bộ 82%…). Tách nhạc & Cài đặt chuyển thành màn hình công cụ cùng phong cách, tác vụ nặng vào **bảng Tác vụ** có hàng đợi.

## 5. Câu hỏi cần anh/chị xác nhận
1. **Chọn hướng**: A, B, C hay hướng đề xuất (A + chế độ)?
2. **Người dùng chính**: chủ yếu người làm karaoke thường xuyên (nhiều bài/ngày) hay người mới, dùng thỉnh thoảng?
3. **Độ phân giải màn hình tối thiểu** cần hỗ trợ: 1366×768 hay từ 1920×1080?
4. **Theme**: giữ tối (dark) làm mặc định? Có cần bản sáng không?
5. Có muốn giữ **menu chữ** (Tệp/Chỉnh sửa…) không, hay chỉ dùng toolbar + phím tắt?
6. Tách nhạc: giữ là trang riêng, hay thành hộp thoại/công cụ trong project?
