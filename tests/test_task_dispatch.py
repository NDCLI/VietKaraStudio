import json
from pathlib import Path
import subprocess
import sys

import pytest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
import app_runtime
import engine


def test_worker_process_does_not_inherit_invalid_gui_stdin(monkeypatch):
    calls = []

    class Process:
        stdout = ()

        def poll(self):
            return 0

        def wait(self):
            return 0

    def popen(*args, **kwargs):
        calls.append(kwargs)
        return Process()

    monkeypatch.setattr(engine.subprocess, 'Popen', popen)
    engine.command(['worker.exe'])
    assert calls[0]['stdin'] is engine.subprocess.DEVNULL


def test_compiled_tasks_reenter_product_executable_without_python_interpreter_flags(monkeypatch):
    monkeypatch.setattr(app_runtime,'COMPILED',True)
    monkeypatch.setattr(sys,'executable','C:/Product/VietKaraStudio.exe')
    assert app_runtime.task_command('speech',Path('request.json'))==[
        'C:/Product/VietKaraStudio.exe','--vcl-task','speech','request.json']
    assert app_runtime.task_command('demucs','--two-stems','vocals')==[
        'C:/Product/VietKaraStudio.exe','--vcl-task','demucs','--two-stems','vocals']


def test_source_tasks_use_same_dispatch_contract(monkeypatch):
    monkeypatch.setattr(app_runtime,'COMPILED',False)
    args=app_runtime.task_command('probe')
    assert args[0]==sys.executable and Path(args[2]).name=='desktop_entry.py'
    assert args[3:]==['--vcl-task','probe']


def test_dispatch_rejects_arbitrary_code_and_missing_request_files():
    with pytest.raises(ValueError):app_runtime.task_command('eval','1+1')
    with pytest.raises(ValueError):app_runtime.dispatch('python',['-c','1+1'])
    with pytest.raises(ValueError):app_runtime.dispatch('probe',['unexpected'])
    with pytest.raises(ValueError):app_runtime.dispatch('speech',[])


def test_source_entry_unknown_task_never_launches_server(tmp_path):
    result=subprocess.run([sys.executable,str(Path(app_runtime.__file__).with_name('desktop_entry.py')),
                           '--vcl-task','unknown'],cwd=tmp_path,capture_output=True,text=True,timeout=10)
    assert result.returncode!=0
    assert 'Unknown application task' in result.stderr
    assert '127.0.0.1' not in result.stdout


def test_vendor_finder_resolves_children_of_compiled_stdlib_packages(tmp_path):
    import importlib
    from types import ModuleType
    from desktop_entry import VendorStdlibFinder
    package=ModuleType('vcl_test_stdlib');package.__path__=[]
    folder=tmp_path/'vcl_test_stdlib';folder.mkdir()
    (folder/'dynamic.py').write_text('VALUE = 42\n',encoding='utf-8')
    finder=VendorStdlibFinder(tmp_path)
    sys.modules[package.__name__]=package;sys.meta_path.append(finder)
    try:
        child=importlib.import_module('vcl_test_stdlib.dynamic')
        assert child.VALUE==42
        assert Path(child.__file__).resolve().is_relative_to(tmp_path)
        assert finder.find_spec('vcl_missing_stdlib.child') is None
    finally:
        sys.meta_path.remove(finder)
        sys.modules.pop('vcl_test_stdlib.dynamic',None);sys.modules.pop('vcl_test_stdlib',None)


def test_compiled_entry_sets_real_executable_before_library_imports(tmp_path,monkeypatch):
    import desktop_entry
    from types import SimpleNamespace
    executable=str(tmp_path/'RenamedStudio.exe')
    monkeypatch.setattr(desktop_entry,'COMPILED',True)
    monkeypatch.setattr(desktop_entry,'__compiled__',SimpleNamespace(process_exe=executable),raising=False)
    monkeypatch.setattr(desktop_entry,'_dll_handles',[])
    monkeypatch.setattr(sys,'executable',str(tmp_path/'python.exe'))
    monkeypatch.setattr(sys,'_base_executable',str(tmp_path/'python.exe'),raising=False)
    monkeypatch.setattr(sys,'path',sys.path.copy())
    monkeypatch.setattr(sys,'meta_path',sys.meta_path.copy())
    monkeypatch.setenv('OBS_DATA_DIR',str(tmp_path/'test-data'))
    # Isolate all process-environment changes made by the entry point.
    for name in ('TCL_LIBRARY','TK_LIBRARY','VIETKARA_PRODUCTION','NUMBA_CACHE_DIR','MPLCONFIGDIR','PYTHONUTF8','PYTHONNOUSERSITE'):
        monkeypatch.setenv(name,'test')
    try:
        assert desktop_entry.prepare_environment()==tmp_path
        assert sys.executable==executable and sys._base_executable==executable
        assert app_runtime.task_command('probe')[0]==executable
    finally:
        for handle in desktop_entry._dll_handles:handle.close()
