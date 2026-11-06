# Kế hoạch còn lại — làm trên Windows

> **Mục đích:** tài liệu này là danh sách việc còn lại của kế hoạch chuyển
> sang Rust + GPUI, viết để làm **trên máy Windows 10/11**. Mọi phần làm
> được trên Linux đã xong và đã push; đừng làm lại, chỉ kiểm chứng lại
> trên Windows rồi làm tiếp từ đây.
>
> **Nhánh:** `codex/rust-gpui-rewrite`
> **Điểm xuất phát:** commit `df1a652` — "Rust rewrite: headless
> jobs/media/render/ai-bridge + Windows spike runbook"
> **Kế hoạch gốc:** `RUST_REWRITE_PLAN.md` (đọc cùng tài liệu này;
> tài liệu này không thay thế nó, chỉ liệt kê phần còn lại theo thứ tự
> làm trên Windows).
> **Ma trận tương thích:** `docs/compatibility-matrix.md`
> **Runbook spike:** `docs/windows-spike.md` +
> `scripts/windows-spike-check.ps1`

## 0. Điểm xuất phát — đã xong trên Linux, đừng làm lại

Kiểm chứng trên Linux 2026-11-05: `cargo test` 6 crate **62/62 pass**,
`cargo fmt --check` và `cargo clippy -- -D warnings` sạch; Python
`tests/test_timing.py` 4/4, JS tapping/timeline 14/14.

| Đã xong | Ghi chú |
|---|---|
| Phase 0 — fixture + ma trận tương thích | Fixture project v1/v2, `docs/compatibility-matrix.md`, Rust ghim **1.99.1** |
| Phase 2 (đầu) — `vietkara-domain` | ID 32 hex, migration v1→v2 đúng Python, save atomic, typed word fields |
| Phase 2 (đầu) — `vietkara-storage` | Manifest SHA-256, version snapshot, restore an toàn (verify → stage → swap), bundle ZIP |
| Phase 3–5 (phần headless) — `vietkara-jobs` | Lifecycle/cancel/persist/recover, batch queue |
| Phase 3–5 (phần headless) — `vietkara-media` | FFprobe + fallback `ffmpeg -i`, WAV, waveform, metrics, fake clock (`NullPlayer`) |
| Phase 4 (phần headless) — `vietkara-render` | Helper ASS, render profile, builder lệnh FFmpeg, `verify_output` |
| Phase 5 (phần headless) — `vietkara-ai-bridge` | Protocol JSON Lines v1, spawn/timeout/cancel/crash recovery (test bằng fake worker) |

**Chưa xong — tất cả ở dưới đây, đều cần Windows (trừ khi ghi rõ):**

- Spike GPUI/Windows/IME/playback (Giai đoạn 1) — `cargo check` và mở cửa sổ
  app đã pass trên Windows; kiểm tra IME/DPI/Unicode/playback thủ công còn mở.
- Phát audio bằng thiết bị thật, waveform/playhead thật, tap-sync thật.
- Full ASS layout (`build_ass` / `display_plan`) — cần đo font trên Windows.
- Entry point JSONL `ai_worker.py` đã bọc runtime Python hiện có; smoke test
  worker thật với model và cancel/timeout end-to-end còn mở.
- UI thật trong `vietkara-app` (hiện chỉ là mock: cờ `playing` không bao giờ đổi, chưa có action mở file/playback/timeline/save/export).
- Undo/redo, autosave/recovery gắn vào UI, preset/scene, updater, installer/portable, rollout.

**Nguyên tắc khi làm trên Windows (từ kế hoạch gốc §3, §8):**

1. App Python vẫn là production baseline và là oracle tương thích cho đến
   khi từng lát Rust đạt parity, có fixture và có đường rollback.
2. Không port lỗi toàn vẹn dữ liệu đã biết của Python (ví dụ: restore xóa
   media trước khi chép, import bundle giải nén trước khi kiểm tra) — Rust
   đã sửa hai lỗi này, giữ nguyên cách sửa.
3. Beat gốc là audio xuất video; vocal tách riêng chỉ phục vụ nhận dạng/căn lời.
4. Mọi thao tác thay media/restore/export phải có transaction hoặc bản sao
   an toàn; không xóa nguồn gốc trước khi kiểm tra file đích và fingerprint.
5. Không commit dữ liệu cá nhân, project thật, bài hát, video, model,
   `data/`, `windows-spike-results.log`, file `.log`, file tạm.

## 1. Chuẩn bị máy Windows

### 1.1. Máy và phần mềm

- [ ] Windows 10/11 x64, màn hình đổi được DPI/scale (100% / 125% / 150%).
- [ ] Visual Studio Build Tools — workload **Desktop development with C++**
  (MSVC, cần cho Rust target `x86_64-pc-windows-msvc`).
- [ ] Git, PowerShell 5.1 trở lên.
- [ ] Rust **1.99.1** — `rust-toolchain.toml` đã ghim; kiểm tra:
  `rustc --version` và `cargo --version` đều phải là `1.99.1`.
  Nếu rustup tự cài toolchain khác, chạy `rustup toolchain install 1.99.1`
  rồi kiểm tra lại trong thư mục repo (toolchain file sẽ chọn đúng).
- [ ] FFmpeg/FFprobe — cho probe/render; đặt trong `PATH` hoặc `bin/`
  theo `engine.binary()` (xem `engine.py`).
- [ ] Python 3.12 x64 + FFmpeg/FFprobe — chỉ để chạy app Python baseline
  và lấy golden output (xem §5). Tạo venv theo `README.md`:
  `.venv/Scripts/python.exe -m pytest tests -q`.
- [ ] Bộ gõ tiếng Việt (Unikey Telex hoặc bàn phím Vietnamese của Windows),
  font hỗ trợ Unicode; chuỗi test: `Xin chào`, `Tạm biệt`, `Điệp khúc`,
  `Bài hát tiếng Việt`.
- [ ] Node.js — để chạy `node --test tests/tapping.test.js
  tests/timeline_edit.test.js`.

### 1.2. Lấy code và kiểm chứng lại baseline

```powershell
git clone https://github.com/NDCLI/VietKaraStudio.git
cd VietKaraStudio
git checkout codex/rust-gpui-rewrite
git log --oneline -1   # baseline spike: eb0df8d (hoặc commit mới hơn)
rustc --version        # kỳ vọng: 1.99.1
cargo --version        # kỳ vọng: 1.99.1
powershell -ExecutionPolicy Bypass -File scripts\windows-spike-check.ps1
```

Script trên chạy: `cargo fmt --check`, test/clippy 6 crate headless,
`cargo check -p vietkara-app`, sau đó test/clippy toàn workspace gồm GPUI.
Kỳ vọng tối thiểu: 6 crate vẫn **62/62** trên Windows như trên Linux.
Nếu crate nào fail trên Windows, **dừng lại, ghi lỗi vào
`docs/windows-spike.md` §5** — đó là phát hiện quan trọng hơn cả GPUI.

### 1.3. Dữ liệu test — chỉ dùng fixture, không dùng dữ liệu cá nhân

- [ ] Dùng `crates/vietkara-domain/tests/fixtures/project-v1.json` và
  `project-v2.json` cho mọi test load/save/migration.
- [ ] Sinh file audio/video test bằng FFmpeg (không chép nhạc thật vào repo):
  ví dụ sine WAV 2–5 giây và video màu/testsrc ngắn cho probe/render.
- [ ] Model AI (Whisper/Demucs) tải riêng theo `README.md` / `model_manager.py`,
  không commit model vào repo.

## 2. Giai đoạn 1 — Spike GPUI, Windows và playback (làm trước tiên)

**Tài liệu chính:** `docs/windows-spike.md` — làm đúng theo đó, không
viết lại ở đây. Tóm tắt các bước và điểm quyết định:

1. [x] Chạy `scripts/windows-spike-check.ps1` trên PowerShell 5.1; 6 crate
   62/62, `fmt`, `clippy`, `cargo check -p vietkara-app`, workspace tests
   và workspace clippy pass. Log nằm ở
   `windows-spike-results.log` (gitignore, không commit).
   Lần sau vẫn cần chạy lại khi có thay đổi code.
2. [ ] `cargo run -p vietkara-app`: cửa sổ mở, resize, DPI 100% → 125% →
   150%, file picker / drag-drop, IME tiếng Việt, phím Space/phím tắt,
   font Unicode/VNI/TCVN3, đường dẫn Unicode (thư mục `Bài hát tiếng Việt`).
3. [x] Fake playback clock: hoãn tới vertical slice playback; fake clock
   đã có unit tests ở `vietkara-media`, nhưng chưa có UI hook để đo
   responsiveness. Không thêm thread giả trong spike.
4. [ ] Điền quyết định vào `docs/windows-spike.md` §5:
   **GPUI 0.2.2: tiếp tục / dừng**, phiên bản đã khóa (Rust 1.99.1,
   GPUI 0.2.2, commit spike), giới hạn ghi nhận được.
5. [ ] Commit kết quả spike (chỉ file tài liệu + code spike nếu có):
   cập nhật `docs/windows-spike.md` §5 và `docs/compatibility-matrix.md`,
   push lên nhánh `codex/rust-gpui-rewrite`.

**Kết quả Windows 2026-11-05:** bước tự động đã pass trên PowerShell 5.1
(6 crate 62/62, fmt, clippy, `cargo check -p vietkara-app`,
`cargo test --workspace`, workspace clippy); Python baseline 167/167 và
Node tapping/timeline 14/14 cũng pass. Python suite cần `resources/source`,
được nối tạm từ assets local có sẵn và đã gỡ junction sau test. App mở được
cửa sổ. Bước thủ công chưa đủ để quyết định go/no-go: xem kết quả chi tiết
trong `docs/windows-spike.md` §4–5. GO tiếp tục W3 theo quyết định và giới hạn ở windows-spike.md §5. Các spike riêng đã xác nhận mở/hủy dialog
native, dán Unicode, Ctrl+A và Telex cơ bản (`as` → `á`, Space commit,
Backspace trước/sau commit). Chưa đủ chuỗi Telex, DPI/resize và chọn
đường dẫn qua dialog; xem bảng bằng chứng hiện tại trong runbook.

**Điểm dừng (kế hoạch gốc §4.1):** nếu GPUI không đáp ứng IME, file
dialog hoặc render ổn định, **dừng ở đây** — giữ `vietkara-domain` /
`vietkara-storage` / các crate headless, thay UI shell trước khi port
editor. Không port thêm tính năng vào `vietkara-app` cho tới khi có
quyết định mới, và ghi rõ quyết định đó vào `RUST_REWRITE_PLAN.md`.

## 3. Giai đoạn 2 — Hoàn thiện domain, storage và migration

Phần lớn đã xong trên Linux. Còn lại, làm trên Windows theo thứ tự:

Bắt đầu W3: `vietkara-domain::edit` có Editor sở hữu project, command sửa
word theo ID, anchors/settings/scenes, inverse commands và event thành công.
Lệnh lỗi không publish candidate hoặc thay history. Đã thêm InsertWord/RemoveWord và Batch (một undo step, từng lệnh con phải
giữ project hợp lệ). Batch lỗi ở lệnh sau không publish thay đổi trước đó.
Domain command tests và
Clippy pass, gồm edit→undo→redo→save→reopen và lỗi giữ redo. Chưa nối UI,
chưa autosave; bước dưới vẫn chưa hoàn tất.

1. [ ] **Undo/redo — command/event model** (kế hoạch §4.2): thiết kế
   command cho sửa word/anchor/settings/scene thay vì để UI sửa trực
   tiếp struct; mỗi command có undo/redo, và autosave không ghi đè bản
   chính khi command lỗi. Test: sửa → undo → redo → save → reopen,
   nội dung phải khớp.
Tiến độ settings: đã có `settings::Settings` cho font/font_size, aspect,
resolution, render quality/format/fps, audio output/normalize và timeline
linked; enum theo baseline, default và flatten giữ các field còn lại.
Boundary `from_value` chuẩn hóa font và font_size, test domain 14/14 và
Clippy pass. `Project.settings` và SetSettings command đã dùng typed struct;
renderer ánh xạ enum trực tiếp. Project deserialize chuẩn hóa core settings,
validate từ chối font_size không hữu hạn/ngoài giới hạn. Chưa nối UI;
chưa tuyên bố parity toàn bộ `styles.validate_settings`.

2. [ ] **Settings đã typed:** chuyển các field settings hay dùng trong
   `styles.validate_settings` / `DEFAULT_SETTINGS` (font, font_size,
   aspect, resolution, render_quality/format/fps, audio_output, …)
   từ `serde_json::Value` sang typed struct có default, nhưng vẫn giữ
   unknown settings fields qua `flatten` (như project/word).
Tiến độ autosave: `vietkara-storage::autosave` ghi file recovery riêng bằng
atomic domain save, kiểm tra project identity, chỉ đề xuất snapshot có
mtime mới hơn bản chính. Candidate giữ snapshot đã đọc; accept trả project
trong RAM, không ghi bản chính. Storage 8/8 tests và Clippy pass, gồm lỗi
giữ bản chính/recovery cũ. Chưa có timer, prompt UI hoặc crash/restart test.

3. [ ] **Autosave + recovery gắn vào app:** autosave ghi file tạm +
   rename (dùng `vietkara-storage`), recovery khi mở lại: nếu có bản
   autosave mới hơn bản chính, hỏi người dùng trước khi khôi phục, không
   tự ghi đè. Test: kill app giữa chừng → mở lại → không mất project.
4. [ ] **Kiểm chứng lại trên Windows:** `cargo test -p vietkara-domain
   -p vietkara-storage`, load/save fixture v1/v2 trong thư mục tên
   tiếng Việt có dấu + khoảng trắng, round-trip giữ nguyên unknown
   field `future_python_field` và ID 32 hex.
5. [ ] Cập nhật `docs/compatibility-matrix.md` nếu phát hiện khác biệt
   Windows (đặc biệt: đường dẫn Unicode, hard link snapshot trên NTFS).

**Hoàn tất khi:** domain/storage chạy không cần GUI; load/save/migrate/
restore round-trip qua fixture schema 1/2; bản gốc và media không bị
sửa khi migration/import/restore lỗi.

## 4. Giai đoạn 3 — Vertical slice MVP: project → playback → tap-sync → save

**Mục tiêu:** một luồng Rust dùng được hằng ngày, vẫn quay về app Python
được khi gặp lỗi. Làm theo thứ tự dưới đây, mỗi bước xong mới sang bước sau.

### 4.1. `vietkara-app`: create/open/relink/save/close/reopen

- [ ] Gắn action thật vào UI mock hiện tại (`crates/vietkara-app/src/main.rs`):
  mở file (file picker GPUI), tạo project mới, save/close/reopen.
- [x] Relink audio project bị thiếu: hiển thị trạng thái thiếu file và nút
  chọn lại beat/bản hát; chọn đúng nội dung cũ được nhận diện bằng fingerprint
  để giữ anchors/lời như `vietkara-storage::audio_import` đã hỗ trợ.
- [ ] Dùng `vietkara-domain` + `vietkara-storage` cho load/save, không
  viết lại logic save trong app.
- [ ] Test: tạo project → lưu → đóng → mở lại → nội dung khớp fixture v2.

### 4.2. Playback thật bằng thiết bị Windows

- [ ] Thêm player thiết bị thật vào `vietkara-media` (trait `Player` đã
  có, `NullPlayer` chỉ là fake clock): chọn thiết bị audio Windows,
  play/pause/seek, clock là nguồn thời gian chuẩn (plan §3.5), UI/timeline
  chỉ nhận snapshot clock, không tự chạy clock theo frame render.
- [ ] Xử lý state thiết bị/lỗi: không có thiết bị, thiết bị bận, file
  media lỗi — hiển thị lỗi hành động được, không treo UI.
- [ ] Test: phát beat 5 giây, seek, pause/resume, playhead theo đúng
  clock; probe/render/worker chạy ngoài UI thread, kéo/scroll cửa sổ
  không đơ.

### 4.3. Import SRT/lyrics, tap-sync, chỉnh timeline

- [ ] Import SRT/lyrics (dùng `timing.estimate_words` / `regroup_words`
  của Python làm oracle): chọn câu/từ, Space tap-sync, chỉnh start/end,
  liên kết/độc lập mốc (xem `tests/tapping.test.js` và
  `tests/timeline_edit.test.js` để biết đúng hành vi).
- [ ] Timeline hai hàng: zoom/scroll, playhead theo playback clock,
  cảnh báo media thiếu, kéo một mốc cập nhật timestamp.
- [ ] Undo/redo (§3.1) + autosave + version snapshot trước thao tác
  phá hủy (dùng `vietkara-storage::create_version`).

### 4.4. Bài test nghiệm thu của Giai đoạn 3

Chạy bằng tay, ghi lại kết quả (pass/fail + ghi chú) vào
`docs/compatibility-matrix.md`:

> Mở nhạc → nhập lời → căn bằng Space → sửa mốc → lưu → đóng → mở lại
> — nội dung khớp, job hủy không để lại project hỏng, UI vẫn phản hồi.

### W4: trạng thái và relink audio bị thiếu

Studio đối chiếu metadata audio với file an toàn cạnh project, hiện banner
relink nếu file beat/bản hát bị mất; tên file có separator/path traversal bị
coi là không hợp lệ. Media panel không còn báo "đã nạp" chỉ dựa trên metadata.
Khi file gốc thiếu, banner xác nhận giải thích fingerprint sẽ giữ mốc/lời nếu
người dùng chọn đúng nguồn cũ. Playback cũng mở file theo metadata của project.
Đã mở app Windows trên fixture thiếu cả hai file: banner hai nút relink và
nhãn trong media panel hiện đúng; project fixture không bị chỉnh sửa. Runtime
cũng lộ flex làm co cột lời/đẩy inspector khỏi cửa sổ; đã khóa co hai panel và
cho vùng giữa co lại, ảnh kiểm tra sau sửa cho thấy đủ ba cột. Unit test
trạng thái/file path pass; luồng convert, fingerprint và relink thật vẫn cần
kiểm tra runtime có FFmpeg trên Windows.

## 5. Giai đoạn 4 — Preview/export parity

### 5.1. Golden output từ baseline Python (làm trước khi port layout)

- [ ] Trên Windows, chạy app Python baseline với fixture project v2,
  lưu đầu ra tham chiếu vào thư mục tạm (không commit file lớn):
  normalized project JSON, file ASS, lệnh FFmpeg (argv), probe metadata,
  render info, vài frame đại diện.
- [ ] Ghi rõ ngưỡng sai số cho phép: thời gian, kích thước frame,
  màu/font/layout, audio stream (kế hoạch §4.0).

### 5.2. Full ASS layout — cần đo font trên Windows

- [ ] Port `layout.display_plan` sang Rust, dùng font đo được trên
  Windows (font hệ thống + font bundled trong `bundled_assets.py`):
  `frame_size`, `split_tokens`/`split_text`, `regroup_words`,
  vị trí từng từ, role icon, countdown, title.
- [ ] Port `subtitles.build_ass` đầy đủ (hiện `vietkara-render` mới có
  helper + builder lệnh FFmpeg cho trường hợp nền màu đơn giản):
  karaoke sweep, gradient, duet colors/icons, title/logo/background/
  countdown, subtitle/lyricsmv mode, shadow/outline.
- [ ] Quy tắc: preview và export dùng **chung một** layout model, không
  có hai thuật toán positioning độc lập.
- [ ] Test golden: ASS sinh ra khớp file ASS tham chiếu §5.1 theo ngưỡng
  đã ghi; frame comparison và audio-stream assertion trên fixture chuẩn.

### 5.3. Render thật và kiểm chứng

- [ ] Mở rộng `vietkara-render`: scene/logo/countdown overlays,
  video/image background, guide audio (`amix`), audio normalize
  (`loudnorm`), NVENC fallback sang libx264 khi lỗi (như `engine.render`).
- [ ] Xuất MP4 tối thiểu đạt parity fixture, audio dùng đúng beat gốc;
  `verify_output` + sample frames (`render_info.verify_output` /
  `sample_frames`) phải pass.
- [ ] Lỗi FFmpeg hiển thị được nguyên nhân có thể hành động (không chỉ
  mã lỗi).

**Hoàn tất khi:** preview/export không lệch các thuộc tính đã cam kết,
progress/cancel hoạt động, export folder + render info đúng.

## 6. Giai đoạn 5 — AI bridge, audio sync và batch jobs

### 6.1. Entry point worker JSONL mới (làm trước tiên trong giai đoạn này)

Hiện trạng: `vietkara-ai-bridge` đã xong protocol. `worker.py` vẫn giữ
contract file request cho app cũ; entry point mới `ai_worker.py` nhận
protocol JSONL và gọi lại runtime Python hiện tại. Nó chạy qua
`desktop_entry.py --vcl-task speech <request-file.json>`, in
`OBS_PROGRESS {...}` ra stdout và ghi kết quả ra file
(`app_runtime.py::dispatch`, `worker.py::run`/`write_result`).

- [x] Tạo entry point mới `ai_worker.py`:
  đọc **một dòng JSON** từ stdin (`protocol_version: 1`,
  `request_id`, `kind`, `project_id`, `input`, `options`),
  in các dòng JSONL `progress` / `result` / `error` ra stdout,
  log ra **stderr only** (stdout chỉ dành cho protocol).
- [x] Các `kind`: `separate`, `recognize`, `align`, `align_region`,
  `role_suggest` — khớp `AiKind` trong `vietkara-ai-bridge`.
  Media truyền bằng path + metadata, không nhúng bytes.
- [ ] Test end-to-end qua `vietkara-ai-bridge::run_request` với worker
  thật (model nhỏ/tải riêng): progress hiển thị, cancel dừng worker,
  worker crash không treo app, timeout có lỗi rõ ràng.
- [x] Không thay model/runtime ngay (Demucs/Whisper vẫn chạy bằng Python).

### 6.2. Audio sync, cache và batch render

- [ ] Port/reuse audio sync (`audio_sync.py`) và cache
  (`analysis_cache.py`) với fingerprint nguồn + model + language +
  options — cache không được trả nhầm kết quả khác fingerprint.
- [ ] Port flow: separation, recognition/alignment, align region, role
  suggestion; project không bị ghi đè trước khi người dùng xác nhận
  kết quả AI.
- [ ] Batch render queue (dùng `vietkara-jobs::BatchRecord` đã có):
  record bền vững, resume sau restart (`first_pending`), per-item error,
  cancel; UI hiển thị tiến độ từng item.
- [ ] Giới hạn tài nguyên, CPU-only/NVIDIA, model download riêng,
  không xóa cache khi update.

**Hoàn tất khi:** worker lỗi/hủy không treo app; batch resume đúng item;
cache không trả nhầm; project không bị ghi đè trước khi xác nhận AI.

## 7. Giai đoạn 6 — Parity nâng cao, đóng gói và chuyển người dùng

- [ ] Preset import/export, scenes/assets đầy đủ, fonts, updater
  (`updater.py`, `docs/DISTRIBUTION.md`), health diagnostics
  (chỉ lưu cục bộ, không thu media/project).
- [ ] Đóng gói Windows: portable/installer (`packaging/`,
  `VietKaraStudio-Nuitka.iss` / `VietKaraStudio.iss` làm tham chiếu),
  đóng gói FFmpeg + Python runtime theo thành phần; model tải riêng.
- [ ] Test trên máy sạch Windows 10/11 x64, đường dẫn Unicode, CPU-only
  và NVIDIA; kiểm tra upgrade từ bản cũ (1.1.1 → 1.1.7 → bản Rust).
- [ ] Smoke test sau cài đặt, crash recovery, file association,
  update/rollback, giữ nguyên `data/`, model/cache.
- [ ] Chạy song song app cũ/app mới theo opt-in; chỉ đổi mặc định sau
  khi checklist parity P0/P1 đạt và có bản rollback.
  (P0/P1/P2 xem `RUST_REWRITE_PLAN.md` §5.)

**Hoàn tất khi:** installer/portable chạy trên máy sạch; mở project cũ
không phá hủy; update không xóa data; bản cũ vẫn mở được nếu rollback.

## 8. Thứ tự làm việc đề xuất (tóm tắt)

| # | Việc | Phụ thuộc | Ước lượng* |
|---|---|---|---|
| W1 | Chuẩn bị máy + kiểm chứng lại 62 test (§1) | — | 0,5 ngày |
| W2 | Spike GPUI + quyết định go/no-go (§2) | W1 | 1–3 ngày |
| W3 | Undo/redo, settings typed, autosave (§3) | W2 (go) | 1–2 tuần |
| W4 | Vertical slice MVP: open/save/playback/tap-sync/timeline (§4) | W2, W3 | 4–7 tuần |
| W5 | Golden ASS/FFmpeg + full layout + render parity (§5) | W4 (save/reopen ổn định) | 3–6 tuần |
| W6 | Worker JSONL + AI flows + audio sync + batch (§6) | W4, W5 (render ổn định) | 4–8 tuần |
| W7 | Preset/scene/updater/installer/rollout (§7) | W5, W6 | 2–4 tuần |

\* Ước lượng gốc trong `RUST_REWRITE_PLAN.md` §10 cho một kỹ sư đã quen
Rust/media; cập nhật lại sau spike W2. Tổng tham khảo toàn bộ kế hoạch:
**16–32 tuần**. Mỗi mốc phải giao một bản chạy được; không đợi port xong
toàn bộ mới kiểm chứng.

**Không làm song song một cách tùy tiện:** W4 cần W2 go; W5 cần W4;
W6 cần W4/W5; W7 cần W5/W6. Trong một giai đoạn, có thể tách việc
headless (đã làm xong phần lớn trên Linux) và việc UI/thiết bị.

## 9. Lệnh kiểm chứng trên Windows (PowerShell, từ thư mục repo)

```powershell
# Sau mỗi giai đoạn, chạy đủ các lệnh này trước khi commit:
cargo fmt --check
cargo test -p vietkara-domain -p vietkara-storage -p vietkara-jobs -p vietkara-media -p vietkara-render -p vietkara-ai-bridge
cargo clippy -p vietkara-domain -p vietkara-storage -p vietkara-jobs -p vietkara-media -p vietkara-render -p vietkara-ai-bridge --all-targets -- -D warnings
# Script spike chạy thêm hai lệnh dưới sau khi app có thể build:
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
# Baseline Python + JS (venv theo README.md):
.venv\Scripts\python.exe -m pytest tests -q
node --test tests/tapping.test.js tests/timeline_edit.test.js
```

`cargo test --workspace` và workspace clippy đã pass trên Windows 11 Pro
2026-11-05. Nếu một lần chạy sau fail vì `vietkara-app`/GPUI trên Windows,
ghi rõ lỗi vào `docs/windows-spike.md` §5 và tiếp tục chạy theo `-p` như
trên — đừng bỏ qua test của các crate khác.

## 10. Cách ghi lại kết quả và push

1. [ ] Sau mỗi giai đoạn, cập nhật tiến độ vào `RUST_REWRITE_PLAN.md`
   (mục "Tiến độ" ở đầu file) và `docs/compatibility-matrix.md`
   (thêm hàng mới vào bảng hợp đồng / crate, ghi rõ khác biệt Windows
   nếu có).
2. [ ] Commit trên nhánh `codex/rust-gpui-rewrite`, **không commit vào
   `main`**, không merge nhánh khác vào khi chưa có quyết định.
3. [ ] Thông điệp commit rõ ràng, ví dụ:
   `Rust rewrite Phase 1: GPUI spike on Windows — go, GPUI 0.2.2`.
4. [ ] Push và kiểm tra lại trên GitHub:
   ```powershell
   git status --short --branch
   git push origin codex/rust-gpui-rewrite
   git log --oneline -3
   ```
5. [ ] Nếu một bước fail và chưa sửa được, commit riêng phần đã xong
   (kèm ghi chú "chưa xong: …") rồi mới tiếp tục — đừng để work chưa
   commit nằm lâu trên máy Windows.
6. [ ] Không commit: `data/`, model, project/bài hát/video thật,
   `windows-spike-results.log`, `*.log`, file tạm, file build
   (`target/` đã gitignore).

## 11. Rủi ro và điểm dừng (tóm tắt từ kế hoạch gốc §8)

- GPUI thay đổi nhanh / không đáp ứng IME/file dialog → điểm dừng §2.
- Playback/device/seek khó → clock/player trait đã có trong
  `vietkara-media`; test bằng fake clock trước, thiết bị thật sau.
- Preview lệch export → một layout model dùng chung (§5.2); golden
  frame/font test trước khi thêm hiệu ứng.
- Schema Python linh hoạt → typed known fields + unknown map; migration
  không phá hủy; fixture cho field lạ.
- Archive/media độc hại hoặc quá lớn → allowlist, giới hạn size/count,
  fingerprint, ghi tạm an toàn (đã có trong `vietkara-storage`).
- Worker AI treo/crash → process isolation, timeout, cancel, request ID,
  stderr log, recovery state (đã có trong `vietkara-ai-bridge`).
- Scope creep do parity → bám P0/P1/P2 (`RUST_REWRITE_PLAN.md` §5);
  không port feature trước khi MVP path ổn.
- Hai app cùng sửa project → khóa/phiên bản file hoặc cảnh báo conflict;
  chỉ một app làm writer trong rollout.

## Phụ lục — Tài liệu tham chiếu khi làm trên Windows

- `RUST_REWRITE_PLAN.md` — kế hoạch gốc đầy đủ (mục tiêu, kiến trúc,
  từng giai đoạn, tiêu chí chất lượng, nghiệm thu tổng thể §11).
- `docs/compatibility-matrix.md` — hợp đồng tương thích Python ↔ Rust.
- `docs/windows-spike.md` + `scripts/windows-spike-check.ps1` — spike
  Giai đoạn 1.
- `docs/DISTRIBUTION.md`, `docs/RELEASE.md` — đóng gói/phát hành baseline.
- Code Python làm oracle: `app.py`, `timing.py`, `layout.py`,
  `subtitles.py`, `engine.py`, `media_tools.py`, `project_archive.py`,
  `batch_render.py`, `worker.py`, `app_runtime.py`, `render_info.py`,
  `styles.py`, `audio_sync.py`, `duet_roles.py`.
- Code Rust đã xong: `crates/vietkara-domain`, `crates/vietkara-storage`,
  `crates/vietkara-jobs`, `crates/vietkara-media`,
  `crates/vietkara-render`, `crates/vietkara-ai-bridge`
  (mỗi crate có doc comment ở đầu `src/lib.rs` ghi rõ đã port gì,
  cố tình khác Python ở đâu, và chưa làm gì).

### W3: nối command vào studio

Studio hiện sở hữu Editor, chỉ đọc project qua accessor; cỡ chữ +/-
phát SetSettings command, có nút undo/redo và trạng thái thành công/lỗi.
Giới hạn 20..240, bấm ở giới hạn không thêm history. App check và Clippy
all-targets pass. Chưa xác minh thao tác nút bằng runtime ở lượt này;
chưa có preview render theo setting, keyboard shortcut, open/save thật
hoặc timer/prompt autosave trong studio. Các mục W3 vẫn đang làm.

### W3: mở đường dẫn và recovery trong studio

`cargo run -p vietkara-app -- <project.json>` mở project và kiểm tra
candidate mới hơn. Banner yêu cầu chọn Khôi phục hoặc Dùng bản chính;
chặn edit/save khi chưa chọn. Recovery chấp nhận chỉ đổi Editor trong RAM.
Nút Lưu project ghi atomic trên background executor; edit bị chặn trong
lúc lưu để trạng thái kết quả không bị gán nhầm cho thay đổi mới. Chưa
có Save As cho project mới, dialog Open, timer autosave hoặc kiểm tra
crash/restart. App check/Clippy all-targets pass; runtime UI chưa kiểm tra.

### W3: timer autosave trong studio

Timer 30 giây được giữ bằng Task thuộc Studio (đóng entity thì hủy task),
ghi snapshot bằng background executor khi command/undo/redo thành công.
Không ghi khi chưa có path, đang save chính hoặc đang chờ recovery choice.
Manual save bị chặn trong lúc autosave để tránh hai writer cạnh tranh.
Edit khi autosave chạy vẫn đặt pending cho lượt sau; lỗi autosave đặt lại
pending để retry và hiện lỗi, không báo saved nhầm. Save chính thành công
xóa pending. App check/Clippy all-targets pass; chưa có bằng chứng timer
thực sự ghi sau 30 giây hoặc kill/restart recovery trong runtime UI.

### Runtime studio trên bản sao fixture

Build app thành công; launch với `target/runtime-recovery/project.json`
(copy fixture v2) mở cửa sổ, hiện tên project, 4 từ và cỡ chữ 110.
Chưa có command thành công được xác nhận sau thao tác click cỡ chữ qua
UI helper; không coi đây là bằng chứng timer đã ghi. Bản runtime vẫn
được giữ để kiểm tra tiếp; chưa chạy kill/restart.

Runtime đối chiếu bổ sung: UI studio hiện cỡ chữ 100 và `Đã lưu project`,
khớp JSON file thử font_size 100, field lạ còn nguyên. Chưa xác định người
thực hiện các thao tác lưu/chỉnh trước đó. Khi thử tạo thêm thay đổi,
helper báo `user input was detected in this window`; dừng điều khiển để
không tranh với người dùng. Chưa có snapshot autosave được quan sát.

### W3: keyboard actions

Studio có FocusHandle/key context riêng, gán focus lúc mở. Ctrl+S gọi
luồng save hiện có; Ctrl+Z undo; Ctrl+Y/Ctrl+Shift+Z redo. Các action dùng
cùng guard recovery/save và cùng Editor với nút bấm, không sửa state
ngoài command. App check và Clippy all-targets pass. Chưa build/chạy lại
UI để xác minh shortcut; executable đang chạy từ lượt trước chưa có chúng.

### Storage recovery sau khi kill tiến trình

Integration test `autosave_crash` spawn một writer child, chờ marker sau
atomic autosave, kill child và đợi kết thúc. Recovery đọc lại đúng ID,
tên chỉnh sửa và field settings lạ; bytes bản chính giữ nguyên. Test
chạy pass trên Windows, Clippy storage all-targets pass. ChildGuard bảo
đảm kill/wait child nếu test lỗi. Đây là crash của writer storage sau khi
ghi xong snapshot, chưa phải crash GPUI trong lúc ghi hoặc kiểm chứng
timer30s/prompt UI. Các phép thử runtime app đó vẫn còn mở.

Rà soát phát hiện key context/action trước đó gắn nhầm vào từng lyric row.
Đã chuyển tất cả lên root Studio và refocus root khi click, để project
rỗng vẫn có nơi nhận phím và tránh lặp focus handle theo số từ. App check
và Clippy all-targets pass sau sửa; vẫn cần runtime shortcut trên project
rỗng/có từ. Không dùng kết quả compile làm bằng chứng thao tác phím đạt.

Theo yêu cầu người dùng, từ lượt này gom kiểm thử vào mốc tính năng.
Đã viết Save As/native path picker và Ctrl+Shift+S; Ctrl+S cho project mới
mở picker. Path chỉ được cập nhật sau save thành công, hủy/lỗi giữ path
cũ. Thay đổi mới chưa build/test; còn cần kiểm tra media refs khi Save As
sang thư mục khác trước khi coi luồng Save As đầy đủ.

Luồng Open mới: Ctrl+O/nút Mở dùng dialog native, load/candidate/Editor
trên background executor; chỉ thay session sau khi tất cả thành công.
Dirty độc lập với autosave_pending, không mất cờ chưa lưu sau autosave.
Nếu dirty, banner cho bỏ thay đổi và mở hoặc hủy; có thể lưu bằng nút
Lưu/Ctrl+S trước rồi chọn Mở lại. Cancel/load lỗi giữ project/history/path.
Thay đổi này đang viết, chưa build/test theo yêu cầu gom kiểm thử.

Đã viết guard đóng cửa sổ: chặn trong lúc ghi file, hỏi đóng không lưu
hoặc tiếp tục khi dirty. Header có dấu * cho thay đổi chưa lưu, tách
trạng thái recovery với save chính. Autosave thành công cập nhật trạng
thái riêng và giữ dirty. Chưa build/test thay đổi mới (gom kiểm thử).

Save As bảo toàn layout media Python: copy managed media sang thư mục đích,
preflight từ chối media cùng tên khác nội dung, publish no-clobber (hard-link
hoặc staged `persist_noclobber`) và ghi JSON sau cùng. File đích cùng bytes
được chấp nhận khi retry; file khác bytes không bị thay. Nếu lưu JSON lỗi,
media mới có thể còn để retry, JSON cũ vẫn nguyên. Storage tests xác nhận
copy allowlist, bỏ file không quản lý, retry idempotent, conflict giữ nguyên
JSON/media và fallback publish chạy được; phần copy Save As vẫn cần thao tác
UI runtime để kiểm tra dialog/path thực tế.

Đang viết nhập SRT theo `static/app.js`: chia từ đều trong cue trên clock
beat, inverse-map qua anchors về reference, bỏ tag, normalize và validate
trước khi tạo SetLyrics command (một undo). Từ chối thay lời có khóa.
Chưa nối nút nhập vào UI, chưa build/test theo lịch gom kiểm thử.

Đã viết nút Nhập SRT/Ctrl+Shift+I: đọc UTF-8 có giới hạn 10 MiB, parse
ở background, chờ banner xác nhận rồi commit một command. Trong lúc
chờ, chặn chỉnh sửa/đổi project để candidate không áp vào state khác.
Hủy/error giữ data/history. Chưa build/test các thay đổi mới.

Đã bổ sung test parser SRT cho BOM/CRLF, cue nhiều dòng, timestamp dấu phẩy
và dấu chấm, HTML tag, chia mốc từng từ, inverse-map qua anchor, cue rỗng/
không map được và project có từ khóa. `cargo test -p vietkara-domain` pass
27 unit tests cùng 3 integration tests. Kiểm thử này xác nhận domain; thao
tác dialog/confirm vẫn dựa trên smoke test UI ghi ở phần runtime bên dưới.

Đang viết backend audio Windows DevicePlayer dùng Rodio 0.21.1 đã ghim
API (docs.rs versioned). Play/pause/stop/seek và position từ sink, không
dùng fake clock để giả playback thật. Có reload khi kết thúc file. Chưa
có worker adapter/UI hook và chưa build/test (gom kiểm thử cuối mốc).

Đã viết playback worker Windows: queue bounded16, load/unload/play/pause/
stop/seek, snapshot position/duration/error cập nhật 20ms. Device chỉ tồn
tại trong worker, drop controller đặt cancellation, không join/block UI.
Lỗi device/load được trả trong snapshot. Chưa nối transport UI; chưa
build/test phần mới theo lịch gom kiểm thử.

Đang nối transport thật: project-local beat.wav, controller mới khi đổi
project, poll snapshot50ms, play/pause/stop/seek ±5s. Ctrl+Space phát,
Escape dừng, Ctrl+Left/Right seek; vị trí lấy từ sink. Chưa import/relink
beat và chưa build/test code mới.

Đã viết shared map_time reference/beat cho SRT và timeline; click chọn
từ để seek, highlight từ theo sink clock, panel hiện text/start/end thật
và chỉnh đầu/cuối ±50ms qua ReplaceWord command, tôn trọng locked và
validation. Chưa có timeline hình học hai hàng/zoom/scroll/waveform;
code mới chưa build/test theo yêu cầu gom kiểm thử.

Đã viết TapTiming theo state machine JS: tap nhanh commit ở nhịp kế,
giữ ≥180ms commit khi thả, ngưỡng từ40ms, chống repeat, cancel/finish/back.
Wall time tách beat time. tap_command inverse-map sang reference và từ
chối locked/mốc âm. Chưa nối keydown/up vào UI và chưa build/test mới.

Đang nối tap-sync Ctrl+T và Space keydown/up. Staging words riêng để
commit một SetLyrics undo step khi hoàn tất; autosave chụp cả mốc đã tap
trong staging. Lock sửa lời/open/save trong session. Vị trí lấy trực tiếp
snapshot sink lúc event, không dùng tick UI. Còn cần xử lý backspace,
blur/end-of-audio và hoàn thiện nút điều khiển. Chưa build/test mới.

Bổ sung tap session: Backspace trả từ về mốc trước session và seek lùi1s;
activation observer pause/cancel hold khi mất focus. Pause/seek hủy hold
nhưng giữ session, Stop/Escape kết thúc các tap đã commit. Hết audio hoặc
hết từ tự hoàn tất. Close guard tính cả staging chưa lưu. Có nút bắt đầu,
từ trước, hoàn tất và bộ đếm. Chưa build/test code mới.

Đang viết audio import transaction: convert WAV staging, waveform và
SHA256, checkpoint trước replace, rollback WAV khi JSON save lỗi. Fingerprint
khớp giữ anchors/alignment; replace khác reset theo baseline. Import media
là checkpoint transaction, sẽ reset Editor history sau thành công thay vì
cho undo metadata mà không khôi phục WAV. Chưa nối UI/build/test mới.

Đã viết UI nhập beat/reference và Ctrl+B/Ctrl+R, chọn nguồn rồi banner
xác nhận checkpoint/ảnh hưởng mốc lời trước convert. Yêu cầu project đã
có path. Playback được nhả trước transaction và reset cả khi lỗi. Sau
thành công, session chuyển sang project đã save, history cũ nằm trong
version checkpoint. Chưa build/test mới (gom kiểm thử).

Đã viết resize clock beat theo timeline_edit.js: duration tối thiểu50ms,
chỉ nối hàng xóm chạm trong60ms và cùng line; inverse-map về reference.
Neighbor locked trả lỗi. Một Batch command cho resize và neighbor, có
toggle linked/independent trong UI. Đang chuẩn bị phần vẽ waveform/
timeline hình học; code mới chưa build/test.

Đã viết timeline hình học: waveform reference/beat từ metadata peaks,
axis thời gian, khối từ hai hàng theo line, playhead sink, zoom10..500px/s
và scroll ngang. Danh sách từ cuộn riêng để chọn. Khối từ trên timeline
hiện vẫn dựng toàn bộ; cần culling/virtualization khi project lớn.

Đã triển khai kéo đầu/cuối khối từ: preview riêng, commit Batch một lần khi
mouseup/up-out; delta tính zoom và scroll. Khóa các lệnh đổi state trong
gesture, zoom giữ nguyên trong gesture. Code đã qua workspace build/test.
Runtime Windows với fixture 3.000 từ: kéo mép phải từ 0.35s lên 0.55s,
Ctrl+Z trả về 0.35s; kéo mép trái từ 0s lên 0.10s, Ctrl+Z trả về 0s.
UI cập nhật panel thời gian/trạng thái dirty tương ứng; fixture được lưu lại
và đọc JSON xác nhận giá trị gốc 0.00–0.35s, không giữ thay đổi test.

Đã chuyển danh sách lời bên trái sang `gpui::uniform_list` của GPUI 0.2.2:
callback chỉ tạo row trong visible range, selection vẫn gọi cùng action và
highlight active word lấy từ playback snapshot của lần render. Nội dung mỗi
row được giữ một dòng để thỏa điều kiện chiều cao đồng nhất của UniformList.
`cargo check -p vietkara-app` pass. Runtime Windows dùng project fixture 3.000
từ: app mở project bằng dialog, hiện đúng tổng số từ, cuộn danh sách và chọn
`Từ16`; editor cập nhật nội dung, vai và khoảng thời gian tương ứng. Điều này
xác nhận scroll/selection của UniformList; chưa đo thời gian render hay xác
nhận culling riêng của các khối timeline.

Đã culling block lời và bin waveform theo vùng timeline đang thấy, cộng một
viewport overscan ở mỗi bên; vùng này tính từ scroll offset âm, độ rộng
scroll viewport và timeline zoom. GPUI scroll wheel notify view nên range
được tính lại ở lần render kế tiếp. Test helper xác nhận offset/overscan/
clamp; runtime với fixture trên mở timeline ở các block đầu, cuộn ngang rồi
thấy block quanh từ 123–146. Culling giảm số node được tạo/layout, nhưng
hiện vẫn quét toàn bộ words để tìm phần giao viewport; waveform bins được
quét tối đa 1.600 bin mỗi nguồn.

Đã thêm `TimelineWordIndex` cache các interval đã map theo anchor và prefix
maximum end. Trên project ổn định, binary search tìm candidate viewport rồi
chỉ xét các interval giao nhau; kết quả vẫn sắp theo thứ tự project để giữ
z-order/hit testing. Cache rebuild khi backing words hoặc anchors đổi; drag/
tap preview dùng staging scan để không đọc index cũ. Test cover word order
không theo thời gian, interval dài bắt đầu ngoài viewport và invalidation
mapping khi anchors đổi. Build Windows mới sau thay đổi cache được mở lại
với fixture 3.000 từ; cuộn ngang vẫn hiện đúng nhóm block khoảng 123–146.

### W3: kiểm thử gom mốc (sau commit 516dd2d)

Lượt gom kiểm thử đầu tiên cho code viết chưa build trước đó. `cargo check`
lỗi E0502 ở `Studio::render` (mượn `cx` khi `lyric_rows` còn sống): đã gọi
`timeline_view(cx)` trước khi tạo `lyric_rows`. Sửa thêm hai cảnh báo
clippy `collapsible_if` (let-chain) và chạy `cargo fmt`. Kết quả trên
Windows: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
warnings` sạch; `cargo test --workspace` pass toàn bộ (vietkara-media cần
`ffmpeg`/`ffprobe` trong PATH, nếu thiếu 3 test fixture sẽ fail). Chưa có
bằng chứng runtime cho transport/tap-sync/timeline drag/Save As/Open/import
beat; các mục đó vẫn mở.

### W3: runtime hạn chế sau commit de281db

Build debug và mở bản sao fixture v2 tại `target/runtime-w3/Bài hát W3/`
(đường dẫn Unicode, kèm `beat.wav` sine 5 giây sinh bằng FFmpeg). Chụp cửa
sổ xác nhận: app lên, tên project và nhãn tiếng Việt đúng dấu, cỡ chữ 110,
timeline hai hàng với bốn từ đúng thời điểm, bộ đếm `0.00 / 5.00 s`, stderr
rỗng, `project.json` không đổi byte. Click/phím giả lập qua `PostMessage`
không được GPUI nhận (nút Phát không đổi trạng thái), nên chưa kiểm chứng:
playback thật và playhead, Space tap-sync, kéo khối từ, undo/redo, Lưu,
Save As, Mở, nhập SRT/beat, autosave 30 giây. Các mục đó cần thao tác tay
hoặc công cụ UI automation; không coi bằng chứng hiển thị này là parity
tương tác.

### Tham khảo GPUI Kit

`https://gpui-kit.com/` (trang hiện giới thiệu gpui-kit 0.9.0) hữu ích làm
tham khảo về component controls có trạng thái rõ, semantic theme tokens,
focus/accessibility và virtualized list cho lyric/timeline dài. Chưa thêm
dependency: app đang ghim GPUI 0.2.2 và chưa xác minh tương thích API/version
giữa hai dự án. Giữ GPUI hiện tại; áp dụng các nguyên tắc interaction/theme
bằng component nội bộ, chỉ đánh giá thư viện sau khi có spike tương thích.

### W3: playback worker hardening

Snapshot `loading` được publish trước khi decode/mở device; các lệnh đến
trong giai đoạn chuẩn bị được giữ FIFO. Worker panic được đổi thành lỗi
snapshot và đánh dấu worker đã dừng để lệnh gửi sau báo lỗi ngay. Windows
target check, workspace clippy và test load-failure/unload pass. Playback
thiết bị thật/playhead đã smoke test trên máy này; seek và pause/resume vẫn
cần kiểm tra riêng (xem phần runtime bên dưới).

### W3: runtime Save As/Open/playback trên Windows

Mở app với fixture ở `target/runtime-w3/Bài hát W3/project.json` (đường dẫn
thư mục có dấu, project `Bài hát tiếng Việt`), xác nhận lời và timeline hiển
thị. Ctrl+Shift+S mở native Save As; lưu thành
`target/runtime-w3/saveas-vietnam/project-copy.json`, UI báo `Đã lưu project`,
thư mục có `project-copy.json` và `beat.wav` (882078 bytes). Ctrl+O mở lại
bản copy; UI báo `Đã mở project`, nội dung 4 từ vẫn hiện. Ctrl+Space phát beat
trên audio device Windows, playhead chạy tới khoảng 5 giây; Escape dừng và
đưa playhead về 0. Tiến trình test đã đóng. Đây là smoke test Open/Save As/
playback, chưa kiểm tra relink thực tế (reference fixture vẫn thiếu
`source.wav`), SRT, seek/pause-resume, timer autosave hoặc crash UI.

### W3: runtime tap-sync trên Windows

Fixture Save As ban đầu có một từ `locked=true`; UI từ chối bắt đầu phiên
tap cho đến khi mở khóa lời. Tạo bản riêng `target/runtime-w3/tap-fixture/`
không có cờ khóa. Ctrl+T bắt đầu session, Space khi beat đang phát ghi một
mốc (UI báo `Đã căn nhịp • chưa lưu`, thời điểm lấy từ snapshot playback),
Escape kết thúc session; Ctrl+Z hoàn tác đúng một bước và khôi phục từ
`Xin` về 0.20–0.60s. Recovery status chỉ ghi trong thư mục fixture test.
Đây là smoke test một tap + undo; nhiều từ liên tiếp, giữ Space ≥180ms,
Backspace, blur, hết audio và mapping khi nghe reference vẫn cần kiểm tra.
State machine Rust hiện có 7 unit tests đối chiếu các hợp đồng JS: quick tap,
hold theo wall clock ≥180ms, chống repeat/press sát nhau, mốc dưới 40ms,
clock lùi, Backspace/finish và beat→reference/locked validation. `cargo test
-p vietkara-domain` pass 24 unit + 3 settings integration; `node --test
tests/tapping.test.js` pass 10/10; workspace Clippy và test đã lọc FFmpeg pass.
