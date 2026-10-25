import io
import json
from pathlib import Path
import sys
import pytest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import engine
import media_tools
from render_info import fps_value,encoding_options
from test_app import client,wait,wave_bytes


def test_render_quality_and_rational_frame_rates():
    assert abs(fps_value('60000/1001')-59.94006)<.0001
    assert fps_value('0/0')==30
    assert fps_value('nan')==30
    for encoder in ('h264_nvenc','libx264'):
        assert '12' in encoding_options('maximum',encoder)
        assert '16' in encoding_options('high',encoder)


@pytest.mark.parametrize('format',['mp4','mov','mkv'])
def test_export_manifest_original_video_fps_and_selected_folder(client,tmp_path,monkeypatch,format):
    # Test real CPU encoding and actual file delivery, without relying on an installed GPU.
    monkeypatch.setattr(engine.hardware,'detect',lambda:{'video':{'encoder':'libx264','name':'CPU'}})
    pid=client.post('/api/projects',json={'name':'Export quality'}).get_json()['id'];url='/api/projects/'+pid
    wait(client,client.post(url+'/media/beat',data={'file':(wave_bytes(660,1.2),'beat.wav')}))
    source=tmp_path/'source.mp4'
    engine.command([engine.binary('ffmpeg'),'-y','-v','error','-f','lavfi','-i','testsrc2=s=160x90:r=24','-t','1.2','-c:v','libx264',source])
    wait(client,client.post(url+'/media/background',data={'file':(io.BytesIO(source.read_bytes()),'source.mp4')}))
    client.patch(url,json={'words':[{'text':'Hát','start':.2,'end':1,'line':0}],
        'settings':{'render_quality':'high','render_fps':'source','render_format':format}})
    folder=tmp_path/'chosen';folder.mkdir()
    assert client.post(url+'/export-folder',json={'folder':str(folder)}).status_code==200
    info=client.get(url+'/render-info').get_json()
    assert info['output']['fps']==24 and info['background']['source']['width']==160
    assert info['folder']==str(folder.resolve()) and info['warnings'] and not info['errors']
    assert client.post(url+'/export-folder',json={'folder':str(tmp_path/'missing')}).status_code==400
    result=wait(client,client.post(url+'/render',json={}))['result']
    output=Path(result['output_path'])
    assert output.parent==folder and output.is_file()
    assert output.suffix=='.'+format and result['format']==format
    assert client.get(result['video']).status_code==200
    assert abs(media_tools.probe_video(output)['fps']-24)<.01
    assert not list(folder.glob('*.render-info.json'))
    assert not list(folder.glob('*.srt')) and not list(folder.glob('*.ass')) and not list(folder.glob('*.lrc'))
    assert not list(folder.glob('*.partial'))
    assert client.patch(url,json={'settings':{'render_quality':'invalid'}}).status_code==400
    assert client.patch(url,json={'settings':{'render_format':'../avi'}}).status_code==400
