from pathlib import Path
import io
import json
import sys

import numpy as np
from PIL import Image
import pytest
import soundfile as sf

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import app as service
import engine
import layout
from styles import validate_settings
from worker import vocal_bounds, check_alignment, repair_prefix
from test_app import client,wait,wave_bytes


def words(text, start=1):
    return [{'text':token,'start':start+i*.3,'end':start+i*.3+.25,'line':0,'role':'all','estimated':False}
            for i,token in enumerate(text.split())]


def test_measured_split_preserves_every_word_and_timestamp(client):
    text='Nhiều khi anh mong được một lần nói ra hết tất cả thay vì ngồi lặng im'
    before=words(text)
    pid=client.post('/api/projects',json={}).get_json()['id']
    response=client.patch('/api/projects/'+pid,json={'words':before,'text':text,
        'settings':{'aspect':'portrait','font_size':120,'italic':True}})
    assert response.status_code==200
    before=response.get_json()['words']
    result=client.post('/api/projects/'+pid+'/autosplit',json={'text':text}).get_json()
    after=result['project']['words']
    assert [(w['text'],w['start'],w['end']) for w in after]==[(w['text'],w['start'],w['end']) for w in before]
    assert len(set(w['line'] for w in after))>1
    assert all(len(line.split())<=5 for line in result['project']['text'].splitlines())
    response=client.post('/api/projects/'+pid+'/autosplit',json={'text':'Đã đổi nội dung'})
    assert response.status_code==400
    assert client.get('/api/projects/'+pid).get_json()['words']==after


def test_two_rows_fit_frame_and_never_lose_long_lines():
    text=('MMMM rộng nghiêng một từ nữa ')*6
    p={'settings':{'aspect':'portrait','font_size':180,'italic':True,'outline_width':10},'words':words(text)}
    plan=layout.display_plan(p)
    assert len(plan['rows'])>2
    assert plan['rows'][1]['show_start']==plan['rows'][0]['show_start']
    assert [w['text'] for r in plan['rows'] for w in r['words']]==text.split()
    for row in plan['rows']:
        assert row['slot'] in (0,1)
        assert row['show_start']<=row['start']<row['show_end']
        assert row['end']<=row['show_end']
        assert row['x']>=0 and row['x']+row['width']<=plan['width']
    huge=layout.display_plan({'settings':{'font_size':240},'words':words('W'*200)})['rows'][0]
    assert huge['font_size']<240 and huge['width']<=1920*.9+1


def test_manual_rows_sections_and_overlap_warning():
    a=words('Xin chào',2)
    b=[{**w,'line':1,'row':0} for w in words('Hát cùng',2.1)]
    plan=layout.display_plan({'words':a+b})
    assert plan['rows'][1]['slot']==0
    assert any(w['type']=='overlap' for w in plan['warnings'])
    grouped=layout.regroup_words(words('Một hai ba bốn'),'Một hai\n\nba bốn')
    assert grouped[2]['section'] is True


def test_intro_silence_trim_and_bad_alignment_guard():
    sr=16000
    rng=np.random.default_rng(4)
    samples=np.concatenate([np.zeros(sr*30),rng.normal(0,.1,sr*10),np.zeros(sr*5)]).astype(np.float32)
    a,b=vocal_bounds(samples)
    assert a/sr==pytest.approx(29.5,abs=.1)
    assert b/sr==pytest.approx(40.7,abs=.1)
    good=words(' '.join(['từ']*80))
    bad=[{**w,'start':29.6,'end':29.63} if i<30 else w for i,w in enumerate(good)]
    with pytest.raises(RuntimeError,match='Kết quả cũ'):
        check_alignment(bad,True)
    assert check_alignment(good,True)==0


def test_countdown_only_opening_and_long_instrumental_breaks():
    items=[{'text':'Hát','start':a,'end':b,'line':i,'section':True}
           for i,(a,b) in enumerate([(20,21),(27,28),(42,44),(45,46),(49,50)])]
    p={'words':items,'settings':{'countdown':'mic','countdown_seconds':4}}
    windows=layout.display_plan(p)['countdowns']
    assert len(windows)==2
    assert [w['end'] for w in windows]==pytest.approx([19.95,41.95])
    p['settings']['countdown']='none'
    assert layout.display_plan(p)['countdowns']==[]


def test_duet_overlap_does_not_create_false_instrumental_break():
    items=[{'text':'Hát','start':a,'end':b,'line':i,'row':i%2}
           for i,(a,b) in enumerate([(20,70),(60,61),(75,76)])]
    plan=layout.display_plan({'words':items,'settings':{'countdown':'mic'}})
    assert len(plan['countdowns'])==1


def test_prefix_rescue_requires_matching_phrase():
    from types import SimpleNamespace as S
    class Model:
        def transcribe_original(self,*args,**kwargs):
            segment=S(words=[S(word='Nhiều',start=.1,end=.9),S(word='khi',start=.9,end=1.2),S(word='anh',start=1.2,end=1.5)])
            return iter([segment]),S()
    sequence=[{'text':'Nhiều','start':33.2,'end':33.23},
              {'text':'khi','start':33.3,'end':33.6},{'text':'anh','start':33.6,'end':33.9}]
    assert repair_prefix(Model(),np.zeros(16000),sequence,32.3,'vi')==1
    assert sequence[0]['start']==pytest.approx(32.4)
    assert sequence[0]['end']==pytest.approx(33.2)
    other=[{**w,'text':'khác'} for w in sequence]
    other[0]['end']=other[0]['start']+.03
    assert repair_prefix(Model(),np.zeros(16000),other,32.3,'vi')==0


@pytest.mark.parametrize('mode',['karaoke','lyricsmv','subtitle'])
def test_render_modes_video_background_and_original_beat(client,tmp_path,mode):
    pid=client.post('/api/projects',json={}).get_json()['id']
    wait(client,client.post(f'/api/projects/{pid}/media/beat',data={'file':(wave_bytes(660,1.6),'beat.wav')}))
    path=engine.DATA/'projects'/pid
    background=tmp_path/'background.mp4'
    # A loud, different soundtrack in the background video must never be exported.
    engine.command([engine.binary('ffmpeg'),'-y','-v','error','-f','lavfi','-i','testsrc2=size=320x180:rate=10',
                    '-f','lavfi','-i','sine=frequency=1200:sample_rate=44100','-t','1.6',
                    '-c:v','libx264','-preset','ultrafast','-c:a','aac',background])
    wait(client,client.post(f'/api/projects/{pid}/media/background',data={'file':(io.BytesIO(background.read_bytes()),'visual.mp4')}))
    setting={'display_mode':mode,'background_type':'video','sung_gradient':True,'sweep_feather':14,
             'sweep_glow':100,'countdown':'mic','line_animation':'slowfade'}
    if mode=='subtitle':setting['resolution']='4k'
    project=client.patch(f'/api/projects/{pid}',json={'words':words('Hát cùng',.8),'settings':setting}).get_json()
    assert project['background_video']['name']=='visual.mp4'
    result=wait(client,client.post(f'/api/projects/{pid}/render',json={}))
    output=path/'exports'/result['id']/'karaoke.mp4'
    audio=tmp_path/'rendered.wav'
    engine.command([engine.binary('ffmpeg'),'-y','-v','error','-i',output,'-vn',audio])
    y,sr=sf.read(audio);y=y.mean(axis=1) if y.ndim>1 else y
    freq=np.fft.rfftfreq(len(y),1/sr)[np.argmax(np.abs(np.fft.rfft(y)))]
    assert abs(freq-660)<2
    if mode=='subtitle':
        import media_tools
        assert media_tools.probe_video(output)['width']==3840


def test_custom_countdown_png_export(client,tmp_path):
    pid=client.post('/api/projects',json={}).get_json()['id']
    wait(client,client.post(f'/api/projects/{pid}/media/beat',data={'file':(wave_bytes(660,1.6),'beat.wav')}))
    image=io.BytesIO();Image.new('RGBA',(32,32),(255,0,0,180)).save(image,'PNG');image.seek(0)
    wait(client,client.post(f'/api/projects/{pid}/media/countdown',data={'file':(image,'icon.png')}))
    assert client.patch(f'/api/projects/{pid}',json={'words':words('Xin chào',1),'settings':{'countdown':'image'}}).status_code==200
    result=wait(client,client.post(f'/api/projects/{pid}/render',json={}))
    assert result['result']['encoder'] in ('h264_nvenc','libx264')


def test_all_animation_values_produce_ass():
    from styles import ANIMATIONS
    for mode in ANIMATIONS:
        content=engine.build_ass({'words':words('Xin chào'),'settings':{'line_animation':mode,'sung_gradient':True}})
        assert 'Dialogue:' in content and '\\clip(' in content
