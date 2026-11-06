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


def test_dispatch_uses_cache_on_match_and_stores_cache_miss(tmp_path, monkeypatch):
    worker_calls = []
    cache = {"result": None}
    worker_module = ModuleType("worker")
    cache_module = ModuleType("analysis_cache")

    def run(payload, device, report_progress):
        worker_calls.append(payload)
        Path(payload["output"]).write_text(
            '{"words": [], "text": "fixture"}', encoding="utf-8"
        )

    def cache_key(source, model, language, lyrics, preference, region, extra_options=None):
        assert source == Path("vocals.wav")
        assert extra_options == {"device": "cpu"}
        return "f" * 64

    def load_cache(data_root, key):
        assert data_root == tmp_path and key == "f" * 64
        return {"result": cache["result"]} if cache["result"] else None

    def store_cache(data_root, key, result, vocals):
        assert data_root == tmp_path and key == "f" * 64
        assert vocals == Path("vocals.wav")
        cache["result"] = result
        return True

    worker_module.run = run
    cache_module.cache_key = cache_key
    cache_module.load = load_cache
    cache_module.store = store_cache
    monkeypatch.setitem(sys.modules, "worker", worker_module)
    monkeypatch.setitem(sys.modules, "analysis_cache", cache_module)
    payload = request(input={"vocals": "vocals.wav", "reference": "reference.wav"},
                      options={"device": "cpu", "data_root": str(tmp_path)})

    first = ai_worker.dispatch(payload, lambda *_: None)
    second = ai_worker.dispatch(payload, lambda *_: None)

    assert first == second == {"words": [], "text": "fixture"}
    assert len(worker_calls) == 1


def test_analysis_cache_key_includes_runtime_options(tmp_path):
    import analysis_cache

    source = tmp_path / "reference.wav"
    source.write_bytes(b"stable source fixture")
    cpu = analysis_cache.cache_key(source, "small", "vi", "", "auto",
                                   extra_options={"device": "cpu"})
    cuda = analysis_cache.cache_key(source, "small", "vi", "", "auto",
                                    extra_options={"device": "cuda"})
    same = analysis_cache.cache_key(source, "small", "vi", "", "auto",
                                    extra_options={"device": "cpu"})
    assert cpu != cuda
    assert cpu == same


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
