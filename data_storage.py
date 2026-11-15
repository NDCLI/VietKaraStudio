"""Verified migration of managed application data to the installation directory."""
from __future__ import annotations

import hashlib
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import tempfile
import uuid


class MigrationCancelled(Exception):
    pass


def data_directory(root: Path, override: str | None = None) -> Path:
    path = Path(override).resolve() if override else (root / 'data').resolve()
    try:
        path.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(prefix='.write-check-', dir=path):
            pass
    except OSError as exc:
        raise RuntimeError(f'Không ghi được thư mục dữ liệu {path}. Hãy đặt VietKara Studio trong thư mục có quyền ghi trên ổ bạn muốn sử dụng. Dữ liệu cũ vẫn được giữ nguyên.') from exc
    return path


def _hash(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def _signature(path):
    info = path.stat()
    link = str(path.readlink()) if path.is_symlink() else None
    return [info.st_size, info.st_mtime_ns, info.st_ino, link]


def _inventory(root):
    result = {}
    for parent, dirs, files in os.walk(root, followlinks=False):
        parent = Path(parent)
        for name in dirs:
            path = parent / name
            if path.is_symlink() or path.is_junction():
                raise RuntimeError(f'Chưa thể tự chuyển thư mục có liên kết ngoài: {path}. Dữ liệu cũ vẫn được giữ nguyên.')
        for name in files:
            path = parent / name
            # File links are materialized; their external targets are never deleted.
            result[path.relative_to(root).as_posix()] = _signature(path)
    return result


def _write_json(path, value):
    pending = path.with_name(path.name + '.tmp')
    pending.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf-8')
    pending.replace(path)


def _remap_metadata(target, source, receipt, journal):
    prefix = str(source).replace('\\', '/').rstrip('/')
    def remap(value):
        if isinstance(value, dict):
            return {k: remap(v) for k, v in value.items()}
        if isinstance(value, list):
            return [remap(v) for v in value]
        if isinstance(value, str):
            path = value.replace('\\', '/')
            if path.casefold() == prefix.casefold() or path.casefold().startswith(prefix.casefold() + '/'):
                return str(target) + path[len(prefix):].replace('/', os.sep)
        return value
    paths = list((target / 'jobs').glob('*.json')) + list((target / 'batch-renders').glob('*/batch.json'))
    for project in (target / 'projects').glob('*'):
        if project.is_dir():
            paths.extend(project.glob('*.json'))
    for path in paths:
        try:
            original = json.loads(path.read_text(encoding='utf-8-sig'))
        except (OSError, ValueError):
            continue
        updated = remap(original)
        if updated != original:
            backup = target / '.migration-metadata-backup' / path.relative_to(target)
            backup.parent.mkdir(parents=True, exist_ok=True)
            if not backup.exists():
                shutil.copy2(path, backup)
            name = path.relative_to(target).as_posix()
            if name in receipt['files']:
                payload = json.dumps(updated, ensure_ascii=False, indent=2).encode('utf-8')
                receipt['files'][name]['target_sha256'] = hashlib.sha256(payload).hexdigest()
                _write_json(journal, receipt)
            _write_json(path, updated)


def _cleanup(source, target, receipt):
    """Delete only unchanged, verified original files, never recursive directory trees."""
    original = Path(receipt['source'])
    if source.parent != original.parent or not source.name.startswith(original.name + '.migrated-'):
        raise RuntimeError('Đường dẫn dọn dữ liệu cũ không hợp lệ.')
    if not source.is_dir() or source.is_symlink() or source.is_junction():
        return []
    remaining = []
    for name, entry in receipt['files'].items():
        relative = Path(name)
        if relative.is_absolute() or '..' in relative.parts:
            raise RuntimeError('Đường dẫn dữ liệu cũ không hợp lệ.')
        old = source / relative
        new = target / relative
        if not old.exists() and not old.is_symlink():
            continue
        if not old.parent.resolve().is_relative_to(source.resolve()) or not new.is_file():
            remaining.append(name)
            continue
        if _hash(new) != entry['target_sha256']:
            remaining.append(name)
            continue
        # Links may become dangling as their old blob targets are removed.
        if not old.is_symlink() and _signature(old) != entry['signature']:
            remaining.append(name)
            continue
        try:
            old.unlink()
        except OSError:
            remaining.append(name)
    for parent, dirs, files in os.walk(source, topdown=False, followlinks=False):
        try:
            Path(parent).rmdir()
        except OSError:
            pass
    if source.exists():
        remaining.extend(p.relative_to(source).as_posix() for p in source.rglob('*') if p.is_file() or p.is_symlink())
    return sorted(set(remaining))


@contextmanager
def _migration_lock(source):
    source.parent.mkdir(parents=True, exist_ok=True)
    with (source.parent / '.VietKaraStudio-data-migration.lock').open('a+b') as lock:
        if os.fstat(lock.fileno()).st_size == 0:
            lock.write(b'0')
            lock.flush()
        lock.seek(0)
        try:
            if os.name == 'nt':
                import msvcrt
                msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as exc:
            raise RuntimeError('Một cửa sổ VietKara khác đang chuyển dữ liệu. Hãy chờ chuyển xong rồi mở lại app.') from exc
        try:
            yield
        finally:
            lock.seek(0)
            if os.name == 'nt':
                msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def migrate_legacy(source: Path, target: Path, progress=None, cancelled=None):
    with _migration_lock(source.absolute()):
        return _migrate_legacy(source, target, progress, cancelled)


def _migrate_legacy(source: Path, target: Path, progress=None, cancelled=None):
    source = source.absolute()
    target = target.resolve()
    notify = progress or (lambda percent, message: None)
    def check():
        if cancelled and cancelled():
            raise MigrationCancelled()
    journal = target / '.data-migration.json'
    saved = json.loads(journal.read_text(encoding='utf-8')) if journal.is_file() else None
    if (saved and saved.get('state') in ('publishing', 'published', 'complete') and saved.get('source') == str(source)
            and (not source.exists() or source.resolve() == target or Path(saved.get('backup', '')).is_dir())):
        if saved.get('backup'):
            saved['remaining'] = _cleanup(Path(saved['backup']), target, saved)
            saved['state'] = 'complete' if not saved['remaining'] else 'published'
            _write_json(journal, saved)
        return saved
    if not source.exists() or source.resolve() == target:
        return None
    if not source.is_dir():
        raise RuntimeError('Đường dẫn dữ liệu cũ không phải thư mục. Dữ liệu cũ vẫn được giữ nguyên.')
    if source.is_junction() or source.is_symlink():
        raise RuntimeError('Dữ liệu AppData đang trỏ tới thư mục khác. Hãy giữ nguyên liên kết hoặc chuyển dữ liệu thủ công trước khi dùng vị trí mới.')
    source = source.resolve()
    if target.is_relative_to(source) or source.is_relative_to(target):
        raise RuntimeError('Hai thư mục dữ liệu không được nằm bên trong nhau.')
    data_directory(target.parent, str(target))
    notify(0, 'Đang kiểm tra dữ liệu cũ…')
    files = _inventory(source)
    previous = saved.get('files', {}) if saved and saved.get('source') == str(source) else {}
    receipt = {'source': str(source), 'destination': str(target), 'state': 'copying', 'files': dict(previous)}
    existing_hashes = {}
    needed = 0
    for name, signature in files.items():
        check()
        destination = target / name
        if not destination.resolve().is_relative_to(target):
            raise RuntimeError('Thư mục đích có liên kết ra ngoài. Dữ liệu cũ vẫn được giữ nguyên.')
        checksum = None
        if destination.exists():
            if not destination.is_file():
                raise RuntimeError(f'Trùng tên dữ liệu tại {destination}. Hai bản được giữ nguyên.')
            checksum = _hash(source / name)
            existing = _hash(destination)
            existing_hashes[name] = (checksum, existing)
            if existing != checksum and previous.get(name, {}).get('target_sha256') != existing:
                raise RuntimeError(f'Hai thư mục có dữ liệu khác nhau tại {destination}. App chưa xóa hoặc ghi đè bản nào; hãy hợp nhất dữ liệu trước.')
        if checksum is None or not destination.is_file() or _hash(destination) != checksum:
            needed += signature[0]
    if shutil.disk_usage(target).free < needed + 32 * 1024**2:
        raise RuntimeError('Ổ cài đặt không đủ chỗ để chuyển dữ liệu. Dữ liệu AppData vẫn được giữ nguyên.')
    total = sum(value[0] for value in files.values())
    completed = 0
    seen = {}
    for name, signature in files.items():
        check()
        original = source / name
        destination = target / name
        if _signature(original) != signature:
            raise RuntimeError('Dữ liệu đang thay đổi. Hãy đóng các cửa sổ VietKara rồi mở lại để tiếp tục chuyển.')
        destination.parent.mkdir(parents=True, exist_ok=True)
        identity = (original.stat().st_dev, signature[2])
        known = existing_hashes.get(name)
        if known and known[0] == known[1] and _hash(destination) == known[0]:
            checksum = known[0]
            seen[identity] = (destination, checksum)
            receipt['files'][name] = {'signature': signature, 'sha256': checksum, 'target_sha256': checksum}
            _write_json(journal, receipt)
            completed += signature[0]
            continue
        pending = destination.with_name('.migrating-' + uuid.uuid4().hex)
        try:
            if identity in seen:
                linked, checksum = seen[identity]
                try:
                    os.link(linked, pending)
                except OSError:
                    shutil.copy2(linked, pending)
            else:
                state = hashlib.sha256()
                with original.open('rb') as incoming, pending.open('xb') as outgoing:
                    while chunk := incoming.read(8 * 1024**2):
                        check()
                        outgoing.write(chunk)
                        state.update(chunk)
                        notify(min(94, (completed + outgoing.tell()) * 94 / max(1, total)), 'Đang chuyển project, model và cache…')
                checksum = state.hexdigest()
                shutil.copystat(original, pending)
            if _hash(pending) != checksum or _signature(original) != signature:
                raise RuntimeError('Không xác minh được dữ liệu đã chuyển. Dữ liệu gốc vẫn được giữ nguyên.')
            pending.replace(destination)
            seen[identity] = (destination, checksum)
            receipt['files'][name] = {'signature': signature, 'sha256': checksum, 'target_sha256': checksum}
            _write_json(journal, receipt)
        finally:
            pending.unlink(missing_ok=True)
        completed += signature[0]
    check()
    if _inventory(source) != files:
        raise RuntimeError('Dữ liệu nguồn đã thay đổi trong lúc chuyển. Bản gốc vẫn được giữ nguyên; hãy mở lại app để tiếp tục.')
    notify(96, 'Đang hoàn tất và cập nhật đường dẫn…')
    _remap_metadata(target, source, receipt, journal)
    for name, entry in receipt['files'].items():
        entry['target_sha256'] = _hash(target / name)
    backup = source.with_name(source.name + '.migrated-' + uuid.uuid4().hex)
    receipt.update(state='publishing', backup=str(backup))
    # Persist recovery information before renaming the old tree.
    _write_json(journal, receipt)
    try:
        source.rename(backup)
    except OSError as exc:
        receipt['state'] = 'copying'
        _write_json(journal, receipt)
        raise RuntimeError('Dữ liệu đã sao chép nhưng thư mục cũ đang được sử dụng. Hãy đóng VietKara và mở lại để hoàn tất.') from exc
    if os.name == 'nt':
        import _winapi
        try:
            _winapi.CreateJunction(str(target), str(source))
        except OSError:
            # Metadata has already been relinked; the verified data remains usable.
            pass
    receipt['state'] = 'published'
    _write_json(journal, receipt)
    receipt['remaining'] = _cleanup(backup, target, receipt)
    receipt['state'] = 'complete' if not receipt['remaining'] else 'published'
    _write_json(journal, receipt)
    notify(100, 'Đã chuyển dữ liệu vào thư mục cài đặt.')
    return receipt


def migrate_on_startup(source, target, visible=True):
    """Show a small native startup window while large data folders are transferred."""
    if not visible or os.name != 'nt' or not source.exists() or source.resolve() == target.resolve():
        return migrate_legacy(source, target)
    import ctypes
    from ctypes import wintypes
    import queue
    import threading
    import time
    user = ctypes.WinDLL('user32', use_last_error=True)
    user.CreateWindowExW.argtypes = [wintypes.DWORD, wintypes.LPCWSTR, wintypes.LPCWSTR, wintypes.DWORD,
                                  ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                  wintypes.HWND, wintypes.HMENU, wintypes.HINSTANCE, ctypes.c_void_p]
    user.CreateWindowExW.restype = wintypes.HWND
    user.IsWindow.argtypes = [wintypes.HWND]
    user.DestroyWindow.argtypes = [wintypes.HWND]
    user.SetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPCWSTR]
    user.PeekMessageW.argtypes = [ctypes.POINTER(wintypes.MSG), wintypes.HWND, wintypes.UINT, wintypes.UINT, wintypes.UINT]
    user.TranslateMessage.argtypes = [ctypes.POINTER(wintypes.MSG)]
    user.DispatchMessageW.argtypes = [ctypes.POINTER(wintypes.MSG)]
    user.DispatchMessageW.restype = ctypes.c_ssize_t
    window = user.CreateWindowExW(1, 'STATIC', 'VietKara Studio · Chuyển dữ liệu', 0x10C80000,
                                 -2147483648, -2147483648, 620, 185, None, None, None, None)
    if not window:
        return migrate_legacy(source, target)
    label = user.CreateWindowExW(0, 'STATIC', 'Đang chuẩn bị chuyển dữ liệu…', 0x50000000,
                                20, 20, 570, 100, window, None, None, None)
    cancelled = threading.Event()
    updates = queue.Queue()
    outcome = []
    def work():
        try:
            outcome.append((True, migrate_legacy(source, target, lambda percent, message: updates.put((percent, message)), cancelled.is_set)))
        except BaseException as exc:
            outcome.append((False, exc))
    worker = threading.Thread(target=work, daemon=True)
    worker.start()
    message = wintypes.MSG()
    try:
        while worker.is_alive():
            while user.PeekMessageW(ctypes.byref(message), None, 0, 0, 1):
                user.TranslateMessage(ctypes.byref(message))
                user.DispatchMessageW(ctypes.byref(message))
            if not user.IsWindow(window):
                cancelled.set()
            latest = None
            while not updates.empty():
                latest = updates.get_nowait()
            if latest and user.IsWindow(label):
                percent, text = latest
                user.SetWindowTextW(label, f'{text} {percent:.0f}%\n\nNơi lưu mới: {target}')
            time.sleep(.05)
    finally:
        if user.IsWindow(window):
            user.DestroyWindow(window)
    if not outcome[0][0]:
        raise outcome[0][1]
    return outcome[0][1]
