"""ASS generation for karaoke, lyrics MV, subtitles and timed vector countdowns."""
import math

from layout import display_plan
from styles import validate_settings


def stamp(seconds):
    n = max(0, round(seconds * 100))
    return f'{n//360000}:{n//6000%60:02d}:{n//100%60:02d}.{n%100:02d}'


def color(hex_value, opacity=100):
    alpha = round(255 * (1-max(0,min(100,opacity))/100))
    return f'&H{alpha:02X}{hex_value[5:7]}{hex_value[3:5]}{hex_value[1:3]}'


def literal(text):
    return str(text).replace('\\','＼').replace('{','｛').replace('}','｝').replace('\n',' ')


def blend(a, b, p):
    p = max(0,min(1,p))
    return '#'+''.join(f'{round(int(a[i:i+2],16)*(1-p)+int(b[i:i+2],16)*p):02x}' for i in (1,3,5))


def animation(row, opts):
    duration = max(1, round((row['show_end']-row['show_start'])*1000))
    ms = min(600, duration//3)
    x,y,size = row['x'],row['y'],row['font_size']
    mode = opts['line_animation']
    pos = f'\\an7\\pos({x:.2f},{y:.2f})'
    fade_in = ms if mode != 'fadesec' or row['section_start'] else 0
    fade_out = ms if mode != 'fadesec' or row['section_end'] else 0
    if mode in ('none',): return pos
    if mode in ('slowfade','fadesec'): return pos+f'\\fad({fade_in},{fade_out})'
    sx = opts['horizontal_scale']
    if mode in ('zoom','zoomout','popbounce','punchzoom'):
        initial = {'zoom':65,'zoomout':135,'popbounce':70,'punchzoom':135}[mode]
        mid = 112 if mode == 'popbounce' else 100
        tags = f'\\fscx{sx*initial/100:.2f}\\fscy{initial}\\t(0,{ms//2},\\fscx{sx*mid/100:.2f}\\fscy{mid})\\t({ms//2},{ms},\\fscx{sx}\\fscy100)'
        tags += f'\\t({duration-ms},{duration},\\fscx{sx*initial/100:.2f}\\fscy{initial})'
        return pos+tags+f'\\fad({ms//2},{ms//2})'
    if mode == 'flipup':
        return pos+f'\\frx70\\t(0,{ms},\\frx0)\\t({duration-ms},{duration},\\frx70)\\fad({ms},{ms})'
    # Use a continuous movement with a short enter and reverse exit in separate events.
    dx = size * (1 if row['align'] != 'right' else -1) if mode == 'split' else 0
    dy = -size if mode == 'slidedown' else size if mode == 'slideup' else 0
    return f'\\an7\\move({x+dx:.2f},{y+dy:.2f},{x:.2f},{y:.2f},0,{ms})\\fad({ms},{ms})'


def rounded_box(x, y, w, h, radius):
    r = min(radius, w/2, h/2)
    k = r*.552285
    return (f'm {x+r:.2f} {y:.2f} l {x+w-r:.2f} {y:.2f} '
            f'b {x+w-r+k:.2f} {y:.2f} {x+w:.2f} {y+r-k:.2f} {x+w:.2f} {y+r:.2f} '
            f'l {x+w:.2f} {y+h-r:.2f} b {x+w:.2f} {y+h-r+k:.2f} {x+w-r+k:.2f} {y+h:.2f} {x+w-r:.2f} {y+h:.2f} '
            f'l {x+r:.2f} {y+h:.2f} b {x+r-k:.2f} {y+h:.2f} {x:.2f} {y+h-r+k:.2f} {x:.2f} {y+h-r:.2f} '
            f'l {x:.2f} {y+r:.2f} b {x:.2f} {y+r-k:.2f} {x+r-k:.2f} {y:.2f} {x+r:.2f} {y:.2f}')


ICONS = {
 'mic':'m 8 3 b 8 0 16 0 16 3 l 16 11 b 16 16 8 16 8 11 l 8 3 m 5 10 l 6 10 b 6 20 18 20 18 10 l 19 10 b 19 17 15 20 13 20 l 13 23 18 23 18 25 6 25 6 23 11 23 11 20 b 7 20 5 16 5 10',
 'star':'m 12 0 l 15 8 24 9 17 15 19 24 12 19 5 24 7 15 0 9 9 8 12 0',
 'heart':'m 12 23 b -7 11 0 -5 12 5 b 24 -5 31 11 12 23',
 'note':'m 9 3 l 22 0 22 18 b 20 24 12 22 15 17 b 16 15 19 15 20 15 l 20 6 11 9 11 21 b 8 27 0 24 3 20 b 4 18 7 18 9 18 l 9 3',
 'moon':'m 20 1 b 1 -4 -7 22 13 25 b 20 26 25 22 26 18 b 8 23 4 5 20 1',
}


def build_ass(project):
    opts = validate_settings(project.get('settings', {}))
    plan = display_plan(project)
    width,height = plan['width'],plan['height']
    font = plan['font'].replace(',','')
    s = opts
    lines = ['[Script Info]','ScriptType: v4.00+',f'PlayResX: {width}',f'PlayResY: {height}',
             'WrapStyle: 2','ScaledBorderAndShadow: yes','','[V4+ Styles]',
             'Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding',
             f"Style: Main,{font},{s['font_size']},{color(s['sung'])},{color(s['unsung'])},{color(s['outline_unsung'],s['outline_unsung_opacity'])},{color(s['shadow_color'])},-1,{-1 if s['italic'] else 0},0,0,{s['horizontal_scale']},100,0,0,1,{s['outline_width']},{s['shadow']},7,0,0,0,1",
             '', '[Events]', 'Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text']
    def event(layer, start, end, tags, text):
        if end <= start: return
        lines.append(f'Dialogue: {layer},{stamp(start)},{stamp(end)},Main,,0,0,0,,{{{tags}}}{text}')
    def paint(hex_color, outline, opacity=100, shadow=True):
        return (f'\\1c{color(hex_color)}\\1a&H{round(255*(1-opacity/100)):02X}&'
                f"\\3c{color(s['outline_'+outline])}\\3a&H{round(255*(1-s['outline_'+outline+'_opacity']*opacity/10000)):02X}&"
                + ('' if shadow else '\\shad0'))
    def gradient_colors(row):
        if not s['sung_gradient'] or row['role'] != 'all': return [(0,height,row['color'])]
        n=10
        top=row['y']-row['font_size']*.12
        bottom=row['y']+row['height']
        bands=[]
        for j in range(n):
            frac=(j+.5)/n
            # The stop controls are expressed from bottom (0) to top (100), as in VCL.
            p=(100-frac*100-s['gradient_start'])/max(1,s['gradient_end']-s['gradient_start'])
            bands.append((max(0,top+(bottom-top)*j/n), min(height,top+(bottom-top)*(j+1)/n+.5), blend(s['gradient_bottom'],s['gradient_top'],p)))
        bands[0]=(0,bands[0][1],bands[0][2]);bands[-1]=(bands[-1][0],height,bands[-1][2])
        return bands
    def plain(row, hex_color, opacity=100, gradient=False):
        ds,de=row['show_start'],row['show_end']
        tags=animation(row,s)+f"\\fs{row['font_size']:.3f}"
        if gradient and s['sung_gradient']:
            for y1,y2,hex_color in gradient_colors(row):
                event(2,ds,de,tags+paint(hex_color,'unsung',opacity)+f'\\clip(0,{y1:.2f},{width},{y2:.2f})',literal(row['text']))
        else:
            event(2,ds,de,tags+paint(hex_color,'unsung',opacity),literal(row['text']))
    rows=plan['rows']
    for i,row in enumerate(rows):
        ds,de=row['show_start'],row['show_end']
        if s['display_mode']=='subtitle':
            if s['subtitle_box']:
                pad=row['font_size']*.18
                shape=rounded_box(0,0,row['width']+2*pad,row['height']+2*pad,s['subtitle_box_radius'])
                box_row={**row,'x':row['x']-pad,'y':row['y']-pad}
                event(0,ds,de,'\\fscx100\\fscy100'+animation(box_row,s)+f"\\p1\\bord0\\shad0\\1c{color(s['subtitle_box_color'])}\\1a&H{round(255*(1-s['subtitle_box_opacity']/100)):02X}&",shape)
            plain(row,s['unsung'],gradient=True)
            continue
        if s['display_mode']=='lyricsmv':
            plain(row,row['color'],gradient=True)
            if s['mv_style']!='single':
                count=2 if s['mv_style']=='scroll' else 1
                for delta in range(-count,count+1):
                    if not delta or not 0<=i+delta<len(rows): continue
                    neighbor=rows[i+delta]
                    scale=.7
                    w=neighbor['width']*scale
                    x=(width-w)/2 if row['align']=='center' else row['x'] if row['align']=='left' else row['x']+row['width']-w
                    ghost={**row,'text':neighbor['text'],'font_size':neighbor['font_size']*scale,
                           'x':x,'y':row['y']+delta*row['font_size']*s['line_height'], 'section_start':False, 'section_end':False}
                    plain(ghost,s['unsung'],max(20,65-abs(delta)*15))
            continue
        for word in row['words']:
            a=max(0,round((word['start']-ds)*1000)); b=max(a+10,round((word['end']-ds)*1000))
            margin=s['outline_width']+row['font_size']*.18+s['shadow']+3
            left=word['x']-margin;right=word['x']+word['width']+margin
            feather=min(s['sweep_feather']/3,max(0,word['width']*.25))
            # Animate rectangular clips; this keeps sweep duration on the word clock.
            def clip(x1,y1,x2,y2): return f'\\clip({x1:.2f},{y1:.2f},{x2:.2f},{y2:.2f})'
            def moving(x1,y1,x2,y2,ex1,ex2):
                return clip(x1,y1,x2,y2)+f'\\t({a},{b},'+clip(ex1,y1,ex2,y2)+')'
            wr={**row,'x':word['x']}
            motion=animation(wr,s)+f"\\fs{row['font_size']:.3f}"
            base=motion+paint(s['unsung'],'unsung')+moving(left+feather/2,0,right+feather,height,right+feather/2,right+feather)
            event(1,ds,de,base,literal(word['text']))
            for y1,y2,hex_color in gradient_colors(row):
                main=motion+paint(hex_color,'sung',shadow=False)+moving(left,y1,left,y2,left,right-feather/2)
                event(3,ds,de,main,literal(word['text']))
                if feather:
                    for j in range(4):
                        offset=-feather/2+feather*j/4
                        alpha=100*(1-(j+.5)/4)
                        strip=motion+paint(hex_color,'sung',alpha,shadow=False)
                        strip+=moving(left+offset,y1,left+offset+feather/4,y2,right+offset,right+offset+feather/4)
                        event(4,ds,de,strip,literal(word['text']))
            if s['sweep_glow']:
                # A blurred white copy is visible only around the moving edge and inside this word's interval.
                band=max(2,min(word['width']*.2,feather+row['font_size']*.07))
                duration=max(10,round((word['end']-word['start'])*1000))
                tags=(f"\\an7\\pos({word['x']:.2f},{row['y']:.2f})\\fs{row['font_size']:.3f}\\bord0\\shad0\\1c&HFFFFFF&"
                      f"\\1a&H{round(255*(1-min(1,s['sweep_glow']/150))):02X}&\\blur{1+s['sweep_feather']/12+s['sweep_glow']/100:.2f}"
                      +clip(left-band,0,left+band,height)+f'\\t(0,{duration},'+clip(right-band,0,right+band,height)+')')
                event(5,max(ds,word['start']),min(de,word['end']),tags,literal(word['text']))
    if s['display_mode']=='karaoke' and s['countdown'] in ICONS:
        for countdown in plan['countdowns']:
            for j in range(4):
                start,end=countdown['start'],countdown['end']-j
                if end<=start:continue
                scale=countdown['size']/25*100
                x=countdown['x']+j*countdown['size']*1.28
                tags=(f"\\an7\\pos({x:.2f},{countdown['y']:.2f})\\p1\\fscx{scale:.2f}\\fscy{scale:.2f}\\bord.5\\shad0\\1c{color(s['sung'])}\\3c{color(s['outline_unsung'])}\\fad(120,100)")
                event(6,start,end,tags,ICONS[s['countdown']])
    return '\n'.join(lines)+'\n'
