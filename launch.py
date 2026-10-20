from __future__ import annotations

import argparse
import socket
import threading
import webbrowser
import os

from werkzeug.serving import make_server
from app import app


def main():
    parser = argparse.ArgumentParser(description="VietKara Studio")
    parser.add_argument("--browser", action="store_true", help="Mở trong trình duyệt thay cho cửa sổ desktop")
    parser.add_argument("--no-open", action="store_true")
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
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
    import webview
    webview.settings["ALLOW_DOWNLOADS"] = True
    webview.create_window("VietKara Studio", url, width=1480, height=940, min_size=(1000,720),
                          background_color="#0b1019")
    try:
        webview.start()
    finally:
        from app import JOBS
        for job in list(JOBS.values()):
            if job.state in ("running", "queued"):
                job.stop()
        if production:server.close()
        else:server.shutdown()


if __name__ == "__main__":
    main()
