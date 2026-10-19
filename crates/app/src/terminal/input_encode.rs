//! 终端键盘/IME → PTY 字节编码：keystroke_to_pty 及辅助、bracketed
//! paste、IME commit；TermInput（gpui InputHandler）桥。

use std::sync::Arc;

use super::*;
use alacritty_terminal::event_loop::EventLoopSender;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};
use gpui::{Keystroke, Pixels, Point, SharedString, UTF16Selection, WeakEntity, Window, px};

/// Translate a gpui keystroke into PTY bytes. Returns None when the keystroke
/// is not terminal input (e.g. plain modifier presses are filtered upstream).
pub fn keystroke_to_pty(k: &Keystroke, mode: &TermMode) -> Option<Vec<u8>> {
    let ctrl = k.modifiers.control;
    let alt = k.modifiers.alt;
    let shift = k.modifiers.shift;
    let key = k.key.as_str();
    let alt_prefix: &[u8] = if alt { b"\x1b" } else { b"" };

    // ctrl combos -> control characters
    if ctrl {
        if let Some(b) = ctrl_byte(key) {
            let mut v = alt_prefix.to_vec();
            v.push(b);
            return Some(v);
        }
        // ctrl+arrows etc. fall through to the modified CSI form below
    }

    // modifier mask for CSI 1;m{A} / CSI n;{m}~  (1 + shift + alt*2 + ctrl*4)
    let m = 1 + shift as i32 + 2 * alt as i32 + 4 * ctrl as i32;

    match key {
        "enter" => return Some(between(alt_prefix, b"\r")),
        "tab" => {
            return Some(if shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            });
        }
        "backspace" => return Some(between(alt_prefix, b"\x7f")),
        "escape" => return Some(between(alt_prefix, b"\x1b")),
        "space" => {
            if ctrl {
                return Some(b"\x00".to_vec());
            }
            return Some(between(alt_prefix, b" "));
        }
        _ => {}
    }

    // arrows / home / end / page / insert / delete / F-keys
    if let Some(seq) = special_key(key, m, mode) {
        return Some(seq);
    }

    // printable input: key_char carries layout/shift-correct characters
    if !ctrl {
        if let Some(s) = &k.key_char {
            if !s.is_empty() {
                let mut v = alt_prefix.to_vec();
                v.extend_from_slice(s.as_bytes());
                return Some(v);
            }
        }
        if key.len() == 1 && key.as_bytes()[0].is_ascii_graphic() {
            let mut v = alt_prefix.to_vec();
            v.push(key.as_bytes()[0]);
            return Some(v);
        }
    }
    None
}

fn between(prefix: &[u8], rest: &[u8]) -> Vec<u8> {
    let mut v = prefix.to_vec();
    v.extend_from_slice(rest);
    v
}

fn ctrl_byte(key: &str) -> Option<u8> {
    let b = match key {
        "space" | "@" => 0x00,
        "a" => 0x01,
        "b" => 0x02,
        "c" => 0x03,
        "d" => 0x04,
        "e" => 0x05,
        "f" => 0x06,
        "g" => 0x07,
        "h" => 0x08,
        "i" => 0x09,
        "j" => 0x0a,
        "k" => 0x0b,
        "l" => 0x0c,
        "m" => 0x0d,
        "n" => 0x0e,
        "o" => 0x0f,
        "p" => 0x10,
        "q" => 0x11,
        "r" => 0x12,
        "s" => 0x13,
        "t" => 0x14,
        "u" => 0x15,
        "v" => 0x16,
        "w" => 0x17,
        "x" => 0x18,
        "y" => 0x19,
        "z" => 0x1a,
        "[" => 0x1b,
        "\\" => 0x1c,
        "]" => 0x1d,
        "^" => 0x1e,
        "_" | "-" => 0x1f,
        _ => return None,
    };
    Some(b)
}

pub(super) fn special_key(key: &str, m: i32, mode: &TermMode) -> Option<Vec<u8>> {
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let seq: Vec<u8> = match key {
        "left" | "right" | "up" | "down" => {
            let c = match key {
                "left" => 'D',
                "right" => 'C',
                "up" => 'A',
                _ => 'B',
            };
            if m > 1 {
                format!("\x1b[1;{m}{c}").into_bytes()
            } else if app_cursor {
                format!("\x1bO{c}").into_bytes()
            } else {
                format!("\x1b[{c}").into_bytes()
            }
        }
        "home" | "end" => {
            let c = if key == "home" { 'H' } else { 'F' };
            if m > 1 {
                format!("\x1b[1;{m}{c}").into_bytes()
            } else if app_cursor {
                format!("\x1bO{c}").into_bytes()
            } else {
                format!("\x1b[{c}").into_bytes()
            }
        }
        "insert" => tilde(2, m),
        "delete" => tilde(3, m),
        "pageup" => tilde(5, m),
        "pagedown" => tilde(6, m),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => tilde(15, m),
        "f6" => tilde(17, m),
        "f7" => tilde(18, m),
        "f8" => tilde(19, m),
        "f9" => tilde(20, m),
        "f10" => tilde(21, m),
        "f11" => tilde(23, m),
        "f12" => tilde(24, m),
        _ => return None,
    };
    Some(seq)
}

fn tilde(n: u32, m: i32) -> Vec<u8> {
    if m > 1 {
        format!("\x1b[{n};{m}~").into_bytes()
    } else {
        format!("\x1b[{n}~").into_bytes()
    }
}

/// Paste text: bracketed when the app enabled it, CR-normalized otherwise
/// (zed `Terminal::paste` parity).
pub fn paste_bytes(text: &str, mode: &TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        let mut v = Vec::with_capacity(text.len() + 12);
        v.extend_from_slice(b"\x1b[200~");
        v.extend_from_slice(text.replace('\x1b', "").as_bytes());
        v.extend_from_slice(b"\x1b[201~");
        v
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

// ---------------------------------------------------------------------------
// IME input handler (window.handle_input target)
// ---------------------------------------------------------------------------

/// IME committed text → PTY bytes: CR-normalized UTF-8 with the ESC byte
/// stripped (commit is typed input, not paste — no bracketed-paste markers).
pub fn ime_commit_bytes(text: &str) -> Vec<u8> {
    text.replace("\r\n", "\r")
        .replace('\n', "\r")
        .replace('\x1b', "")
        .into_bytes()
}

/// The terminal's `InputHandler`: without a handler registered for the
/// focused frame, the Windows IME path (`with_input_handler` in
/// WM_IME_COMPOSITION handling) drops composition and commit silently.
/// The terminal has no document model, so ranges are ignored (the Windows
/// path always passes None) — same shape as zed's terminal input handler.
pub(super) struct TermInput {
    pub(super) tab_id: usize,
    pub(super) term: Arc<FairMutex<Term<Proxy>>>,
    pub(super) pty: EventLoopSender,
    pub(super) weak: WeakEntity<Chat>,
    /// element bounds, captured at paint — base for caret-cell coordinates
    pub(super) element_bounds: gpui::Bounds<Pixels>,
    pub(super) cell_w: f32,
    pub(super) line_h: f32,
    pub(super) font_size: f32,
}

impl TermInput {
    /// Caret cell in viewport coordinates `(col, row)`, None when scrolled
    /// out of view.
    fn caret_cell(&self) -> Option<(usize, usize)> {
        let term = self.term.lock();
        let offset = term.grid().display_offset() as i32;
        let content = term.renderable_content();
        let row = content.cursor.point.line.0 + offset;
        let rows = term.grid().screen_lines();
        (row >= 0 && (row as usize) < rows).then(|| (content.cursor.point.column.0, row as usize))
    }

    /// Text left of the grid cursor on its row (snapshot rules: wide spacers
    /// skipped, empty/control cells render as spaces). Shaped, this gives the
    /// pixel-true caret x — CJK fallback glyphs don't advance exactly
    /// 2 cells, so col*cell_w drifts off the painted text.
    fn caret_prefix_text(term: &Term<Proxy>) -> String {
        let content = term.renderable_content();
        let grid_row = &term.grid()[Line(content.cursor.point.line.0)];
        let mut s = String::new();
        for c in 0..content.cursor.point.column.0 {
            let cell = &grid_row[Column(c)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let mut ch = if cell.flags.contains(Flags::HIDDEN) {
                ' '
            } else {
                cell.c
            };
            if ch == '\0' || ch == '\n' || ch == '\r' {
                ch = ' ';
            }
            s.push(ch);
        }
        s
    }

    fn set_preedit(&self, text: Option<String>, cx: &mut gpui::App) {
        let _ = self.weak.update(cx, |chat, cx| {
            if let Some(tab) = chat.ws.terminals.iter_mut().find(|t| t.id == self.tab_id) {
                if tab.preedit != text {
                    tab.preedit = text;
                    cx.notify();
                }
            }
        });
    }

    fn commit(&self, text: &str, cx: &mut gpui::App) {
        use alacritty_terminal::event_loop::Msg;
        self.set_preedit(None, cx);
        if !text.is_empty() {
            let _ = self.pty.send(Msg::Input(ime_commit_bytes(text).into()));
        }
    }
}

impl gpui::InputHandler for TermInput {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut gpui::App,
    ) -> Option<UTF16Selection> {
        // caret = empty selection at a virtual document origin
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &mut self,
        _window: &mut Window,
        _cx: &mut gpui::App,
    ) -> Option<std::ops::Range<usize>> {
        None
    }

    fn text_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        _adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut gpui::App,
    ) -> Option<String> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut gpui::App,
    ) {
        self.commit(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        cx: &mut gpui::App,
    ) {
        self.set_preedit(
            if new_text.is_empty() {
                None
            } else {
                Some(new_text.to_string())
            },
            cx,
        );
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut gpui::App) {
        self.set_preedit(None, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        window: &mut Window,
        _cx: &mut gpui::App,
    ) -> Option<gpui::Bounds<Pixels>> {
        // IME candidate window placement at the caret: shape the row text
        // left of the cursor for the pixel-true x (col*cell_w drifts with
        // wide chars — see caret_prefix_text)
        let (_, row) = self.caret_cell()?;
        let prefix = {
            let term = self.term.lock();
            Self::caret_prefix_text(&term)
        };
        let run = gpui::TextRun {
            len: prefix.len(),
            font: cell_font(&CellStyle {
                fg: hsl(TERM_FG),
                bg: None,
                bold: false,
                italic: false,
                underline: false,
                strike: false,
            }),
            color: hsl(TERM_FG),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            SharedString::from(prefix),
            px(self.font_size),
            &[run],
            None,
        );
        Some(gpui::Bounds {
            origin: gpui::point(
                self.element_bounds.origin.x + px(PAD_L) + line.width,
                self.element_bounds.origin.y + px(PAD_T) + px(self.line_h * row as f32),
            ),
            size: gpui::size(px(self.cell_w), px(self.line_h)),
        })
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut gpui::App,
    ) -> Option<usize> {
        None
    }
}
