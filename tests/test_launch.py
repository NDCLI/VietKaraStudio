import sys
from pathlib import Path
from types import ModuleType, SimpleNamespace

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import launch


@pytest.mark.parametrize("renderer,allowed", [("edgechromium", True), ("mshtml", False)])
def test_desktop_rejects_silent_legacy_fallback(monkeypatch, renderer, allowed):
    webview = ModuleType("webview")
    guilib = ModuleType("webview.guilib")
    requests = []
    def initialize(gui):
        requests.append(gui)
        return SimpleNamespace(renderer=renderer)
    guilib.initialize = initialize
    monkeypatch.setitem(sys.modules, "webview", webview)
    monkeypatch.setitem(sys.modules, "webview.guilib", guilib)
    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setattr(launch, "windows_app_identity", lambda: None)
    if allowed:
        assert launch.desktop_webview() is webview
    else:
        with pytest.raises(RuntimeError, match="WebView2 Runtime"):
            launch.desktop_webview()
    assert requests == ["edgechromium"]


def test_headless_server_does_not_require_a_desktop_runtime(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["launch.py", "--no-open"])
    monkeypatch.delenv("VIETKARA_PRODUCTION", raising=False)
    def fail():
        raise AssertionError("headless mode must not load a GUI")
    monkeypatch.setattr(launch, "desktop_webview", fail)
    calls = []
    monkeypatch.setattr(launch, "make_server", lambda *a, **k: SimpleNamespace(
        server_port=1234, serve_forever=lambda: calls.append("run")))
    launch.main()
    assert calls == ["run"]
