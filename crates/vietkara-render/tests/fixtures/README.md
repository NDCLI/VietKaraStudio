# ASS baseline fixture

`python-c8dae55-simple-karaoke.ass` was generated with `subtitles.build_ass`
from baseline commit `c8dae557ce941485150aa4e6b525f6b136e8319f` on
`codex/ui-redesign-1.1.4`.

The simple fixture supplies one fixed 1920×1080 plan with two Vietnamese lyric
words, karaoke sweep timing and explicit geometry. It isolates serializer
parity from platform-specific font measurement.

`python-c8dae55-layout-fixed-width.json` records `layout.display_plan` for a
shared project containing wrapped duet lyrics, a long pause, beat anchors,
role icons, countdown windows, and a title card with credit and template.
Python and Rust use the same deterministic per-character width function, so
the test covers row splitting, lane assignment, timing windows, alignment,
role artwork geometry/default colors, countdown placement, lyric and title
coordinates, and title box geometry without hiding platform font differences.
The checked-in fixture was generated from the same baseline commit. To
regenerate it, set `VIETKARA_PYTHON_ROOT` to that Python checkout and run
`python tests/generate_layout_python_golden.py`.

`python-c8dae55-layout-fixed-width.ass` is the Python `subtitles.build_ass`
output for that same project. `ass_layout_python_golden.rs` compares all
non-icon event payloads and icon color/run data; `tests/test_ass_raster_parity.ps1`
also compares six FFmpeg/libass frames for both the simple fixture and this
full fixed-metric fixture. Font shaping still needs separate Windows testing.

`role-icons-32/` contains the Pillow FASTOCTREE 32-color versions used by the
Python ASS renderer. Regenerate them from `role-icons/` with
`python tests/generate_role_icon_ass_assets.py`.
