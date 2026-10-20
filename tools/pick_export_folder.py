"""Explicitly requested native folder picker; no visible console window."""
import json
import tkinter as tk
from tkinter import filedialog

root=tk.Tk()
root.withdraw()
root.attributes('-topmost',True)
try:
    folder=filedialog.askdirectory(parent=root,title='Chọn thư mục xuất video karaoke',mustexist=True)
    print(json.dumps({'folder':folder},ensure_ascii=True),flush=True)
finally:
    root.destroy()
