"""Exercise compiled workers and HTTP export without modifying user projects or downloading models."""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))


def main():
    sys.stdout.reconfigure(encoding='utf-8',errors='replace')
    parser=argparse.ArgumentParser();parser.add_argument('folder');parser.add_argument('--report',required=True)
    parser.add_argument('--audio',type=Path);parser.add_argument('--start',type=float,default=0)
    parser.add_argument('--demucs-model',type=Path)
    parser.add_argument('--download-demucs',action='store_true',help='Exercise a fresh official checkpoint download')
    args=parser.parse_args()
    folder=Path(args.folder).resolve();exe=folder/'VietKaraStudio.exe'
    report={'folder':str(folder),'checks':[],'timings':{}}
    assert not list(folder.rglob('python.exe')) and not list(folder.rglob('pythonw.exe'))
    report['checks'].append('No python.exe/pythonw.exe in payload')
    with tempfile.TemporaryDirectory(prefix='vietkara-nuitka-smoke-') as temporary:
        work=Path(temporary);data=work/'data'
        env={**os.environ,'OBS_DATA_DIR':str(data),'HF_HUB_OFFLINE':'1','PYTHONNOUSERSITE':'1'}
        env.pop('PYTHONPATH',None);env.pop('PYTHONHOME',None)
        def task(name,*argv,timeout=180,label=None):
            print('Checking compiled task: '+name,flush=True)
            started=time.perf_counter()
            result=subprocess.run([str(exe),'--vcl-task',name,*(str(v) for v in argv)],cwd=work,env=env,
                                  capture_output=True,text=True,encoding='utf-8',errors='replace',timeout=timeout,
                                  creationflags=0x08000000)
            key=label or name
            report['timings'][key]=round(time.perf_counter()-started,3)
            print(key+': '+str(report['timings'][key])+'s',flush=True)
            if result.returncode:raise RuntimeError(name+' failed:\n'+result.stdout+'\n'+result.stderr)
            return result.stdout
        def last_json(text):return json.loads(text.strip().splitlines()[-1])
        diagnostics=last_json(task('diagnose'))
        assert diagnostics['compiled'] and all(diagnostics['compiled_modules'].values()),diagnostics
        assert Path(diagnostics['executable']).resolve()==exe
        assert all(Path(p).resolve().is_relative_to(folder) for p in diagnostics['paths'] if p),diagnostics['paths']
        report['diagnostics']=diagnostics;report['checks'].append('Compiled application modules; isolated vendor imports')
        hardware=last_json(task('probe'));report['hardware']=hardware
        assert hardware['whisper']['device']=='cuda' and hardware['demucs']['device'].startswith('cuda'),hardware
        report['checks'].append('Compiled GPU probe finds CUDA for speech and separation')
        picker=last_json(task('pick-folder','--check'));assert picker['available']
        report['checks'].append('Compiled native folder helper initializes Tk without a modal')
        # Cached model lookup must be offline and must not need a Python subprocess.
        task('model',data,'small');report['checks'].append('Compiled cached model task works offline')
        if args.download_demucs:
            task('model',data,'demucs','--fresh',label='demucs_download')
            import model_manager
            assert model_manager.valid_demucs(data/'models'/model_manager.DEMUCS_FILE)
            report['checks'].append('Fresh official Demucs download through the EXE passes SHA-256')
        ffmpeg=folder/'bin/ffmpeg.exe'
        vocal=work/'vocal.wav'
        if args.audio:
            subprocess.run([str(ffmpeg),'-y','-v','error','-ss',str(args.start),'-i',str(args.audio.resolve()),
                            '-t','8','-ar','44100','-ac','2',str(vocal)],check=True,creationflags=0x08000000)
        else:
            subprocess.run([str(ffmpeg),'-y','-v','error','-f','lavfi','-i','sine=frequency=180:sample_rate=16000',
                            '-t','4',str(vocal)],check=True,creationflags=0x08000000)
        if args.demucs_model:
            repo=data/'models';repo.mkdir(parents=True,exist_ok=True)
            shutil.copy2(args.demucs_model.resolve(),repo/args.demucs_model.name)
            task('demucs','--two-stems','vocals','--float32','--shifts','1','--jobs','1',
                 '--repo',repo,'-n',args.demucs_model.stem.split('-')[0],'-d','cuda:0','-o',work/'stems',vocal,timeout=300)
            assert list((work/'stems').rglob('vocals.wav'))
            report['checks'].append('Real compiled Demucs task produces both stems on GPU')
        # Exercise vendor Numba JIT through a compiled role-analysis function.
        import numpy as np
        import soundfile as sf
        sr=16000;t=np.arange(sr*8)/sr;freq=np.where(t<4,120,260);phase=2*np.pi*np.cumsum(freq)/sr
        role_vocal=work/'roles.wav';sf.write(role_vocal,.3*np.sin(phase)+.15*np.sin(phase*2)+.07*np.sin(phase*3),sr)
        request=work/'roles-request.json';result=work/'roles-result.json'
        request.write_text(json.dumps({'vocal':str(role_vocal),'segments':[{'start':.5,'end':2.5},{'start':4.5,'end':6.5}],
                                      'output':str(result)}),encoding='utf-8')
        task('roles',request,timeout=180)
        roles=json.loads(result.read_text());assert roles['roles']==['male','female'],roles
        report['checks'].append('Compiled role analysis retains working librosa/Numba JIT')
        if args.audio:
            from model_manager import resolve_whisper
            model=resolve_whisper(ROOT/'data','small');assert model
            speech_request=work/'speech-request.json';speech_result=work/'speech-result.json'
            request_data={'vocals':str(vocal),'model':'small','models_root':str(ROOT/'data'),'language':'vi',
                          'device':'cuda','device_index':hardware['whisper']['device_index'],
                          'compute_type':'float16','output':str(speech_result),'name':hardware['whisper']['name']}
            speech_request.write_text(json.dumps(request_data),encoding='utf-8')
            task('speech',speech_request,timeout=240,label='transcription')
            heard=json.loads(speech_result.read_text(encoding='utf-8'));assert heard['words'],heard
            report['recognized_words']=len(heard['words'])
            request_data['lyrics']=heard['text'];speech_request.write_text(json.dumps(request_data),encoding='utf-8')
            task('speech',speech_request,timeout=240,label='alignment')
            aligned=json.loads(speech_result.read_text(encoding='utf-8'));assert aligned['words']
            report['checks'].append('Real compiled Whisper GPU transcription and supplied-lyrics alignment')
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        base=f'http://127.0.0.1:{port}';server_log=(work/'server.log').open('w',encoding='utf-8')
        server=subprocess.Popen([str(exe),'--no-open','--port',str(port)],cwd=work,env=env,
                                stdout=server_log,stderr=subprocess.STDOUT,creationflags=0x08000000)
        def request_api(path,payload=None,method=None):
            body=json.dumps(payload).encode() if payload is not None else None
            return json.load(urllib.request.urlopen(urllib.request.Request(base+path,data=body,method=method,
                                  headers={'Content-Type':'application/json'}),timeout=120))
        started=time.perf_counter()
        try:
            while True:
                try:health=request_api('/api/health');break
                except OSError:
                    if server.poll() is not None or time.perf_counter()-started>40:
                        server_log.flush();raise RuntimeError((work/'server.log').read_text())
                    time.sleep(.1)
            assert health['ok'];report['timings']['http_startup']=round(time.perf_counter()-started,3)
            assert request_api('/api/projects')==[]
            project=request_api('/api/projects',{'name':'Compiled export smoke'});pid=project['id'];url='/api/projects/'+pid
            project_dir=data/'projects'/pid;shutil.copy2(role_vocal,project_dir/'beat.wav')
            project=request_api(url,{'words':[{'text':'Nuitka','start':.5,'end':1.5,'line':0,'role':'all'}],
                                     'settings':{'title_enabled':True,'title_text':'Nuitka 1.1.0','title_end':3}},'PATCH')
            # Media metadata is normally created by upload; use the actual endpoint here.
            boundary='compiledsmoke';body=(f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="beat.wav"\r\nContent-Type: audio/wav\r\n\r\n').encode()+role_vocal.read_bytes()+f'\r\n--{boundary}--\r\n'.encode()
            upload=json.load(urllib.request.urlopen(urllib.request.Request(base+url+'/media/beat',data=body,
                              headers={'Content-Type':'multipart/form-data; boundary='+boundary}),timeout=30))
            def wait_job(job):
                deadline=time.monotonic()+180
                while job['state'] in ('queued','running') and time.monotonic()<deadline:
                    time.sleep(.2);job=request_api('/api/jobs/'+job['id'])
                assert job['state']=='done',job
                return job
            wait_job(upload)
            if args.download_demucs:
                wait_job(request_api(url+'/models/demucs',{}))
                models=request_api('/api/models')
                demucs=next(item for item in models if item['id']=='demucs')
                assert demucs['ready'] and demucs['path']==str(data/'models'/model_manager.DEMUCS_FILE)
                report['checks'].append('Model UI API launches the compiled helper and finds its downloaded checkpoint')
            wait_job(request_api(url+'/render',{}))
            assert list((project_dir/'exports').rglob('*.mp4'))
            report['checks'].append('Compiled production HTTP server uploads media and exports video with title')
        finally:
            server.terminate();server.wait(timeout=20);server_log.close()
    report['passed']=True
    Path(args.report).write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps(report,ensure_ascii=False,indent=2))


if __name__=='__main__':main()
