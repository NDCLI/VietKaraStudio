# VietKara Studio — UI/UX Pro Max design system

This is a desktop audio and karaoke editor rendered with HTML/CSS/JavaScript in WebView2, backed by Flask. It is not a marketing website.

## Skill evidence and decisions

- Installed skill: nextlevelbuilder/ui-ux-pro-max-skill, `.claude/skills/ui-ux-pro-max`.
- Searches: `audio editor desktop dark` and one narrower retry `desktop productivity dashboard`, both with design-system, density 8, variance 3.
- Verified match: Minimalism & Swiss Style fits professional tools, panel grids, clear hierarchy and low rendering cost.
- The generated landing-page patterns did not match the editor; they are not adopted or persisted as product recommendations. The workstation arrangement below is a project-specific fallback guided by the skill's accessibility, layout and interaction rules.
- Keyboard focus search: visible focus and focus not obscured. Reviewed quick-reference priorities 1–3 and pro-rules.

## Visual system

- Dark charcoal/navy surfaces with violet primary actions, cyan transport feedback and restrained amber for backing tracks.
- Text: #edf2fa; secondary #a8b6ca; tertiary #90a1b8. Surface #121a26; page #0b1018. Semantic variables live in static/tokens.css.
- Use the existing brand SVG and one 1.7px outline icon family. No emoji for navigation or system controls.
- Segoe UI/system sans for offline Vietnamese support. Body 14px, panels 13px, labels 12px; mobile form text 16px. Karaoke preview fonts remain project-owned.
- Spacing 4/8/12/16/24/32px. Controls 36px on desktop, at least 44px on coarse pointers. Small timeline/mixer controls retain a 24px minimum desktop target.
- Focus rings 2px with a 3px offset. Do not use color as the only selected-state cue.

## Structure

- Header: brand, primary pages, project title/save state, processing-settings disclosure, export.
- Editor: lyrics/source panel, central preview, properties panel. Mixer and word timeline span the workspace below the transport.
- Processing model choices are available in a labeled disclosure rather than occupying the main work area.
- Empty project: file/YouTube import with a clear entry into the editor. No fake processing or invented results.
- Separation: source → mode → progress → independent listen/download cards.
- Settings: data location first; models grouped by purpose with technical details behind disclosure, followed by update controls.

## Interaction and validation

- Keep existing element IDs, endpoints, handlers, keyboard shortcuts and media timing.
- Respect reduced-motion. No parallax, autoplay or decorative animation.
- Menus close outside/on Escape; native dialogs contain focus. Errors remain near actions. Loading controls are disabled with a readable status.
- Validate empty and populated editor, separation, settings, update dialog, narrow windows and touch layouts at 375/768/1024/1366/1920 widths. Verify there is no document-level horizontal scrolling.
# UI QA completed 2026-11-18

- Chromium/Edge headless against Flask with an isolated OBS_DATA_DIR; generated 30-second audio and four timed lyric lines.
- Widths 375, 768, 1024, 1366 and 1920: Karaoke, Separation and Settings have no document horizontal overflow.
- Verified playback, line-gap slider, both panel collapse actions, processing device selection, outside-click/Escape menu dismissal, arrow-key tabs, export dialog and update Later action.
- Axe WCAG A/AA: no violations in Separation or Settings. Editor findings are limited to project-rendered outlined karaoke glyphs; theme colors do not replace project lyric styling. The app's existing render review reports lyric contrast separately.
- Python: 201 tests passed. JavaScript: 29 tests passed. Changed scripts passed syntax checks.
- No live user projects or model files were modified during UI QA.


## Review refinements — 2026-11-18

- Rename Preset to Mẫu chữ in user-facing controls. Thumbnail names are available through accessible labels and hover titles; sample text uses the source-font encoding adapter.
- Replace the horizontal thumbnail strip with an adaptive grid filling the properties panel. Keep import/export/delete below the scrolling samples.
- Draw measured waveform peaks as a continuous, monotone interpolated envelope, preserving peak maxima when zoomed out. Played and future portions share the same silhouette; no synthetic audio detail is added.
- Use a shared waveform/timeline gutter so mixer labels do not cover the start of the waveform.
- Checked sample bounds and grid scrolling at 375/768/1366/1440/2560 widths, applying a sample and export-button state, waveforms at zoom 1 and 64, and 29 JavaScript tests. Viewed the refreshed native preview with Mẫu chữ open. Release remains unpublished for user review.


## Waveform detail correction — 2026-11-18

- Replace smoothed coarse-peak interpolation with viewport-based signed audio minima/maxima. Zoom requests measured details from the existing WAV and renders a jagged, asymmetric silhouette.
- Normalize against a cached whole-track peak so quiet and loud regions keep the same scale when zooming; cache keys include modification time and file size.
- Bound each response to 6,000 bins; prefetch adjacent audio and reuse the result while the viewport stays covered. Refresh paused canvases after async data arrives.
- Checks: 205 Python tests and 29 JavaScript tests pass. Four new tests cover signed transients, stable gain, frame boundaries/output limits, source replacement and endpoint validation. Browser checks verified detail at zoom 1/64, mobile width, no request loop and no JS errors.
- Earlier smooth-envelope guidance is superseded by this correction. Release remains unpublished during visual review.

- Header review: project title/save state, processing settings and export form one right-aligned group with 8–12px gaps at desktop widths. Verified 1024/1366/1920/2560 widths and preserved mobile reflow.


## Alignment and role color review — 2026-11-18

- Replace four alignment text buttons with consistent outline SVGs, accessible names, pressed states and hover titles. Rename only the inspector Song ca tab to Phân vai.
- Role artwork fills follow the row's sung color for male, female and shared singing. Preserve original white/neutral borders, alpha and geometry; apply one tint implementation to preview PNGs and ASS drawing layers.
- Role artwork choices also preview current role colors. Neutral Kiểu labels describe the artwork variants without presenting obsolete fixed color names.
- Verified all four alignment actions, live male color edits and female preview, and a frame rendered through FFmpeg/ASS. Python 208 tests and JavaScript 29 tests pass. Release remains unpublished for review.
