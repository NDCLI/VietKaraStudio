"""Packaged entry point: paths are resolved before importing the application."""
import faulthandler
import logging
import os
from pathlib import Path
import sys
import traceback
sys.dont_write_bytecode=True

HOME=Path(__file__).resolve().parent
portable=(HOME/'portable.flag').is_file()
default_data=HOME/'data' if portable else Path(os.environ.get('LOCALAPPDATA',Path.home()/'AppData/Local'))/'VietKaraStudio'
data=Path(os.environ.get('OBS_DATA_DIR',default_data)).resolve()
data.mkdir(parents=True,exist_ok=True)
os.environ['OBS_DATA_DIR']=str(data)
os.environ['VIETKARA_PRODUCTION']='1'
os.environ['PYTHONUTF8']='1'
os.environ['PYTHONNOUSERSITE']='1'
os.environ['PYTHONDONTWRITEBYTECODE']='1'
os.environ.setdefault('NUMBA_CACHE_DIR',str(data/'cache/numba'))
os.environ.setdefault('MPLCONFIGDIR',str(data/'cache/matplotlib'))
logs=data/'logs';logs.mkdir(parents=True,exist_ok=True)
from logging.handlers import RotatingFileHandler
handler=RotatingFileHandler(logs/'VietKaraStudio.log',maxBytes=5*1024*1024,backupCount=3,encoding='utf-8')
handler.setFormatter(logging.Formatter('%(asctime)s %(levelname)s %(message)s'))
logging.basicConfig(level=logging.INFO,handlers=[handler])
class LogStream:
    def write(self,text):
        if text.strip():logging.info(text.rstrip())
    def flush(self):pass
if '--no-open' not in sys.argv:
    sys.stdout=LogStream();sys.stderr=LogStream()
crash=(logs/'native-crash.log').open('a',encoding='utf-8')
faulthandler.enable(crash)
sys.path.insert(0,str(HOME/'app'))
os.chdir(HOME/'app')
try:
    from launch import main
    main()
except Exception:
    logging.exception('VietKara Studio failed')
    if '--no-open' in sys.argv:traceback.print_exc()
    else:
        try:
            import tkinter as tk
            from tkinter import messagebox
            root=tk.Tk();root.withdraw()
            messagebox.showerror('VietKara Studio','Không khởi động được ứng dụng.\nChi tiết lỗi: '+str(logs/'VietKaraStudio.log'))
            root.destroy()
        except Exception:pass
    sys.exit(1)
