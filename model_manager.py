"""Explicit model downloads, using the same locations as analysis workers."""
import json
from pathlib import Path
import shutil
import sys

MODELS = ('small', 'medium', 'large-v3-turbo', 'demucs')

def local_whisper(root, name):
    path = Path(root) / 'models' / 'whisper' / name
    return path if all((path / f).is_file() for f in ('model.bin', 'config.json', 'tokenizer.json')) else None


def cached_whisper(root,name):
    from faster_whisper.utils import download_model
    # Read both managed and pre-existing shared caches without moving/deleting either.
    for cache in (str(Path(root)/'cache/huggingface/hub'),None):
        try:
            path=Path(download_model(name,local_files_only=True,cache_dir=cache))
            if all((path/f).is_file() for f in ('model.bin','config.json','tokenizer.json')):return path
        except (OSError,ValueError):pass
    return None


def resolve_whisper(root,name):
    if name not in MODELS[:-1]:raise ValueError('Model không hợp lệ.')
    path=local_whisper(root,name) or cached_whisper(root,name)
    if path:return path
    from faster_whisper.utils import download_model
    destination=Path(root)/'models/whisper'/name
    download_model(name,output_dir=str(destination),cache_dir=str(Path(root)/'cache/huggingface/hub'))
    path=local_whisper(root,name)
    if not path:raise ValueError('Model tải chưa đầy đủ. Hãy tải lại trong Cài đặt model.')
    return path


def cached_demucs(root):
    import torch
    for folder in (Path(root)/'cache/torch/hub/checkpoints',Path(torch.hub.get_dir())/'checkpoints'):
        path=next(folder.glob('955717e8-*.th'),None)
        if path:return path
    return None

def status(root):
    from faster_whisper.utils import download_model
    items = []
    for name in MODELS[:-1]:
        path = local_whisper(root, name)
        path=path or cached_whisper(root,name)
        target=Path(root)/'models/whisper'/name
        items.append({'id': name, 'ready': bool(path), 'path': str(path or target),
                      'download_path':str(target),'location':'app' if path==target else 'cache' if path else 'missing'})
    bundled = Path(__file__).parent.parent / 'VCL_data/models/hub/checkpoints/955717e8-8726e21a.th'
    local = Path(root) / 'models/955717e8-8726e21a.th'
    import torch
    hub = Path(torch.hub.get_dir()) / 'checkpoints'
    managed=Path(root)/'cache/torch/hub/checkpoints'
    ready = local if local.is_file() else bundled if bundled.is_file() else next(managed.glob('955717e8-*.th'),None) or next(hub.glob('955717e8-*.th'), None)
    items.append({'id': 'demucs', 'ready': bool(ready), 'path': str(ready or local),
                  'download_path':str(managed),'location':'app' if ready in (local,bundled) else 'cache' if ready else 'missing'})
    return items

def download(root, name):
    if name not in MODELS:
        raise ValueError('Model không hợp lệ.')
    print('Đang chuẩn bị model ' + name, flush=True)
    if name == 'demucs':
        existing=Path(root)/'models/955717e8-8726e21a.th'
        if existing.is_file() or cached_demucs(root):
            print('Dùng model Demucs đã có trên máy.',flush=True)
            return
        bundled = Path(__file__).parent.parent / 'VCL_data/models/hub/checkpoints/955717e8-8726e21a.th'
        if bundled.is_file():
            dest = Path(root) / 'models' / bundled.name
            dest.parent.mkdir(parents=True, exist_ok=True)
            if not dest.is_file():
                shutil.copyfile(bundled, dest)
        else:
            import torch
            torch.hub.set_dir(str(Path(root)/'cache/torch/hub'))
            from demucs.pretrained import get_model
            get_model('htdemucs')
    else:
        path=resolve_whisper(root,name)
        print('Sử dụng model: '+str(path),flush=True)
    print('Model đã sẵn sàng.', flush=True)

if __name__ == '__main__':
    download(Path(sys.argv[1]), sys.argv[2])
