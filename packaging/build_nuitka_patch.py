"""Build a small GitHub release patch for compatible Nuitka Setup installs."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

import build_nuitka

ROOT=build_nuitka.ROOT
STAGE=build_nuitka.STAGE
BASE=ROOT/'packaging/nuitka-patch-base.json'
PATCH_FORMAT='vietkara-nuitka-patch-v1'


def fingerprint(info):
    payload={key:info.get(key) for key in ('python','nuitka','libraries')}
    return hashlib.sha256(json.dumps(payload,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode('utf-8')).hexdigest()


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--base-info',type=Path,default=BASE)
    args=parser.parse_args()
    info_path=STAGE/'BUILD-INFO.json'
    executable=STAGE/'VietKaraStudio.exe'
    if not executable.is_file() or not info_path.is_file():raise SystemExit('Build and assemble the Nuitka application first.')
    if not args.base_info.is_file():raise SystemExit('Missing full-Setup baseline BUILD-INFO: '+str(args.base_info))
    current=json.loads(info_path.read_text(encoding='utf-8'))
    base=json.loads(args.base_info.read_text(encoding='utf-8'))
    if fingerprint(current)!=fingerprint(base):
        raise SystemExit('AI/runtime libraries changed since the full Setup baseline; publish a full Setup instead of a patch.')
    if tuple(map(int,current['version'].split('.'))) <= tuple(map(int,base['version'].split('.'))):
        raise SystemExit('Patch version must be newer than its full-Setup baseline.')
    files=[executable,info_path]
    for folder in ('static','resources'):
        files.extend(p for p in (STAGE/folder).rglob('*') if p.is_file())
    manifest={'format':PATCH_FORMAT,'version':current['version'],'minimum_version':base['version'],
              'runtime_fingerprint':fingerprint(current),
              'files':{p.relative_to(STAGE).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(files)}}
    output=ROOT/'releases'/f"VietKaraStudio-{current['version']}-Nuitka-Patch.zip"
    output.parent.mkdir(parents=True,exist_ok=True)
    temp=output.with_suffix('.partial')
    with zipfile.ZipFile(temp,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as archive:
        for path in sorted(files):archive.write(path,path.relative_to(STAGE).as_posix())
        archive.writestr('UPDATE-MANIFEST.json',json.dumps(manifest,ensure_ascii=False,indent=2))
    with zipfile.ZipFile(temp) as archive:
        if archive.testzip() is not None:raise SystemExit('Patch archive verification failed.')
        for name,digest in manifest['files'].items():
            if hashlib.sha256(archive.read(name)).hexdigest()!=digest:raise SystemExit('Patch hash mismatch: '+name)
        if any(name.startswith(('data/','vendor/','bin/')) for name in archive.namelist()):
            raise SystemExit('Patch contains user data or bundled dependencies.')
    temp.replace(output)
    with output.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
    output.with_suffix('.sha256').write_text(digest+'  '+output.name+'\n',encoding='ascii')
    print(json.dumps({'file':str(output),'bytes':output.stat().st_size,'files':len(files),'verified':True},ensure_ascii=False))


if __name__=='__main__':main()
