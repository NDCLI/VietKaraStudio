# VietKara Studio 1.0.0

Ứng dụng karaoke cho Windows 10/11 x64: beat gốc, nhận dạng/căn lời với GPU, đồng bộ bằng Space hoặc timeline, preset chữ, song ca, logo và xuất video MP4/MOV/MKV qua FFmpeg.

## Tải và chạy

GitHub giới hạn mỗi tệp release dưới 2 GiB. Mỗi gói Setup và Portable được chia thành `.001` và `.002`.

1. Chọn **Setup** hoặc **Portable**, tải đủ hai phần của gói đó.
2. Tải thêm `downloads.json`, `Ghep-File.ps1` và `Ghep-File.cmd` vào cùng thư mục.
3. Chạy `Ghep-File.cmd`. Công cụ kiểm SHA-256 từng phần và tệp ghép, tạo lại EXE/ZIP nguyên vẹn. Cần khoảng 6 GB dung lượng trống cho mỗi gói.
4. Setup: chạy EXE. Portable: giải nén ZIP rồi chạy `VietKaraStudio.exe`.

Đã kèm Python, FFmpeg và runtime GPU NVIDIA; model AI tải riêng trong Cài đặt. Cần Edge WebView2 Runtime. GPU không phù hợp sẽ dùng CPU.

## Kiểm chứng

44 kiểm thử Python và 14 kiểm thử JavaScript đạt. Đã kiểm launcher, bộ cài/gỡ cài, portable, Whisper CUDA float16, render FFmpeg/libass/NVENC và tính toàn vẹn ZIP trên máy build. Chưa kiểm trên Windows VM sạch; bản phát hành chưa ký số.

Không kèm bài hát, video, dự án cá nhân hay model cache. Dữ liệu portable ở `data/` cạnh app; bản Setup lưu tại `%LOCALAPPDATA%/VietKaraStudio`.

## Cập nhật bản cài Nuitka qua GitHub

Bản Setup Nuitka có thể nhận gói vá nhỏ qua **Cài đặt → Cập nhật VietKara Studio**. Mặc định app kiểm tra khi mở và tự tải bản vá nếu có; người dùng chọn **Cài đặt và khởi động lại** để áp dụng. Tệp project, nhạc, model, cache AI, FFmpeg và thư viện AI không nằm trong gói vá.

Để phát hành bản vá chỉ gồm EXE, giao diện và tài nguyên:

1. Tăng cùng phiên bản trong `app_runtime.py`, `packaging/build_nuitka.py` và `packaging/VietKaraStudio-Nuitka.iss`.
2. Chạy `python packaging/build_nuitka.py compile`, sau đó `python packaging/build_nuitka.py assemble`.
3. Chạy `python packaging/build_nuitka_patch.py`. Lệnh so runtime hiện tại với mốc Setup đầy đủ gần nhất và từ chối tạo patch nếu thư viện AI/Python khác.
4. Tạo GitHub Release có tag `v<phiên-bản>` rồi tải lên cả `VietKaraStudio-<phiên-bản>-Nuitka-Patch.zip` và tệp `.sha256` đi kèm. App chỉ nhận đúng tên gói này từ release mới nhất.

Khi đổi Python, Nuitka hoặc thư viện AI, phát hành Setup đầy đủ. Tạo Setup đầy đủ bằng `python packaging/build_nuitka.py all`; thao tác đó cập nhật mốc tương thích patch cho các bản vá sau.

## Rust/GPUI preview

Bộ cài Rust/GPUI dùng AppId và thư mục riêng để cài song song với bản Python.
Updater trong bản Setup chỉ nhận stable GitHub Release có tag `vMAJOR.MINOR.PATCH`,
`downloads.json` và đúng asset `VietKaraStudio-Rust-<version>-Setup-x64.exe`.
Updater tải từng part qua HTTPS, xác minh SHA-256, ghép bộ cài rồi mới chạy Setup;
portable ZIP cập nhật thủ công bằng cách tải ZIP mới và thay thư mục app, giữ
`data/` riêng.

Sau khi hoàn tất kiểm thử và tạo cùng phiên bản stage/Setup, chia artifact vượt
giới hạn GitHub bằng:

```powershell
python packaging/split_rust_release.py `
  releases/VietKaraStudio-Rust-<version>-Setup-x64.exe `
  releases/VietKaraStudio-Rust-Portable-x64.zip
```

Tải toàn bộ `.001…`, `downloads.json` và `SHA256SUMS.txt` lên cùng GitHub Release.
Thư mục output của `split_rust_release.py` cũng tạo sẵn `Ghep-File.ps1`,
`Ghep-File.cmd` và `HUONG-DAN.txt`; tải thêm các tệp này để người dùng có thể
ghép và kiểm tra artifact trực tiếp trên Windows.
Không đưa thư mục `data/`, model hoặc cache cá nhân vào artifact. Build portable
và installer cần cùng phiên bản; kiểm `BUILD-INFO.json` trước khi upload.
Updater Rust compile được nhưng chưa kiểm download thật vì chưa có Rust Release
trên GitHub. Local build4 trong `releases/` có sửa cleanup file tải dở và đã
qua smoke install/uninstall; hash và giới hạn kiểm chứng được ghi trong
`docs/windows-plan.md`. Đây là snapshot kiểm thử, chưa phải artifact phát hành
GitHub; trước khi public cần hoàn tất QA máy sạch và kiểm tra update E2E với
release thật.
