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

- Spike GPUI/Windows/IME/playback (Giai đoạn 1) — chưa từng build `vietkara-app`.
- Phát audio bằng thiết bị thật, waveform/playhead thật, tap-sync thật.
- Full ASS layout (`build_ass` / `display_plan`) — cần đo font trên Windows.
- Entry point worker JSONL thật — `worker.py` hiện tại chưa nói protocol mới.
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
git log --oneline -1   # kỳ vọng: df1a652
rustc --version        # kỳ vọng: 1.99.1
cargo --version        # kỳ vọng: 1.99.1
powershell -ExecutionPolicy Bypass -File scripts\windows-spike-check.ps1
```

Script trên chạy: `cargo fmt --check`, `cargo test` 6 crate headless,
`cargo clippy -- -D warnings`, rồi `cargo check -p vietkara-app`.
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

1. [ ] Chạy `scripts/windows-spike-check.ps1`, ghi log vào
   `windows-spike-results.log` (đã gitignore, không commit).
2. [ ] `cargo run -p vietkara-app`: cửa sổ mở, resize, DPI 100% → 125% →
   150%, file picker / drag-drop, IME tiếng Việt, phím Space/phím tắt,
   font Unicode/VNI/TCVN3, đường dẫn Unicode (thư mục `Bài hát tiếng Việt`).
3. [ ] Fake playback clock: quyết định tại chỗ theo `windows-spike.md` §4
   mục 8 — (a) thêm thread tick clock giả 10 Hz ngoài UI thread để đo UI
   có block không, hay (b) hoãn sang `vietkara-media` ở §4. Ghi quyết định
   và lý do, đừng giả vờ đã test playback thật.
4. [ ] Điền quyết định vào `docs/windows-spike.md` §5:
   **GPUI 0.2.2: tiếp tục / dừng**, phiên bản đã khóa (Rust 1.99.1,
   GPUI 0.2.2, commit spike), giới hạn ghi nhận được.
5. [ ] Commit kết quả spike (chỉ file tài liệu + code spike nếu có):
   cập nhật `docs/windows-spike.md` §5 và `docs/compatibility-matrix.md`,
   push lên nhánh `codex/rust-gpui-rewrite`.

**Điểm dừng (kế hoạch gốc §4.1):** nếu GPUI không đáp ứng IME, file
dialog hoặc render ổn định, **dừng ở đây** — giữ `vietkara-domain` /
`vietkara-storage` / các crate headless, thay UI shell trước khi port
editor. Không port thêm tính năng vào `vietkara-app` cho tới khi có
quyết định mới, và ghi rõ quyết định đó vào `RUST_REWRITE_PLAN.md`.

## 3. Giai đoạn 2 — Hoàn thiện domain, storage và migration

Phần lớn đã xong trên Linux. Còn lại, làm trên Windows theo thứ tự:

1. [ ] **Undo/redo — command/event model** (kế hoạch §4.2): thiết kế
   command cho sửa word/anchor/settings/scene thay vì để UI sửa trực
   tiếp struct; mỗi command có undo/redo, và autosave không ghi đè bản
   chính khi command lỗi. Test: sửa → undo → redo → save → reopen,
   nội dung phải khớp.
2. [ ] **Settings đã typed:** chuyển các field settings hay dùng trong
   `styles.validate_settings` / `DEFAULT_SETTINGS` (font, font_size,
   aspect, resolution, render_quality/format/fps, audio_output, …)
   từ `serde_json::Value` sang typed struct có default, nhưng vẫn giữ
   unknown settings fields qua `flatten` (như project/word).
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
  mở file (file picker GPUI), tạo project mới, relink media thiếu
  (hiện cảnh báo + chọn lại file), save/close/reopen.
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

Hiện trạng: `vietkara-ai-bridge` đã xong protocol, nhưng `worker.py`
hiện tại **không nói protocol này**. Nó chạy qua
`desktop_entry.py --vcl-task speech <request-file.json>`, in
`OBS_PROGRESS {...}` ra stdout và ghi kết quả ra file
(`app_runtime.py::dispatch`, `worker.py::run`/`write_result`).

- [ ] Tạo entry point mới (ví dụ `worker_jsonl.py`, hoặc thêm task
  `--vcl-task jsonl` vào `app_runtime.py::TASKS` + `dispatch`):
  đọc **một dòng JSON** từ stdin (`protocol_version: 1`,
  `request_id`, `kind`, `project_id`, `input`, `options`),
  in các dòng JSONL `progress` / `result` / `error` ra stdout,
  log ra **stderr only** (stdout chỉ dành cho protocol).
- [ ] Các `kind`: `separate`, `recognize`, `align`, `align_region`,
  `role_suggest` — khớp `AiKind` trong `vietkara-ai-bridge`.
  Media truyền bằng path + metadata, không nhúng bytes.
- [ ] Test end-to-end qua `vietkara-ai-bridge::run_request` với worker
  thật (model nhỏ/tải riêng): progress hiển thị, cancel dừng worker,
  worker crash không treo app, timeout có lỗi rõ ràng.
- [ ] Không thay model/runtime ngay (Demucs/Whisper vẫn chạy bằng Python).

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
# Khi vietkara-app đã build được trên Windows, thêm:
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
# Baseline Python + JS (venv theo README.md):
.venv\Scripts\python.exe -m pytest tests -q
node --test tests/tapping.test.js tests/timeline_edit.test.js
```

Nếu `cargo test --workspace` vẫn fail vì `vietkara-app`/GPUI trên Windows,
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
