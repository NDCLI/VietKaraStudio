import io
import json
import sys
from pathlib import Path
from types import ModuleType

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import ai_worker


def request(kind="recognize", **kwargs):
    return {
        "protocol_version": 1,
        "request_id": "a" * 32,
        "kind": kind,
        "project_id": "b" * 32,
        "input": kwargs.pop("input", {"vocals": "vocals.wav"}),
        "options": kwargs.pop("options", {}),
    }


def test_dispatch_maps_recognition_to_existing_worker(tmp_path, monkeypatch):
    calls = []
    module = ModuleType("worker")

    def run(payload, device, report_progress):
        calls.append((payload, device))
        report_progress(25, "Đang nhận dạng")
        Path(payload["output"]).write_text('{"words": []}', encoding="utf-8")

    module.run = run
    monkeypatch.setitem(sys.modules, "worker", module)
    progress = []
    result = ai_worker.dispatch(request(options={"model": "small", "language": "vi", "device": "cpu"}),
                               lambda *event: progress.append(event))
    payload, device = calls[0]
    assert device == "cpu" and payload["vocals"] == "vocals.wav"
    assert payload["model"] == "small" and payload["language"] == "vi"
    assert result == {"words": []}
    assert progress == [(25, "Đang nhận dạng")]


def test_align_region_requires_region_and_passes_it_through():
    with pytest.raises(ValueError, match="options.region"):
        ai_worker.dispatch(request("align_region"), lambda *_: None)


def test_jsonl_stdio_emits_only_protocol_on_stdout(monkeypatch):
    def dispatch(payload, report_progress):
        print("library diagnostic")
        report_progress(50, "half")
        return {"ok": True}

    monkeypatch.setattr(ai_worker, "dispatch", dispatch)
    source = json.dumps(request()) + "\n"
    stdout, stderr = io.StringIO(), io.StringIO()
    assert ai_worker.main_stdio(io.StringIO(source), stdout, stderr) == 0
    messages = [json.loads(line) for line in stdout.getvalue().splitlines()]
    assert [message["type"] for message in messages] == ["progress", "result"]
    assert all(message["request_id"] == "a" * 32 for message in messages)
    assert "library diagnostic" in stderr.getvalue()


def test_jsonl_stdio_reports_errors_as_terminal_message():
    source = json.dumps(request("unknown")) + "\n"
    stdout, stderr = io.StringIO(), io.StringIO()
    assert ai_worker.main_stdio(io.StringIO(source), stdout, stderr) == 1
    message = json.loads(stdout.getvalue())
    assert message["type"] == "error"
    assert message["request_id"] == "a" * 32
    assert "kind" in message["error"]
