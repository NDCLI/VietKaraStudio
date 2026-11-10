from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import updater


def test_apply_rejects_read_only_installation_before_launch(tmp_path, monkeypatch):
    helper = tmp_path / "VietKaraUpdater.exe"
    helper.touch()
    monkeypatch.setattr(updater, "_ready_update", lambda: {"version": "1.1.13", "folder": str(tmp_path)})
    monkeypatch.setattr(updater, "_helper", lambda: helper)

    def denied(**_):
        raise PermissionError("Access is denied")

    def unexpected(*args, **kwargs):
        pytest.fail("The updater should leave the app open when the install folder is not writable.")

    monkeypatch.setattr(updater.tempfile, "NamedTemporaryFile", denied)
    monkeypatch.setattr(updater.subprocess, "Popen", unexpected)
    with pytest.raises(ValueError, match="từ chối quyền ghi"):
        updater.apply()
