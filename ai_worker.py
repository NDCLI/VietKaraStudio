"""JSON Lines entry point for the Rust AI bridge.

The existing desktop speech worker remains available through ``worker.py``.
This adapter translates the versioned Rust protocol to those same model
functions and keeps stdout exclusively for protocol messages.
"""
from __future__ import annotations

import contextlib
import json
import shutil
import sys
import tempfile
from pathlib import Path

PROTOCOL_VERSION = 1
KINDS = frozenset(("separate", "recognize", "align", "align_region", "role_suggest",
                   "sync_suggest"))


class _SyncProgress:
    """Adapt the audio-sync job callbacks to the JSONL progress stream.

    Cancellation stops the worker process in the bridge, so check() is only
    needed to preserve the audio_sync job interface between progress reports.
    """

    def __init__(self, report_progress):
        self.report_progress = report_progress

    def check(self):
        return None

    def update(self, progress, message):
        self.report_progress(progress, message)


def _required_text(mapping, key):
    value = mapping.get(key)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"Thiếu trường {key} dạng chuỗi.")
    return value


def dispatch(request, report_progress):
    """Run one validated request and return its JSON-serializable result."""
    if not isinstance(request, dict):
        raise ValueError("Request phải là một JSON object.")
    if type(request.get("protocol_version")) is not int or request["protocol_version"] != PROTOCOL_VERSION:
        raise ValueError("protocol_version không được hỗ trợ.")
    if not isinstance(request.get("request_id"), str) or not request["request_id"]:
        raise ValueError("Thiếu request_id.")
    kind = request.get("kind")
    if kind not in KINDS:
        raise ValueError(f"kind không được hỗ trợ: {kind!r}")
    inputs = request.get("input")
    options = request.get("options")
    if not isinstance(inputs, dict) or not isinstance(options, dict):
        raise ValueError("input và options phải là JSON object.")

    if kind == "sync_suggest":
        reference = Path(_required_text(inputs, "reference"))
        beat = Path(_required_text(inputs, "beat"))
        cache_key = None
        cache_root_value = options.get("data_root") or options.get("cache_root")
        cache_root = Path(cache_root_value) if cache_root_value else None
        if cache_root is not None:
            from analysis_cache import load_sync, sync_cache_key

            cache_options = {
                name: value for name, value in options.items()
                if name not in ("data_root", "cache_root")
            }
            cache_key = sync_cache_key(reference, beat, extra_options=cache_options)
            if cache_key:
                cached = load_sync(cache_root, cache_key)
                if cached is not None:
                    return cached
        from audio_sync import suggest

        result = suggest(reference, beat, _SyncProgress(report_progress))
        if cache_root is not None and cache_key:
            from analysis_cache import store_sync, sync_cache_key

            current_key = sync_cache_key(reference, beat, extra_options=cache_options)
            if current_key != cache_key:
                raise RuntimeError("File audio đã đổi trong lúc phân tích đồng bộ; hãy chạy lại.")

            store_sync(cache_root, cache_key, result)
        return result

    if kind in ("recognize", "align", "align_region"):
        from worker import run

        vocals = _required_text(inputs, "vocals")
        model = options.get("model", "medium")
        language = options.get("language", "vi")
        if model not in ("small", "medium", "large-v3-turbo"):
            raise ValueError("Model không hợp lệ.")
        if language not in ("vi", "en", "auto"):
            raise ValueError("Ngôn ngữ không hợp lệ.")
        lyrics = options.get("lyrics", "")
        if not isinstance(lyrics, str):
            raise ValueError("options.lyrics phải là chuỗi.")
        if kind == "align" and not lyrics.strip():
            raise ValueError("align cần options.lyrics.")
        region = options.get("region")
        if kind == "align_region" and not isinstance(region, dict):
            raise ValueError("align_region cần options.region.")
        cache_key = None
        cache_root_value = options.get("data_root") or options.get("cache_root")
        if cache_root_value:
            from analysis_cache import cache_key as make_cache_key, load as load_cache

            # Recognition consumes the vocal track, so fingerprint that exact
            # file even when the request also carries its original reference.
            source = Path(vocals)
            cache_options = {
                name: options[name]
                for name in ("device", "device_index", "compute_type", "device_name")
                if name in options
            }
            cache_key = make_cache_key(
                source,
                model,
                language,
                lyrics,
                options.get("preference", "auto"),
                region,
                extra_options=cache_options,
            )
            if cache_key:
                cached = load_cache(Path(cache_root_value), cache_key)
                if cached:
                    return cached["result"]
        request_for_worker = {
            "vocals": vocals,
            "model": model,
            "language": language,
            "lyrics": lyrics,
            "device": options.get("device", "cpu"),
            "device_index": options.get("device_index", 0),
            "compute_type": options.get("compute_type"),
            "models_root": options.get("models_root", str(Path(__file__).parent / "data")),
            "name": options.get("device_name"),
            "fallback_reason": options.get("fallback_reason"),
        }
        if region is not None:
            request_for_worker["region"] = region
        with tempfile.TemporaryDirectory(prefix="vietkara-ai-") as directory:
            output = Path(directory) / "result.json"
            request_for_worker["output"] = str(output)
            run(request_for_worker, request_for_worker["device"], report_progress)
            result = json.loads(output.read_text(encoding="utf-8"))
        if cache_key:
            from analysis_cache import store as store_cache

            try:
                store_cache(Path(cache_root_value), cache_key, result, Path(vocals))
            except OSError:
                # Cache is opportunistic; a failed write must not discard a
                # valid recognition result or turn the AI job into an error.
                pass
        return result

    if kind == "role_suggest":
        vocal = _required_text(inputs, "vocal")
        segments = inputs.get("segments")
        if not isinstance(segments, list):
            raise ValueError("role_suggest cần input.segments dạng danh sách.")
        from duet_roles import analyze

        return analyze(vocal, segments, report_progress)

    # Match the current desktop Demucs invocation while letting the bridge
    # own the output path. Its progress bars/logs are redirected to stderr.
    reference = _required_text(inputs, "reference")
    destination = Path(_required_text(inputs, "output"))
    device = options.get("device", "cpu")
    model = options.get("model", "htdemucs")
    if not isinstance(device, str) or not isinstance(model, str):
        raise ValueError("options.device và options.model phải là chuỗi.")
    from demucs.separate import main

    report_progress(0.0, "Đang tách vocal bằng Demucs…")
    with tempfile.TemporaryDirectory(prefix="vietkara-demucs-") as directory:
        args = ["--two-stems", "vocals", "--float32", "--shifts", "1", "--jobs", "1",
                "-o", directory, "-n", model, "-d", device, reference]
        main(args)
        found = list(Path(directory).glob("*/*/vocals.wav"))
        if not found:
            raise RuntimeError("Demucs chưa tạo được track vocals.wav.")
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(found[0], destination)
    report_progress(100.0, "Đã tách vocal.")
    return {"vocals": str(destination)}


def _write(stream, request_id, message_type, **fields):
    message = {"protocol_version": PROTOCOL_VERSION, "request_id": request_id,
               "type": message_type, **fields}
    stream.write(json.dumps(message, ensure_ascii=False, allow_nan=False) + "\n")
    stream.flush()


def main_stdio(stdin=None, stdout=None, stderr=None):
    stdin = stdin or sys.stdin
    stdout = stdout or sys.stdout
    stderr = stderr or sys.stderr
    lines = [line for line in stdin if line.strip()]
    request_id = ""
    try:
        if len(lines) != 1:
            raise ValueError("Worker cần đúng một JSON request trên stdin.")
        request = json.loads(lines[0])
        if isinstance(request, dict) and isinstance(request.get("request_id"), str):
            request_id = request["request_id"]

        def report_progress(value, message):
            _write(stdout, request_id, "progress", progress=float(value), message=str(message))

        # Existing model libraries and their CLI helpers sometimes print to
        # stdout. Keep that diagnostic output on stderr, reserving stdout for
        # the protocol writer above.
        with contextlib.redirect_stdout(stderr):
            result = dispatch(request, report_progress)
        _write(stdout, request_id, "result", result=result)
        return 0
    except Exception as exc:
        print(f"AI worker error: {exc}", file=stderr, flush=True)
        _write(stdout, request_id, "error", error=str(exc))
        return 1


if __name__ == "__main__":
    raise SystemExit(main_stdio())
