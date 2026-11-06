# ASS baseline fixture

`python-c8dae55-simple-karaoke.ass` was generated with `subtitles.build_ass`
from baseline commit `c8dae557ce941485150aa4e6b525f6b136e8319f` on
`codex/ui-redesign-1.1.4`.

The fixture supplies one fixed 1920×1080 plan with two Vietnamese lyric words,
karaoke sweep timing and explicit geometry. Fixing the plan isolates ASS
serialization from platform-specific font measurement; layout and raster
parity still require separate Windows golden tests.

`python-c8dae55-layout-fixed-width.json` records `layout.display_plan` for a
shared project containing wrapped duet lyrics, a long pause, and beat anchors.
Python and Rust use the same deterministic per-character width function, so
the test covers row splitting, lane assignment, timing windows, alignment,
and each word's coordinates without hiding platform font differences. The
checked-in fixture was generated from the same baseline commit. To regenerate
it, set `VIETKARA_PYTHON_ROOT` to that Python checkout and run
`python tests/generate_layout_python_golden.py`.
