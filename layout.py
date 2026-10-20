"""One measured lyric layout shared by preview, auto-split and ASS export."""
from collections import OrderedDict
from functools import lru_cache
import os
from pathlib import Path
import re
import struct

from PIL import ImageFont
from fontTools.ttLib import TTFont

from styles import validate_settings
from timing import beat_words, lyric_lines


@lru_cache(maxsize=1)
def font_catalog():
    result = []
    if os.name == "nt":
        import winreg
        roots = [Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts",
                 Path(os.environ.get("LOCALAPPDATA", "")) / "Microsoft/Windows/Fonts"]
        for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
            try:
                with winreg.OpenKey(hive, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts") as key:
                    for i in range(winreg.QueryInfoKey(key)[1]):
                        name, filename, _ = winreg.EnumValue(key, i)
                        file = Path(str(filename))
                        paths = [file] if file.is_absolute() else [root / file for root in roots]
                        file = next((p for p in paths if p.is_file()), None)
                        if file and file.suffix.lower() in (".ttf", ".otf", ".ttc"):
                            try:
                                font = ImageFont.truetype(str(file), 100)
                                family, style = font.getname()
                                result.append((family, style, str(file)))
                            except OSError:
                                pass
            except OSError:
                pass
    if not result:
        for file in Path('/usr/share/fonts').rglob('*.ttf'):
            try:
                family, style = ImageFont.truetype(str(file), 100).getname()
                result.append((family, style, str(file)))
            except OSError:
                pass
    return result


@lru_cache(maxsize=80)
def resolve_font(family, italic=False):
    candidates = [item for item in font_catalog() if item[0].casefold() == family.casefold()]
    missing = not candidates
    if missing:
        candidates = [item for item in font_catalog() if item[0].casefold() in ('arial', 'dejavu sans')]
    if not candidates:
        raise ValueError("Không tìm thấy font TrueType để đo chữ.")
    def score(item):
        style = item[1].lower()
        return (5 * ('bold' in style) + 3 * (('italic' in style or 'oblique' in style) == italic))
    chosen = max(candidates, key=score)
    return chosen, missing


@lru_cache(maxsize=80)
def font_factor(path):
    # ASS sizes use the Windows ascender + descender rather than the font's em square.
    # Read the public TrueType table directory (TTC face 0); no execution of font code.
    try:
        with open(path, 'rb') as file:
            header = file.read(12)
            base = 0
            if header[:4] == b'ttcf':
                base = struct.unpack('>I', file.read(4))[0]
                file.seek(base)
                header = file.read(12)
            count = struct.unpack('>H', header[4:6])[0]
            records = {}
            for _ in range(count):
                tag, _, offset, length = struct.unpack('>4sIII', file.read(16))
                records[tag] = (offset, length)
            file.seek(records[b'head'][0] + 18)
            units = struct.unpack('>H', file.read(2))[0]
            file.seek(records[b'OS/2'][0] + 74)
            ascent, descent = struct.unpack('>HH', file.read(4))
        return max(.5, min(1.2, units / max(1, ascent + descent)))
    except (OSError, KeyError, struct.error):
        return .85


@lru_cache(maxsize=1)
def vietnamese_fonts():
    import unicodedata
    letters = 'ĂăÂâĐđÊêÔôƠơƯưÀÁẢÃẠàáảãạÈÉẺẼẸèéẻẽẹÌÍỈĨỊìíỉĩịÒÓỎÕỌòóỏõọÙÚỦŨỤùúủũụỲÝỶỸỴỳýỷỹỵ'
    letters += ''.join(chr(c) for c in range(0x1EA0, 0x1EFA))
    required = {ord(c) for c in letters + unicodedata.normalize('NFD', letters)}
    safe = []
    for family in sorted({f[0] for f in font_catalog()}, key=str.casefold):
        try:
            paths = {resolve_font(family, italic)[0][2] for italic in (False, True)}
            for path in paths:
                with TTFont(path, fontNumber=0, lazy=True) as font:
                    if not required.issubset(font.getBestCmap() or {}):
                        break
            else:
                safe.append(family)
        except Exception:
            continue
    return safe


@lru_cache(maxsize=160)
def measured_font(family, size, italic=False):
    chosen, missing = resolve_font(family, italic)
    factor = font_factor(chosen[2])
    return ImageFont.truetype(chosen[2], size * factor), chosen[0], factor, missing


class Measure:
    def __init__(self, opts):
        self.opts = opts
        self.font, self.family, self.factor, self.fallback = measured_font(opts['font'], opts['font_size'], opts['italic'])
        self.scale = opts['horizontal_scale'] / 100

    @lru_cache(maxsize=2048)
    def width(self, text):
        return float(self.font.getlength(text)) * self.scale

    def fits(self, tokens, width):
        extra = 2 * self.opts['outline_width'] + self.opts['shadow'] + (self.opts['font_size'] * .15 if self.opts['italic'] else 0)
        return self.width(' '.join(tokens)) + extra <= width


def frame_size(opts):
    return (1080, 1920) if opts['aspect'] == 'portrait' else (1920, 1080)


def split_tokens(tokens, measure, width, maximum=0):
    if not tokens:
        return []
    groups, current = [], []
    for token in tokens:
        if current and ((maximum and len(current) >= maximum) or not measure.fits(current + [token], width)):
            groups.append(current)
            current = []
        current.append(token)
    if current:
        groups.append(current)
    if len(groups) > 1:
        # Prefer balanced rows; otherwise avoid leaving a single token at the end.
        n, k = len(tokens), len(groups)
        q, r = divmod(n, k)
        sizes = [q + (i < r) for i in range(k)]
        balanced, at = [], 0
        for size in sizes:
            balanced.append(tokens[at:at+size])
            at += size
        if all((not maximum or len(g) <= maximum) and measure.fits(g, width) for g in balanced):
            groups = balanced
        elif len(groups[-1]) == 1 and len(groups[-2]) >= 3:
            candidate = groups[-2][-1:] + groups[-1]
            if measure.fits(candidate, width):
                groups[-2] = groups[-2][:-1]
                groups[-1] = candidate
    return groups


def split_text(text, settings):
    opts = validate_settings(settings)
    measure = Measure(opts)
    width, _ = frame_size(opts)
    out, changed = [], 0
    for line in text.replace('\r', '').split('\n'):
        tokens = line.split()
        groups = split_tokens(tokens, measure, width * .9, opts['line_max_words'])
        out.extend(' '.join(g) for g in groups) if groups else out.append('')
        changed += max(0, len(groups) - 1)
    return {'text': '\n'.join(out), 'changed': changed, 'font': measure.family,
            'font_fallback': measure.fallback}


def regroup_words(words, text):
    rows = [line.split() for line in text.splitlines()]
    tokens = [token for row in rows for token in row]
    if tokens != [word['text'] for word in words]:
        raise ValueError("Lời vừa nhập khác các từ đã căn. Hãy căn lời lại trước, hoặc giữ nguyên chữ và chỉ đổi xuống dòng.")
    result, at, section = [], 0, True
    for li, row in enumerate(rows):
        if not row:
            section = True
        for j, _ in enumerate(row):
            item = {**words[at], 'line': li}
            item.pop('section', None)
            item.pop('row', None)
            if j == 0 and section:
                item['section'] = True
            result.append(item)
            at += 1
        if row:
            section = False
    return result


def display_plan(project):
    opts = validate_settings(project.get('settings', {}))
    measure = Measure(opts)
    width, height = frame_size(opts)
    rows = []
    indexed = {**project, 'words': [{**word, 'source_index':i} for i,word in enumerate(project.get('words', []))]}
    for original in lyric_lines(beat_words(indexed)):
        # Break long pauses as well as lines too wide for the frame.
        chunks, current = [], []
        for word in original:
            if current and (word['start'] - current[-1]['end'] > 4 or
                            not measure.fits([w['text'] for w in current] + [word['text']], width * .9)):
                chunks.append(current)
                current = []
            current.append(word)
        if current: chunks.append(current)
        rows.extend(chunks)
    rows.sort(key=lambda row: row[0]['start'])
    plan, countdowns, block_row = [], [], 0
    slots=[]
    for i, words in enumerate(rows):
        if i == 0 or words[0].get('section') or words[0]['start']-rows[i-1][-1]['end']>4:
            block_row=0
        slots.append(words[0].get('row',block_row%2))
        block_row+=1
    block_row=0
    block_start=0
    for i, words in enumerate(rows):
        first = i == 0 or words[0].get('section') or words[0]['start'] - rows[i-1][-1]['end'] > 4
        if first:
            block_row = 0
            block_start=i
        slot = slots[i]
        block_row += 1
        start, end = words[0]['start'], max(w['end'] for w in words)
        ds = max(0, start - 1.5)
        previous = next((j for j in range(i-1,-1,-1) if slots[j] == slot), None)
        following = next((j for j in range(i+1,len(rows)) if slots[j] == slot), None)
        if previous is not None: ds = min(start,max(ds, rows[previous][-1]['end'] + .05))
        if i==block_start+1 and plan[block_start]['slot']!=slot:
            ds=min(ds,plan[block_start]['show_start'])
        de = end + .6
        if following is not None: de = min(de, max(end, rows[following][0]['start'] - 1.55))
        if opts['display_mode'] != 'karaoke':
            ds, de, slot = start, end + .1, 0
            if i+1 < len(rows): de = min(de, max(end, rows[i+1][0]['start']))
        size = opts['font_size']
        text = ' '.join(w['text'] for w in words)
        row_width = measure.width(text)
        shrink = min(1, width * .9 / max(1, row_width))
        size *= shrink
        row_width *= shrink
        align = opts['align']
        if align == 'justify': align = 'right' if slot else 'left'
        x = width * .05 if align == 'left' else width * .95 - row_width if align == 'right' else (width-row_width)/2
        x += width * opts['horizontal_offset']/100
        x = max(width*.02, min(width*.98-row_width, x))
        baseline = {'bottom':height - size * 1.25, 'center':height/2+size*.5,
                    'top':size*(opts['line_height']+2)}[opts['position']]
        if opts['display_mode'] == 'lyricsmv' and opts['position'] == 'bottom': baseline = height*.5
        bottom = baseline - height * opts['vertical_offset']/100
        y = max(0, bottom - size*(1.15 + (opts['line_height'] if slot == 0 and opts['display_mode']=='karaoke' else 0)))
        role = words[0].get('role', 'all') if opts['singing_mode'] == 'duet' and opts['display_mode']=='karaoke' else 'all'
        color = {'male': opts['role_a_color'], 'female': opts['role_b_color']}.get(role, opts['sung'])
        positioned, cursor = [], x
        for j, word in enumerate(words):
            # Measure prefixes so kerning between adjacent tokens is included in placement.
            prefix = ' '.join(w['text'] for w in words[:j])
            left = x + (measure.width(prefix + ' ') if j else 0) * shrink
            word_width = measure.width(word['text']) * shrink
            positioned.append({**word, 'x': round(left, 3), 'width': round(word_width, 3)})
        plan.append({'index':i, 'line':words[0]['line'], 'slot':slot, 'section_start':bool(first),
                     'section_end': i == len(rows)-1 or bool(rows[i+1][0].get('section')) or rows[i+1][0]['start']-end>4,
                     'start':start, 'end':end, 'show_start':ds, 'show_end':max(ds+.01,de),
                     'x':round(x,3), 'y':round(y,3), 'width':round(row_width,3), 'height':round(size*1.2,3),
                     'font_size':round(size,3), 'color':color, 'align':align, 'role':role, 'text':text, 'words':positioned})
        # A new lyric paragraph/row is not necessarily an instrumental break.
        # Count in only at the opening or after at least eight seconds without lyrics.
        previous_vocal_end = max((r['end'] for r in plan[:-1]), default=0)
        count_in = i == 0 or start - previous_vocal_end >= 8
        if count_in and opts['display_mode'] == 'karaoke' and opts['countdown'] != 'none':
            cd_end = max(0, start - .05)
            cd_start = max(0, previous_vocal_end, cd_end-opts['countdown_seconds'])
            if cd_end - cd_start >= .4:
                countdowns.append({'start':cd_start, 'end':cd_end, 'x':width*.05,
                                   'y':max(0,y-size*.9), 'size':size*.7})
    warnings=[]
    for i,row in enumerate(plan):
        previous=next((other for other in reversed(plan[:i]) if other['slot']==row['slot']),None)
        if previous and row['start']<previous['end']-.05:
            warnings.append({'type':'overlap','line':row['line'],'previous_line':previous['line']})
    return {'width':width, 'height':height, 'font':measure.family, 'font_factor':measure.factor,
            'font_fallback':measure.fallback, 'rows':plan, 'countdowns':countdowns,'warnings':warnings}
