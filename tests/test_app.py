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
    video=engine.DATA/'projects'/pid/'exports'/result['id']/'karaoke.mp4'
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
