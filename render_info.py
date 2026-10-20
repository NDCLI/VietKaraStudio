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
    estimated=sum(bool(w.get('estimated')) for w in project.get('words',[]))
    if estimated:warnings.append(f'{estimated} từ đang có thời gian ước lượng; nên nghe kiểm tra trước khi xuất.')
    from pathlib import Path
    folder=project.get('export_directory','')
    if folder and (not Path(folder).is_absolute() or not Path(folder).is_dir()):errors.append('Thư mục xuất không còn tồn tại. Hãy chọn lại.')
    return {'project':project.get('name',''),'output':{'width':width,'height':height,'fps':round(fps,5),'format':s['render_format'].upper()+' · H.264','audio':'AAC · 320 kbps','quality':s['render_quality']},
            'folder':folder or 'Thư mục exports của project (mặc định)',
            'duration':(project.get('beat') or {}).get('duration',0),'background':{'kind':kind,'name':path.name if path else s['background'],'source':source},
            'words':len(project.get('words',[])),'lines':len({w['line'] for w in project.get('words',[])}),
            'logo':bool(s['logo_enabled'] and project.get('logo_name')),'settings':s,'warnings':warnings,'errors':errors,
            'audio_source':(project.get('beat') or {}).get('name'),'excluded':['Giọng hát tham chiếu','Âm thanh video nền','Màu nhãn timeline','Bố cục màn hình chỉnh sửa']}
