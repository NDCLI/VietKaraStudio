"""One allowlisted task protocol for source runs and Nuitka standalone builds."""
from pathlib import Path
import sys

COMPILED = '__compiled__' in globals()
VERSION='1.1.1'
TASKS = frozenset(('probe','speech','roles','demucs','model','pick-folder','diagnose'))


def task_command(task,*arguments):
    if task not in TASKS:
        raise ValueError('Unknown application task: '+task)
    executable=[sys.executable] if COMPILED else [sys.executable,'-u',str(Path(__file__).with_name('desktop_entry.py'))]
    return [*executable,'--vcl-task',task,*(str(a) for a in arguments)]


def dispatch(task,arguments):
    if task not in TASKS:
        raise ValueError('Unknown application task: '+task)
    import json
    if task=='probe':
        if arguments:raise ValueError('Probe does not accept arguments')
        from hardware import probe
        print(json.dumps(probe(),ensure_ascii=False),flush=True)
    elif task=='speech':
        if len(arguments)!=1:raise ValueError('Speech requires one request file')
        from worker import run
        request=json.loads(Path(arguments[0]).read_text(encoding='utf-8'))
        run(request,request['device'])
    elif task=='roles':
        if len(arguments)!=1:raise ValueError('Roles requires one request file')
        from duet_roles import analyze
        request=json.loads(Path(arguments[0]).read_text(encoding='utf-8'))
        def progress(percent,message):
            print('OBS_PROGRESS '+json.dumps({'progress':percent,'message':message},ensure_ascii=False),flush=True)
        result=analyze(request['vocal'],request['segments'],progress)
        Path(request['output']).write_text(json.dumps(result,ensure_ascii=False),encoding='utf-8')
    elif task=='demucs':
        from demucs.separate import main
        main(arguments)
    elif task=='model':
        if len(arguments) not in (2,3) or (len(arguments)==3 and arguments[2]!='--fresh'):raise ValueError('Model requires a data path and name')
        from model_manager import download
        download(Path(arguments[0]),arguments[1],force_download=len(arguments)==3)
    elif task=='pick-folder':
        from tools.pick_export_folder import main
        main(arguments)
    elif task=='diagnose':
        if arguments:raise ValueError('Diagnose does not accept arguments')
        import importlib
        packages={}
        for name in ('numpy','scipy','soundfile','torch','ctranslate2','librosa','stable_whisper','demucs','webview'):
            module=importlib.import_module(name)
            packages[name]=getattr(module,'__version__','available')
        import engine,worker,duet_roles,hardware
        print(json.dumps({'compiled':COMPILED,'executable':sys.executable,'paths':sys.path,'packages':packages,
                          'compiled_modules':{m.__name__:hasattr(m,'__compiled__') for m in (engine,worker,duet_roles,hardware)}},ensure_ascii=False),flush=True)
