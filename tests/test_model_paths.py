from pathlib import Path
import sys

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import model_manager


def complete(folder):
    folder.mkdir(parents=True,exist_ok=True)
    for name in ('model.bin','config.json','tokenizer.json'):(folder/name).write_text('test')
    return folder


def test_existing_shared_cache_is_used_without_downloading(tmp_path,monkeypatch):
    import faster_whisper.utils
    shared=complete(tmp_path/'shared-cache')
    calls=[]
    def fake(name,**kwargs):
        calls.append(kwargs)
        assert kwargs['local_files_only'] is True
        if kwargs.get('cache_dir'):raise ValueError('cache absent')
        return str(shared)
    monkeypatch.setattr(faster_whisper.utils,'download_model',fake)
    assert model_manager.resolve_whisper(tmp_path,'small')==shared
    assert len(calls)==2
    local=complete(tmp_path/'models/whisper/small')
    assert model_manager.resolve_whisper(tmp_path,'small')==local
    assert len(calls)==2


def test_automatic_download_writes_to_app_models_and_cache(tmp_path,monkeypatch):
    import faster_whisper.utils
    downloads=[]
    def fake(name,**kwargs):
        if kwargs.get('local_files_only'):raise ValueError('not cached')
        downloads.append(kwargs)
        return str(complete(Path(kwargs['output_dir'])))
    monkeypatch.setattr(faster_whisper.utils,'download_model',fake)
    assert model_manager.resolve_whisper(tmp_path,'medium')==tmp_path/'models/whisper/medium'
    assert downloads==[{'output_dir':str(tmp_path/'models/whisper/medium'),'cache_dir':str(tmp_path/'cache/huggingface/hub')}]


def test_manual_backing_model_download_and_file_detection(tmp_path, monkeypatch):
    import hashlib
    import backing_vocals
    contents = {'quality.ckpt': b'quality model', 'quality.yaml': b'config', 'light.pth': b'light model'}
    def files(model):
        names = ['quality.ckpt', 'quality.yaml'] if model == 'roformer' else ['light.pth']
        return [(name, hashlib.sha256(contents[name]).hexdigest(), 'example/models') for name in names]
    monkeypatch.setattr(backing_vocals, 'model_files', files)
    monkeypatch.setattr(model_manager, 'cached_whisper', lambda *args: None)
    monkeypatch.setattr(model_manager, 'cached_demucs', lambda *args: None)
    folder = tmp_path / 'models/backing'
    folder.mkdir(parents=True)
    (folder / 'quality.ckpt').write_bytes(contents['quality.ckpt'])
    items = {item['id']: item for item in model_manager.status(tmp_path)}
    assert not items['backing-roformer']['ready']
    assert not items['backing-uvr-lite']['ready']
    calls = []
    def download(target, staging, repository, checksum, progress):
        if backing_vocals.fingerprint(target) != checksum:
            calls.append(target.name)
            target.write_bytes(contents[target.name])
    monkeypatch.setattr(backing_vocals, '_download_model_file', download)
    model_manager.download(tmp_path, 'backing-roformer')
    model_manager.download(tmp_path, 'backing-uvr-lite')
    model_manager.download(tmp_path, 'backing-uvr-lite')
    assert calls == ['quality.yaml', 'light.pth']
    items = {item['id']: item for item in model_manager.status(tmp_path)}
    assert items['backing-roformer']['ready'] and items['backing-uvr-lite']['ready']
    assert len(items['backing-roformer']['files']) == 2
    # Manually copied, corrupted weights must not appear ready.
    (folder / 'light.pth').write_bytes(b'broken')
    assert not {item['id']: item for item in model_manager.status(tmp_path)}['backing-uvr-lite']['ready']
