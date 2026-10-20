import pytest
from test_app import client


def test_label_color_and_link_preference_survive_save_split_and_reload(client):
    pid=client.post('/api/projects',json={}).get_json()['id'];url='/api/projects/'+pid
    words=[{'text':t,'start':i+.2,'end':i+.8,'line':0,'label_color':'#E8833A'} for i,t in enumerate('Một câu hát mới'.split())]
    result=client.patch(url,json={'words':words,'text':'Một câu hát mới','settings':{'timeline_linked':False}})
    assert result.status_code==200
    result=client.get(url).get_json()
    assert result['settings']['timeline_linked'] is False
    assert all(w['label_color']=='#e8833a' for w in result['words'])
    before=result['words']
    result=client.post(url+'/autosplit',json={'text':'Một câu hát mới','max_words':2}).get_json()['project']
    assert [(w['start'],w['end'],w['label_color']) for w in result['words']]==[(w['start'],w['end'],w['label_color']) for w in before]
    assert client.patch(url,json={'words':[{**words[0],'label_color':'red'}]}).status_code==400
    assert client.patch(url,json={'settings':{'timeline_linked':'false'}}).status_code==400
