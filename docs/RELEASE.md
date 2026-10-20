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
