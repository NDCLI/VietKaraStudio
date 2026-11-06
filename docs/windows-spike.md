# Spike GPUI / Windows — runbook (Giai đoạn 1)

> Kế hoạch: `RUST_REWRITE_PLAN.md` §4.1 và §9 bước 3.
> Nhánh gốc: `codex/rust-gpui-rewrite`; nhánh làm việc local:
> `codex/rust-gpui-rewrite-local`; commit baseline `eb0df8d`
> (Phase 0 + headless jobs/media/render/ai-bridge + kế hoạch Windows,
> kiểm chứng trên Linux 2026-11-05: 62/62 test).
> Mục tiêu spike: quyết định **tiếp tục với GPUI 0.2.2 đã khóa, hay dừng ở
> điểm dừng** (giữ `vietkara-domain`/`vietkara-storage`, thay UI shell)
> trước khi đầu tư editor.

## 0. Tình trạng hiện tại — đọc trước khi chạy

- Đã xong trên Linux: `cargo test` 6 crate (domain, storage, jobs, media,
  render, ai-bridge) **62/62 pass**; `cargo fmt --check` và `cargo clippy
  --all-targets -- -D warnings` sạch; Python `tests/test_timing.py` 4/4,
  JS tapping/timeline 14/14.
- `vietkara-app` dùng GPUI **0.2.2**; trên Windows `cargo check`,
  `cargo test --workspace`, workspace Clippy và mở cửa sổ đã pass. Studio đã
  có project lifecycle, dialog open/save, playback, lyrics/timeline,
  preview/export và các panel chỉnh sửa; đây không còn là mock.
- Các example `ime_spike` và `dialog_spike` vẫn là harness tách biệt để kiểm
  IME/dialog. Kết quả của chúng chỉ chứng minh tương tác được thử trong spike;
  không thay cho runtime QA của toàn bộ Studio. Các mục chưa chạy phải được
  giữ riêng, không gộp thành pass chỉ vì ứng dụng build được.
- `vietkara-media` đã có playback worker dùng thiết bị thật và fake clock cho
  unit test. Phát/seek/pause trên thiết bị Windows cần được kiểm tra runtime
  theo fixture hiện hành; unit test hoặc mock clock không chứng minh được
  thiết bị cụ thể.
- **Windows 11 Pro x64, 2026-11-05:** dùng Rust 1.99.1, GPUI 0.2.2,
  MSVC Build Tools và PowerShell 5.1. `scripts/windows-spike-check.ps1`
  chạy thành công: 6 crate headless **62/62 test**, `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings` và
  `cargo check -p vietkara-app`; `cargo test --workspace` và
  `cargo clippy --workspace --all-targets -- -D warnings` cũng pass. App đã
  chạy và mở được cửa sổ; nhãn
  tiếng Việt hiển thị dấu đúng, maximize/restore hoạt động.
- Trong lần kiểm chứng này đã sửa import GPUI không tồn tại ở
  `vietkara-app`, chọn lệnh Python tương thích Windows cho fake worker
  tests, và sửa script để chạy được trên PowerShell 5.1.
- **Cập nhật sau kiểm tra trực tiếp của người dùng (2026-11-06):** gõ Telex
  tiếng Việt, thao tác Open/Save As trong dialog spike, kéo file, resize dọc
  và scale 125% được xác nhận ổn. Scale 150%, VNI/TCVN3 và runtime playback/
  timeline/export trên toàn bộ fixture vẫn chưa được xác nhận. Bảng §4 ghi
  từng mục và mức bằng chứng.

## 1. Máy và công cụ cần có

- Windows 10/11 x64, màn hình có thể đổi DPI/scale (100% / 125% / 150%).
- Visual Studio Build Tools (MSVC, workload Desktop development with C++),
  Git, PowerShell 5.1 trở lên.
- Rust **1.99.1** — `rust-toolchain.toml` đã ghim; kiểm tra bằng
  `rustc --version` và `cargo --version` (phải là 1.99.1).
- FFmpeg/FFprobe phải có trong `PATH` để chạy đủ test media của spike;
  FFmpeg sinh fixture test, FFprobe kiểm tra metadata/output. Script dừng
  trước test nếu thiếu một trong hai tool.
- Bộ gõ tiếng Việt (Unikey Telex hoặc Windows Vietnamese keyboard) và font
  hỗ trợ Unicode; chuẩn bị sẵn chuỗi test: `Xin chào`, `Tạm biệt`,
  `Điệp khúc`, `Bài hát tiếng Việt`.

## 2. Chuẩn bị

```powershell
git clone https://github.com/NDCLI/VietKaraStudio.git
cd VietKaraStudio
git checkout codex/rust-gpui-rewrite
git log --oneline -1   # kỳ vọng tối thiểu: eb0df8d (baseline của spike;
                     # nếu nhánh đã có commit mới hơn thì lấy commit đó)
rustc --version        # kỳ vọng: 1.99.1
cargo --version        # kỳ vọng: 1.99.1
```

## 3. Kiểm tra tự động

```powershell
powershell -ExecutionPolicy Bypass -File scripts\windows-spike-check.ps1
```

Script chạy, theo thứ tự: phiên bản toolchain, nhánh/commit, FFmpeg/FFprobe,
`cargo fmt --check`, `cargo test`/`cargo clippy` 6 crate headless,
`cargo check -p vietkara-app`, rồi `cargo test --workspace` và
`cargo clippy --workspace --all-targets -- -D warnings` để kiểm tra cả GPUI.
Kết quả ghi vào `windows-spike-results.log` (đã nằm trong `.gitignore`
qua `*.log`, không commit). Nếu `cargo check -p vietkara-app` treo quá
10 phút: Ctrl+C, ghi lại thời gian treo và output cuối cùng vào log —
đó chính là kết quả spike, đừng coi là "chưa chạy".

Kỳ vọng tối thiểu: 6 crate headless vẫn 62/62 trên Windows như trên Linux.
Nếu crate nào fail trên Windows, dừng lại và ghi lỗi — đó là phát
hiện quan trọng hơn cả GPUI.

## 4. Kiểm tra thủ công

Bản debug hiển thị kích thước viewport theo logical pixel và hệ số scale
ở thanh trạng thái (`GPUI spike`). Khi resize, đối chiếu kích thước này;
khi đổi DPI, ghi lại hệ số scale cùng ảnh giao diện. Bản release giữ
trạng thái thông thường. Chỉ báo hỗ trợ quan sát, không tự chứng minh
resize/DPI đã pass.


Chạy app (sau khi `cargo check` pass):

```powershell
cargo run -p vietkara-app
```

| # | Kiểm tra | Cách làm | Kết quả |
|---|---|---|---|
| 1 | Cửa sổ mở, resize | Kéo cạnh dọc/ngang, maximize/restore | Mở cửa sổ và maximize/restore pass; dialog spike: mép phải đổi viewport 900×500 → 719×500 ở scale 1; ảnh người dùng cho thấy 831×637 và 1243×757; người dùng xác nhận resize dọc ổn. Chưa thử đủ bốn cạnh/góc trên Studio |
| 2 | DPI/scale | Đổi scale Windows, mở lại app, kiểm tra chữ/layout | Người dùng xác nhận scale 125% (1.25) hoạt động ổn; 150% chưa xác nhận riêng |
| 3 | File picker / drag-drop | Open fixture, Save As bản sao, kéo file từ Explorer | Người dùng xác nhận các thao tác Open/Save As và kéo file trong dialog spike hoạt động; ảnh cho thấy nội dung tiếng Việt đã đọc được. Chưa thay thế runtime Open/Save trên nhiều fixture của Studio |
| 4 | IME tiếng Việt | Gõ bằng Telex: `Xin chào`, `Tạm biệt`, `Điệp khúc`; paste; sửa/xóa composition | Người dùng xác nhận gõ các chuỗi bằng Windows Vietnamese Telex; ảnh hiện tiếng Việt có dấu. Harness riêng đã xác nhận `as` → `á`, Space commit, Backspace trước/sau commit. Sửa/xóa trong lúc composition đang mở chưa có kết quả độc lập. Base layout label US không đủ để chẩn đoán TSF profile |
| 5 | Phím Space / phím tắt | Thử playback/tap-sync và các phím tắt Studio | Studio đã có transport và tap-sync actions; trong IME spike Space commit composition và Ctrl+A/Ctrl+V pass. Chưa kiểm thử toàn bộ phím tắt Studio trên fixture |
| 6 | Font Unicode / VNI / TCVN3 | Hiển thị chuỗi test ở §1; ghi font nào render đúng, font nào lỗi | Unicode tiếng Việt tĩnh: pass; VNI/TCVN3: chưa chạy |
| 7 | Đường dẫn Unicode | Copy `crates/vietkara-domain/tests/fixtures/project-v2.json` vào thư mục tên `Bài hát tiếng Việt` (có dấu + khoảng trắng), mở/save round-trip, kiểm tra field lạ `future_python_field` còn nguyên và ID vẫn 32 hex | Helper round-trip trên filesystem Unicode: 2/2 tests pass; spike đã có open/save, chưa xác minh chọn đường dẫn và round-trip qua dialog (§8) |
| 8 | Fake playback clock | **Chưa có trong app.** Quyết định tại chỗ: (a) thêm một thread tick clock giả 10 Hz ngoài UI thread trong spike để đo UI có block không, hay (b) hoãn sang `vietkara-media` ở bước 4. Ghi quyết định và lý do, đừng giả vờ đã test playback thật. | Hoãn tới vertical slice playback; fake clock hiện chỉ được test ở crate media, chưa nối UI |
| 9 | UI responsiveness | Trong lúc chạy test nặng (nếu có job nền), kéo/scroll cửa sổ xem có đơ không | Chưa chạy: mock không có job nền |

## 5. Ghi quyết định (điền sau khi chạy xong)

- GPUI 0.2.2 trên Windows 11 Pro x64: **GO — tiếp tục** (quyết định đầu
  ngày 2026-11-05; xác nhận lại 2026-11-06). Script tự động pass; người dùng
  xác nhận Telex, Open/Save As + kéo file trong dialog spike, resize dọc và
  scale 125%. Những mục chưa xác nhận riêng vẫn giữ trong checklist; đây
  không phải chứng nhận parity runtime đầy đủ.
- Phiên bản đã khóa: Rust 1.99.1, GPUI 0.2.2; baseline `eb0df8d`, các
  thay đổi hiện nằm trên nhánh local `codex/rust-gpui-rewrite-local`.
- Giới hạn đã ghi nhận: Studio hiện có action open/save, playback, tap-sync,
  timeline và export; vẫn cần runtime QA tổng hợp với fixture. `ime_spike`
  và `dialog_spike` là harness riêng để quan sát input/dialog, không thay thế
  Studio. User đã xác nhận chuỗi Telex, Open/Save As + kéo file, resize dọc
  và scale 125%; VNI/TCVN3, scale 150% và toàn bộ runtime Studio còn mở.
  Lỗi accessibility/focus cũ của helper không phải bằng chứng GPUI không hỗ trợ.
- Bước tiếp theo đã chuyển khỏi spike: tiếp tục QA runtime theo
  `docs/windows-plan.md` (thiết bị audio, playback/tap/timeline, save/recovery,
  batch/AI/export fixture) và đóng các khoảng parity còn lại.
- Giới hạn còn theo dõi: VNI/TCVN3, responsiveness khi có job thật, scale
  150% và chi tiết round-trip UI qua nhiều fixture chưa có log độc lập. Nếu phát
  hiện lỗi IME/dialog/render, đánh giá lại GO trước khi port tiếp editor.

## 6. Không thuộc spike này

- Golden ASS/FFmpeg output (Giai đoạn 4, sau khi layout contract ổn định).
- Runtime QA playback audio, waveform và tap-sync của Studio (Giai đoạn 3;
  code đã được nối, nhưng thiết bị/fixture Windows cần kiểm tra riêng).
- Installer/portable, máy Windows sạch (Giai đoạn 6).

## 7. IME input spike riêng

Chạy `cargo run -p vietkara-app --example ime_spike` trên Windows.
Ô nhập dùng EntityInputHandler của ví dụ GPUI 0.2.2 (nguồn và giấy phép
trong `third-party/gpui-input`). Gõ Telex các chuỗi ở §1; thử Ctrl+A,
Ctrl+C/X/V, phím mũi tên, Shift+mũi tên, Home/End, Backspace/Delete.
Kiểm tra cả lúc đang ghép dấu và sau khi kết thúc composition.
Ghi kết quả thực tế vào §4; build thành công không chứng minh IME pass.
Spike này độc lập với mock studio và chưa thay thế editor sản phẩm.

### Kết quả chạy IME spike trên Windows (2026-11-05)

- `cargo build -p vietkara-app --example ime_spike`: pass; cửa sổ mở.
- Click ô nhập rồi dán `Xin chào, Tạm biệt, Điệp khúc` qua UI helper:
  hiển thị đúng toàn bộ chuỗi và dấu; log trong spike ghi `ctrl-v`.
- Ctrl+A: toàn bộ chuỗi được highlight, log ghi `ctrl-a`.
- Layout hiển thị `Keyboard US`; thử Alt+Shift vẫn là US. Chưa có bằng
  chứng Telex composition, không đánh dấu IME hoàn tất.
- Accessibility chỉ lộ cửa sổ và title bar; xác nhận nội dung dựa trên
  screenshot thực tế. Lần chạy này capture sau thao tác hoạt động bình thường.
- Chưa xác minh cut/copy, sửa khi composition, DPI hoặc dialog.

## 8. Native dialog spike riêng

Chạy `cargo run -p vietkara-app --example dialog_spike`. Thử cả mở file,
hủy mở, chọn đường dẫn lưu và hủy lưu. Chọn fixture trong thư mục
`Bài hát tiếng Việt` để đối chiếu đường dẫn có dấu. Spike đọc project bằng domain loader trên background executor. Sau khi mở
fixture, nút lưu ghi project atomic rồi đọc lại trên background executor.
Hãy chọn tên bản sao mới, không dùng fixture gốc. Đối chiếu JSON bản sao
với fixture (nhất là field lạ và ID); thông báo đọc lại thành công chưa
chứng minh parity của mọi field. Runtime round-trip chưa được xác minh.
Trong khi dialog mở, các yêu cầu mới bị bỏ qua. Kết quả lỗi/hủy được
hiển thị riêng, không thay đổi project.

### Kết quả native dialog thực tế (2026-11-05)

- Build `dialog_spike`: pass; cửa sổ mở ở viewport 900×500, scale 1.
- Click mở file: dialog Windows `Open` hiện, caption chọn fixture tiếng Việt
  hiển thị đúng. Escape đóng dialog; app hiển thị `Đã hủy mở file`.
- Sau hủy, click lưu vẫn mở được dialog `Save As`; tên gợi ý
  `Bài hát tiếng Việt.json` hiển thị đúng dấu. Escape đóng dialog; app
  hiển thị `Đã hủy chọn đường dẫn lưu`. Pending đã được giải phóng.
- App được launch từ UI helper nên current directory là thư mục host,
  không phải repo; dialog lưu dùng current directory đúng theo code.
- Chưa xác minh đường dẫn được chọn trả về hay read/write round-trip.

Cập nhật code round-trip: `cargo check` và Clippy cho `dialog_spike` pass.
Nếu mở lỗi/hủy, giữ project đã mở trước đó; lưu khi chưa có project chỉ
hiện hướng dẫn. Các kết quả dialog ở trên được ghi trước thay đổi này;
cần chạy lại bản mới để xác minh đọc/ghi thực tế.

### Kiểm chứng dữ liệu round-trip trên Windows

`cargo test -p vietkara-app --example dialog_spike`: 2/2 pass. Test dùng
đúng helper lưu/đọc lại mà UI gọi, tạo thư mục tạm `Bài hát tiếng Việt`
và file `Bản sao Điệp khúc.json`; so sánh toàn bộ model JSON sau migration,
đồng thời kiểm tra ID và `future_python_field.keep`. Test còn lại xác nhận
lỗi lưu khi thiếu thư mục cha. Clippy all-targets của app pass. Script
spike đã thêm lệnh test example vì workspace tests không tự chạy các test
trong example. Bằng chứng này xác minh filesystem round-trip; chưa chứng
minh đường dẫn được chọn qua native dialog hoặc tương đương JSON thô
trước migration. Helper UI nay báo lỗi nếu toàn bộ model đọc lại khác.

### Lần kiểm tra dialog bản round-trip mới

Build example mới thành công và cửa sổ mới hiển thị nút lưu bản sao.
Đã chuẩn bị fixture tại `target/Bài hát tiếng Việt/fixture.json` (ignored).
Dialog Open mở, nhưng sau khi click ô filename qua accessibility index,
UI helper trả `accessibility: null`, không có focused element; refresh
và rebind lại cửa sổ vẫn không có focus observation. Không nhập đường dẫn
khi chưa xác nhận focus. Đã Escape để trả về spike. Chưa có kết quả chọn
file hoặc round-trip qua dialog ở lần này; không đánh dấu pass.

### Điểm cần kiểm tra trực tiếp để gỡ chặn W2

Các lần thử UI helper liên tiếp chưa xác nhận được focus ô filename
(kể cả Alt+N); không coi đây là bằng chứng lỗi GPUI. Cần người dùng thử
trực tiếp hai example: Telex composition trong `ime_spike`, chọn fixture
Unicode và lưu bản sao trong `dialog_spike`; đồng thời kiểm tra resize và
DPI 100/125/150%. Ghi kết quả thật trước quyết định go/no-go. Người dùng xác nhận dùng Windows Vietnamese Telex. Kiểm tra bằng Windows
PowerShell 5.1 xác nhận danh sách ngôn ngữ có en-US và vi với input profile
Telex; lần đọc trước bằng host shell cho output rỗng nên không đủ bằng
chứng kết luận chưa cài. IME spike vẫn hiển thị Keyboard US; Ctrl+Shift
không chuyển layout. Cần chọn Telex cho cửa sổ spike trước khi thử composition. Không có quyết định GO
được suy ra từ build hoặc filesystem tests.

### Telex composition sau khi người dùng chuyển profile

Người dùng xác nhận đã chọn Windows Vietnamese Telex. Sau khi activate
spike và click trực tiếp ô nhập để có caret, UI helper gửi phím vật lý
`a`, chụp trạng thái, rồi `s`, chụp trạng thái: chữ `a` mới chèn biến
thành `á` (không dùng paste/type_text). Đây là bằng chứng composition
cơ bản hoạt động. Nhãn GPUI vẫn ghi `Keyboard US`, vì vậy nhãn layout
không đủ để chẩn đoán TSF profile. Trước khi click lại ô nhập, phím chỉ
xuất hiện trong log mà không sửa nội dung; cần xác nhận caret/focus thực.
Chưa kiểm tra đầy đủ ba chuỗi Telex, sửa/xóa khi composition, hoặc commit
qua Space. Không đánh dấu toàn bộ IME checklist pass từ phép thử `as`.

### Commit và xóa ký tự Telex

Đã thử chuỗi độc lập bằng phím thật, chụp sau mỗi bước: Ctrl+A chọn
toàn bộ nội dung; `a` thay selection thành `a` có gạch composition;
`s` đổi thành `á` có gạch composition; Space commit thành `á `, caret
sau khoảng trắng và không còn gạch composition. Backspace lần 1 bỏ
khoảng trắng, giữ `á`; lần 2 xóa `á`, ô nhập trở về placeholder. Không
mất/crash ở các bước này. Chưa chứng minh sửa/xóa khi composition đang
mở hoặc toàn bộ các chuỗi Telex trong checklist.

### Backspace trong composition đang mở

Từ ô rỗng, gửi phím thật `a` rồi `s`: hiện `á` có gạch composition.
Chưa Space/commit, nhấn Backspace: ô trở về placeholder, không treo.
Đây là quan sát hành vi xóa ký tự đang ghép của Telex; chưa khẳng định
parity mọi cách sửa composition. Nhãn spike được đổi thành `Base layout`
và ghi rõ TSF/Telex có thể khác, tránh nhầm nhãn US với profile đang dùng.

### Resize bằng kéo mép thực tế

Trong dialog spike, ảnh ban đầu ghi viewport 900×500, scale 1. Kéo đúng
mép ngoài phải làm viewport đổi thành 719×500, các nút co theo chiều rộng
và trạng thái vẫn render. Lần kéo trước ở điểm trong client không resize.
Phép thử mép dưới bị UI helper chặn với `user input was detected in this
window; call get_window_state before continuing`; đã refresh, ảnh lúc đó
hiển thị ứng dụng khác nên dừng thao tác UI để không tranh quyền điều khiển.
Chưa xác minh resize dọc và đủ bốn cạnh/góc.

### Hook drag-drop trong dialog spike

Root của `dialog_spike` nhận `ExternalPaths` từ GPUI. Thả đúng một file
để đọc project trên background executor; nhiều file hiện thông báo và
giữ project cũ. Trong khi đang đọc/dialog pending, yêu cầu thả mới bị
bỏ qua. Load lỗi giữ project trước đó. Có thể dùng project vừa thả để
lưu bản sao qua dialog. Clippy all-targets và hai test round-trip hiện
có pass sau thay đổi; các test đó không gọi native drag-drop handler.
Chưa chạy thao tác kéo file từ Explorer, không đánh dấu drag-drop pass.

### Chạy lại toàn bộ script sau khi thêm các example

2026-11-05 19:28 (giờ máy): `powershell.exe -NoProfile -ExecutionPolicy
Bypass -File scripts/windows-spike-check.ps1` exit 0, ALL AUTOMATED CHECKS
PASSED. Gồm fmt, headless tests/clippy, app check, hai test dialog example,
workspace tests và workspace clippy all-targets. Các tool được đưa vào
PATH riêng của shell: Rust user profile, Python venv workspace cũ và
FFmpeg/FFprobe trong portable local. Không đổi PATH hệ thống. Log vẫn
ignored. Đây là kết quả tự động của source hiện tại, không hoàn tất W2
thủ công hoặc quyết định go/no-go.

### Ảnh người dùng cung cấp sau lượt kiểm tra thủ công

Hai ảnh ngày 2026-11-05 được giữ local trong
`target/windows-spike-evidence/` (ignored), tên clipboard kết thúc
`427d1b0ea1d2.png` và `30e9564d2c65.png`.

- IME spike hiển thị `Xin chào Tạm Biệt, Điệp Khúc`, toàn chuỗi được chọn;
  log có Ctrl+A/C và nhiều phím ký tự. Ảnh chứng minh hiển thị và selection;
  chưa đủ để khẳng định phương thức nhập của toàn chuỗi.
- Dialog spike báo đã đọc project `Bài karaoke mới`, 0 từ, từ project.json
  trong workspace cũ; đường dẫn ASCII này tồn tại trên máy. Thông báo là
  nhánh mở dialog (khác nhánh `Đã đọc file thả`). Chọn file và đọc project
  qua dialog đã có bằng chứng, nhưng chưa phải fixture Unicode round-trip.
- Viewport lần lượt 831×637 và 1243×757, scale đều 1. Có bằng chứng thay
  đổi cả chiều rộng và cao; không suy ra DPI 125%/150% từ kích thước ảnh.
- Chưa thấy thông báo lưu/đọc lại hoặc đọc file thả; không đánh dấu hai
  thao tác đó pass từ ảnh. Chờ người dùng xác nhận các thao tác đã làm.

### Phản hồi người dùng và quyết định GO

Ảnh clipboard kết thúc `4dec8159eb8a.png` hiển thị `Đã đọc file thả:
reference | 312 từ`, viewport 872×620, scale 1: native drag-drop đã đọc
được project thật, khác với bằng chứng mở file qua dialog trước đó.
Người dùng trả lời `ok r` cho câu hỏi gộp về gõ Telex, lưu bản sao,
kéo thả và scale 125%/150%. Ghi nhận đây là báo cáo thủ công ngắn, chưa
có kết quả tách từng thao tác; không suy ra ảnh scale 1 chứng minh DPI khác.
Quyết định kỹ thuật: tiếp tục W3 theo bằng chứng hiện có, giữ nguyên các
khoảng trống kiểm chứng được nêu để theo dõi trong quá trình tích hợp.

Người dùng bổ sung: `scale cx ok đã chuyển 1.25`. Ghi nhận scale 125%
hoạt động theo báo cáo trực tiếp; chưa có xác nhận riêng cho 150%.

Người dùng xác nhận tiếp `ok hết`: ghi nhận các mục kiểm tra thủ công đã
được người dùng chấp nhận, bao gồm phần còn lại trong câu hỏi gộp. GO
được giữ để triển khai W3. Ảnh/log độc lập vẫn chỉ chứng minh đúng phạm vi
đã ghi; không đổi các báo cáo người dùng thành kiểm chứng tự động.
