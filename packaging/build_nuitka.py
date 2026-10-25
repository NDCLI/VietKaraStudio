"""Compile application code and dispatch into one EXE; retain JIT-compatible AI wheels."""
import argparse
from datetime import datetime,timezone
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

import build_release as legacy

ROOT=Path(__file__).resolve().parents[1]
VERSION='1.1.2'
BUILD=ROOT/'build-tools/nuitka-1.1.0'
DIST=BUILD/'desktop_entry.dist'
STAGE=BUILD/'payload'
SITE=ROOT/'.venv/Lib/site-packages'
SKIP={'__pycache__','.git','.pytest_cache','tests','test','idlelib','ensurepip','venv','python.exe','pythonw.exe','pip','nuitka','nuitka-4.3.1.dist-info','_pytest','pytest'}


def ignore(folder,names):
    return [n for n in names if n in SKIP or n.endswith(('.pyc','.pyo','.log','.partial'))
            or (Path(folder).name=='torch' and n=='include')]


def compile_app():
    modules=[p.stem for p in ROOT.glob('*.py') if p.stem not in ('configure_gpu','desktop_entry')]
    external=set()
    for p in SITE.iterdir():
        name=p.name if p.is_dir() else p.stem if p.suffix in ('.py','.pyd') else ''
        if re.fullmatch(r'[A-Za-z_]\w*',name) and name not in modules and name!='tools':external.add(name)
    BUILD.mkdir(parents=True,exist_ok=True)
    args=[sys.executable,'-m','nuitka','--mode=standalone','--msvc=latest','--jobs=4','--lto=no',
          # The desktop app and its worker tasks must never own an attachable
          # console: closing a Windows console sends WM_CLOSE to Intel runtime
          # handlers and can abort MKL/NumPy with forrtl error 200.
          '--windows-console-mode=disable','--deployment','--python-flag=isolated','--python-flag=no_site',
          '--python-flag=dont_write_bytecode','--assume-yes-for-downloads','--output-dir='+str(BUILD),
          '--output-filename=VietKaraStudio.exe','--windows-icon-from-ico='+str(ROOT/'resources/VietKaraStudio.ico'),
          '--product-name=VietKara Studio','--file-description=VietKara Studio Nuitka',
          '--product-version='+VERSION,'--file-version='+VERSION+'.0',
          '--report='+str(BUILD/'compilation-report.xml')]
    args += ['--include-module='+name for name in modules+['tools.pick_export_folder']]
    args += ['--nofollow-import-to='+name for name in sorted(external)]
    args += [str(ROOT/'desktop_entry.py')]
    (BUILD/'build-command.json').write_text(json.dumps(args,indent=2),encoding='utf-8')
    with (BUILD/'compile.log').open('w',encoding='utf-8') as log:
        subprocess.run(args,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
    print('Compiled EXE: '+str(DIST/'VietKaraStudio.exe'),flush=True)


def assemble():
    if not (DIST/'VietKaraStudio.exe').is_file():raise SystemExit('Compile first')
    if (STAGE/'data').exists():raise SystemExit('Use a clean stage; private data/ exists')
    shutil.copytree(DIST,STAGE,dirs_exist_ok=True,ignore=ignore)
    vendor=STAGE/'vendor';vendor.mkdir(exist_ok=True)
    base=Path(sys.base_prefix)
    # Stable-ABI extensions (PyAV/pythonnet) need this forwarding DLL even
    # though Nuitka's own modules only link directly to python312.dll.
    shutil.copy2(base/'python3.dll',STAGE/'python3.dll')
    system=Path(os.environ['WINDIR'])/'System32'
    for name in ('msvcp140.dll','msvcp140_1.dll','msvcp140_2.dll','msvcp140_atomic_wait.dll','msvcp140_codecvt_ids.dll','vcruntime140.dll','vcruntime140_1.dll'):
        if not (STAGE/name).is_file() and (system/name).is_file():shutil.copy2(system/name,STAGE/name)
    shutil.copytree(base/'Lib',vendor/'Lib',dirs_exist_ok=True,
                    ignore=lambda folder,names:ignore(folder,names)+(['site-packages'] if Path(folder)==base/'Lib' else []))
    for name in ('DLLs','tcl'):shutil.copytree(base/name,vendor/name,dirs_exist_ok=True,ignore=ignore)
    shutil.copytree(SITE,vendor/'Lib/site-packages',dirs_exist_ok=True,ignore=ignore)
    for name in ('static','resources'):
        shutil.copytree(ROOT/name,STAGE/name,dirs_exist_ok=True,ignore=ignore)
    index=STAGE/'static/index.html'
    index.write_text(re.sub(r'(<footer class="app-footer"><span>VIETKARA STUDIO <b>)[^<]+(</b>)',
                            lambda match:match.group(1)+VERSION+match.group(2),
                            index.read_text(encoding='utf-8')),encoding='utf-8')
    (STAGE/'bin').mkdir(exist_ok=True)
    ffmpeg_sources = [ROOT/'bin', BUILD/'payload/bin', ROOT/'releases/VietKaraStudio-1.1.0-Nuitka-Portable-x64/bin']
    for name in ('ffmpeg.exe','ffprobe.exe'):
        source = next((folder/name for folder in ffmpeg_sources if (folder/name).is_file()), None)
        if source is None:
            raise SystemExit(f'Missing packaged media binary: {name}. Put it in bin/ or an existing Nuitka payload.')
        destination = STAGE/'bin'/name
        if source.resolve() != destination.resolve():
            shutil.copy2(source, destination)
    license_info=subprocess.run([str(STAGE/'bin/ffmpeg.exe'),'-L'],capture_output=True,text=True,encoding='utf-8',errors='replace',check=True)
    (STAGE/'bin/FFMPEG-LICENSE.txt').write_text(license_info.stdout+license_info.stderr+'\nUpstream source and build information: https://ffmpeg.org/\n',encoding='utf-8')
    shutil.copy2(base/'LICENSE.txt',STAGE/'PYTHON-LICENSE.txt')
    compile_updater()
    (STAGE/'portable.flag').write_text('Keep user data beside the compiled EXE.\n',encoding='utf-8')
    original_stage=legacy.STAGE
    try:
        legacy.STAGE=STAGE
        dependency=legacy.webview2_bootstrapper()
    finally:legacy.STAGE=original_stage
    packages=sorted(({'name':d.metadata['Name'],'version':d.version} for d in importlib.metadata.distributions()),key=lambda d:d['name'].lower())
    (STAGE/'BUILD-INFO.json').write_text(json.dumps({'app':'VietKara Studio','version':VERSION,
        'built_utc':datetime.now(timezone.utc).isoformat(),'packaging':'Nuitka standalone application + source-compatible JIT dependencies',
        'nuitka':importlib.metadata.version('nuitka'),'python':sys.version,'libraries':packages,
        'webview2_bootstrapper':dependency,'models_bundled':False,'private_projects_bundled':False},indent=2),encoding='utf-8')
    (STAGE/'README-VI.txt').write_text(f'''VIETKARA STUDIO {VERSION} - NUITKA x64
Chay VietKaraStudio.exe. Khong can cai Python; khong co python.exe/pythonw.exe trong goi.
Ma ung dung va cac tac vu AI duoc bien dich trong EXE. Thu vien AI giu dang tuong thich JIT.
Portable luu trong data/. Setup luu tai %LOCALAPPDATA%/VietKaraStudio.
Model tai rieng, cache/model cu duoc su dung lai. WebView2 can cho giao dien.
''',encoding='utf-8-sig')
    (STAGE/'Start-Diagnostics.cmd').write_text('@echo off\r\ncd /d "%~dp0"\r\nVietKaraStudio.exe --no-open\r\npause\r\n',encoding='ascii')
    forbidden=list(STAGE.rglob('python.exe'))+list(STAGE.rglob('pythonw.exe'))
    if forbidden:raise SystemExit('Unexpected interpreter executables: '+str(forbidden))
    if any((STAGE/(p.stem+'.py')).exists() for p in ROOT.glob('*.py')):raise SystemExit('Application sources leaked into root')
    print('Nuitka stage ready: '+str(STAGE),flush=True)


def compile_updater():
    compiler=Path(os.environ['WINDIR'])/'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
    if not compiler.is_file():raise SystemExit('Missing Windows .NET Framework C# compiler: '+str(compiler))
    output=STAGE/'VietKaraUpdater.exe'
    subprocess.run([str(compiler),'/nologo','/target:winexe','/optimize+','/out:'+str(output),
                    '/reference:System.Windows.Forms.dll',str(ROOT/'packaging/UpdateHelper.cs')],check=True)


def setup():
    if (STAGE/'data').exists():raise SystemExit('Refusing a stage with private data')
    compiler=ROOT/'build-tools/InnoSetup/ISCC.exe'
    subprocess.run([str(compiler),'/Qp','/DSourceDir='+str(STAGE),str(ROOT/'packaging/VietKaraStudio-Nuitka.iss')],check=True)
    exe=ROOT/'releases'/f'VietKaraStudio-{VERSION}-Nuitka-Setup-x64.exe'
    with exe.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
    exe.with_suffix('.sha256').write_text(digest+'  '+exe.name+'\n',encoding='ascii')
    base_info=json.loads((STAGE/'BUILD-INFO.json').read_text(encoding='utf-8'))
    (ROOT/'packaging/nuitka-patch-base.json').write_text(json.dumps(
        {key:base_info[key] for key in ('version','python','nuitka','libraries')},indent=2),encoding='utf-8')
    print('SETUP_COMPLETE '+str(exe),flush=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('mode',choices=('compile','assemble','setup','all'))
    args=parser.parse_args()
    if args.mode in ('compile','all'):compile_app()
    if args.mode in ('assemble','all'):assemble()
    if args.mode in ('setup','all'):setup()
