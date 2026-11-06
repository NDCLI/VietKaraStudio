# Ma trận tương thích VietKara Studio — Rust / Python

> Phase 0 (plan `RUST_REWRITE_PLAN.md` §4, Giai đoạn 0). Baseline Python là
> nhánh `codex/ui-redesign-1.1.4`, commit `c8dae55` (release 1.1.7).
> Nhánh Rust: `codex/rust-gpui-rewrite`.

## Môi trường kiểm chứng trong lần cập nhật này

- Rust toolchain: **1.99.1** (đã ghim trong `rust-toolchain.toml`; trước đây
  `stable-x86_64-pc-windows-msvc` làm `cargo` lỗi trên mọi host không phải
  Windows: `target tuple in channel name`).
- Host kiểm chứng Linux: x86_64. `cargo check/test -p vietkara-domain` và
  `-p vietkara-storage` chạy được.
- `vietkara-app` (GPUI 0.2.2) chưa được build/run trên Linux. Trên Windows,
  `cargo check`, release build, workspace tests/Clippy và runtime window đã
  pass. Người dùng xác nhận Telex, Open/Save As + kéo file ở dialog spike,
  resize dọc và scale 125%; scale 150%, VNI/TCVN3 và playback/export fixture
  vẫn còn mở, xem `docs/windows-spike.md` §4–5.
- Python baseline: `python3 -m pytest tests/test_timing.py tests/test_app.py -q`
  (xem README). Trong lần này chỉ chạy được `test_timing.py` (4/4 pass)
  và JS tapping/timeline (14/14 pass); `test_app.py` cần Flask và các
  dependency Python khác, chưa cài trên máy Linux này.
- Version snapshot dùng hard link cho media (như Python `os.link`):
  media chỉ được thay bằng rename (atomic), không được ghi đè/truncate
  tại chỗ, nếu không snapshot sẽ bị hỏng theo.
- Kết quả Rust 2026-11-05 (đợt 1): `cargo test -p vietkara-domain
  -p vietkara-storage` 14/14 pass; `cargo fmt --check` và
  `cargo clippy -p vietkara-domain -p vietkara-storage --all-targets
  -- -D warnings` sạch.
- Kết quả Rust 2026-11-05 (đợt 2, phần headless): 6 crate, `cargo test`
  **62/62 pass** — domain 8, storage 6, jobs 5, media 14, render 18,
  ai-bridge 11; `cargo fmt --check` và `cargo clippy` (6 crate,
  `-- -D warnings`) sạch. FFmpeg/ffprobe 8.1.2 có sẵn trên máy Linux
  kiểm chứng, nên probe/render thật đã được test bằng file fixture
  sinh bằng ffmpeg.
- Kết quả Windows 11 Pro x64 2026-11-05/06: PowerShell 5.1 chạy
  `scripts/windows-spike-check.ps1` thành công; Rust 1.99.1,
  `cargo fmt --check`, 6 crate **62/62 test**, `cargo clippy` với
  `-D warnings`, `cargo check -p vietkara-app`, `cargo test --workspace`
  và `cargo clippy --workspace --all-targets -- -D warnings` đều pass.
  GPUI 0.2.2 đã mở cửa sổ; người dùng xác nhận IME Telex, thao tác
  Open/Save As + kéo file trong dialog spike, resize dọc và scale 125%.
  Scale 150%, VNI/TCVN3, runtime QA trên Studio và Windows sạch chưa được
  xác nhận. Quyết định GPUI là GO theo runbook, chưa phải parity đầy đủ.
- Baseline Python `pytest tests -q`: **167/167 pass**; Node tapping/timeline:
  **14/14 pass**. Python cần `resources/source`, không có trong Git checkout
  mới; dùng junction tạm tới asset local sẵn có rồi gỡ sau khi chạy test.

### Cập nhật 2026-11-06 — CI và test phụ thuộc môi trường

- Thêm `.github/workflows/ci.yml`:
  - **rust-headless** (Ubuntu): `cargo fmt --check`, clippy `-D warnings` và
    test cho 6 crate headless.
  - **rust-windows**: fmt, clippy workspace, `cargo test --workspace` và release
    build `vietkara-app` trên `windows-latest`.
  - **scripting**: `python -m compileall` và Node tapping/timeline.
- Các test cần FFmpeg/FFprobe (`vietkara-media`, `vietkara-render`,
  `vietkara-app` export) hoặc Python có `numpy/scipy/soundfile`
  (`vietkara-ai-bridge` sync) nay **tự skip** khi thiếu binary/module thay vì
  fail. Trên máy không có FFmpeg/numpy: `cargo test --workspace` **209 pass,
  1 ignored, 0 failed**; trước đây cùng môi trường fail 7 test.
- Trên máy có FFmpeg/ffprobe và Python đầy đủ, các test trên vẫn chạy bình
  thường; CI Windows có thể cài thêm FFmpeg nếu muốn bật integration.

## Trạng thái các crate Rust

| Crate | Port từ Python | Trạng thái |
|---|---|---|
| `vietkara-jobs` | `engine.Job`, `app.py::persist_job`/`recover_jobs`, `batch_render.py` (record state machine) | ✅ lifecycle/cancel/persist/recover, batch `first_pending`/retry |
| `vietkara-media` | `media_tools.probe_video`, `engine.waveform`/`convert`, `render_info._audio_metrics`/`fps_value` | ✅ ffprobe JSON + fallback `ffmpeg -i`, WAV PCM/float, waveform peak = max tuyệt đối trên mọi kênh (đã sửa để khớp Python; mono mix sẽ triệt tiêu stereo ngược pha), metrics, `NullPlayer` + fake clock, và player thiết bị qua worker. Runtime thiết bị thật/seek/pause-resume vẫn cần fixture QA. |
| `vietkara-render` | `subtitles.py`, `layout.py`, `render_info.py`, `engine.render` | ✅ `display_plan`, ASS lyrics/title/role artwork/scenes/countdown, render profile, backgrounds/overlays/audio normalize, FFmpeg command và output verification. Windows `cargo test --workspace` pass với FFmpeg/ffprobe từ portable build4; test render fallback FFmpeg không ffprobe cũng pass. Golden parity đầy đủ và runtime fixture sản phẩm vẫn cần chốt. |
| `vietkara-ai-bridge` | `worker.py` qua adapter `ai_worker.py`, plan §3.4/Phase 5 | ✅ JSON Lines v1, spawn/timeout/cancel/crash recovery và adapter separation/recognition/alignment/role/sync. Runtime với model thật vẫn cần smoke test trên Windows. |

## Fixture

| Fixture | Mô tả |
|---|---|
| `crates/vietkara-domain/tests/fixtures/project-v1.json` | Schema 1, anchors chưa sắp xếp, word thiếu/trùng/sai định dạng ID, role không hợp lệ, từ trống, `label_color` viết hoa, `row`/`section`/`locked`/`manual`, unknown field `future_python_field` ở cấp project. |
| `crates/vietkara-domain/tests/fixtures/project-v2.json` | Schema 2 đã chuẩn hóa, ID 32 hex, anchors đã sắp xếp, unknown field được giữ. Dùng làm golden round-trip: load → save → load phải giữ nguyên nội dung (theo JSON đã parse, không so chuỗi). |
| `crates/vietkara-storage/tests/fixtures/` | Project + media nhỏ để kiểm tra manifest SHA-256, version snapshot và restore an toàn. |

ID trong fixture v1 cố tình không ổn định sau migration (Python và Rust đều
sinh ID mới bằng `uuid4().hex` khi ID cũ sai định dạng/trùng). Vì vậy test
migration chỉ assert *tính chất* (định dạng 32 hex, duy nhất, `line_uid`
dùng chung trong cùng `line`, role đã chuẩn hóa, thời gian làm tròn 4 chữ
số thập phân, từ trống bị bỏ), không assert ID cụ thể.

## Hợp đồng tương thích đã chốt

| Hạng mục | Python baseline (`app.py`, `timing.py`, `project_archive.py`) | Rust (`vietkara-domain`, `vietkara-storage`) | Trạng thái |
|---|---|---|---|
| Project ID | `uuid.uuid4().hex` — 32 ký tự `[0-9a-f]` | `Uuid::new_v4().simple()` — cùng định dạng; ID rỗng khi load được sinh mới, ID đã có được giữ nguyên | ✅ |
| Word / line ID | Sai định dạng hoặc trùng → sinh mới; `line_uid` dùng chung cho mọi word cùng `line` | `Project::migrate()` làm đúng quy tắc trên | ✅ test |
| Schema | Thiếu schema = 1; `load()` migrate lên 2 khi schema < 2 hoặc word thiếu id/line_uid; import chỉ nhận 1/2 | Thiếu schema = 1 (trước đây mặc định sai thành 2); `load()` luôn chuẩn hóa rồi đặt schema = 2; schema > 2 → `FutureSchema` | ✅ test |
| Word text | `strip()`, cắt 200 ký tự, bỏ từ trống | Cùng quy tắc, đếm theo ký tự (không phải byte) | ✅ test |
| Word timing | `start >= 0`, `end > start`, làm tròn 4 chữ số | Cùng quy tắc; lỗi `InvalidWordTime` | ✅ test |
| Role | Không hợp lệ → `"all"` (không lỗi) | `migrate()` ép về `"all"`; `validate()` vẫn báo `UnknownRole` với project chưa chuẩn hóa | ✅ test |
| Duet lanes | Baseline có các vai mở rộng `voice_c`…`voice_f` | Studio hiển thị/chỉnh hai vai Nam/Nữ; dữ liệu vai cũ được giữ nguyên khi load/save, nhưng preview và ASS gập `voice_c`/`voice_e` vào Nam, `voice_d`/`voice_f` vào Nữ | ✅ quyết định sản phẩm; kiểm thử renderer |
| Word fields giữ lại | `estimated` (luôn có), `locked`/`section`/`manual` (chỉ khi true), `section_name` (cắt 40 ký tự), `label_color` (`#RRGGBB`, lỗi nếu sai, lưu lowercase), `row` (int 0–5) | Rust vẫn đọc legacy `row` 0–5, rồi migrate thành lane 0 (Nam) cho số chẵn hoặc lane 1 (Nữ) cho số lẻ; reject row ngoài 0–5; dữ liệu đã chuẩn hóa chỉ nhận 0/1 | ✅ test |
| Anchors | Sắp xếp theo `ref`, `>= 0`, khoảng cách `>= 0.02` cả `ref` và `beat` | `migrate()` sắp xếp trước khi validate; cùng ngưỡng | ✅ test |
| Unknown fields | `validate_words` *bỏ* field lạ ở cấp word khi save qua Python; project-level field lạ được giữ trong dict | Rust giữ unknown fields ở cấp project/word qua `serde(flatten)` — **an toàn hơn Python, cố tình khác**: round-trip Rust không làm mất field lạ; khi project quay lại Python và save, Python có thể bỏ field lạ ở cấp word (ghi rõ để không bất ngờ) | ✅ test, khác biệt đã ghi |
| Save | Ghi file tạm cùng thư mục rồi `replace()` (atomic); `updated_at = time.time()` | `Project::save()` ghi file tạm duy nhất cùng thư mục rồi rename (atomic); **chưa** đặt `updated_at` — storage/caller chịu trách nhiệm nếu cần | ✅ test |
| Archive manifest | SHA-256 từng media, giới hạn ZIP 3 GiB / 2.000 entry, project JSON 20 MiB, allowlist tên media (`FIXED_MEDIA` + pattern `scene_…`) | `vietkara-storage`: cùng hằng số và allowlist, `media_manifest()` tính SHA-256 | ✅ test |
| Restore version | **Không an toàn**: xóa media hiện có trước khi chép media khôi phục; lỗi giữa chừng có thể mất media | `restore_version()`: xác minh size + SHA-256 toàn bộ trước, stage vào thư mục tạm, swap rồi mới xóa bản cũ; lỗi thì rollback từ bản sao lưu | ✅ test (cố tình khác Python, an toàn hơn) |
| Import bundle | Giải nén media vào thư mục đích trước khi kiểm tra xong manifest/fingerprint | Storage: chỉ chấp nhận sau khi kiểm tra manifest/fingerprint trong staging; bundle ZIP do `read_bundle`/`write_bundle` kiểm tra giới hạn và tên file | ⚠️ ZIP bundle: API có, test round-trip có; kiểm tra trên Windows chưa làm |
| Beat / vocal | Beat gốc là audio xuất video; vocal tách riêng chỉ phục vụ nhận dạng/căn lời | Chưa port media/AI — ngoài phạm vi Giai đoạn 2 | ⏳ Giai đoạn 3–5 |

## Ưu tiên (từ plan §5)

- **P0 — MVP lát dọc:** schema 1/2 + migration an toàn; open/save/autosave/relink;
  nhập SRT/lyrics; phát beat, seek, tap-sync; chỉnh timeline; preview cơ bản;
  MP4 export dùng beat gốc; progress/cancel/error.
- **P1 — parity:** archive/version restore, undo/redo đầy đủ, tách vocal/ASR/
  alignment qua worker, audio sync, duet roles, presets/font, title/logo/
  background/countdown, batch render.
- **P2 — phát hành:** updater, installer/portable, diagnostics, tối ưu GPU/cache.

## Lệnh kiểm chứng

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 -m pytest tests/test_timing.py tests/test_app.py -q
node --test tests/tapping.test.js tests/timeline_edit.test.js
```

Lưu ý: `cargo test --workspace` cần host Windows để build `vietkara-app`
(GPUI); trên Linux dùng danh sách `-p` của job `rust-headless` trong CI.
Test tích hợp cần FFmpeg/FFprobe hoặc `numpy/scipy/soundfile` tự skip khi
môi trường thiếu, nên suite vẫn xanh trên checkout sạch.
