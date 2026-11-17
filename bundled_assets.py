"""Local artwork and fonts shared by the browser, layout and video renderer."""
from functools import lru_cache
from io import BytesIO
import json
import re
from pathlib import Path
import unicodedata
from urllib.parse import quote

from PIL import Image

ASSET_ROOT = Path(__file__).resolve().parent / 'resources' / 'source'
FONT_ROOT = ASSET_ROOT / 'FONT'
ICON_CHOICES = {
    'male': [('man-new-blue.png', 'Xanh dương'), ('man-new-green.png', 'Xanh lá')],
    'female': [('lady-pink.png', 'Hồng'), ('lady-red.png', 'Đỏ'), ('lady-darkred.png', 'Đỏ đậm')],
    'all': [('men(blue) & lady(pink).png', 'Nam xanh dương · Nữ hồng'),
            ('men(blue) & lady(red).png', 'Nam xanh dương · Nữ đỏ'),
            ('men(green) & lady(pink).png', 'Nam xanh lá · Nữ hồng'),
            ('men(green) & lady(red).png', 'Nam xanh lá · Nữ đỏ')],
}
ICON_SETTINGS = {'male': 'role_a_icon', 'female': 'role_b_icon', 'all': 'role_all_icon'}


def asset_url(relative):
    return '/api/assets/source/' + quote(relative, safe='/')


@lru_cache(maxsize=1)
def bundled_fonts():
    return json.loads((ASSET_ROOT / 'fonts.json').read_text(encoding='utf-8'))


@lru_cache(maxsize=256)
def font_encoding(family):
    return next((item['encoding'] for item in bundled_fonts()
                 if item['family'].casefold() == family.casefold()), 'unicode')


# VNI Windows character positions (base letter followed by its accent glyph).
# Verified against UniKey's public encoding tables:
# https://github.com/vn-input/ibus-unikey/blob/master/ukengine/data.cpp
@lru_cache(maxsize=1)
def vni_table():
    rows = [
        ('aáàảãạ', ['a', 'aù', 'aø', 'aû', 'aõ', 'aï']),
        ('âấầẩẫậ', ['aâ', 'aá', 'aà', 'aå', 'aã', 'aä']),
        ('ăắằẳẵặ', ['aê', 'aé', 'aè', 'aú', 'aü', 'aë']),
        ('eéèẻẽẹ', ['e', 'eù', 'eø', 'eû', 'eõ', 'eï']),
        ('êếềểễệ', ['eâ', 'eá', 'eà', 'eå', 'eã', 'eä']),
        ('iíìỉĩị', ['i', 'í', 'ì', 'æ', 'ó', 'ò']),
        ('oóòỏõọ', ['o', 'où', 'oø', 'oû', 'oõ', 'oï']),
        ('ôốồổỗộ', ['oâ', 'oá', 'oà', 'oå', 'oã', 'oä']),
        ('ơớờởỡợ', ['ô', 'ôù', 'ôø', 'ôû', 'ôõ', 'ôï']),
        ('uúùủũụ', ['u', 'uù', 'uø', 'uû', 'uõ', 'uï']),
        ('ưứừửữự', ['ö', 'öù', 'öø', 'öû', 'öõ', 'öï']),
        ('yýỳỷỹỵ', ['y', 'yù', 'yø', 'yû', 'yõ', 'î']),
        ('đ', ['ñ']),
    ]
    table = {}
    for letters, encoded in rows:
        for letter, value in zip(letters, encoded):
            table[letter] = value
            table[letter.upper()] = value.upper()
    return table


def font_text(text, family):
    text = unicodedata.normalize('NFC', str(text))
    encoding = font_encoding(family)
    if encoding in ('vni', 'tcvn3'):
        table = vni_table() if encoding == 'vni' else tcvn_table()
        return ''.join(table.get(char, char) for char in text)
    return text


@lru_cache(maxsize=1)
def tcvn_table():
    letters = 'áàảãạâấầẩẫậăắằẳẵặéèẻẽẹêếềểễệíìỉĩịóòỏõọôốồổỗộơớờởỡợúùủũụưứừửữựýỳỷỹỵđ'
    values = '¸µ¶·¹©ÊÇÈÉË¨¾»¼½ÆÐÌÎÏÑªÕÒÓÔÖÝ×ØÜÞãßáâä«èåæçé¬íêëìîóïñòô\u00adøõö÷ùýúûüþ®'
    table = dict(zip(letters, values))
    table.update({key.upper(): value for key, value in list(table.items())})
    table.update(dict(zip('ÂĂÊÔƠƯĐ', '¢¡£¤¥¦§')))
    return table


def role_asset(role, settings):
    role = role if role in ICON_CHOICES else 'all'
    return settings.get(ICON_SETTINGS[role], ICON_CHOICES[role][0][0])


@lru_cache(maxsize=9)
def icon_geometry(filename):
    with Image.open(ASSET_ROOT / 'ICON' / filename) as image:
        return image.size


def role_icon_image(filename, color=None):
    if filename not in {name for choices in ICON_CHOICES.values() for name, _ in choices}:
        raise ValueError('Biểu tượng vai hát không hợp lệ.')
    with Image.open(ASSET_ROOT / 'ICON' / filename) as source:
        image = source.convert('RGBA')
    if color is None:
        return image
    if not re.fullmatch(r'#[0-9a-fA-F]{6}', color):
        raise ValueError('Màu biểu tượng không hợp lệ.')
    target = tuple(int(color[i:i+2], 16) for i in (1, 3, 5))
    counts = {rgba: count for count, rgba in image.getcolors(image.width*image.height)}
    base = max((rgba for rgba in counts if rgba[3] and max(rgba[:3])-min(rgba[:3])>32),
               key=counts.get)
    base_low, chroma = min(base[:3]), max(base[:3])-min(base[:3])
    palette = {}
    for rgba in counts:
        red, green, blue, alpha = rgba
        spread = max(red, green, blue)-min(red, green, blue)
        if not alpha or spread <= 16:
            palette[rgba] = rgba
            continue
        weight = min(1, spread/chroma)
        neutral = max(0, min(red, green, blue)-weight*base_low)
        palette[rgba] = tuple(min(255, round(neutral+weight*c)) for c in target)+(alpha,)
    pixels = image.load()
    image.putdata([palette[pixels[x, y]] for y in range(image.height) for x in range(image.width)])
    return image


@lru_cache(maxsize=128)
def role_icon_png(filename, color):
    output = BytesIO()
    role_icon_image(filename, color).save(output, format='PNG')
    return output.getvalue()


@lru_cache(maxsize=256)
def icon_drawing(filename, size, color=None):
    """Embed the PNG in ASS as colored scanline rectangles; no external image needed.

    ASS has no bitmap primitive. Palette layers preserve the supplied artwork,
    transparency and row animation, including in standalone subtitle exports.
    """
    image = role_icon_image(filename, color).quantize(colors=32, method=Image.Quantize.FASTOCTREE).convert('RGBA')
    width, height = image.size
    factor = size / height
    layers = {}
    pixels = image.load()
    for y in range(height):
        x = 0
        while x < width:
            rgba = pixels[x, y]
            end = x + 1
            while end < width and pixels[end, y] == rgba:
                end += 1
            if rgba[3]:
                x1, x2, y1, y2 = x*factor, end*factor, y*factor, (y+1)*factor
                layers.setdefault(rgba, []).append(
                    f'm {x1:.3f} {y1:.3f} l {x2:.3f} {y1:.3f} {x2:.3f} {y2:.3f} {x1:.3f} {y2:.3f} {x1:.3f} {y1:.3f}')
            x = end
    # Degenerate contours keep each color layer aligned to the complete image.
    bounds = f'm 0 0 l 0 0 m {width*factor:.3f} {size:.3f} l {width*factor:.3f} {size:.3f} '
    return tuple((rgba, bounds + ' '.join(paths)) for rgba, paths in layers.items())
