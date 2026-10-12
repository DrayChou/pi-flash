# Changelog

All notable changes to released versions live here; see `docs/CHANGELOG规范.md` for the format.

## [0.1.2] - 2026-10-12

## [0.1.2] - 2026-10-12

A terminal & file-editing fix batch: copy/paste entries for the terminal, a set of field-reported UI fixes, and HTML files now open as source in the editor.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Terminal copy/paste entries: a context menu (copy / paste / select all) plus Windows-style keys — Ctrl+C copies when there is a selection (still sends ^C interrupt without one), Ctrl+Shift+C / Ctrl+Insert to copy, Shift+Insert to paste, Ctrl+Shift+A to select all. (9daa12c)
- Terminal selection: double-click selects a word, triple-click a whole (logical) line, and dragging past the terminal edge auto-scrolls. (1c68a5b)
- Terminal editing with a selection: Backspace / Delete removes the whole selection at once, and pasting replaces it. (3afb8a3)
- New "auto-collapse after" setting: 1 / 3 / 7-day options; project groups inactive beyond the threshold start collapsed. (e03659e)

### Bug fixes
- Terminal: with multiple terminals open, switching tabs no longer sends keystrokes to the background terminal; clicking a tab focuses it, no extra click on the terminal needed. (80ede11)
- Terminal: "Open in terminal" no longer switches the left dock to the file tree; a project can have multiple terminals instead of jumping to the existing one. (73173bd)
- Sessions: switching to or creating a session brings the content area back to the chat view instead of staying on a terminal / file tab. (badaed6)
- Sessions: a new session appears in the sidebar as soon as its first message is sent, not after the whole turn finishes. (23506b7)
- Sessions: tightened end-of-turn notification dots — an errored turn no longer shows the "completed" blue dot, and a single tool error no longer marks the whole turn red. (faee211)
- Session list: titles showing "(empty)" — when a long system-prompt line or a huge paste pushed the first user message out of the index, the first 50 characters are now recovered as the preview. (fff7f8a)
- Sessions: during the auto-retry backoff window (2–8s) the UI wrongly showed idle; it now stays "running" and the status line reports "error, retrying in Ns (attempt x/y)". (0747542)
- Markdown rendering: inline `code`, bold and links inside list items were dropped entirely (only punctuation remained). (cd7528c)
- Editor: syntax highlighting for large HTML files (an 118KB design file in testing) went from ~8s per frame to smooth. (382ab0b)
- Images: result / large image previews scale proportionally, no more vertical padding. (05317a0)
- Settings: the "show thinking" description showed Chinese on Traditional Chinese / English UI. (0f50b2c)
- Project card: clicking outside the card during rename now cancels it instead of forcing a rename. (2bc46de)
- The taskbar icon had a ring of padding around it; the .ico was regenerated full-bleed at all nine sizes. (7ad6c63)

### Improvements
- HTML files now open as source in the editor (syntax highlighting, editable, autosave), with "Open in browser" kept as the first action in the context menu. (8fd3bcf, e3e3dd5)
- The completion sound is now an original warm marimba two-tone (A4→E5). (4cba884)
- New i18n missing-translation guard: UI copy without a Traditional Chinese / English translation now fails tests instead of silently falling back to Chinese. (ed2e126)

First official public release of pi-flash: a fast pi coding agent desktop client built with Rust + GPUI.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19. 
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.

### New
- Agent sessions
- File management and editing
- Pi configuration: models / skills / extensions / MCP
- Other settings

### Bug fixes
- The npm package installs correctly: the thin shell fetches the payload from the GitHub Release by its own version and verifies SHA-256
## [0.1.0] - 2026-10-12

First official public release of pi-flash: a fast pi coding agent desktop client built with Rust + GPUI.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19. 
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.

### New
- Agent sessions
- File management and editing
- Pi configuration: models / skills / extensions / MCP
- Other settings
