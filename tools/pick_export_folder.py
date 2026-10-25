"""Explicitly requested native folder picker; no visible console window."""
from __future__ import annotations

import json


def main(arguments=None):
    import tkinter as tk
    from tkinter import filedialog
    if arguments not in (None, [], ['--check']):
        raise ValueError('Invalid folder picker arguments')
    root = tk.Tk()
    root.withdraw()
    root.attributes('-topmost', True)
    try:
        if arguments == ['--check']:
            print(json.dumps({'available': True, 'tk': root.tk.call('info', 'patchlevel')}), flush=True)
            return
        folder = filedialog.askdirectory(parent=root, title='Chọn thư mục xuất video karaoke', mustexist=True)
        print(json.dumps({'folder': folder}, ensure_ascii=True), flush=True)
    finally:
        root.destroy()


if __name__ == '__main__':
    main()
