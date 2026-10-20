from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from timing import beat_words, map_time, validate_anchors, validate_words


def test_piecewise_mapping_and_inverse():
    anchors = validate_anchors([{"ref":10,"beat":12}, {"ref":30,"beat":32}, {"ref":50,"beat":62}])
    assert map_time(5, anchors) == 7
    assert map_time(20, anchors) == 22
    assert map_time(40, anchors) == 47
    assert map_time(60, anchors) == 77
    for t in (0,5,10,20,30,40,50,60):
        assert map_time(map_time(t, anchors), anchors, inverse=True) == pytest.approx(t)


def test_reject_reordered_sections_and_nonfinite_times():
    with pytest.raises(ValueError):
        validate_anchors([{"ref":0,"beat":20}, {"ref":10,"beat":5}])
    with pytest.raises(ValueError):
        validate_anchors([{"ref":float('nan'),"beat":0}])


def test_clips_words_to_original_beat_duration():
    project={"beat":{"duration":10},"anchors":[{"ref":2,"beat":0}],"words":[
        {"text":"A","start":0,"end":1,"line":0},
        {"text":"B","start":1,"end":3,"line":0},
        {"text":"C","start":11,"end":13,"line":1},
        {"text":"D","start":13,"end":14,"line":1}]}
    assert [(w['text'],w['start'],w['end']) for w in beat_words(project)] == [('B',0,1),('C',9,10)]
    assert beat_words({"beat":None,"words":[],"anchors":[]}) == []


def test_manual_timing_survives_save_and_maps_back_to_beat():
    anchors = validate_anchors([{"ref": 10, "beat": 8}, {"ref": 20, "beat": 13}])
    words = validate_words([{"text": "hát", "start": map_time(9, anchors, inverse=True),
                             "end": map_time(10, anchors, inverse=True), "manual": True,
                             "estimated": False, "row": 1}])
    assert words[0]["manual"] is True and words[0]["estimated"] is False
    mapped = beat_words({"words": words, "anchors": anchors, "beat": {"duration": 15}})
    assert (mapped[0]["start"], mapped[0]["end"], mapped[0]["row"]) == (9, 10, 1)

