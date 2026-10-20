//! 搜索结果面板（083）——**对齐现有 editor 内容区**，不自造代码渲染。
//!
//! 2026-10-21 用户返工意见（本文件按这四条重写）：
//! 1. 搜索框不加 border；
//! 2. 组头黑色 = 配色错误（`bg_subtle` 是 8 位 RRGGBBAA，必须走 `rgba()`，
//!    拿 `rgb()` 读会变成近黑）；
//! 3. 文件块 100% 宽度（去掉左右外边距/圆角/边框）；
//! 4. 内容区复用 editor 的渲染轮子，与 editor 内容区对齐。
//!
//! 第 4 条怎么落实：**字体/字号/行高/底色/语法高亮全部取 editor 的同一套**
//! （`appearance::file_font()` + `line_height(1.618)` = Zed BufferLineHeight::
//! Comfortable（见 editor/view.rs 同款注释）+ `t.bg` + 现有
//! `editor::markdown::highlight::cached_highlight_segments`（syntect + 线程局部
//! LRU，markdown 代码块用的就是它）。不新写高亮器、不新写行距/字体常量。
//!
//! 唯一无法"复用 editor 本体"的地方是**行号**：Zed 的结果是 MultiBuffer
//! （每个命中一个 excerpt，行号是文件真值）；PF 的 editor 底座是
//! gpui-component `InputState`，没有多缓冲/摘录概念，把结果拼进一个编辑器
//! 就只剩 1..N 的局部行号。所以行号由本文件按文件真值绘制（这是数据展示，
//! 不是代码渲染）。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, WeakEntity, div, px, rgb};
use std::rc::Rc;

use super::{Phase, ResultBlock, ResultLine};
use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;

/// 行号 gutter 宽（editor 行号列同量级）
const GUTTER_W: f32 = 46.;
/// 段间距（同文件多段摘录）
const SEGMENT_GAP: f32 = 6.;

/// 打开结果的动作：`(块下标, 目标行, 命中序号, &mut App)`。抽成闭包是为了让
/// 渲染层不依赖 `Chat`（无头布局测试能直接跑生产渲染代码，见 tests）。
type OnOpen = Rc<dyn Fn(usize, u32, Option<usize>, &mut gpui::App)>;

/// editor 行高倍数（`editor/view.rs`：Zed `BufferLineHeight::Comfortable`）
const LINE_HEIGHT: f32 = 1.618;
/// 一行的高度（随「文档字号」设置缩放，与 editor 内容区同源）
fn line_h() -> f32 {
    (crate::appearance::file_font().size * LINE_HEIGHT).round()
}

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
    let on_open: OnOpen = Rc::new(move |block_ix, line, ordinal, cx| {
        let _ = weak_rows.update(cx, |chat, cx| {
            chat.search_open_block(block_ix, line, ordinal, cx)
        });
    });
    results_shell(
        t,
        gpui::list(ui.list.clone(), move |ix, _window, _cx| {
            match blocks.get(ix) {
                Some(block) => render_block(ix, block, active, &on_open),
                None => div().into_any_element(),
            }
        })
        // list 自身要 flex_1 从父列拿高度（Auto 尺寸下会被布局成 0 高）
        .flex_1()
        .min_h_0(),
    )
    .into_any_element()
}

/// 结果区外壳。**必须是 flex 列**：gpui 的 `div` 默认不是 flex 容器，缺
/// `flex().flex_col()` 时内部 list 拿不到定高 → 高度 0，结果全丢（2026-10-21
/// 用户实测「只显示计数 1/222 不显示内容」的根因）。与 md 预览
/// （`editor/markdown/doc.rs`）的列表容器配方逐字一致；抽成函数是为了让
/// [`tests::results_shell_lays_out_list`] 能直接测生产代码的布局。
fn results_shell(t: &'static Theme, list: impl gpui::IntoElement) -> gpui::Stateful<gpui::Div> {
    div()
        .id("search-results")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .child(list)
}

/// 一个文件结果块：组头 + 若干摘录段（**整宽**，无外边距/圆角/边框）。
fn render_block(
    block_ix: usize,
    block: &ResultBlock,
    active: Option<usize>,
    on_open: &OnOpen,
) -> AnyElement {
    let t = T();
    let card = div()
        .id(SharedString::from(format!("psb-{block_ix}")))
        .w_full()
        .flex()
        .flex_col()
        .child(header_row(block_ix, block, on_open));
    if block.path_only {
        return card.into_any_element();
    }
    let mut card = card;
    // 语法高亮的语言 token：直接用扩展名（syntect `find_syntax_by_token`
    // 同时认扩展名与语法名，未知扩展回落纯文本——与 markdown 代码块同路径）
    let lang = std::path::Path::new(&block.rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    for (seg_ix, segment) in block.segments.iter().enumerate() {
        if seg_ix > 0 {
            // 段间分隔（同文件多段摘录）
            card = card.child(
                div()
                    .w_full()
                    .h(px(SEGMENT_GAP))
                    .bg(rgb(t.bg))
                    .border_t_1()
                    .border_color(rgb(t.border)),
            );
        }
        // 整段一次性过 syntect（跨行上下文比逐行准），再按行切分
        let seg_text = segment
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let runs =
            crate::editor::markdown::highlight::cached_highlight_segments(&seg_text, &lang, t.dark);
        let per_line = split_runs_by_line(&runs, segment.len());
        for (line_ix, line) in segment.iter().enumerate() {
            card = card.child(render_line(
                block_ix,
                line,
                active,
                on_open,
                seg_ix,
                line_ix,
                per_line.get(line_ix).map(Vec::as_slice).unwrap_or(&[]),
            ));
        }
    }
    card.into_any_element()
}

/// 组头：▾ + 文件图标 + 文件名 + 路径 + 命中数 + 「打开文件 Alt+Enter」。
fn header_row(block_ix: usize, block: &ResultBlock, on_open: &OnOpen) -> AnyElement {
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
    let click = on_open.clone();
    div()
        .id(SharedString::from(format!("psb-h-{block_ix}")))
        .h(px(28.))
        .w_full()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        // 组头底：bg_panel（`bg_subtle` 是 RRGGBBAA，rgb() 读会变近黑——2026-10-21
        // 用户报的「黑色是配色错误」根因）
        .bg(rgb(t.bg_panel))
        .border_b_1()
        .border_color(rgb(t.border))
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            click(block_ix, 1, None, cx);
        })
        .child(crate::ui::icon(
            "chevron-down",
            crate::ui::tokens::icon::XS,
            t.text_dim,
        ))
        .child(crate::ui::icon_path(
            crate::services::file_icons::get_icon(&std::path::PathBuf::from(&block.rel)),
            14.,
            t.text_dim,
        ))
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::appearance::file_font().family)
                .text_size(px(crate::appearance::file_font().size))
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
                .font_family(crate::appearance::panel_font().family)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_faint))
                .child(dir),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::appearance::panel_font().family)
                .text_size(crate::appearance::ui_size(10.5))
                .text_color(rgb(t.text_dim))
                .child(hint),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(8.))
                .font_family(crate::appearance::panel_font().family)
                .text_size(crate::appearance::ui_size(10.5))
                .text_color(rgb(t.text_faint))
                .child(tr("search.open_file").to_string())
                .child(crate::ui::kit::keycap("alt-enter")),
        )
        .into_any_element()
}

/// 摘录行：行号 gutter + 正文（editor 同族同号 + syntect 高亮 + 命中洗底）。
fn render_line(
    block_ix: usize,
    line: &ResultLine,
    active: Option<usize>,
    on_open: &OnOpen,
    seg_ix: usize,
    line_ix: usize,
    runs: &[(String, [u8; 3])],
) -> AnyElement {
    let t = T();
    let font = crate::appearance::file_font();
    let is_active = line.hit && active.is_some() && line.ordinal == active;
    let click = on_open.clone();
    let ordinal = line.ordinal;
    let line_no = line.line;
    let row_h = line_h();
    let mut el = div()
        .id(SharedString::from(format!(
            "psb-l-{block_ix}-{seg_ix}-{line_ix}"
        )))
        .h(px(row_h))
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
            click(block_ix, line_no, ordinal, cx);
        })
        // 行号 gutter：与 editor 行号同族同号，右对齐
        .child(
            div()
                .w(px(GUTTER_W))
                .flex_shrink_0()
                .pr(px(10.))
                .text_align(gpui::TextAlign::Right)
                .font_family(font.family.clone())
                .text_size(px(font.size * 0.9))
                .text_color(rgb(if line.hit { t.text_dim } else { t.text_faint }))
                .child(SharedString::from(line.line.to_string())),
        )
        // 正文：editor 同族/同号/同行高；syntect 高亮 + 命中区间洗底
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .line_height(gpui::relative(LINE_HEIGHT))
                .font_family(font.family)
                .text_size(px(font.size))
                .text_color(rgb(if line.hit { t.text } else { t.text_dim }))
                .child(decorated_line(&line.text, runs, &line.spans, t)),
        );
    if !line.hit {
        el = el.opacity(0.9);
    }
    el.into_any_element()
}

/// syntect run 串 → 按行切分（`highlight_segments` 保证 run 不跨行，
/// 行尾 `\n` 落在该行最后一段里，这里剥掉）。
fn split_runs_by_line(
    runs: &[(String, [u8; 3])],
    line_count: usize,
) -> Vec<Vec<(String, [u8; 3])>> {
    let mut out: Vec<Vec<(String, [u8; 3])>> = vec![Vec::new(); line_count];
    let mut line_ix = 0usize;
    for (text, color) in runs {
        let mut rest = text.as_str();
        while let Some(nl) = rest.find('\n') {
            if line_ix < line_count && nl > 0 {
                out[line_ix].push((rest[..nl].to_string(), *color));
            }
            rest = &rest[nl + 1..];
            line_ix += 1;
        }
        if !rest.is_empty() && line_ix < line_count {
            out[line_ix].push((rest.to_string(), *color));
        }
    }
    out
}

/// 把一行的语法高亮 run 与命中区间叠在一起（命中段 = accent 洗底 + 正文色）。
fn decorated_line(
    line: &str,
    runs: &[(String, [u8; 3])],
    spans: &[std::ops::Range<usize>],
    t: &'static Theme,
) -> AnyElement {
    let wash: gpui::Hsla = {
        let a: gpui::Hsla = rgb(t.accent).into();
        gpui::hsla(a.h, a.s, a.l, 0.22)
    };
    let is_hit = |pos: usize| spans.iter().any(|s| s.contains(&pos));
    let mut row = div().flex().items_center();
    // 没有任何语法 run（空行/高亮器没吐东西）时也照样画命中洗底
    let pieces: Vec<(String, Option<[u8; 3]>)> = if runs.is_empty() {
        vec![(line.to_string(), None)]
    } else {
        runs.iter()
            .map(|(text, color)| (text.clone(), Some(*color)))
            .collect()
    };
    let mut pos = 0usize;
    for (text, color) in pieces {
        // 逐字符分段：命中/非命中边界处切开（保持语法色不变）
        let mut buf = String::new();
        let mut buf_hit = false;
        for ch in text.chars() {
            let hit = is_hit(pos);
            if hit != buf_hit && !buf.is_empty() {
                row = row.child(styled_piece(&buf, buf_hit, color, wash, t));
                buf.clear();
            }
            buf_hit = hit;
            buf.push(ch);
            pos += ch.len_utf8();
        }
        if !buf.is_empty() {
            row = row.child(styled_piece(&buf, buf_hit, color, wash, t));
        }
    }
    row.into_any_element()
}

fn styled_piece(
    text: &str,
    hit: bool,
    color: Option<[u8; 3]>,
    wash: gpui::Hsla,
    t: &'static Theme,
) -> AnyElement {
    let fg = if hit {
        rgb(t.text)
    } else {
        match color {
            Some([r, g, b]) => gpui::rgb(u32::from_be_bytes([0, r, g, b])),
            None => rgb(t.text),
        }
    };
    let el = div()
        .text_color(fg)
        .child(SharedString::from(text.to_string()));
    if hit {
        el.rounded(px(2.))
            .bg(gpui::Fill::from(wash))
            .into_any_element()
    } else {
        el.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{
        AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled,
        TestAppContext, VisualTestContext, Window, div, point, px, size,
    };

    use super::{ResultBlock, ResultLine, results_shell};

    fn sample_blocks() -> Vec<ResultBlock> {
        vec![
            // 内容命中：两段摘录（行号是文件真值）
            ResultBlock {
                rel: "docs/note.md".into(),
                path_only: false,
                count: 2,
                segments: vec![
                    vec![
                        ResultLine {
                            line: 10,
                            text: "# 标题".into(),
                            spans: Vec::new(),
                            hit: false,
                            ordinal: None,
                        },
                        ResultLine {
                            line: 11,
                            text: "命中 openai 关键字".into(),
                            spans: vec![3..9],
                            hit: true,
                            ordinal: Some(0),
                        },
                    ],
                    vec![ResultLine {
                        line: 30,
                        text: "另一段 openai".into(),
                        spans: vec![3..9],
                        hit: true,
                        ordinal: Some(1),
                    }],
                ],
            },
            // 文件名命中（无内容行）
            ResultBlock {
                rel: "src/openai_client.rs".into(),
                path_only: true,
                count: 0,
                segments: Vec::new(),
            },
        ]
    }

    /// 记下本帧真正构建了哪些块 + 块高（列表有高度才会构建 → 布局探针）。
    struct TestView {
        seen: Rc<RefCell<Vec<usize>>>,
        heights: Rc<RefCell<Vec<f32>>>,
        list: gpui::ListState,
        blocks: Rc<Vec<ResultBlock>>,
    }

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let seen = self.seen.clone();
            let heights = self.heights.clone();
            let blocks = self.blocks.clone();
            let on_open: super::OnOpen =
                Rc::new(|_: usize, _: u32, _: Option<usize>, _: &mut gpui::App| {});
            // 外层 = 搜索面板内容列（定高，模拟 content 区）
            div()
                .w(px(600.))
                .h(px(240.))
                .flex()
                .flex_col()
                .child(results_shell(
                    crate::appearance::theme::theme(),
                    gpui::list(self.list.clone(), move |ix, _window, _cx| {
                        seen.borrow_mut().push(ix);
                        match blocks.get(ix) {
                            Some(block) => {
                                let el = super::render_block(ix, block, None, &on_open);
                                // 单块高度：包一层量高（列表本身也会测一次）
                                div()
                                    .w_full()
                                    .on_children_prepainted({
                                        let heights = heights.clone();
                                        move |bounds, _, _| {
                                            if let Some(b) = bounds.first() {
                                                heights.borrow_mut().push(f32::from(b.size.height));
                                            }
                                        }
                                    })
                                    .child(el)
                                    .into_any_element()
                            }
                            None => div().into_any_element(),
                        }
                    })
                    .flex_1()
                    .min_h_0(),
                ))
        }
    }

    fn frame(
        cx: &mut VisualTestContext,
        seen: &Rc<RefCell<Vec<usize>>>,
        heights: &Rc<RefCell<Vec<f32>>>,
    ) -> Vec<usize> {
        seen.borrow_mut().clear();
        heights.borrow_mut().clear();
        let blocks = Rc::new(sample_blocks());
        cx.draw::<Entity<TestView>>(point(px(0.), px(0.)), size(px(600.), px(240.)), |_, cx| {
            cx.new(|_| TestView {
                seen: seen.clone(),
                heights: heights.clone(),
                list: gpui::ListState::new(blocks.len(), gpui::ListAlignment::Top, px(1000.)),
                blocks,
            })
        });
        let mut rows = seen.borrow().clone();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// 回归锁（2026-10-21「只显示计数不显示内容」）：结果区容器必须把高度
    /// 传给列表，且真实块渲染出来必须有像素高度（0 高 = 用户看到的整片空白）。
    #[gpui::test]
    fn real_blocks_render_with_height(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<usize>>> = Default::default();
        let heights: Rc<RefCell<Vec<f32>>> = Default::default();
        let rows = frame(window, &seen, &heights);
        assert_eq!(rows, vec![0, 1], "两个结果块都必须被构建");
        let hs = heights.borrow().clone();
        assert!(!hs.is_empty(), "块必须完成一次 prepaint（有实际 bounds）");
        assert!(
            hs.iter().all(|h| *h > 0.),
            "块高度必须 > 0，实测 {hs:?}（0 = 结果区空白）"
        );
    }
}
