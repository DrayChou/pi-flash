//! 搜索结果面板（083）：Zed 多缓冲结果编辑器的视觉等价物。
//!
//! Zed 把结果摆进一个 `MultiBuffer`（每个命中一个 excerpt，带文件头块、
//! 行号 gutter、语法高亮、命中 range 高亮）。PF 不复刻编辑器内核，只复刻
//! **视觉结构与交互**：
//!
//! - 文件组头：文件图标 + 相对路径 + 该文件命中行数（Zed `Block::BufferHeader`
//!   的位置与信息）；
//! - 摘录行：等宽等高，左侧行号 gutter，命中行整行提亮 + 命中区间底色高亮
//!   （Zed excerpt 的 primary range 高亮）；
//! - 上下文行与命中行同高（Zed excerpt 的上下文行同款淡化）；
//! - 活跃命中：整行 accent 洗底 + 左缘 2px accent 竖条（Zed 结果编辑器的
//!   选中命中视觉）；
//! - 列表等高虚拟化（`vlist`）：一万行也只建视口内的行。
//!
//! 为什么不给结果区套编辑器：PF 的编辑器底座是 gpui-component `InputState`，
//! 它没有多缓冲/摘录/块系统；为了「结果可读」去造一套 multibuffer 内核是
//! 本末倒置（Zed 的 multimuffer 是整个编辑器宇宙的一部分）。等宽等高列表
//! 在结果浏览这个具体场景里信息密度和滚动性能都持平甚至更好。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, WeakEntity, div, px, rgb};

use super::{LIST_ID, Phase, ROW_H, Row};
use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::ui::vlist::{VListHeight, vlist};
use std::sync::Arc;

/// 行号 gutter 宽（3 位数以内的行号右对齐，Zed gutter 同宽量级）
const GUTTER_W: f32 = 44.;

/// 结果区：空态文案 或 虚拟化结果列表。
pub(super) fn view(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let ui = &chat.ws.search;
    if ui.rows.is_empty() {
        // 查询词走镜像字段（渲染帧不读输入框实体）
        let has_query = !ui.query_text.is_empty();
        let text = super::panel::empty_hint(ui.phase, has_query);
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
    // 每帧只克隆 Arc（行模型跨帧复用；追加走 Arc::make_mut）
    let rows: Arc<Vec<Row>> = ui.rows.clone();
    let active = ui.active;
    let weak_rows = weak.clone();
    vlist(
        LIST_ID,
        rows.len(),
        ROW_H,
        VListHeight::Fill,
        false,
        true,
        tr("search.no_results"),
        None,
        move |ix, _window, cx| render_row(&rows, ix, active, &weak_rows, cx),
    )
}

fn render_row(
    rows: &[Row],
    ix: usize,
    active: Option<usize>,
    weak: &WeakEntity<Chat>,
    _cx: &mut gpui::App,
) -> AnyElement {
    let t = T();
    let Some(row) = rows.get(ix) else {
        return div().into_any_element();
    };
    match row {
        Row::Header {
            rel,
            count,
            path_only,
        } => {
            let t2 = t;
            let name: SharedString = rel.rsplit('/').next().unwrap_or(rel).to_string().into();
            let dir: SharedString = {
                let name_len = name.len();
                let d = &rel[..rel.len().saturating_sub(name_len)];
                d.trim_end_matches('/').to_string().into()
            };
            let path_only = *path_only;
            let count = *count;
            let rel_click = rel.clone();
            let weak = weak.clone();
            div()
                .id(SharedString::from(format!("psh-{ix}")))
                .w_full()
                .h(px(ROW_H))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(6.))
                .border_t_1()
                .border_color(rgb(t2.border))
                .bg(rgb(t2.bg_subtle))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(t2.bg_hover)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, c| {
                    let rel = rel_click.clone();
                    let _ = weak.update(c, |chat, cx| {
                        let Some(cwd) = chat.ws.cwd.clone() else {
                            return;
                        };
                        let abs = cwd.join(&rel);
                        chat.ws.pending_jump = Some((abs.clone(), 1));
                        chat.open_file_tab(abs, cx);
                    });
                })
                .child(crate::ui::icon_path(
                    crate::services::file_icons::get_icon(&std::path::PathBuf::from(rel)),
                    14.,
                    t2.text_dim,
                ))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(crate::appearance::ui_size(11.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t2.text))
                        .child(name),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t2.text_faint))
                        .child(dir),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(crate::appearance::ui_size(10.5))
                        .text_color(rgb(t2.text_dim))
                        .child(SharedString::from(if path_only {
                            tr("search.path_match").to_string()
                        } else {
                            count.to_string()
                        })),
                )
                .into_any_element()
        }
        Row::Line {
            line,
            text,
            spans,
            hit,
            ordinal,
        } => {
            let is_active = *hit && *ordinal == active && active.is_some();
            let row_weak = weak.clone();
            let mut el = div()
                .id(SharedString::from(format!("psl-{ix}")))
                .w_full()
                .h(px(ROW_H))
                .flex()
                .items_center()
                .when(is_active, |d| {
                    // 活跃命中：accent 洗底 + 左缘竖条（Zed 选中命中视觉）
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
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, c| {
                    let _ = row_weak.update(c, |chat, cx| chat.search_open_row(ix, cx));
                })
                // 行号 gutter（右对齐，等宽）
                .child(
                    div()
                        .w(px(GUTTER_W))
                        .flex_shrink_0()
                        .pr(px(8.))
                        .text_align(gpui::TextAlign::Right)
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.5))
                        .text_color(rgb(if *hit { t.text_dim } else { t.text_faint }))
                        .child(SharedString::from(line.to_string())),
                )
                // 代码正文：命中区间底色高亮（Zed primary range highlight）
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(11.5))
                        .text_color(rgb(if *hit { t.text } else { t.text_dim }))
                        .child(highlighted(text, spans, t)),
                );
            // 上下文行与命中行的差别只在文字亮度（Zed 同：上下文不淡化到
            // 看不清，但命中行更亮）
            if !*hit {
                el = el.opacity(0.92);
            }
            el.into_any_element()
        }
    }
}

/// 把一行按命中区间切成「普通段 + 高亮段」。
fn highlighted(text: &str, spans: &[std::ops::Range<usize>], t: &'static Theme) -> AnyElement {
    let mut row = div().flex().items_center();
    if spans.is_empty() {
        return row
            .child(SharedString::from(text.to_string()))
            .into_any_element();
    }
    let mut cursor = 0usize;
    // 命中高亮底色：accent 22%（Zed `search_match_background` 的等价物）
    let wash: gpui::Hsla = {
        let a: gpui::Hsla = rgb(t.accent).into();
        gpui::hsla(a.h, a.s, a.l, 0.22)
    };
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
