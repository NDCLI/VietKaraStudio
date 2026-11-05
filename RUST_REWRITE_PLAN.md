# VietKara Studio: kế hoạch viết lại bằng Rust + GPUI

## Mục tiêu

Viết lại ứng dụng desktop Windows thành app Rust dùng GPUI, giữ khả năng mở project hiện tại và đạt luồng làm karaoke hoàn chỉnh: nhập nguồn, căn lời, chỉnh timeline, xem trước, lưu project và xuất video. Nhánh nền: `codex/ui-redesign-1.1.4` tại commit `c8dae55` (bản 1.1.7). Nhánh triển khai riêng: `codex/rust-gpui-rewrite`.

## Quyết định kiến trúc ban đầu

- Dùng Rust cho giao diện, project domain, điều phối job, playback, timeline và lưu trữ.
- Dùng GPUI cho cửa sổ và controls. Trước khi viết editor đầy đủ, phải kiểm chứng GPUI trên Windows với nhập tiếng Việt/IME, resize, file picker và vẽ liên tục.
- Giữ FFmpeg/FFprobe làm media backend và renderer đầu tiên để giữ kết quả xuất tương đồng.
- Giữ worker Python tách tiến trình cho Demucs, Whisper và stable-ts trong giai đoạn đầu; giao tiếp bằng JSON Lines có version, stdout dành riêng cho protocol, stderr dành cho log, có progress/cancel.
- Project cũ phải được import không phá hủy; lưu phiên bản/schema và migration tường minh.
- Playback clock là nguồn thời gian chuẩn; UI không tự làm clock. Tác vụ media/AI không chạy trên UI thread.

## Các giai đoạn và tiêu chí hoàn tất

### 0. Kiểm kê và baseline

- Lập ma trận tính năng từ nhánh nền: media slots, project JSON, timing, duet roles, presets, typography, title/logo/background, export, batch render, archives, models/cache, updater.
- Thu thập các project mẫu không chứa dữ liệu cá nhân và đầu ra tham chiếu; ghi rõ codec, FPS, độ phân giải, font, preset.
- Ghi nhận schema project, ASS generation, FFmpeg command, worker protocol và thư mục data hiện tại.
- Tiêu chí: ma trận có owner Rust/worker/FFmpeg, mức ưu tiên, mẫu kiểm chứng và tương thích cũ.

### 1. Nguyên mẫu rủi ro GPUI + media

- Một cửa sổ GPUI trên Windows, layout panel co giãn, menu/chọn file, IME tiếng Việt, drag/drop.
- Nạp audio, phát/dừng/seek, waveform tĩnh; đồng bộ playhead với audio clock.
- Timeline prototype có zoom, kéo một mốc lời và cập nhật timestamp.
- So sánh hiển thị chữ Việt, font, DPI scaling và vẽ waveform ở kích thước project thực tế.
- Tiêu chí: build/run được trên Windows mục tiêu; thao tác không block UI; ghi lại các hạn chế GPUI và quyết định tiếp tục/dừng.

### 2. Nền workspace và domain Rust

Workspace crates dự kiến:

| Crate | Phạm vi |
|---|---|
| `vietkara-app` | khởi động, cửa sổ GPUI, điều phối màn hình |
| `vietkara-domain` | Project, media, lyric/word timing, duet, preset, validation, undo/redo |
| `vietkara-storage` | serde, schema version, migration, autosave, paths và project archive |
| `vietkara-jobs` | job lifecycle, progress, cancellation và subprocess |
| `vietkara-media` | FFmpeg/FFprobe, probe, waveform, playback abstraction, audio sync |
| `vietkara-render` | ASS/layout mapping, export profiles, render queue và output validation |
| `vietkara-ai-bridge` | Python worker protocol, model status/cache, AI tasks |

- Thiết lập error model, tracing/logging, config/data directories, crash recovery và serialization fixtures.
- Tiêu chí: domain và migration có thể chạy độc lập khỏi GUI; mở/lưu project round-trip giữ dữ liệu.

### 3. Vertical slice editor

- Tạo/mở project; nạp beat/audio; nhập SRT hoặc lyrics; nghe và căn bằng Space.
- Hai hàng timeline, waveform, playhead, zoom/scroll, chọn đoạn, sửa start/end, liên kết/độc lập.
- Autosave, undo/redo, cảnh báo nguồn media thiếu và relink.
- Tiêu chí: hoàn thành bài thử “mở nhạc → căn lời → sửa → lưu → đóng → mở lại” không mất dữ liệu.

### 4. Preview và export tương thích

- Chuyển project sang ASS/FFmpeg renderer hiện có; tái sử dụng preset và quy tắc timing.
- Preview khớp export về font, line wrapping, vị trí, outline/shadow, karaoke sweep, duet colors/icons, title/logo/background/countdown.
- Hỗ trợ các format/resolution/FPS hiện có; progress, cancel, export folder và render info.
- Tiêu chí: frame comparison trên fixture chuẩn và kiểm tra âm thanh xác nhận beat gốc được dùng theo đúng lựa chọn.

### 5. AI bridge và phần còn lại

- Worker Python phiên bản hóa cho tách vocal, nhận dạng/căn lời, role suggestion và model download.
- Truyền đường dẫn/metadata thay vì nhúng media; progress và cancel xuyên suốt; tái sử dụng cache hiện có.
- Bổ sung batch render, audio sync, project archive, presets import/export, fonts/assets và updater.
- Tiêu chí: task hủy an toàn, lỗi worker rõ ràng, app tiếp tục dùng được; dữ liệu hiện có không bị ghi đè.

### 6. Đóng gói và chuyển người dùng

- Installer/portable Rust app; phân phối FFmpeg và Python AI runtime theo thành phần; model tải riêng.
- Mở project bản cũ, sao lưu trước migration, rollback/update rõ ràng.
- Chạy trên máy Windows sạch, máy CPU-only và NVIDIA; kiểm tra đường dẫn có dấu/ký tự Unicode.
- Chuyển mặc định sang app mới chỉ khi đạt parity checklist; giữ cách mở app cũ trong thời gian chuyển tiếp.

## Ưu tiên MVP

MVP dùng được gồm: project create/open/save, audio playback, lyrics/SRT import, Space tap-sync, waveform/timeline edit, autosave/undo, preview cơ bản và MP4 export bằng FFmpeg. Tách vocal/ASR, preset nâng cao, title/logo, duet suggestion, batch render và updater đi sau vertical slice.

## Rủi ro cần giải quyết sớm

1. GPUI đang phát triển nhanh; chốt revision/version và khóa dependency sau prototype.
2. Playback, device changes, seek và đồng bộ UI cần abstraction riêng, không gắn clock vào frame render.
3. Preview và FFmpeg export có thể lệch font/layout; dùng cùng dữ liệu layout và so sánh frame tự động.
4. Worker AI và model/runtime nặng; giữ process tách biệt để UI, packaging và cập nhật ít phụ thuộc nhau.
5. Project cũ có đường dẫn và trường tùy chọn; migration phải giữ unknown fields hoặc báo rõ dữ liệu chưa hỗ trợ.
6. Rust chưa có trong PATH của môi trường hiện tại; cần cài stable MSVC toolchain cùng Visual Studio C++ Build Tools/Windows SDK trước khi build prototype.

## Thứ tự công việc đầu tiên

1. Kiểm kê schema/project, FFmpeg/ASS export và các luồng media trên nhánh nền.
2. Cài và xác nhận Rust stable MSVC, Cargo, C++ Build Tools và Windows SDK.
3. Tạo GPUI window prototype, xác minh IME, file picker, resize và render trên máy Windows.
4. Tạo workspace crates, bắt đầu `vietkara-domain` và `vietkara-storage` với fixture project cũ.
5. Làm playback + tap-sync vertical slice trước khi port các panel nâng cao.

## Ước lượng và nhịp giao hàng

Ước lượng ban đầu cho một kỹ sư toàn thời gian đã quen Rust/media: prototype 2–3 tuần; domain/editor MVP 4–6 tuần; preview/export parity 3–5 tuần; AI/nâng cao 3–5 tuần; ổn định và đóng gói 2–3 tuần. Tổng khoảng 14–22 tuần, cần hiệu chỉnh sau prototype GPUI và inventory. Mỗi giai đoạn giao một bản chạy được, không đợi port toàn bộ mới kiểm chứng.

## Điều kiện nghiệm thu tổng thể

- Project cũ mở được và migration không ghi đè bản gốc.
- Không mất lyric timing, preset, media mapping hoặc thông số export khi lưu lại.
- Playback/timeline responsive; tác vụ AI/render có progress và cancel.
- Preview và file xuất tương ứng trong bộ fixture đã thống nhất.
- Installer chạy trên Windows sạch; model/cache không bị xóa khi app cập nhật.
