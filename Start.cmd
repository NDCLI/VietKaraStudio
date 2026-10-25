@echo off
setlocal
cd /d "%~dp0"
if not exist ".venv\Scripts\python.exe" (
    py -3.12 -m venv .venv
    if errorlevel 1 goto :failed
)
".venv\Scripts\python.exe" -c "import importlib.util; assert all(importlib.util.find_spec(n) for n in ('flask','numpy','scipy','soundfile','librosa','demucs','stable_whisper','webview','imageio_ffmpeg','yt_dlp','PIL'))" >nul 2>&1
if errorlevel 1 (
    ".venv\Scripts\python.exe" -m pip install -r requirements.txt
    if errorlevel 1 goto :failed
)
".venv\Scripts\python.exe" configure_gpu.py
".venv\Scripts\python.exe" launch.py
if errorlevel 1 goto :failed
exit /b 0
:failed
echo App chua khoi dong duoc. Xem thong bao loi phia tren.
pause
exit /b 1
