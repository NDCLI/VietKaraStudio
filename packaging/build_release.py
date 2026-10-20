"""Build a relocatable Windows runtime, launcher, portable ZIP and installer."""
import argparse
from datetime import datetime,timezone
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zipfile

ROOT=Path(__file__).resolve().parents[1]
VERSION='1.0.0'
RELEASE=ROOT/'releases'
STAGE=RELEASE/f'VietKaraStudio-{VERSION}-Portable-x64'
RUNTIME=STAGE/'runtime'
PACK=ROOT/'packaging'
SKIP={'__pycache__','.git','.pytest_cache','tests','test','idlelib','ensurepip','pip','pip-25.0.1.dist-info','_pytest','pytest','pytest-9.1.1.dist-info'}


def ignore(folder,names):
    return [n for n in names if n in SKIP or (Path(folder).name=='torch' and n=='include') or n.endswith(('.pyc','.pyo','.log','.partial'))]


def tree(source,destination):
    shutil.copytree(source,destination,dirs_exist_ok=True,ignore=ignore)


def assemble():
    STAGE.mkdir(parents=True,exist_ok=True)
    RUNTIME.mkdir(exist_ok=True)
    base=Path(sys.base_prefix)
    for name in ('python.exe','pythonw.exe','python312.dll','python3.dll','vcruntime140.dll','vcruntime140_1.dll','LICENSE.txt'):
        shutil.copy2(base/name,RUNTIME/name)
    # App-local VC++ runtime: don't depend on a machine-wide VC redist installation.
    system=Path(os.environ['WINDIR'])/'System32'
    for name in ('msvcp140.dll','msvcp140_1.dll','msvcp140_2.dll','msvcp140_atomic_wait.dll','msvcp140_codecvt_ids.dll','vcruntime140.dll','vcruntime140_1.dll'):
        if (system/name).is_file():shutil.copy2(system/name,RUNTIME/name)
    print('Copying Python runtime...',flush=True)
    for name in ('DLLs','tcl'):tree(base/name,RUNTIME/name)
    shutil.copytree(base/'Lib',RUNTIME/'Lib',dirs_exist_ok=True,ignore=lambda folder,names:ignore(folder,names)+(['site-packages'] if Path(folder)==base/'Lib' else []))
    print('Copying application libraries including CUDA...',flush=True)
    tree(ROOT/'.venv/Lib/site-packages',RUNTIME/'Lib/site-packages')
    (RUNTIME/'python312._pth').write_text('.\nLib\nDLLs\nLib/site-packages\n../app\nimport site\n',encoding='ascii')
    app=STAGE/'app';app.mkdir(exist_ok=True)
    for source in ROOT.glob('*.py'):
        if source.name!='configure_gpu.py':shutil.copy2(source,app/source.name)
    for name in ('static','resources','tools'):tree(ROOT/name,app/name)
    (app/'bin').mkdir(exist_ok=True)
    for name in ('ffmpeg.exe','ffprobe.exe'):shutil.copy2(ROOT.parent/name,app/'bin'/name)
    shutil.copy2(PACK/'bootstrap.py',STAGE/'bootstrap.py')
    (STAGE/'portable.flag').write_text('Store user data next to this portable application.\n',encoding='utf-8')
    # User projects, uploads, model caches and credentials are never input to this build.
    (STAGE/'README-VI.txt').write_text('''VIETKARA STUDIO 1.0.0 - WINDOWS x64

PORTABLE: giai nen TOAN BO ZIP vao thu muc co quyen ghi, sau do chay VietKaraStudio.exe.
SETUP: chay file Setup-x64.exe. Ung dung cai theo tai khoan Windows, khong can admin.
Khong can cai Python hay FFmpeg. Can Windows 10/11 x64 va Microsoft Edge WebView2 Runtime.
May chua co WebView2: cai Evergreen Runtime tu https://developer.microsoft.com/microsoft-edge/webview2/
GPU NVIDIA tuong thich CUDA 12.4 se duoc uu tien; neu khong dung duoc thi chuyen CPU.
Khong kem model AI: vao Cai dat model de tai small/medium/large-v3-turbo/Demucs.
Can Internet trong lan tai model dau tien; model da tai duoc dung lai offline.

DU LIEU
Portable luu project/model/log trong thu muc data ben canh EXE.
Setup luu trong %LOCALAPPDATA%\\VietKaraStudio; go cai dat KHONG xoa project/model.
Ban build khong kem bai hat, video, logo rieng, project hay cache cua nguoi build.
Luu/y xuat project cu tu app dang chay; khong tu dong chep du lieu ca nhan vao ban phat hanh.

LOI KHOI DONG
Xem data\\logs\\VietKaraStudio.log (portable) hoac %LOCALAPPDATA%\\VietKaraStudio\\logs.
Mo thu muc Chuan doan bang cach chay Start-Diagnostics.cmd de ghi log terminal.
EXE/Setup chua duoc ky so. Bo phat hanh nay khong kem chung thu code-signing.

VIDEO
FFmpeg + libass ghep chu/nhạc/logo. Am thanh dau ra la beat da nap.
MP4/MOV/MKV dung H.264 + AAC. MOV khong co alpha. MKV co the can trinh phat ngoai.
Nguon phan giai thap khong the tro thanh 4K chi tiet that chi bang tang kich thuoc.

THU VIEN
Danh sach phien ban: BUILD-INFO.json. Ban quyen thu vien nam trong runtime/Lib/site-packages/*dist-info/.
Python license: runtime/LICENSE.txt. FFmpeg: app/bin/FFMPEG-LICENSE.txt.
''',encoding='utf-8-sig')
    (STAGE/'Start-Diagnostics.cmd').write_text('@echo off\r\ncd /d "%~dp0"\r\n"runtime\\python.exe" bootstrap.py --browser\r\npause\r\n',encoding='ascii')
    ffmpeg_license=subprocess.run([str(app/'bin/ffmpeg.exe'),'-L'],capture_output=True,text=True,encoding='utf-8',errors='replace')
    (app/'bin/FFMPEG-LICENSE.txt').write_text(ffmpeg_license.stdout+ffmpeg_license.stderr+'\nUpstream source and build information: https://ffmpeg.org/\n',encoding='utf-8')
    libraries=sorted(({'name':d.metadata['Name'],'version':d.version} for d in importlib.metadata.distributions()),key=lambda d:d['name'].lower())
    (STAGE/'BUILD-INFO.json').write_text(json.dumps({'app':'VietKara Studio','version':VERSION,'architecture':'Windows x64',
        'built_utc':datetime.now(timezone.utc).isoformat(),'python':sys.version,'libraries':libraries,'models_bundled':False,'private_projects_bundled':False},indent=2),encoding='utf-8')
    csc=Path(os.environ['WINDIR'])/'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
    subprocess.run([str(csc),'/nologo','/target:winexe','/platform:x64','/reference:System.Windows.Forms.dll',
                    '/win32icon:'+str(PACK/'VietKaraStudio.ico'),'/out:'+str(STAGE/'VietKaraStudio.exe'),str(PACK/'Launcher.cs')],check=True)
    print('Stage ready: '+str(STAGE),flush=True)


def artifacts(setup_only=False):
    if (STAGE/'data').exists():raise SystemExit('Refusing to package a stage containing private root data/. Use a clean stage.')
    compiler=ROOT/'build-tools/InnoSetup/ISCC.exe'
    print('Building Setup...',flush=True)
    subprocess.run([str(compiler),'/Qp','/DSourceDir='+str(STAGE),str(PACK/'VietKaraStudio.iss')],check=True)
    destination=RELEASE/(STAGE.name+'.zip')
    if not setup_only:
        print('Compressing portable ZIP...',flush=True)
        with zipfile.ZipFile(destination,'w',zipfile.ZIP_DEFLATED,compresslevel=3,allowZip64=True) as archive:
            for path in STAGE.rglob('*'):
                if path.is_file() and 'data' not in path.relative_to(STAGE).parts[:1] and '__pycache__' not in path.parts and path.suffix not in ('.pyc','.pyo'):
                    archive.write(path,path.relative_to(RELEASE))
    lines=[]
    for path in sorted(RELEASE.iterdir()):
        if path.is_file() and path.suffix in ('.exe','.zip','.bin'):
            print('Hashing '+path.name,flush=True)
            digest=hashlib.file_digest(path.open('rb'),'sha256').hexdigest()
            lines.append(digest+'  '+path.name)
    (RELEASE/'SHA256SUMS.txt').write_text('\n'.join(lines)+'\n',encoding='ascii')
    print('Release artifacts complete.',flush=True)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--artifacts',action='store_true');parser.add_argument('--setup-only',action='store_true');args=parser.parse_args()
    if args.artifacts or args.setup_only:artifacts(args.setup_only)
    else:assemble()
