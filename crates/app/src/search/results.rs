//! 搜索结果面板（083）——**对齐 Zed 的排版**，复用 editor 的渲染轮子。
//!
//! 2026-10-21 用户两轮返工后的定稿口径：
//! 1. 搜索框不加 border（`TextInput::flat()`）；
//! 2. 组头不能是黑色（`bg_subtle` 是 RRGGBBAA，`rgb()` 读会成近黑 → 用 `bg_panel`）；
//! 3. 文件块 100% 宽度（无外边距/圆角/边框/块间空隙）；
//! 4. 内容区与 editor 内容区对齐：字体/字号/语法高亮都取 editor 那一套
//!    （`appearance::file_font()` + `editor::markdown::highlight::cached_highlight_segments`）；
//! 5. **排版照 Zed**：长行**折行**、行高 1.5、组头高 ≈1.35 行且用正文字号、
//!    行号列随字号加宽（`字号 × 3.4`）不与正文贴脸、命中底色用主题的中性
//!    selection 色（`bg_selected`，不是 accent 洗底）。
//!
//! 显示规则（在 `search/mod.rs` 落实）：**标题命中且内容也命中的文件只显示
//! 内容组；只有标题命中才显示文件栏**——所以这里 `path_only` 的块是纯文件栏。
//!
//! 行号仍是按文件真值画的：Zed 的行号来自 MultiBuffer（每命中一个 excerpt），
//! 而 PF 的编辑器底座是 gpui-component `InputState`，没有多缓冲概念。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, StyledText, WeakEntity, div, px, rgb};
use std::ops::Range;
use std::rc::Rc;

use super::{Phase, ResultBlock, ResultLine};
use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;

/// 打开结果的动作：`(块下标, 目标行, 命中序号, &mut App)`。抽成闭包是为了让
/// 渲染层不依赖 `Chat`（无头布局测试能直接跑生产渲染代码，见 tests）。
type OnOpen = Rc<dyn Fn(usize, u32, Option<usize>, &mut gpui::App)>;

/// 行高倍数：Zed 结果编辑器偏紧凑（`editor/view.rs` 的 1.618 是文档阅读态，
/// 搜索结果按 1.5 收，长行折行后不显松散）
const LINE_HEIGHT: f32 = 1.5;

/// 一行的高度（随「文档字号」设置缩放，与 editor 内容区同源）
fn line_h() -> f32 {
    (crate::appearance::file_font().size * LINE_HEIGHT).round()
}

/// 行号列宽：随字号缩放（44/13 ≈ 3.4 字符），并留出 12px 与正文的间隙——
/// 固定 46px 在 16px 字号下会贴脸（用户 2026-10-21 实测「行号拥挤」）
fn gutter_w() -> f32 {
    (crate::appearance::file_font().size * 3.4).round().max(44.)
}

/// 组头高：≈1.75 行（Zed 的 excerpt 头明显高于代码行），字号用正文字号
fn header_h() -> f32 {
    (line_h() * 1.75).round().max(line_h() + 14.)
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
/// [`tests::real_blocks_render_with_height`] 能直接测生产代码的布局。
fn results_shell(t: &'static Theme, list: impl gpui::IntoElement) -> gpui::Stateful<gpui::Div> {
    div()
        .id("search-results")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // 第一条文件头与搜索框留 5px 间隙（2026-10-21 用户定案）
        .pt(px(5.))
        .child(list)
}

/// 一个文件结果块：组头 + 若干摘录段（整宽、无外边距、无圆角、块间无空隙）。
fn render_block(
    block_ix: usize,
    block: &ResultBlock,
    active: Option<usize>,
    on_open: &OnOpen,
) -> AnyElement {
    let t = T();
    let mut card = div()
        .id(SharedString::from(format!("psb-{block_ix}")))
        .w_full()
        .flex()
        .flex_col()
        .child(header_row(block_ix, block, on_open));
    if block.path_only {
        return card.into_any_element();
    }
    // 语法高亮的语言 token：直接用扩展名（syntect `find_syntax_by_token`
    // 同时认扩展名与语法名，未知扩展回落纯文本——与 markdown 代码块同路径）
    let lang = std::path::Path::new(&block.rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();
    for (seg_ix, segment) in block.segments.iter().enumerate() {
        if seg_ix > 0 {
            // 段间分隔：1px 细线（Zed 的 excerpt 之间是头尾控制条，PF 用细线
            // 表达同一件事，不占高度、不刷底色）
            card = card.child(div().w_full().h(px(1.)).bg(rgb(t.border)));
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

/// 组头（文件操作栏）：`▾ 文件名 路径 …… N 处  打开文件 Alt+Enter`。
/// 高度 ≈1.35 行、字号 = 正文，左侧 `px 12`、右侧留白加大（此前固定 28px
/// + 小字号 → 用户评「抠抠搜搜」）。
fn header_row(block_ix: usize, block: &ResultBlock, on_open: &OnOpen) -> AnyElement {
    let t = T();
    let font = crate::appearance::file_font();
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
        .h(px(header_h()))
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        // 组头底：bg_panel（`bg_subtle` 是 RRGGBBAA，rgb() 读会变近黑——2026-10-21
        // 用户报的「黑色是配色错误」根因）
        .bg(rgb(t.bg_panel))
        // 对齐 Zed 的文件头：**明显圆角 + 下层投影 + 无描边**
        // （此前是整宽平色带 + 1px 上边框，用户评「难看」）
        .rounded(px(8.))
        .shadow_sm()
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
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .font_weight(gpui::FontWeight::MEDIUM)
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
                .font_family(font.family.clone())
                .text_size(px(font.size * 0.95))
                .text_color(rgb(t.text_faint))
                .child(dir),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::appearance::panel_font().family)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(hint),
        )
        .into_any_element()
}

/// 摘录行：行号 gutter + 正文（editor 同族同号 + syntect 高亮 + 命中洗底）。
/// 长行**折行**（`StyledText` 原生换行），行高随内容增长，与 Zed 一致。
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
    let highlights = line_highlights(&line.text, runs, &line.spans, t);
    let mut el = div()
        .id(SharedString::from(format!(
            "psb-l-{block_ix}-{seg_ix}-{line_ix}"
        )))
        .min_h(px(row_h))
        .w_full()
        .flex()
        // 折行后行会变高：gutter 与正文都从顶对齐
        .items_start()
        .when(is_active, |d| {
            let a: gpui::Hsla = rgb(t.accent).into();
            d.bg(gpui::hsla(a.h, a.s, a.l, 0.10))
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
        // 行号 gutter：与正文同族同号，右对齐，右侧留 12px
        .child(
            div()
                .w(px(gutter_w()))
                .flex_shrink_0()
                .pr(px(12.))
                .text_align(gpui::TextAlign::Right)
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .line_height(gpui::relative(LINE_HEIGHT))
                .text_color(rgb(if line.hit { t.text_faint } else { t.text_faint }))
                .child(SharedString::from(line.line.to_string())),
        )
        // 正文：editor 同族/同号/同行高；syntect 高亮 + 命中底色（StyledText
        // 原生折行，不再逐 run 拼 div —— 逐 run 拼的节点无法换行，长行被裁切）
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(px(8.))
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .line_height(gpui::relative(LINE_HEIGHT))
                .text_color(rgb(if line.hit { t.text } else { t.text_muted }))
                .child(StyledText::new(line.text.clone()).with_highlights(highlights)),
        );
    if !line.hit {
        el = el.opacity(0.85);
    }
    el.into_any_element()
}

/// 语法 run + 命中区间 → 不重叠、按字节序的 highlight 列表（`StyledText`
/// 契约：run 必须按序且不重叠）。
fn line_highlights(
    line: &str,
    runs: &[(String, [u8; 3])],
    spans: &[Range<usize>],
    t: &'static Theme,
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
    if line.is_empty() {
        return Vec::new();
    }
    // 语法色按字节查表（run 是连续切分，前缀和定位）
    let mut run_bounds = Vec::with_capacity(runs.len() + 1);
    let mut pos = 0usize;
    run_bounds.push(0usize);
    for (text, _) in runs {
        pos += text.len();
        run_bounds.push(pos.min(line.len()));
    }
    let syntax_color = |at: usize| -> Option<[u8; 3]> {
        if runs.is_empty() {
            return None;
        }
        let ix = run_bounds
            .windows(2)
            .position(|w| at >= w[0] && at < w[1])?;
        runs.get(ix).map(|(_, c)| *c)
    };
    // 边界集合：run 边界 + 命中区间边界
    let mut bounds: Vec<usize> = run_bounds.clone();
    for span in spans {
        bounds.push(span.start.min(line.len()));
        bounds.push(span.end.min(line.len()));
    }
    bounds.push(line.len());
    bounds.retain(|b| line.is_char_boundary(*b));
    bounds.sort_unstable();
    bounds.dedup();

    let match_bg: gpui::Hsla = rgb(t.bg_selected).into();
    let mut out: Vec<(Range<usize>, gpui::HighlightStyle)> = Vec::new();
    for pair in bounds.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let hit = spans.iter().any(|s| s.contains(&start));
        let mut style = gpui::HighlightStyle::default();
        if hit {
            // Zed 的命中底色是中性 selection 色（不是 accent 洗底）
            style.background_color = Some(match_bg);
            style.color = Some(rgb(t.text).into());
        } else if let Some([r, g, b]) = syntax_color(start) {
            style.color = Some(gpui::rgb(u32::from_be_bytes([0, r, g, b])).into());
        }
        out.push((start..end, style));
    }
    out
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
            // 内容命中：两段摘录（行号是文件真值）+ 一行超长文本（验证折行）
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
                        ResultLine {
                            line: 12,
                            text: "超长行 ".repeat(60),
                            spans: Vec::new(),
                            hit: false,
                            ordinal: None,
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
            // 仅文件名命中（文件栏）
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
    /// 传给列表，且真实块渲染出来必须有像素高度（0 高 = 用户看到的空白）。
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

    /// 命中区间必须进 highlight（Zed 的命中底色），且 run 不重叠、按序。
    #[gpui::test]
    fn match_spans_become_highlights(_cx: &mut TestAppContext) {
        let t = crate::appearance::theme::theme();
        let line = "abc openai def";
        let runs = vec![("abc openai def".to_string(), [200u8, 100, 50])];
        let hs = super::line_highlights(line, &runs, &[4..10], t);
        assert!(!hs.is_empty());
        let mut last_end = 0usize;
        for (range, _) in &hs {
            assert!(range.start >= last_end, "run 必须按序不重叠: {hs:?}");
            assert!(range.start < range.end);
            last_end = range.end;
        }
        assert_eq!(hs.last().map(|(r, _)| r.end), Some(line.len()));
        let hit = hs
            .iter()
            .find(|(r, _)| r.start == 4)
            .expect("命中区间必须单独成 run");
        assert!(hit.1.background_color.is_some(), "命中 run 必须带底色");
    }
}
