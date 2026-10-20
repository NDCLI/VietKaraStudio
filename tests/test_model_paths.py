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
