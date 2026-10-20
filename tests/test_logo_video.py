import io
import sys
from pathlib import Path

import numpy as np
from PIL import Image
import pytest
import soundfile as sf

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import engine
from styles import validate_settings
from test_app import client, wait, wave_bytes


@pytest.mark.parametrize('effect',['none','fade','float','pulse'])
def test_logo_export_transparency_effect_and_original_audio(client,tmp_path,effect):
    pid=client.post('/api/projects',json={}).get_json()['id']
    url=f'/api/projects/{pid}'
    wait(client,client.post(url+'/media/beat',data={'file':(wave_bytes(660,1.5),'beat.wav')}))
    # Transparent half must stay transparent in preview asset and rendered output.
    logo=Image.new('RGBA',(80,40),(0,0,0,0))
    for x in range(40):
        for y in range(40):logo.putpixel((x,y),(255,0,0,255))
    data=io.BytesIO();logo.save(data,'PNG');data.seek(0)
    wait(client,client.post(url+'/media/logo',data={'file':(data,'logo.png')}))
    loaded=client.get(url).get_json()
    assert loaded['logo_image']=={'width':80,'height':40}
    assert loaded['settings']['logo_enabled'] is True
    assert Image.open(io.BytesIO(client.get(url+'/logo').data)).getpixel((79,20))[3]==0
    if effect=='float':
        icon=io.BytesIO();Image.new('RGBA',(16,16),'yellow').save(icon,'PNG');icon.seek(0)
        wait(client,client.post(url+'/media/countdown',data={'file':(icon,'icon.png')}))
    client.patch(url,json={'words':[{'text':'Hát','start':1,'end':1.4,'line':0}],
        'settings':{'logo_size':20,'logo_x':0,'logo_y':0,'logo_effect':effect,'background_type':'color','background':'#000000'}})
    result=wait(client,client.post(url+'/render',json={}))
    output=engine.DATA/'projects'/pid/'exports'/result['id']/'karaoke.mp4'
    frame=tmp_path/'frame.png'
    engine.command([engine.binary('ffmpeg'),'-y','-v','error','-ss','0.5','-i',output,'-frames:v','1',frame])
    pixels=Image.open(frame).convert('RGB')
    assert pixels.getpixel((60,60))[0]>90
    assert max(pixels.getpixel((330,60)))<20
    audio=tmp_path/'audio.wav'
    engine.command([engine.binary('ffmpeg'),'-y','-v','error','-i',output,'-vn',audio])
    y,sr=sf.read(audio);y=y.mean(axis=1) if y.ndim>1 else y
    peak=np.fft.rfftfreq(len(y),1/sr)[np.argmax(np.abs(np.fft.rfft(y)))]
    assert abs(peak-660)<2


def test_logo_settings_validate_bounds():
    s=validate_settings({'logo_size':200,'logo_x':-20,'logo_opacity':130})
    assert (s['logo_size'],s['logo_x'],s['logo_opacity'])==(40,0,100)
    with pytest.raises(ValueError):validate_settings({'logo_effect':'unknown'})
    with pytest.raises(ValueError):validate_settings({'logo_enabled':'yes'})


def test_video_reference_and_separation_reuse_loaded_video(client,tmp_path,monkeypatch):
    pid=client.post('/api/projects',json={}).get_json()['id'];url=f'/api/projects/{pid}'
    wait(client,client.post(url+'/media/beat',data={'file':(wave_bytes(660,1.2),'original.wav')}))
    video=tmp_path/'video.mp4'
    engine.command([engine.binary('ffmpeg'),'-y','-v','error','-f','lavfi','-i','color=blue:s=160x90:r=10',
                    '-f','lavfi','-i','sine=frequency=1200:sample_rate=44100','-t','1.2','-c:v','libx264','-c:a','aac',video])
    wait(client,client.post(url+'/media/background',data={'file':(io.BytesIO(video.read_bytes()),'music.mp4')}))
    path=engine.DATA/'projects'/pid;beat_before=(path/'beat.wav').read_bytes()
    assert client.get(url).get_json()['reference'] is None
    wait(client,client.post(url+'/video-reference',json={}))
    p=client.get(url).get_json()
    assert p['reference']['name']=='music.mp4' and p['background_video']['name']=='music.mp4'
    assert p['reference']['video']['source_file']!=p['background_video']['source_file']
    def separate(source,folder,job,device):
        (folder/'reference_instrumental.wav').write_bytes(b'separated beat')
        (folder/'vocals.wav').write_bytes(b'vocal')
    monkeypatch.setattr(engine,'separate',separate)
    result=wait(client,client.post(url+'/separate',json={'source':'background','device':'cpu'}))['result']
    assert client.get(result['beat']).data==b'separated beat'
    assert (path/'beat.wav').read_bytes()==beat_before
