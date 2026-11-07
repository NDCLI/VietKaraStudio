from __future__ import annotations

import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import threading
import time

import numpy as np
import soundfile as sf
import hardware
from app_runtime import task_command
from styles import validate_settings

from timing import beat_words, lyric_lines

ROOT = Path(__file__).resolve().parent
DATA = Path(os.environ.get("OBS_DATA_DIR", ROOT / "data")).resolve()
CREATE_NO_WINDOW = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def binary(name):
    """Resolve packaged, system, or optional imageio FFmpeg binaries."""
    explicit = os.environ.get(f"OBS_{name.upper()}")
    candidates = [Path(explicit)] if explicit else []
    candidates += [ROOT / "bin" / f"{name}.exe", ROOT.parent / f"{name}.exe"]
    if name == "ffprobe":
        # A packaged ffprobe normally sits beside the selected ffmpeg binary.
        ffmpeg_override = os.environ.get("OBS_FFMPEG")
        if ffmpeg_override:
            candidates.append(Path(ffmpeg_override).with_name("ffprobe.exe"))
    for path in candidates:
        if path.is_file():
            return str(path)
    found = shutil.which(name)
    if found:
        return found
    if name == "ffmpeg":
        try:
            import imageio_ffmpeg
            path = Path(imageio_ffmpeg.get_ffmpeg_exe())
            if path.is_file():
                return str(path)
        except (ImportError, RuntimeError, OSError):
            pass
    raise RuntimeError(f"Chưa tìm thấy {name}. Đặt {name}.exe trong thư mục bin của app hoặc PATH.")


class Cancelled(Exception):
    pass


class Job:
    def __init__(self, jid, project_id, kind, on_change=None):
        self.id, self.project_id, self.kind = jid, project_id, kind
        self.state = "queued"
        self.message = "Đang đợi xử lý…"
        self.progress = 0
        self.result = None
        self.cancelled = threading.Event()
        self.process = None
        self._lock = threading.Lock()
        self.devices = {}
        self.fallbacks = []
        self.on_change = on_change

    def _changed(self):
        if self.on_change:
            self.on_change(self)

    def record_device(self, stage, device, reason=""):
        with self._lock:
            self.devices[stage] = dict(device)
            if reason:
                self.fallbacks.append({"stage": stage, "reason": reason[:1000]})
        self._changed()

    def update(self, progress, message):
        self.check()
        with self._lock:
            self.progress, self.message = progress, message
        self._changed()

    def check(self):
        if self.cancelled.is_set():
            raise Cancelled()

    def stop(self):
        self.cancelled.set()
        with self._lock:
            process = self.process
        if process and process.poll() is None:
            try:
                process.terminate()
            except OSError:
                pass
        self._changed()

    def public(self):
        with self._lock:
            return {"id": self.id, "project_id": self.project_id, "kind": self.kind,
                    "state": self.state, "message": self.message,
                    "progress": self.progress, "result": self.result,
                    "devices": dict(self.devices), "fallbacks": list(self.fallbacks),
                    "cancel_requested": self.cancelled.is_set(),
                    "updated_at": getattr(self, "updated_at", None)}


def command(args, job=None, cwd=None, progress=None, environment=None):
    if job:
        job.check()
    env = os.environ.copy()
    env.update(environment or {})
    env["PYTHONUTF8"] = "1"
    # Only media tasks need FFmpeg on PATH. AI-only workers must remain usable
    # in source installs that have not bundled the optional media binaries.
    needs_ffmpeg = Path(str(args[0])).stem.lower() in ("ffmpeg", "ffprobe")
    if needs_ffmpeg:
        ffmpeg_dir = str(Path(binary("ffmpeg")).parent)
        env["PATH"] = ffmpeg_dir + os.pathsep + env.get("PATH", "")
    process = subprocess.Popen([str(a) for a in args], cwd=cwd, env=env,
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, text=True,
                               encoding="utf-8", errors="replace",
                               creationflags=CREATE_NO_WINDOW)
    if job:
        with job._lock:
            job.process = process
        if job.cancelled.is_set():
            process.terminate()
    tail = []
    try:
        for line in process.stdout:
            tail.append(line.rstrip())
            tail = tail[-35:]
            if progress:
                progress(line)
            if job:
                job.check()
        code = process.wait()
        if job:
            job.check()
        if code:
            raise RuntimeError("\n".join(tail)[-4500:] or f"Tiến trình dừng với mã {code}")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        if job:
            with job._lock:
                job.process = None


def convert(source, destination, job=None):
    command([binary("ffmpeg"), "-y", "-v", "error", "-i", source,
             "-vn", "-ar", "44100", "-ac", "2", "-c:a", "pcm_s16le", destination], job)


def waveform(path, bins=1800):
    info = sf.info(str(path))
    step = max(1, int(np.ceil(info.frames / bins)))
    peaks = []
    with sf.SoundFile(str(path)) as file:
        while True:
            samples = file.read(step, dtype="float32", always_2d=True)
            if not len(samples):
                break
            peaks.append(round(float(np.max(np.abs(samples))), 4))
    scale = max(max(peaks, default=0), .001)
    return {"duration": round(info.duration, 4), "peaks": [p / scale for p in peaks]}


def model_progress(job, start=5, end=95):
    def update(line):
        if line.startswith('OBS_PROGRESS '):
            try:
                item = json.loads(line[len('OBS_PROGRESS '):])
                value = max(0, min(100, float(item['progress'])))
                job.update(start + (end - start) * value / 100, item['message'])
            except (ValueError, KeyError, TypeError):
                pass
    return update


def separate(reference, directory, job, device=None):
    cached = directory / "vocals.wav"
    if cached.is_file():
        job.record_device("demucs", {"device": "cache", "name": "Vocal đã có"})
        return cached
    device = device or hardware.select_ai()["demucs"]
    job.record_device("demucs", device)
    job.update(12, f"Đang tách vocal bằng Demucs · {device['name']}…")
    models = DATA / "models"
    models.mkdir(parents=True, exist_ok=True)
    bundled = ROOT.parent / "VCL_data" / "models" / "hub" / "checkpoints" / "955717e8-8726e21a.th"
    if (models / bundled.name).is_file():
        bundled = models / bundled.name
    elif not bundled.is_file():
        from model_manager import cached_demucs
        bundled=cached_demucs(DATA) or bundled
    args = [*task_command("demucs"), "--two-stems", "vocals", "--float32",
            "--shifts", "1", "--jobs", "1", "-o", str(directory / "separated")]
    if bundled.is_file():
        dest = models / bundled.name
        if not dest.is_file():
            try:
                os.link(bundled, dest)
            except OSError:
                shutil.copyfile(bundled, dest)
        args += ["--repo", str(models), "-n", "955717e8"]
    else:
        args += ["-n", "htdemucs"]
    def separation_progress(line):
        match = re.search(r"(\d{1,3})%\|", line)
        if match:
            percent = min(100, int(match.group(1)))
            job.update(12 + percent * .3, f"Demucs · {device['name']}: {percent}%…")
    try:
        command(args + ["-d", device["device"], str(reference)], job, progress=separation_progress,
                environment={'TORCH_HOME':str(DATA/'cache/torch')})
    except RuntimeError as exc:
        if not device["device"].startswith("cuda") or not hardware.gpu_error(exc):
            raise
        job.check()
        device = hardware.cpu_backend()
        job.record_device("demucs", device, str(exc))
        job.update(12, "GPU tách vocal gặp lỗi; đang tiếp tục bằng CPU…")
        command(args + ["-d", "cpu", str(reference)], job, progress=separation_progress,
                environment={'TORCH_HOME':str(DATA/'cache/torch')})
    job.check()
    found = list((directory / "separated").glob("*/*/vocals.wav"))
    if not found:
        raise RuntimeError("Demucs chưa tạo được track vocal.")
    shutil.copyfile(found[0], cached)
    inst = found[0].with_name("no_vocals.wav")
    if inst.exists():
        # This file is ONLY for comparing the two clocks. Export never reads it.
        shutil.copyfile(inst, directory / "reference_instrumental.wav")
    return cached


def recognize(directory, job, model="medium", language="vi", lyrics="", preference="auto", region=None):
    if model not in ("small", "medium", "large-v3-turbo"):
        raise ValueError("Model không hợp lệ.")
    if language not in ("vi", "en", "auto"):
        raise ValueError("Ngôn ngữ không hợp lệ.")
    job.update(3, "Đang kiểm tra thiết bị xử lý…")
    devices = hardware.select_ai(preference=preference)
    vocals = separate(directory / "reference.wav", directory, job, devices["demucs"])
    job.update(45, "Đang căn lời có sẵn theo vocal…" if lyrics.strip() else "Whisper đang nhận dạng lời và mốc từng từ…")
    request_path = directory / "asr_request.json"
    result_path = directory / "asr_result.json"
    params = {"vocals": str(vocals), "model": model, "models_root": str(DATA), "language": language, "lyrics": lyrics,
              "output": str(result_path), **devices["whisper"]}
    if region is not None:
        params["region"] = {"start": float(region["start"]), "end": float(region["end"])}
    def write_request():
        request_path.write_text(json.dumps(params, ensure_ascii=False), encoding="utf-8")
    write_request()
    job.record_device("whisper", devices["whisper"])
    def progress(line):
        if line.startswith("OBS_PROGRESS "):
            try:
                item = json.loads(line[len("OBS_PROGRESS "):])
                job.update(item["progress"], item["message"])
            except (ValueError, KeyError):
                pass
    try:
        command(task_command("speech", request_path), job, progress=progress)
    except RuntimeError as exc:
        if params["device"] != "cuda" or not hardware.gpu_error(exc):
            raise
        job.check()
        # Retry in a NEW process so an OOM or CUDA context failure releases VRAM first.
        cpu = hardware.cpu_backend()
        params.update(cpu)
        job.record_device("whisper", cpu, str(exc))
        params["fallback_reason"] = str(exc)[-1000:]
        job.update(46, "GPU nhận lời gặp lỗi; đang tiếp tục bằng CPU…")
        write_request()
        command(task_command("speech", request_path), job, progress=progress)
    job.check()
    return json.loads(result_path.read_text(encoding="utf-8"))


def suggest_duet(project,directory,job,preference='auto'):
    if not project.get('reference') or not (directory/'reference.wav').is_file():
        raise ValueError('Cần bản có giọng hát để phân vai. Beat không chứa giọng hát.')
    if (directory/'vocals.wav').is_file():
        vocal=directory/'vocals.wav'
    else:
        device=hardware.select_ai(preference=preference)['demucs']
        vocal=separate(directory/'reference.wav',directory,job,device)
    job.record_device('roles',{'device':'cpu','name':'Phân tích cao độ vocal · CPU'})
    groups={}
    for word in project['words']:
        line=word['line']
        # Words are stored in reference time; anchors must NOT be applied to vocal audio.
        item=groups.setdefault(line,{'line':line,'start':word['start'],'end':word['end']})
        item['start']=min(item['start'],word['start']);item['end']=max(item['end'],word['end'])
    segments=list(groups.values())
    output=directory/f'roles_{job.id}.json'
    request=directory/f'roles_request_{job.id}.json'
    request.write_text(json.dumps({'vocal':str(vocal),'segments':segments,'output':str(output)}),encoding='utf-8')
    def progress(line):
        if line.startswith('OBS_PROGRESS '):
            item=json.loads(line[len('OBS_PROGRESS '):]);job.update(item['progress'],item['message'])
    command(task_command('roles', request), job, progress=progress)
    job.check()
    result=json.loads(output.read_text(encoding='utf-8'))
    result['lines']=[item['line'] for item in segments]
    return result


def ass_time(seconds):
    cs = max(0, round(seconds * 100))
    return f"{cs // 360000}:{cs // 6000 % 60:02d}:{cs // 100 % 60:02d}.{cs % 100:02d}"


def srt_time(seconds):
    ms = max(0, round(seconds * 1000))
    return f"{ms // 3600000:02d}:{ms // 60000 % 60:02d}:{ms // 1000 % 60:02d},{ms % 1000:03d}"


def ass_color(color):
    if not re.fullmatch(r"#[0-9a-fA-F]{6}", str(color)):
        raise ValueError("Màu không hợp lệ.")
    return "&H00" + color[5:7] + color[3:5] + color[1:3]


def ass_text(text):
    # Escaping literal user text prevents it from creating ASS override tags.
    return str(text).replace("\\", "＼").replace("{", "｛").replace("}", "｝").replace("\n", " ")


def build_ass(project, directory=None):
    from subtitles import build_ass as compose
    return compose(project, directory)


def export_text(project, kind, directory=None):
    lines = lyric_lines(beat_words(project))
    if kind == "ass":
        return build_ass(project, directory)
    if kind == "srt":
        return "\n\n".join(f"{i + 1}\n{srt_time(line[0]['start'])} --> {srt_time(line[-1]['end'])}\n" +
                              " ".join(w["text"] for w in line) for i, line in enumerate(lines)) + "\n"
    if kind == "lrc":
        def stamp(t):
            cs = round(t * 100)
            return f"[{cs // 6000:02d}:{cs // 100 % 60:02d}.{cs % 100:02d}]"
        return "\n".join(stamp(line[0]["start"]) + " ".join(w["text"] for w in line) for line in lines) + "\n"
    raise ValueError("Định dạng không hợp lệ.")


def render(project, directory, job, render_id=None):
    """Render from the original beat, optionally mixed with separated guide vocals."""
    if not project.get("beat") or not (directory / "beat.wav").exists():
        raise ValueError("Hãy nạp beat gốc trước khi xuất.")
    opts = validate_settings(project.get("settings", {}))
    project = {**project, 'settings':opts}
    import media_tools
    from layout import display_plan
    render_id = render_id if isinstance(render_id, str) and re.fullmatch(r"[0-9a-f]{32}", render_id) else job.id
    export = directory / "exports" / render_id
    export.mkdir(parents=True, exist_ok=True)
    (export / "lyrics.ass").write_text(build_ass(project, directory), encoding="utf-8-sig")
    width, height = (1080, 1920) if opts.get("aspect") == "portrait" else (1920, 1080)
    if opts['resolution']=='4k':width,height=width*2,height*2
    bg = opts.get("background", "#101d2b")
    ass_color(bg)
    duration = project["beat"]["duration"]
    preview = project.get("render_preview") if isinstance(project.get("render_preview"), dict) else None
    render_start = 0.
    render_duration = duration
    if preview is not None:
        render_start=max(0.,min(max(0.,duration-.5),float(preview.get("start",0))))
        render_duration=max(.5,min(12.,float(preview.get("duration",8)),duration-render_start))
    import render_info
    manifest=render_info.describe(project,directory)
    if manifest['errors']:raise ValueError(' '.join(manifest['errors']))
    if preview is not None:
        manifest['render_preview']={'start':round(render_start,3),'duration':round(render_duration,3)}
    if project.get('export_directory') and preview is None:
        import tempfile
        try:
            with tempfile.TemporaryFile(dir=project['export_directory']):pass
        except OSError as exc:
            raise ValueError('Không ghi được vào thư mục xuất. Hãy chọn thư mục khác.') from exc
    fps = manifest['output']['fps']
    video_device = hardware.detect().get("video", {"encoder": "libx264", "name": "CPU"})
    job.record_device("video", video_device)
    executable = video_device.get("ffmpeg") or binary("ffmpeg")
    args = [executable, "-y", "-v", "error", "-progress", "pipe:1", "-nostats"]
    background_kind,background=media_tools.background_source(project,directory)
    if background_kind == 'video':
        args += ['-stream_loop','-1','-i',str(background)]
    elif background_kind == 'image':
        args += ["-loop", "1", "-framerate", str(fps), "-i", str(background)]
    else:
        args += ["-f", "lavfi", "-i", f"color=c=0x{bg[1:]}:s={width}x{height}:r={fps}"]
    # The default soundtrack is always beat.wav; guide variants optionally add separated vocals.
    args += ['-i',str(directory/'beat.wav')]
    guide_audio = opts['audio_output'] == 'guide'
    if guide_audio:
        vocal_path=directory/'vocals.wav'
        if not vocal_path.is_file():
            reference_path=directory/'reference.wav'
            if not reference_path.is_file():raise ValueError('Cần bản hát tham chiếu hoặc vocal đã tách để tạo bản hướng dẫn.')
            vocal_path=separate(reference_path,directory,job,hardware.select_ai()['demucs'])
        args += ['-i',str(vocal_path)]
    import scene_tools
    scene_items = scene_tools.timeline(project, duration, directory)
    next_input = 3 if guide_audio else 2
    scene_inputs = []
    for scene in scene_items:
        if not scene.get('image_path'):
            continue
        args += ['-loop', '1', '-framerate', str(fps), '-i', str(scene['image_path'])]
        scene_inputs.append({**scene, 'input_index': next_input})
        next_input += 1
    decoration_inputs = []
    for scene in scene_items:
        if not scene.get('decoration_path'):
            continue
        args += ['-loop', '1', '-framerate', str(fps), '-i', str(scene['decoration_path'])]
        decoration_inputs.append({**scene, 'input_index': next_input})
        next_input += 1
    from bundled_assets import FONT_ROOT
    # A relative fontsdir avoids Windows drive-colon escaping in FFmpeg filters.
    fonts_dir = export / 'fonts'
    fonts_dir.mkdir(exist_ok=True)
    from layout import resolve_font
    for family, italic in ((opts['font'], opts['italic']), (opts['title_font'], opts['title_italic'])):
        chosen, _ = resolve_font(family, italic)
        font_path = Path(chosen[2])
        if font_path.parent == FONT_ROOT:
            # libass selects weight/style by metadata; include this family's faces.
            from bundled_assets import ASSET_ROOT, bundled_fonts
            for face in bundled_fonts():
                if face['family'] == chosen[0]:
                    shutil.copy2(ASSET_ROOT / face['file'], fonts_dir / Path(face['file']).name)
    background_filter=f'setpts=PTS-STARTPTS,fps={fps},scale={width}:{height}:force_original_aspect_ratio=increase:flags=lanczos,crop={width}:{height},setsar=1'
    visual_filter=background_filter+',ass=lyrics.ass:fontsdir=fonts'
    graph=f'[0:v]{background_filter}[scene_base]'
    last_background='scene_base'
    for index, scene in enumerate(scene_inputs):
        start, end = scene['start'], scene['end']
        clip_duration = max(.05, end - start)
        fade = min(scene['fade_seconds'], clip_duration / 2)
        frames=scene.get('keyframes') or [{'x':0,'y':0,'scale':100,'opacity':100}]*2
        first,last=frames[0],frames[1]
        local_progress=f"min(max(on/{clip_duration*fps:.6f},0),1)"
        alpha_progress=f"min(max(T/{clip_duration:.6f},0),1)"
        def lerp(key, progress):
            a=float(first.get(key,100 if key in ('scale','opacity') else 0))
            b=float(last.get(key,a))
            return f"({a:.4f}+({b-a:.4f})*{progress})"
        scale_expr=f"max(1,{lerp('scale',local_progress)}/100)"
        opacity_expr=f"({scene['opacity']:.4f}*{lerp('opacity',alpha_progress)}/100)"
        opacity_start=float(first.get('opacity',100))
        opacity_end=float(last.get('opacity',opacity_start))
        x_expr=lerp('x',local_progress)
        y_expr=lerp('y',local_progress)
        scene_filter=(f"[{scene['input_index']}:v]setpts=PTS-STARTPTS,"
                      f"scale={width}:{height}:force_original_aspect_ratio=increase:flags=lanczos,"
                      f"crop={width}:{height},setsar=1,"
                      f"zoompan=z='{scale_expr}':"
                      f"x='iw/2-iw/zoom/2-({x_expr})*(iw-iw/zoom)/100':"
                      f"y='ih/2-ih/zoom/2-({y_expr})*(ih-ih/zoom)/100':"
                      f"d=1:s={width}x{height}:fps={fps},format=rgba")
        if abs(opacity_end-opacity_start)<.001:
            scene_filter+=f",colorchannelmixer=aa={scene['opacity']*opacity_start/10000:.4f}"
        else:
            scene_filter+=f",geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='alpha(X,Y)*({opacity_expr})/100'"
        if fade >= .05:
            scene_filter += f',fade=t=in:st=0:d={fade:.3f}:alpha=1'
            scene_filter += f',fade=t=out:st={max(0, clip_duration-fade):.3f}:d={fade:.3f}:alpha=1'
        scene_filter += f',trim=duration={clip_duration:.3f},setpts=PTS-STARTPTS+{start:.3f}/TB[scene_image{index}]'
        graph += ';' + scene_filter
        graph += (f";[{last_background}][scene_image{index}]overlay=x=0:y=0:eof_action=pass:shortest=0:"
                  f"enable='between(t,{start:.3f},{end:.3f})'[scene_bg{index}]")
        last_background = f'scene_bg{index}'
    for index, scene in enumerate(decoration_inputs):
        start, end = scene['start'], scene['end']
        clip_duration = max(.05, end - start)
        fade = min(scene['fade_seconds'], clip_duration / 2)
        frames=scene.get('decoration_keyframes') or [{'x':50,'y':50,'scale':100,'opacity':100}]*2
        first,last=frames[0],frames[1]
        local_progress=f"min(max(t/{clip_duration:.6f},0),1)"
        alpha_progress=f"min(max(T/{clip_duration:.6f},0),1)"
        def decoration_lerp(key, progress):
            a=float(first.get(key,50 if key in ('x','y') else 100))
            b=float(last.get(key,a))
            return f"({a:.4f}+({b-a:.4f})*{progress})"
        width_expr=f"max(1,{width}*{scene['decoration_size']:.4f}/100*{decoration_lerp('scale',local_progress)}/100)"
        try:
            from PIL import Image
            with Image.open(scene['decoration_path']) as artwork:
                aspect=artwork.height/max(1,artwork.width)
        except (OSError, ValueError):
            aspect=1.
        canvas_width=math.ceil(width*scene['decoration_size']*.02)+2
        canvas_height=math.ceil(canvas_width*aspect)+2
        opacity_expr=decoration_lerp('opacity',alpha_progress)
        position_progress=f"min(max((t-{start:.6f})/{clip_duration:.6f},0),1)"
        x_expr=decoration_lerp('x',position_progress)
        y_expr=decoration_lerp('y',position_progress)
        decoration_filter=(f"[{scene['input_index']}:v]setpts=PTS-STARTPTS,format=rgba,"
                           f"scale=w='{width_expr}':h=-1:eval=frame,"
                           f"pad=w={canvas_width}:h={canvas_height}:x='(ow-iw)/2':y='(oh-ih)/2':color=black@0,"
                           f"geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='alpha(X,Y)*({opacity_expr})/100'")
        if fade >= .05:
            decoration_filter += f',fade=t=in:st=0:d={fade:.3f}:alpha=1'
            decoration_filter += f',fade=t=out:st={max(0, clip_duration-fade):.3f}:d={fade:.3f}:alpha=1'
        decoration_filter += f',trim=duration={clip_duration:.3f},setpts=PTS-STARTPTS+{start:.3f}/TB[scene_decoration{index}]'
        graph += ';' + decoration_filter
        graph += (f";[{last_background}][scene_decoration{index}]overlay="
                  f"x='W*({x_expr})/100-w/2':y='H*({y_expr})/100-h/2':"
                  f"eof_action=pass:shortest=0:enable='between(t,{start:.3f},{end:.3f})'[scene_decor{index}]")
        last_background=f'scene_decor{index}'
    if scene_inputs or decoration_inputs:
        graph += f';[{last_background}]ass=lyrics.ass:fontsdir=fonts[base]'
    else:
        graph = f'[0:v]{visual_filter}[base]'
    last='base'
    overlay=media_tools.custom_countdown_filter(project,directory,display_plan(project),width,height,next_input)
    if overlay:
        countdown_graph,last=overlay
        args+=['-loop','1','-framerate',str(fps),'-i',str(directory/'countdown.png')]
        graph+=';'+countdown_graph.rstrip(';')
        next_input+=1
    logo=media_tools.logo_filter(project,directory,width,height,next_input,last)
    if logo:
        logo_graph,last=logo
        args+=['-loop','1','-framerate',str(fps),'-i',str(directory/'logo.png')]
        graph+=';'+logo_graph
    if guide_audio:
        graph+=';[1:a:0][2:a:0]amix=inputs=2:duration=first:dropout_transition=0:normalize=1[audio]'
    if scene_inputs or decoration_inputs or overlay or logo or guide_audio:
        args+=['-filter_complex',graph,'-map',f'[{last}]','-map','[audio]' if guide_audio else '1:a:0']
    else:
        args+=['-vf',visual_filter,'-map','0:v:0','-map','1:a:0']
    video_options = render_info.encoding_options(opts['render_quality'],video_device['encoder'])
    video_name='karaoke.'+opts['render_format']
    trailer = ["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "320k"]
    if render_start>0:trailer += ["-ss",str(render_start)]
    trailer += ["-t",str(render_duration)]
    if opts.get('audio_normalize'):
        trailer += ["-af", "loudnorm=I=-16:TP=-1.5:LRA=11"]
    if opts['render_format'] in ('mp4','mov'):trailer += ['-movflags','+faststart']
    trailer += [video_name]
    job.update(5, f"Đang dựng video từ beat gốc · {video_device['name']}…")
    def progress(line):
        if line.startswith("out_time_us="):
            try:
                seconds = int(line.split("=", 1)[1]) / 1e6
                soundtrack='beat gốc + vocal hướng dẫn' if guide_audio else 'beat gốc'
                job.update(min(98, 5 + seconds / render_duration * 93), f"Đang ghép chữ và {soundtrack} · {video_device['name']}…")
            except ValueError:
                pass
    try:
        command(args + video_options + trailer, job, cwd=export, progress=progress)
    except RuntimeError as exc:
        if video_device["encoder"] != "h264_nvenc" or not hardware.gpu_error(exc):
            raise
        job.check()
        video_device = {"encoder": "libx264", "name": "CPU"}
        job.record_device("video", video_device, str(exc))
        job.update(5, "NVENC gặp lỗi; đang dựng lại bằng CPU…")
        command([binary("ffmpeg")] + args[1:] +
                render_info.encoding_options(opts['render_quality'],'libx264') + trailer,
                job, cwd=export, progress=progress)
    job.check()
    manifest['postflight'] = render_info.verify_output(export / video_name, render_duration, width, height)
    try:
        manifest['postflight']['sample_sheet'] = render_info.sample_frames(
            export / video_name, export / 'qa-contact.jpg', render_duration)
    except Exception as exc:
        manifest['postflight']['sample_sheet'] = {'ok': False, 'error': str(exc)[-500:]}
    manifest['encoder']=video_device['encoder']
    (export/'render-info.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2),encoding='utf-8')
    output_path=str(export/video_name)
    if project.get('export_directory') and preview is None:
        target=Path(project['export_directory'])
        stem=re.sub(r'[<>:"/\\|?*\x00-\x1f]','_',project.get('name','karaoke')).strip(' .')[:65] or 'karaoke'
        stem=stem+'_'+render_id
        # The selected export folder receives the rendered video only.
        name=video_name
        job.check()
        suffix=Path(name).suffix
        destination=target/(stem+suffix)
        partial=target/(stem+suffix+'.partial')
        try:
            with (export/name).open('rb') as source,partial.open('xb') as output:
                while chunk:=source.read(8*1024*1024):
                    job.check();output.write(chunk)
            partial.replace(destination)
        finally:partial.unlink(missing_ok=True)
        output_path=str(destination)
    return {"video": f"/api/projects/{project['id']}/exports/{render_id}/{video_name}",
            "qa_contact": f"/api/projects/{project['id']}/exports/{render_id}/qa-contact.jpg" if (export/'qa-contact.jpg').is_file() else None,
            "format":opts['render_format'],
            "encoder": video_device["encoder"], "output_path": output_path,
            **({"preview":True,"preview_start":round(render_start,3),
                "preview_duration":round(render_duration,3)} if preview is not None else {})}
