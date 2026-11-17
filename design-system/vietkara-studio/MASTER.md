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
