"""Export settings and assets, shared by the dialog and saved render manifest."""
from fractions import Fraction
import json

import media_tools
from styles import validate_settings


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
    if not project.get('words'):errors.append('Chưa có lời để xuất.')
    if s['logo_enabled'] and project.get('logo_name') and not (directory/'logo.png').is_file():errors.append('Thiếu file logo. Hãy nạp lại ảnh logo.')
    if s['countdown']=='image' and not (directory/'countdown.png').is_file():errors.append('Thiếu ảnh đếm ngược.')
    words=project.get('words',[])
    estimated=sum(bool(w.get('estimated')) for w in words)
    if estimated:warnings.append(f'{estimated} từ đang có thời gian ước lượng; nên nghe kiểm tra trước khi xuất.')
    checks=[]
    def check(message,word_index,word,severity='warning'):
        checks.append({'severity':severity,'message':message,'word_index':word_index,
                       'line':word.get('line',0),'time':float(word.get('start',0))})
    estimated_by_line={}
    previous_by_line={}
    from timing import map_time
    anchors=project.get('anchors',[])
    beat_duration=float((project.get('beat') or {}).get('duration') or 0)
    reference_duration=float((project.get('reference') or {}).get('duration') or 0)
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
    for line_id,(count,index,word) in estimated_by_line.items():
        check(f'{count} từ trong câu đang có mốc ước lượng.',index,word)
    from pathlib import Path
    folder=project.get('export_directory','')
    if folder and (not Path(folder).is_absolute() or not Path(folder).is_dir()):errors.append('Thư mục xuất không còn tồn tại. Hãy chọn lại.')
    return {'project':project.get('name',''),'output':{'width':width,'height':height,'fps':round(fps,5),'format':s['render_format'].upper()+' · H.264','audio':'AAC · 320 kbps','quality':s['render_quality']},
            'folder':folder or 'Thư mục exports của project (mặc định)',
            'duration':(project.get('beat') or {}).get('duration',0),'background':{'kind':kind,'name':path.name if path else s['background'],'source':source},
            'words':len(words),'lines':len({w['line'] for w in words}),
            'logo':bool(s['logo_enabled'] and project.get('logo_name')),'settings':s,'warnings':warnings,'errors':errors,
            'checks':checks,
            'audio_source':(project.get('beat') or {}).get('name'),'excluded':['Giọng hát tham chiếu','Âm thanh video nền','Màu nhãn timeline','Bố cục màn hình chỉnh sửa']}
