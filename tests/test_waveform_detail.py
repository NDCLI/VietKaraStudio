from pathlib import Path
import sys
import numpy as np
import pytest
import soundfile as sf
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import engine
import app as service


def test_zoom_preserves_signed_transients_and_global_gain(tmp_path):
    path=tmp_path/'audio.wav'
    samples=np.zeros(4000,dtype='float32')
    samples[100]=.08
    samples[2001]=.8
    samples[2002]=-.4
    sf.write(path,samples,1000,subtype='FLOAT')
    quiet=engine.waveform_detail(path,0,1,100)
    zoom=engine.waveform_detail(path,2,2.01,10)
    assert max(quiet['max'])==pytest.approx(.1)
    assert max(zoom['max'])==pytest.approx(1)
    assert min(zoom['min'])==pytest.approx(-.5)
    assert zoom['start']==2 and zoom['end']==2.01
    assert zoom['step']==.001


def test_waveform_clamps_range_and_bounds_output(tmp_path):
    path=tmp_path/'audio.wav'
    sf.write(path,np.zeros(20000),1000)
    result=engine.waveform_detail(path,-10,100,1000000)
    assert result['start']==0 and result['end']==20
    assert len(result['min'])==len(result['max'])<=6000
    empty=engine.waveform_detail(path,30,40)
    assert empty['min']==empty['max']==[]
    with pytest.raises(ValueError):engine.waveform_detail(path,float('nan'))


def test_replaced_audio_invalidates_normalization_cache(tmp_path):
    path=tmp_path/'audio.wav'
    sf.write(path,np.array([.1,.8]),1000,subtype='FLOAT')
    assert engine.waveform_detail(path,0,.001,1)['max']==[.125]
    sf.write(path,np.array([.1,.4,0]),1000,subtype='FLOAT')
    assert engine.waveform_detail(path,0,.001,1)['max']==[.25]


def test_waveform_endpoint_reads_only_project_track(tmp_path,monkeypatch):
    monkeypatch.setattr(engine,'DATA',tmp_path)
    with service.app.test_client() as client:
        pid=client.post('/api/projects',json={}).get_json()['id']
        sf.write(tmp_path/'projects'/pid/'beat.wav',np.array([.5,-.25]),1000,subtype='FLOAT')
        response=client.get(f'/api/projects/{pid}/waveform/beat?start=0&end=.002&bins=2')
        assert response.status_code==200
        assert response.get_json()['min']==[1,-.5]
        assert response.headers['Cache-Control']=='no-store'
        assert client.get(f'/api/projects/{pid}/waveform/unknown').status_code==400
        assert client.get(f'/api/projects/{pid}/waveform/vocals').status_code==404
        assert client.get(f'/api/projects/{pid}/waveform/beat?start=nan').status_code==400
