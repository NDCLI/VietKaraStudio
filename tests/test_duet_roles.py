from pathlib import Path
import sys

import numpy as np
import soundfile as sf

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import app as service
import engine
from duet_roles import suggest,analyze
from test_app import client,wait


def test_single_voice_melody_is_not_automatically_split():
    times=np.arange(0,8,.064)
    segments=[{'start':0,'end':4},{'start':4,'end':8}]
    for pitch in (np.full(len(times),160.),np.exp2(np.linspace(np.log2(110),np.log2(300),len(times))),np.full(len(times),np.nan)):
        result=suggest(times,pitch,segments)
        assert not result['two_registers'] and result['roles']==[None,None]


def test_two_registers_mixed_line_and_missing_audio():
    times=np.arange(0,12,.064);pitch=np.where(times<6,120.,260.)
    segments=[{'start':0,'end':4},{'start':8,'end':10},{'start':4,'end':8},{'start':13,'end':14}]
    result=suggest(times,pitch,segments)
    assert result['two_registers']
    assert result['roles']==['male','female','all',None]


def test_real_vocal_analysis_preserves_words_anchors_and_times(client,monkeypatch):
    pid=client.post('/api/projects',json={}).get_json()['id'];url='/api/projects/'+pid
    assert client.post(url+'/suggest-roles',json={}).status_code==400
    project=service.load(pid);folder=engine.DATA/'projects'/pid
    sr=16000;t=np.arange(sr*8)/sr;frequency=np.where(t<4,120,260)
    phase=2*np.pi*np.cumsum(frequency)/sr
    y=.3*np.sin(phase)+.15*np.sin(phase*2)+.07*np.sin(phase*3)
    sf.write(folder/'reference.wav',y,sr);sf.write(folder/'vocals.wav',y,sr)
    project['reference']={'name':'voices.wav','duration':8,'peaks':[]};service.save(project)
    sequence=[{'text':'Trầm','start':.5,'end':2.5,'line':0,'role':'all'},
              {'text':'Cao','start':4.5,'end':6.5,'line':1,'role':'all'},
              {'text':'Chung','start':2.5,'end':5.5,'line':2,'role':'all'},
              {'text':'Giữ','start':9,'end':10,'line':3,'role':'female'}]
    before=client.patch(url,json={'words':sequence,'text':'Trầm\nCao\nChung\nGiữ','anchors':[{'ref':0,'beat':20}]}).get_json()
    def no_separation(*args,**kwargs):raise AssertionError('must reuse existing vocal')
    monkeypatch.setattr(engine,'separate',no_separation)
    result=wait(client,client.post(url+'/suggest-roles',json={}))['result']
    assert result['roles']==['male','female','all',None]
    after=client.get(url).get_json()
    assert [{k:v for k,v in w.items() if k!='role'} for w in after['words']]==[{k:v for k,v in w.items() if k!='role'} for w in before['words']]
    assert after['text']==before['text'] and after['anchors']==before['anchors']
    assert [w['role'] for w in after['words']]==['male','female','all','female']
    # A later line edit reuses the full-track pitch cache, not stale line decisions.
    import librosa
    monkeypatch.setattr(librosa,'pyin',no_separation)
    cached=analyze(folder/'vocals.wav',[{'start':4.5,'end':6.5},{'start':.5,'end':2.5}])
    assert cached['roles']==['female','male']
