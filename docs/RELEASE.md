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
