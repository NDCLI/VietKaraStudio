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
import tempfile
import time
import uuid
from urllib.parse import urlparse

from flask import Flask, jsonify, request, send_file, send_from_directory
from bundled_assets import ASSET_ROOT, bundled_fonts, asset_url, ICON_CHOICES, vni_table, tcvn_table
from werkzeug.exceptions import HTTPException

import engine
import analysis_cache
from app_runtime import task_command
import hardware
import presets
import layout
import media_tools
import audio_sync
import batch_render
import project_archive
import updater
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


@app.get('/api/updates/check')
def update_check():
    return jsonify(updater.check())


@app.post('/api/updates/download')
def update_download():
    return jsonify(updater.download())


@app.post('/api/updates/apply')
def update_apply():
    return jsonify(updater.apply())


@app.post('/api/projects/<pid>/models/<name>')
def download_model(pid, name):
    import model_manager
    import sys
    directory(pid)
    if name not in model_manager.MODELS:
        raise ValueError('Model không hợp lệ.')
    def work(job):
        job.update(5, 'Đang tải model ' + name + '… lần đầu có thể cần vài phút.')
        engine.command(task_command('model', engine.DATA, name), job,
                       progress=engine.model_progress(job))
        return {'model': name}
    return task(pid, 'model', work)


@app.post('/api/projects/<pid>/separate')
def separate_tool(pid):
    idle(pid)
    project_path = directory(pid)
    payload = request.get_json(silent=True) or {}
    file = request.files.get('file')
    from_video = payload.get('source') == 'background'
    from_youtube = payload.get('source') == 'youtube'
    apply_project = from_video and payload.get('apply_to_project') is True
    if apply_project:
        project_archive.create_version(project_path, load(pid), "Trước khi thay media bằng kết quả tách vocal/beat")
    source = media_tools.local_asset(project_path, load(pid).get('background_video', {}).get('source_file')) if from_video else None
    youtube_url = str(payload.get('url', '')).strip() if from_youtube else ''
    if from_video and not source:
        raise ValueError('Hãy nạp video trong File nguồn trước.')
    if from_youtube and not media_tools.youtube_video_id(youtube_url):
        raise ValueError('URL YouTube không hợp lệ hoặc có playlist. Hãy dán link một video đơn.')
    if not from_video and not from_youtube and (not file or not file.filename):
        raise ValueError('Chọn bài nhạc hoặc dán URL YouTube cần tách.')
    ext = source.suffix.lower() if from_video else Path(file.filename).suffix.lower() if file else '.m4a'
    if not from_youtube and ext not in ('.wav', '.mp3', '.flac', '.m4a', '.aac', '.ogg', '.mp4', '.mov', '.m4v', '.avi', '.mkv', '.webm', '.wma'):
        raise ValueError('Định dạng âm thanh/video chưa hỗ trợ.')
    identifier = uuid.uuid4().hex
    folder = engine.DATA / 'tools' / identifier
    folder.mkdir(parents=True)
    incoming = folder / ('source' + ext) if not from_youtube else None
    if from_video:
        shutil.copy2(source, incoming)
    elif not from_youtube:
        file.save(incoming)
    preference = payload.get('device', 'auto') if (from_video or from_youtube) else request.form.get('device', 'auto')
    def work(job):
        downloaded = None
        try:
            if from_youtube:
                downloaded = media_tools.download_youtube(youtube_url, folder, job)
                input_source = downloaded
            else:
                input_source = incoming
            engine.convert(input_source, folder / 'reference.wav', job)
            device = hardware.select_ai(preference=preference)['demucs']
            engine.separate(folder / 'reference.wav', folder, job, device)
            job.check()
            if apply_project:
                metadata = {slot: {'name': name, **engine.waveform(folder / file_name)} for slot, name, file_name in (
                    ('reference', source.name, 'reference.wav'),
                    ('beat', 'Beat · ' + load(pid)['background_video']['name'], 'reference_instrumental.wav'),
                    ('vocals', 'Vocal · ' + load(pid)['background_video']['name'], 'vocals.wav'))}
                with LOCK:
                    current = load(pid)
                    for original, target in (('reference.wav', 'reference.wav'), ('reference_instrumental.wav', 'beat.wav'),
                                             ('vocals.wav', 'vocals.wav'), ('reference_instrumental.wav', 'reference_instrumental.wav')):
                        staged = project_path / (target + '.next')
                        shutil.copy2(folder / original, staged)
                        staged.replace(project_path / target)
                    current.update(metadata)
                    current['reference']['name'] = current['background_video']['name']
                    current['words'] = []; current['anchors'] = []; current['alignment'] = None
                    current.pop('processing', None); current.pop('role_analysis', None)
                    current['settings'] = validate_settings({'background_type': 'video'}, current['settings'])
                    save(current)
            return {'beat': f'/api/tools/{identifier}/beat', 'vocals': f'/api/tools/{identifier}/vocals', 'project_id': pid, 'applied': apply_project}
        finally:
            if downloaded:
                downloaded.unlink(missing_ok=True)
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
        project = json.loads((directory(pid) / "project.json").read_text(encoding="utf-8"))
        words = project.get("words", [])
        if project.get("schema", 1) < 2 or any(not word.get("id") or not word.get("line_uid") for word in words):
            project["words"] = validate_words(words)
            project["schema"] = 2
            save(project)
        return project


def save(project):
    with LOCK:
        project["schema"] = 2
        project["words"] = validate_words(project.get("words", []))
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
    if result['vocals_ready'] and not result.get('vocals'):
        result['vocals']={'name':'Vocal riêng',**engine.waveform(engine.DATA/'projects'/project['id']/'vocals.wav')}
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


def start_batch_job(record):
    owner = record["owner"]
    job = engine.Job(record["job_id"], owner, "batch-render")
    with LOCK:
        existing = JOBS.get(job.id)
        if existing and existing.state in ("queued", "running"):
            return existing
        JOBS[job.id] = job
    def work():
        try:
            job.state = "running"
            result = batch_render.run(engine.DATA, record["id"], job)
            with job._lock:
                job.result = result
                job.state, job.progress, job.message = "done", 100, "Hoàn tất hàng đợi dựng video"
        except engine.Cancelled:
            batch_render.set_status(engine.DATA, record["id"], "cancelled")
            job.state, job.message = "cancelled", "Đã dừng hàng đợi. Có thể tiếp tục từ project chưa xong."
        except Exception as error:
            batch_render.set_status(engine.DATA, record["id"], "error")
            job.state, job.message = "error", str(error)[-3000:]
    POOL.submit(work)
    return job


@app.before_request
def local_requests():
    if request.method not in ("GET", "HEAD", "OPTIONS"):
        origin = request.headers.get("Origin")
        if origin and urlparse(origin).netloc != request.host:
            return jsonify(error="Yêu cầu phải đến từ cửa sổ app."), 403


@app.after_request
def refresh_static_assets(response):
    if request.path.startswith('/static/'):
        response.headers['Cache-Control'] = 'no-cache, must-revalidate'
    return response


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
    for name in ("demucs", "faster_whisper", "stable_whisper", "torch", "librosa", "yt_dlp"):
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
    if request.args.get('catalog') == '1':
        bundled = bundled_fonts()
        families = {item['family'] for item in bundled}
        return jsonify(bundled=bundled,
                       system=[name for name in layout.vietnamese_fonts() if name not in families],
                       encodings={'vni':vni_table(), 'tcvn3':tcvn_table()},
                       icons={role:[{'file':name, 'label':label, 'url':asset_url('ICON/'+name)}
                                    for name,label in choices] for role,choices in ICON_CHOICES.items()})
    if request.args.get('vietnamese') == '1':
        return jsonify(layout.vietnamese_fonts())
    return jsonify(sorted(set(item[0] for item in layout.font_catalog()), key=str.casefold))


@app.get('/api/assets/source/<path:filename>')
def source_asset(filename):
    return send_from_directory(ASSET_ROOT, filename)


@app.get('/api/fonts/bundled.css')
def bundled_font_css():
    rules = []
    # Use the same preferred bold face as layout measurement and ASS karaoke.
    for family in sorted({item['family'] for item in bundled_fonts()}):
        for italic in (False, True):
            chosen, _ = layout.resolve_font(family, italic)
            relative = Path(chosen[2]).relative_to(ASSET_ROOT).as_posix()
            rules.append('@font-face{font-family:'+json.dumps(family)+';src:url('+json.dumps(asset_url(relative))
                         +');font-weight:100 900;font-style:'+('italic' if italic else 'normal')+';font-display:block;}')
    return app.response_class('\n'.join(rules), mimetype='text/css')


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
    project = {"id": pid, "schema": 2, "name": str(data.get("name", "Bài karaoke mới"))[:150],
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


@app.get("/api/projects/<pid>/versions")
def get_project_versions(pid):
    return jsonify(project_archive.list_versions(directory(pid)))


@app.post("/api/projects/<pid>/versions")
def create_project_version(pid):
    idle(pid)
    data = request.get_json(silent=True) or {}
    record = project_archive.create_version(directory(pid), load(pid), data.get("note", ""))
    return jsonify(record), 201


@app.post("/api/projects/<pid>/versions/<identifier>/restore")
def restore_project_version(pid, identifier):
    idle(pid)
    path = directory(pid)
    current = load(pid)
    project_archive.create_version(path, current, "Tự lưu trước khi khôi phục phiên bản")
    restored = project_archive.restore_version(path, identifier)
    restored["id"] = pid
    save(restored)
    return jsonify(public(load(pid)))


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
    audio_extensions = (".mp3", ".wav", ".flac", ".m4a", ".aac", ".ogg", ".opus")
    allowed = (media_tools.IMAGE_EXTENSIONS + media_tools.VIDEO_EXTENSIONS if slot == "background"
               else media_tools.IMAGE_EXTENSIONS if slot in ('countdown', 'logo')
               else audio_extensions + media_tools.VIDEO_EXTENSIONS)
    if ext not in allowed:
        raise ValueError("Định dạng file chưa được hỗ trợ.")
    incoming = path / ("incoming_" + uuid.uuid4().hex + ext)
    file.save(incoming)
    project_archive.create_version(path, load(pid), f"Trước khi thay nguồn {slot}")
    def work(job):
        relinked = False
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
                project=load(pid)
                relinked=_matches_missing_media(project,slot,destination)
                destination.replace(path/f'{slot}.png')
                project[f'{slot}_name']=filename
                if slot == 'logo' and not relinked:
                    project['logo_version']=uuid.uuid4().hex
                    project['logo_image']={'width':image.width,'height':image.height}
                if not relinked:
                    project['settings']=validate_settings({'logo_enabled':True} if slot == 'logo' else {'countdown':'image'},project['settings'])
            elif slot == 'background' and ext in media_tools.VIDEO_EXTENSIONS:
                metadata=media_tools.probe_video(incoming)
                project=load(pid)
                relinked=_matches_missing_media(project,slot,incoming)
                proxy=path/'background_preview_next.mp4'
                media_tools.make_proxy(incoming,proxy,job)
                job.check()
                proxy.replace(path/'background_preview.mp4')
                source_file='background_source'+ext
                incoming.replace(path/source_file)
                project['background_video']={'name':filename,'source_file':source_file,'preview_file':'background_preview.mp4',**metadata}
                if not relinked:
                    project['settings']=validate_settings({'background_type':'video'},project['settings'])
            elif slot == "background":
                destination = path / "background_next.jpg"
                engine.command([engine.binary("ffmpeg"), "-y", "-v", "error", "-i", incoming,
                                "-frames:v", "1", "-update", "1", destination], job)
                job.check()
                project = load(pid)
                relinked=_matches_missing_media(project,slot,destination)
                destination.replace(path / "background.jpg")
                project["background_name"] = filename
                if not relinked:
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
                project = load(pid)
                relinked = _matches_missing_media(project, slot, temporary)
                was_missing = bool((project.get("missing_media") or {}).get(slot))
                had_reference = bool(project.get("reference") or (project.get("missing_media") or {}).get("reference"))
                replaced_existing = bool(project.get(slot) or was_missing)
                temporary.replace(path / f"{slot}.wav")
                if video:
                    proxy.replace(path/video['preview_file'])
                    incoming.replace(path/video['source_file'])
                    metadata['video']=video
                project[slot] = metadata
                if not relinked:
                    if replaced_existing:
                        project["anchors"] = []
                    project["alignment"] = None
                    if slot == "reference":
                        project.pop('vocals',None)
                        if had_reference:
                            project["words"] = []
                        for old in ("vocals.wav", "reference_instrumental.wav"):
                            (path / old).unlink(missing_ok=True)
                if project["name"] == "Bài karaoke mới":
                    project["name"] = Path(filename).stem[:150]
            if isinstance(project.get("missing_media"), dict):
                project["missing_media"].pop(slot, None)
            save(project)
            return {"project_id": pid, "relinked": relinked, "slot": slot}
        finally:
            incoming.unlink(missing_ok=True)
    try:
        return task(pid, "upload", work)
    except Exception:
        incoming.unlink(missing_ok=True)
        raise


def _matches_missing_media(project, slot, media_path):
    expected = (project.get("missing_media") or {}).get(slot, {}).get("fingerprint")
    if not isinstance(expected, dict):
        return False
    try:
        stat = media_path.stat()
        if stat.st_size != int(expected.get("size", -1)):
            return False
        digest = str(expected.get("sha256", ""))
        return bool(re.fullmatch(r"[0-9a-f]{64}", digest) and project_archive.sha256(media_path) == digest)
    except (OSError, TypeError, ValueError):
        return False


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
        project.pop('vocals',None)
        project['anchors']=[]
        project['alignment']=None
        for old in ('vocals.wav','reference_instrumental.wav'):
            (path/old).unlink(missing_ok=True)
        save(project)
        return {'project_id':pid}
    return task(pid,'upload',work)


def recognize_cached(path, job, model, language, lyrics, preference, region=None):
    """Reuse recognition only when source content and every analysis input match."""
    reference = path / "reference.wav"
    key = analysis_cache.cache_key(reference, model, language, lyrics, preference, region)
    cached = analysis_cache.load(engine.DATA, key) if key else None
    if cached:
        job.check()
        temporary = path / ("vocals_cache_next_" + uuid.uuid4().hex + ".wav")
        try:
            shutil.copy2(cached["vocals"], temporary)
            job.check()
            temporary.replace(path / "vocals.wav")
        except OSError:
            temporary.unlink(missing_ok=True)
            cached = None
        if cached:
            job.record_device("demucs", {"device": "cache", "name": "Vocal dùng lại từ cache"})
            job.record_device("whisper", {"device": "cache", "name": "Nhận lời dùng lại từ cache"})
            job.update(95, "Đang nạp kết quả nhận lời đã lưu theo nguồn và thiết lập…")
            return deepcopy(cached["result"]), True
    result = engine.recognize(path, job, model, language, lyrics, preference, region=region)
    job.check()
    if key:
        analysis_cache.store(engine.DATA, key, result, path / "vocals.wav")
    return result, False


@app.post("/api/projects/<pid>/analyze")
def analyze(pid):
    idle(pid)
    project = load(pid)
    if not project.get("reference"):
        raise ValueError("Hãy nạp bản có giọng hát tham chiếu.")
    if any(word.get('locked') for word in project.get('words', [])):
        raise ValueError('Mở khóa các câu đã khóa trước khi nhận lời và căn lại toàn bài.')
    project_archive.create_version(directory(pid), project, "Trước khi AI nhận lời và căn toàn bài")
    options = request.get_json(silent=True) or {}
    lyrics = str(options.get("lyrics", ""))[:300000]
    model, language, preference = (options.get("model", "medium"),
                                   options.get("language", "vi"), options.get("device", "auto"))
    def work(job):
        result, cache_hit = recognize_cached(directory(pid), job, model, language, lyrics, preference)
        job.check()
        with LOCK:
            current = load(pid)
            current["words"] = validate_words(result["words"])
            current["text"] = result["text"]
            current['vocals']={'name':'Vocal · '+current['reference']['name'],**engine.waveform(directory(pid)/'vocals.wav')}
            current["processing"] = {"devices": job.public()["devices"], "fallbacks": job.public()["fallbacks"],
                                      "device": result["device"], "compute_type": result["compute_type"],
                                      'alignment_diagnostics':result.get('alignment_diagnostics',{})}
            save(current)
        return {"project_id": pid, "words": len(current["words"]), "device": result.get("device", "cache"),
                "device_name": result.get("device_name", "cache"),
                "compute_type": result.get("compute_type", "cache"), "cache_hit": cache_hit}
    return task(pid, "analyze", work)


@app.post('/api/projects/<pid>/align-region')
def align_region(pid):
    idle(pid)
    project = load(pid)
    if not project.get('reference'):
        raise ValueError('Hãy nạp bản có giọng hát tham chiếu.')
    if not project.get('words'):
        raise ValueError('Cần có lời đã căn trước khi căn lại một câu.')
    options = request.get_json(silent=True) or {}
    raw_ids = options.get('line_ids')
    if not isinstance(raw_ids, list) or not raw_ids or len(raw_ids) > 8:
        raise ValueError('Chọn từ 1 đến 8 câu để căn lại.')
    if any(type(value) is not int or value < 0 for value in raw_ids):
        raise ValueError('Danh sách câu không hợp lệ.')
    line_ids = list(dict.fromkeys(raw_ids))
    line_order = list(dict.fromkeys(word['line'] for word in project['words']))
    selected = {line_id: [word for word in project['words'] if word['line'] == line_id] for line_id in line_ids}
    if any(not words for words in selected.values()):
        raise ValueError('Không tìm thấy câu cần căn lại.')
    if any(any(word.get('locked') for word in words) for words in selected.values()):
        raise ValueError('Câu này đang khóa. Mở khóa trước khi căn lại.')
    lyrics = '\n'.join(' '.join(word['text'] for word in selected[line_id]) for line_id in line_ids)
    if sum(len(words) for words in selected.values()) > 500:
        raise ValueError('Đoạn chọn quá dài. Hãy căn lại từng phần ngắn hơn.')
    reference_duration = float(project['reference'].get('duration') or 0)
    start = max(0., min(word['start'] for words in selected.values() for word in words) - 1.2)
    end = max(word['end'] for words in selected.values() for word in words) + 1.2
    if reference_duration:
        end = min(reference_duration, end)
    if end - start > 120:
        raise ValueError('Đoạn căn lại dài hơn 2 phút. Hãy chọn ít câu hơn.')
    if end - start < 1:
        raise ValueError('Không đủ âm thanh quanh câu để căn lại.')
    model, language, preference = (options.get('model', 'medium'),
                                   options.get('language', 'vi'), options.get('device', 'auto'))

    def work(job):
        result, cache_hit = recognize_cached(directory(pid), job, model, language, lyrics, preference,
                                             region={'start': start, 'end': end})
        job.check()
        aligned = [[] for _ in line_ids]
        for word in result['words']:
            index = int(word.get('line', 0))
            if index < 0 or index >= len(line_ids):
                continue
            line_id = line_ids[index]
            source = selected[line_id][0]
            word['line'] = line_id
            word['role'] = source.get('role', 'all')
            if source.get('row') in (0, 1):
                word['row'] = source['row']
            aligned[index].append(word)
        if any(not words for words in aligned):
            raise RuntimeError('AI chưa căn đủ các câu đã chọn. Chia câu ngắn hơn rồi thử lại.')
        replacements = {line_id: aligned[index] for index, line_id in enumerate(line_ids)}
        line_words = {line_id: [] for line_id in line_order}
        for word in project['words']:
            if word['line'] not in replacements:
                line_words.setdefault(word['line'], []).append(word)
        for line_id, words in replacements.items():
            line_words[line_id] = words
        candidate = []
        for line_id in line_order:
            candidate.extend(sorted(line_words.get(line_id, []), key=lambda word: (word['start'], word['end'])))
        candidate = validate_words(candidate)
        old_words = [word for line_id in line_ids for word in selected[line_id]]
        new_words = [word for line_id in line_ids for word in replacements[line_id]]
        text_by_line = {}
        for word in candidate:
            text_by_line.setdefault(word['line'], []).append(word['text'])
        text = '\n'.join(' '.join(tokens) for tokens in text_by_line.values())
        return {'line_ids': line_ids, 'old_words': old_words, 'new_words': new_words,
                'cache_hit': cache_hit,
                'words': candidate, 'text': text, 'diagnostics': result.get('alignment_diagnostics', {})}
    return task(pid, 'align-region', work)


@app.post('/api/projects/<pid>/sync-suggest')
def sync_suggest(pid):
    idle(pid)
    project = load(pid)
    if not project.get('reference') or not project.get('beat'):
        raise ValueError('Hãy nạp cả bản hát tham chiếu và beat trước khi dò đồng bộ.')
    path = directory(pid)
    if not (path / 'reference.wav').is_file() or not (path / 'beat.wav').is_file():
        raise ValueError('Thiếu file âm thanh gốc để dò. Hãy nạp lại nguồn của project.')
    return task(pid, 'sync-suggest', lambda job: audio_sync.suggest(path / 'reference.wav', path / 'beat.wav', job))


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
    if any(word.get('locked') for word in project.get('words', [])):
        raise ValueError('Mở khóa các câu đã khóa trước khi tạo lại mốc toàn bài.')
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


@app.post("/api/batches/render")
def create_render_batch():
    data = request.get_json(silent=True) or {}
    raw_ids = data.get("project_ids")
    if not isinstance(raw_ids, list) or not raw_ids or len(raw_ids) > 50:
        raise ValueError("Chọn từ 1 đến 50 project để dựng theo lô.")
    project_ids = list(dict.fromkeys(raw_ids))
    if len(project_ids) != len(raw_ids):
        raise ValueError("Danh sách project bị trùng.")
    aspects = data.get("aspects", ["current"])
    if (not isinstance(aspects, list) or not aspects or len(aspects) > 3 or
            any(value not in ("current", "landscape", "portrait") for value in aspects)):
        raise ValueError("Chọn tỉ lệ hiện tại, ngang hoặc dọc cho đầu ra theo lô.")
    aspects = list(dict.fromkeys(["current", *aspects]))
    include_guide = data.get("include_guide", False)
    if type(include_guide) is not bool:
        raise ValueError("Lựa chọn vocal hướng dẫn không hợp lệ.")
    selected = []
    for pid in project_ids:
        idle(pid)
        project = load(pid)
        if not project.get("beat") or not (directory(pid) / "beat.wav").is_file():
            raise ValueError(f"Project {project['name']} chưa có beat gốc.")
        if not project.get("words"):
            raise ValueError(f"Project {project['name']} chưa có lời để xuất.")
        selected.append((project, directory(pid)))
    record = batch_render.create(engine.DATA, selected, project_ids[0], aspects, include_guide)
    job = start_batch_job(record)
    return jsonify(job.public()), 202


@app.get("/api/batches/<batch_id>")
def get_render_batch(batch_id):
    if not re.fullmatch(r"[0-9a-f]{32}", batch_id):
        raise ValueError("Mã hàng đợi không hợp lệ.")
    return jsonify(batch_render.load_record(engine.DATA, batch_id))


@app.get("/api/batches")
def list_render_batches():
    root = engine.DATA / "batch-renders"
    records = []
    if root.is_dir():
        for file in root.glob("*/batch.json"):
            try:
                record = json.loads(file.read_text(encoding="utf-8"))
                records.append({"id": record["id"], "status": record["status"],
                               "created_at": record["created_at"], "next_index": record.get("next_index", 0),
                               "count": len(record.get("items", [])), "completed": record.get("completed", 0),
                               "failed": record.get("failed", 0)})
            except (OSError, ValueError, KeyError, TypeError):
                continue
    return jsonify(sorted(records, key=lambda item: item["created_at"], reverse=True)[:30])


@app.post("/api/batches/<batch_id>/resume")
def resume_render_batch(batch_id):
    if not re.fullmatch(r"[0-9a-f]{32}", batch_id):
        raise ValueError("Mã hàng đợi không hợp lệ.")
    record = batch_render.load_record(engine.DATA, batch_id)
    if record.get("status") not in ("cancelled", "error", "done"):
        raise ValueError("Chỉ có thể tiếp tục hàng đợi đã dừng hoặc hoàn tất có project lỗi.")
    record["next_index"] = batch_render.first_pending(record)
    if record["next_index"] >= len(record["items"]):
        raise ValueError("Hàng đợi đã hoàn tất, không còn project cần dựng lại.")
    record["job_id"] = uuid.uuid4().hex
    record["status"] = "queued"
    batch_render.save_record(engine.DATA, record)
    return jsonify(start_batch_job(record).public()), 202


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
        result=subprocess.run([sys.executable,engine.ROOT/'tools/pick_export_folder.py'],stdin=subprocess.DEVNULL,
                              capture_output=True,text=True,encoding='utf-8',timeout=300,
                              creationflags=engine.CREATE_NO_WINDOW)
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
    if not re.fullmatch(r"[0-9a-f]{32}", jid) or filename not in ("karaoke.mp4", "karaoke.mov", "karaoke.mkv", "lyrics.ass", "lyrics.srt", "lyrics.lrc", "render-info.json", "qa-contact.jpg"):
        raise ValueError("File không hợp lệ.")
    return send_file(directory(pid) / "exports" / jid / filename, as_attachment=True)


@app.get("/api/projects/<pid>/download/<kind>")
def download(pid, kind):
    project = load(pid)
    if kind in ("bundle-light", "bundle-full"):
        output = tempfile.SpooledTemporaryFile(max_size=16 * 1024 * 1024)
        project_archive.write_bundle(output, project, directory(pid), kind == "bundle-full")
        output.seek(0)
        safe_name = re.sub(r'[^\w\- ]', '', project["name"])[:70] or "vietkara-project"
        return send_file(output, mimetype="application/zip", as_attachment=True,
                         download_name=safe_name + ("-day-du.zip" if kind == "bundle-full" else "-gon.zip"))
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
    if data.get("schema") not in (1, 2):
        raise ValueError("Định dạng project chưa được hỗ trợ.")
    project = {"id": uuid.uuid4().hex, "schema": 2, "name": str(data.get("name", "Project đã nhập"))[:150],
               "reference": None, "beat": None, "words": validate_words(data.get("words", [])),
               "anchors": validate_anchors(data.get("anchors", [])), "text": str(data.get("text", ""))[:300000],
               "alignment": None, "settings": deepcopy(DEFAULT_SETTINGS)}
    project["settings"] = validate_settings(data.get("settings", {}))
    (engine.DATA / "projects" / project["id"]).mkdir(parents=True, exist_ok=True)
    save(project)
    return jsonify(public(project)), 201


@app.post("/api/projects/import-bundle")
def import_project_bundle():
    uploaded = request.files.get("file")
    if not uploaded or not uploaded.filename:
        raise ValueError("Chọn file project ZIP cần nhập.")
    pid = uuid.uuid4().hex
    path = engine.DATA / "projects" / pid
    path.mkdir(parents=True, exist_ok=True)
    try:
        data, copied, fingerprints = project_archive.read_bundle(uploaded.stream, path)
        reference = deepcopy(data.get("reference")) if isinstance(data.get("reference"), dict) else None
        beat = deepcopy(data.get("beat")) if isinstance(data.get("beat"), dict) else None
        missing_media = {}
        if reference and "reference.wav" not in copied:
            missing_media["reference"] = {"name": reference.get("name", "Bản hát"),
                                          "fingerprint": fingerprints.get("reference.wav")}
            reference = None
        if beat and "beat.wav" not in copied:
            missing_media["beat"] = {"name": beat.get("name", "Beat"),
                                     "fingerprint": fingerprints.get("beat.wav")}
            beat = None
        for item in (reference, beat):
            video = item.get("video") if item else None
            if video and video.get("source_file") not in copied:
                item.pop("video", None)
        background_video = deepcopy(data.get("background_video")) if isinstance(data.get("background_video"), dict) else None
        if background_video and background_video.get("source_file") not in copied:
            missing_media["background"] = {"name": background_video.get("name", "Video nền"),
                                            "fingerprint": fingerprints.get(background_video.get("source_file"))}
            background_video = None
        elif data.get("background_name") and "background.jpg" not in copied:
            missing_media["background"] = {"name": data.get("background_name"),
                                            "fingerprint": fingerprints.get("background.jpg")}
        project = {"id": pid, "schema": 2, "name": str(data.get("name", "Project đã nhập"))[:150],
                   "reference": reference, "beat": beat,
                   "words": validate_words(data.get("words", [])),
                   "anchors": validate_anchors(data.get("anchors", [])),
                   "text": str(data.get("text", ""))[:300000], "alignment": data.get("alignment"),
                   "settings": validate_settings(data.get("settings", {})),
                   "background_video": background_video,
                   "relink_assets": fingerprints, "missing_media": missing_media}
        for key in ("background_name", "logo_name", "logo_image", "logo_version", "countdown_name", "processing"):
            if key in data:
                project[key] = data[key]
        if data.get("logo_name") and "logo.png" not in copied:
            project["missing_media"]["logo"] = {"name": data["logo_name"], "fingerprint": fingerprints.get("logo.png")}
        if data.get("countdown_name") and "countdown.png" not in copied:
            project["missing_media"]["countdown"] = {"name": data["countdown_name"], "fingerprint": fingerprints.get("countdown.png")}
        save(project)
    except Exception:
        shutil.rmtree(path, ignore_errors=True)
        raise
    return jsonify(public(project)), 201


def recover_render_batches():
    root = engine.DATA / "batch-renders"
    if not root.is_dir():
        return
    for file in root.glob("*/batch.json"):
        try:
            record = json.loads(file.read_text(encoding="utf-8"))
            if record.get("status") not in ("queued", "running") or not record.get("items"):
                continue
            record["next_index"] = batch_render.first_pending(record)
            for item in record["items"]:
                if item.get("status") == "running":
                    item["status"] = "queued"
            if record["next_index"] >= len(record["items"]):
                record["status"] = "done"
                batch_render.save_record(engine.DATA, record)
                continue
            record["status"] = "queued"
            batch_render.save_record(engine.DATA, record)
            start_batch_job(record)
        except (OSError, ValueError, KeyError, TypeError):
            app.logger.exception("Unable to recover a batch render queue")


recover_render_batches()
