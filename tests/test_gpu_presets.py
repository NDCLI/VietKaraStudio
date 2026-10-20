from pathlib import Path
import json
import sys

import numpy as np
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import app as service
import engine
import hardware
import presets
from styles import DEFAULT_SETTINGS
from worker import write_result


def test_numpy_model_scalars_are_saved_atomically(tmp_path):
    dest = tmp_path / 'result.json'
    write_result(dest, {'words': [{'estimated': np.bool_(True), 'start': np.float32(.5),
                                 'line': np.int64(1)}]})
    result = json.loads(dest.read_text(encoding='utf-8'))['words'][0]
    assert result == {'estimated': True, 'start': .5, 'line': 1}
    assert type(result['estimated']) is bool
    assert not dest.with_suffix('.tmp').exists()


def test_per_backend_selection_and_cpu_preference():
    probe = {'demucs': hardware.cpu_backend('No torch CUDA'),
             'whisper': {'device': 'cuda', 'name': 'Test GPU'}}
    assert hardware.select_ai(probe)['whisper']['device'] == 'cuda'
    assert hardware.select_ai(probe)['demucs']['device'] == 'cpu'
    assert all(x['device'] == 'cpu' for x in hardware.select_ai(probe, 'cpu').values())
    assert not hardware.gpu_error('TypeError: Object of type bool is not JSON serializable')


def test_recognition_gpu_failure_retries_new_cpu_worker(tmp_path, monkeypatch):
    (tmp_path / 'vocals.wav').touch()
    gpu = {'device': 'cuda', 'name': 'Test GPU', 'compute_type': 'float16', 'device_index': 0}
    monkeypatch.setattr(hardware, 'detect', lambda: {'whisper': gpu})
    calls = []
    def fake_command(args, job=None, **kw):
        request = json.loads((tmp_path/'asr_request.json').read_text(encoding='utf-8'))
        calls.append(request['device'])
        if request['device'] == 'cuda':
            raise RuntimeError('CUDA out of memory')
        assert 'lyrics' not in job.devices['whisper']
        Path(request['output']).write_text(json.dumps({'device': 'cpu', 'words': []}), encoding='utf-8')
    monkeypatch.setattr(engine, 'command', fake_command)
    job = engine.Job('j', 'p', 'analyze')
    assert engine.recognize(tmp_path, job, lyrics='Xin chào')['device'] == 'cpu'
    assert calls == ['cuda', 'cpu']
    assert job.fallbacks[0]['stage'] == 'whisper'


def test_cancellation_does_not_retry_cpu(tmp_path, monkeypatch):
    (tmp_path/'vocals.wav').touch()
    monkeypatch.setattr(hardware, 'detect', lambda: {'whisper': {'device': 'cuda', 'name': 'GPU'}})
    calls = []
    def cancel(*args, **kw):
        calls.append(1)
        raise engine.Cancelled()
    monkeypatch.setattr(engine, 'command', cancel)
    with pytest.raises(engine.Cancelled):
        engine.recognize(tmp_path, engine.Job('j','p','analyze'))
    assert len(calls) == 1


def test_presets_roundtrip_validation_and_project_application(tmp_path, monkeypatch):
    monkeypatch.setattr(engine, 'DATA', tmp_path)
    with service.app.test_client() as client:
        items = client.get('/api/presets').get_json()
        assert len(items) == 16 and all(p['builtin'] for p in items)
        assert all(p['settings']['font_size'] == 110 for p in items)
        default = next(p for p in items if p['id'] == 'vcl-default')
        assert default['settings']['font_size'] == 110
        assert default['settings']['outline_width'] == 8
        rubellite = next(p for p in items if p['id'] == 'vcl-anime_pop')
        assert rubellite['settings']['shadow'] == 14
        assert rubellite['settings']['outline_sung'] == '#3a0a6a'
        assert rubellite['settings']['outline_unsung'] == '#7b2ff7'
        assert default['settings']['sung'] == '#0ba800'
        assert default['settings']['italic'] is True
        pid = client.post('/api/projects', json={}).get_json()['id']
        response = client.patch('/api/projects/'+pid, json={'settings': default['settings']})
        assert response.status_code == 200
        assert response.get_json()['settings']['background'] == DEFAULT_SETTINGS['background']
        exported = client.get('/api/presets/vcl-default/download').get_json()
        saved = client.post('/api/presets', json=exported)
        assert saved.status_code == 201
        identifier = saved.get_json()['id']
        assert client.get('/api/presets/'+identifier+'/download').get_json() == exported
        assert client.delete('/api/presets/vcl-default').status_code == 400
        assert client.delete('/api/presets/'+identifier).status_code == 200
        exported['settings']['sung'] = 'red;bad'
        assert client.post('/api/presets', json=exported).status_code == 400
        assert client.patch('/api/projects/'+pid, json={'settings': {'font_size': float('nan')}}).status_code == 400
        assert client.post('/api/projects/'+pid+'/match').status_code == 404


def test_all_presets_reach_export_styling():
    for p in presets.catalog(Path('/no-custom-presets')):
        settings = {**DEFAULT_SETTINGS, **p['settings']}
        project = {'settings': settings, 'beat': {'duration': 3}, 'anchors': [],
                   'words': [{'text': 'Xin', 'start': .5, 'end': 1, 'line': 0}]}
        ass = engine.build_ass(project)
        assert engine.ass_color(settings['sung']) in ass
        assert engine.ass_color(settings['unsung']) in ass
        assert str(settings['outline_width']) in ass
        assert '\\t(500,1000,' in ass


def test_video_gpu_failure_retries_cpu_and_keeps_original_beat(tmp_path, monkeypatch):
    (tmp_path/'beat.wav').touch()
    monkeypatch.setattr(hardware, 'detect', lambda: {'video': {'encoder':'h264_nvenc','name':'NVENC','ffmpeg':'gpu-ffmpeg'}})
    monkeypatch.setattr(engine, 'binary', lambda name: 'cpu-ffmpeg')
    calls = []
    def run(args, job=None, **kw):
        calls.append(args)
        if len(calls) == 1: raise RuntimeError('NVENC CUDA error')
    monkeypatch.setattr(engine, 'command', run)
    job = engine.Job('j', 'p', 'render')
    result = engine.render({'id':'p','beat':{'duration':2},'anchors':[],
                            'words':[{'text':'Xin','start':.2,'end':1,'line':0}]}, tmp_path, job)
    assert result['encoder'] == 'libx264'
    assert len(calls) == 2 and calls[1][0] == 'cpu-ffmpeg'
    for args in calls:
        assert str(tmp_path/'beat.wav') in args
        assert not any('vocals.wav' in str(a) for a in args)
