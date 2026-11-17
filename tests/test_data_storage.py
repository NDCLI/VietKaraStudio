import json
from pathlib import Path
import shutil
import sys
import pytest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import data_storage as storage


def source_data(tmp_path):
    old=tmp_path/'AppData/VietKaraStudio'
    target=tmp_path/'installed/data'
    (old/'projects/abc').mkdir(parents=True)
    (old/'projects/abc/beat.wav').write_bytes(b'audio payload')
    (old/'models').mkdir()
    (old/'models/model.th').write_bytes(b'model payload')
    (old/'jobs').mkdir()
    (old/'jobs/job.json').write_text(json.dumps({'result':{'output_path':str(old/'projects/abc/video.mp4')}}))
    return old,target


def test_default_storage_is_beside_executable_and_override_is_respected(tmp_path):
    assert storage.data_directory(tmp_path/'installed')==tmp_path/'installed/data'
    assert storage.data_directory(tmp_path/'installed',str(tmp_path/'custom'))==tmp_path/'custom'


def test_verified_migration_preserves_files_relinks_metadata_and_is_idempotent(tmp_path):
    old,target=source_data(tmp_path)
    result=storage.migrate_legacy(old,target)
    assert result['state']=='complete'
    assert (target/'projects/abc/beat.wav').read_bytes()==b'audio payload'
    assert (target/'models/model.th').read_bytes()==b'model payload'
    assert json.loads((target/'jobs/job.json').read_text())['result']['output_path']==str(target/'projects/abc/video.mp4')
    assert (target/'.migration-metadata-backup/jobs/job.json').is_file()
    assert not Path(result['backup']).exists()
    if sys.platform=='win32':
        assert old.is_junction() and old.resolve()==target.resolve()
        assert (old/'projects/abc/beat.wav').read_bytes()==b'audio payload'
    assert storage.migrate_legacy(old,target)['state']=='complete'


def test_conflicting_existing_data_is_not_overwritten_or_deleted(tmp_path):
    old,target=source_data(tmp_path)
    (target/'projects/abc').mkdir(parents=True)
    (target/'projects/abc/beat.wav').write_bytes(b'new project audio')
    with pytest.raises(RuntimeError,match='dữ liệu khác nhau'):
        storage.migrate_legacy(old,target)
    assert (old/'projects/abc/beat.wav').read_bytes()==b'audio payload'
    assert (target/'projects/abc/beat.wav').read_bytes()==b'new project audio'


def test_equal_files_merge_with_existing_projects(tmp_path):
    old,target=source_data(tmp_path)
    (target/'projects/abc').mkdir(parents=True)
    (target/'projects/abc/beat.wav').write_bytes(b'audio payload')
    (target/'projects/another').mkdir()
    (target/'projects/another/beat.wav').write_bytes(b'keep existing')
    storage.migrate_legacy(old,target)
    assert (target/'projects/another/beat.wav').read_bytes()==b'keep existing'


def test_failed_copy_retains_original_and_can_resume(tmp_path,monkeypatch):
    old,target=source_data(tmp_path)
    real_hash=storage._hash
    def broken(path):
        if path.name.startswith('.migrating-'):raise OSError('disk error')
        return real_hash(path)
    monkeypatch.setattr(storage,'_hash',broken)
    with pytest.raises(OSError,match='disk error'):storage.migrate_legacy(old,target)
    assert (old/'models/model.th').read_bytes()==b'model payload'
    assert not list(target.rglob('.migrating-*'))
    monkeypatch.setattr(storage,'_hash',real_hash)
    assert storage.migrate_legacy(old,target)['state']=='complete'


def test_cancellation_keeps_original_data(tmp_path):
    old,target=source_data(tmp_path)
    with pytest.raises(storage.MigrationCancelled):storage.migrate_legacy(old,target,cancelled=lambda:True)
    assert (old/'projects/abc/beat.wav').is_file()


def test_disk_full_keeps_original_data(tmp_path,monkeypatch):
    old,target=source_data(tmp_path)
    monkeypatch.setattr(storage.shutil,'disk_usage',lambda path:shutil._ntuple_diskusage(100,99,1))
    with pytest.raises(RuntimeError,match='không đủ chỗ'):storage.migrate_legacy(old,target)
    assert (old/'models/model.th').is_file()


def test_overlapping_directories_are_rejected(tmp_path):
    old,target=source_data(tmp_path)
    with pytest.raises(RuntimeError,match='bên trong nhau'):storage.migrate_legacy(old,old/'data')


def test_read_only_destination_explains_how_to_fix(tmp_path,monkeypatch):
    def denied(**kwargs):raise PermissionError('denied')
    monkeypatch.setattr(storage.tempfile,'NamedTemporaryFile',denied)
    with pytest.raises(RuntimeError,match='thư mục có quyền ghi'):storage.data_directory(tmp_path/'installed')


def test_interrupted_publish_resumes_without_losing_old_or_new_data(tmp_path,monkeypatch):
    old,target=source_data(tmp_path)
    rename=Path.rename
    def fail_once(self,destination):
        if self==old:raise OSError('old app still using directory')
        return rename(self,destination)
    monkeypatch.setattr(Path,'rename',fail_once)
    with pytest.raises(RuntimeError,match='đang được sử dụng'):storage.migrate_legacy(old,target)
    assert (old/'models/model.th').exists()
    monkeypatch.setattr(Path,'rename',rename)
    assert storage.migrate_legacy(old,target)['state']=='complete'


def test_linked_audio_versions_keep_shared_storage(tmp_path):
    import os
    old,target=source_data(tmp_path)
    os.link(old/'projects/abc/beat.wav',old/'projects/abc/beat-copy.wav')
    storage.migrate_legacy(old,target)
    assert (target/'projects/abc/beat.wav').stat().st_ino==(target/'projects/abc/beat-copy.wav').stat().st_ino


def test_simultaneous_migration_is_blocked(tmp_path):
    old,target=source_data(tmp_path)
    with storage._migration_lock(old):
        with pytest.raises(RuntimeError,match='đang chuyển dữ liệu'):
            storage.migrate_legacy(old,target)
    assert (old/'models/model.th').is_file()


def test_corrupt_copy_is_rejected_before_any_source_deletion(tmp_path,monkeypatch):
    old,target=source_data(tmp_path)
    real_hash=storage._hash
    monkeypatch.setattr(storage,'_hash',lambda path:'corrupted' if path.name.startswith('.migrating-') else real_hash(path))
    with pytest.raises(RuntimeError,match='Không xác minh'):storage.migrate_legacy(old,target)
    assert (old/'projects/abc/beat.wav').read_bytes()==b'audio payload'
    assert (old/'models/model.th').read_bytes()==b'model payload'


def test_locked_original_is_retained_until_next_startup(tmp_path,monkeypatch):
    old,target=source_data(tmp_path)
    unlink=Path.unlink
    def locked(self,*args,**kwargs):
        if self.name=='model.th' and any('.migrated-' in part for part in self.parts):raise PermissionError('loaded model')
        return unlink(self,*args,**kwargs)
    monkeypatch.setattr(Path,'unlink',locked)
    result=storage.migrate_legacy(old,target)
    assert result['state']=='published' and 'models/model.th' in result['remaining']
    assert (Path(result['backup'])/'models/model.th').read_bytes()==b'model payload'
    monkeypatch.setattr(Path,'unlink',unlink)
    assert storage.migrate_legacy(old,target)['state']=='complete'
    assert not Path(result['backup']).exists()


def test_power_failure_before_publish_can_resume_relinked_metadata(tmp_path,monkeypatch):
    old,target=source_data(tmp_path)
    write=storage._write_json
    def power_failure(path,value):
        write(path,value)
        if value.get('state')=='publishing':raise OSError('power failure')
    monkeypatch.setattr(storage,'_write_json',power_failure)
    with pytest.raises(OSError,match='power failure'):storage.migrate_legacy(old,target)
    assert (old/'projects/abc/beat.wav').is_file()
    monkeypatch.setattr(storage,'_write_json',write)
    assert storage.migrate_legacy(old,target)['state']=='complete'
    assert json.loads((target/'jobs/job.json').read_text())['result']['output_path']==str(target/'projects/abc/video.mp4')


def test_changed_destination_is_not_used_to_delete_retained_original(tmp_path,monkeypatch):
    old,target=source_data(tmp_path)
    unlink=Path.unlink
    def locked(self,*args,**kwargs):
        if self.name=='model.th' and any('.migrated-' in part for part in self.parts):raise PermissionError('locked')
        return unlink(self,*args,**kwargs)
    monkeypatch.setattr(Path,'unlink',locked)
    first=storage.migrate_legacy(old,target)
    monkeypatch.setattr(Path,'unlink',unlink)
    (target/'models/model.th').write_bytes(b'changed after migration')
    result=storage.migrate_legacy(old,target)
    assert result['state']=='published'
    assert (Path(first['backup'])/'models/model.th').read_bytes()==b'model payload'
