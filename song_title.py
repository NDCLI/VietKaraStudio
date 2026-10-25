"""Measured song title geometry and independent entrance/exit motion for ASS/preview."""
from styles import validate_settings

TITLE_TEMPLATE_STYLES = {
    'gradient': {'colors':['#762ee5','#e83e86','#ffab51'],'opacity':96,'border':'#ffe8f2','border_width':0,'radius':14,'pad_x':.42,'pad_y':.24,'align':'left'},
    'midnight': {'colors':['#101a2b','#1c2b42'],'opacity':95,'border':'#e8c568','border_width':2,'radius':8,'pad_x':.42,'pad_y':.25,'align':'left'},
    'crystal': {'colors':['#183144','#476d82'],'opacity':82,'border':'#b8e7f5','border_width':1.5,'radius':18,'pad_x':.4,'pad_y':.28,'align':'left'},
    'neon': {'colors':['#10112b','#301446'],'opacity':96,'border':'#48e9dc','border_width':2,'radius':7,'pad_x':.42,'pad_y':.25,'align':'left'},
    'retro': {'colors':['#321b18','#73401f'],'opacity':96,'border':'#e5aa58','border_width':2,'radius':2,'pad_x':.42,'pad_y':.25,'align':'left'},
    'pastel': {'colors':['#71395e','#554d83'],'opacity':94,'border':'#ffc7e1','border_width':1.5,'radius':16,'pad_x':.4,'pad_y':.26,'align':'left'},
}


def title_plan(project):
    from layout import Measure, frame_size
    s=validate_settings(project.get('settings',{}))
    if not s['title_enabled'] or not s['title_text']:return None
    width,height=frame_size(s)
    end=min(s['title_end'],(project.get('beat') or {}).get('duration',s['title_end']))
    if end<=s['title_start']:return None
    rows=[]
    preview_rows=[]
    template=TITLE_TEMPLATE_STYLES.get(s['title_template'])
    y=height*s['title_y']/100
    for text,factor in ((s['title_text'],1),(s['title_credit'],.43)):
        if not text:continue
        opts={**s,'font':s['title_font'],'font_size':s['title_size']*factor,'italic':s['title_italic'],'horizontal_scale':100}
        measure=Measure(opts);size=opts['font_size']
        measured=measure.width(text)
        preview_width=measured/max(.01,measure.factor)
        rows.append(dict(text=text,x=0,y=y,width=measured,size=size))
        preview_rows.append(dict(text=text,x=0,y=y,width=preview_width,size=size))
        y+=size*1.45
    initial_shrink=min(1,width*.86/max(1,max(row['width'] for row in preview_rows)))
    if initial_shrink<1:
        first_y=rows[0]['y']
        for row_set in (rows,preview_rows):
            for row in row_set:
                row['size']*=initial_shrink
                row['width']*=initial_shrink
                row['y']=first_y+(row['y']-first_y)*initial_shrink
    if template:
        paint_x=s['title_outline_width']+s['title_shadow']*2.5+template['border_width']+3
        paint_y=s['title_outline_width']+s['title_shadow']*2.5+template['border_width']+3
        main_size=rows[0]['size']
        pad_x=max(main_size*template['pad_x'],paint_x+main_size*.04)
        pad_y=max(main_size*template['pad_y'],paint_y+main_size*.04)
        preview_content=max(row['width'] for row in preview_rows)
        shrink=min(1,(width*.94-2*pad_x)/max(1,preview_content*1.02))
        if shrink<1:
            first_y=rows[0]['y']
            for row_set in (rows,preview_rows):
                for row in row_set:
                    row['size']*=shrink
                    row['width']*=shrink
                    row['y']=first_y+(row['y']-first_y)*shrink
            main_size=rows[0]['size']
            pad_x=max(main_size*template['pad_x'],paint_x+main_size*.04)
            pad_y=max(main_size*template['pad_y'],paint_y+main_size*.04)
        box_height=(rows[-1]['y']+rows[-1]['size']*1.2-rows[0]['y'])+2*pad_y
        box_y=max(height*.02,min(height*.98-box_height,rows[0]['y']-pad_y))

        def place(rows_to_place, width_factor):
            content_width=max(row['width'] for row in rows_to_place)
            safe_width=content_width*width_factor
            box_width=min(width*.94,safe_width+2*pad_x)
            text_inset=pad_x+(safe_width-content_width)/2
            box_x=max(width*.02,min(width*.98-box_width,width*s['title_x']/100-box_width/2))
            placed=[]
            for row in rows_to_place:
                placed.append({**row,'x':box_x+text_inset,
                               'y':row['y']+box_y+pad_y-rows_to_place[0]['y']})
            return placed,{'x':box_x,'y':box_y,'width':box_width,'height':box_height,**template}

        rows,box=place(rows,1)
        preview_rows,preview_box=place(preview_rows,1.02)
    else:
        content_width=max(row['width'] for row in rows)
        block_x=max(width*.02,min(width*.98-content_width,width*s['title_x']/100-content_width/2))
        rows=[{**row,'x':block_x} for row in rows]
        preview_content=max(row['width'] for row in preview_rows)
        preview_x=max(width*.02,min(width*.98-preview_content,width*s['title_x']/100-preview_content/2))
        preview_rows=[{**row,'x':preview_x} for row in preview_rows]
        overflow=max(0,rows[-1]['y']+rows[-1]['size']*1.2-height*.95)
        rows=[{**row,'y':row['y']-overflow} for row in rows]
        preview_rows=[{**row,'y':row['y']-overflow} for row in preview_rows]
        box=None
        preview_box=None
    return dict(start=s['title_start'],end=end,effect_seconds=min(s['title_effect_seconds'],(end-s['title_start'])/2),
                enter=s['title_enter'],exit=s['title_exit'],rows=rows,color=s['title_color'],outline=s['title_outline'],
                outline_width=s['title_outline_width'],shadow=s['title_shadow'],shadow_color=s['title_shadow_color'],
                weight=s['title_weight'],italic=s['title_italic'],font=measure.family,box=box,
                preview_rows=preview_rows,preview_box=preview_box,template=s['title_template'])


def ass_motion(row,plan):
    duration=round((plan['end']-plan['start'])*1000);ms=max(1,round(plan['effect_seconds']*1000))
    x,y=row['x'],row['y'];d=row['size']*.65
    offsets={'slide_up':(0,d),'slide_down':(0,-d),'slide_left':(d,0),'slide_right':(-d,0)}
    def state(mode):
        dx,dy=offsets.get(mode,(0,0))
        return dx,dy,65 if mode=='zoom' else 100,75 if mode=='flip' else 0
    enter,exit=plan['enter'],plan['exit']
    dx,dy,scale,rx=state(enter)
    start=f'\\an7\\pos({x:.3f},{y:.3f})'
    if dx or dy:start=f'\\an7\\move({x+dx:.3f},{y+dy:.3f},{x:.3f},{y:.3f},0,{ms})'
    start+=f'\\fscx{scale}\\fscy{scale}\\frx{rx}\\t(0,{ms},\\fscx100\\fscy100\\frx0)'
    start+=f'\\fad({0 if enter=="none" else ms},{0 if exit=="none" else ms})'
    # Pos/move cannot transition twice in one ASS event: split at exit boundary.
    dx2,dy2,scale2,rx2=state(exit)
    leave=(f'\\an7\\move({x:.3f},{y:.3f},{x-dx2:.3f},{y-dy2:.3f},0,{ms})'
           if dx2 or dy2 else f'\\an7\\pos({x:.3f},{y:.3f})')
    leave+=f'\\fscx100\\fscy100\\frx0\\t(0,{ms},\\fscx{scale2}\\fscy{scale2}\\frx{rx2})'
    leave+=f'\\fad(0,{0 if exit=="none" else ms})'
    # Fade out belongs exclusively to exit event, otherwise it begins too early.
    start=start.replace(f'\\fad({0 if enter=="none" else ms},{0 if exit=="none" else ms})',f'\\fad({0 if enter=="none" else ms},0)')
    return start,leave,plan['end']-ms/1000
