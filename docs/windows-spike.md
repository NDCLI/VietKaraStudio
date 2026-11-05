# Spike GPUI / Windows — runbook (Giai đoạn 1)

> Kế hoạch: `RUST_REWRITE_PLAN.md` §4.1 và §9 bước 3.
> Nhánh: `codex/rust-gpui-rewrite`, commit baseline `a047c6b`
> (Phase 0 + đầu Phase 2, kiểm chứng trên Linux 2026-11-05).
> Mục tiêu spike: quyết định **tiếp tục với GPUI 0.2.2 đã khóa, hay dừng ở
> điểm dừng** (giữ `vietkara-domain`/`vietkara-storage`, thay UI shell)
> trước khi đầu tư editor.

## 0. Tình trạng hiện tại — đọc trước khi chạy

- Đã xong trên Linux: `cargo test` 6 crate (domain, storage, jobs, media,
  render, ai-bridge) **62/62 pass**; `cargo fmt --check` và `cargo clippy
  --all-targets -- -D warnings` sạch; Python `tests/test_timing.py` 4/4,
  JS tapping/timeline 14/14.
- **Chưa kiểm chứng:** `vietkara-app` (GPUI **0.2.2**) chưa build/run được
  trên Linux và chưa build trên Windows lần nào.
- **Giới hạn quan trọng của app hiện tại:** `crates/vietkara-app/src/main.rs`
  chỉ là UI mô phỏng — chưa có action thật cho mở file, playback,
  timeline, import, save hay export; cờ `playing` không bao giờ đổi.
  Vì vậy các mục thủ công dưới đây kiểm chứng **khả năng của GPUI trên
  Windows** (cửa sổ, IME, file picker, đường dẫn Unicode, không block UI),
  không phải tính năng của app. Ghi rõ mục nào app chưa có, đừng đánh
  "pass" cho tính năng chưa tồn tại.
- Các crate headless `vietkara-jobs`/`vietkara-media`/`vietkara-render`/
  `vietkara-ai-bridge` đã xong trên Linux (phần không cần GUI/thiết bị);
  bước playback của spike vẫn chỉ dùng **fake clock** (đồng hồ giả tick
  ngoài UI thread), chưa phát audio thật — thiết bị audio thật là việc
  của spike, không phải của `vietkara-media` (`NullPlayer`).

## 1. Máy và công cụ cần có

- Windows 10/11 x64, màn hình có thể đổi DPI/scale (100% / 125% / 150%).
- Visual Studio Build Tools (MSVC, workload Desktop development with C++),
  Git, PowerShell 5.1 trở lên.
- Rust **1.99.1** — `rust-toolchain.toml` đã ghim; kiểm tra bằng
  `rustc --version` và `cargo --version` (phải là 1.99.1).
- FFmpeg/FFprobe (khuyến nghị; chỉ để probe, chưa bắt buộc cho spike).
- Bộ gõ tiếng Việt (Unikey Telex hoặc Windows Vietnamese keyboard) và font
  hỗ trợ Unicode; chuẩn bị sẵn chuỗi test: `Xin chào`, `Tạm biệt`,
  `Điệp khúc`, `Bài hát tiếng Việt`.

## 2. Chuẩn bị

```powershell
git clone https://github.com/NDCLI/VietKaraStudio.git
cd VietKaraStudio
git checkout codex/rust-gpui-rewrite
git log --oneline -1   # kỳ vọng: a047c6b
rustc --version        # kỳ vọng: 1.99.1
cargo --version        # kỳ vọng: 1.99.1
```

## 3. Kiểm tra tự động

```powershell
powershell -ExecutionPolicy Bypass -File scripts\windows-spike-check.ps1
```

Script chạy, theo thứ tự: phiên bản toolchain, nhánh/commit, FFmpeg/FFprobe
(nếu có), `cargo fmt --check`,
`cargo test` 6 crate headless,
`cargo clippy` 6 crate `--all-targets -- -D warnings`,
rồi `cargo check -p vietkara-app` (bước chưa từng pass).
Kết quả ghi vào `windows-spike-results.log` (đã nằm trong `.gitignore`
qua `*.log`, không commit). Nếu `cargo check -p vietkara-app` treo quá
10 phút: Ctrl+C, ghi lại thời gian treo và output cuối cùng vào log —
đó chính là kết quả spike, đừng coi là "chưa chạy".

Kỳ vọng tối thiểu: 6 crate headless vẫn 62/62 trên Windows như trên Linux.
Nếu crate nào fail trên Windows, dừng lại và ghi lỗi — đó là phát
hiện quan trọng hơn cả GPUI.

## 4. Kiểm tra thủ công

Chạy app (sau khi `cargo check` pass):

```powershell
cargo run -p vietkara-app
```

| # | Kiểm tra | Cách làm | Kết quả |
|---|---|---|---|
| 1 | Cửa sổ mở, resize | Kéo 4 cạnh/góc, maximize/minimize/restore | Pass / Fail + ghi chú |
| 2 | DPI/scale | Đổi scale Windows 100% → 125% → 150%, mở lại app, kiểm tra chữ/layout có vỡ không | Pass / Fail + ghi chú |
| 3 | File picker / drag-drop | Mở file picker (nếu app chưa có nút, ghi "chưa có action" — đây là gap, không phải lỗi GPUI); kéo file project vào cửa sổ | Pass / Fail / Chưa có |
| 4 | IME tiếng Việt | Gõ bằng Telex: `Xin chào`, `Tạm biệt`, `Điệp khúc`; thử paste đoạn tiếng Việt; kiểm tra dấu hiển thị đúng, không mất ký tự | Pass / Fail + ghi chú |
| 5 | Phím Space / phím tắt | Nhấn Space và các phím tắt trong app; ghi rõ phím nào có tác dụng, phím nào chưa được xử lý (app hiện chưa gắn action) | Pass / Fail / Chưa có |
| 6 | Font Unicode / VNI / TCVN3 | Hiển thị chuỗi test ở §1; ghi font nào render đúng, font nào lỗi | Pass / Fail + ghi chú |
| 7 | Đường dẫn Unicode | Copy `crates/vietkara-domain/tests/fixtures/project-v2.json` vào thư mục tên `Bài hát tiếng Việt` (có dấu + khoảng trắng), mở/save round-trip, kiểm tra field lạ `future_python_field` còn nguyên và ID vẫn 32 hex | Pass / Fail + ghi chú |
| 8 | Fake playback clock | **Chưa có trong app.** Quyết định tại chỗ: (a) thêm một thread tick clock giả 10 Hz ngoài UI thread trong spike để đo UI có block không, hay (b) hoãn sang `vietkara-media` ở bước 4. Ghi quyết định và lý do, đừng giả vờ đã test playback thật. | Quyết định: ___ |
| 9 | UI responsiveness | Trong lúc chạy test nặng (nếu có job nền), kéo/scroll cửa sổ xem có đơ không | Pass / Fail + ghi chú |

## 5. Ghi quyết định (điền sau khi chạy xong)

- GPUI 0.2.2 trên Windows 10/11: **tiếp tục / dừng** (khoanh một).
- Phiên bản đã khóa: Rust 1.99.1, GPUI 0.2.2, commit spike `___`.
- Giới hạn GPUI ghi nhận được: ___
- Bước tiếp theo theo §9: vertical slice playback/tap-sync có UI thật
  (dùng `vietkara-jobs` + `vietkara-media` đã xong phần headless) — chỉ
  bắt đầu sau khi mục 5 này được điền và commit kết quả spike.
- Nếu dừng: giữ `vietkara-domain`/`vietkara-storage`, thay UI shell trước
  khi port editor (theo điểm dừng §4.1); không port thêm tính năng vào
  `vietkara-app` cho tới khi có quyết định mới.

## 6. Không thuộc spike này

- Golden ASS/FFmpeg output (Giai đoạn 4, sau khi layout contract ổn định).
- Playback audio thật, waveform thật, tap-sync thật (Giai đoạn 3).
- Installer/portable, máy Windows sạch (Giai đoạn 6).
