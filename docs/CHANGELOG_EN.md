# Changelog

All notable changes to released versions live here; see `docs/CHANGELOG规范.md` for the format.

## [0.1.6] - 2026-10-14

An editor open-capability batch: images open directly, legacy encodings like GBK/UTF-16 open and save; a tab context menu and a changelog entry point; fixes for frozen large single-line files and CJK crashes; scrolling feel aligned with Zed.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Images open directly: the open route sniffs file headers (previously any file containing NUL bytes went gray-screen, so png and friends never got rendered); png/jpg/gif/webp/bmp etc. render in an image tab, and with tiff/ico/pnm added the set matches Zed's eight formats; svg opens as source (preview stays a separate action for later). (f8046cd, b8ba388)
- Legacy encodings open and save: opening text now runs an encoding-detection pipeline (BOM / UTF-16 NUL distribution / chardetng sniffing + magic header table, ported line-by-line from Zed's file_content), so GBK and UTF-16LE files like .reg open, and saving writes back in the original encoding, round-trip-safe. (f8046cd)
- Tab context menu: Close / Close Others / Close Left / Close Right / Close All (same as Zed); batch-closing folds multiple dirty files into a single confirmation (Save & Close / Close Without Saving applies to the whole batch); the first session tab can never be closed. (11331e6)
- Click the version number on the new-session page to open the changelog: the dimmed idle version label is clickable — it reads the local cache first and fetches by tag if missing; a failed fetch now times out at 15s and shows "暂时无法连接 GitHub" (can't reach GitHub right now), and clicking again retries, instead of waiting 30s and landing on a misleading "no changelog". (e423e6b, 4a480f6)

### Bug fixes
- Large single-line files froze: opening minified frontend bundles (e.g. echarts.min.js, a 940k-character last line) froze for minutes per frame — syntax highlighting and text shaping are now clipped to the viewport byte window, back to milliseconds. (1d30077)
- Crashes and misplaced highlights on CJK files: after viewport clipping, style spans no longer covered the whole visible text, so files containing Chinese were sliced mid-multibyte-character and crashed outright (0xc0000409), while ASCII files got silently misaligned highlights — default-style fillers before and after the window restore the tiling invariant. (17138f2)
- Markdown soft-wrap regression: turning WYSIWYG off used to drag soft-wrap down with it; the two are decoupled and source editing wraps again (consistent with the preview state). (b654ae8)
- First-launch-after-update popup accounting: dev/smoke builds no longer write last_seen — previously a single run polluted the ledger and swallowed the real update's first-launch popup. (dab6fe7)

### Improvements
- Scrolling feel aligned with Zed: per-frame tree-sitter re-queries and per-capture heap allocations inside the viewport are gone (verified equal by a differential test), the double frame per wheel tick is eliminated, and line heights snap to whole device pixels (fixing sub-pixel shimmer at 1.5× scaling); app-side overhead for large-file scrolling is essentially zero. (35836ff)
## [0.1.5] - 2026-10-13

In-app updates gain mirror failover and a download percentage, the context usage popup gets a redesign, plus a batch of chrome alignment tweaks.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- In-app update mirror failover: payload downloads speed-test the direct connection (abandoned after a 3s average below 128KB/s) and walk through mirrors until one hits; checksums are pinned to the official hash over a direct connection to prevent cross-packed downloads. `PI_FLASH_MIRROR` / `PI_FLASH_MIRRORS` share the installer's rules. (6734db6)
- In-app update download percentage: the version slot now refreshes in place with NN% (omitted when the server sends no content-length). (8e82205)
- Context usage popup redesign: a title line (used / window · %) with an accent progress bar, a per-category breakdown row (7-bucket estimate, descending color dots) and the cache-hit rate, in a 420px two-column grid with thousand-separated token counts. (bf03158, c17247c, 6b6c827, a618b1c)

### Bug fixes
- Phantom keybinding hints: the tooltips for Ctrl+P / N / K could show fake keys like Win+P (gpui's cmd is a platform modifier); the cmd-* twin bindings were removed. (d63039e)

### Improvements
- A batch of chrome alignment (owner-decided): topbar and statusbar are now equal height at 40px (window control buttons follow at 42×40); bar tabs fit the bar height; the statusbar settings button's right edge aligns with the list right line and the first tab card's left edge sits on the icon alignment line; the icon alignment line collapses into a single token (11→12px); composer right-group button spacing matches the left group (15px). (d2760c5, c100b22, 61981c5, 418512c, d00de94, e015300, 189008a)
## [0.1.4] - 2026-10-13

A UI polish batch: bundled pi upgraded to 1.2.0, global shortcuts and keybinding hints, a set of completion-menu / cross-project fixes, and several owner-decided UI tweaks.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Global shortcuts: Ctrl+P open project, Ctrl+N new session, Ctrl+K search sessions — available anywhere in the app. (ec16a1e)
- Keybinding hints: sidebar header and project-row buttons now have tooltips with a keybinding chip that follows rebinds automatically; tooltip delay shortened to 350ms. (ec16a1e, 670869a, c55c993)
- Binary file handling: unparseable files open a tab with a gray page showing the detected type plus an "open with default app" fallback (Zed-style). (25e9965)
- Markdown WYSIWYG toggle: new switch on the Settings > Interface page, off by default. (bcb8e04)
- Session rename box upgraded: three auto-growing lines, selects all on open with the cursor anchored at the start, so typing replaces the whole name. (04cbbb2)

### Bug fixes
- @ / slash completion menu: menu items were not clickable (clicking did nothing and the raw query was sent, losing the directory prefix), and Enter accepted item 0 regardless of the selection. Both fixed. (7913e03, 3fe693c)
- Cross-project session switching: opening a session from another project left the file tree / changes panel on the previous project (empty tree, wrong changes); ghost-tab clicks and the automation entry are covered too. (5d708ff, 1e46aaa)
- Session rename box opened blank (text scrolled horizontally off-screen); the first frame now shows the full text selected. (72e0dfc)
- About page author name went missing (a call site dropped in the semantic-key migration). (63516a3)

### Improvements
- Bundled pi upgraded from 1.1.0 to 1.2.0, with all protocol review points passing. (3695715)
- Panels decoupled from the content area: switching the left dock no longer swaps the content view — switching to the file tree on a chat page no longer replaces the conversation with a document. (a7eca88)
- A batch of UI tweaks (owner-decided): composer extensions button becomes a plug icon + [n] count; dock collapse button aligned with the list icon line; terminal icon unified to the boxed style; "more" icon switched to a bare lucide ellipsis; eye/search toolbar buttons brought up to spec; v5 wordmark enlarged and the welcome logo +20%. (1a2121a, b7e7a71, 0583b77, 7c2024b, 9c03115, 3d3b656, dd985f6, f84e8c5, 91c51d0, c0ab7fa)
## [0.1.3] - 2026-10-13

Instant sidebar feedback + a faster download path: ghost tabs appear the moment you send, and slow downloads switch mirrors automatically.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Sidebar ghost tabs: the moment a new session's first message is sent, a tab appears in the sidebar immediately (spinner + first-line preview) and turns into the real tab once the session file lands; rejected messages, dead processes or no session file remove the placeholder instead of leaving a tab that never becomes real. (5914900)
- Download speed-test with automatic mirror fallback: payload downloads for install/update measure the direct connection and switch through the gh-proxy.com / ghfast.top / ghproxy.net mirrors when slow; the download line refreshes percentage and speed in place, with a 30s no-activity timeout. `PI_FLASH_MIRROR` still pins a single source. (8b2048e)

### Bug fixes
- Startup restore: closing the app without sending anything no longer loses the last session (the "phantom path" — pi reporting the session file during construction before it exists on disk — is no longer bound). (5914900)

### Improvements
- Download residue cleanup: partial downloads (payload.tmp-* / update.tmp-*) are removed on launch and before downloads based on whether the owning process is alive; stale leftovers are always cleaned. Checksums come from the official source over a direct connection while the zip goes through mirrors, preventing cross-packed downloads. (8b2048e)
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

## [0.1.1] - 2026-10-12

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
