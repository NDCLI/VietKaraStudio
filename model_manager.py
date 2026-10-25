"""Explicit model downloads, using the same locations as analysis workers."""
from __future__ import annotations

import hashlib
from functools import lru_cache
from http.client import IncompleteRead
import json
import os
from pathlib import Path
import sys
import time
import urllib.request

MODELS = ('small', 'medium', 'large-v3-turbo', 'demucs')
DEMUCS_FILE = '955717e8-8726e21a.th'
# Pinned upstream HTDemucs checkpoint, also listed in demucs/remote/files.txt.
DEMUCS_URL = 'https://dl.fbaipublicfiles.com/demucs/hybrid_transformer/' + DEMUCS_FILE
DEMUCS_SHA256 = '8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4'


def progress(percent, message):
    print('OBS_PROGRESS ' + json.dumps({'progress': percent, 'message': message}, ensure_ascii=False), flush=True)


@lru_cache(maxsize=32)
def _valid_checkpoint(path, size, mtime, expected):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest() == expected


def valid_demucs(path):
    try:
        info = Path(path).stat()
        return _valid_checkpoint(str(Path(path).resolve()), info.st_size, info.st_mtime_ns, DEMUCS_SHA256)
    except OSError:
        return False


def local_whisper(root, name):
    path = Path(root) / 'models' / 'whisper' / name
    return path if all((path / f).is_file() for f in ('model.bin', 'config.json', 'tokenizer.json')) else None


def cached_whisper(root, name):
    from faster_whisper.utils import download_model
    # Read both managed and pre-existing shared caches without moving/deleting either.
    for cache in (str(Path(root) / 'cache/huggingface/hub'), None):
        try:
            path = Path(download_model(name, local_files_only=True, cache_dir=cache))
            if all((path / f).is_file() for f in ('model.bin', 'config.json', 'tokenizer.json')):
                return path
        except (OSError, ValueError):
            pass
    return None


def resolve_whisper(root, name):
    if name not in MODELS[:-1]:
        raise ValueError('Model không hợp lệ.')
    path = local_whisper(root, name) or cached_whisper(root, name)
    if path:
        return path
    from faster_whisper.utils import download_model
    destination = Path(root) / 'models/whisper' / name
    download_model(name, output_dir=str(destination), cache_dir=str(Path(root) / 'cache/huggingface/hub'))
    path = local_whisper(root, name)
    if not path:
        raise ValueError('Model tải chưa đầy đủ. Hãy tải lại trong Cài đặt model.')
    return path


def cached_demucs(root):
    torch_home = Path(os.environ.get('TORCH_HOME', Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'torch'))
    paths = (
        Path(root) / 'models' / DEMUCS_FILE,
        Path(__file__).resolve().parent.parent / 'VCL_data/models/hub/checkpoints' / DEMUCS_FILE,
        Path(root) / 'cache/torch/hub/checkpoints' / DEMUCS_FILE,
        torch_home / 'hub/checkpoints' / DEMUCS_FILE,
    )
    return next((path for path in paths if valid_demucs(path)), None)


def download_demucs(root, force_download=False):
    """Download the checkpoint without importing Torch or allocating GPU memory."""
    root = Path(root)
    existing = None if force_download else cached_demucs(root)
    if existing:
        progress(100, 'Dùng model Demucs đã có trên máy.')
        return existing

    destination = root / 'models' / DEMUCS_FILE
    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_suffix('.th.partial')
    error = None
    for attempt in range(3):
        try:
            offset = partial.stat().st_size if partial.exists() else 0
            headers = {'User-Agent': 'VietKaraStudio/1.1.1'}
            if offset:
                headers['Range'] = f'bytes={offset}-'
            progress(0, 'Đang kết nối máy chủ model Demucs…')
            with urllib.request.urlopen(urllib.request.Request(DEMUCS_URL, headers=headers), timeout=30) as response:
                # A server may ignore Range. Restart instead of appending duplicate bytes.
                if response.status == 206:
                    content_range = response.headers.get('Content-Range', '')
                    if not content_range.startswith(f'bytes {offset}-'):
                        raise OSError('Máy chủ trả vị trí tải tiếp không hợp lệ.')
                else:
                    offset = 0
                length = int(response.headers.get('Content-Length', 0))
                total = offset + length
                received = offset
                last = -1
                with partial.open('ab' if offset else 'wb') as stream:
                    while chunk := response.read(1024 * 1024):
                        stream.write(chunk)
                        received += len(chunk)
                        percent = min(98, int(received * 98 / total)) if total else 0
                        if percent != last:
                            progress(percent, f'Đang tải Demucs: {received / 1048576:.1f}' +
                                      (f' / {total / 1048576:.1f}' if total else '') + ' MB')
                            last = percent
                if total and received != total:
                    raise OSError('Kết nối bị ngắt khi tải model.')
            progress(99, 'Đang kiểm tra model Demucs…')
            if not valid_demucs(partial):
                partial.unlink(missing_ok=True)
                raise OSError('Model tải về bị lỗi kiểm tra SHA-256.')
            os.replace(partial, destination)
            progress(100, 'Model Demucs đã sẵn sàng.')
            return destination
        except (OSError, ValueError, IncompleteRead) as exc:
            error = exc
            # A complete partial file after an interrupted final response is usable.
            if valid_demucs(partial):
                os.replace(partial, destination)
                progress(100, 'Model Demucs đã sẵn sàng.')
                return destination
            if attempt < 2:
                progress(0, f'Kết nối bị gián đoạn, đang thử lại ({attempt + 2}/3)…')
                time.sleep(attempt + 1)
    raise RuntimeError('Không tải được Demucs. Kiểm tra kết nối mạng rồi bấm tải lại; app sẽ tiếp tục file đang tải. Chi tiết: ' + str(error)) from error


def status(root):
    items = []
    for name in MODELS[:-1]:
        path = local_whisper(root, name) or cached_whisper(root, name)
        target = Path(root) / 'models/whisper' / name
        items.append({'id': name, 'ready': bool(path), 'path': str(path or target),
                      'download_path': str(target),
                      'location': 'app' if path == target else 'cache' if path else 'missing'})
    local = Path(root) / 'models' / DEMUCS_FILE
    ready = cached_demucs(root)
    items.append({'id': 'demucs', 'ready': bool(ready), 'path': str(ready or local),
                  'download_path': str(local),
                  'location': 'app' if ready == local else 'cache' if ready else 'missing'})
    return items


def download(root, name, force_download=False):
    if name not in MODELS:
        raise ValueError('Model không hợp lệ.')
    print('Đang chuẩn bị model ' + name, flush=True)
    if name == 'demucs':
        path = download_demucs(root, force_download)
    else:
        path = resolve_whisper(root, name)
    print('Sử dụng model: ' + str(path), flush=True)
    print('Model đã sẵn sàng.', flush=True)


if __name__ == '__main__':
    download(Path(sys.argv[1]), sys.argv[2], force_download='--fresh' in sys.argv[3:])
