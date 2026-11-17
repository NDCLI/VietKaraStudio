"""Duet markers are visual decoration, never timed lyric tokens."""
import copy
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import engine
from layout import display_plan, split_text
from styles import ANIMATIONS, validate_settings
from subtitles import build_ass, stamp


def project(settings=None):
    return {'settings':{'singing_mode':'duet', 'countdown':'none', **(settings or {})},
            'words':[{'text':text, 'start':i*3+2, 'end':i*3+3, 'line':i, 'role':role}
                     for i,(text,role) in enumerate([('Nam','male'),('Nữ','female'),('Chung','all')])]}


def test_duet_markers_reserve_space_and_share_row_lifetime_without_changing_lyrics():
    p=project({'align':'center', 'horizontal_scale':150})
    before=copy.deepcopy(p)
    plan=display_plan(p)
    assert [r['role_icon']['role'] for r in plan['rows']]==['male','female','all']
    assert len({r['role_icon']['path'] for r in plan['rows']})==3
    for row in plan['rows']:
        marker=row['role_icon'];word=row['words'][0]
        assert marker['x']==row['x']
        assert marker['x']+marker['size']<word['x']
        assert row['x']+row['width']/2==pytest.approx(plan['width']/2,abs=.01)
        assert word['x']+word['width']==pytest.approx(row['x']+row['width'],abs=.01)
        assert marker['label']
    text_only=project({'role_icons':False, 'horizontal_scale':150})
    assert engine.export_text(p,'srt')==engine.export_text(text_only,'srt')
    assert engine.export_text(p,'lrc')==engine.export_text(text_only,'lrc')
    assert p==before


@pytest.mark.parametrize('settings',[
    {'singing_mode':'solo'}, {'role_icons':False},
    {'display_mode':'lyricsmv'}, {'display_mode':'subtitle'},
])
def test_markers_only_appear_in_enabled_duet_karaoke(settings):
    p=project({'subtitle_box':False, **settings})
    assert all(r['role_icon'] is None for r in display_plan(p)['rows'])
    assert '\\p1' not in build_ass(p)


def test_wrapping_and_single_long_word_leave_room_for_marker():
    p=project({'aspect':'portrait','font_size':240,'align':'right','italic':True})
    p['words']=[{'text':'Rộng', 'start':i*.5+1, 'end':i*.5+1.4, 'line':0, 'role':'female'}
                for i in range(16)]
    p['words'].append({'text':'W'*100,'start':12,'end':13,'line':1,'role':'male'})
    before=copy.deepcopy(p)
    plan=display_plan(p)
    assert len(plan['rows'])>2
    markers=[row['role_icon'] for row in plan['rows'] if row['role_icon']]
    assert [marker['role'] for marker in markers]==['female','male']
    for row in plan['rows']:
        marker=row['role_icon']
        if marker:
            assert marker['x']>=0
            assert marker['x']+marker['size']<row['words'][0]['x']
        assert row['x']+row['width']<=plan['width']*.98+.01
        assert all(w['x']+w['width']<=row['x']+row['width']+.01 for w in row['words'])
    assert [w['text'] for row in plan['rows'] for w in row['words']]==[w['text'] for w in p['words']]
    split=split_text(' '.join(w['text'] for w in p['words']),p['settings'])
    assert split['text'].split()==[w['text'] for w in p['words']]
    assert p==before


@pytest.mark.parametrize('mode',ANIMATIONS)
def test_ass_artwork_layers_share_line_times_and_preserve_word_sweeps(mode):
    p=project({'line_animation':mode,'horizontal_scale':150,'sweep_feather':8})
    plan=display_plan(p)
    content=build_ass(p)
    events=[line for line in content.splitlines() if line.startswith('Dialogue:')]
    markers=[line for line in events if '\\p1' in line]
    assert len(markers)>len(plan['rows'])
    for row in plan['rows']:
        layers=[marker for marker in markers if marker.split(',',9)[1:3]==[stamp(row['show_start']),stamp(row['show_end'])]]
        assert len(layers)>1
        for marker in layers:
            assert '}m ' in marker and ' l ' in marker
            assert '\\fscx100\\fscy100' in marker
            assert '\\bord0\\shad0' in marker
    assert '\\clip(' in content and '\\t(' in content
    assert all(any(line.endswith('}'+word['text']) for line in events) for word in p['words'])


def test_role_icons_defaults_on_but_only_accepts_boolean():
    assert validate_settings({})['role_icons'] is True
    assert validate_settings({'role_icons':False})['role_icons'] is False
    with pytest.raises(ValueError):
        validate_settings({'role_icons':'false'})


def test_role_artwork_tint_matches_sung_color_preserving_alpha_and_border():
    from bundled_assets import role_icon_image
    for filename in ['man-new-blue.png','lady-pink.png','men(blue) & lady(pink).png']:
        original=role_icon_image(filename)
        tinted=role_icon_image(filename,'#12c45a')
        assert tinted.size==original.size
        assert tinted.getchannel('A').tobytes()==original.getchannel('A').tobytes()
        source=original.load();result=tinted.load();found=False
        for y in range(original.height):
            for x in range(original.width):
                if source[x,y][:3] in [(255,255,255),(0,0,0)]:
                    assert result[x,y]==source[x,y]
                if result[x,y]==(18,196,90,255):found=True
        assert found


def test_role_plan_and_ass_use_current_role_sung_colors():
    from bundled_assets import icon_drawing
    colors=['#18c65a','#f38a26','#9944ee']
    p=project(dict(zip(['role_a_color','role_b_color','role_all_color'],colors)))
    plan=display_plan(p)
    content=build_ass(p)
    for row,color in zip(plan['rows'],colors):
        marker=row['role_icon']
        assert marker['color']==row['color']==color
        assert marker['url'].endswith('?color='+color[1:])
        layers=icon_drawing(marker['asset'],marker['size'],color)
        assert any(max(abs(r-int(color[1:3],16)),abs(g-int(color[3:5],16)),abs(b-int(color[5:7],16)))<10
                   for (r,g,b,a),shape in layers if a==255)
        assert marker['label'] and '\\p1' in content


def test_role_icon_png_endpoint_validates_asset_and_color():
    import app as service
    from io import BytesIO
    from PIL import Image
    with service.app.test_client() as client:
        response=client.get('/api/assets/source/ICON/man-new-blue.png?color=12c45a')
        assert response.status_code==200 and response.mimetype=='image/png'
        with Image.open(BytesIO(response.data)) as image:
            assert image.mode=='RGBA'
        assert client.get('/api/assets/source/ICON/man-new-blue.png?color=oops').status_code==400
        assert client.get('/api/assets/source/ICON/unknown.png?color=12c45a').status_code==400
