# Kế hoạch hoàn thiện VietKara Studio — bàn giao cho Claude Code

Ngày chốt: 2026-11-07. Người thực hiện: **Claude Code**, chạy trực tiếp trong repository trên máy cá nhân tại nhà.

## 1. Mục tiêu và điểm bắt đầu

**Ưu tiên mới của người dùng: UI trước, engine nối sau.** Hoàn thiện khung giao diện,
các tab/nút/hộp thoại và tương tác bằng dữ liệu mẫu trước. Dùng chế độ `--ui-demo`
để duyệt UI mà không gọi engine. Chặng 0 chỉ chuẩn bị môi trường/UI; các kiểm tra
worker/media chuyển sang chặng tích hợp tương ứng. Chặng 1 là mốc bàn giao đầu;
chặng 2–6 là công việc nối xử lý sau đó. Không hiển thị thành công giả cho thao tác
chưa nối backend. Giữ editor và engine hiện có để tái sử dụng sau.

Hoàn thiện bản Windows dùng nội bộ: người dùng có thể nhập nhạc, nhập/nhận dạng lời, căn lời, thiết kế, xuất video, lưu và mở lại project an toàn. Bàn giao portable và bộ cài đã kiểm chứng trên môi trường độc lập với máy build.

- Repository: `https://github.com/NDCLI/VietKaraStudio`.
- Nhánh làm việc và push: `codex/rust-gpui-rewrite`.
- Checkout công ty trước bàn giao: `D:\Box\VietKaraStudio`; trên máy nhà sử dụng đường dẫn clone thực tế.
- Commit gần nhất tại thời điểm viết: `1d7b216` (UI ba cột và timeline toàn chiều ngang).
- Commit `90ca587`: giới hạn chờ worker AI khi stdout đóng nhưng process còn sống.
- Luôn fetch và đọc đầu nhánh thực tế trước khi làm; các commit trên là mốc tham chiếu, không phải yêu cầu quay ngược nhánh.

Giữ Rust + GPUI cho ứng dụng, Python worker cho AI và FFmpeg cho xử lý/xuất media. Tái sử dụng code hiện có; không viết lại toàn bộ hoặc port toàn bộ AI engine sang Rust.

Đợt này chưa bao gồm phát hành công khai, ký số, updater GitHub end-to-end hoặc thay runtime/model AI. Không công bố dự án hoàn tất nếu còn lỗi mất dữ liệu, treo tác vụ, sai timing/audio xuất hoặc chưa kiểm tra bộ cài trên môi trường độc lập.

## 2. Quy trình làm việc của Claude

1. Đọc hướng dẫn repository, `RUST_REWRITE_PLAN.md`, `docs/windows-plan.md` và `docs/compatibility-matrix.md`.
2. Kiểm tra working tree, fetch và tích hợp đầu remote an toàn. Giữ thay đổi của người dùng; không reset hoặc force-push.
3. Rà phần đang có trước khi thêm tính năng. Đối chiếu tiến độ mới nhất vì tài liệu có nhiều ghi chú lịch sử.
4. Thực hiện từng phần hoàn chỉnh: sửa code → kiểm tra liên quan → cập nhật bằng chứng → commit → **push ngay** lên nhánh rewrite.
5. Tiếp tục phần độc lập khi gặp điều kiện môi trường bị thiếu. Chỉ hỏi khi thiếu quyết định sản phẩm, dữ liệu không thể tự tìm hoặc môi trường bắt buộc.
6. Không commit media/project cá nhân, model, credentials, cache, log hoặc artifact phát hành lớn.

### Build trên máy nhà

- Không có ràng buộc AhnLab; **không bắt buộc `-j 1`**. Cho phép Cargo dùng mức song song mặc định theo tài nguyên máy.
- Giữ cache Cargo và `target`; chỉ giảm số job khi thiếu RAM/CPU hoặc gặp lỗi tài nguyên. Không dùng `cargo clean` như cách sửa lỗi mặc định.
- Dùng Rust **1.99.1**, GPUI **0.2.2** theo manifest/toolchain hiện có; không tự nâng dependency ngoài nhu cầu sửa lỗi cụ thể.
- Build/check/test trước đây bị công cụ bảo mật trên máy công ty giữ lại phải được chạy lại ở máy nhà. Đặc biệt, UI `1d7b216` chưa xác nhận compile/runtime; `90ca587` đã cargo check nhưng chưa có kết quả regression test runtime cho tình huống stdout đóng sớm.
- Kết quả kiểm thử lịch sử không chứng minh đầu nhánh hiện tại đã đạt. Test tự skip không được tính là kiểm chứng chức năng thực tế.

### Ghi nhận tiến độ

Cập nhật `docs/windows-plan.md` sau mỗi phần: commit, hành vi đã làm, lệnh kiểm tra, kết quả, bằng chứng runtime và việc còn mở. Trạng thái: **chưa làm / đang làm / bị chặn / đạt**. Sửa tài liệu khi bằng chứng mới thay thế ghi chú cũ; không làm mất lịch sử hữu ích.

## 3. Các chặng triển khai

### Chặng 0 — Xác minh hiện trạng và môi trường

**Công việc**

- Đồng bộ nhánh, xem CI mới nhất và kiểm kê chức năng theo hành trình người dùng.
- Xác định Python runtime, FFmpeg/FFprobe, fonts, app data và model root thực tế; không dùng cứng đường dẫn của máy công ty.
- Tạo fixture tổng hợp: audio ngắn, SRT/TXT, tên project và đường dẫn tiếng Việt. Không dùng bài hát/project riêng.
- Chạy lại check/build và regression liên quan đến UI `1d7b216`, worker `90ca587`.
- Lập danh sách lỗi theo mức: mất dữ liệu → chặn công việc → sai đầu ra → giao diện.

**Đạt khi:** có lệnh chạy app tái lập được, bảng tồn đọng cụ thể và kết quả kiểm tra đầu nhánh. Không chuyển sang mở rộng UI khi còn lỗi compile.

### Chặng 1 — Hoàn thiện UI editor

**Bố cục và hành vi**

- Trái: danh sách lời và tab nguồn media; giữa: preview; phải: thuộc tính; dưới cả ba cột: timeline toàn chiều ngang.
- Thanh phát nhạc luôn truy cập được. Cửa sổ thấp cho cuộn nội dung thay vì cắt mất điều khiển.
- “Căn lời” mở danh sách lời và thuộc tính thời gian; “Thiết kế” mở thuộc tính thiết kế. Chuyển chế độ không sửa dữ liệu project.
- Chia Inspector thiết kế thành nhóm thu gọn: chữ, song ca, nền/scene, tiêu đề/logo, countdown và xuất.
- Chọn từ trong danh sách hoặc timeline đồng bộ vùng chọn, Inspector và vị trí xem trước; không ngắt playback ngoài hành vi đã quy định.
- Empty state hướng dẫn nhập nhạc/lời. Điều khiển không khả dụng có trạng thái rõ; giữ resize panel và preferences.
- Không thêm chức năng mới ngoài luồng karaoke chỉ để lấp đầy giao diện.

**Đạt khi:** kiểm tra cửa sổ thường/maximize, viewport 1280×720 và 1440×900, scale 100%/125%/150%; không chồng panel, mất nút hoặc mất vùng timeline. Lưu ảnh runtime gắn với commit và viewport.

### Chặng 2 — Project và an toàn dữ liệu

**Công việc**

- Xác minh tạo/mở/lưu/Save As/đóng và mở lại trong process mới.
- Kiểm tra schema 1/2, unknown fields, ID, timing, role, Unicode path và media mapping.
- Relink file thiếu có hướng dẫn; không tự xóa timing khi thay đường dẫn nguồn.
- Save trước khi đổi project/đóng app; Hủy giữ phiên đang sửa.
- Kiểm chứng autosave, recovery sau crash, version snapshot và undo/redo.
- Lỗi import/restore/save giữ bản chính và media hiện có; không publish trạng thái mới trước khi transaction thành công.

**Đạt khi:** fixture chỉnh sửa được lưu, đóng process và mở lại không mất dữ liệu. Mô phỏng lỗi ghi file và hủy thao tác không phá bản lưu trước đó. Unknown fields được giữ qua round-trip.

### Chặng 3 — Playback, timeline và căn lời

**Công việc**

- Kiểm tra play, pause/resume, stop, seek liên tục và seek khi audio đang nạp.
- Playhead/tap-sync lấy thời gian từ playback clock, không dùng clock riêng của UI.
- Quick tap, giữ Space, key repeat, Backspace, Escape, mất focus và hết audio phải có kết quả xác định.
- Kéo thân/mép từ, mốc liên kết/độc lập, zoom/scroll và undo/redo hoạt động nhất quán.
- Kiểm tra nghe beat/reference và mapping anchors.
- Kiểm tra output mặc định, chuyển thiết bị và rút thiết bị; nếu thiếu phần cứng phải ghi đúng ca chưa chạy.

**Đạt khi:** phát nhạc → căn nhiều từ → chỉnh mốc → undo/redo → lưu/mở lại thành công; UI vẫn phản hồi khi nạp/phát audio, lỗi thiết bị không treo app.

### Chặng 4 — Preview và video xuất

**Công việc**

- Giữ layout contract chung giữa preview và exporter; tránh hai thuật toán bố trí độc lập.
- Kiểm tra Unicode/VNI/TCVN3, font, viền/bóng, sweep, hai vai hát và countdown.
- Kiểm tra nền màu/ảnh/video, tiêu đề, logo, scene và chuyển động.
- Audio xuất mặc định dùng beat gốc. Guide mix chỉ dùng khi người dùng chọn.
- Kiểm tra CPU encoder; chỉ đánh dấu NVENC/fallback đạt khi có bằng chứng phù hợp.
- Progress/cancel, output đã tồn tại, thư mục không ghi được, thiếu media/font và FFmpeg lỗi phải được xử lý rõ.

**Đạt khi**

- MP4 mở được, đủ video/audio, đúng kích thước và FPS chọn.
- Thời lượng lệch không quá 0,1 giây so với hợp đồng xuất của fixture; timing giữ độ chính xác 4 chữ số thập phân qua save/reopen.
- Golden raster hiện có đạt trong cùng môi trường render. Lấy frame trước/giữa/sau từ, đầu/cuối câu và chuyển scene để đối chiếu nội dung, vị trí, màu, timing.
- Kiểm tra riêng preview GPUI và video xuất; không suy ra parity UI chỉ từ ASS test.

### Chặng 5 — AI và batch thực tế

**Công việc**

- Giữ JSON Lines protocol v1. Kiểm chứng nhận dạng, căn toàn bài/câu, tách vocal, gợi ý vai và sync audio.
- AI cần xác nhận trước khi áp dụng; loại kết quả stale khi project/lời/media đã đổi.
- Kiểm tra thiếu model, crash, timeout, cancel và worker đóng stdout nhưng process còn sống; không để process mồ côi.
- Batch dùng snapshot bất biến, progress từng mục và resume sau restart.
- Kiểm tra snapshot bị sửa, media thiếu, output tồn tại và lỗi từng mục.

**Đạt khi:** chạy fixture tổng hợp qua worker thật với model được ghi rõ; batch ba project gồm mục hợp lệ, mục lỗi và mục hủy. Lỗi một mục không làm hỏng dữ liệu mục khác. Có bằng chứng crash/resume và cancellation.

### Chặng 6 — Tích hợp, đóng gói và bàn giao

**Công việc**

- Chạy fmt, Clippy và test workspace trên đầu nhánh; đối chiếu CI. Chạy Python/Node và packaging tests liên quan, ghi riêng lỗi baseline Python.
- Dùng `packaging/build_rust_portable.py` và `packaging/build_rust_installer.py`; không tạo hệ đóng gói mới khi script hiện tại đã đáp ứng.
- Đóng gói app, Python worker/runtime, FFmpeg, fonts và license cần thiết. Model tải riêng; không đưa cache/project cá nhân vào payload.
- Trên Windows/VM độc lập: cài mới → mở app → chạy bài mẫu → xuất MP4 → gỡ → cài lại.
- Xác minh data/model/cache ở ngoài vùng uninstall xóa; có đường quay lại app Python baseline.
- Tạo SHA-256, hướng dẫn cài/chạy, giới hạn đã biết và báo cáo nghiệm thu.

**Đạt khi:** portable và installer chạy được trên môi trường độc lập với máy build. Nếu chưa có môi trường này, chỉ bàn giao **bản ứng viên**, không đánh dấu dự án hoàn tất. Không cần public GitHub release hoặc updater E2E để đạt bản nội bộ.

## 4. Hợp đồng kỹ thuật và kiểm thử

- Không đổi project schema, public API hoặc worker protocol chỉ để đổi UI.
- Nếu thực sự cần đổi hợp đồng: ghi rõ đầu vào/đầu ra, tương thích ngược và migration; thêm fixture trước khi dùng cho dữ liệu người dùng.
- Domain/storage không phụ thuộc GPUI. AI chạy process riêng; stdout chỉ dành cho protocol, stderr cho log.
- Dùng Python baseline làm oracle hành vi/đầu ra; không port lại lỗi mất dữ liệu đã biết.

| Nhóm | Ca nghiệm thu bắt buộc |
|---|---|
| Dữ liệu | Schema 1/2, unknown fields, Unicode path, lỗi save, recovery |
| Editor | Chọn từ, kéo timing, undo/redo, SRT/TXT, đổi chế độ, resize/DPI |
| Playback | Pause/resume, seek, tap nhiều từ, reference anchors, lỗi device |
| Render | FHD ngang, video dọc, song ca, nền/overlay, cancel/lỗi export |
| AI | Thành công, lỗi, timeout, cancel, stale result, stdout đóng sớm, không rò process |
| Batch | Snapshot, lỗi từng mục, cancel, crash/restart |
| Phân phối | Portable, install/uninstall/reinstall, giữ data, SHA-256 |

Lệnh kiểm tra nền: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `cargo build -p vietkara-app --release`. Chạy test liên quan sau mỗi sửa; chạy bộ tích hợp đầy đủ tại mốc cuối. Ghi phiên bản FFmpeg, Python, model và danh sách skip cùng kết quả.

## 5. Prompt khởi động cho Claude Code

```text
Đọc CLAUDE_IMPLEMENTATION_PLAN.md trong repo và trực tiếp triển khai từ Chặng 0 đến Chặng 6. Mục tiêu là bản Windows dùng nội bộ, giữ Rust/GPUI + Python AI worker + FFmpeg hiện có.

Kiểm tra working tree, hướng dẫn repo, fetch và đầu remote trước khi sửa. Đừng làm lại phần đã hoàn thành; ưu tiên xác minh UI 1d7b216 và worker fix 90ca587. Máy thực hiện là máy cá nhân tại nhà, không có ràng buộc AhnLab và không bắt buộc -j 1; dùng cache và mức song song phù hợp tài nguyên.

Mỗi phần hoàn chỉnh: kiểm tra đúng phần thay đổi, cập nhật docs/windows-plan.md, commit và push ngay lên codex/rust-gpui-rewrite. Không force-push hoặc commit dữ liệu cá nhân/artifact lớn. Nếu thiếu môi trường kiểm chứng, ghi rõ và làm tiếp phần độc lập; không báo pass khi chưa kiểm chứng.

Hãy thực hiện code và hoàn thiện sản phẩm, không chỉ viết thêm kế hoạch. Tiếp tục tới bộ cài dùng được và báo cáo nghiệm thu; chỉ hỏi khi thiếu quyết định sản phẩm hoặc điều kiện bắt buộc không thể tự tìm.
```
