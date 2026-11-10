"""Background video is visual only; its audio is never mapped into an export."""
import json
import hashlib
import os
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.request import Request, urlopen
from urllib.parse import parse_qs, urlparse

import hardware

VIDEO_EXTENSIONS = ('.mp4','.mov','.mkv','.webm','.avi','.m4v')
IMAGE_EXTENSIONS = ('.jpg','.jpeg','.png','.webp')
YOUTUBE_HOSTS = frozenset(('youtube.com', 'www.youtube.com', 'm.youtube.com', 'youtu.be'))
YOUTUBE_TOOLS = {
    'yt-dlp.exe': (
        'https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.26/yt-dlp.exe',
        'f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d', 23575744,
    ),
    'qjs.exe': (
        'https://github.com/quickjs-ng/quickjs/releases/download/v0.18.0/qjs-windows-x86_64.exe',
        'ff03398d74ea6e6bd3a8e58058f7003225abcac229d7d57775b7aaae812a49c7', 1873152,
    ),
}


def youtube_video_id(value):
    """Return a single YouTube video id, rejecting playlists and non-video URLs."""
    try:
        parsed = urlparse(str(value).strip())
    except ValueError:
        return None
    if parsed.scheme not in ('http', 'https') or parsed.hostname not in YOUTUBE_HOSTS:
        return None
    query = parse_qs(parsed.query)
    if 'list' in query:
        return None
    if parsed.hostname == 'youtu.be':
        video_id = parsed.path.strip('/').split('/')[0]
    else:
        if parsed.path.rstrip('/') not in ('', '/watch'):
            return None
        video_id = query.get('v', [''])[0]
    if not re.fullmatch(r'[A-Za-z0-9_-]{6,20}', video_id or ''):
        return None
    return video_id


def _windows_youtube_tool(name, folder, job):
    url, expected, size = YOUTUBE_TOOLS[name]
    # Include the upstream digest so a future release can run alongside an older job.
    folder = folder / expected[:16]
    folder.mkdir(parents=True, exist_ok=True)
    tool = folder / name

    def valid():
        try:
            with tool.open('rb') as stream:
                return hashlib.file_digest(stream, 'sha256').hexdigest() == expected
        except FileNotFoundError:
            return False

    if valid():
        return tool
    job.update(5, f'Đang tải công cụ YouTube ({name}) từ GitHub…')
    request = Request(url, headers={'User-Agent': 'VietKaraStudio'})
    with tempfile.TemporaryDirectory(prefix='download-', dir=folder) as staging:
        temp = Path(staging) / name
        digest = hashlib.sha256()
        total = 0
        with urlopen(request, timeout=25) as response, temp.open('wb') as stream:
            while chunk := response.read(256 * 1024):
                job.check()
                total += len(chunk)
                if total > size:
                    raise ValueError('Công cụ YouTube vượt dung lượng dự kiến.')
                digest.update(chunk)
                stream.write(chunk)
        if total != size or digest.hexdigest() != expected:
            raise ValueError('Tệp công cụ YouTube tải về không khớp mã kiểm tra.')
        job.check()
        try:
            temp.replace(tool)
        except PermissionError:
            if not valid():
                raise
    return tool


def _download_youtube_windows(url, output, job):
    import engine
    folder = engine.DATA / 'tools' / 'youtube-runtime'
    try:
        downloader = _windows_youtube_tool('yt-dlp.exe', folder, job)
        javascript = _windows_youtube_tool('qjs.exe', folder, job)
    except (OSError, ValueError) as exc:
        raise RuntimeError(f'Không chuẩn bị được công cụ YouTube từ GitHub: {exc}') from exc

    job.update(5, 'Đang kết nối YouTube…')

    def progress(line):
        match = re.search(r'VCL_PROGRESS:\s*(\d+(?:\.\d+)?)%', line)
        if match:
            job.update(min(40, 5 + float(match[1]) * .35), 'Đang tải nguồn YouTube…')

    # The Windows distribution includes the YouTube solver scripts.
    engine.command([downloader, '--ignore-config', '--no-plugin-dirs', '--no-cache-dir',
                    '--no-playlist', '--restrict-filenames', '--no-mtime', '--socket-timeout', '15',
                    '--retries', '1', '--fragment-retries', '1', '--extractor-retries', '1',
                    '--js-runtimes', f'quickjs:{javascript}', '--ffmpeg-location', engine.binary('ffmpeg'),
                    '--newline', '--progress', '--progress-template', 'download:VCL_PROGRESS:%(progress._percent_str)s',
                    '-f', 'bestaudio/best', '-o', output, '--', url], job, progress=progress)


def youtube_available():
    import importlib.util
    return os.name == 'nt' or importlib.util.find_spec('yt_dlp') is not None


def download_youtube(url, destination, job):
    """Download one user-authorized YouTube video into a temporary job folder."""
    video_id = youtube_video_id(url)
    if not video_id:
        raise ValueError('URL YouTube không hợp lệ hoặc có playlist. Hãy dán link một video đơn.')
    url = f'https://www.youtube.com/watch?v={video_id}'
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    output = destination / 'youtube_source.%(ext)s'
    try:
        if os.name == 'nt':
            _download_youtube_windows(url, output, job)
        else:
            _download_youtube_python(url, output, job)
    except Exception as exc:
        if job.cancelled.is_set():
            raise
        raise RuntimeError('Không tải được video YouTube: ' + str(exc).strip()[-600:]) from exc
    candidates = [path for path in destination.glob('youtube_source.*')
                  if path.is_file() and path.suffix not in ('.part', '.ytdl', '.tmp')]
    if len(candidates) != 1:
        raise RuntimeError('YouTube không trả về file âm thanh hợp lệ.')
    return candidates[0]


def _download_youtube_python(url, output, job):
    try:
        import yt_dlp
    except ImportError as exc:
        raise RuntimeError('Thiếu yt-dlp. Hãy cài lại dependency của app rồi thử lại.') from exc
    def hook(item):
        job.check()
        if item.get('status') == 'downloading':
            total = item.get('total_bytes') or item.get('total_bytes_estimate') or 0
            received = item.get('downloaded_bytes') or 0
            percent = 5 + (received * 35 / total if total else 0)
            job.update(min(40, percent), 'Đang tải nguồn YouTube…')
        elif item.get('status') == 'finished':
            job.update(40, 'Đã tải nguồn YouTube, đang chuẩn bị tách vocal…')

    options = {
        'format': 'bestaudio/best',
        'outtmpl': str(output),
        'noplaylist': True,
        'quiet': True,
        'no_warnings': True,
        'progress_hooks': [hook],
        'restrictfilenames': True,
        'socket_timeout': 15,
        'retries': 1,
    }
    with yt_dlp.YoutubeDL(options) as downloader:
        downloader.download([url])


def probe_video(path):
    import engine
    try:
        executable = engine.binary('ffprobe')
    except RuntimeError:
        # imageio-ffmpeg ships ffmpeg but not ffprobe. Keep source installs usable
        # by parsing the stable human-readable stream summary as a fallback.
        result = subprocess.run([engine.binary('ffmpeg'), '-hide_banner', '-i', str(path)],
                                stdin=subprocess.DEVNULL, capture_output=True, text=True,
                                encoding='utf-8', errors='replace', timeout=30,
                                creationflags=engine.CREATE_NO_WINDOW)
        details = result.stderr
        match = re.search(r'Duration: (\d+):(\d+):(\d+(?:\.\d+)?)', details)
        stream = re.search(r'Video:.*?,\s+(\d+)x(\d+)(?:\s|\[).*?(\d+(?:\.\d+)?) fps', details)
        if not stream:
            stream = re.search(r'Video:.*?,\s+(\d+)x(\d+)(?:\s|\[)', details)
        if not stream:
            raise ValueError('Không đọc được thông tin video.')
        duration = ((int(match.group(1)) * 60 + int(match.group(2))) * 60 + float(match.group(3))) if match else 0.
        fps = float(stream.group(3)) if len(stream.groups()) >= 3 else 30.
        return {'duration': duration, 'fps': fps, 'width': int(stream.group(1)), 'height': int(stream.group(2))}
    r = subprocess.run([executable, '-v', 'error', '-show_streams', '-show_format', '-of', 'json', str(path)],
                       stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding='utf-8',
                       errors='replace', timeout=30, creationflags=engine.CREATE_NO_WINDOW)
    if r.returncode:
        raise ValueError('Không đọc được thông tin video.')
    data = json.loads(r.stdout)
    video = next((s for s in data.get('streams', []) if s.get('codec_type') == 'video' and
                  not s.get('disposition', {}).get('attached_pic')), None)
    if not video:
        raise ValueError('File không có hình ảnh video.')
    from render_info import fps_value
    return {'duration': float(data.get('format', {}).get('duration', video.get('duration', 0))),
            'fps': fps_value(video.get('avg_frame_rate') or video.get('r_frame_rate', '30')),
            'width': int(video['width']), 'height': int(video['height'])}


def make_proxy(source, destination, job):
    import engine
    video=hardware.detect().get('video',{'encoder':'libx264','name':'CPU'})
    job.record_device('video_prepare',video)
    job.update(12,'Đang chuẩn bị xem trước video · '+video['name']+'…')
    common=['-y','-v','error','-i',str(source),'-map','0:v:0','-an','-vf',"scale='min(960,iw)':-2",'-r','24']
    suffix=['-pix_fmt','yuv420p','-movflags','+faststart',str(destination)]
    try:
        engine.command([video.get('ffmpeg') or engine.binary('ffmpeg')]+common+
                       (['-c:v','h264_nvenc','-preset','p4','-cq','27'] if video['encoder']=='h264_nvenc' else
                        ['-c:v','libx264','-preset','veryfast','-crf','27'])+suffix,job)
    except RuntimeError as exc:
        if video['encoder']!='h264_nvenc' or not hardware.gpu_error(exc):raise
        job.check()
        job.record_device('video_prepare',{'encoder':'libx264','name':'CPU'},str(exc))
        engine.command([engine.binary('ffmpeg')]+common+['-c:v','libx264','-preset','veryfast','-crf','27']+suffix,job)


def local_asset(directory, name):
    if not isinstance(name,str) or Path(name).name != name:
        return None
    path=directory/name
    return path if path.is_file() else None


def background_source(project, directory):
    settings=project.get('settings',{})
    kind=settings.get('background_type','auto')
    if kind=='original':
        for slot in ('reference','beat'):
            meta=(project.get(slot) or {}).get('video',{})
            path=local_asset(directory,meta.get('source_file'))
            if path:return 'video',path
        raise ValueError('Chưa có video gốc. Hãy nạp bản hát/beat dạng video hoặc chọn nền khác.')
    if kind=='video':
        path=local_asset(directory,project.get('background_video',{}).get('source_file'))
        if path:return 'video',path
        raise ValueError('Hãy nạp video nền trước khi xuất.')
    if kind=='image' or (kind=='auto' and settings.get('use_background',True)):
        path=directory/'background.jpg'
        if path.is_file():return 'image',path
        if kind=='image':raise ValueError('Hãy nạp ảnh nền trước khi xuất.')
    return 'color',None


def custom_countdown_filter(project, directory, plan, width, height, input_index=2):
    """Return an FFmpeg overlay graph for a custom transparent PNG."""
    if project['settings']['countdown']!='image' or project['settings']['display_mode']!='karaoke' or not plan['countdowns']:
        return None
    if not (directory/'countdown.png').is_file():raise ValueError('Hãy chọn ảnh đếm ngược PNG trước khi xuất.')
    factor=width/plan['width']
    # Four stable slots, disappearing right to left. Position and size can change between sections.
    events=[]
    for window in plan['countdowns']:
        for j in range(4):
            if window['end']-j>window['start']:
                events.append((window,j))
    parts=[f'[{input_index}:v]format=rgba,split={len(events)}'+''.join(f'[cd{i}]' for i in range(len(events)))+';']
    current='base'
    for i,(window,j) in enumerate(events):
        size=max(2,round(window['size']*factor))
        x=(window['x']+j*window['size']*1.28)*factor
        y=window['y']*factor
        parts.append(f'[cd{i}]scale={size}:{size}:force_original_aspect_ratio=decrease,pad={size}:{size}:(ow-iw)/2:(oh-ih)/2:color=black@0[icon{i}];')
        parts.append(f"[{current}][icon{i}]overlay=x={x:.2f}:y={y:.2f}:enable='between(t,{window['start']:.4f},{window['end']-j:.4f})'[ov{i}];")
        current=f'ov{i}'
    return ''.join(parts),current


def logo_filter(project, directory, width, height, input_index, base):
    """Overlay the same bounded image and time-based effects as the preview."""
    s=project['settings']
    if not s.get('logo_enabled') or not project.get('logo_name'):
        return None
    if not (directory/'logo.png').is_file():
        raise ValueError('Không tìm thấy logo. Hãy chọn lại ảnh trong Cá nhân hóa.')
    from PIL import Image
    with Image.open(directory/'logo.png') as image:
        ratio=image.width/image.height
    w=max(2,round(min(width*s['logo_size']/100,height*.4*ratio)))
    h=max(2,round(w/ratio))
    alpha=s['logo_opacity']/100
    effect=s['logo_effect']
    filters=f'setpts=PTS-STARTPTS,scale={w}:{h},format=rgba,colorchannelmixer=aa={alpha:.4f}'
    if effect=='fade':
        filters+=',fade=t=in:st=0:d=1:alpha=1'
    elif effect=='pulse':
        filters+=",geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='alpha(X,Y)*(0.8+0.2*sin(PI*T/2))'"
    x=f'(W-w)*{s["logo_x"]/100:.6f}'
    y=f'(H-h)*{s["logo_y"]/100:.6f}'
    if effect=='float':
        y=f'max(0,min(H-h,{y}+{8*height/1080:.4f}*sin(PI*t/2)))'
    return f"[{input_index}:v]{filters}[watermark];[{base}][watermark]overlay=x='{x}':y='{y}':format=auto[withlogo]",'withlogo'
