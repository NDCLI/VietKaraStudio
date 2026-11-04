from pathlib import Path
import sys

import numpy as np
import pytest
import soundfile as sf
from scipy.signal import resample_poly

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from audio_sync import suggest


@pytest.fixture(scope="module")
def reference_track():
    """A deterministic, non-repeating musical signal for sync fixtures."""
    sample_rate = 22050
    rng = np.random.default_rng(2301)
    note_length = int(sample_rate * .3)
    samples = np.zeros(sample_rate * 48, dtype=np.float32)
    for index, pitch in enumerate(rng.integers(48, 80, size=160)):
        start = index * note_length
        end = start + note_length
        time = np.arange(note_length, dtype=np.float32) / sample_rate
        frequency = 440 * 2 ** ((int(pitch) - 69) / 12)
        phase = rng.random() * 2 * np.pi
        envelope = np.minimum(1, time / .012) * np.exp(-time * .9)
        tone = (np.sin(2 * np.pi * frequency * time + phase) +
                .34 * np.sin(4 * np.pi * frequency * time + phase * .7) +
                .16 * np.sin(6 * np.pi * frequency * time + phase * .4))
        samples[start:end] += (.22 * envelope * tone).astype(np.float32)
    return sample_rate, samples[:, None]


def save_audio(path: Path, samples: np.ndarray, sample_rate: int) -> Path:
    sf.write(path, samples, sample_rate)
    return path


def test_sync_suggest_detects_intro_offset_and_tempo_change(tmp_path, reference_track):
    sample_rate, reference = reference_track
    reference_path = save_audio(tmp_path / "reference.wav", reference, sample_rate)

    with_intro = np.concatenate((np.zeros((6 * sample_rate, 1), dtype=np.float32), reference))
    intro_result = suggest(reference_path, save_audio(tmp_path / "intro.wav", with_intro, sample_rate))
    assert abs(intro_result["offset_seconds"] - 6) < .3
    assert abs(intro_result["beat_per_reference"] - 1) < .005
    assert intro_result["requires_confirmation"] is True

    faster = resample_poly(reference, 100, 103, axis=0).astype(np.float32)
    tempo_result = suggest(reference_path, save_audio(tmp_path / "faster.wav", faster, sample_rate))
    assert abs(tempo_result["beat_per_reference"] - .97) <= .005
    assert 0 <= tempo_result["coverage"] <= 1
    assert tempo_result["requires_confirmation"] is True


def test_sync_suggest_marks_cut_and_repeated_regions(tmp_path, reference_track):
    sample_rate, reference = reference_track
    reference_path = save_audio(tmp_path / "reference.wav", reference, sample_rate)
    cut_start, cut_end = 15 * sample_rate, 23 * sample_rate
    cut = np.concatenate((np.zeros((6 * sample_rate, 1), dtype=np.float32),
                          reference[:cut_start], reference[cut_end:]))
    cut_result = suggest(reference_path, save_audio(tmp_path / "cut.wav", cut, sample_rate))
    assert {region["status"] for region in cut_result["regions"]} & {"cut", "added"}
    assert cut_result["requires_confirmation"] is True

    repeat_start, repeat_end = 30 * sample_rate, 42 * sample_rate
    repeated = np.concatenate((np.zeros((6 * sample_rate, 1), dtype=np.float32),
                               reference[:repeat_start], reference[repeat_start:repeat_end],
                               reference[repeat_start:repeat_end], reference[repeat_end:]))
    repeated_result = suggest(reference_path, save_audio(tmp_path / "repeated.wav", repeated, sample_rate))
    assert any(region["status"] == "repeated" for region in repeated_result["regions"])
    assert any(region.get("alternate_beat_start") is not None
               for region in repeated_result["regions"] if region["status"] == "repeated")


def test_sync_suggest_identifies_unmatched_intro(tmp_path, reference_track):
    sample_rate, music = reference_track
    reference = np.concatenate((np.zeros((10 * sample_rate, 1), dtype=np.float32), music))
    reference_path = save_audio(tmp_path / "reference.wav", reference, sample_rate)
    beat_path = save_audio(tmp_path / "beat.wav", music, sample_rate)

    result = suggest(reference_path, beat_path)

    assert result["regions"][0]["status"] == "unmatched"
    assert result["requires_confirmation"] is True
