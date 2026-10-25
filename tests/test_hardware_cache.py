import json
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import engine
import hardware


@pytest.fixture
def probe_clock(monkeypatch):
    clock = [100.]
    monkeypatch.setattr(hardware, '_cache', None)
    monkeypatch.setattr(hardware, '_cache_time', 0.)
    monkeypatch.setattr(hardware, '_cache_failed', False)
    monkeypatch.setattr(hardware.time, 'monotonic', lambda: clock[0])
    monkeypatch.setattr(hardware, 'nvidia_cards', lambda: [])
    return clock


def probe_result(device='cuda'):
    return {'demucs': {'device': device, 'name': device},
            'whisper': {'device': device, 'name': device},
            'video': {'encoder': 'libx264', 'name': 'CPU'},
            'gpus': [], 'preferred': 'cpu' if device == 'cpu' else 'gpu'}


@pytest.mark.parametrize('device', ['cuda', 'cpu'])
def test_successful_probe_survives_idle_time_and_refreshes_on_request(monkeypatch, probe_clock, device):
    calls = []
    def run(*args, **kwargs):
        calls.append(args)
        return SimpleNamespace(returncode=0, stdout=json.dumps(probe_result(device)), stderr='')
    monkeypatch.setattr(hardware.subprocess, 'run', run)
    first = hardware.detect()
    first['whisper']['device'] = 'changed-by-caller'
    probe_clock[0] += 3600
    assert hardware.detect()['whisper']['device'] == device
    assert len(calls) == 1
    hardware.detect(refresh=True)
    assert len(calls) == 2


@pytest.mark.parametrize('failure', ['exit', 'timeout', 'empty', 'invalid'])
def test_failed_probe_retries_after_two_minutes(monkeypatch, probe_clock, failure):
    calls = []
    def run(*args, **kwargs):
        calls.append(args)
        if len(calls) == 1:
            if failure == 'timeout':
                raise subprocess.TimeoutExpired('probe', 75)
            if failure == 'exit':
                return SimpleNamespace(returncode=1, stdout='', stderr='CUDA probe failed')
            return SimpleNamespace(returncode=0, stdout='' if failure == 'empty' else '{}', stderr='')
        return SimpleNamespace(returncode=0, stdout=json.dumps(probe_result()), stderr='')
    monkeypatch.setattr(hardware.subprocess, 'run', run)
    assert hardware.detect()['whisper']['device'] == 'cpu'
    probe_clock[0] += 119
    assert hardware.detect()['whisper']['device'] == 'cpu'
    assert len(calls) == 1
    probe_clock[0] += 1
    assert hardware.detect()['whisper']['device'] == 'cuda'
    assert len(calls) == 2
    probe_clock[0] += 3600
    assert hardware.detect()['whisper']['device'] == 'cuda'
    assert len(calls) == 2


def unexpected_probe(*args, **kwargs):
    pytest.fail('This operation should not probe hardware')


def test_explicit_cpu_selection_does_not_probe(monkeypatch):
    monkeypatch.setattr(hardware, 'detect', unexpected_probe)
    selected = hardware.select_ai(preference='cpu')
    assert all(backend['device'] == 'cpu' for backend in selected.values())
    assert all(backend['reason'] == 'Đã chọn CPU' for backend in selected.values())
    with pytest.raises(ValueError):
        hardware.select_ai(preference='invalid')


def test_cached_vocals_skip_probe_and_keep_job_metadata(tmp_path, monkeypatch):
    vocal = tmp_path / 'vocals.wav'
    vocal.write_bytes(b'existing vocal')
    monkeypatch.setattr(hardware, 'detect', unexpected_probe)
    monkeypatch.setattr(engine, 'command', unexpected_probe)
    job = engine.Job('job', 'project', 'separate')
    assert engine.separate(tmp_path / 'reference.wav', tmp_path, job) == vocal
    assert job.public()['devices']['demucs']['device'] == 'cache'
    assert vocal.read_bytes() == b'existing vocal'


def test_cpu_recognition_skips_probe_and_passes_cpu_to_worker(tmp_path, monkeypatch):
    (tmp_path / 'vocals.wav').touch()
    monkeypatch.setattr(hardware, 'detect', unexpected_probe)
    calls = []
    def command(args, job=None, **kwargs):
        request = json.loads(Path(args[-1]).read_text(encoding='utf-8'))
        calls.append(request)
        Path(request['output']).write_text(json.dumps({'device': request['device'], 'words': []}), encoding='utf-8')
    monkeypatch.setattr(engine, 'command', command)
    job = engine.Job('job', 'project', 'analyze')
    assert engine.recognize(tmp_path, job, preference='cpu')['device'] == 'cpu'
    assert len(calls) == 1
    assert calls[0]['compute_type'] == 'int8'
    assert job.public()['devices']['demucs']['device'] == 'cache'
    assert job.public()['devices']['whisper']['device'] == 'cpu'
