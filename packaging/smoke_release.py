"""Exercise the shipped interpreter and production HTTP server from outside its folder."""
import argparse
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def main():
    parser=argparse.ArgumentParser();parser.add_argument('folder');parser.add_argument('--report',required=True);args=parser.parse_args()
    folder=Path(args.folder).resolve();report={'folder':str(folder),'checks':[]}
    python=folder/'runtime/python.exe'
    env={**os.environ,'PYTHONNOUSERSITE':'1','PYTHONDONTWRITEBYTECODE':'1'};env.pop('PYTHONPATH',None);env.pop('PYTHONHOME',None)
    with tempfile.TemporaryDirectory(prefix='vietkara-release-') as temporary:
        data=Path(temporary);env['OBS_DATA_DIR']=str(data/'data')
        inspection=subprocess.run([str(python),'-c',
            "import sys,json,torch,ctranslate2,webview,librosa,stable_whisper,tkinter;print(json.dumps({'prefix':sys.prefix,'paths':sys.path,'torch':torch.__version__,'cuda':torch.cuda.is_available(),'ct2_cuda':ctranslate2.get_cuda_device_count()}))"],
            cwd=data,env=env,capture_output=True,text=True,encoding='utf-8',errors='replace',timeout=120)
        if inspection.returncode:raise RuntimeError(inspection.stdout+inspection.stderr)
        runtime=json.loads(inspection.stdout.splitlines()[-1]);report['runtime']=runtime
        assert Path(runtime['prefix']).resolve()==folder/'runtime'
        assert all(str(folder).lower() in str(p).lower() for p in runtime['paths'] if p)
        report['checks'].append('isolated bundled Python, torch/CUDA, CTranslate2, webview, tkinter, librosa imports')
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        base=f'http://127.0.0.1:{port}'
        log=(data/'server.log').open('w',encoding='utf-8')
        server=subprocess.Popen([str(python),str(folder/'bootstrap.py'),'--no-open','--port',str(port)],cwd=data,env=env,stdout=log,stderr=subprocess.STDOUT,creationflags=0x08000000)
        def request(path,payload=None,method=None):
            body=json.dumps(payload).encode() if payload is not None else None
            r=urllib.request.urlopen(urllib.request.Request(base+path,data=body,method=method,headers={'Content-Type':'application/json'}),timeout=120)
            return json.load(r)
        def upload(path,content,filename):
            boundary='vk'+uuid.uuid4().hex
            body=(f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{filename}"\r\nContent-Type: application/octet-stream\r\n\r\n').encode()+content+f'\r\n--{boundary}--\r\n'.encode()
            return json.load(urllib.request.urlopen(urllib.request.Request(base+path,data=body,headers={'Content-Type':'multipart/form-data; boundary='+boundary}),timeout=60))
        def wait(job):
            deadline=time.monotonic()+180
            while job['state'] in ('queued','running') and time.monotonic()<deadline:
                time.sleep(.3);job=request('/api/jobs/'+job['id'])
            assert job['state']=='done',job
            return job
        try:
            deadline=time.monotonic()+60
            while True:
                try:health=request('/api/health');break
                except (OSError,urllib.error.URLError):
                    if server.poll() is not None or time.monotonic()>deadline:
                        log.flush();raise RuntimeError((data/'server.log').read_text(encoding='utf-8'))
                    time.sleep(.3)
            assert health['ok'],health
            response=urllib.request.urlopen(base)
            assert response.headers['Server']=='VietKaraStudio'
            assert 'VietKara Studio' in response.read().decode()
            assert request('/api/projects')==[]
            report['checks'].append('Waitress local-only server; clean data; frontend and all dependencies available')
            project=request('/api/projects',{'name':'Release smoke'});pid=project['id'];url='/api/projects/'+pid
            wav=data/'tone.wav'
            subprocess.run([str(folder/'app/bin/ffmpeg.exe'),'-y','-v','error','-f','lavfi','-i','sine=frequency=660:sample_rate=44100','-t','1.3',str(wav)],check=True,creationflags=0x08000000)
            wait(upload(url+'/media/beat',wav.read_bytes(),'tone.wav'))
            patch=request(url,{'text':'Viet Kara','words':[{'text':'Viet','start':.2,'end':.7,'line':0},{'text':'Kara','start':.7,'end':1.2,'line':0}],
                               'settings':{'render_quality':'high','render_format':'mp4','font_size':110}},'PATCH')
            output=data/'chosen-output';output.mkdir()
            request(url+'/export-folder',{'folder':str(output)})
            hardware=request('/api/hardware');report['hardware']=hardware
            wait(request(url+'/render',{}))
            movie=next(output.glob('*.mp4'))
            assert movie.stat().st_size>1000
            assert next(output.glob('*.ass')).is_file() and next(output.glob('*.render-info.json')).is_file()
            report['checks'].append('FFmpeg/libass render with bundled runtime; chosen folder and sidecar delivery')
            fonts=request('/api/fonts?vietnamese=1');assert len(fonts)>0
            assert request('/api/layout',{'settings':patch['settings'],'words':patch['words'],'anchors':[]})['rows']
            assert len(request('/api/models'))==4
            report['checks'].append('Vietnamese fonts, layout API, model inventory, GPU discovery')
            diagnostic=Path(env['OBS_DATA_DIR'])/'logs/VietKaraStudio.log';assert diagnostic.is_file()
            report['checks'].append('persistent diagnostic logging')
        finally:
            server.terminate();server.wait(timeout=20);log.close()
    report['passed']=True
    Path(args.report).write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(report,ensure_ascii=True,indent=2))


if __name__=='__main__':main()
