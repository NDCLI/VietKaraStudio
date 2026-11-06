# VietKara Studio: kế hoạch chuyển dần sang Rust + GPUI

> **Trạng thái kế hoạch:** cập nhật sau khi kiểm kê workspace hiện tại.

> **Tiến độ 2026-11-06 — batch UI và xác minh FFmpeg fallback:** toolbar GPUI
> được phép wrap để nút batch không bị cắt ở cửa sổ thấp/rộng. Tách parser
> summary FFmpeg để unit test fallback; test FFmpeg 7 không có ffprobe phát
> hiện và sửa việc nhận mã codec `0x...` thành kích thước video. `cargo test
> --workspace` pass trên Windows với Python `.venv` và cặp FFmpeg/ffprobe từ
> portable build4. `cargo clippy --workspace --all-targets -- -D warnings`,
> `cargo fmt --all -- --check` và `git diff --check` cũng pass. Whisper small
> model test vẫn ignored vì cần model/fixture riêng; UI batch, runtime NVENC,
> QA cài đặt máy sạch và phát hành/update E2E còn mở. Node tapping/timeline
> 14/14 pass. Python suite baseline riêng 162/173 pass; 11 fail ở test export,
> phân vai vocal và UI/video baseline, không thuộc workspace Rust. Batch
> snapshot được tách khỏi GPUI và có regression test cho media allowlist, hash,
> source đổi sau khi queue, retry không ghi đè và tamper snapshot; UI batch
> runtime vẫn chưa được xác minh.
>
> **Nguyên tắc:** không “đập đi viết lại” một lần. Ứng dụng Python hiện tại vẫn là production baseline cho đến khi từng lát chức năng Rust đạt parity, có fixture kiểm chứng và có đường rollback.
>
> **Tiến độ W7 — preset mẫu trong Studio:**
> - Nhúng 16 preset VCL hiện có, hiển thị catalog trong Studio; chọn mẫu chỉ
>   mở bước xem lại, áp dụng bằng một `SetSettings` undo step. Preset cá nhân
>   được lưu trong app data, nạp lại sau restart và xóa có xác nhận. Import
>   VCL/JSON và export an toàn cũng đã có.
> - Nhúng thư viện 192 họ font baseline (315 TTF, Unicode/VNI/TCVN3), có tìm
>   kiếm/chọn font; GPUI dùng font trong preview, FFmpeg nhận đúng biến thể
>   font đang dùng trong `fonts/` tạm. Còn smoke test ảnh/render Windows.
> - Thêm chẩn đoán cục bộ Python/module AI/worker/FFmpeg/fonts/app-data có
>   timeout, không đọc media/project hay gửi mạng; báo cáo nằm trong app data.
> - Inspector cho gỡ ảnh nền scene riêng với artwork trang trí. Thao tác tạo
>   version snapshot, giữ metadata và decoration nếu còn, dọn record khi scene
>   không còn artwork; hai nhánh transaction đã pass test storage.
> - Workspace check/clippy, Python syntax và Windows release build pass.
> - Thêm `packaging/build_rust_portable.py`: dựng stage portable từ app release,
>   Python runtime + AI dependencies và FFmpeg do người build chỉ định; tạo
>   launcher cấu hình worker/media/data, manifest SHA-256 và license. Script
>   chặn output đè thư mục có sẵn; không chép project/model riêng của máy build.
>   Đã dựng stage Windows local 5,3 GiB từ Python 3.12 + runtime AI Torch/CUDA
>   và FFmpeg; import AI trong runtime đã copy, hai binary media chạy được.
>   Smoke JSONL phát hiện lỗi Windows cp1252 làm worker không ghi được tiếng
>   Việt; đã ép UTF-8 ở stdin/stdout/stderr và thêm regression test. Smoke
>   lỗi protocol sau sửa pass với `PYTHONIOENCODING=cp1252`; chưa chạy trên máy
>   sạch. `packaging/archive_rust_portable.py` đã xác minh hash từng file trong
>   manifest, tạo ZIP 3,61 GB và SHA256SUMS; đọc lại ZIP/CRC/checksum đều pass
>   (16.541 entries, không có `data/`). Installer/updater và rollout còn mở.
> - Inno Setup per-user installer đã build ở `releases/`, AppId/thư mục cài
>   riêng để cài song song baseline; launcher cấp runtime paths và lưu data
>   tại `%LOCALAPPDATA%/VietKaraStudio-Rust`, bên ngoài thư mục uninstall.
>   Setup 2,98 GB và SHA-256 pass; silent install/uninstall trên thư mục thử
>   pass; build3 cũng đã mở app và xác nhận shortcut Desktop trỏ đúng launcher.
>   Build4 mới nhất silent install/uninstall pass; payload chứa updater helper
>   mới. Windows sạch vẫn chưa xác minh.
> - Thêm updater helper C# vào installer; nút “Cập nhật” trong Studio chỉ mở
>   helper khi project sạch. Helper chọn đúng stable Rust release (bỏ qua tag
>   baseline Python), tải các part GitHub, xác minh HTTPS/host, size/hash từng
>   part và hash installer ghép rồi mới mở Setup. `split_rust_release.py` tạo
>   asset parts/manifest; C# compile và cargo check pass. Chưa có Rust release
>   trên GitHub nên download/update end-to-end chưa chạy. Build4 local Setup
>   gồm cleanup cho `.partial`; SHA-256 và chi tiết smoke nằm trong
>   `docs/windows-plan.md`. Đây là snapshot kiểm thử, chưa phải GitHub release.
> - Lượt test gom đã chạy: Rust workspace, Clippy `-D warnings`, fmt check,
>   Python **188/188**, Node **17/17**, cùng smoke ghép part bằng PowerShell 5.1.
>   Bổ sung smoke harness cho updater: tải/ghép hợp lệ, hash sai, stream/part
>   lỗi và giữ nguyên `.partial` không thuộc updater; pass trên Windows.
>   Build3/4 Setup cài/gỡ silent pass; build3 mở app/shortcut, build4 có helper
>   updater mới và cài/gỡ thành công.
>   Latest GitHub release kiểm tra ngày 2026-11-06 vẫn là baseline Python 1.1.7;
>   còn QA máy sạch, IME/DPI đầy đủ sau cài và updater E2E với release Rust thật.
>
> **Tiến độ 2026-11-06 — artwork vai hát và hai lane:**
> - Đóng gói đủ chín role icon PNG của baseline vào `vietkara-render`; lượng tử
>   xuống 32 màu và cache hình học scanline. Preview GPUI và ASS xuất dùng chung
>   geometry, ASS tự chứa artwork nên không cần file ảnh cạnh project.
> - Inspector cho đổi biến thể màu của icon Nam, Nữ và Cùng hát. Legacy
>   `voice_c`/`voice_e` map trực tiếp Nam (lane 0), `voice_d`/`voice_f` map Nữ
>   (lane 1); test-only preview oracle cũng bỏ mô hình sáu slot.
> - Giới hạn dữ liệu lane trong editor ở Nam/Nữ: khi migrate project cũ, `row`
>   0–5 được gộp chẵn → lane 0, lẻ → lane 1; giá trị ngoài phạm vi cũ vẫn bị
>   từ chối. Validation project đã chuẩn hóa chỉ nhận 0/1.
> - `cargo fmt`, `cargo check --workspace` và `git diff --check` pass. Thêm
>   regression tests nhưng chưa chạy theo yêu cầu gom test cuối; cần kiểm tra
>   ảnh preview/export trên Windows. Commit `1dc84aa` đã push lên nhánh rewrite.
>
> **Tiến độ W6 — gợi ý phân vai hai lane:**
> - Nối app với worker JSONL `separate`/`role_suggest`; vocal được cache ngoài
>   project theo SHA-256 nguồn. Progress/cancel chạy nền; kết quả cũ bị loại nếu
>   project, lời hoặc audio đổi trong lúc phân tích.
> - Gợi ý chỉ áp dụng sau khi người dùng xác nhận; một Batch command đổi vai,
>   giữ nguyên từ khóa/câu đã gán vai thủ công, bật duet và tạo một bước undo.
>   Thêm test command/locked words và cache no-clobber. Workspace `cargo check`
>   pass; test và runtime model thật
>   chưa chạy theo yêu cầu gom cuối.
>
> **Tiến độ W6 — nhận dạng/căn lời trong Studio:**
> - Nút AI chọn `recognize` khi chưa có lời hoặc `align` khi đã có lời; cùng
>   dùng vocal cache theo fingerprint, progress/cancel và kiểm tra file nguồn.
> - Inspector có `align_region` cho câu được chọn; vùng âm thanh lấy thêm
>   1,2 giây ngữ cảnh, chỉ thay dòng đó và giữ nguyên các câu khác (kể cả câu
>   đang khóa). Kết quả căn toàn bài và căn một câu đều cần xác nhận.
> - Kết quả được chuẩn hóa ID/line UID, kiểm tra project trước khi hiển thị
>   xem trước; người dùng phải xác nhận mới đổi lời/mốc bằng một `SetLyrics`
>   undo step. Vai/track/các thuộc tính dòng hiện có được giữ theo line.
> - Test helper cho dữ liệu lời AI đã thêm; `cargo check --workspace --tests`
>   pass, chưa chạy suite theo yêu cầu gom test cuối.
>
> **Tiến độ W6 — batch render trong Studio:**
> - Toolbar cho chọn nhiều project JSON và tạo hàng đợi bền vững bằng
>   `vietkara-jobs::BatchRecord`; mỗi item đóng băng JSON cùng toàn bộ media
>   thuộc allowlist vào data root, render tuần tự ra file mới cạnh project,
>   không ghi đè file xuất có sẵn.
> - Tiến độ từng mục, hủy batch, lỗi theo từng project và resume batch sau
>   restart đã nối vào UI. Khi resume, project snapshot và từng media được
>   đối chiếu SHA-256; mục còn `running` sau crash được phục hồi về `queued`.
> - `cargo fmt --all` và `cargo check -p vietkara-app --tests` pass. Test
>   chưa chạy theo yêu cầu gom cuối; smoke test batch thực tế trên Windows
>   vẫn còn trước khi gọi batch production-ready.
>
> **Tiến độ W6 — cấu hình thiết bị/model AI:**
> - Preferences local cho chế độ tự động/CPU và model Whisper small/medium/
>   large-v3-turbo; worker tự chọn backend CUDA đã qua hardware probe, cache
>   recognition phân biệt CPU/CUDA và model root nằm trong app data.
> - Demucs chạy một job song song; model/cache không nằm trong project hay
>   payload đóng gói. Rust check/clippy và Python syntax check pass; kiểm tra
>   RAM/VRAM, tải model thật và fallback hết bộ nhớ còn chờ smoke test.
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
> - Preview GPUI lấy cửa sổ hiển thị theo thời gian lời và map beat/reference,
>   chọn câu mới nhất đang hát hoặc vừa hát cho mỗi lane karaoke; hỗ trợ lane
>   duet hai lane theo role/row, lấy màu theo role và cửa sổ một câu cho
>   subtitle. LyricsMV có stack/scroll lân cận. Đây mới là phần lựa chọn dòng
>   và tô chữ; preview chưa dùng chung layout ASS với renderer.
> - Tích hợp text input GPUI từ spike IME vào inspector; hỗ trợ chọn/sửa từ,
>   paste và phím điều hướng. Sửa chữ cập nhật `words` + `project.text` trong
>   một command undo/redo; đổi vai áp dụng cho toàn câu bằng một batch.
> - Kiểm tra sau cập nhật preview: Rust **81/81**, fmt/clippy/check/build pass;
>   Python **167/167**, Node **17/17** pass.
> - Preview hỗ trợ nền màu và ảnh `background.jpg`; import ảnh chuẩn hóa JPG,
>   tạo version snapshot và rollback nếu lưu project lỗi.
> - Bố cục ba cột và preview co theo logical viewport GPUI, giữ tỷ lệ 16:9/9:16;
>   tham khảo pattern Tabs/Toolbar/Resizable của GPUI Kit nhưng chưa thêm Kit
>   dependency vì version hiện tại yêu cầu snapshot GPUI mới hơn bản app đang ghim.
> - Inspector có điều khiển vị trí lời (trên/giữa/dưới) và căn lề (trái/giữa/phải,
>   hoặc đẩy hai lane karaoke ra hai bên), lưu qua Settings/undo/autosave.
> - Kiểm tra UI/media: Rust **86/86**, fmt/clippy/check/build pass; Python
>   **167/167**, Node **17/17** pass.
> - Kiểm tra lát trước: Rust **78/78**, fmt/clippy/check/build pass; Python **167/167**,
>   Node **17/17** pass. App khởi chạy với fixture schema 2. Accessibility
>   tree của cửa sổ GPUI không đọc được trong công cụ desktop, nên thao tác gõ
>   IME trực tiếp trên input sản phẩm vẫn cần kiểm tra thủ công.

> **Tiến độ 2026-11-06 — nhập và preview nền video:**
> - Thêm nhập MP4/MOV/MKV/WebM/AVI/M4V: lưu nguồn trong project, tạo proxy
>   H.264 960px/24fps, trích frame đầu, lưu metadata và checkpoint phiên bản.
>   Publish file có rollback nếu lưu project thất bại.
> - Preview GPUI cập nhật frame từ proxy theo playback position (mỗi 0,5 giây)
>   trong khi phát hoặc tua; nếu project cũ thiếu proxy thì fallback về nguồn.
>   Frame extraction không dùng audio nền; soundtrack vẫn theo beat. Cache frame
>   tạm giới hạn 16 ảnh.
> - `cargo test --workspace` **88/88**, workspace fmt/clippy/check và app build
>   pass. Chưa có phát video liên tục, original-video source hoặc layout ASS
>   dùng chung.

> **Tiến độ 2026-11-06 — renderer nhận nền project:**
> - `RenderInputs::configure_background_from_project` đọc `background_type`
>   và resolve an toàn file nền image/video/original trong project; thiếu file
>   bắt buộc hoặc đường dẫn traversal sẽ trả lỗi rõ ràng.
> - Builder FFmpeg lặp nền tĩnh hoặc video; audio vẫn map từ beat và vocal
>   hướng dẫn vẫn `amix`. Bộ test xác minh map/index cho từng loại nền.
> - Renderer này hiện có thể dựng khung từ nền project nhưng GPUI chưa có
>   pipeline/job/UI export hoàn chỉnh; scene/logo/countdown và ASS layout
>   parity vẫn đang mở.
> - Tiếp theo đã port timeline scene theo section + anchors, resolve ảnh
>   `scene_<id>.png`, ghép opacity/fade trước ASS và giữ soundtrack beat.
>   Integration test tạo ảnh/video thật bằng FFmpeg và xác minh output.
>   Workspace hiện **94/94**; fmt/clippy/check/build pass.
> - Port tiếp keyframe đầu/cuối cho pan, zoom và alpha của scene image; test
>   integration chạy FFmpeg với chuyển động và alpha thay đổi theo thời gian.
> - Nối thêm ảnh decoration của scene, cùng timeline, vị trí/alpha keyframe
>   và fade. Scale decoration chạy qua canvas có kích thước cố định và `zoompan`
>   để tránh crash của `scale=eval=frame` trên FFmpeg bundled Windows; test
>   render thật xác nhận zoom/opacity/position keyframe cùng hoạt động.
> - Nối logo theo kích thước/tọa độ/độ mờ và hiệu ứng none/fade/pulse/float;
>   nối custom countdown PNG theo khoảng nghỉ lời, slots và vị trí khung.
>   Sửa filter graph để scene/compositing nằm dưới ASS một lần duy nhất.
>   Integration test FFmpeg dựng scene + decoration + countdown + logo pulse
>   và float. Workspace **95/95**; fmt/clippy/check/build pass. Countdown dạng
>   glyph/ASS và nút/job export GPUI vẫn chưa có.
> - Đồng bộ invariant hai lane giữa preview GPUI và renderer: role `voice_c`…
>   `voice_f` cùng mọi `row` legacy được gập vào slot 0/1; karaoke luôn dùng
>   hai vị trí dọc. Thêm test bao phủ cả role lẫn row ngoài phạm vi 0/1.
> - Bắt đầu port lyric layout vào `vietkara-render::layout`: plan dùng một
>   `TextMeasurer` được inject để tính wrap/tọa độ từ cùng font metrics, map
>   beat anchors, show windows, alignment và word positions; invariant hai
>   lane được giữ. GPUI preview karaoke đã gọi planner, đo glyph tiếng Việt
>   bằng `WindowTextSystem::shape_line`, cache plan theo nội dung/settings và
>   đặt từng từ theo tọa độ/cỡ chữ trong canvas. `ass::build_lyrics_ass` và
>   `RenderInputs::prepare_lyrics_ass` đã nhận cùng plan, xuất row karaoke với
>   sweep từng từ hoặc subtitle row đơn; writer chỉ tạo file mới, không ghi đè.
>   Integration test dựng thật bằng FFmpeg xác nhận ASS hợp lệ. Chưa nối writer
>   vào action export GPUI; title/marker/gradient/countdown/readability backing
>   chưa port. Workspace **104/104**, fmt/clippy/check/build app đều pass.
> - Nối action **Xuất video** vào pipeline: chụp project + measured display plan
>   trên UI thread, chọn file đích, lấy beat và media project, viết ASS vào
>   thư mục tạm cạnh file đích, gọi FFmpeg ngoài UI thread, đọc tiến độ, cho
>   phép hủy, xác minh bằng FFprobe rồi publish không ghi đè (hard link khi
>   filesystem hỗ trợ, exclusive create + copy trên volume không hỗ trợ).
>   Thêm Ctrl+Shift+E và báo lỗi FFmpeg theo log cuối. Test tích hợp của app đã
>   dựng MP4 thật với lời “Xin chào”, xác minh audio/video/size/duration bằng
>   FFprobe, kiểm tra tiến độ và xác nhận file đích cũ không bị ghi đè.
>   Workspace **106/106**, fmt/clippy/check/build app đều pass. Chưa kiểm tra
>   thao tác nút trên cửa sổ GPUI; title/marker/gradient/readability backing,
>   glyph countdown và đóng gói font vẫn chưa đạt parity.
> - Port audio normalization theo đúng trailer Python `-af
>   loudnorm=I=-16:TP=-1.5:LRA=11`; mặc định vẫn tắt, khi bật áp dụng lên
>   soundtrack sau bước mix guide vocal. Thêm test argv cho setting opt-in và
>   chạy FFmpeg thật với filter bật trong smoke test export.
> - Port countdown vector `mic/star/heart/note/moon` vào ASS, gồm thời điểm
>   nghỉ, vị trí, cỡ và bốn icon thu dần theo logic Python. Test kiểm tra lịch
>   sự kiện và render ASS có icon thật qua FFmpeg. Workspace **108/108**;
>   fmt/clippy/check/build app đều pass. Title cards, role artwork, gradients,
>   readability backing và bundled fonts vẫn còn thiếu để đạt parity.
> - Đồng bộ kích thước layout ASS với aspect typed của project: portrait dùng
>   PlayRes 1080×1920 thay vì đọc nhầm field đã được domain parser tiêu thụ.
> - Port gradient chữ đã hát theo 10 dải clip và hộp nền bo góc cho subtitle,
>   đọc đúng màu/opacity/radius từ settings. Test xác minh dải màu và layer
>   backing; integration render FFmpeg chạy gradient cùng countdown vector.
>   Workspace **109/109**; title cards, role artwork, readability backing
>   theo scene và bundled fonts vẫn còn thiếu.
> - Port title-card measured layout vào `DisplayPlan`: title/credit, time window,
>   font/weight/italic, safe-area position, sáu template, và geometry box dùng
>   chung cho preview/export. ASS tạo 24 dải gradient ngang, viền bo góc và
>   hai event riêng cho entrance/exit (`fade`, slide, zoom, flip); GPUI preview
>   hiển thị các dòng title trong khoảng thời gian đã cấu hình. Title ASS có
>   unit test xác minh text, timing, template và motion; crate/app clippy,
>   workspace check và app build pass. Toàn bộ FFmpeg integration/workspace
>   suite chưa chạy được trong shell này do thiếu `ffmpeg.exe`/`ffprobe.exe`;
>   template/effect của title trong GPUI preview chưa vẽ đầy đủ.
> - Port scene readability backing: đọc percentile 80 grayscale ở thumbnail
>   48×27, ngưỡng sáng 0.64, opacity auto 44% / always 48%; mode off và subtitle
>   có box không thêm backing. ASS backing theo show window của row và scene
>   active tại `row.start`; preview dùng cùng estimator theo section scene.
>   `vietkara-render` không chạy 37 tests không cần FFmpeg (2 integration bị
>   loại do binary chưa có), clippy workspace và app build pass.
> - Port fallback vector role marker cho male/female/all: planner dành chỗ
>   trước khi wrap, gắn marker ở lần role xuất hiện/đổi role, lấy màu từ role
>   settings, ASS vẽ silhouette bằng drawing path và GPUI preview dùng marker
>   cùng vị trí/màu. Tất cả voice role vẫn gập vào hai lane 0/1. Render tests
>   37/37 pass khi bỏ 2 FFmpeg integration; app tests 7/8 (test export thực
>   cần `ffmpeg.exe`), workspace clippy và app build pass. PNG palette gốc chưa
>   được đóng gói; Rust đang dùng vector fallback tương thích baseline.
> - Hoàn thiện title preview GPUI: vẽ 24 dải gradient của template, viền bo
>   góc và opacity; title text dùng màu/font settings, chuyển động fade/slide/
>   zoom/flip được nội suy theo thời gian playback giống ASS. Test riêng kiểm
>   tra pha vào/giữ/ra; app clippy và build pass.
> - Port mã hoá font legacy VNI/TCVN3 theo bảng baseline, NFC-normalize input
>   trước khi map, áp dụng đồng nhất khi đo layout, GPUI preview, title và ASS
>   export; Unicode gốc vẫn được giữ trong Project. Tests cho VNI, TCVN3, chữ
>   hoa và input decomposed pass; lookup tables được cache một lần để không
>   tạo bảng mới mỗi lần đo glyph. Workspace clippy và app build pass.
> - Thêm nhóm TITLE CARD vào inspector GPUI: bật/tắt, lần bật đầu lấy tên
>   project nếu title trống, sửa nội dung title bằng GPUI text input, chọn
>   template và hiệu ứng vào/ra. Settings đi qua `SetSettings`, giữ
>   dirty/autosave hiện có. Bổ sung credit/nghệ sĩ, cỡ chữ, vị trí dọc,
>   khoảng thời gian, và palette màu title; text inputs riêng cho title/credit
>   đồng bộ lại khi settings thay đổi. App fmt/check/Clippy sạch, unit tests
>   **8/8** pass (integration export cần FFmpeg được bỏ qua).
> - Inspector bổ sung mode solo/duet, bật/tắt role markers cho duet và chọn
>   countdown vector (mic/star/heart/note/moon) cùng thời lượng 1–8 giây. Các
>   giá trị dùng settings renderer sẵn có; app check/Clippy sạch và unit tests
>   **8/8** pass.
> - Thêm import logo PNG vào project với snapshot version, bật setting logo và
>   rollback file/project nếu lưu lỗi; GPUI preview hiển thị đúng vị trí/kích
>   thước/opacity và fade/pulse/float. Inspector có bật/tắt, size, XY, opacity
>   và effect. Test PNG thật + snapshot và test từ chối file không phải PNG
>   **2/2** pass.
> - Thêm import ảnh countdown PNG với version snapshot/rollback, điều khiển
>   biểu tượng vector hoặc ảnh riêng trong inspector và preview count-in theo
>   thời gian đầu bài. Test conversion PNG + metadata/snapshot và từ chối file
>   không hợp lệ **2/2** pass; app unit tests **8/8**, workspace check/Clippy
>   sạch.
> - Inspector/preview hỗ trợ subtitle backing: bật/tắt, chọn màu, opacity,
>   radius; GPUI chỉ vẽ trong subtitle mode và dùng đúng các settings ASS.
>   App fmt/check/Clippy sạch, unit tests **8/8** pass.
> - Đồng bộ count-in preview với lịch renderer: grouped line timings, opening
>   countdown và countdown sau khoảng nghỉ từ 8 giây; hiển thị đúng số slot
>   còn lại và hỗ trợ PNG tùy chỉnh. Thêm regression test cho cửa sổ đầu bài,
>   khoảng nghỉ và thời điểm không hoạt động; app unit tests **9/9** pass.
> - Thêm nhóm cài đặt export trong inspector: MP4/MOV/MKV, 1080p/4K,
>   quality, FPS, beat/guide vocal và audio normalization. Các lựa chọn cập
>   nhật typed Settings bằng `SetSettings` để giữ undo/autosave và được đọc
>   trực tiếp bởi `RenderProfile::from_project`. Workspace check/Clippy pass;
>   app **9/9**, domain **20/20**, jobs **5/5**, storage **19/19**, media
>   non-FFmpeg **11/11** và render non-FFmpeg **39/39** pass. Full FFmpeg
>   integrations chưa chạy được vì `ffmpeg.exe`/`ffprobe.exe` không có trong PATH.
> - Inspector tab Từ có metadata section cho dòng đang chọn: đặt/bỏ biên
>   section, đặt hoặc xóa tên section áp dụng cho cả dòng. Tên giới hạn theo
>   domain 40 ký tự; thao tác dùng `SetLyrics`, giữ undo/redo và autosave, đồng
>   thời tái tạo `project.text` để marker dòng trống khớp state. App **10/10**,
>   workspace check/Clippy pass.
> - Bổ sung chọn 7 loại section scene chuẩn và nhập/thay ảnh PNG/JPG/WebP vào
>   project dưới dạng PNG, có snapshot trước thay đổi và rollback khi lưu lỗi.
>   Thay ảnh giữ nguyên transition/keyframe/decoration đã cấu hình; GPUI preview
>   nạp ảnh scene theo section ở playback position và opacity. Storage tests
>   cho import/replacement/giữ settings và từ chối section lạ pass; workspace
>   check/Clippy sạch.
> - Nối artwork scene: nạp/thay/xóa ảnh decoration PNG/JPG/WebP, giữ file và
>   metadata theo scene ID, tạo version snapshot, rollback khi thao tác lưu lỗi.
>   Preview GPUI xếp lớp decoration trên scene image, nội suy keyframe ảnh nền
>   và decoration theo playback, crossfade đồng thời các section liền kề theo
>   cùng timeline overlap của renderer trên nền màu/ảnh/video, áp fade in/out
>   theo transition duration; video preview lấy frame từ proxy an toàn, fallback
>   về nguồn nếu thiếu proxy;
>   inspector chỉnh opacity, transition cut/fade/duration, contrast mode, size
>   và keyframe riêng đầu/cuối cho ảnh nền lẫn decoration. Regression tests
>   interpolation, fade, scene crossfade và ưu tiên proxy/fallback/path traversal
>   pass. Storage **19/19**, app **14/14** (bỏ qua
>   integration export phụ thuộc FFmpeg app packaging); workspace check/Clippy
>   sạch. Video background vẫn là ảnh frame cập nhật theo bước 0,5 giây, chưa
>   phát liên tục trong canvas preview.
> - Đối chiếu GPUI preview với baseline Python cho LyricsMV: ASS renderer Rust
>   trước đó bỏ qua `mv_style` và dùng màu chưa hát cho cả câu chính lẫn dòng
>   phụ. Đã port `single`/`stack`/`scroll`, geometry dòng phụ, alpha/outline,
>   màu câu chính và gradient; regression test xác minh số event và opacity.
>   `vietkara-render` **40/40** test không cần FFmpeg pass (2 integration render
>   thật đã lọc do thiếu binary); workspace test khả dụng và clippy pass.
> - Port hiệu ứng sweep karaoke từ Python vào ASS: unsung base clip chạy theo
>   start/end của từng từ, lớp shadow, feather 4 dải và glow mask tùy chọn;
>   thêm điều khiển `sweep_feather` / `sweep_glow` trong inspector. Regression
>   test kiểm tra timing, layer count, alpha và blur. `vietkara-render` **41/41**
>   test non-FFmpeg, `vietkara-app` **14/14**, fmt/check/workspace clippy pass.
> - Đồng bộ thêm GPUI karaoke preview: màu hát được clip theo tiến độ bên trong
>   từ, feather dựng từ 4 dải alpha và glow band bám theo mép; tỉ lệ hình học
>   scale theo preview viewport. Thêm test cho fill, segment, opacity và glow.
>   Workspace test khả dụng pass; 6 test tích hợp cần FFmpeg/FFprobe tiếp tục
>   được lọc do binary không có trên PATH. Làm mềm glow bằng 9 lớp glyph trắng
>   alpha thấp lệch quanh mép trong clipping band; GPUI 0.2.2 chưa có blur glyph
>   trực tiếp nên đây là xấp xỉ raster qua các lớp, không phải Gaussian blur.
> - Thước timeline GPUI đã có thể click để seek beat tới từng mốc thời gian; thao
>   tác gửi lệnh seek cho playback worker, cập nhật playhead/preview ngay, giới
>   hạn trong duration và bỏ qua khi đang căn nhịp hoặc kéo mốc. Regression test
>   kiểm tra clamp và giá trị không hữu hạn; app check và test mục tiêu pass.
> - Playback transport cho phép chọn nghe beat hoặc bản hát; đổi nguồn sẽ khởi
>   tạo worker với `beat.wav` / `reference.wav` tương ứng, và nhập audio mới sẽ
>   chuyển sang track vừa nhập. Beat vẫn là clock chuẩn của preview; vị trí và
>   duration từ reference được đổi qua anchors, chiều ngược lại dùng khi seek
>   timeline/chọn từ/lùi tap. Regression test kiểm tra ánh xạ track và round-trip
>   với anchors có drift.
> - Waveform reference được ánh xạ từng bin lên trục beat theo anchors và cắt
>   phần trước mốc 0; độ dài timeline lấy max giữa duration beat/reference và
>   lời. Regression test kiểm tra khoảng bin sau drift, clipping và beat giữ
>   nguyên tọa độ.
> - Timeline cho phép thêm anchor tại playhead và kéo anchor trên trục beat.
>   Preview waveform/lời cập nhật tạm trong lúc kéo; khi thả, `SetAnchors` đi
>   qua editor transaction nên giữ validation, undo/redo và autosave hiện có.
>   Test kiểm tra vị trí reference được nội suy và từ chối anchor gần trùng.
>   Đường kéo kiểm tra riêng ràng buộc anchor để tránh clone/validate toàn bộ
>   project ở mỗi mouse-move; domain validation đầy đủ vẫn chạy một lần khi thả.
> - Anchor trên timeline có trạng thái chọn riêng; có thể xóa anchor được chọn
>   qua `SetAnchors`, giữ undo/redo và autosave. Thêm anchor mới sẽ tự chọn nó;
>   đổi project hoặc undo/redo sẽ xóa trạng thái chọn cũ để tránh nhầm index.
>   Inspector timeline hiển thị rõ thời gian Ref ↔ Beat của anchor đang chọn.
> - Thêm action tự căn anchor bằng tương quan Pearson giữa envelope waveform
>   Ref/Beat trong cửa sổ ±350 ms, tìm shift trong ±500 ms. Chỉ áp dụng khi
>   tương quan đạt 0.45 và cải thiện ít nhất 0.08 so với vị trí hiện tại; dữ
>   liệu phẳng/thiếu hoặc thứ tự anchors sai sẽ không bị sửa. Action dùng
>   `SetAnchors`, hiển thị confidence/shift và undo được; test waveform giả lập
>   shift 150 ms cùng trường hợp envelope phẳng.

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
  batch queue state machine; batch render đã nối vào Studio, còn QA resume/
  cancel trên project/media thực tế cần hoàn tất.
- `crates/vietkara-media`: FFprobe/fallback, WAV reader, waveform/metrics,
  playback transport và output device; kiểm chứng thiết bị/audio runtime trên
  các cấu hình Windows mục tiêu vẫn còn.
- `crates/vietkara-render`: shared display plan, ASS lyrics/title/scene,
  background/overlay/audio graph, render profile, FFmpeg argv và output
  verification; có raster golden đơn giản 6 khung hình so với Python baseline
  `c8dae55`; full layout/font parity và QA full render Windows còn mở.
- `crates/vietkara-ai-bridge`: JSON Lines process bridge với timeout, cancel
  và crash handling; `ai_worker.py` adapter đã nối recognition/alignment,
  separation, role suggestion và sync; model thật, phần cứng CPU/NVIDIA và
  cancellation runtime vẫn cần nghiệm thu.
- `crates/vietkara-app`: GPUI 0.2.2 Studio có project lifecycle, undo/redo,
  autosave/recovery, audio/tap/timeline, lyrics/AI, preview/export, batch,
  presets/fonts/scenes và diagnostics. W7 packaging/updater code đã có;
  release artifacts cần rebuild, test tổng và QA Windows sạch còn mở.

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
  đã có. Settings core đã typed, còn các field ít dùng/đặc thù Python trong
  `settings.extra`; `reference`, `beat`, `alignment`, `scenes` giữ JSON linh
  hoạt để không làm mất field chưa hiểu.
- `vietkara-storage` đã có atomic save, manifest media, SHA-256, giới hạn
  ZIP 3 GiB/2.000 entry, project JSON 20 MiB, allowlist tên media và restore
  an toàn theo verify → stage → swap/rollback. Các crate storage tests
  xác minh fixture; thao tác UI và kiểm chứng path Unicode trên Windows
  còn mở.
- Giữ khác biệt an toàn với Python: `restore_version` stage và xác minh
  fingerprint trước khi swap; bundle import kiểm tra trong staging trước
  khi đưa media vào project.
- Settings core đã typed; các field ít dùng/đặc thù Python tiếp tục nằm trong
  `extra` cho round-trip. `reference`, `beat`, `alignment` và dữ liệu scene vẫn
  giữ JSON linh hoạt để bảo toàn field chưa hiểu.
- Luồng UI W3–W7 đã được nối thành phần lớn thao tác người dùng; các rủi ro
  còn lại tập trung ở test tổng, runtime/hardware Windows, golden parity,
  clean-machine packaging, updater E2E và rollout có rollback.
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
