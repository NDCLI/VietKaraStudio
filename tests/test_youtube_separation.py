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
    job = type('Job', (), {'cancelled': type('Event', (), {'is_set': lambda self: False})(),
                            'check': lambda self: None,
                            'update': lambda self, value, message: events.append((value, message))})()
    result = media_tools.download_youtube('https://youtu.be/abcDEF_123', tmp_path, job)
    assert result.name == 'youtube_source.webm'
    assert events and events[-1][0] == 40
