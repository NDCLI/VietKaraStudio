"""Probe actual backend capabilities in a disposable process; never assume GPU == usable CUDA."""
from __future__ import annotations

import csv
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parent
NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
_dll_handles = []
_cache = None
_cache_time = 0.
_lock = threading.Lock()


def cuda_library_paths():
    spec = importlib.util.find_spec("torch")
    paths = [Path(spec.origin).parent / "lib"] if spec and spec.origin else []
    return [p for p in paths if p.is_dir()]


def prepare_cuda():
    """Keep DLL directory handles alive, including cuBLAS/cuDNN loaded lazily by CTranslate2."""
    for path in cuda_library_paths():
        entries = os.environ.get("PATH", "").split(os.pathsep)
        if str(path) not in entries:
            os.environ["PATH"] = str(path) + os.pathsep + os.environ.get("PATH", "")
        if os.name == "nt" and hasattr(os, "add_dll_directory"):
            _dll_handles.append(os.add_dll_directory(str(path)))


def nvidia_cards():
    executable = shutil.which("nvidia-smi")
    if not executable and os.name == "nt":
        candidate = Path(os.environ.get("WINDIR", "C:/Windows")) / "System32/nvidia-smi.exe"
        executable = str(candidate) if candidate.is_file() else None
    if not executable:
        return []
    try:
        result = subprocess.run([executable, "--query-gpu=index,name,memory.total,driver_version",
                                 "--format=csv,noheader,nounits"], capture_output=True, text=True,
                                timeout=12, creationflags=NO_WINDOW)
        if result.returncode:
            return []
        return [{"index": int(row[0]), "name": row[1].strip(), "memory_mb": int(row[2]),
                 "driver": row[3].strip()} for row in csv.reader(result.stdout.splitlines()) if len(row) == 4]
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return []


def cpu_backend(reason=""):
    return {"device": "cpu", "device_index": 0, "name": "CPU", "compute_type": "int8", "reason": reason}


def gpu_error(error):
    text = str(error).lower()
    return any(token in text for token in (
        "cuda", "cudnn", "cublas", "out of memory", "float16",
        "device-side", "driver", "nvenc", "mã -107", "code -107"))


def select_ai(probe, preference="auto"):
    if preference not in ("auto", "cpu"):
        raise ValueError("Thiết bị phải là auto hoặc cpu.")
    if preference == "cpu":
        return {"demucs": cpu_backend("Đã chọn CPU"), "whisper": cpu_backend("Đã chọn CPU")}
    return {key: dict(probe.get(key) or cpu_backend()) for key in ("demucs", "whisper")}


def ffmpeg_candidates():
    candidates = []
    override = os.environ.get("OBS_FFMPEG")
    if override:
        candidates.append(override)
    for path in (ROOT / "bin/ffmpeg.exe", ROOT.parent / "ffmpeg.exe"):
        if path.is_file():
            candidates.append(str(path))
    if shutil.which("ffmpeg"):
        candidates.append(shutil.which("ffmpeg"))
    try:
        import imageio_ffmpeg
        candidates.append(imageio_ffmpeg.get_ffmpeg_exe())
    except (ImportError, RuntimeError):
        pass
    return list(dict.fromkeys(candidates))


def probe_encoder(cards):
    if not cards:
        return {"encoder": "libx264", "name": "CPU", "reason": "Không phát hiện NVIDIA GPU"}
    reason = "NVENC chưa sẵn sàng"
    for executable in ffmpeg_candidates():
        try:
            result = subprocess.run([executable, "-hide_banner", "-v", "error", "-f", "lavfi", "-i",
                                     "color=size=256x144:rate=30", "-frames:v", "3", "-c:v", "h264_nvenc",
                                     "-f", "null", "-"], capture_output=True, text=True, encoding="utf-8",
                                    errors="replace", timeout=20, creationflags=NO_WINDOW)
            if result.returncode == 0:
                return {"encoder": "h264_nvenc", "name": "NVIDIA NVENC", "ffmpeg": executable, "reason": ""}
            reason = (result.stderr.splitlines() or [reason])[0][:500]
        except (OSError, subprocess.TimeoutExpired) as exc:
            reason = str(exc)[:500]
    return {"encoder": "libx264", "name": "CPU", "reason": reason}


def probe():
    prepare_cuda()
    cards = nvidia_cards()
    result = {"gpus": cards, "demucs": cpu_backend(), "whisper": cpu_backend(),
              "torch_version": None, "cuda_version": None, "scanned_at": time.time()}
    torch_gpus = []
    try:
        import torch
        result["torch_version"], result["cuda_version"] = torch.__version__, torch.version.cuda
        if torch.cuda.is_available():
            for index in range(torch.cuda.device_count()):
                try:
                    # Exercise CUDA + a convolution, rather than merely querying the driver.
                    with torch.cuda.device(index):
                        a = torch.ones((1,1,64), device=f"cuda:{index}")
                        torch.nn.functional.conv1d(a, torch.ones((1,1,3), device=f"cuda:{index}"))
                        torch.cuda.synchronize(index)
                        properties = torch.cuda.get_device_properties(index)
                        torch_gpus.append({"index": index, "name": properties.name,
                                           "memory_mb": int(properties.total_memory / 1024**2)})
                        del a
                        torch.cuda.empty_cache()
                except Exception as exc:
                    result["demucs"]["reason"] = str(exc)[:500]
        else:
            result["demucs"]["reason"] = "PyTorch đang là bản CPU" if not torch.version.cuda else "PyTorch không khởi tạo được CUDA"
    except Exception as exc:
        result["demucs"]["reason"] = str(exc)[:500]
    if torch_gpus:
        best = max(torch_gpus, key=lambda item: item["memory_mb"])
        result["demucs"] = {"device": f"cuda:{best['index']}", "device_index": best["index"],
                             "name": best["name"], "compute_type": "float32", "reason": ""}
        if not cards:
            result["gpus"] = torch_gpus
    try:
        import ctranslate2
        if os.name == "nt":
            import ctypes
            # A device count alone does not prove these runtime libraries are loadable.
            for name in ("cublas64_12.dll", "cudnn64_9.dll"):
                ctypes.WinDLL(name)
        usable = []
        for index in range(ctranslate2.get_cuda_device_count()):
            types = ctranslate2.get_supported_compute_types("cuda", index)
            compute = next((kind for kind in ("float16", "int8_float16", "float32") if kind in types), None)
            if compute:
                info = next((p for p in torch_gpus if p["index"] == index),
                            next((p for p in cards if p["index"] == index), {"name": f"NVIDIA GPU {index}", "memory_mb": 0}))
                usable.append({"device": "cuda", "device_index": index, "name": info["name"],
                               "compute_type": compute, "memory_mb": info["memory_mb"], "reason": ""})
        if usable:
            result["whisper"] = max(usable, key=lambda item: item["memory_mb"])
        else:
            result["whisper"]["reason"] = "CTranslate2 không tìm thấy thiết bị CUDA dùng được"
    except Exception as exc:
        result["whisper"]["reason"] = str(exc)[:500]
    result["video"] = probe_encoder(result["gpus"])
    result["preferred"] = "gpu" if any(result[key]["device"].startswith("cuda") for key in ("demucs", "whisper")) else "cpu"
    return result


def detect(refresh=False):
    global _cache, _cache_time
    with _lock:
        if _cache is not None and not refresh and time.monotonic() - _cache_time < 120:
            return json.loads(json.dumps(_cache))
        try:
            process = subprocess.run([sys.executable, str(Path(__file__).resolve()), "--probe"],
                                     capture_output=True, text=True, encoding="utf-8", errors="replace",
                                     timeout=75, creationflags=NO_WINDOW)
            if process.returncode:
                raise RuntimeError(process.stderr[-1000:] or "Không kiểm tra được GPU")
            result = json.loads(process.stdout.strip().splitlines()[-1])
        except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as exc:
            reason = str(exc)[:500]
            result = {"gpus": nvidia_cards(), "demucs": cpu_backend(reason), "whisper": cpu_backend(reason),
                      "video": {"encoder": "libx264", "name": "CPU", "reason": reason}, "preferred": "cpu",
                      "scanned_at": time.time()}
        _cache, _cache_time = result, time.monotonic()
        return json.loads(json.dumps(result))


if __name__ == "__main__":
    print(json.dumps(probe(), ensure_ascii=False))
