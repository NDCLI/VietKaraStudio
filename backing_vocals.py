"""Prepare a reusable karaoke backing mix on the beat clock."""
from __future__ import annotations

from functools import lru_cache
import hashlib
import json
from pathlib import Path
import re
import sys
import tempfile
from urllib.request import urlopen

import numpy as np
import soundfile as sf

import engine
import hardware
from project_archive import sha256
from timing import map_time, validate_anchors


MODEL = "mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956.ckpt"
MODEL_CONFIG = "mel_band_roformer_karaoke_aufr33_viperx_sdr_10.1956_config.yaml"
MODEL_SHA256 = "1de20d459332fe8869aeb01327a31df0032262706e1365114e852dc271779813"
CONFIG_SHA256 = "b35077d94861f068097cce1a5e54633c055e7dcc2613eade4e4dc7c7c9c3f48b"
STEM_FILE = "backing.wav"
MIX_PATTERN = r"bb_[0-9a-f]{24}\.wav"
SAMPLE_RATE = 44100
MIX_REVISION = 1


@lru_cache(maxsize=128)
def _digest(path: Path, size: int, modified: int, changed: int) -> str:
    return sha256(path)


def fingerprint(path: Path) -> str | None:
    try:
        info = path.stat()
        return _digest(path, info.st_size, info.st_mtime_ns, info.st_ctime_ns)
    except FileNotFoundError:
        return None


def source_key(directory: Path) -> str | None:
    vocals = fingerprint(directory / "vocals.wav")
    if not vocals:
        return None
    return f"{MODEL_SHA256}:{fingerprint(directory / 'reference.wav')}:{vocals}"


def mix_key(project: dict, beat_path: Path, source: str | None) -> str | None:
    beat = fingerprint(beat_path)
    if not beat or not source:
        return None
    data = [MIX_REVISION, beat, source, validate_anchors(project.get("anchors", []))]
    return hashlib.sha256(json.dumps(data, sort_keys=True).encode()).hexdigest()


def mix_file(project: dict, directory: Path) -> Path | None:
    saved = project.get("backing_vocals")
    key = saved.get("mix") if isinstance(saved, dict) else None
    if isinstance(key, str) and re.fullmatch(r"[0-9a-f]{64}", key):
        return directory / f"bb_{key[:24]}.wav"
    return None


def status(project: dict, directory: Path, *, beat_name: str = "beat.wav") -> str:
    saved = project.get("backing_vocals")
    if not isinstance(saved, dict):
        return "missing"
    source = source_key(directory)
    if not source or source != saved.get("source"):
        return "missing"
    if saved.get("weak"):
        return "weak"
    if not (directory / STEM_FILE).is_file():
        return "missing"
    mix = mix_file(project, directory)
    if not mix or not mix.is_file() or saved.get("mix") != mix_key(project, directory / beat_name, source):
        return "stale"
    if saved.get("rejected"):
        return "weak"
    return "ready" if saved.get("confirmed") is True else "review"


def _download_model_file(target: Path, staging: Path, repository: str, expected_sha: str, progress):
    if fingerprint(target) == expected_sha:
        return
    url = f"https://github.com/{repository}/{target.name}"
    partial = staging / (target.name + ".partial")
    digest = hashlib.sha256()
    with urlopen(url, timeout=30) as response, partial.open("wb") as output:
        total = int(response.headers.get("Content-Length", 0))
        received = 0
        reported = -1
        for chunk in iter(lambda: response.read(1024 * 1024), b""):
            output.write(chunk)
            digest.update(chunk)
            received += len(chunk)
            percent = min(100, 100 * received // total) if total else 0
            if percent != reported:
                progress(percent * .25, f"Đang tải model tách bè · {received / 1048576:.0f} MB")
                reported = percent
    if digest.hexdigest() != expected_sha:
        raise RuntimeError("Model tách bè tải về không khớp mã kiểm tra. Hãy thử lại.")
    try:
        partial.replace(target)
    except PermissionError:
        if fingerprint(target) != expected_sha:
            raise


def run_separator(vocals: Path, destination: Path, models: Path, device: str):
    from worker import progress

    hardware.prepare_cuda()
    addon = Path(sys.executable).resolve().parent / "resources" / "backing_runtime"
    if addon.is_dir():
        sys.path.insert(0, str(addon))
    try:
        from audio_separator.separator import Separator
    except ImportError as exc:
        raise RuntimeError("Thiếu thư viện tách bè. Cài các thư viện trong requirements.txt hoặc dùng bộ cài mới.") from exc
    import torch

    if device == "cpu":
        torch.set_num_threads(min(4, torch.get_num_threads()))
    models.mkdir(parents=True, exist_ok=True)
    _download_model_file(models / MODEL, destination.parent, "TRvlvr/model_repo/releases/download/all_public_uvr_models",
                         MODEL_SHA256, progress)
    _download_model_file(models / MODEL_CONFIG, destination.parent, "nomadkaraoke/python-audio-separator/releases/download/model-configs",
                         CONFIG_SHA256, progress)

    class KaraokeSeparator(Separator):
        def check_ffmpeg_installed(self):
            # Input and output are PCM WAV handled by soundfile in this task.
            pass

        def download_model_files(self, model_filename):
            return MODEL, "MDXC", "Mel-RoFormer Karaoke", str(models / MODEL), MODEL_CONFIG

    progress(26, "Đang nạp model tách giọng bè…")
    separator = KaraokeSeparator(model_file_dir=str(models), output_dir=str(destination.parent),
                                  output_format="WAV", output_single_stem="Instrumental",
                                  normalization_threshold=1.0, use_soundfile=True)
    separator.torch_device = torch.device(device)
    separator.load_model(MODEL)
    progress(30, "Đang phân tách giọng chính và giọng bè…")
    files = separator.separate(str(vocals))
    if len(files) != 1:
        raise RuntimeError("Model chưa tạo được track bè.")
    (destination.parent / Path(files[0]).name).replace(destination)
    progress(100, "Đã xử lý track bè.")


def _audible(stem: Path) -> bool:
    with sf.SoundFile(stem) as audio:
        # Reject near-silence. The user reviews actual voice separation.
        return any(float(np.sqrt(np.mean(chunk ** 2))) >= .003
                   for chunk in audio.blocks(blocksize=max(1, audio.samplerate // 4), dtype="float32"))


def _tempo_filters(rate: float) -> list[str]:
    filters = []
    while rate > 2:
        filters.append("atempo=2")
        rate /= 2
    while rate < .5:
        filters.append("atempo=0.5")
        rate *= 2
    if abs(rate - 1) > 1e-8:
        filters.append(f"atempo={rate:.10f}")
    return filters


def _aligned_filter(anchors: list[dict], source_frames: int, beat_frames: int) -> str:
    # Only the inner anchors can change the slope of the reference-to-beat map.
    first = max(0, round(map_time(0, anchors) * SAMPLE_RATE))
    last = min(beat_frames, round(map_time(source_frames / SAMPLE_RATE, anchors) * SAMPLE_RATE))
    if first >= last:
        raise ValueError("Giọng bè nằm ngoài thời lượng beat. Kiểm tra các mốc đồng bộ.")
    boundaries = [first, *(round(a["beat"] * SAMPLE_RATE) for a in anchors[1:-1]
                           if first < round(a["beat"] * SAMPLE_RATE) < last), last]
    labels = [f"src{i}" for i in range(len(boundaries) - 1)]
    split = (f"asplit={len(labels)}" if len(labels) > 1 else "anull")
    graph = ["[1:a]aresample=44100,aformat=channel_layouts=stereo," +
             split + "".join(f"[{label}]" for label in labels)]
    for index, (start, end) in enumerate(zip(boundaries, boundaries[1:])):
        ref_start = max(0, round(map_time(start / SAMPLE_RATE, anchors, inverse=True) * SAMPLE_RATE))
        ref_end = min(source_frames, round(map_time(end / SAMPLE_RATE, anchors, inverse=True) * SAMPLE_RATE))
        length = end - start
        if ref_end <= ref_start:
            graph.append(f"[src{index}]atrim=end_sample={length},volume=0,apad=whole_len={length}[part{index}]")
            continue
        filters = [f"atrim=start_sample={ref_start}:end_sample={ref_end}", "asetpts=PTS-STARTPTS",
                   *_tempo_filters((ref_end - ref_start) / length), f"apad=whole_len={length}",
                   f"atrim=end_sample={length}"]
        fade = min(SAMPLE_RATE // 200, length // 4)
        if fade and index:
            filters.append(f"afade=t=in:ns={fade}")
        if fade and index < len(labels) - 1:
            filters.append(f"afade=t=out:ss={length - fade}:ns={fade}")
        graph.append(f"[src{index}]" + ",".join(filters) + f"[part{index}]")
    parts = "".join(f"[part{i}]" for i in range(len(labels)))
    concat = f"concat=n={len(labels)}:v=0:a=1" if len(labels) > 1 else "anull"
    graph.append(f"{parts}{concat},adelay={first}S:all=1,volume=0.65[backing]")
    graph.append("[0:a][backing]amix=inputs=2:duration=first:normalize=0,"
                 "alimiter=limit=0.98:level=0:latency=1[mix]")
    return ";\n".join(graph)


def prepare_mix(beat: Path, stem: Path, anchors: list[dict], destination: Path, job):
    beat_info, stem_info = sf.info(beat), sf.info(stem)
    beat_frames = round(beat_info.duration * SAMPLE_RATE)
    source_frames = round(stem_info.duration * SAMPLE_RATE)
    graph = _aligned_filter(validate_anchors(anchors), source_frames, beat_frames)
    filter_args = ["-filter_complex", graph]
    if len(graph) > 24000:
        script = destination.with_suffix(".filter")
        script.write_text(graph, encoding="utf-8")
        filter_args = ["-/filter_complex", script]
    engine.command([engine.binary("ffmpeg"), "-y", "-v", "error", "-i", beat, "-i", stem,
                    *filter_args, "-map", "[mix]", "-ar", SAMPLE_RATE,
                    "-ac", 2, "-c:a", "pcm_s16le", destination], job)


def prepare(project: dict, directory: Path, job, preference: str, *, beat_name: str = "beat.wav") -> dict:
    current = status(project, directory, beat_name=beat_name)
    if current in ("ready", "review", "weak"):
        return project["backing_vocals"]
    beat = directory / beat_name
    vocals = directory / "vocals.wav"
    reference = directory / "reference.wav"
    device = None
    if not vocals.is_file():
        if not reference.is_file():
            raise ValueError("Cần bản hát tham chiếu hoặc vocal đã tách.")
        device = hardware.select_ai(preference=preference)["demucs"]
        vocals = engine.separate(reference, directory, job, device)
    source = source_key(directory)
    key = mix_key(project, beat, source)
    if not key:
        raise ValueError("Cần nạp beat và bản hát để tách giọng bè.")
    if current != "stale" and not _audible(vocals):
        return {"source": source, "weak": True}
    stem = directory / STEM_FILE
    result = {"source": source, "mix": key, "confirmed": False}
    target = mix_file({"backing_vocals": result}, directory)
    with tempfile.TemporaryDirectory(prefix="backing-", dir=directory) as staging:
        stage = Path(staging)
        if current != "stale":
            device = device or hardware.select_ai(preference=preference)["demucs"]
            stem = stage / STEM_FILE
            args = [vocals, stem, engine.DATA / "models" / "backing"]
            model_progress = engine.model_progress(job, 42, 85)

            def progress(line):
                percent = re.match(r"\s*(\d{1,3})%\|", line)
                if percent:
                    value = min(100, int(percent.group(1)))
                    job.update(55 + value * .30, f"Đang phân tách giọng bè · {value}%")
                else:
                    model_progress(line)

            job.record_device("backing", device)
            try:
                engine.command(engine.task_command("backing", *args, device["device"]), job,
                               progress=progress)
            except RuntimeError as exc:
                if not device["device"].startswith("cuda") or not hardware.gpu_error(exc):
                    raise
                job.record_device("backing", hardware.cpu_backend(), str(exc))
                engine.command(engine.task_command("backing", *args, "cpu"), job,
                               progress=progress)
            if not _audible(stem):
                return {"source": source, "weak": True}
        job.update(90, "Đang căn giọng bè với beat…")
        if not target.is_file():
            mix = stage / target.name
            prepare_mix(beat, stem, project.get("anchors", []), mix, job)
            job.check()
            mix.replace(target)
        if stem.parent == stage:
            stem.replace(directory / STEM_FILE)
    return result


def discard_old_mixes(project: dict, directory: Path):
    current = mix_file(project, directory)
    for item in directory.glob("bb_*.wav"):
        if item != current and re.fullmatch(MIX_PATTERN, item.name):
            try:
                item.unlink(missing_ok=True)
            except OSError:
                # Windows can hold a previously streamed mix open.
                pass
