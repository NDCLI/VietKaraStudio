"""Nuitka executable entry; workers dispatch before importing the desktop server."""
import os
from pathlib import Path
import sys

COMPILED='__compiled__' in globals()
_dll_handles=[]


class VendorStdlibFinder:
    """Resolve dynamically imported children of partially compiled stdlib packages."""
    def __init__(self,library):self.library=library

    def find_spec(self,fullname,path=None,target=None):
        from importlib.machinery import PathFinder
        parent=fullname.rpartition('.')[0]
        if not parent:return None
        directory=self.library.joinpath(*parent.split('.'))
        if directory.is_dir():return PathFinder.find_spec(fullname,[str(directory)])
        return None


def prepare_environment():
    if COMPILED:
        # Nuitka's default sys.executable initially names python.exe until its
        # multiprocessing hook runs. Workers must reenter the real EXE even
        # before importing Torch or any multiprocessing-dependent library.
        sys.executable=__compiled__.process_exe
        sys._base_executable=sys.executable
    root=Path(sys.executable).resolve().parent if COMPILED else Path(__file__).resolve().parent
    if COMPILED:
        # Preserve the source-based JIT dependencies (Numba/librosa/torch) while
        # application modules and the entire task protocol remain compiled.
        vendor=root/'vendor'
        for path in reversed((vendor/'Lib/site-packages',vendor/'Lib',vendor/'DLLs')):
            sys.path.insert(0,str(path))
        sys.meta_path.append(VendorStdlibFinder(vendor/'Lib'))
        if os.name=='nt':
            site=vendor/'Lib/site-packages'
            # Torch/CT2 manage their own search scopes. Registering both here
            # can select a different OpenMP DLL and deadlock their imports.
            for path in (root,vendor/'DLLs',*site.glob('*.libs')):
                if not path.is_dir():continue
                _dll_handles.append(os.add_dll_directory(str(path)))
        os.environ.setdefault('TCL_LIBRARY',str(vendor/'tcl/tcl8.6'))
        os.environ.setdefault('TK_LIBRARY',str(vendor/'tcl/tk8.6'))
        local=Path(os.environ.get('LOCALAPPDATA',Path.home()/'AppData/Local'))/'VietKaraStudio'
        data=Path(os.environ.get('OBS_DATA_DIR',root/'data' if (root/'portable.flag').is_file() else local)).resolve()
        data.mkdir(parents=True,exist_ok=True)
        os.environ['OBS_DATA_DIR']=str(data)
        os.environ['VIETKARA_PRODUCTION']='1'
        os.environ.setdefault('NUMBA_CACHE_DIR',str(data/'cache/numba'))
        os.environ.setdefault('MPLCONFIGDIR',str(data/'cache/matplotlib'))
    os.environ['PYTHONUTF8']='1'
    os.environ['PYTHONNOUSERSITE']='1'
    sys.dont_write_bytecode=True
    for stream in (sys.stdout,sys.stderr):
        if hasattr(stream,'reconfigure'):stream.reconfigure(encoding='utf-8',errors='replace')
    return root


def main():
    root=prepare_environment()
    from app_runtime import dispatch
    if sys.argv[1:2]==['--vcl-task']:
        if len(sys.argv)<3:raise ValueError('Missing application task')
        dispatch(sys.argv[2],sys.argv[3:])
        return
    # A compiled executable must never act as a generic Python interpreter.
    if any(arg in ('-c','-m','--vcl-task') or arg.endswith('.py') for arg in sys.argv[1:]):
        raise ValueError('Invalid desktop arguments')
    if COMPILED:
        import logging
        from logging.handlers import RotatingFileHandler
        logs=Path(os.environ['OBS_DATA_DIR'])/'logs';logs.mkdir(parents=True,exist_ok=True)
        handler=RotatingFileHandler(logs/'VietKaraStudio.log',maxBytes=5*1024**2,backupCount=3,encoding='utf-8')
        handler.setFormatter(logging.Formatter('%(asctime)s %(levelname)s %(message)s'))
        logging.basicConfig(level=logging.INFO,handlers=[handler])
        logging.info('Nuitka standalone desktop; executable=%s',sys.executable)
    from launch import main as launch
    mutex=None
    if COMPILED and os.name=='nt' and '--no-open' not in sys.argv:
        import ctypes
        kernel=ctypes.WinDLL('kernel32',use_last_error=True)
        kernel.CreateMutexW.argtypes=[ctypes.c_void_p,ctypes.c_bool,ctypes.c_wchar_p]
        kernel.CreateMutexW.restype=ctypes.c_void_p
        kernel.CloseHandle.argtypes=[ctypes.c_void_p]
        mutex=kernel.CreateMutexW(None,False,'Local\\VietKaraStudio_1_0')
        if not mutex:raise ctypes.WinError(ctypes.get_last_error())
        if ctypes.get_last_error()==183:
            kernel.CloseHandle(mutex)
            ctypes.windll.user32.MessageBoxW(None,'VietKara Studio đang chạy. Hãy mở lại cửa sổ đang có.','VietKara Studio',0)
            return
    try:launch()
    finally:
        if mutex:kernel.CloseHandle(mutex)


if __name__=='__main__':
    try:
        main()
    except Exception:
        if '--vcl-task' in sys.argv or '--no-open' in sys.argv:raise
        import logging
        logging.exception('VietKara Studio failed to start')
        if os.name=='nt':
            import ctypes
            logs=Path(os.environ.get('OBS_DATA_DIR','.'))/'logs/VietKaraStudio.log'
            ctypes.windll.user32.MessageBoxW(None,'Không khởi động được VietKara Studio.\nChi tiết: '+str(logs),'VietKara Studio',0x10)
        raise SystemExit(1)
