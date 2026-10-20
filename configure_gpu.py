"""Install a matching optional CUDA runtime once when an NVIDIA card is present."""
from __future__ import annotations

import importlib.metadata
from pathlib import Path
import subprocess
import sys

from hardware import nvidia_cards


def main():
    cards = nvidia_cards()
    if not cards:
        print("Khong phat hien NVIDIA GPU. App se dung CPU.", flush=True)
        return
    try:
        version = importlib.metadata.version("torch")
    except importlib.metadata.PackageNotFoundError:
        version = ""
    if "+cu" in version:
        return
    print("Da phat hien GPU: " + ", ".join(card["name"] for card in cards), flush=True)
    print("Dang cai CUDA cho app (lan dau can tai khoang 2.5 GB)...", flush=True)
    result = subprocess.run([sys.executable, "-m", "pip", "install", "--disable-pip-version-check",
                             "--timeout", "30", "--retries", "1", "--no-deps", "--upgrade",
                             "-r", str(Path(__file__).with_name("requirements-gpu.txt"))])
    if result.returncode:
        print("Chua cai duoc CUDA. App se kiem tra lai va dung CPU neu can.", flush=True)


if __name__ == "__main__":
    main()
