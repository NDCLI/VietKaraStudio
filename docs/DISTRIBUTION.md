# VietKara Studio 1.0.1 — đóng gói Windows

Hai gói ở `releases/`: Setup x64 một file EXE và ZIP portable. Có Python 3.12, FFmpeg/FFprobe, thư viện GPU CUDA 12.4 và launcher .NET; model AI tải riêng. Yêu cầu Windows 10/11 x64 và Edge WebView2 Runtime. Chưa ký số.

- Portable: giải nén toàn bộ, chạy VietKaraStudio.exe, dữ liệu ở data/ cạnh app.
- Setup: cài theo tài khoản người dùng, dữ liệu ở %LOCALAPPDATA%/VietKaraStudio; uninstall không xóa thư mục dữ liệu.
- Runtime sử dụng python312._pth để không đọc thư viện Python bên ngoài. Server production là Waitress, chỉ nghe 127.0.0.1, cổng tự chọn.
- Có log quay vòng, icon, khóa một instance launcher; CPU fallback khi GPU không dùng được.
- Không đưa data/, VCL_data/, cache model, file nhạc, project, logo riêng hay bài test vào payload.

Build lại từ workspace:

1. .venv/Scripts/python.exe packaging/build_release.py
2. .venv/Scripts/python.exe packaging/smoke_release.py releases/VietKaraStudio-1.0.1-Portable-x64 --report releases/portable-smoke.json
3. .venv/Scripts/python.exe packaging/build_release.py --artifacts

Compiler dùng Inno Setup 6.7.3 trong build-tools/InnoSetup; launcher biên dịch bằng csc của .NET Framework. Không cần cài PyInstaller. requirements-lock.txt ghi môi trường build. runtime/ chứa license Python và metadata/license của thư viện; app/bin/FFMPEG-LICENSE.txt chứa thông tin FFmpeg.

Kiểm tra thực tế trên máy build: chạy runtime từ thư mục ngoài với dữ liệu tạm sạch; kiểm sys.path chỉ trỏ nội bộ; GPU torch/CTranslate2; API/font/layout/model; render FFmpeg/libass và xuất sidecar đúng thư mục. Nạp Whisper small từ cache hiện có và thực thi nhận dạng trên CUDA float16, không tải thêm model. File launcher EXE đã được kiểm khởi động server. Báo cáo JSON trong releases/.

Phạm vi: chưa có máy Windows sạch/VM để kiểm ma trận driver và mọi bản Windows. Không tuyên bố đã thử trên máy khác. Các báo cáo và SHA256SUMS cho phép kiểm lại payload nhận được.


Bản 1.0.1 kèm Microsoft WebView2 Evergreen Bootstrapper có chữ ký Microsoft. Setup kiểm tra Runtime trong registry và cài prerequisite nếu thiếu; cần Internet ở bước này. Portable cung cấp dependencies/MicrosoftEdgeWebview2Setup.exe để cài riêng. Build kiểm chữ ký và ghi SHA-256 vào BUILD-INFO.json; không chạy bộ cài prerequisite trên máy build. Nguồn chính thức: https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution

Ứng dụng kiểm bộ hiển thị thực tế trước khi mở cửa sổ và từ chối MSHTML. resources/VietKaraStudio.ico dùng cho cửa sổ; AppUserModelID VietKaraStudio.Desktop khớp shortcut Setup.

Bộ kiểm thử 1.0.1: 77 Python test, 14 Node test đều đạt; smoke bằng runtime đi kèm đạt. Ảnh render vector nam/nữ/cùng hát được kiểm trong releases/qa-1.0.1. Chưa thử cài prerequisite trên máy Windows 10 sạch.
