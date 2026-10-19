//! 终端 256 色/命名色调色板：alacritty Color → gpui 颜色的换算层
//! （paint 每格消费）。

use super::*;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// 256-color palette (+256 fg / 257 bg / 258 cursor) from the xterm theme.
pub fn palette256(i: usize) -> u32 {
    match i {
        0..=15 => ANSI16[i],
        16..=231 => {
            let i = i - 16;
            let levels = [0x00u32, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
            let r = levels[i / 36];
            let g = levels[(i % 36) / 6];
            let b = levels[i % 6];
            (r << 16) | (g << 8) | b
        }
        232..=255 => {
            let v = (8 + (i - 232) * 10) as u32;
            (v << 16) | (v << 8) | v
        }
        256 => TERM_FG,
        257 => TERM_BG,
        258 => TERM_CURSOR,
        _ => TERM_FG,
    }
}

/// ColorRequest answer: palette entry as `Rgb`.
pub fn term_rgb(i: usize) -> Rgb {
    let v = palette256(i);
    Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    }
}

pub(super) fn named_index(n: NamedColor) -> usize {
    use NamedColor::*;
    match n {
        Black => 0,
        Red => 1,
        Green => 2,
        Yellow => 3,
        Blue => 4,
        Magenta => 5,
        Cyan => 6,
        White => 7,
        BrightBlack => 8,
        BrightRed => 9,
        BrightGreen => 10,
        BrightYellow => 11,
        BrightBlue => 12,
        BrightMagenta => 13,
        BrightCyan => 14,
        BrightWhite => 15,
        Foreground | BrightForeground | DimForeground => 256,
        Background => 257,
        Cursor => 258,
        DimBlack => 0,
        DimRed => 1,
        DimGreen => 2,
        DimYellow => 3,
        DimBlue => 4,
        DimMagenta => 5,
        DimCyan => 6,
        DimWhite => 7,
    }
}

pub(super) fn color_to_rgb(c: &Color) -> u32 {
    match c {
        Color::Spec(rgb) => ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32,
        Color::Indexed(i) => palette256(*i as usize),
        Color::Named(n) => palette256(named_index(*n)),
    }
}
