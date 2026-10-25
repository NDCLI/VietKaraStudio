import io
from pathlib import Path
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import app as server
import engine
from model_manager import local_whisper


def test_incomplete_manual_model_is_not_used(tmp_path):
    folder = tmp_path / 'models/whisper/medium'
    folder.mkdir(parents=True)
    (folder / 'model.bin').write_bytes(b'incomplete')
    assert local_whisper(tmp_path, 'medium') is None
    for name in ('config.json', 'tokenizer.json'):
        (folder / name).write_text('{}')
    assert local_whisper(tmp_path, 'medium') == folder


def test_separation_downloads_are_separate_from_project_beat(tmp_path, monkeypatch):
    monkeypatch.setattr(engine, 'DATA', tmp_path)
    monkeypatch.setattr(engine, 'convert', lambda source, dest, job: dest.write_bytes(b'original input'))
    monkeypatch.setattr(server.hardware, 'detect', lambda: {})
    monkeypatch.setattr(server.hardware, 'select_ai', lambda *args, **kwargs: {'demucs': {'device': 'cpu'}})
    def separate(source, folder, job, device):
        (folder / 'vocals.wav').write_bytes(b'vocal output')
        (folder / 'reference_instrumental.wav').write_bytes(b'beat output')
    monkeypatch.setattr(engine, 'separate', separate)
    with server.app.test_client() as client:
        project = client.post('/api/projects', json={}).get_json()
        pid = project['id']
        response = client.post(f'/api/projects/{pid}/separate', data={'file': (io.BytesIO(b'audio'), 'song.wav')})
        assert response.status_code == 202
        job = response.get_json()
        for _ in range(100):
            job = client.get('/api/jobs/' + job['id']).get_json()
            if job['state'] not in ('queued', 'running'):
                break
            time.sleep(.01)
        assert job['state'] == 'done', job
        assert client.get(job['result']['beat']).data == b'beat output'
        assert client.get(job['result']['vocals']).data == b'vocal output'
        unchanged = client.get('/api/projects/' + pid).get_json()
        assert unchanged['beat'] is None and unchanged['reference'] is None and unchanged['words'] == []
        assert client.post(f'/api/projects/{pid}/models/unknown', json={}).status_code == 400
