import hashlib
import io
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import media_tools


def test_youtube_video_id_accepts_single_video_links():
    assert media_tools.youtube_video_id('https://www.youtube.com/watch?v=abcDEF_123') == 'abcDEF_123'
    assert media_tools.youtube_video_id('https://youtu.be/abcDEF_123?t=20') == 'abcDEF_123'
    assert media_tools.youtube_video_id('https://m.youtube.com/watch?v=abcDEF_123') == 'abcDEF_123'


@pytest.mark.parametrize('url', [
    'https://www.youtube.com/playlist?list=PL123456',
    'https://www.youtube.com/watch?v=abcDEF_123&list=PL123456',
    'https://youtube.com/channel/abcDEF_123',
    'https://example.com/watch?v=abcDEF_123',
    'not a url',
])
def test_youtube_video_id_rejects_playlists_and_other_hosts(url):
    assert media_tools.youtube_video_id(url) is None


def test_youtube_downloader_is_temporary_and_reports_progress(tmp_path, monkeypatch):
    events = []
    class FakeYoutubeDL:
        def __init__(self, options):
            self.options = options
        def __enter__(self):
            return self
        def __exit__(self, *_):
            return False
        def download(self, urls):
            self.options['progress_hooks'][0]({'status': 'downloading', 'downloaded_bytes': 5, 'total_bytes': 10})
            (tmp_path / 'youtube_source.webm').write_bytes(b'audio')
            self.options['progress_hooks'][0]({'status': 'finished'})
    monkeypatch.setitem(sys.modules, 'yt_dlp', type('Module', (), {'YoutubeDL': FakeYoutubeDL}))
    monkeypatch.setattr(media_tools, '_download_youtube_windows', media_tools._download_youtube_python)
    job = type('Job', (), {'cancelled': type('Event', (), {'is_set': lambda self: False})(),
                            'check': lambda self: None,
                            'update': lambda self, value, message: events.append((value, message))})()
    result = media_tools.download_youtube('https://youtu.be/abcDEF_123', tmp_path, job)
    assert result.name == 'youtube_source.webm'
    assert events and events[-1][0] == 40


def test_windows_downloader_verifies_and_reuses_tools(tmp_path, monkeypatch):
    payload = b'official executable'
    download_url = 'https://github.com/example/releases/download/v1/tool.exe'
    checksum = hashlib.sha256(payload).hexdigest()
    monkeypatch.setitem(media_tools.YOUTUBE_TOOLS, 'test.exe', (download_url, checksum, len(payload)))
    downloads = []

    def download(request, timeout):
        downloads.append(request.full_url)
        return io.BytesIO(payload if len(downloads) > 1 else b'wrong')

    monkeypatch.setattr(media_tools, 'urlopen', download)
    import engine
    job = engine.Job('test', 'test', 'separate')
    with pytest.raises(ValueError, match='mã kiểm tra'):
        media_tools._windows_youtube_tool('test.exe', tmp_path, job)
    tool = media_tools._windows_youtube_tool('test.exe', tmp_path, job)
    assert tool.read_bytes() == payload
    assert media_tools._windows_youtube_tool('test.exe', tmp_path, job) == tool
    assert downloads == [download_url, download_url]
