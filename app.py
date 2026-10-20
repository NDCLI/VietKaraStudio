from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from copy import deepcopy
import io
import json
import os
from pathlib import Path
import re
import shutil
import threading
import time
import uuid
from urllib.parse import urlparse

from flask import Flask, jsonify, request, send_file, send_from_directory
from werkzeug.exceptions import HTTPException

import engine
import hardware
import presets
import layout
import media_tools
from styles import DEFAULT_SETTINGS, validate_settings
from timing import (beat_words, estimate_words, finite, validate_anchors,
                    validate_words)

app = Flask(__name__, static_folder="static")
app.config["MAX_CONTENT_LENGTH"] = 3 * 1024 ** 3
LOCK = threading.RLock()
POOL = ThreadPoolExecutor(max_workers=1, thread_name_prefix="original-beat")
JOBS = {}


@app.get('/api/models')
def model_status():
    import model_manager
    return jsonify(model_manager.status(engine.DATA))


@app.post('/api/projects/<pid>/models/<name>')
def download_model(pid, name):
    import sys
    import model_manager
    directory(pid)
    if name not in model_manager.MODELS:
        raise ValueError('Model không hợp lệ.')
    def work(job):
        job.update(5, 'Đang tải model ' + name + '… lần đầu có thể cần vài phút.')
        engine.command([sys.executable, '-u', engine.ROOT / 'model_manager.py', engine.DATA, name], job)
        return {'model': name}
    return task(pid, 'model', work)


@app.post('/api/projects/<pid>/separate')
def separate_tool(pid):
    idle(pid)
    project_path=directory(pid)
    file = request.files.get('file')
    from_video=(request.get_json(silent=True) or {}).get('source')=='background'
    source=media_tools.local_asset(project_path,load(pid).get('background_video',{}).get('source_file')) if from_video else None
    if from_video and not source:
        raise ValueError('Hãy nạp video trong File nguồn trước.')
    if not from_video and (not file or not file.filename):
        raise ValueError('Chọn bài nhạc để tách.')
    ext = source.suffix.lower() if from_video else Path(file.filename).suffix.lower()
    if ext not in ('.wav', '.mp3', '.flac', '.m4a', '.aac', '.ogg', '.mp4', '.mov', '.m4v', '.avi', '.mkv', '.webm', '.wma'):
        raise ValueError('Định dạng âm thanh/video chưa hỗ trợ.')
    identifier = uuid.uuid4().hex
    folder = engine.DATA / 'tools' / identifier
    folder.mkdir(parents=True)
    incoming = folder / ('source' + ext)
    if from_video:
        shutil.copy2(source,incoming)
    else:
        file.save(incoming)
    preference = (request.get_json(silent=True) or {}).get('device','auto') if from_video else request.form.get('device', 'auto')
    def work(job):
        engine.convert(incoming, folder / 'reference.wav', job)
        device = hardware.select_ai(hardware.detect(), preference)['demucs']
        engine.separate(folder / 'reference.wav', folder, job, device)
        return {'beat': f'/api/tools/{identifier}/beat', 'vocals': f'/api/tools/{identifier}/vocals'}
    return task(pid, 'separate', work)


@app.get('/api/tools/<identifier>/<stem>')
def separated_audio(identifier, stem):
    if not re.fullmatch(r'[0-9a-f]{32}', identifier) or stem not in ('beat', 'vocals'):
        raise ValueError('File không hợp lệ.')
    name = 'reference_instrumental.wav' if stem == 'beat' else 'vocals.wav'
    return send_file(engine.DATA / 'tools' / identifier / name, as_attachment=True, download_name=stem + '.wav')


def directory(pid):
    if not re.fullmatch(r"[0-9a-f]{32}", pid):
        raise ValueError("ID project không hợp lệ.")
    path = engine.DATA / "projects" / pid
    if not (path / "project.json").is_file():
        raise FileNotFoundError("Không tìm thấy project.")
    return path


def load(pid):
    with LOCK:
        return json.loads((directory(pid) / "project.json").read_text(encoding="utf-8"))


def save(project):
    with LOCK:
        project["updated_at"] = time.time()
        dest = engine.DATA / "projects" / project["id"] / "project.json"
        temp = dest.with_suffix(".tmp")
        temp.write_text(json.dumps(project, ensure_ascii=False, indent=2), encoding="utf-8")
        temp.replace(dest)


def public(project):
    result = deepcopy(project)
    result["settings"] = validate_settings(project.get("settings", {}))
    result["beat_words"] = beat_words(project)
    result["vocals_ready"] = (engine.DATA / "projects" / project["id"] / "vocals.wav").is_file()
    return result


def idle(pid):
    with LOCK:
        if any(j.project_id == pid and j.state in ("queued", "running") for j in JOBS.values()):
            raise ValueError("Project đang xử lý. Hãy chờ xong hoặc hủy tác vụ trước.")


def task(pid, kind, fn):
    with LOCK:
        idle(pid)
        job = engine.Job(uuid.uuid4().hex, pid, kind)
        JOBS[job.id] = job
    def work():
        try:
            job.check()
            job.state = "running"
            result = fn(job)
            job.check()
            with job._lock:
                job.result = result
                job.state, job.progress, job.message = "done", 100, "Hoàn tất"
        except engine.Cancelled:
            job.state, job.message = "cancelled", "Đã hủy tác vụ."
        except Exception as exc:
            import traceback
            traceback.print_exc()
            if job.cancelled.is_set():
                job.state, job.message = "cancelled", "Đã hủy tác vụ."
            else:
                job.state, job.message = "error", str(exc)[-4500:]
    POOL.submit(work)
    return jsonify(job.public()), 202


@app.before_request
def local_requests():
    if request.method not in ("GET", "HEAD", "OPTIONS"):
        origin = request.headers.get("Origin")
        if origin and urlparse(origin).netloc != request.host:
            return jsonify(error="Yêu cầu phải đến từ cửa sổ app."), 403


@app.errorhandler(Exception)
def handle_error(error):
    if isinstance(error, HTTPException):
        return jsonify(error=error.description), error.code
    if isinstance(error, FileNotFoundError):
        return jsonify(error=str(error)), 404
    if isinstance(error, (ValueError, KeyError, TypeError)):
        return jsonify(error=str(error)), 400
    app.logger.exception("Request failed")
    return jsonify(error=str(error)), 500


@app.get("/")
def index():
    return send_from_directory(app.static_folder, "index.html")


@app.get("/api/health")
def health():
    deps = {}
    import importlib.util
    for name in ("demucs", "faster_whisper", "stable_whisper", "torch", "librosa"):
        deps[name] = importlib.util.find_spec(name) is not None
    for name in ("ffmpeg", "ffprobe"):
        try:
            deps[name] = bool(engine.binary(name))
        except RuntimeError:
            deps[name] = False
    return jsonify(ok=all(deps.values()), dependencies=deps, version="0.3.0")


@app.get("/api/hardware")
def get_hardware():
    return jsonify(hardware.detect(refresh=request.args.get("refresh") == "1"))


@app.get('/api/fonts')
def fonts():
    if request.args.get('vietnamese') == '1':
        return jsonify(layout.vietnamese_fonts())
    return jsonify(sorted(set(item[0] for item in layout.font_catalog()), key=str.casefold))


@app.post('/api/layout')
def get_layout():
    data=request.get_json()
    project={'settings':validate_settings(data.get('settings',{})),
             'words':validate_words(data.get('words',[])), 'anchors':validate_anchors(data.get('anchors',[])),
             'beat':data.get('beat')}
    if project['beat']:
        project['beat']={'duration':max(0,finite(project['beat']['duration']))}
    return jsonify(layout.display_plan(project))


@app.post('/api/projects/<pid>/autosplit')
def autosplit(pid):
    idle(pid)
    with LOCK:
        project=load(pid)
        data=request.get_json(silent=True) or {}
        text=str(data.get('text',project.get('text','')))[:300000]
        result=layout.split_text(text,project['settings'])
        if project['words']:
            project['words']=layout.regroup_words(project['words'],result['text'])
        project['text']=result['text']
        save(project)
    return jsonify(project=public(project),changed=result['changed'],font=result['font'],font_fallback=result['font_fallback'])


@app.get("/api/presets")
def list_presets():
    return jsonify(presets.catalog(engine.DATA))


@app.post("/api/presets")
def add_preset():
    return jsonify(presets.create(engine.DATA, request.get_json())), 201


@app.delete("/api/presets/<identifier>")
def delete_preset(identifier):
    presets.remove(engine.DATA, identifier)
    return jsonify(ok=True)


@app.get("/api/presets/<identifier>/download")
def download_preset(identifier):
    item = next((p for p in presets.catalog(engine.DATA) if p["id"] == identifier), None)
    if item is None:
        raise FileNotFoundError("Không tìm thấy preset.")
    content = json.dumps(presets.export(item), ensure_ascii=False, indent=2).encode("utf-8")
    return send_file(io.BytesIO(content), as_attachment=True, download_name=identifier + ".preset.json",
                     mimetype="application/json")


@app.get("/api/projects")
def projects():
    results = []
    root = engine.DATA / "projects"
    for file in root.glob("*/project.json"):
        try:
            item = json.loads(file.read_text(encoding="utf-8"))
            results.append({"id": item["id"], "name": item["name"], "updated_at": item["updated_at"]})
        except (OSError, ValueError, KeyError):
            continue
    return jsonify(sorted(results, key=lambda p: p["updated_at"], reverse=True))


@app.post("/api/projects")
def new_project():
    data = request.get_json(silent=True) or {}
    pid = uuid.uuid4().hex
    path = engine.DATA / "projects" / pid
    path.mkdir(parents=True, exist_ok=True)
    project = {"id": pid, "schema": 1, "name": str(data.get("name", "Bài karaoke mới"))[:150],
               "reference": None, "beat": None, "words": [], "anchors": [],
               "settings": deepcopy(DEFAULT_SETTINGS), "text": "", "alignment": None}
    save(project)
    return jsonify(public(project)), 201


@app.get("/api/projects/<pid>")
def get_project(pid):
    project = public(load(pid))
    project["job"] = next((j.public() for j in JOBS.values() if j.project_id == pid and
                           j.state in ("queued", "running")), None)
    return jsonify(project)


@app.patch("/api/projects/<pid>")
def update_project(pid):
    idle(pid)
    data = request.get_json()
    with LOCK:
        project = load(pid)
        if "name" in data:
            project["name"] = str(data["name"]).strip()[:150] or "Bài karaoke"
        if "text" in data:
            project["text"] = str(data["text"])[:300000]
        if "words" in data:
            project["words"] = validate_words(data["words"])
        if "anchors" in data:
            project["anchors"] = validate_anchors(data["anchors"])
        if "settings" in data:
            project["settings"] = validate_settings(data["settings"], project["settings"])
        save(project)
    return jsonify(public(project))


@app.post("/api/projects/<pid>/media/<slot>")
def media_upload(pid, slot):
    idle(pid)
    if slot not in ("reference", "beat", "background", "countdown", "logo"):
        raise ValueError("Loại media không hợp lệ.")
    path = directory(pid)
    file = request.files.get("file")
    if not file or not file.filename:
        raise ValueError("Hãy chọn file.")
    filename = Path(file.filename).name
    ext = Path(filename).suffix.lower()
    allowed = media_tools.IMAGE_EXTENSIONS + media_tools.VIDEO_EXTENSIONS if slot == "background" else media_tools.IMAGE_EXTENSIONS if slot in ('countdown', 'logo') else (
        ".mp3", ".wav", ".flac", ".m4a", ".aac", ".ogg", ".opus", ".mp4", ".mov", ".mkv", ".webm")
    if ext not in allowed:
        raise ValueError("Định dạng file chưa được hỗ trợ.")
    incoming = path / ("incoming_" + uuid.uuid4().hex + ext)
    file.save(incoming)
    def work(job):
        try:
            job.update(5, "Đang chuẩn bị file âm thanh…" if slot != "background" else "Đang chuẩn bị ảnh nền…")
            if slot in ('countdown', 'logo'):
                from PIL import Image
                destination=path/f'{slot}_next.png'
                with Image.open(incoming) as image:
                    image=image.convert('RGBA')
                    image.thumbnail((2048,2048) if slot == 'logo' else (512,512))
                    image.save(destination,'PNG')
                job.check()
                destination.replace(path/f'{slot}.png')
                project=load(pid)
                project[f'{slot}_name']=filename
                if slot == 'logo':
                    project['logo_version']=uuid.uuid4().hex
                    project['logo_image']={'width':image.width,'height':image.height}
                project['settings']=validate_settings({'logo_enabled':True} if slot == 'logo' else {'countdown':'image'},project['settings'])
            elif slot == 'background' and ext in media_tools.VIDEO_EXTENSIONS:
                metadata=media_tools.probe_video(incoming)
                proxy=path/'background_preview_next.mp4'
                media_tools.make_proxy(incoming,proxy,job)
                job.check()
                proxy.replace(path/'background_preview.mp4')
                source_file='background_source'+ext
                incoming.replace(path/source_file)
                project=load(pid)
                project['background_video']={'name':filename,'source_file':source_file,'preview_file':'background_preview.mp4',**metadata}
                project['settings']=validate_settings({'background_type':'video'},project['settings'])
            elif slot == "background":
                destination = path / "background_next.jpg"
                engine.command([engine.binary("ffmpeg"), "-y", "-v", "error", "-i", incoming,
                                "-frames:v", "1", "-update", "1", destination], job)
                job.check()
                destination.replace(path / "background.jpg")
                project = load(pid)
                project["background_name"] = filename
                project['settings']=validate_settings({'background_type':'image','use_background':True},project['settings'])
            else:
                video=None
                if ext in media_tools.VIDEO_EXTENSIONS:
                    info=media_tools.probe_video(incoming)
                    proxy=path/f'{slot}_preview_next.mp4'
                    media_tools.make_proxy(incoming,proxy,job)
                    video={'source_file':slot+'_source'+ext,'preview_file':slot+'_preview.mp4',**info}
                temporary = path / f"{slot}_next.wav"
                engine.convert(incoming, temporary, job)
                metadata = {"name": filename, **engine.waveform(temporary)}
                job.check()
                temporary.replace(path / f"{slot}.wav")
                if video:
                    proxy.replace(path/video['preview_file'])
                    incoming.replace(path/video['source_file'])
                    metadata['video']=video
                project = load(pid)
                had_reference = bool(project.get("reference"))
                replaced_existing = bool(project.get(slot))
                project[slot] = metadata
                if replaced_existing:
                    project["anchors"] = []
                project["alignment"] = None
                if slot == "reference":
                    if had_reference:
                        project["words"] = []
                    for old in ("vocals.wav", "reference_instrumental.wav"):
                        (path / old).unlink(missing_ok=True)
                if project["name"] == "Bài karaoke mới":
                    project["name"] = Path(filename).stem[:150]
            save(project)
            return {"project_id": pid}
        finally:
            incoming.unlink(missing_ok=True)
    try:
        return task(pid, "upload", work)
    except Exception:
        incoming.unlink(missing_ok=True)
        raise


@app.get("/api/projects/<pid>/audio/<slot>")
def audio(pid, slot):
    if slot not in ("reference", "beat", "vocals"):
        raise ValueError("Track không hợp lệ.")
    return send_file(directory(pid) / f"{slot}.wav", conditional=True)


@app.get("/api/projects/<pid>/background")
def background(pid):
    return send_file(directory(pid) / "background.jpg", conditional=True)


@app.get('/api/projects/<pid>/video/<slot>')
def video_preview(pid,slot):
    if slot not in ('reference','beat','background'):
        raise ValueError('Loại video không hợp lệ.')
    return send_file(directory(pid)/f'{slot}_preview.mp4',conditional=True)


@app.get('/api/projects/<pid>/countdown')
def countdown_image(pid):
    return send_file(directory(pid)/'countdown.png',conditional=True)


@app.get('/api/projects/<pid>/logo')
def logo_image(pid):
    return send_file(directory(pid)/'logo.png',conditional=True)


@app.post('/api/projects/<pid>/video-reference')
def video_reference(pid):
    idle(pid)
    path=directory(pid)
    metadata=load(pid).get('background_video',{})
    source=media_tools.local_asset(path,metadata.get('source_file'))
    if not source:
        raise ValueError('Hãy nạp video trong File nguồn trước.')
    def work(job):
        temporary=path/'reference_next.wav'
        engine.convert(source,temporary,job)
        audio_metadata={'name':metadata['name'],**engine.waveform(temporary)}
        job.check()
        # A separate copy keeps later background changes from replacing the reference.
        video_name='reference_source'+source.suffix
        shutil.copy2(source,path/video_name)
        shutil.copy2(path/metadata['preview_file'],path/'reference_preview.mp4')
        audio_metadata['video']={**metadata,'source_file':video_name,'preview_file':'reference_preview.mp4'}
        temporary.replace(path/'reference.wav')
        project=load(pid)
        if project.get('reference'):
            project['words']=[]
        project['reference']=audio_metadata
        project['anchors']=[]
        project['alignment']=None
        for old in ('vocals.wav','reference_instrumental.wav'):
            (path/old).unlink(missing_ok=True)
        save(project)
        return {'project_id':pid}
    return task(pid,'upload',work)


@app.post("/api/projects/<pid>/analyze")
def analyze(pid):
    project = load(pid)
    if not project.get("reference"):
        raise ValueError("Hãy nạp bản có giọng hát tham chiếu.")
    options = request.get_json(silent=True) or {}
    lyrics = str(options.get("lyrics", ""))[:300000]
    def work(job):
        result = engine.recognize(directory(pid), job, options.get("model", "medium"),
                                  options.get("language", "vi"), lyrics, options.get("device", "auto"))
        job.check()
        with LOCK:
            current = load(pid)
            current["words"] = validate_words(result["words"])
            current["text"] = result["text"]
            current["processing"] = {"devices": job.public()["devices"], "fallbacks": job.public()["fallbacks"],
                                      "device": result["device"], "compute_type": result["compute_type"],
                                      'alignment_diagnostics':result.get('alignment_diagnostics',{})}
            save(current)
        return {"project_id": pid, "words": len(current["words"]), "device": result["device"],
                "device_name": result["device_name"], "compute_type": result["compute_type"]}
    return task(pid, "analyze", work)


@app.post('/api/projects/<pid>/suggest-roles')
def suggest_roles(pid):
    idle(pid)
    project=load(pid)
    if not project.get('reference'):
        raise ValueError('Nạp bản hát có giọng trước khi tự phân vai song ca.')
    if not project['words']:
        raise ValueError('Nhận lời hoặc đồng bộ lời trước khi tự phân vai.')
    preference=(request.get_json(silent=True) or {}).get('device','auto')
    def work(job):
        result=engine.suggest_duet(project,directory(pid),job,preference)
        job.check()
        with LOCK:
            current=load(pid)
            roles=dict(zip(result['lines'],result['roles']))
            if result['two_registers']:
                for word in current['words']:
                    role=roles.get(word['line'])
                    if role is not None:word['role']=role
                current['settings']=validate_settings({'singing_mode':'duet'},current['settings'])
            current['role_analysis']={key:value for key,value in result.items() if key not in ('roles','lines')}
            save(current)
        return result
    return task(pid,'roles',work)


@app.post("/api/projects/<pid>/estimate")
def estimate(pid):
    idle(pid)
    project = load(pid)
    duration = (project.get("reference") or project.get("beat") or {}).get("duration")
    if not duration:
        raise ValueError("Hãy nạp audio trước.")
    project["text"] = str((request.get_json() or {}).get("text", ""))[:300000]
    project["words"] = estimate_words(project["text"], duration)
    save(project)
    return jsonify(public(project))


@app.get("/api/jobs/<jid>")
def job_status(jid):
    if jid not in JOBS:
        raise FileNotFoundError("Không tìm thấy tác vụ; app có thể đã khởi động lại.")
    return jsonify(JOBS[jid].public())


@app.post("/api/jobs/<jid>/cancel")
def job_cancel(jid):
    if jid not in JOBS:
        raise FileNotFoundError("Không tìm thấy tác vụ.")
    JOBS[jid].stop()
    return jsonify(JOBS[jid].public())


@app.get('/api/projects/<pid>/render-info')
def render_information(pid):
    import render_info
    return jsonify(render_info.describe(load(pid),directory(pid)))


@app.post('/api/projects/<pid>/export-folder')
def choose_export_folder(pid):
    import subprocess
    import sys
    idle(pid)
    data=request.get_json(silent=True) or {}
    if data.get('pick'):
        result=subprocess.run([sys.executable,engine.ROOT/'tools/pick_export_folder.py'],capture_output=True,
                              text=True,encoding='utf-8',timeout=300,creationflags=engine.CREATE_NO_WINDOW)
        if result.returncode:raise ValueError('Không mở được hộp chọn thư mục. Bạn có thể nhập đường dẫn rồi bấm Lưu.')
        folder=json.loads(result.stdout)['folder']
        if not folder:return jsonify({'cancelled':True})
    else:folder=str(data.get('folder','')).strip()
    if folder:
        target=Path(folder).expanduser()
        if not target.is_absolute() or not target.is_dir():raise ValueError('Chọn thư mục đã tồn tại bằng đường dẫn đầy đủ.')
        folder=str(target.resolve())
    project=load(pid);project['export_directory']=folder;save(project)
    return jsonify({'folder':folder})


@app.post("/api/projects/<pid>/render")
def render(pid):
    snapshot = load(pid)
    return task(pid, "render", lambda job: engine.render(snapshot, directory(pid), job))


@app.get("/api/projects/<pid>/exports/<jid>/<filename>")
def export_file(pid, jid, filename):
    if not re.fullmatch(r"[0-9a-f]{32}", jid) or filename not in ("karaoke.mp4", "karaoke.mov", "karaoke.mkv", "lyrics.ass", "lyrics.srt", "lyrics.lrc", "render-info.json"):
        raise ValueError("File không hợp lệ.")
    return send_file(directory(pid) / "exports" / jid / filename, as_attachment=True)


@app.get("/api/projects/<pid>/download/<kind>")
def download(pid, kind):
    project = load(pid)
    if kind == "json":
        content = json.dumps({k:v for k,v in project.items() if k not in ("reference", "beat")},
                             ensure_ascii=False, indent=2)
    else:
        content = engine.export_text(project, kind)
    safe_name = re.sub(r'[^\w\- ]', '', project["name"])[:70] or "karaoke"
    return send_file(io.BytesIO(content.encode("utf-8-sig")), as_attachment=True,
                     download_name=safe_name + (".obs.json" if kind == "json" else "." + kind),
                     mimetype="application/json" if kind == "json" else "text/plain; charset=utf-8")


@app.post("/api/projects/import")
def import_project():
    data = request.get_json()
    if data.get("schema") != 1:
        raise ValueError("Định dạng project chưa được hỗ trợ.")
    project = {"id": uuid.uuid4().hex, "schema": 1, "name": str(data.get("name", "Project đã nhập"))[:150],
               "reference": None, "beat": None, "words": validate_words(data.get("words", [])),
               "anchors": validate_anchors(data.get("anchors", [])), "text": str(data.get("text", ""))[:300000],
               "alignment": None, "settings": deepcopy(DEFAULT_SETTINGS)}
    project["settings"] = validate_settings(data.get("settings", {}))
    (engine.DATA / "projects" / project["id"]).mkdir(parents=True, exist_ok=True)
    save(project)
    return jsonify(public(project)), 201
