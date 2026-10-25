import hashlib
import io
from pathlib import Path
import sys

import pytest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import model_manager as models


class Response(io.BytesIO):
    def __init__(self,data,status=200,**headers):
        super().__init__(data);self.status=status
        self.headers={'Content-Length':str(len(data)),**headers}


@pytest.fixture
def checkpoint(monkeypatch):
    data=b'valid checkpoint bytes'
    monkeypatch.setattr(models,'DEMUCS_SHA256',hashlib.sha256(data).hexdigest())
    monkeypatch.setattr(models,'cached_demucs',lambda root:None)
    monkeypatch.setattr(models.time,'sleep',lambda seconds:None)
    return data


def test_fresh_download_is_atomic_and_emits_progress(tmp_path,monkeypatch,checkpoint,capsys):
    target=tmp_path/'models'/models.DEMUCS_FILE
    def fetch(request,timeout):
        assert request.full_url==models.DEMUCS_URL and not target.exists()
        return Response(checkpoint)
    monkeypatch.setattr(models.urllib.request,'urlopen',fetch)
    assert models.download_demucs(tmp_path)==target
    assert target.read_bytes()==checkpoint
    assert not target.with_suffix('.th.partial').exists()
    assert 'OBS_PROGRESS' in capsys.readouterr().out


@pytest.mark.parametrize('supports_range',[True,False])
def test_resume_and_server_ignoring_range(tmp_path,monkeypatch,checkpoint,supports_range):
    target=tmp_path/'models'/models.DEMUCS_FILE;target.parent.mkdir()
    target.with_suffix('.th.partial').write_bytes(checkpoint[:5])
    def fetch(request,timeout):
        assert request.headers['Range']=='bytes=5-'
        return Response(checkpoint[5:],206,**{'Content-Range':f'bytes 5-{len(checkpoint)-1}/{len(checkpoint)}'}) if supports_range else Response(checkpoint)
    monkeypatch.setattr(models.urllib.request,'urlopen',fetch)
    assert models.download_demucs(tmp_path).read_bytes()==checkpoint


def test_corrupt_download_never_replaces_existing_model(tmp_path,monkeypatch,checkpoint):
    target=tmp_path/'models'/models.DEMUCS_FILE;target.parent.mkdir();target.write_bytes(checkpoint)
    calls=[]
    def fetch(*args,**kwargs):
        calls.append(True);return Response(b'corrupt data')
    monkeypatch.setattr(models.urllib.request,'urlopen',fetch)
    with pytest.raises(RuntimeError,match='SHA-256'):models.download_demucs(tmp_path,force_download=True)
    assert len(calls)==3 and target.read_bytes()==checkpoint


def test_network_failure_preserves_partial_for_next_attempt(tmp_path,monkeypatch,checkpoint):
    target=tmp_path/'models'/models.DEMUCS_FILE;target.parent.mkdir()
    partial=target.with_suffix('.th.partial');partial.write_bytes(checkpoint[:5])
    def fail(*args,**kwargs):raise OSError('connection lost')
    monkeypatch.setattr(models.urllib.request,'urlopen',fail)
    with pytest.raises(RuntimeError,match='tải lại'):models.download_demucs(tmp_path)
    assert partial.read_bytes()==checkpoint[:5] and not target.exists()


def test_existing_local_model_is_reused_and_corrupt_model_rejected(tmp_path,monkeypatch):
    data=b'known model';monkeypatch.setattr(models,'DEMUCS_SHA256',hashlib.sha256(data).hexdigest())
    target=tmp_path/'models'/models.DEMUCS_FILE;target.parent.mkdir();target.write_bytes(data)
    monkeypatch.setenv('TORCH_HOME',str(tmp_path/'empty-cache'))
    assert models.cached_demucs(tmp_path)==target
    assert models.download_demucs(tmp_path)==target
    target.write_bytes(b'damaged')
    assert models.cached_demucs(tmp_path) is None
