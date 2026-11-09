"""Build a small GitHub release patch for compatible Nuitka Setup installs."""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.metadata
import json
from pathlib import Path
import shutil
import tempfile
import zipfile

import build_nuitka

ROOT=build_nuitka.ROOT
STAGE=build_nuitka.STAGE
BASE=ROOT/'packaging/nuitka-patch-base.json'
PATCH_FORMAT='vietkara-nuitka-patch-v1'
BACKING_DISTRIBUTIONS = (
    'absl-py', 'audio-separator', 'audioread', 'beartype', 'ml-collections',
    'pydub', 'resampy', 'rotary-embedding-torch', 'samplerate', 'six',
)


def fingerprint(info):
    payload={key:info.get(key) for key in ('python','nuitka','libraries')}
    return hashlib.sha256(json.dumps(payload,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode('utf-8')).hexdigest()


def stage_backing_patch(stage: Path, base: dict):
    """Keep the Setup runtime intact and stage the backing-only imports as resources."""
    original = json.loads((STAGE / 'BUILD-INFO.json').read_text(encoding='utf-8'))
    if fingerprint(original) != fingerprint(base):
        raise SystemExit('Use a baseline-compatible Setup payload to build the backing patch.')
    shutil.copy2(build_nuitka.DIST / 'VietKaraStudio.exe', stage / 'VietKaraStudio.exe')
    for name in ('static', 'resources'):
        shutil.copytree(ROOT / name, stage / name, ignore=build_nuitka.ignore)
    site = build_nuitka.SITE.resolve()
    addon = stage / 'resources' / 'backing_runtime'
    libraries = {}
    for package in BACKING_DISTRIBUTIONS:
        distribution = importlib.metadata.distribution(package)
        libraries[package] = distribution.version
        for file in distribution.files or ():
            source = Path(distribution.locate_file(file)).resolve()
            if not source.is_relative_to(site) or not source.is_file():
                continue
            relative = source.relative_to(site)
            if '__pycache__' in relative.parts or source.suffix in ('.pyc', '.pyo', '.exe'):
                continue
            dest = addon / relative
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, dest)
    original.update(version=build_nuitka.VERSION, backing_runtime=libraries,
                    built_utc=datetime.now(timezone.utc).isoformat())
    (stage / 'BUILD-INFO.json').write_text(json.dumps(original, indent=2), encoding='utf-8')


def build_patch(stage: Path, base: dict):
    info_path=stage/'BUILD-INFO.json'
    executable=stage/'VietKaraStudio.exe'
    if not executable.is_file() or not info_path.is_file():raise SystemExit('Build and assemble the Nuitka application first.')
    current=json.loads(info_path.read_text(encoding='utf-8'))
    if fingerprint(current)!=fingerprint(base):
        raise SystemExit('AI/runtime libraries changed since the full Setup baseline; publish a full Setup instead of a patch.')
    if tuple(map(int,current['version'].split('.'))) <= tuple(map(int,base['version'].split('.'))):
        raise SystemExit('Patch version must be newer than its full-Setup baseline.')
    files=[executable,info_path]
    for folder in ('static','resources'):
        files.extend(p for p in (stage/folder).rglob('*') if p.is_file())
    manifest={'format':PATCH_FORMAT,'version':current['version'],'minimum_version':base['version'],
              'runtime_fingerprint':fingerprint(current),
              'files':{p.relative_to(stage).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(files)}}
    output=ROOT/'releases'/f"VietKaraStudio-{current['version']}-Nuitka-Patch.zip"
    output.parent.mkdir(parents=True,exist_ok=True)
    temp=output.with_suffix('.partial')
    with zipfile.ZipFile(temp,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as archive:
        for path in sorted(files):archive.write(path,path.relative_to(stage).as_posix())
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


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--base-info',type=Path,default=BASE)
    parser.add_argument('--with-backing-runtime',action='store_true',
                        help='Package the worker add-on alongside the unchanged Setup runtime.')
    args=parser.parse_args()
    if not args.base_info.is_file():raise SystemExit('Missing full-Setup baseline BUILD-INFO: '+str(args.base_info))
    base=json.loads(args.base_info.read_text(encoding='utf-8'))
    if args.with_backing_runtime:
        with tempfile.TemporaryDirectory(prefix='backing-patch-', dir=build_nuitka.BUILD) as folder:
            stage = Path(folder)
            stage_backing_patch(stage, base)
            build_patch(stage, base)
    else:
        build_patch(STAGE, base)


if __name__=='__main__':main()
