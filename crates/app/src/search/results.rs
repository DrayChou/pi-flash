//! 搜索结果面板（083）——Zed 结果编辑器的视觉等价物。
//!
//! 逐项对照 Zed 截图（用户 2026-10-21 定稿「zed 的代码直接抄就好了」）：
//!
//! ```text
//! ┌ 文件块（圆角卡片，块间 8px 间隔）──────────────────────────────────┐
//! │ ▾  AGENTS.md  .. 目录/路径 ..                    打开文件  Alt+Enter │  ← 组头
//! │ 13 │   - 环境变量 ...                                              │
//! │ 15 │   - UI 验证一律走 ...                        ← 命中区间洗底     │
//! │ 16 │                                                               │
//! │ ⋮  （同文件另一段摘录 = 段间 6px 间隙 + 分隔线）                     │
//! └───────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Zed 用的是多缓冲 + `Block::BufferHeader` + excerpt 头尾控制钮，那套渲染在
//! `crates/editor/src/element.rs` + `display_map/block_map.rs` 里，依赖它整
//! 个编辑器内核（MultiBuffer/DisplayMap）；PF 不复刻编辑器内核，复刻的是
//! **同一套视觉词汇**：分组卡片、文件头行（可点 = 打开文件）、行号 gutter、
//! 命中区间高亮、段间分隔。
//!
//! 列表 = 变高块列表（`ListState` + `list()`，每块一个 item）：一万个文件时
//! 只构建视口内的块，滚动与 md 预览同一套虚拟化机制。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, WeakEntity, div, px, rgb};

use super::{HEADER_H, LINE_H, Phase, ResultBlock, ResultLine};
use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;

/// 行号 gutter 宽（Zed 结果编辑器的 gutter 同量级）
const GUTTER_W: f32 = 40.;
/// 块间距
const BLOCK_GAP: f32 = 8.;
/// 段间距（同文件多段摘录）
const SEGMENT_GAP: f32 = 6.;

/// 结果区：空态提示 或 块列表。
pub(super) fn view(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let ui = &chat.ws.search;
    if ui.blocks.is_empty() {
        let text = super::panel::empty_hint(ui.phase, !ui.query_text.is_empty());
        let mut hint = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(crate::ui::tokens::space::SP3))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(SharedString::from(text.to_string()));
        if ui.phase == Phase::Running {
            hint = hint.child(crate::ui::spinner(14., t.accent));
        }
        return hint.into_any_element();
    }
    let blocks = ui.blocks.clone();
    let active = ui.active;
    let weak_rows = weak.clone();
    div()
        .id("search-results")
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .child(
            gpui::list(ui.list.clone(), move |ix, _window, _cx| {
                match blocks.get(ix) {
                    Some(block) => render_block(ix, block, active, &weak_rows),
                    None => div().into_any_element(),
                }
            })
            // list 自身要 flex_1 从父列拿高度（Auto 尺寸下会被布局成 0 高）
            .flex_1()
            .min_h_0(),
        )
        .into_any_element()
}

/// 一个文件结果块：组头 + 若干摘录段。
fn render_block(
    block_ix: usize,
    block: &ResultBlock,
    active: Option<usize>,
    weak: &WeakEntity<Chat>,
) -> AnyElement {
    let t = T();
    let mut card = div()
        .id(SharedString::from(format!("psb-{block_ix}")))
        .mx(px(8.))
        .mb(px(BLOCK_GAP))
        .flex()
        .flex_col()
        .rounded(px(crate::ui::tokens::radius::CTRL))
        .border_1()
        .border_color(rgb(t.border))
        .overflow_hidden()
        // 组头：点 = 打开文件（Zed 组头右侧的 Open File 同义，整行可点）
        .child(header_row(block_ix, block, weak));
    if block.path_only {
        // 文件名命中：没有摘录，只有头
        return card.into_any_element();
    }
    for (seg_ix, segment) in block.segments.iter().enumerate() {
        if seg_ix > 0 {
            // 段间分隔（Zed 里是两个 excerpt 之间的头尾控制条）
            card = card.child(
                div()
                    .h(px(SEGMENT_GAP))
                    .w_full()
                    .bg(rgb(t.bg_subtle))
                    .border_t_1()
                    .border_color(rgb(t.border)),
            );
        }
        for (line_ix, line) in segment.iter().enumerate() {
            card = card.child(render_line(block_ix, line, active, weak, seg_ix, line_ix));
        }
    }
    card.into_any_element()
}

/// 组头：▾ + 文件图标 + 文件名 + 路径 + 右侧「打开文件 Alt+Enter」。
fn header_row(block_ix: usize, block: &ResultBlock, weak: &WeakEntity<Chat>) -> AnyElement {
    let t = T();
    let name: SharedString = block
        .rel
        .rsplit('/')
        .next()
        .unwrap_or(&block.rel)
        .to_string()
        .into();
    let dir: SharedString = {
        let d = &block.rel[..block.rel.len().saturating_sub(name.len())];
        d.trim_end_matches('/').to_string().into()
    };
    let hint: SharedString = if block.path_only {
        tr("search.path_match").into()
    } else {
        crate::i18n::tf("search.match_count", &[("n", block.count.to_string())]).into()
    };
    let weak_row = weak.clone();
    div()
        .id(SharedString::from(format!("psb-h-{block_ix}")))
        .h(px(HEADER_H))
        .w_full()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .bg(rgb(t.bg_subtle))
        .border_b_1()
        .border_color(rgb(t.border))
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let _ = weak_row.update(cx, |chat, cx| chat.search_open_block(block_ix, 1, None, cx));
        })
        .child(icon("chevron-down", t.text_dim))
        .child(crate::ui::icon_path(
            crate::services::file_icons::get_icon(&std::path::PathBuf::from(&block.rel)),
            14.,
            t.text_dim,
        ))
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(name),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_faint))
                .child(dir),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(10.5))
                .text_color(rgb(t.text_dim))
                .child(hint),
        )
        // Zed 组头右侧的「Open File  Alt-Enter」同款：动作提示 + 键帽
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(8.))
                .text_size(crate::appearance::ui_size(10.5))
                .text_color(rgb(t.text_faint))
                .child(tr("search.open_file").to_string())
                .child(crate::ui::kit::keycap("alt-enter")),
        )
        .into_any_element()
}

/// 摘录行：行号 gutter + 正文（命中区间洗底）。
fn render_line(
    block_ix: usize,
    line: &ResultLine,
    active: Option<usize>,
    weak: &WeakEntity<Chat>,
    seg_ix: usize,
    line_ix: usize,
) -> AnyElement {
    let t = T();
    let is_active = line.hit && active.is_some() && line.ordinal == active;
    let weak_row = weak.clone();
    let ordinal = line.ordinal;
    let line_no = line.line;
    let mut el = div()
        .id(SharedString::from(format!(
            "psb-l-{block_ix}-{seg_ix}-{line_ix}"
        )))
        .h(px(LINE_H))
        .w_full()
        .flex()
        .items_center()
        .when(is_active, |d| {
            let a: gpui::Hsla = rgb(t.accent).into();
            d.bg(gpui::hsla(a.h, a.s, a.l, 0.13))
                .border_l_2()
                .border_color(rgb(t.accent))
        })
        .when(!is_active, |d| {
            d.border_l_2().border_color(gpui::rgba(0x00000000))
        })
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let _ = weak_row.update(cx, |chat, cx| {
                chat.search_open_block(block_ix, line_no, ordinal, cx)
            });
        })
        // 行号 gutter（右对齐；命中行亮一档，上下文行暗）
        .child(
            div()
                .w(px(GUTTER_W))
                .flex_shrink_0()
                .pr(px(8.))
                .text_align(gpui::TextAlign::Right)
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(10.5))
                .text_color(rgb(if line.hit { t.text_dim } else { t.text_faint }))
                .child(SharedString::from(line.line.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.5))
                .text_color(rgb(if line.hit { t.text } else { t.text_dim }))
                .child(highlighted(&line.text, &line.spans, t)),
        );
    if !line.hit {
        el = el.opacity(0.92);
    }
    el.into_any_element()
}

/// 一行按命中区间切成「普通段 + 高亮段」。
fn highlighted(text: &str, spans: &[std::ops::Range<usize>], t: &'static Theme) -> AnyElement {
    let mut row = div().flex().items_center();
    if spans.is_empty() {
        return row
            .child(SharedString::from(text.to_string()))
            .into_any_element();
    }
    let wash: gpui::Hsla = {
        let a: gpui::Hsla = rgb(t.accent).into();
        gpui::hsla(a.h, a.s, a.l, 0.22)
    };
    let mut cursor = 0usize;
    for span in spans {
        let start = span.start.min(text.len());
        let end = span.end.min(text.len());
        if start > cursor {
            row = row.child(SharedString::from(text[cursor..start].to_string()));
        }
        if end > start {
            row = row.child(
                div()
                    .rounded(px(crate::ui::tokens::radius::XS))
                    .bg(gpui::Fill::from(wash))
                    .text_color(rgb(t.text))
                    .child(SharedString::from(text[start..end].to_string())),
            );
        }
        cursor = end.max(cursor);
    }
    if cursor < text.len() {
        row = row.child(SharedString::from(text[cursor..].to_string()));
    }
    row.into_any_element()
}

fn icon(name: &'static str, color: u32) -> AnyElement {
    crate::ui::icon(name, crate::ui::tokens::icon::XS, color)
}
