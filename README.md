# VietKara Studio

Studio karaoke chạy cục bộ trên Windows 10/11 x64. Dùng **beat gốc** cho video xuất; vocal tách riêng phục vụ nhận dạng và căn lời.

## Bản cục bộ 1.1.1

Setup Nuitka x64 đã được build tại `releases/VietKaraStudio-1.1.1-Nuitka-Setup-x64.exe`; đây là bản cài, không phải portable. Bản này có mục tự kiểm tra và tải gói vá nhỏ từ GitHub Releases, giữ nguyên project, nhạc và model. Bản Setup 1.1.1 chưa được phát hành trên GitHub.

Gói Setup mang theo ứng dụng, Python, FFmpeg và runtime NVIDIA CUDA. Model AI tải riêng trong Cài đặt. Setup kiểm tra và cài Edge WebView2 Runtime nếu thiếu (cần Internet). Bản phát hành chưa ký số.

## Tính năng

- Nhập audio/video, beat gốc, SRT hoặc lời riêng; tự nhận lời và căn thời gian từng từ.
- Tự ưu tiên GPU, có CPU dự phòng; tải model chủ động và dùng lại cache.
- Đồng bộ thủ công bằng gõ/giữ Space, nghe trước 15 giây từ điểm chọn; chỉnh bổ sung trên timeline hai hàng.
- Timeline có zoom bằng cuộn chuột, tự chạy theo nhạc, nhãn màu và kéo mép liên kết/độc lập.
- Tự chia dòng; preset cỡ chữ 110, font tiếng Việt, viền/bóng, hiệu ứng quét màu và song ca.
- Video/ảnh/màu nền, đếm ngược đầu bài hoặc sau nhạc dạo, logo và hiệu ứng logo.
- Xuất MP4/MOV/MKV, chọn thư mục, fps, chất lượng, FHD/4K; kèm SRT/LRC/ASS và thông tin render.
- Tách vocal/beat thành WAV riêng. Gợi ý phân vai song ca dựa trên cao độ, cần nghe và sửa khi cần.
- Chỉnh bố cục nhiều tính năng, chọn video làm nguồn, tiêu đề căn trái có nền ôm sát chữ; cập nhật trạng thái tác vụ bằng câu dễ hiểu.
- Tự kiểm tra và tải bản vá từ GitHub Releases; khi đổi runtime AI/Python thì vẫn cần Setup đầy đủ.

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

## Thay đổi 1.0.1

- Gợi ý phân vai theo từng câu khi cao độ các giọng chồng lấn; câu chưa rõ được giữ nguyên. Đây là gợi ý theo dải giọng, cần nghe và sửa lại.
- Biểu tượng nam/nữ/cùng hát dạng vector ở đầu câu trong preview và video karaoke song ca; bật/tắt trong Vai & màu song ca.
- Giữ kết quả kiểm tra GPU trong phiên app, bỏ quét khi chọn CPU hoặc đã có vocal cache.
- Chặn bộ hiển thị MSHTML cũ, bổ sung CSS dự phòng và màu danh sách chọn; dùng icon/định danh VietKara cho cửa sổ.

Bản 1.0.1 đã kiểm trên máy build và runtime đóng gói; chưa xác nhận trên máy Windows 10 sạch. Model, nhạc và project cá nhân không được đưa vào bộ cài.

## Thay đổi 1.0.2

- Giữ lớp bóng chữ riêng suốt thời gian hiển thị câu, kể cả sau khi từ đã hát xong.
- Chọn Beat của bạn / Bản có vocal trên banner; điều khiển nghe/tắt tiếng tại ba hàng sóng Beat, bản gốc, Vocal riêng.
- Tách video ngay trong Karaoke, áp dụng beat, âm thanh gốc và vocal vào cùng project; xác nhận trước khi thay nguồn và mốc cũ.
- Tiêu đề bài hát và ca sĩ/tác giả: font, màu, cỡ, vị trí, thời gian, 8 lựa chọn hiệu ứng vào/ra độc lập. Các mẫu màu/font dựa trên preset VCL có sẵn; chuyển động tiêu đề được triển khai trong VietKara.
- Bản cập nhật nhẹ chứa mã/tài nguyên/launcher; dùng thư viện AI/FFmpeg của bản portable 1.0.x hiện có. Đóng app rồi giải nén đè vào đúng thư mục portable, không xóa data/.
