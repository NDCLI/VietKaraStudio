import io
import json
import os
import subprocess
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


def test_dispatch_auto_selects_verified_nvidia_backend(monkeypatch):
    calls = []
    hardware = ModuleType("hardware")
    worker_module = ModuleType("worker")

    def select_ai(preference):
        calls.append(preference)
        return {"whisper": {
            "device": "cuda", "device_index": 2, "name": "RTX fixture",
            "compute_type": "float16", "reason": "",
        }}

    def run(payload, device, report_progress):
        calls.append((payload, device))
        Path(payload["output"]).write_text(
            json.dumps({"device": device, "index": payload["device_index"],
                        "compute_type": payload["compute_type"]}),
            encoding="utf-8",
        )

    hardware.select_ai = select_ai
    worker_module.run = run
    monkeypatch.setitem(sys.modules, "hardware", hardware)
    monkeypatch.setitem(sys.modules, "worker", worker_module)

    result = ai_worker.dispatch(
        request(options={"device": "auto", "data_root": "C:/VietKaraData"}),
        lambda *_: None,
    )

    assert calls[0] == "auto"
    assert calls[1][1] == "cuda"
    assert result == {"device": "cuda", "index": 2, "compute_type": "float16"}


def test_auto_backend_cache_key_uses_the_resolved_runtime(tmp_path, monkeypatch):
    import analysis_cache

    hardware = ModuleType("hardware")
    hardware.select_ai = lambda preference: {
        "whisper": {
            "device": "cuda", "device_index": 1, "name": "GPU",
            "compute_type": "float16", "reason": "",
        }
    }
    monkeypatch.setitem(sys.modules, "hardware", hardware)
    selected = ai_worker._resolve_ai_backend({"device": "auto"}, "whisper")
    vocals = tmp_path / "vocals.wav"
    vocals.write_bytes(b"audio fixture")
    cpu_key = analysis_cache.cache_key(
        vocals, "small", "vi", "", "auto",
        extra_options={"device": "cpu", "device_index": 0, "compute_type": "int8"},
    )
    gpu_key = analysis_cache.cache_key(
        vocals, "small", "vi", "", "auto",
        extra_options={
            "device": selected["device"],
            "device_index": selected["device_index"],
            "compute_type": selected["compute_type"],
        },
    )
    assert cpu_key != gpu_key


def test_separation_auto_selects_verified_demucs_device(tmp_path, monkeypatch):
    hardware = ModuleType("hardware")
    demucs = ModuleType("demucs")
    demucs.__path__ = []
    separate = ModuleType("demucs.separate")
    calls = []

    def select_ai(preference):
        assert preference == "auto"
        return {"demucs": {
            "device": "cuda:1", "device_index": 1, "name": "RTX fixture",
            "compute_type": "float32", "reason": "",
        }}

    def main(args):
        calls.extend(args)
        output_root = Path(args[args.index("-o") + 1])
        model = args[args.index("-n") + 1]
        vocals = output_root / model / "reference" / "vocals.wav"
        vocals.parent.mkdir(parents=True)
        vocals.write_bytes(b"vocal fixture")

    hardware.select_ai = select_ai
    separate.main = main
    monkeypatch.setitem(sys.modules, "hardware", hardware)
    monkeypatch.setitem(sys.modules, "demucs", demucs)
    monkeypatch.setitem(sys.modules, "demucs.separate", separate)
    destination = tmp_path / "cache" / "vocals.wav"
    result = ai_worker.dispatch(
        request("separate", input={
            "reference": str(tmp_path / "reference.wav"),
            "output": str(destination),
        }, options={"device": "auto"}),
        lambda *_: None,
    )
    assert calls[calls.index("-d") + 1] == "cuda:1"
    assert destination.read_bytes() == b"vocal fixture"
    assert result["vocals"] == str(destination)


def test_ai_device_rejects_unsupported_values():
    with pytest.raises(ValueError, match="Thiết bị AI"):
        ai_worker._resolve_ai_backend({"device": "mps"}, "whisper")


def test_dispatch_maps_region_alignment_with_lyrics_and_audio_bounds(monkeypatch):
    calls = []
    module = ModuleType("worker")

    def run(payload, device, report_progress):
        calls.append((payload, device))
        report_progress(68, "Đang căn câu đã chọn")
        Path(payload["output"]).write_text(
            json.dumps({"text": "Xin chào", "words": [
                {"text": "Xin", "start": 2.1, "end": 2.4, "line": 0},
                {"text": "chào", "start": 2.4, "end": 2.8, "line": 0},
            ]}),
            encoding="utf-8",
        )

    module.run = run
    monkeypatch.setitem(sys.modules, "worker", module)
    progress = []
    payload = request(
        "align_region",
        input={"vocals": "cache/vocals.wav"},
        options={
            "device": "cpu",
            "language": "vi",
            "lyrics": "Xin chào",
            "region": {"start": 1.0, "end": 4.0},
        },
    )

    result = ai_worker.dispatch(payload, lambda *event: progress.append(event))

    worker_payload, device = calls[0]
    assert device == "cpu"
    assert worker_payload["vocals"] == "cache/vocals.wav"
    assert worker_payload["lyrics"] == "Xin chào"
    assert worker_payload["region"] == {"start": 1.0, "end": 4.0}
    assert result["words"][0]["start"] == 2.1
    assert progress == [(68, "Đang căn câu đã chọn")]


def test_dispatch_maps_sync_suggestion_to_python_analysis(tmp_path, monkeypatch):
    calls = []
    module = ModuleType("audio_sync")

    def suggest(reference, beat, job):
        calls.append((reference, beat))
        job.check()
        job.update(42, "Đang kiểm tra đoạn")
        return {
            "anchors": [{"ref": 0.0, "beat": 2.0}],
            "requires_confirmation": True,
        }

    module.suggest = suggest
    monkeypatch.setitem(sys.modules, "audio_sync", module)
    progress = []
    payload = request(
        "sync_suggest",
        input={"reference": str(tmp_path / "reference.wav"),
               "beat": str(tmp_path / "beat.wav")},
    )

    result = ai_worker.dispatch(payload, lambda *event: progress.append(event))

    assert calls == [(tmp_path / "reference.wav", tmp_path / "beat.wav")]
    assert result == {"anchors": [{"ref": 0.0, "beat": 2.0}],
                      "requires_confirmation": True}
    assert progress == [(42, "Đang kiểm tra đoạn")]


def test_dispatch_maps_role_suggestion_without_mutating_project_state(monkeypatch):
    calls = []
    module = ModuleType("duet_roles")

    def analyze(vocal, segments, report_progress):
        calls.append((vocal, segments))
        report_progress(72, "Đang phân tích hai lane")
        return {
            "roles": ["male", "female", None],
            "two_registers": True,
            "male": 1,
            "female": 1,
            "shared": 0,
            "uncertain": 1,
        }

    module.analyze = analyze
    monkeypatch.setitem(sys.modules, "duet_roles", module)
    vocal = Path("cache") / "vocal.wav"
    segments = [
        {"line": 4, "start": 0.0, "end": 1.5},
        {"line": 9, "start": 2.0, "end": 3.5},
        {"line": 12, "start": 4.0, "end": 5.0},
    ]
    progress = []
    result = ai_worker.dispatch(
        request("role_suggest", input={"vocal": str(vocal), "segments": segments}),
        lambda *event: progress.append(event),
    )

    assert calls == [(str(vocal), segments)]
    assert result["roles"] == ["male", "female", None]
    assert result["two_registers"] is True
    assert progress == [(72, "Đang phân tích hai lane")]


def test_sync_suggestion_cache_keys_both_tracks_and_reuses_only_matching_result(
    tmp_path, monkeypatch
):
    reference = tmp_path / "reference.wav"
    beat = tmp_path / "beat.wav"
    reference.write_bytes(b"reference-v1")
    beat.write_bytes(b"beat-v1")
    calls = []
    module = ModuleType("audio_sync")

    def suggest(reference_path, beat_path, job):
        calls.append((reference_path, beat_path))
        job.update(90, "Phân tích xong")
        return {
            "anchors": [{"ref": 0.0, "beat": float(len(calls))}],
            "requires_confirmation": True,
        }

    module.suggest = suggest
    monkeypatch.setitem(sys.modules, "audio_sync", module)
    payload = request(
        "sync_suggest",
        input={"reference": str(reference), "beat": str(beat)},
        options={"cache_root": str(tmp_path / "data"), "algorithm_option": "default"},
    )

    first = ai_worker.dispatch(payload, lambda *_: None)
    cached = ai_worker.dispatch(payload, lambda *_: None)
    assert first == cached
    assert len(calls) == 1

    beat.write_bytes(b"beat-v2")
    changed_source = ai_worker.dispatch(payload, lambda *_: None)
    assert changed_source != first
    assert len(calls) == 2

    payload["options"]["algorithm_option"] = "changed"
    changed_option = ai_worker.dispatch(payload, lambda *_: None)
    assert changed_option != changed_source
    assert len(calls) == 3


def test_sync_cache_key_includes_both_audio_fingerprints_and_options(tmp_path):
    import analysis_cache

    reference = tmp_path / "reference.wav"
    beat = tmp_path / "beat.wav"
    reference.write_bytes(b"reference-v1")
    beat.write_bytes(b"beat-v1")
    initial = analysis_cache.sync_cache_key(reference, beat, {"mode": "safe"})
    same = analysis_cache.sync_cache_key(reference, beat, {"mode": "safe"})
    changed_option = analysis_cache.sync_cache_key(reference, beat, {"mode": "fast"})
    beat.write_bytes(b"beat-v2")
    changed_beat = analysis_cache.sync_cache_key(reference, beat, {"mode": "safe"})
    reference.write_bytes(b"reference-v2")
    changed_reference = analysis_cache.sync_cache_key(reference, beat, {"mode": "safe"})

    assert initial == same
    assert len({initial, changed_option, changed_beat, changed_reference}) == 4


def test_sync_worker_does_not_cache_if_source_changes_during_analysis(tmp_path, monkeypatch):
    reference = tmp_path / "reference.wav"
    beat = tmp_path / "beat.wav"
    reference.write_bytes(b"reference")
    beat.write_bytes(b"beat-before")
    module = ModuleType("audio_sync")

    def suggest(reference_path, beat_path, job):
        beat_path.write_bytes(b"beat-during-analysis")
        return {"anchors": [], "requires_confirmation": True}

    module.suggest = suggest
    monkeypatch.setitem(sys.modules, "audio_sync", module)
    payload = request(
        "sync_suggest",
        input={"reference": str(reference), "beat": str(beat)},
        options={"data_root": str(tmp_path / "data")},
    )

    with pytest.raises(RuntimeError, match="đã đổi trong lúc phân tích"):
        ai_worker.dispatch(payload, lambda *_: None)

    cache_root = tmp_path / "data" / "cache" / "audio_sync"
    assert not cache_root.exists() or not list(cache_root.iterdir())


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
        assert extra_options == {
            "device": "cpu", "device_index": 0, "compute_type": "int8"
        }
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


def test_jsonl_worker_forces_utf8_protocol_even_with_legacy_windows_encoding():
    environment = os.environ.copy()
    environment["PYTHONIOENCODING"] = "cp1252"
    source = json.dumps({
        "protocol_version": 99,
        "request_id": "utf8-smoke",
        "kind": "recognize",
        "input": {},
        "options": {},
    }, ensure_ascii=False) + "\n"
    result = subprocess.run(
        [sys.executable, str(Path(ai_worker.__file__).resolve())],
        input=source.encode("utf-8"), capture_output=True, env=environment, timeout=15,
    )
    assert result.returncode == 1
    message = json.loads(result.stdout.decode("utf-8"))
    assert message["type"] == "error"
    assert "không được hỗ trợ" in message["error"]
