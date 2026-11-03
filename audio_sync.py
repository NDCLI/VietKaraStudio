"""Conservative reference-vocal to beat alignment suggestions."""
from __future__ import annotations

import math
from pathlib import Path


def _features(path: Path):
    import numpy as np
    import soundfile as sf
    from math import gcd
    from scipy.signal import resample_poly, stft

    samples, source_rate = sf.read(path, dtype="float32", always_2d=True)
    if samples.size == 0:
        raise ValueError("File âm thanh rỗng.")
    mono = samples.mean(axis=1)
    target_rate = 11025
    divisor = gcd(int(source_rate), target_rate)
    if source_rate != target_rate:
        mono = resample_poly(mono, target_rate // divisor, int(source_rate) // divisor).astype("float32")
    hop, fft_size = 1024, 2048
    frequencies, _, spectrum = stft(mono, fs=target_rate, nperseg=fft_size,
                                     noverlap=fft_size - hop, boundary="zeros")
    magnitude = np.log1p(np.abs(spectrum) * 30.0)
    midi = np.rint(69 + 12 * np.log2(np.maximum(frequencies, 1) / 440.0)).astype(int)
    chroma = np.zeros((magnitude.shape[1], 12), dtype=np.float32)
    usable = (frequencies >= 45) & (frequencies <= 5000)
    for pitch in range(12):
        bins = usable & (midi % 12 == pitch)
        if np.any(bins):
            chroma[:, pitch] = magnitude[bins].mean(axis=0)
    flux = np.maximum(0, np.diff(magnitude, axis=1, prepend=magnitude[:, :1]))
    onset = np.sqrt(np.mean(flux * flux, axis=0))[:, None]
    onset /= np.maximum(onset.max(), 1e-6)
    chroma /= np.maximum(np.linalg.norm(chroma, axis=1, keepdims=True), 1e-6)
    features = np.concatenate((chroma, onset * .65), axis=1)
    features /= np.maximum(np.linalg.norm(features, axis=1, keepdims=True), 1e-6)
    return features.astype(np.float32), len(mono) / target_rate


def suggest(reference_path: Path, beat_path: Path, job=None) -> dict:
    """Find a best global linear mapping. The result is advisory and never applied here."""
    import numpy as np
    from scipy.signal import correlate, correlation_lags

    reference, reference_duration = _features(reference_path)
    beat, beat_duration = _features(beat_path)
    if min(reference_duration, beat_duration) < 8:
        raise ValueError("Hai file cần dài ít nhất 8 giây để so khớp an toàn.")
    if job:
        job.update(18, "Đang so sánh đặc trưng âm thanh của bản hát và beat…")
    frame_rate = 11025 / 1024
    scales = np.round(np.arange(.94, 1.061, .005), 3)
    minimum_overlap = max(5, min(30, .35 * min(reference_duration, beat_duration)))
    best = None
    for scale_index, scale in enumerate(scales):
        if job:
            job.check()
        transformed_count = max(1, int(math.ceil((len(beat) - 1) / scale)) + 1)
        source_positions = np.arange(transformed_count, dtype=np.float32) * scale
        left = np.minimum(source_positions.astype(np.int32), len(beat) - 1)
        right = np.minimum(left + 1, len(beat) - 1)
        weight = (source_positions - left).astype(np.float32)[:, None]
        transformed = beat[left] * (1 - weight) + beat[right] * weight
        cross = np.zeros(len(reference) + len(transformed) - 1, dtype=np.float64)
        for column in range(reference.shape[1]):
            cross += correlate(reference[:, column], transformed[:, column], mode="full", method="fft")
        overlap = correlate(np.ones(len(reference), dtype=np.float32),
                            np.ones(len(transformed), dtype=np.float32), mode="full", method="fft")
        scores = cross / np.maximum(overlap, 1)
        lags = correlation_lags(len(reference), len(transformed), mode="full")
        eligible = overlap >= minimum_overlap * frame_rate
        scores[~eligible] = -1
        index = int(np.argmax(scores))
        score = float(scores[index])
        if best is None or score > best["score"]:
            best = {"score": score, "scale": float(scale), "lag": int(lags[index]),
                    "overlap": float(overlap[index] / frame_rate), "scores": scores,
                    "lags": lags, "overlap_values": overlap, "transformed_length": transformed_count}
        if job and scale_index % 5 == 0:
            job.update(20 + int(65 * scale_index / len(scales)), "Đang dò độ lệch và tốc độ giữa hai bản…")
    if not best or best["score"] < .05:
        raise ValueError("Không tìm được đoạn âm thanh tương đồng đủ tin cậy. Hãy đặt mốc thủ công.")

    scale, lag = best["scale"], best["lag"]
    offset = -lag * scale / frame_rate
    reference_start = max(0.0, -offset / scale)
    reference_end = min(reference_duration, (beat_duration - offset) / scale)
    beat_start = max(0.0, reference_start * scale + offset)
    beat_end = min(beat_duration, reference_end * scale + offset)
    if reference_end - reference_start < 2 or beat_end - beat_start < 2:
        raise ValueError("Đoạn khớp quá ngắn để tạo mốc. Hãy đặt mốc thủ công.")
    anchors = [{"ref": round(reference_start, 3), "beat": round(beat_start, 3)},
               {"ref": round(reference_end, 3), "beat": round(beat_end, 3)}]

    alternatives = best["scores"].copy()
    best_index = int(np.argmin(np.abs(best["lags"] - lag)))
    radius = int(frame_rate * 10)
    alternatives[max(0, best_index - radius):min(len(alternatives), best_index + radius + 1)] = -1
    second_score = float(np.max(alternatives)) if alternatives.size else -1
    margin = max(0.0, best["score"] - second_score)
    overlap_fraction = best["overlap"] / min(reference_duration, beat_duration)
    if best["score"] >= .76 and margin >= .025 and overlap_fraction >= .65:
        confidence = "Cao"
    elif best["score"] >= .58 and margin >= .008 and overlap_fraction >= .4:
        confidence = "Vừa"
    else:
        confidence = "Thấp · cần nghe kiểm tra kỹ"

    if confidence.startswith("Thấp"):
        structure = "Cấu trúc khác hoặc chưa đủ đặc trưng để kết luận"
    elif abs(scale - 1) >= .015:
        structure = "Có thể hai bản khác tốc độ"
    elif beat_start >= 2:
        structure = "Beat có vẻ có đoạn dạo đầu dài hơn"
    elif reference_start >= 2:
        structure = "Beat có vẻ bị cắt phần đầu"
    elif overlap_fraction < .75:
        structure = "Độ dài hoặc cấu trúc hai bản có thể khác nhau"
    else:
        structure = "Có vẻ cùng cấu trúc"
    if job:
        job.update(95, "Đã tìm xong mốc đề xuất; chưa thay đổi project.")
    return {"anchors": anchors, "confidence": confidence, "score": round(best["score"], 3),
            "margin": round(margin, 3), "coverage": round(overlap_fraction, 3),
            "offset_seconds": round(offset, 3), "beat_per_reference": round(scale, 5),
            "reference_overlap": [round(reference_start, 3), round(reference_end, 3)],
            "beat_overlap": [round(beat_start, 3), round(beat_end, 3)],
            "reference_duration": round(reference_duration, 3), "beat_duration": round(beat_duration, 3),
            "structure": structure, "requires_confirmation": True}
