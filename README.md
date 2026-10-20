# VietKara Studio

Studio karaoke chạy cục bộ trên Windows 10/11 x64. Dùng **beat gốc** cho video xuất; vocal tách riêng phục vụ nhận dạng và căn lời.

## Tải bản 1.0.0

Mở [Releases](https://github.com/NDCLI/VietKaraStudio/releases/latest), chọn Setup hoặc Portable. Mỗi gói gồm hai phần do giới hạn dung lượng của GitHub. Tải đủ hai phần và công cụ ghép; xem [hướng dẫn tải](docs/RELEASE.md).

Đã kèm Python, FFmpeg và runtime NVIDIA CUDA. Model AI tải riêng trong Cài đặt. Cần Edge WebView2 Runtime. Bản phát hành chưa ký số.

## Tính năng

- Nhập audio/video, beat gốc, SRT hoặc lời riêng; tự nhận lời và căn thời gian từng từ.
- Tự ưu tiên GPU, có CPU dự phòng; tải model chủ động và dùng lại cache.
- Đồng bộ thủ công bằng gõ/giữ Space, nghe trước 15 giây từ điểm chọn; chỉnh bổ sung trên timeline hai hàng.
- Timeline có zoom bằng cuộn chuột, tự chạy theo nhạc, nhãn màu và kéo mép liên kết/độc lập.
- Tự chia dòng; preset cỡ chữ 110, font tiếng Việt, viền/bóng, hiệu ứng quét màu và song ca.
- Video/ảnh/màu nền, đếm ngược đầu bài hoặc sau nhạc dạo, logo và hiệu ứng logo.
- Xuất MP4/MOV/MKV, chọn thư mục, fps, chất lượng, FHD/4K; kèm SRT/LRC/ASS và thông tin render.
- Tách vocal/beat thành WAV riêng. Gợi ý phân vai song ca dựa trên cao độ, cần nghe và sửa khi cần.

## Chạy từ mã nguồn

Cài Python 3.12 x64 và FFmpeg/FFprobe (trong `bin/` hoặc PATH), sau đó chạy `Start.cmd`. Launcher chuẩn bị môi trường Python và mở cửa sổ app. Dùng `.venv/Scripts/python.exe launch.py --browser` để mở bằng trình duyệt.

GPU NVIDIA dùng PyTorch CUDA cho Demucs, CTranslate2 cho Whisper và NVENC khi xuất video. Xem `requirements-gpu.txt` và `configure_gpu.py`; driver phải tương thích. Một số bước xử lý vẫn chạy CPU.

## Dữ liệu và model

- Chạy nguồn/portable: `data/` cạnh app. Bản Setup: `%LOCALAPPDATA%/VietKaraStudio`.
- Whisper tải vào `data/models/whisper/<model>/`; Demucs dùng `data/models/` và cache quản lý `data/cache/`.
- Model đã có trong cache người dùng được dùng lại; có thể chép model thủ công theo đường dẫn trong Cài đặt.
- Project tự lưu; bản `.obs.json` không chứa media, cần nạp lại nguồn khi chuyển máy.
- Mã nguồn không chứa dự án cá nhân, bài hát, video, model hoặc runtime đóng gói.

## Kiểm thử và đóng gói

```powershell
.venv/Scripts/python.exe -m pytest tests -q
node --test tests/tapping.test.js tests/timeline_edit.test.js
```

Xem [hướng dẫn build](docs/DISTRIBUTION.md) và [thông tin bản phát hành](docs/RELEASE.md). AI có thể căn sai; nghe và kiểm tra trước khi xuất cả bài.
