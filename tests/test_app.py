from pathlib import Path
import io
import json
import subprocess
import sys
import time

import numpy as np
import pytest
import soundfile as sf

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import engine
import app as service
import backing_vocals


@pytest.fixture
def client(tmp_path,monkeypatch):
    monkeypatch.setattr(engine,'DATA',tmp_path)
    service.app.config.update(TESTING=True)
    with service.app.test_client() as client:
        yield client
    # Jobs must finish before the fixture removes its project directory.
    for job in list(service.JOBS.values()):
        deadline=time.monotonic()+30
        while job.state in ('running','queued') and time.monotonic()<deadline:
            time.sleep(.05)


def wait(client,response):
    assert response.status_code==202,response.get_json()
    jid=response.get_json()['id']
    deadline=time.monotonic()+60
    while time.monotonic()<deadline:
        job=client.get('/api/jobs/'+jid).get_json()
        if job['state'] in ('done','error','cancelled'):
            assert job['state']=='done',job
            return job
        time.sleep(.05)
    pytest.fail('Job did not finish')


def wave_bytes(frequency=660,duration=2.4):
    sr=44100
    t=np.arange(round(sr*duration))/sr
    wav=io.BytesIO()
    sf.write(wav,.35*np.sin(2*np.pi*frequency*t),sr,format='WAV')
    wav.seek(0)
    return wav


def test_project_creation_validation_and_local_origin(client):
    result=client.post('/api/projects',json={})
    assert result.status_code==201
    project=result.get_json()
    assert project['beat_words']==[]
    pid=project['id']
    assert client.patch('/api/projects/'+pid,json={'anchors':[{'ref':0,'beat':4},{'ref':3,'beat':2}]}).status_code==400
    assert client.post('/api/projects',json={},headers={'Origin':'https://unrelated.example'}).status_code==403
    assert client.get('/api/projects/not-a-project').status_code==400


def test_backing_review_alignment_and_render(client, monkeypatch, tmp_path):
    project = client.post('/api/projects', json={'name': 'Backing mix'}).get_json()
    url = '/api/projects/' + project['id']
    path = engine.DATA / 'projects' / project['id']
    wait(client, client.post(url + '/media/beat', data={'file': (wave_bytes(440, 4), 'beat.wav')}))
    (path / 'vocals.wav').write_bytes(wave_bytes(1200, 4).getvalue())
    beat = (path / 'beat.wav').read_bytes()
    client.patch(url, json={'anchors': [{'ref': 0, 'beat': .4}],
                            'words': [{'text': 'Hát', 'start': .2, 'end': 1, 'line': 0}]})

    commands = []
    command = engine.command

    def separate(args, *extra, **options):
        if 'backing' in args and '--vcl-task' in args:
            commands.append(args)
            Path(args[-3]).write_bytes(wave_bytes(880, 3).getvalue())
        else:
            return command(args, *extra, **options)

    monkeypatch.setattr(engine, 'command', separate)
    monkeypatch.setattr(engine.hardware, 'detect', lambda: {'video': {'encoder': 'libx264', 'name': 'CPU'}})

    response = wait(client, client.post(url + '/backing', json={'device': 'cpu'}))
    assert response['result']['backing_status'] == 'review'
    state = client.get(url).get_json()
    key = state['backing_vocals']['mix']
    assert client.post(url + '/backing/review', json={'accepted': True, 'mix': 'old'}).status_code == 400
    candidate = client.get(url + '/audio/beat_backing')
    assert candidate.status_code == 200
    mixed, rate = sf.read(io.BytesIO(candidate.data))
    assert len(mixed) == 4 * rate
    assert (path / 'beat.wav').read_bytes() == beat

    def amplitude(samples, frequency):
        return abs(np.dot(samples, np.exp(-2j * np.pi * frequency * np.arange(len(samples)) / rate))) * 2 / len(samples)

    assert amplitude(mixed[:rate // 4, 0], 880) < .001
    assert amplitude(mixed[rate:2 * rate, 0], 880) > .1
    assert amplitude(mixed[rate:2 * rate, 0], 1200) < .001

    client.patch(url, json={'settings': {'audio_output': 'backing'}})
    assert client.post(url + '/render').status_code == 400
    assert client.post(url + '/backing/review', json={'accepted': True, 'mix': key}).get_json()['backing_status'] == 'ready'
    job = wait(client, client.post(url + '/render', json={'preview': {'start': 1, 'duration': 1}}))
    video = tmp_path / 'export.mp4'
    video.write_bytes(client.get(job['result']['video']).data)
    exported = tmp_path / 'export.wav'
    command([engine.binary('ffmpeg'), '-v', 'error', '-i', video, '-vn', '-ar', rate, exported])
    soundtrack, _ = sf.read(exported)
    assert amplitude(soundtrack[:rate // 2, 0], 440) > .1
    assert amplitude(soundtrack[:rate // 2, 0], 880) > .1
    assert amplitude(soundtrack[:rate // 2, 0], 1200) < .005
    approved = service.load(project['id'])
    mix_file = backing_vocals.mix_file(approved, path)
    batch = service.batch_render.create(engine.DATA, [(approved, path)], project['id'],
                                        audio_outputs=['beat', 'backing'])
    beat_item, backing_item = batch['items']
    assert beat_item['assets'] == ['beat.wav']
    assert backing_item['variant']['audio'] == 'backing'
    assert backing_item['assets'] == ['beat.wav', mix_file.name]
    frozen_mix = Path(backing_item['directory']) / mix_file.name
    assert frozen_mix.read_bytes() == candidate.data
    bundle = client.get(url + '/download/bundle-full')
    restored = client.post('/api/projects/import-bundle', data={'file': (io.BytesIO(bundle.data), 'backing.zip')})
    assert restored.status_code == 201
    assert restored.get_json()['backing_status'] == 'ready'

    updated = client.patch(url, json={'anchors': [{'ref': 0, 'beat': .8}]}).get_json()
    assert updated['backing_status'] == 'stale'
    assert client.get(url + '/audio/beat_backing').status_code == 404
    assert wait(client, client.post(url + '/backing'))['result']['backing_status'] == 'review'
    assert len(commands) == 1
    new_key = client.get(url).get_json()['backing_vocals']['mix']
    assert key != new_key
    assert frozen_mix.read_bytes() == candidate.data
    rejected = client.post(url + '/backing/review', json={'accepted': False, 'mix': new_key}).get_json()
    assert rejected['backing_status'] == 'weak'
    assert client.get(url + '/audio/beat_backing').status_code == 404
    assert client.patch(url, json={'anchors': []}).get_json()['backing_status'] == 'stale'


def test_backing_silent_source_cannot_be_confirmed(client, monkeypatch):
    project = client.post('/api/projects', json={}).get_json()
    url = '/api/projects/' + project['id']
    wait(client, client.post(url + '/media/beat', data={'file': (wave_bytes(), 'beat.wav')}))
    path = engine.DATA / 'projects' / project['id']
    sf.write(path / 'vocals.wav', np.zeros(44100), 44100)

    def unexpected_model(*args, **kwargs):
        pytest.fail('Silence should be rejected before invoking the model')

    monkeypatch.setattr(engine, 'command', unexpected_model)
    job = wait(client, client.post(url + '/backing'))
    assert job['result']['backing_status'] == 'weak'
    assert client.get(url + '/audio/beat_backing').status_code == 404
    assert client.post(url + '/backing/review', json={'accepted': True, 'mix': 'missing'}).status_code == 400


def test_separation_tool_backing_uses_isolated_result_and_requires_review(client, monkeypatch):
    project = client.post('/api/projects', json={}).get_json()
    url = '/api/projects/' + project['id']
    command = engine.command

    def separate(reference, folder, job, device):
        (folder / 'vocals.wav').write_bytes(wave_bytes(1200).getvalue())
        (folder / service.TOOL_BEAT).write_bytes(wave_bytes(440).getvalue())

    def command_with_model(args, *extra, **options):
        if 'backing' in args and '--vcl-task' in args:
            Path(args[-3]).write_bytes(wave_bytes(880).getvalue())
            return
        return command(args, *extra, **options)

    monkeypatch.setattr(engine, 'separate', separate)
    monkeypatch.setattr(engine, 'command', command_with_model)
    monkeypatch.setattr(engine.hardware, 'select_ai', lambda **_: {'demucs': engine.hardware.cpu_backend()})
    separated = wait(client, client.post(url + '/separate', data={'file': (wave_bytes(), 'input.wav')}))['result']
    tool_url = '/api/tools/' + separated['id']
    beat = client.get(separated['beat']).data
    assert client.get(tool_url + '/beat_backing?preview=1').status_code == 404
    prepared = wait(client, client.post(url + '/separate/' + separated['id'] + '/backing'))['result']
    assert prepared['status'] == 'review'
    assert client.get(prepared['mix']).status_code == 404
    preview = client.get(prepared['mix'] + '?preview=1').data
    assert preview != beat
    assert client.get(tool_url + '/backing').status_code == 404
    assert client.post(tool_url + '/backing-review', json={'accepted': True}).get_json()['status'] == 'ready'
    assert client.get(prepared['mix']).data == preview
    assert client.get(tool_url + '/backing').data == wave_bytes(880).getvalue()
    assert client.get(separated['beat']).data == beat
    assert client.get(url).get_json()['beat'] is None


def test_backing_alignment_across_multiple_anchors(tmp_path):
    rate = backing_vocals.SAMPLE_RATE
    stem = tmp_path / 'backing.wav'
    beat = tmp_path / 'beat.wav'
    mixed = tmp_path / 'mix.wav'
    anchors = [{'ref': .1, 'beat': .3}, {'ref': 1.5, 'beat': 1.7}, {'ref': 3, 'beat': 3.5}]
    samples = np.zeros(4 * rate)
    times = [.5, 1.1, 1.9, 2.8]
    for point in times:
        start = round(point * rate)
        samples[start:start + 2500] = np.hanning(2500)
    sf.write(stem, samples, rate)
    sf.write(beat, np.zeros(5 * rate), rate)
    backing_vocals.prepare_mix(beat, stem, anchors, mixed, engine.Job('test', 'test', 'backing'))
    output, _ = sf.read(mixed)
    assert len(output) == 5 * rate
    expected = [.7, 1.3, 2.18, 3.26]
    for point in expected:
        region = output[round((point - .035) * rate):round((point + .1) * rate)]
        assert np.max(np.abs(region)) > .2


def test_backup_relink_keeps_timing_and_anchors(client):
    data={'schema':1,'name':'Bản lưu','words':[{'text':'Xin','start':.4,'end':.8,'line':0}],
          'anchors':[{'ref':0,'beat':.1}],'text':'Xin'}
    response=client.post('/api/projects/import',json=data)
    assert response.status_code==201
    pid=response.get_json()['id']
    for slot in ('reference','beat'):
        wait(client,client.post(f'/api/projects/{pid}/media/{slot}',data={'file':(wave_bytes(),'source.wav')}))
    project=client.get('/api/projects/'+pid).get_json()
    assert project['anchors']==data['anchors']
    assert project['words'][0]['start']==.4
    assert project['beat_words'][0]['start']==pytest.approx(.5)


def test_export_uses_only_original_beat_and_mapped_lyrics(client,tmp_path):
    pid=client.post('/api/projects',json={'name':'Kiểm thử beat gốc'}).get_json()['id']
    wait(client,client.post(f'/api/projects/{pid}/media/beat',data={'file':(wave_bytes(660),'original_beat.wav')}))
    path=engine.DATA/'projects'/pid
    # Distinct, loud decoy reference and stems must not appear in the exported soundtrack.
    for name in ('reference.wav','vocals.wav','reference_instrumental.wav'):
        (path/name).write_bytes(wave_bytes(1200).getvalue())
    response=client.patch(f'/api/projects/{pid}',json={
        'settings': next(p['settings'] for p in client.get('/api/presets').get_json() if p['id']=='vcl-retro_vien_doi'),
        'words':[{'text':'Xin','start':.2,'end':.6,'line':0},
                 {'text':'{chào}\\','start':.6,'end':1.,'line':0}],
        'anchors':[{'ref':0,'beat':.3}]})
    assert response.status_code==200,response.get_json()
    srt=client.get(f'/api/projects/{pid}/download/srt').data.decode('utf-8-sig')
    assert '00:00:00,500 --> 00:00:01,300' in srt
    ass=client.get(f'/api/projects/{pid}/download/ass').data.decode('utf-8-sig')
    assert '｛chào｝＼' in ass
    result=wait(client,client.post(f'/api/projects/{pid}/render',json={}))
    assert result['result']['encoder'] in ('h264_nvenc','libx264')
    video=Path(result['result']['output_path'])
    assert video.is_file() and video.stat().st_size>1000
    soundtrack=tmp_path/'soundtrack.wav'
    engine.command([engine.binary('ffmpeg'),'-y','-v','error','-i',video,'-vn',soundtrack])
    y,sr=sf.read(soundtrack)
    if y.ndim>1:y=y.mean(axis=1)
    spec=np.abs(np.fft.rfft(y))
    freq=np.fft.rfftfreq(len(y),1/sr)
    main=freq[int(np.argmax(spec))]
    assert abs(main-660)<2  # Rendered audio is the original beat, not a stem or reference.
    assert client.get(result['result']['video']).status_code==200
