from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path
import threading
import webbrowser
import os

from werkzeug.serving import make_server
from app import app


WEBVIEW2_HELP = "https://developer.microsoft.com/microsoft-edge/webview2/"
APP_ICON = Path(__file__).resolve().parent / "resources" / "VietKaraStudio.ico"


def windows_app_identity():
    import ctypes
    # Give the Python-hosted window the product identity in the Windows taskbar.
    ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("VietKaraStudio.Desktop")


def desktop_webview():
    """Reject legacy MSHTML before it opens a broken modern web interface."""
    import webview
    if sys.platform == "win32":
        windows_app_identity()
        from webview.guilib import initialize
        # pywebview can still choose MSHTML when edgechromium is requested.
        # setup_app is idempotent; start() reuses the selected backend module.
        backend = initialize("edgechromium")
        if backend.renderer != "edgechromium":
            raise RuntimeError(
                "VietKara Studio cần Microsoft Edge WebView2 Runtime để hiển thị đúng.\n"
                "Hãy cài hoặc sửa WebView2 Evergreen Runtime rồi mở lại ứng dụng.\n"
                "Có trình duyệt Edge chưa có nghĩa là đã có WebView2 Runtime.\n\n"
                + WEBVIEW2_HELP
            )
        logging.info("Desktop renderer: %s", backend.renderer)
    return webview


def main():
    parser = argparse.ArgumentParser(description="VietKara Studio")
    parser.add_argument("--browser", action="store_true", help="Mở trong trình duyệt thay cho cửa sổ desktop")
    parser.add_argument("--no-open", action="store_true")
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    webview = None
    if not (args.browser or args.no_open):
        try:
            webview = desktop_webview()
        except Exception as exc:
            logging.exception("Desktop runtime initialization failed")
            message = str(exc)
            if WEBVIEW2_HELP not in message:
                message = ("Không khởi tạo được giao diện VietKara Studio.\n"
                           "Hãy kiểm tra Microsoft Edge WebView2 Runtime.\n\n"
                           + message + "\n\n" + WEBVIEW2_HELP)
            if sys.platform == "win32":
                import ctypes
                ctypes.windll.user32.MessageBoxW(None, message, "VietKara Studio", 0x10)
            raise SystemExit(1) from exc
    production=os.environ.get('VIETKARA_PRODUCTION')=='1'
    if production:
        from waitress.server import create_server
        server=create_server(app,host='127.0.0.1',port=args.port,threads=8,max_request_body_size=3*1024**3,
                             expose_tracebacks=False,ident='VietKaraStudio')
        port=server.effective_port
        run=server.run
    else:
        server = make_server("127.0.0.1", args.port, app, threaded=True)
        port=server.server_port
        run=server.serve_forever
    url = f"http://127.0.0.1:{port}"
    print(f"VietKara Studio: {url}", flush=True)
    if args.browser or args.no_open:
        if not args.no_open:
            webbrowser.open(url)
        try:
            run()
        except KeyboardInterrupt:
            pass
        return
    threading.Thread(target=run, daemon=True).start()
    try:
        webview.settings["ALLOW_DOWNLOADS"] = True
        webview.create_window("VietKara Studio", url, width=1480, height=940, min_size=(1000,720),
                              background_color="#0b1019")
        webview.start(gui="edgechromium" if sys.platform == "win32" else None,
                      icon=str(APP_ICON) if APP_ICON.is_file() else None)
    finally:
        from app import JOBS
        for job in list(JOBS.values()):
            if job.state in ("running", "queued"):
                job.stop()
        if production:server.close()
        else:server.shutdown()


if __name__ == "__main__":
    main()
