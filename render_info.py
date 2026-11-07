"""Export settings and assets, shared by the dialog and saved render manifest."""
from fractions import Fraction
from functools import lru_cache
import json
import math
from pathlib import Path
import re
import subprocess

import media_tools
from styles import validate_settings


@lru_cache(maxsize=32)
def _audio_metrics(path: str, mtime_ns: int, size: int) -> dict:
    import numpy as np
    import soundfile as sf
    peak = 0.0
    squares = 0.0
    samples = clipped = 0
    with sf.SoundFile(path) as audio:
        while True:
            chunk = audio.read(max(1, audio.samplerate * 10), dtype="float32", always_2d=True)
            if not len(chunk):
                break
            absolute = np.abs(chunk)
            peak = max(peak, float(absolute.max(initial=0)))
            squares += float(np.square(chunk, dtype=np.float64).sum())
            samples += chunk.size
            clipped += int(np.count_nonzero(absolute >= .9999))
    rms = math.sqrt(squares / max(1, samples))
    return {"peak_dbfs": round(20 * math.log10(max(peak, 1e-12)), 1),
            "rms_dbfs": round(20 * math.log10(max(rms, 1e-12)), 1),
            "clipped_samples": clipped}


@lru_cache(maxsize=64)
def _font_codepoints(path: str) -> frozenset[int]:
    from fontTools.ttLib import TTFont
    with TTFont(path, fontNumber=0, lazy=True) as font:
        return frozenset((font.getBestCmap() or {}).keys())


def _relative_luminance(color: str) -> float:
    channels=[]
    for offset in (1, 3, 5):
        value=int(color[offset:offset+2],16)/255
        channels.append(value/12.92 if value<=.04045 else ((value+.055)/1.055)**2.4)
    return .2126*channels[0]+.7152*channels[1]+.0722*channels[2]


def _contrast_ratio(first: str, second: str) -> float:
    high,low=sorted((_relative_luminance(first),_relative_luminance(second)),reverse=True)
    return (high+.05)/(low+.05)


def verify_output(path: Path, duration: float, width: int, height: int) -> dict:
    """Confirm the rendered file has playable video and audio streams."""
    import engine
    if not path.is_file() or path.stat().st_size < 1024:
        raise RuntimeError("Video sau render bị thiếu hoặc quá nhỏ để phát.")
    try:
        probe = engine.binary("ffprobe")
    except RuntimeError:
        probe = None
    if probe:
        result = subprocess.run([probe, "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path)],
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8",
                                errors="replace", timeout=90, creationflags=engine.CREATE_NO_WINDOW)
        if result.returncode:
            raise RuntimeError("Không đọc được thông tin video sau render: " + result.stderr[-700:])
        data = json.loads(result.stdout)
        streams = data.get("streams", [])
        video = next((stream for stream in streams if stream.get("codec_type") == "video"), None)
        audio = next((stream for stream in streams if stream.get("codec_type") == "audio"), None)
        if not video or not audio:
            raise RuntimeError("Video sau render phải có cả luồng hình và âm thanh.")
        output_duration = float(data.get("format", {}).get("duration") or 0)
        if output_duration and abs(output_duration - duration) > max(1.0, duration * .01):
            raise RuntimeError(f"Thời lượng sau render lệch nguồn: {output_duration:.2f}s / {duration:.2f}s.")
        actual_width, actual_height = int(video.get("width", 0)), int(video.get("height", 0))
        if (actual_width, actual_height) != (width, height):
            raise RuntimeError(f"Kích thước video sau render là {actual_width}×{actual_height}, cần {width}×{height}.")
        return {"verified": True, "duration": round(output_duration, 3),
                "width": actual_width, "height": actual_height,
                "video_codec": video.get("codec_name"), "audio_codec": audio.get("codec_name")}
    result = subprocess.run([engine.binary("ffmpeg"), "-hide_banner", "-i", str(path)], stdin=subprocess.DEVNULL,
                            capture_output=True, text=True, encoding="utf-8", errors="replace",
                            timeout=90, creationflags=engine.CREATE_NO_WINDOW)
    summary = result.stderr
    if "Video:" not in summary or "Audio:" not in summary:
        raise RuntimeError("Không xác nhận được cả luồng hình và tiếng của video sau render.")
    match = re.search(r"Duration: (\d+):(\d+):(\d+(?:\.\d+)?)", summary)
    output_duration = (int(match.group(1)) * 3600 + int(match.group(2)) * 60 + float(match.group(3))) if match else 0
    size_match = re.search(r"Video:.*?,\s*(\d+)x(\d+)", summary)
    actual_size = (int(size_match.group(1)), int(size_match.group(2))) if size_match else None
    if actual_size and actual_size != (width, height):
        raise RuntimeError(f"Kích thước video sau render là {actual_size[0]}×{actual_size[1]}, cần {width}×{height}.")
    if output_duration and abs(output_duration - duration) > max(1.0, duration * .01):
        raise RuntimeError(f"Thời lượng sau render lệch nguồn: {output_duration:.2f}s / {duration:.2f}s.")
    return {"verified": True, "duration": round(output_duration, 3) if output_duration else None,
            "width": actual_size[0] if actual_size else None, "height": actual_size[1] if actual_size else None,
            "video_codec": None, "audio_codec": None, "probe": "ffmpeg fallback"}


def sample_frames(video: Path, destination: Path, duration: float) -> dict:
    """Create a compact contact sheet sampled from the encoded video."""
    import engine
    from PIL import Image, ImageDraw, ImageFont
    times = sorted(set(round(value, 2) for value in
                       (min(.5, duration / 4), duration / 2, max(0, duration - .5))))
    temporary = destination.parent / "qa-frames"
    temporary.mkdir(parents=True, exist_ok=True)
    frames = []
    try:
        for index, seconds in enumerate(times):
            frame = temporary / f"sample-{index}.png"
            result = subprocess.run([engine.binary("ffmpeg"), "-y", "-v", "error", "-ss", str(seconds),
                                     "-i", str(video), "-frames:v", "1", "-vf", "scale=480:300:force_original_aspect_ratio=decrease",
                                     "-update", "1", str(frame)], stdin=subprocess.DEVNULL,
                                   capture_output=True, text=True, encoding="utf-8", errors="replace",
                                   timeout=90, creationflags=engine.CREATE_NO_WINDOW)
            if result.returncode or not frame.is_file():
                return {"ok": False, "error": result.stderr[-500:] or "Không lấy được ảnh mẫu."}
            frames.append((seconds, Image.open(frame).convert("RGB")))
        width, height = 480, 340
        sheet = Image.new("RGB", (width * len(frames), height), "#111925")
        draw = ImageDraw.Draw(sheet)
        font = ImageFont.load_default()
        for index, (seconds, frame) in enumerate(frames):
            x = index * width
            sheet.paste(frame, (x + (width-frame.width)//2, 8))
            draw.text((x + 12, height - 25), f"{seconds:.2f}s", fill="#e8edf5", font=font)
            frame.close()
        sheet.save(destination, "JPEG", quality=88, optimize=True)
        sheet.close()
        return {"ok": True, "file": destination.name, "times": times}
    finally:
        for _, frame in frames:
            try:frame.close()
            except Exception:pass
        import shutil
        shutil.rmtree(temporary, ignore_errors=True)


def fps_value(rate):
    try:
        value=float(Fraction(str(rate)))
        return value if 1<=value<=120 else 30.
    except (ValueError,ZeroDivisionError,OverflowError):return 30.


def encoding_options(quality,encoder):
    crf,cq,preset={'standard':(20,20,'veryfast'),'high':(16,16,'medium'),'maximum':(12,12,'slow')}[quality]
    if encoder=='h264_nvenc':
        return ['-c:v','h264_nvenc','-preset','p4' if quality=='standard' else 'p7','-rc','vbr','-cq',str(cq),'-b:v','0']
    return ['-c:v','libx264','-preset',preset,'-crf',str(crf)]


def describe(project,directory):
    s=validate_settings(project.get('settings',{}));warnings=[];errors=[]
    width,height=(1080,1920) if s['aspect']=='portrait' else (1920,1080)
    if s['resolution']=='4k':width*=2;height*=2
    kind,path='color',None
    try:kind,path=media_tools.background_source({**project,'settings':s},directory)
    except ValueError as exc:errors.append(str(exc))
    source=None
    if kind=='video':
        try:source=media_tools.probe_video(path)
        except ValueError as exc:errors.append(str(exc))
    fps=fps_value(source.get('fps',30)) if source and s['render_fps']=='source' else float(s['render_fps']) if s['render_fps']!='source' else 30.
    if source and (source['width']<width or source['height']<height):warnings.append('Nguồn video nhỏ hơn khung xuất: phóng lớn không tạo thêm chi tiết thật.')
    if source and fps>fps_value(source.get('fps',30))+.1:warnings.append('FPS xuất cao hơn nguồn chỉ lặp khung hình, không tạo chuyển động mới.')
    if not project.get('beat') or not (directory/'beat.wav').is_file():errors.append('Chưa có beat gốc.')
    if s['audio_output']=='guide' and not (directory/'vocals.wav').is_file() and not (directory/'reference.wav').is_file():
        errors.append('Cần bản hát tham chiếu hoặc vocal đã tách để tạo bản hướng dẫn.')
    if s['logo_enabled'] and project.get('logo_name') and not (directory/'logo.png').is_file():errors.append('Thiếu file logo. Hãy nạp lại ảnh logo.')
    if s['countdown']=='image' and not (directory/'countdown.png').is_file():errors.append('Thiếu ảnh đếm ngược.')
    words=project.get('words',[])
    estimated=sum(bool(w.get('estimated')) for w in words)
    if estimated:warnings.append(f'{estimated} từ đang có thời gian ước lượng; nên nghe kiểm tra trước khi xuất.')
    checks=[]
    def check(message,word_index,word,severity='warning'):
        checks.append({'severity':severity,'message':message,'word_index':word_index,
                       'line':word.get('line',0),'time':float(word.get('start',0))})
    first_word_by_line = {}
    unlocked_lines = set()
    for index, word in enumerate(words):
        line_id = word.get('line', 0)
        first_word_by_line.setdefault(line_id, (index, word))
        if not word.get('locked'):
            unlocked_lines.add(line_id)
    if unlocked_lines:
        index, word = next(entry for line_id, entry in first_word_by_line.items()
                           if line_id in unlocked_lines)
        check(f'{len(unlocked_lines)} câu chưa được khóa sau khi rà. Nhảy tới câu đầu để kiểm tra.', index, word)
    estimated_by_line={}
    previous_by_line={}
    from timing import map_time, lyric_lines
    import layout
    try:
        font, _ = layout.resolve_font(s['font'], s['italic'])
        glyphs = _font_codepoints(font[2])
        for index, word in enumerate(words):
            display_text = layout.font_text(word.get('text', ''), font[0])
            missing = sorted({character for character in display_text
                              if not character.isspace() and ord(character) not in glyphs})
            if missing:
                sample = ''.join(missing[:8])
                check(f"Font {font[0]} có thể thiếu ký tự '{sample}' · chữ sẽ thành ô vuông.", index, word)
    except Exception as exc:
        warnings.append('Chưa kiểm tra được glyph font: '+str(exc)[:180])
    anchors=project.get('anchors',[])
    beat_duration=float((project.get('beat') or {}).get('duration') or 0)
    reference_duration=float((project.get('reference') or {}).get('duration') or 0)
    import scene_tools
    scene_sections={word.get('section_name') for word in words if word.get('section_name')}
    for scene in scene_tools.validate(project.get('scenes',[])):
        if scene['section_name'] in scene_sections and not (directory/scene['media_name']).is_file():
            warnings.append(f"Cảnh {scene['section_name']} chưa có ảnh; sẽ dùng nền chung.")
    scene_manifest=[{key:value for key,value in scene.items()
                     if key not in ('image_path','decoration_path')}
                    for scene in scene_tools.timeline(project,beat_duration,directory)]
    for index,word in enumerate(words):
        start=float(word.get('start',0));end=float(word.get('end',start));duration=end-start
        if word.get('estimated'):
            key=word.get('line',0)
            if key not in estimated_by_line:estimated_by_line[key]=[0,index,word]
            estimated_by_line[key][0]+=1
        if duration<.07:check('Từ có mốc quá ngắn để đọc rõ.',index,word)
        if duration>2.5:check('Từ kéo dài trên 2,5 giây; hãy nghe lại đoạn này.',index,word)
        beat_start=map_time(start,anchors);beat_end=map_time(end,anchors)
        if reference_duration and (start<-.05 or end>reference_duration+.1):
            check('Mốc từ vượt quá thời lượng bản hát tham chiếu.',index,word)
        if beat_duration and (beat_start<-.05 or beat_start>beat_duration+.05 or beat_end>beat_duration+.1):
            check('Mốc từ vượt quá thời lượng beat.',index,word)
        previous=previous_by_line.get(word.get('line',0))
        if previous:
            prev_index,prev=previous
            if start<prev.get('start',0)-.03:
                check('Thứ tự thời gian của lời bị đảo.',index,word)
            elif start<prev.get('end',0)-.08:
                check('Mốc từ này chồng lên từ ngay trước.',index,word)
            if word.get('line')==prev.get('line') and start-prev.get('end',0)>4.5:
                check('Khoảng nghỉ trong cùng câu dài hơn 4,5 giây.',index,word)
        previous_by_line[word.get('line',0)]=(index,word)
    line_groups=[]
    for line in lyric_lines(words):
        if line:
            line_groups.append((min(word['start'] for word in line),max(word['end'] for word in line),line))
    line_groups.sort(key=lambda item:item[0])
    seen_overlaps=set()
    for current_index,(start,end,line) in enumerate(line_groups):
        for other_start,other_end,other in line_groups[:current_index]:
            overlap=min(end,other_end)-max(start,other_start)
            if overlap<=.12:
                continue
            roles_a={word.get('role','all') for word in line}
            roles_b={word.get('role','all') for word in other}
            # Overlap between two explicitly assigned, distinct singers is intentional.
            # Shared or unknown roles remain ambiguous and should still be reviewed.
            distinct_singers=(roles_a and roles_b and 'all' not in roles_a|roles_b and
                              'unknown' not in roles_a|roles_b and roles_a.isdisjoint(roles_b))
            if distinct_singers:
                continue
            pair=tuple(sorted((line[0].get('line_uid',line[0].get('line',0)),
                               other[0].get('line_uid',other[0].get('line',0)))))
            if pair in seen_overlaps:
                continue
            seen_overlaps.add(pair)
            later=next((index for index,word in enumerate(words) if word.get('id')==line[0].get('id')),0)
            check('Hai câu lời chưa gán vai chồng thời gian; kiểm tra bè hoặc mốc bị lệch.',later,line[0])
    for line_id,(count,index,word) in estimated_by_line.items():
        check(f'{count} từ trong câu đang có mốc ước lượng.',index,word)
    plan=layout.display_plan(project)
    for row in plan['rows']:
        if row['font_size']<s['font_size']*.8:
            index=int(row['words'][0].get('source_index',0))
            word=words[index] if index<len(words) else {'line':row.get('line',0),'start':row['start']}
            check(f"Câu phải thu nhỏ chữ còn {round(row['font_size']/s['font_size']*100)}% để vừa khung.",index,word)
    if kind=='color':
        foregrounds={'chữ đã hát':s['sung'],'chữ chưa hát':s['unsung']}
        if s['singing_mode']=='duet':
            foregrounds.update({'vai '+s['role_a_name']:s['role_a_color'],
                                'vai '+s['role_b_name']:s['role_b_color'],
                                s['role_c_name']:s['role_c_color'],
                                s['role_d_name']:s['role_d_color'],
                                s['role_e_name']:s['role_e_color'],
                                s['role_f_name']:s['role_f_color'],
                                s['role_unknown_name']:s['role_unknown_color'],
                                'cùng hát':s['role_all_color']})
        for label,color in foregrounds.items():
            ratio=_contrast_ratio(color,s['background'])
            if ratio<4.5:
                warnings.append(f"Độ tương phản {label} / nền màu chỉ {ratio:.1f}:1; cân nhắc tăng độ tương phản.")
    if s['logo_enabled'] and project.get('logo_name') and (directory/'logo.png').is_file() and s['logo_opacity']>10:
        try:
            dimensions=project.get('logo_image') or {}
            ratio=float(dimensions.get('width',0))/max(1,float(dimensions.get('height',0)))
            if ratio<=0:
                from PIL import Image
                with Image.open(directory/'logo.png') as logo:
                    ratio=logo.width/max(1,logo.height)
            logo_width=max(2,round(min(plan['width']*s['logo_size']/100,plan['height']*.4*ratio)))
            logo_height=max(2,round(logo_width/ratio))
            logo_x=(plan['width']-logo_width)*s['logo_x']/100
            logo_y=(plan['height']-logo_height)*s['logo_y']/100
            collisions=set()
            for row in plan['rows']:
                if (row['x']<logo_x+logo_width and row['x']+row['width']>logo_x and
                        row['y']<logo_y+logo_height and row['y']+row['height']>logo_y):
                    word=row['words'][0]
                    line_id=word.get('line_uid',word.get('line',0))
                    if line_id in collisions:
                        continue
                    collisions.add(line_id)
                    index=int(word.get('source_index',0))
                    source_word=words[index] if index<len(words) else word
                    check('Logo chồng lên vùng lời; đổi vị trí logo hoặc vị trí câu.',index,source_word)
            if len(collisions)>30:
                warnings.append(f'Logo chồng lên {len(collisions)} câu; xem các điểm QA và đổi vị trí logo.')
        except Exception as exc:
            warnings.append('Chưa kiểm tra được va chạm logo / lời: '+str(exc)[:160])
    folder=project.get('export_directory','')
    if folder and (not Path(folder).is_absolute() or not Path(folder).is_dir()):errors.append('Thư mục xuất không còn tồn tại. Hãy chọn lại.')
    audio_quality=None
    beat_file=directory/'beat.wav'
    if beat_file.is_file():
        try:
            stat=beat_file.stat()
            audio_quality=_audio_metrics(str(beat_file.resolve()),stat.st_mtime_ns,stat.st_size)
            if audio_quality['clipped_samples']:
                warnings.append(f"Beat có {audio_quality['clipped_samples']} mẫu gần clipping; hãy nghe lại đoạn đó.")
            elif audio_quality['peak_dbfs']>-.5:
                warnings.append('Peak beat dưới 0,5 dB so với ngưỡng clipping; nên kiểm tra âm lượng.')
        except Exception as exc:
            warnings.append('Chưa đo được peak/RMS của beat: '+str(exc)[:180])
    audio_label='Beat gốc'
    if s['audio_output']=='guide':audio_label='Beat + vocal hướng dẫn'
    return {'project':project.get('name',''),'output':{'width':width,'height':height,'fps':round(fps,5),'format':s['render_format'].upper()+' · H.264','audio':audio_label+' · AAC · 320 kbps'+(' · chuẩn hóa gần −16 LUFS' if s['audio_normalize'] else ''),'quality':s['render_quality']},
            'folder':folder or 'Thư mục exports của project (mặc định)',
            'duration':(project.get('beat') or {}).get('duration',0),'background':{'kind':kind,'name':path.name if path else s['background'],'source':source},
            'scenes':scene_manifest,
            'words':len(words),'lines':len({w['line'] for w in words}),
            'logo':bool(s['logo_enabled'] and project.get('logo_name')),'settings':s,'warnings':warnings,'errors':errors,
            'checks':checks,'audio_quality':audio_quality,
            'audio_source':(project.get('beat') or {}).get('name'),'excluded':(['Giọng hát tham chiếu'] if s['audio_output']=='beat' else [])+['Âm thanh video nền','Màu nhãn timeline','Bố cục màn hình chỉnh sửa']}
