"""Opt-in real model smoke test; never alters a project. python tools/gpu_smoke.py audio.wav"""
from pathlib import Path
import json
import shutil
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import engine
import hardware
from timing import validate_words

if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    root = Path(tempfile.mkdtemp(prefix="obs-gpu-smoke-"))
    source = Path(sys.argv[1]).resolve()
    engine.command([engine.binary("ffmpeg"), "-y", "-v", "error", "-i", source,
                    "-t", "8", "-ar", "44100", "-ac", "2", root / "reference.wav"])
    job = engine.Job("smoke", "smoke", "analyze")
    started = time.monotonic()
    found = hardware.detect()
    print(json.dumps(found), flush=True)
    assert found["demucs"]["device"].startswith("cuda"), found
    assert found["whisper"]["device"] == "cuda", found
    result = engine.recognize(root, job, model="medium", language="vi")
    validate_words(result["words"])
    assert result["device"] == "cuda", result
    assert job.devices["demucs"]["device"].startswith("cuda"), job.public()
    separation = dict(job.devices["demucs"])
    lyrics = result["text"]
    aligned = engine.recognize(root, job, model="medium", language="vi", lyrics=lyrics)
    validate_words(aligned["words"])
    assert aligned["device"] == "cuda", aligned
    assert not job.fallbacks, job.public()
    report = {"directory": str(root), "elapsed_s": round(time.monotonic()-started, 2),
                      "words": len(result["words"]), "aligned": len(aligned["words"]),
              "devices": {**job.devices, "demucs": separation}}
    (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False), flush=True)
