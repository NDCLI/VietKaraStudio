import copy
import json
from pathlib import Path
import sys

import numpy as np
from PIL import Image
import pytest
import soundfile as sf

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import app as service
import engine
from layout import display_plan
from styles import TITLE_EFFECTS,validate_settings
from subtitles import build_ass
from test_app import client,wait


def test_video_separation_promotes_original_beat_and_vocal_into_karaoke(client,monkeypatch):
    p=client.post('/api/projects',json={}).get_json();pid=p['id'];url='/api/projects/'+pid
    folder=engine.DATA/'projects'/pid
    sr=16000;t=np.arange(sr*2)/sr
    original=.2*np.sin(2*np.pi*300*t)
    sf.write(folder/'background_source.wav',original,sr)
    p=service.load(pid);p['background_video']={'name':'song.mp4','source_file':'background_source.wav'}
    p['words']=[{'text':'Old','line':0,'start':0,'end':1,'role':'all'}];p['anchors']=[{'ref':0,'beat':1}];service.save(p)
    def separate(source,dest,job,device):
        sf.write(dest/'vocals.wav',.2*np.sin(2*np.pi*600*t),sr)
        sf.write(dest/'reference_instrumental.wav',.2*np.sin(2*np.pi*900*t),sr)
    monkeypatch.setattr(engine,'separate',separate)
    result=wait(client,client.post(url+'/separate',json={'source':'background','apply_to_project':True,'device':'cpu'}))['result']
    assert result['applied']
    after=client.get(url).get_json()
    assert after['vocals_ready'] and after['vocals']['peaks']
    assert after['reference']['duration']==pytest.approx(2)
    assert after['beat']['duration']==pytest.approx(2)
    assert after['words']==[] and after['anchors']==[]
    assert (folder/'reference.wav').is_file() and (folder/'beat.wav').is_file()
    assert client.get(url+'/audio/vocals').status_code==200
    frequencies=[]
    for name in ('reference.wav','beat.wav','vocals.wav'):
        samples,rate=sf.read(folder/name)
        if samples.ndim>1:samples=samples.mean(axis=1)
        frequencies.append(np.argmax(np.abs(np.fft.rfft(samples)))*rate/len(samples))
    assert frequencies==pytest.approx([300,900,600],abs=1)


@pytest.mark.parametrize('enter',TITLE_EFFECTS)
@pytest.mark.parametrize('exit',TITLE_EFFECTS)
def test_title_layout_and_export_support_independent_effects_and_do_not_change_words(enter,exit):
    p={'settings':{'title_enabled':True,'title_text':'Tên bài hát rất dài '*8,'title_credit':'Ca sĩ · Tác giả',
                   'title_enter':enter,'title_exit':exit,'title_start':1,'title_end':7},
       'beat':{'duration':6},'words':[{'text':'Lời','start':1,'end':2,'line':0,'role':'all'}]}
    before=copy.deepcopy(p);plan=display_plan(p);title=plan['title']
    assert title['end']==6
    assert all(row['x']>=0 and row['x']+row['width']<=plan['width'] for row in title['rows'])
    content=build_ass(p)
    events=[line for line in content.splitlines() if line.startswith('Dialogue: 10,')]
    assert len(events)==4 and 'Ca sĩ · Tác giả' in content
    assert p==before
    for event in events:assert '\\fscx' in event and '\\fad(' in event


def test_title_validation_and_disabled_projects_stay_compatible():
    assert display_plan({'settings':{},'words':[]})['title'] is None
    with pytest.raises(ValueError):validate_settings({'title_start':4,'title_end':3})
    with pytest.raises(ValueError):validate_settings({'title_enter':'unknown'})
    with pytest.raises(ValueError):validate_settings({'title_enabled':'yes'})


def test_rendered_shadow_survives_completed_word(tmp_path):
    p={'settings':{'shadow':12,'shadow_color':'#ff0000','font_size':150,'outline_width':3,
                   'sung':'#ffffff','unsung':'#ffffff','countdown':'none','line_animation':'none'},
       'words':[{'text':'Bóng chữ','start':.4,'end':1.6,'line':0,'role':'all'}]}
    (tmp_path/'lyrics.ass').write_text(build_ass(p),encoding='utf-8-sig')
    for label,at in [('early',.5),('completed',1.9)]:
        engine.command([engine.binary('ffmpeg'),'-y','-v','error','-f','lavfi','-i','color=c=black:s=640x360:r=10:d=3',
                        '-vf','ass=lyrics.ass','-ss',str(at),'-frames:v','1',label+'.png'],cwd=tmp_path)
        pixels=np.array(Image.open(tmp_path/(label+'.png')).convert('RGB'))
        red=(pixels[:,:,0]>130)&(pixels[:,:,1]<70)&(pixels[:,:,2]<70)
        assert red.sum()>30,f'Shadow vanished in {label} frame'
