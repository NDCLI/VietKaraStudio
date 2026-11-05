# VietKara Studio: kế hoạch chuyển dần sang Rust + GPUI

> **Trạng thái kế hoạch:** cập nhật sau khi kiểm kê workspace hiện tại.
>
> **Nguyên tắc:** không “đập đi viết lại” một lần. Ứng dụng Python hiện tại vẫn là production baseline cho đến khi từng lát chức năng Rust đạt parity, có fixture kiểm chứng và có đường rollback.
>
> **Tiến độ 2026-11-05 — Giai đoạn 0 + đầu Giai đoạn 2 (xong, kiểm chứng trên Linux):**
> - `docs/compatibility-matrix.md` mới: ma trận P0/P1/P2, hợp đồng tương thích
>   Python ↔ Rust, fixture v1/v2.
> - `rust-toolchain.toml` ghim Rust **1.99.1** (không host triple; file cũ
>   `stable-x86_64-pc-windows-msvc` làm `cargo` lỗi trên mọi host không
>   phải Windows).
> - `vietkara-domain`: ID 32 hex (`Uuid::simple()`), schema thiếu = 1,
>   migration v1→v2 đúng `timing.validate_words`/`validate_anchors`
>   (đã đối chiếu trực tiếp với Python trên cùng fixture), save atomic,
>   typed word fields (`estimated`/`locked`/`section`/`label_color`/`row`/…).
> - `vietkara-storage` mới: atomic save, manifest SHA-256, version snapshot,
>   restore an toàn (verify → stage → swap, rollback), bundle ZIP
>   (giới hạn 3 GiB / 2.000 entry / 20 MiB, allowlist media, chặn path
>   traversal). `cargo fmt --check`, `cargo clippy -p vietkara-domain
>   -p vietkara-storage -- -D warnings`, `cargo test` 14/14 pass;
>   `pytest tests/test_timing.py` 4/4, `node --test` tapping/timeline 14/14.
> - **Chưa xong tại thời điểm kiểm chứng Linux:** spike GPUI/Windows/IME/
>   playback (Giai đoạn 1); khi đó `cargo test --workspace` (gồm
>   `vietkara-app`/GPUI) chưa chạy được trên Linux. Kết quả Windows cập nhật
>   riêng bên dưới.
>
> **Tiến độ 2026-11-05 (tiếp) — các phần làm được trên Linux, Giai đoạn 3–5
> (phần headless, không GUI/thiết bị):**
> - `vietkara-jobs`: lifecycle/cancel/persist/recover như `engine.Job`,
>   batch queue (`first_pending`, retry trừ `failed`).
> - `vietkara-media`: FFprobe JSON + fallback `ffmpeg -i`, WAV reader,
>   waveform (peak = max tuyệt đối trên **mọi** kênh, như Python),
>   audio metrics, playback abstraction với fake clock (`NullPlayer`).
> - `vietkara-render`: helper ASS (`stamp`/`color`/`literal`/`blend`/
>   `animation`/`rounded_box`) đối chiếu Python, render profile,
>   builder lệnh FFmpeg (nền màu + beat gốc / guide `amix`),
>   `verify_output` bằng ffprobe. **Chưa port** full `build_ass` /
>   `display_plan` (cần đo font) — Giai đoạn 4 sau khi layout contract
>   ổn định.
> - `vietkara-ai-bridge`: protocol JSON Lines v1, spawn/timeout/cancel/
>   crash recovery. **Lưu ý:** `worker.py` hiện tại chưa nói protocol
>   này (nó dùng request file + `OBS_PROGRESS`); cần một entry point
>   worker mới trước khi AI thật chạy qua crate này.
> - Runbook Windows: `docs/windows-spike.md` +
>   `scripts/windows-spike-check.ps1` (Giai đoạn 1, chạy trên Windows).
> - Kiểm chứng Linux: `cargo test` 6 crate **62/62 pass**; `cargo fmt
>   --check` và `cargo clippy -- -D warnings` sạch; Python
>   `test_timing.py` 4/4, JS tapping/timeline 14/14.
>
> **Tiến độ 2026-11-05 — Windows spike tự động (Giai đoạn 1, thủ công còn mở):**
> - Checkout nhánh mới nhất `codex/rust-gpui-rewrite` vào local branch riêng
>   `codex/rust-gpui-rewrite-local`, baseline `eb0df8d`.
> - Windows 11 Pro x64: PowerShell 5.1 chạy thành công
>   `scripts/windows-spike-check.ps1`; 6 crate headless **62/62**, `cargo fmt
>   --check`, `cargo clippy --all-targets -- -D warnings` và
>   `cargo check -p vietkara-app`, `cargo test --workspace` và workspace
>   clippy đều pass. App GPUI chạy và mở cửa sổ; nhãn tiếng Việt hiển thị
>   đúng dấu, maximize/restore hoạt động.
> - Baseline Python `pytest tests -q` **167/167 pass** và Node tapping/
>   timeline **14/14 pass**. Python suite cần `resources/source`, vốn không
>   có trong Git checkout mới; dùng junction tạm tới asset local có sẵn,
>   rồi gỡ junction sau test.
> - Sửa import `ApplicationExt` không có trong GPUI 0.2.2; làm fake-worker
>   tests chọn `python` trên Windows; sửa định dạng `vietkara-jobs` theo
>   rustfmt 1.99.1; làm script spike tương thích PowerShell 5.1.
> - UI hiện vẫn là mock. DPI, IME, file picker, save/load qua đường dẫn
>   Unicode, kéo resize và responsiveness chưa được xác minh; UI helper lỗi
>   khi đọc lại state sau phím Space. **Đã quyết định GO để tiếp tục W3** sau bằng chứng runtime và phản hồi
>   thủ công của người dùng; giới hạn còn theo dõi ở `docs/windows-spike.md` §5.
>
> **Kế hoạch còn lại — làm trên Windows:** xem `docs/windows-plan.md`
> (điểm xuất phát commit `df1a652`). W1 kiểm chứng tự động đã pass;
> W2 đã có quyết định GO, còn các mục kiểm chứng chi tiết được theo dõi;
> các bước tiếp theo theo thứ tự:
> W3 undo/redo + autosave, W4 vertical slice MVP, W5 full ASS
> layout + render parity, W6 worker JSONL + AI + batch, W7 đóng gói +
> rollout — kèm lệnh kiểm chứng, cách ghi kết quả và push.

> **Mốc đối chiếu:** baseline Python là nhánh `codex/ui-redesign-1.1.4`, commit `c8dae55` (release 1.1.7); không dùng `main` tại `3798f4e` làm baseline tính năng. Nhánh Rust hiện là `codex/rust-gpui-rewrite`.

> **Tiến độ 2026-11-06 — lát giao diện/lyrics W3 (đã kiểm tra tự động):**
> - Bố cục GPUI đã chuyển gần cấu trúc editor cũ: danh sách lời / nguồn media
>   bên trái, preview theo khung 16:9 hoặc 9:16 và timeline ở giữa, inspector
>   từ / thiết kế bên phải.
> - Inspector cho phép đổi chế độ karaoke/phụ đề/lời theo câu, tỉ lệ khung,
>   cỡ chữ, chữ nghiêng và màu quét; cập nhật qua command `SetSettings`, dirty
>   state và autosave.
> - Thêm nhập TXT mỗi dòng một câu, ước lượng mốc theo độ dài beat theo quy tắc
>   `timing.estimate_words`, giữ xác nhận trước khi thay toàn bộ lời; thêm test
>   domain cho số từ, ranh giới dòng và dữ liệu lỗi.
> - Kiểm tra sau thay đổi: `cargo fmt --check`, `cargo test --workspace`
>   **77/77**, workspace clippy với `-D warnings`, workspace check và
>   `cargo build -p vietkara-app` đều pass; Python baseline **167/167** và
>   Node **17/17** cũng pass.
> - Đây mới là lát giao diện/lyrics đầu tiên, chưa đạt parity thiết kế đầy đủ;
>   giao diện mới chưa được kiểm tra trực tiếp bằng ảnh runtime, và preview
>   hiện vẫn là khung demo chứ chưa phải preview renderer thật.

## 1. Mục tiêu và phạm vi

Chuyển dần VietKara Studio thành ứng dụng desktop Windows dùng Rust + GPUI cho UI, domain, lưu trữ, điều phối tác vụ và media orchestration; tiếp tục tái sử dụng FFmpeg/FFprobe và worker Python AI trong các giai đoạn đầu.

Phạm vi cuối cùng cần giữ được:

- Mở, chỉnh sửa, lưu và trao đổi project hiện có.
- Nhập reference/beat/vocal, SRT hoặc lyrics; căn lời thủ công và bằng AI.
- Playback, waveform, timeline hai hàng, zoom/scroll, liên kết/độc lập mốc thời gian.
- Preview và export có kết quả tương thích với renderer hiện tại.
- Preset chữ, font tiếng Việt/VNI/TCVN3, duet roles, title/logo/background/countdown.
- Autosave, version archive, relink media, batch render, cache/model, updater và đóng gói Windows.

Không nằm trong MVP đầu tiên: thay toàn bộ Python AI bằng Rust, tự viết audio decoder/player, tự viết video renderer hoặc thay GPUI khi chưa có bằng chứng cần thiết.

## 2. Hiện trạng đã kiểm kê

### 2.1. Phần Rust hiện có

Workspace hiện có bảy crate:

- `crates/vietkara-domain`: model `Project`, `Word`, `Anchor`, schema v2,
  validation/migration v1→v2, ID 32 hex, typed word fields và giữ unknown
  fields qua `serde(flatten)`.
- `crates/vietkara-storage`: atomic save, manifest SHA-256, version snapshot,
  restore an toàn, bundle ZIP staging/validation.
- `crates/vietkara-jobs`: job lifecycle, cancel, persistence/recovery và
  batch queue state machine; chưa nối vào app UI.
- `crates/vietkara-media`: FFprobe/fallback, WAV reader, waveform/metrics,
  playback trait và fake clock (`NullPlayer`); chưa phát qua thiết bị thật.
- `crates/vietkara-render`: ASS helpers, render profile, FFmpeg argv và
  output verification; chưa có `build_ass`/`display_plan` đầy đủ.
- `crates/vietkara-ai-bridge`: JSON Lines v1 process bridge với timeout,
  cancel và crash handling; worker Python JSONL entry point chưa có.
- `crates/vietkara-app`: GPUI 0.2.2 mock editor; `cargo check` và chạy cửa
  sổ pass trên Windows, nhưng chưa có action thật cho mở file, playback,
  timeline, import, save hay export.

Kiểm chứng headless: `cargo test` 6 crate **62/62 pass** trên Linux và
Windows; `cargo fmt --check`/`cargo clippy` sạch. Trên Windows,
`cargo check -p vietkara-app`, `cargo test --workspace`, workspace clippy
và chạy app đều pass; xem giới hạn kiểm tra thủ công trong
`docs/windows-spike.md`.

### 2.2. Production baseline Python

Các năng lực cần coi là hợp đồng tương thích hiện đang nằm trong các module sau:

| Năng lực | Baseline hiện tại |
|---|---|
| HTTP/UI và project lifecycle | `app.py`, `static/studio.js` |
| Schema/migration project | `app.py` và `vietkara-domain` đang bắt đầu tái hiện schema v2 |
| Version archive, bundle, media fingerprint | `project_archive.py` |
| Job progress/cancel/recovery | `app.py`, `worker.py`, `batch_render.py` |
| FFmpeg/FFprobe và media conversion | `engine.py`, `media_tools.py`, `app.py` |
| ASS/layout/font encoding | `layout.py`, `subtitles.py`, `bundled_assets.py` |
| AI recognition/alignment/separation/cache | `app.py`, `worker.py`, `analysis_cache.py`, các module AI liên quan |
| Audio sync và role suggestion | `audio_sync.py`, `duet_roles.py` |
| Batch render | `batch_render.py` |
| Updater/packaging | `updater.py`, `packaging/`, `docs/DISTRIBUTION.md` |
| Hợp đồng kiểm thử | `tests/` và các lệnh test trong `README.md` |

### 2.3. Khoảng trống và ràng buộc đã xác nhận

- Project JSON schema hiện là v2; fixture schema 1/2 xác minh migration,
  ID 32 hex, typed word fields và unknown fields trong các đường round-trip
  đã có. `settings`, `reference`, `beat`, `alignment`, `scenes` vẫn còn
  những field dạng JSON linh hoạt; settings chưa được typed đầy đủ.
- `vietkara-storage` đã có atomic save, manifest media, SHA-256, giới hạn
  ZIP 3 GiB/2.000 entry, project JSON 20 MiB, allowlist tên media và restore
  an toàn theo verify → stage → swap/rollback. Các crate storage tests
  xác minh fixture; thao tác UI và kiểm chứng path Unicode trên Windows
  còn mở.
- Giữ khác biệt an toàn với Python: `restore_version` stage và xác minh
  fingerprint trước khi swap; bundle import kiểm tra trong staging trước
  khi đưa media vào project.
- `vietkara-jobs` đã port lifecycle/recovery và batch queue model, nhưng
  chưa chạy render jobs từ UI; batch render production flow còn thiếu.
- Playback hiện chỉ có abstraction/fake clock, chưa có audio device thật;
  app UI chưa có action playback, timeline, open/save, import/export.
- Full ASS/layout, worker JSONL entry point thật, autosave/recovery gắn UI,
  undo/redo, preset/scene, updater và đóng gói vẫn còn trong các giai đoạn sau.
- Beat gốc là nguồn audio xuất video; vocal tách riêng chỉ phục vụ nhận dạng/căn lời theo README.
- Môi trường mục tiêu là Windows 10/11 x64, có đường dẫn Unicode, CPU-only và NVIDIA.

## 3. Quyết định kiến trúc

1. **Rust thay theo lát dọc.** Mỗi lát gồm domain/storage, UI, job/media adapter, test fixture và đường chạy thật; không port theo từng lớp cô lập quá lâu.
2. **Python app giữ vai trò oracle về tương thích.** Dùng baseline để xác định format và hành vi người dùng cần giữ; không port lỗi toàn vẹn dữ liệu hoặc bảo mật đã biết. Quy tắc parity và quy tắc an toàn phải được phân biệt trong fixture/acceptance.
3. **FFmpeg/FFprobe tiếp tục là backend đầu tiên.** Rust chỉ điều phối process, parse kết quả, chuẩn hóa lỗi và quản lý lifecycle.
4. **AI worker chạy process tách biệt.** Giao tiếp bằng JSON Lines có `protocol_version`, `request_id`, progress, cancel và error; stdout chỉ dành cho protocol, stderr dành cho log.
5. **Playback clock là nguồn thời gian chuẩn.** UI/timeline nhận snapshot clock và không tự chạy clock theo frame render.
6. **Schema model và file format tách nhau.** Rust dùng typed model cho phần đã hiểu, nhưng phải giữ unknown JSON fields; migration có version, fixture và test round-trip.
7. **Mọi thao tác thay media/restore/export đều có transaction hoặc bản sao an toàn.** Không xóa nguồn gốc trước khi kiểm tra file đích và fingerprint.
8. **Khóa phiên bản phụ thuộc sau prototype.** Ghim phiên bản Rust cụ thể (không chỉ channel `stable`), GPUI, FFmpeg và worker protocol trong toolchain/build manifest.

## 4. Các giai đoạn thực hiện

### Giai đoạn 0 — Baseline và hợp đồng tương thích

**Mục tiêu:** biến hành vi Python hiện tại thành fixture có thể kiểm chứng.

**Công việc:**

- Chốt commit baseline của app Python và phiên bản GPUI/Rust toolchain mục tiêu.
- Lập ma trận tính năng: P0 bắt buộc cho MVP, P1 cần cho parity, P2 nâng cao.
- Tạo fixture project JSON schema 1/2 có unknown fields, media mapping, words, anchors, scenes, settings và batch options; không đưa dữ liệu cá nhân vào repo.
- Lưu đầu ra tham chiếu: normalized project JSON, ASS, FFmpeg command, probe metadata, render info và vài frame đại diện.
- Ghi rõ thư mục dữ liệu, cache/model, archive, updater và quy tắc relink.
- Ghi tiêu chí sai số: thời gian, kích thước frame, màu/font/layout và audio stream.

**Hoàn tất khi:** có `compatibility-matrix.md` hoặc tài liệu tương đương, fixture versioned, lệnh chạy lại được và mọi hạng mục có owner Rust/Python/FFmpeg.

### Giai đoạn 1 — Spike rủi ro GPUI, Windows và playback

**Mục tiêu:** kiểm chứng các rủi ro có thể làm thay đổi kiến trúc trước khi đầu tư editor.

**Công việc:**

- Window GPUI tối thiểu trên Windows 10/11: resize, DPI, menu, file picker, drag/drop.
- Nhập tiếng Việt qua IME, paste, phím Space và phím tắt; kiểm tra font Unicode/VNI/TCVN3.
- Dùng file fixture để probe và phát audio qua một abstraction có clock, seek, pause, device/error state.
- Vẽ waveform tĩnh và playhead; timeline prototype kéo một mốc và cập nhật timestamp.
- Đo UI responsiveness khi probe/render/worker chạy ngoài UI thread.

**Hoàn tất khi:** build/run được trên máy mục tiêu, IME và đường dẫn Unicode hoạt động, playback không block UI, có báo cáo giới hạn GPUI và quyết định tiếp tục với revision đã khóa.

**Điểm dừng:** nếu GPUI không đáp ứng IME, file dialog hoặc render ổn định, giữ domain Rust và thay UI shell trước khi port editor.

### Giai đoạn 2 — Domain, storage và migration đáng tin cậy

**Mục tiêu:** biến Rust thành nguồn xử lý project an toàn, độc lập GUI.

**Crate đề xuất:**

| Crate | Trách nhiệm ban đầu |
|---|---|
| `vietkara-domain` | Project/Word/Anchor, media refs, settings đã typed, validation, commands và undo model |
| `vietkara-storage` | load/save atomic, schema migration, autosave, paths, archive/version và bundle |
| `vietkara-fixtures` hoặc test support | fixture project, golden JSON/ASS và helper so sánh |

**Công việc:**

- Hoàn thiện model schema v1/v2 và migration tường minh; phân biệt project schema với archive format.
- Giữ ID project 32 ký tự hex, tương thích với đường dẫn và API Python; kiểm thử ID/line ID cũ và mới.
- Không đánh dấu schema 1 là schema 2 chỉ bằng cách đổi số version: chuẩn hóa fields/word IDs/line IDs theo quy tắc tương đương Python rồi mới ghi schema mới.
- Giữ unknown fields ở cấp project/word/settings nơi cần thiết.
- Port quy tắc archive từ `project_archive.py`: manifest, SHA-256, giới hạn ZIP/entry, allowlist tên media; bổ sung restore/import atomic có rollback khi lỗi.
- Save atomic bằng file tạm + rename; autosave/recovery không làm hỏng bản chính.
- Thiết kế command/event cho undo/redo thay vì để UI sửa trực tiếp struct.
- Test property/negative cases: duplicate IDs, NaN/negative timing, archive traversal, file thiếu/sai fingerprint, schema mới hơn.

**Hoàn tất khi:** crate chạy không cần GUI; load/save/migrate/restore round-trip qua fixture schema 1/2; bản gốc và media hiện có không bị sửa khi migration/import/restore lỗi; test bảo đảm unknown fields và ID tương thích không mất/đổi sai.

### Giai đoạn 3 — Vertical slice MVP: project → playback → tap-sync → save

**Mục tiêu:** có một luồng Rust dùng được hằng ngày, nhưng vẫn có thể quay về Python app.

**Công việc:**

- `vietkara-app` có màn hình create/open project, relink media và save/close/reopen.
- `vietkara-jobs` tối thiểu: lifecycle, progress, cancellation, error, persisted recovery record.
- `vietkara-media` tối thiểu: FFprobe, audio source, waveform cache và playback abstraction.
- Import SRT/lyrics; chọn câu/từ; Space tap-sync; chỉnh start/end; liên kết/độc lập mốc.
- Timeline hai hàng với zoom/scroll, playhead theo playback clock và cảnh báo media thiếu.
- Undo/redo, autosave và version snapshot trước thao tác phá hủy.

**Hoàn tất khi:** bài test “mở nhạc → nhập lời → căn bằng Space → sửa mốc → lưu → đóng → mở lại” chạy được với fixture; job hủy không để lại project hỏng; UI vẫn phản hồi.

### Giai đoạn 4 — Preview/export parity

**Mục tiêu:** Rust tạo preview/export bằng cùng dữ liệu layout và đạt parity đo được với Python.

**Crate đề xuất:**

- `vietkara-render`: layout model, ASS generation, render profile, FFmpeg command, output validation.
- Mở rộng `vietkara-media`: probe, waveform/audio metadata, sync metadata.

**Công việc:**

- Port theo golden output từ `layout.py`, `subtitles.py`, `bundled_assets.py`; không copy mù toàn bộ logic.
- Chuẩn hóa font selection, VNI/TCVN3, line wrapping, outline/shadow, karaoke sweep, duet colors/icons.
- Bổ sung title/logo/background/countdown, format/resolution/FPS và audio stream policy.
- Preview dùng cùng layout data với exporter; không có hai thuật toán positioning độc lập.
- Progress/cancel, export folder, render info và kiểm tra output.
- Frame comparison và audio-stream assertion trên fixture chuẩn; sai lệch phải có ngưỡng được ghi rõ.

**Hoàn tất khi:** xuất MP4 tối thiểu đạt parity fixture, audio dùng đúng beat gốc, preview/export không lệch các thuộc tính đã cam kết, lỗi FFmpeg hiển thị được nguyên nhân có thể hành động.

### Giai đoạn 5 — AI bridge, audio sync và batch jobs

**Mục tiêu:** thay orchestration Python bằng Rust mà không thay model/runtime ngay.

**Công việc:**

- `vietkara-ai-bridge`: protocol version, request ID, worker discovery, model status/cache, progress/cancel, timeout và crash recovery.
- Port các flow: separation, recognition/alignment, align region, role suggestion; media truyền bằng path + metadata, không nhúng bytes.
- Port/reuse audio sync và cache với fingerprint nguồn, model, language, options.
- Batch render queue: record bền vững, resume sau restart, first pending item, per-item error và cancel.
- Giới hạn tài nguyên, CPU-only/NVIDIA, model download riêng và không xóa cache khi update.

**Hoàn tất khi:** worker lỗi/hủy không treo app; batch resume đúng item; cache không trả nhầm kết quả; project không bị ghi đè trước khi người dùng xác nhận kết quả AI.

### Giai đoạn 6 — Parity nâng cao, đóng gói và chuyển người dùng

**Mục tiêu:** thay production baseline với rollback rõ ràng.

**Công việc:**

- Preset import/export, scenes/assets, archive đầy đủ, fonts, updater và health diagnostics.
- Portable/installer Windows; đóng gói FFmpeg và Python runtime theo thành phần; model tải riêng.
- Máy sạch Windows 10/11 x64, đường dẫn Unicode, CPU-only và NVIDIA; kiểm tra upgrade từ bản cũ.
- Smoke test sau cài đặt, crash recovery, file association, update/rollback và giữ nguyên `data/`, model/cache.
- Chạy song song app cũ/app mới theo opt-in; diagnostics chỉ lưu cục bộ và không thu media/project.
- Chỉ đổi mặc định sau khi checklist parity P0/P1 đạt và có bản rollback.

**Hoàn tất khi:** installer/portable chạy trên máy sạch; mở project cũ không phá hủy; update không xóa data; bản cũ vẫn mở được nếu rollback.

## 5. Ưu tiên tính năng và định nghĩa MVP

| Ưu tiên | Phạm vi | Điều kiện đưa vào |
|---|---|---|
| P0 — lát dọc dùng được | Project schema 1/2 + migration an toàn; open/save/autosave/relink; nhập SRT/lyrics; phát beat, seek, tap-sync; chỉnh timeline; preview cơ bản; MP4 export dùng beat gốc; progress/cancel/error | Cần cho một bài hát đi trọn luồng trong app Rust và quay lại Python nếu gặp lỗi |
| P1 — parity sản phẩm | Archive/version restore, undo/redo đầy đủ, tách vocal/ASR/alignment qua worker, audio sync, duet roles, presets/font, title/logo/background/countdown, batch render | Chỉ bắt đầu sau khi P0 ổn định; mỗi nhóm phải có fixture riêng |
| P2 — phát hành/chất lượng vận hành | Updater, installer/portable parity, diagnostics, tối ưu GPU/cache và tiện ích nâng cao | Chỉ trước khi chuyển production baseline; phải qua kiểm tra máy sạch và rollback |

MVP Rust không phải toàn bộ sản phẩm. MVP gồm:

1. tạo/mở/save project schema 1/2, migration và giữ unknown fields;
2. archive/autosave cơ bản và relink media;
3. nhập SRT/lyrics;
4. nạp audio, playback, seek và Space tap-sync;
5. waveform/timeline tối thiểu, chỉnh mốc, undo/redo;
6. preview cơ bản;
7. MP4 export qua FFmpeg với beat gốc;
8. progress/cancel/error cho các job trên.

Archive/version đầy đủ được xếp P1 về parity, nhưng MVP vẫn cần autosave và một bản khôi phục an toàn tối thiểu trước thao tác phá hủy; không được bỏ khả năng phục hồi chỉ để rút ngắn MVP. Tách vocal/ASR, role suggestion, audio sync nâng cao, duet presentation đầy đủ, title/logo/background nâng cao, batch render, updater và installer parity là P1/P2 sau MVP.

## 6. Ma trận crate theo thời điểm tạo

Không tạo tất cả crate ngay từ đầu. Thứ tự đề xuất:

1. `vietkara-domain` — đang có, hoàn thiện ở Giai đoạn 2.
2. `vietkara-storage` — tạo cùng migration/archive fixture.
3. `vietkara-jobs` — tạo trước khi có playback/render async thật.
4. `vietkara-media` — tạo ở spike và mở rộng ở vertical slice.
5. `vietkara-render` — chỉ tạo khi domain/layout contract ổn định.
6. `vietkara-ai-bridge` — tạo khi MVP playback/edit/save đã ổn.
7. `vietkara-app` — giữ là composition root/UI shell, không chứa business rules.

Mỗi crate phải có API nhỏ, test độc lập và không phụ thuộc ngược vào `vietkara-app`.

## 7. Tiêu chí chất lượng và kiểm thử

Mỗi giai đoạn phải chạy được trên CI/local bằng lệnh ghi trong repository:

- Rust: `cargo fmt --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` khi dependency đã ổn định.
- Python baseline: `.venv/Scripts/python.exe -m pytest tests -q`.
- JavaScript baseline: `node --test tests/tapping.test.js tests/timeline_edit.test.js`.
- Golden tests: project JSON normalized, ASS text, FFmpeg argv, probe metadata và frame/audio assertions.
- Integration tests trên Windows cho IME, path Unicode, file dialog, process cancellation và installer smoke.

Không gọi một giai đoạn là hoàn tất chỉ vì compile thành công; phải có fixture, hành trình người dùng và tiêu chí hồi quy tương ứng.

## 8. Rủi ro và cách giảm thiểu

1. **GPUI thay đổi nhanh:** khóa version sau spike; cô lập UI khỏi domain; có điểm dừng thay UI shell.
2. **Playback/device/seek khó:** định nghĩa clock/player trait sớm; test bằng fake clock và fixture audio.
3. **Preview lệch export:** một layout model dùng chung; golden frame/font tests trước khi thêm hiệu ứng.
4. **Schema Python linh hoạt:** typed known fields + unknown map; migration không phá hủy; fixture cho field lạ.
5. **Archive/media độc hại hoặc quá lớn:** allowlist path, giới hạn size/count, fingerprint và ghi tạm an toàn.
6. **Worker AI treo hoặc crash:** process isolation, timeout, cancel, request ID, stderr log và recovery state.
7. **Scope creep do parity:** P0/P1/P2 và ma trận tính năng; không port feature trước khi MVP path ổn.
8. **Đóng gói nặng/phụ thuộc GPU:** tách runtime/model, CPU fallback và test máy sạch.
9. **Hai app cùng sửa project:** khóa/phiên bản file hoặc cảnh báo conflict; chỉ một app làm writer trong migration rollout.

## 9. Thứ tự công việc ngay sau kế hoạch này

1. ✅ Bổ sung fixture project v1/v2, fixture archive và golden ASS/FFmpeg output từ baseline Python. — *2026-11-05: fixture v1/v2 và fixture archive/bundle trong test của `vietkara-storage` đã xong; golden ASS/FFmpeg output vẫn còn (thuộc Giai đoạn 4, làm sau khi layout contract ổn định).*
2. ✅ Hoàn thiện `vietkara-domain` validation/migration contract và tạo `vietkara-storage`.
3. ⏳ Hoàn tất GPUI/Windows spike có IME, DPI, file picker, đường dẫn Unicode
   và responsiveness. — *Windows 11: automated checks, `cargo check` và mở
   cửa sổ đã pass; các kiểm tra thủ công còn mở vì mock không có control
   nhập/file action, UI state capture sau Space bị lỗi. Xem runbook.*
4. ⏳ Bắt đầu vertical slice playback/tap-sync sau khi GPUI có go/no-go. —
   *Jobs/media headless đã xong; app chưa có audio device/player thật hoặc
   timeline actions.*
5. ⏳ Port preview/export sau khi luồng UI save/reopen ổn định. — *Domain/
   storage round-trip fixture pass; phần headless render đã có helper ASS,
   FFmpeg command và `verify_output`; full `build_ass`/layout còn thiếu.*
6. Chỉ sau khi MVP đạt mới đưa AI bridge và batch render vào Rust.

## 10. Ước lượng và mốc quyết định

Ước lượng chỉ dùng để lập kế hoạch, cần cập nhật sau spike:

- Baseline/fixture + spike: 1–3 tuần.
- Domain/storage/migration: 2–4 tuần.
- Vertical slice MVP: 4–7 tuần.
- Preview/export parity: 3–6 tuần.
- AI bridge/batch/parity nâng cao: 4–8 tuần.
- Đóng gói, rollout và hardening: 2–4 tuần.

Tổng tham khảo: **16–32 tuần cho một kỹ sư đã quen Rust/media**, thay đổi theo kết quả GPUI và mức parity title/scene/duet. Mỗi mốc phải giao một bản chạy được; không đợi port xong toàn bộ mới kiểm chứng.

## 11. Điều kiện nghiệm thu tổng thể

- Project schema 1/2 mở được; migration và restore không ghi đè bản gốc.
- Không mất timing, media mapping, preset hoặc trường chưa hiểu trong round-trip.
- Playback/timeline responsive; job render/AI có progress, cancel, timeout và recovery.
- Preview và file xuất khớp fixture theo ngưỡng đã ghi; audio xuất dùng đúng beat gốc.
- Archive/bundle không vượt giới hạn, không cho path traversal và kiểm tra fingerprint.
- Installer/portable chạy trên Windows sạch; model/cache/data sống qua cập nhật và rollback.
- App Python vẫn là fallback cho đến khi checklist P0/P1 đạt trên máy mục tiêu.
