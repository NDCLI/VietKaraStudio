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


def _local_regions(reference, beat, scale, lag, frame_rate, job=None):
    """Inspect short local windows around the global map without auto-applying edits."""
    import numpy as np

    # Use the same resampled beat timebase as the global alignment. This makes
    # a reference window and its candidate beat window directly comparable.
    transformed_count = max(1, int(math.ceil((len(beat) - 1) / scale)) + 1)
    source_positions = np.arange(transformed_count, dtype=np.float32) * scale
    left = np.minimum(source_positions.astype(np.int32), len(beat) - 1)
    right = np.minimum(left + 1, len(beat) - 1)
    weight = (source_positions - left).astype(np.float32)[:, None]
    beat = beat[left] * (1 - weight) + beat[right] * weight

    window = max(32, round(8 * frame_rate))
    if len(reference) < window or len(beat) < window:
        return []
    stride = max(window, round(15 * frame_rate))
    starts = list(range(0, len(reference) - window + 1, stride))
    last = len(reference) - window
    if not starts or last - starts[-1] > window / 2:
        starts.append(last)
    if len(starts) > 240:
        stride = math.ceil(last / 239)
        starts = list(range(0, last + 1, stride))
        if starts[-1] != last:
            starts.append(last)

    radius = round(45 * frame_rate / scale)
    candidate_step = max(1, round(.25 * frame_rate / scale))
    offsets = np.arange(window, dtype=np.int32)[None, :]
    regions = []
    for number, ref_start in enumerate(starts):
        if job:
            job.check()
        predicted = ref_start - lag
        low = max(0, predicted - radius)
        high = min(len(beat) - window, predicted + radius)
        if high < low:
            regions.append({"reference_start": round(ref_start / frame_rate, 2),
                            "reference_end": round((ref_start + window) / frame_rate, 2),
                            "beat_start": None, "beat_end": None, "similarity": 0.,
                            "offset_seconds": None, "status": "unmatched",
                            "note": "Không có đủ vùng chồng để so khớp."})
            continue
        candidates = np.arange(low, high + 1, candidate_step, dtype=np.int32)
        if candidates[-1] != high:
            candidates = np.append(candidates, high)
        query = reference[ref_start:ref_start + window]
        windows = beat[candidates[:, None] + offsets, :]
        scores = np.einsum("cwf,wf->c", windows, query, optimize=True) / window
        best_score = float(scores.max())
        near_best = np.flatnonzero(scores >= best_score - .015)
        chosen_index = min(near_best, key=lambda item: abs(int(candidates[item]) - predicted))
        chosen = int(candidates[chosen_index])
        distant = np.abs(candidates - chosen) >= round(6 * frame_rate / scale)
        alternate_beat_start = None
        if np.any(distant):
            distant_indices = np.flatnonzero(distant)
            alternate_index = int(distant_indices[np.argmax(scores[distant])])
            second_score = float(scores[alternate_index])
            alternate_beat_start = round(int(candidates[alternate_index]) * scale / frame_rate, 2)
        else:
            second_score = -1.
        margin = best_score - second_score
        deviation = (chosen - predicted) * scale / frame_rate
        ref_end = (ref_start + window) / frame_rate
        item = {"reference_start": round(ref_start / frame_rate, 2),
                "reference_end": round(ref_end, 2),
                "beat_start": round(chosen * scale / frame_rate, 2),
                "beat_end": round((chosen + window) * scale / frame_rate, 2),
                "similarity": round(best_score, 3),
                "offset_seconds": round(deviation, 2),
                "margin": round(max(0., margin), 3)}
        if alternate_beat_start is not None:
            item["alternate_beat_start"] = alternate_beat_start
        if best_score < .38:
            item.update(status="unmatched", note="Không tìm thấy vùng beat khớp rõ; có thể là nhạc dạo hoặc khác cấu trúc.")
        elif margin < .035 and best_score >= .55:
            item.update(status="repeated", note="Có nhiều vùng beat tương tự; có thể là điệp khúc lặp. Hãy nghe xác nhận.")
        elif margin < .012:
            item.update(status="uncertain", note="Có nhiều vùng âm thanh tương tự; cần nghe xác nhận.")
        elif abs(deviation) >= 5:
            item.update(status="shifted", note="Đoạn này lệch khỏi mốc tuyến tính toàn bài.")
        else:
            item.update(status="matched", note="Khớp gần với mốc tuyến tính toàn bài.")
        regions.append(item)
        if job and number % 8 == 0:
            job.update(88 + 6 * number / max(1, len(starts)),
                       f"Đang kiểm tra từng đoạn bản hát / beat · {number + 1}/{len(starts)}…")

    for previous, current in zip(regions, regions[1:]):
        if (previous.get("status") in ("matched", "shifted") and
                current.get("status") in ("matched", "shifted") and
                previous.get("offset_seconds") is not None and current.get("offset_seconds") is not None and
                abs(current["offset_seconds"] - previous["offset_seconds"]) >= 7):
            delta = current["offset_seconds"] - previous["offset_seconds"]
            current["status"] = "added" if delta > 0 else "cut"
            current["note"] = ("Độ lệch tăng đột ngột; có thể beat được thêm hoặc lặp một đoạn. Hãy nghe xác nhận."
                               if delta > 0 else
                               "Độ lệch giảm đột ngột; có thể beat bị cắt một đoạn. Hãy nghe xác nhận.")
    return regions


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
    aligned_beat_duration = best["transformed_length"] / frame_rate
    overlap_fraction = min(1.0, best["overlap"] / min(reference_duration, aligned_beat_duration))
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
    regions = _local_regions(reference, beat, scale, lag, frame_rate, job)
    added_or_cut = sum(item["status"] in ("added", "cut") for item in regions)
    repeated_regions = sum(item["status"] == "repeated" for item in regions)
    unmatched_regions = sum(item["status"] == "unmatched" for item in regions)
    if added_or_cut:
        structure += " · có vùng có thể bị thêm hoặc cắt"
    elif repeated_regions:
        structure += " · có vùng có thể lặp cần xác nhận"
    if job:
        job.update(95, "Đã rà mốc toàn bài và từng vùng; chưa thay đổi project.")
    return {"anchors": anchors, "confidence": confidence, "score": round(best["score"], 3),
            "margin": round(margin, 3), "coverage": round(overlap_fraction, 3),
            "offset_seconds": round(offset, 3), "beat_per_reference": round(scale, 5),
            "reference_overlap": [round(reference_start, 3), round(reference_end, 3)],
            "beat_overlap": [round(beat_start, 3), round(beat_end, 3)],
            "reference_duration": round(reference_duration, 3), "beat_duration": round(beat_duration, 3),
            "structure": structure, "regions": regions,
            "structural_regions": added_or_cut,
            "repeated_regions": repeated_regions,
            "unmatched_regions": unmatched_regions,
            "uncertain_regions": sum(item["status"] == "uncertain" for item in regions),
            "requires_confirmation": True}
