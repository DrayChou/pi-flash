//! 搜索结果面板（083）——**对齐 Zed 的排版**，复用 editor 的渲染轮子。
//!
//! 2026-10-21 用户两轮返工后的定稿口径：
//! 1. 搜索框不加 border（`TextInput::flat()`）；
//! 2. 组头不能是黑色（`bg_subtle` 是 RRGGBBAA，`rgb()` 读会成近黑 → 用 `bg_panel`）；
//! 3. 文件块 100% 宽度（无外边距/圆角/边框/块间空隙）；
//! 4. 内容区与 editor 内容区对齐：字体族/字号取 `appearance::file_font()`；
//! 5. **排版照 Zed**：长行折行、行高 1.5、组头圆角+投影+加高、行号列随字号
//!    加宽、两侧留白（右侧让出滚动条宽度）、命中底色用中性 selection 色；
//! 6. **不做语法着色**：结果区是单色正文（syntect 在 markdown 上会给出红/橙/
//!    橄榄一堆颜色，用户评「五花八门」）；也**无 hover 变色**、活跃命中
//!    **只有左缘竖条**不刷整行底（2026-10-21 用户定案，逐条对齐 Zed）。
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

/// 收展一个文件块：`(块下标, &mut App)`。
type OnToggle = Rc<dyn Fn(usize, &mut gpui::App)>;

/// 行高倍数：Zed 结果编辑器偏紧凑（`editor/view.rs` 的 1.618 是文档阅读态，
/// 搜索结果按 1.5 收，长行折行后不显松散）
const LINE_HEIGHT: f32 = 1.5;

/// 一行的高度（随「文档字号」设置缩放，与 editor 内容区同源）
fn line_h() -> f32 {
    (crate::appearance::file_font().size * LINE_HEIGHT).round()
}

/// 行号列几何——**照 Zed `Editor::gutter_dimensions`（editor.rs:12178-12270）**
/// 逐项算，不再拍死一个宽度：
/// - `digit_count = widest_line_number.ilog10() + 1`；
/// - `max_line_number_width = column_pixels(digit_count)`（该字体下 digit_count
///   个空格的推进宽）；
/// - `line_gutter_width = max(上面那个, ch_advance × min_line_number_digits)`，
///   默认设置 `min_line_number_digits = 4`（`assets/settings/default.json:753`）
///   ——不足 4 位也按 4 位留宽，行号位数增长时列宽才不会跳；
/// - `left_padding = ch_width`、`right_padding = ch_width`（只开行号时各 1 字符）；
/// - 列宽 = `line_gutter_width + left + right`。
///
/// 号码取**整个结果集的**最宽行号（Zed 的 `widest_line_number()` 也是整份
/// multibuffer 一个宽），否则各块行号左右错位。
#[derive(Clone, Copy, Debug)]
struct Gutter {
    /// 行号数字区宽（右对齐）
    num: f32,
    left: f32,
    right: f32,
}

impl Gutter {
    fn total(&self) -> f32 {
        self.num + self.left + self.right
    }
}

/// `min_line_number_digits`（Zed 默认 4）
const MIN_LINE_NUMBER_DIGITS: usize = 4;

fn gutter_for(blocks: &[ResultBlock], cx: &gpui::App) -> Gutter {
    let widest = blocks
        .iter()
        .flat_map(|b| b.segments.iter().flatten())
        .map(|l| l.line)
        .max()
        .unwrap_or(1);
    let digits = (vidx_digits(widest)).max(1);
    let font = crate::appearance::file_font();
    let font_id = cx
        .text_system()
        .resolve_font(&gpui::font(font.family.clone()));
    let size = px(font.size);
    let ch_adv = f32::from(
        cx.text_system()
            .ch_advance(font_id, size)
            .unwrap_or(px(font.size * 0.6)),
    );
    let ch_w = f32::from(
        cx.text_system()
            .ch_width(font_id, size)
            .unwrap_or(px(ch_adv)),
    );
    Gutter {
        num: (ch_adv * digits.max(MIN_LINE_NUMBER_DIGITS) as f32).round(),
        left: ch_w.round(),
        right: ch_w.round(),
    }
}

/// 十进制位数（`ilog10` 在 1 上返回 0，得补 1）
fn vidx_digits(mut n: u32) -> usize {
    let mut d = 1;
    while n >= 10 {
        n /= 10;
        d += 1;
    }
    d
}

// ---------------------------------------------------------------------------
// 文件头几何——**逐值照抄 Zed**，不再自己拍：
//   `crates/editor/src/editor.rs:298`  FILE_HEADER_HEIGHT = 2（行）
//   `crates/editor/src/editor.rs:299`  BUFFER_HEADER_PADDING = rems(0.25) = 4px
//   `crates/editor/src/element/header.rs:733`  .p(BUFFER_HEADER_PADDING)
//        .w_full().h(FILE_HEADER_HEIGHT * line_height)   ← 块高 = 2 行
//   `header.rs:718-760`  内条: .pl_1(4) .pr_2(8) .rounded_sm(4px) .gap_1p5(6px)
//        .border_1(border) .bg(editor_subheader_background) .hover(element_hover)
//   收展钮: `header.rs:747`  size 28×28（`rems_from_px(28)`）、rounded_xs(2px)、
//        悬停底 `element_selected`；图标 `IconSize::Small` = 14px
//   → 块高 2 行、条四周留 4px：**块与块之间自然有 8px 间隙**，条与自己的代码
//     行之间有 4px 间隙。这就是用户问的「间隙」。
// ---------------------------------------------------------------------------

/// 文件头**块**高 = 2 行（Zed `FILE_HEADER_HEIGHT`）
fn header_h() -> f32 {
    (line_h() * 2.).round()
}

/// 块内留白 = Zed `BUFFER_HEADER_PADDING`
pub(super) const HEADER_PAD: f32 = 4.;

/// 结果区：空态（Zed 的 landing page）或块列表。
pub(super) fn view(
    chat: &mut Chat,
    weak: &WeakEntity<Chat>,
    cx: &gpui::App,
    t: &'static Theme,
) -> AnyElement {
    if chat.ws.search.blocks.is_empty() {
        return landing(chat, weak, t);
    }
    let ui = &chat.ws.search;
    let blocks = ui.blocks.clone();
    let collapsed = ui.collapsed.clone();
    let active = ui.active;
    let list = ui.list.clone();
    let scrollbar = ui.scrollbar.clone();
    let gutter = gutter_for(&blocks, cx);
    let weak_rows = weak.clone();
    let weak_toggle = weak.clone();
    let on_toggle: OnToggle = Rc::new(move |block_ix, cx| {
        let _ = weak_toggle.update(cx, |chat, cx| chat.search_toggle_block(block_ix, cx));
    });
    let on_open: OnOpen = Rc::new(move |block_ix, line, ordinal, cx| {
        let _ = weak_rows.update(cx, |chat, cx| {
            chat.search_open_block(block_ix, line, ordinal, cx)
        });
    });
    results_shell(
        t,
        &list,
        &scrollbar,
        gpui::list(list.clone(), move |ix, _window, _cx| match blocks.get(ix) {
            Some(block) => render_block(
                ix,
                block,
                active,
                &on_open,
                &on_toggle,
                collapsed.contains(&block.rel),
                gutter,
            ),
            None => div().into_any_element(),
        })
        // list 自身要 flex_1 从父列拿高度（Auto 尺寸下会被布局成 0 高）
        .flex_1()
        .min_h_0(),
    )
    .into_any_element()
}

/// 空态 = Zed `project_search.rs:1049-1092` 的 landing page：
/// 标题（`LabelSize::Large` = **16px**）+ 副标题（`TextSize::Default` = 14px，
/// muted，下留 8px）+ 一排「图标 + 文案」行（`ButtonStyle::Subtle`：
/// 高 **22px**、`px 4`、`gap 4`、`rounded 4`、悬停 `ghost_element_hover`，
/// 图标 `IconSize::Small` = 14px）。**不加任何快捷键徽标**（用户 2026-10-21
/// 定案：一律不加快捷键）——点行等于点对应的开关按钮。
fn landing(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let ui = &chat.ws.search;
    let (heading, body): (&'static str, Option<&'static str>) = match ui.phase {
        Phase::Idle => (tr("search.landing_title"), None),
        Phase::Running if ui.scanned == 0 => (tr("search.loading_project"), None),
        Phase::Running => (tr("search.searching"), None),
        Phase::Done => (tr("search.no_results"), Some(tr("search.no_results_body"))),
    };
    let mut col = div().flex().flex_col().gap(px(4.)).child(
        div()
            .text_size(px(16.))
            .text_color(rgb(t.text))
            .child(SharedString::from(heading)),
    );
    if ui.phase == Phase::Idle {
        col = col.child(landing_rows(weak, t));
    } else if let Some(body) = body {
        col = col.child(
            div()
                .text_size(px(12.))
                .text_color(rgb(t.text))
                .child(SharedString::from(body)),
        );
    }
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .bg(rgb(t.bg))
        .child(col)
        .into_any_element()
}

/// landing 的五条「图标 + 文案」行（Zed 顺序：过滤 / 替换 / 正则 / 大小写 / 整词）。
fn landing_rows(weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    use super::SearchToggle;
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(14.))
                .text_color(rgb(t.text_muted))
                // Zed：副标题 `.mb_2()` = 8px
                .mb(px(8.))
                .child(SharedString::from(tr("search.landing_hint"))),
        )
        .child(landing_row(
            "psl-paths",
            landing_glyph(None, t),
            tr("search.landing_paths"),
            weak,
            SearchToggle::Filters,
            t,
        ))
        .child(landing_row(
            "psl-replace",
            landing_glyph(Some("replace"), t),
            tr("search.landing_replace"),
            weak,
            SearchToggle::Replace,
            t,
        ))
        .child(landing_row(
            "psl-regex",
            landing_glyph(Some(GLYPH_REGEX), t),
            tr("search.landing_regex"),
            weak,
            SearchToggle::Regex,
            t,
        ))
        .child(landing_row(
            "psl-case",
            landing_glyph(Some(GLYPH_CASE), t),
            tr("search.landing_case"),
            weak,
            SearchToggle::CaseSensitive,
            t,
        ))
        .child(landing_row(
            "psl-word",
            landing_glyph(Some(GLYPH_WORD), t),
            tr("search.landing_whole_word"),
            weak,
            SearchToggle::WholeWord,
            t,
        ))
        .into_any_element()
}

/// 无 SVG 的三枚（Zed 本身就是字形图标：`.*` / `Aa` / `wd`）
const GLYPH_REGEX: &str = ".*";
const GLYPH_CASE: &str = "Aa";
const GLYPH_WORD: &str = "wd";

/// 行首图标位（14px，muted）：`Some(svg 名)` 走 svg，`Some(字形)` 走文字，
/// `None` = Zed 的 `IconName::Filter`（PF 的 `sliders-horizontal`）。
fn landing_glyph(icon: Option<&'static str>, t: &'static Theme) -> AnyElement {
    let slot = div()
        .w(px(14.))
        .h(px(14.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(t.text_muted));
    match icon {
        Some("replace") => crate::ui::icon("replace", 14., t.text_muted),
        Some("sliders-horizontal") | None => {
            crate::ui::icon("sliders-horizontal", 14., t.text_muted)
        }
        Some(glyph) => slot
            .font_family(crate::appearance::panel_font().family.clone())
            .text_size(px(11.))
            .child(SharedString::from(glyph))
            .into_any_element(),
    }
}

/// 一行：高 22、`px 4`、`gap 4`、`rounded 4`，悬停 `bg_hover`，点击切开关。
fn landing_row(
    id: &'static str,
    glyph: AnyElement,
    label: &'static str,
    weak: &WeakEntity<Chat>,
    toggle: super::SearchToggle,
    t: &'static Theme,
) -> AnyElement {
    let weak = weak.clone();
    div()
        .id(id)
        // ui-raw: ok Zed landing 行几何逐值复刻（高22/px4/gap4/rounded4），非 PF 档位
        .h(px(22.))
        .px(px(4.))
        .flex()
        .items_center()
        .gap(px(4.))
        .rounded(px(4.))
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |chat, cx| chat.search_toggle(toggle, cx));
        })
        .child(glyph)
        .child(
            div()
                .font_family(crate::appearance::panel_font().family.clone())
                .text_size(px(14.))
                .text_color(rgb(t.text))
                .child(SharedString::from(label)),
        )
        .into_any_element()
}

/// 结果区外壳。**必须是 flex 列**：gpui 的 `div` 默认不是 flex 容器，缺
/// `flex().flex_col()` 时内部 list 拿不到定高 → 高度 0，结果全丢（2026-10-21
/// 用户实测「只显示计数 1/222 不显示内容」的根因）。与 md 预览
/// （`editor/markdown/doc.rs`）的列表容器配方逐字一致；抽成函数是为了让
/// [`tests::real_blocks_render_with_height`] 能直接测生产代码的布局。
fn results_shell(
    t: &'static Theme,
    list_state: &gpui::ListState,
    scrollbar: &gpui_component::scroll::ScrollbarState,
    list: impl gpui::IntoElement,
) -> gpui::Stateful<gpui::Div> {
    let mut shell = div()
        .id("search-results")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // 第一条文件头与搜索框留 5px 间隙：文件头块自带 Zed 的 4px 内留白，
        // 这里补 1px = 5px（用户 2026-10-21 定案的那个 5px）
        .pt(px(1.))
        // 两侧留白 + 右侧让出滚动条宽度：Zed 的文件头与代码行都从这条左线起，
        // 右边留出滚动条（2026-10-21 用户实测「两侧要有明显 padding，右侧还要
        // 留滚动条的宽度」）
        .pl(px(8.))
        .pr(px(18.))
        .relative()
        .child(list);
    // 竖向滚动条（与 md 预览同一挂法：绝对定位叠在右缘，内容已留 18px）
    if list_state.max_offset_for_scrollbar().height > px(0.) {
        let handle = crate::ui::list_handle::ListStateHandle(list_state.clone());
        shell = shell.child(
            div()
                .absolute()
                .top(px(4.))
                .bottom(px(4.))
                .right(px(4.))
                .w(px(8.))
                .child(gpui_component::scroll::Scrollbar::vertical(
                    scrollbar, &handle,
                )),
        );
    }
    shell
}

/// 一个文件结果块：组头 + 若干摘录段（整宽、无外边距、无圆角、块间无空隙）。
fn render_block(
    block_ix: usize,
    block: &ResultBlock,
    active: Option<usize>,
    on_open: &OnOpen,
    on_toggle: &OnToggle,
    collapsed: bool,
    gutter: Gutter,
) -> AnyElement {
    let t = T();
    let mut card = div()
        .id(SharedString::from(format!("psb-{block_ix}")))
        .w_full()
        .flex()
        .flex_col()
        .child(header_row(
            block_ix, block, on_open, on_toggle, collapsed, t,
        ));
    // 收起 = 只留文件头；纯文件栏本来就没有内容行
    if block.path_only || collapsed {
        return card.into_any_element();
    }
    // 不做语法着色：结果区是单色正文（用户 2026-10-21：syntect 在 markdown 上
    // 给出红/橙/柠等一堆颜色，「五花八门」）
    for (seg_ix, segment) in block.segments.iter().enumerate() {
        if seg_ix > 0 {
            // 段间分隔：1px 细线（Zed 的 excerpt 之间是头尾控制条，PF 用细线
            // 表达同一件事，不占高度、不刷底色）
            card = card.child(div().w_full().h(px(1.)).bg(rgb(t.border)));
        }
        for (line_ix, line) in segment.iter().enumerate() {
            card = card.child(render_line(
                block_ix, line, active, on_open, seg_ix, line_ix, gutter,
            ));
        }
    }
    card.into_any_element()
}

/// 组头（文件操作栏）——版式逐项照 `crates/editor/src/element/header.rs`
/// 的 `render_buffer_header`：块 `p 4` + 内条 `rounded_sm(4) / border_1 /
/// pl 4 pr 8 / gap 6`，左起「收展钮（28×28）→ 标题 → 副标题」。
///
/// 会话查询（`search/sessions.rs`）复用同一个 [`card_header`]，保证两种结果
/// 面板的组头一模一样（用户 2026-10-21：会话查询「根据文件查询的界面改造」）。
pub(super) struct HeaderCallbacks {
    pub(super) open: Rc<dyn Fn(&mut gpui::App)>,
    pub(super) toggle: Rc<dyn Fn(&mut gpui::App)>,
}

pub(super) fn card_header(
    id: String,
    title: SharedString,
    sub: SharedString,
    tail: Option<&'static str>,
    collapsed: bool,
    cb: HeaderCallbacks,
    t: &'static Theme,
) -> AnyElement {
    let font = crate::appearance::file_font();
    let HeaderCallbacks { open, toggle } = cb;
    // 块：p(4) + 高 2 行 → 条四周留 4px（块间因此有 8px 间隙）
    div()
        .id(SharedString::from(format!("psb-h-{id}")))
        // ui-raw: ok Zed render_buffer_header 逐值复刻（块p4/内条 pl4 pr8 gap6 rounded4）
        .w_full()
        .h(px(header_h()))
        .p(px(HEADER_PAD))
        .child(
            // 内条
            div()
                .id(SharedString::from(format!("psb-b-{id}")))
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .pl(px(4.))
                .pr(px(8.))
                .gap(px(6.))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(t.border))
                // 组头底：bg_panel（`bg_subtle` 是 RRGGBBAA，rgb() 读会变近黑）
                .bg(rgb(t.bg_panel))
                .shadow_sm()
                .cursor_pointer()
                // Zed 内条本身就有 hover 底（`element_hover`）——源码行为，不是发明
                .hover(move |s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    open(cx);
                })
                // 收展钮：Zed 的 28×28 方钮（**不带 tooltip**——用户定案）。
                // 点它只收展，不打开。
                .child(
                    div()
                        .id(SharedString::from(format!("psb-t-{id}")))
                        .flex_shrink_0()
                        // ui-raw: ok Zed 收展钜 28×28（`rems_from_px(28)`），
                        // 不套 PF 的 `ui_size`（那套基准是 12，会把 28 放大成 31）
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(2.))
                        .hover(move |s| s.bg(rgb(t.bg_selected)))
                        .cursor_pointer()
                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                            // 别让点击冒泡到内条（那会直接打开）
                            cx.stop_propagation();
                            toggle(cx);
                        })
                        .child(crate::ui::icon(
                            if collapsed {
                                "chevron-right"
                            } else {
                                "chevron-down"
                            },
                            14.,
                            t.text_dim,
                        )),
                )
                // 标题 + 副标题（Zed 的 `gap_0p5` = 2px，整块 flex_1）
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex_shrink_0()
                                .font_family(font.family.clone())
                                .text_size(px(font.size))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgb(t.text))
                                .child(title),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .font_family(font.family.clone())
                                .text_size(px(font.size))
                                .text_color(rgb(t.text_muted))
                                .child(sub),
                        ),
                )
                .when_some(tail, |d, label| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .font_family(crate::appearance::panel_font().family)
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(label)),
                    )
                }),
        )
        .into_any_element()
}

fn header_row(
    block_ix: usize,
    block: &ResultBlock,
    on_open: &OnOpen,
    on_toggle: &OnToggle,
    collapsed: bool,
    t: &'static Theme,
) -> AnyElement {
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
    let open = on_open.clone();
    let toggle = on_toggle.clone();
    card_header(
        block_ix.to_string(),
        name,
        dir,
        block.path_only.then(|| tr("search.path_match")),
        collapsed,
        HeaderCallbacks {
            open: Rc::new(move |cx| open(block_ix, 1, None, cx)),
            toggle: Rc::new(move |cx| toggle(block_ix, cx)),
        },
        t,
    )
}

/// 摘录行：行号 gutter + 单色正文 + 命中底色。
/// 长行**折行**（`StyledText` 原生换行），行高随内容增长，与 Zed 一致。
fn render_line(
    block_ix: usize,
    line: &ResultLine,
    active: Option<usize>,
    on_open: &OnOpen,
    seg_ix: usize,
    line_ix: usize,
    gutter: Gutter,
) -> AnyElement {
    let t = T();
    let font = crate::appearance::file_font();
    let is_active = line.hit && active.is_some() && line.ordinal == active;
    let click = on_open.clone();
    let ordinal = line.ordinal;
    let line_no = line.line;
    let row_h = line_h();
    let highlights = match_highlights(&line.text, &line.spans, t);
    div()
        .id(SharedString::from(format!(
            "psb-l-{block_ix}-{seg_ix}-{line_ix}"
        )))
        .min_h(px(row_h))
        .w_full()
        .flex()
        // 折行后行会变高：gutter 与正文都从顶对齐
        .items_start()
        // 活跃命中：**只有左缘竖条**，不刷整行底色（Zed 同；此前刷 accent 洗底
        // 是用户评「颜色五花八门」的主因之一）
        .when(is_active, |d| d.border_l_2().border_color(rgb(t.accent)))
        .when(!is_active, |d| {
            d.border_l_2().border_color(gpui::rgba(0x00000000))
        })
        // 无 hover 变色：Zed 结果区鼠标划过不变色（用户 2026-10-21 定案）
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            click(block_ix, line_no, ordinal, cx);
        })
        // 行号 gutter：与正文同族同号；左留 1ch、数字区右对齐、右留 1ch
        // （宽度随最宽行号位数动态算，见 [`Gutter`]）
        .child(
            div()
                .w(px(gutter.total()))
                .flex_shrink_0()
                .pl(px(gutter.left))
                .pr(px(gutter.right))
                .whitespace_nowrap()
                .text_align(gpui::TextAlign::Right)
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .line_height(gpui::relative(LINE_HEIGHT))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(line.line.to_string())),
        )
        // 正文：单色 + 命中底色（StyledText 原生折行）
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(px(8.))
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .line_height(gpui::relative(LINE_HEIGHT))
                .text_color(rgb(t.text))
                .child(StyledText::new(line.text.clone()).with_highlights(highlights)),
        )
        .into_any_element()
}

/// 命中区间 → `StyledText` 的 highlight 列表（不重叠、按字节序）。
///
/// 只产出「命中底色」，不掺任何语法色——结果区是单色正文（见 [`render_line`]
/// 的视觉口径）。底色用主题中性 selection 色 `bg_selected`（Zed 的命中底色
/// 同样是中性色，不是 accent 洗底）。
pub(super) fn match_highlights(
    line: &str,
    spans: &[Range<usize>],
    t: &'static Theme,
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
    if line.is_empty() || spans.is_empty() {
        return Vec::new();
    }
    let match_bg: gpui::Hsla = rgb(t.bg_selected).into();
    let mut bounds: Vec<usize> = vec![0, line.len()];
    for span in spans {
        bounds.push(span.start.min(line.len()));
        bounds.push(span.end.min(line.len()));
    }
    bounds.retain(|b| line.is_char_boundary(*b));
    bounds.sort_unstable();
    bounds.dedup();

    let mut out = Vec::new();
    for pair in bounds.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let hit = spans.iter().any(|s| s.contains(&start));
        let mut style = gpui::HighlightStyle::default();
        if hit {
            style.background_color = Some(match_bg);
            style.color = Some(rgb(t.text).into());
        }
        out.push((start..end, style));
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
        heights: Rc<RefCell<Vec<(f32, f32)>>>,
        list: gpui::ListState,
        blocks: Rc<Vec<ResultBlock>>,
        collapsed: bool,
    }

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let seen = self.seen.clone();
            let heights = self.heights.clone();
            let blocks = self.blocks.clone();
            let collapsed = self.collapsed;
            let on_open: super::OnOpen =
                Rc::new(|_: usize, _: u32, _: Option<usize>, _: &mut gpui::App| {});
            let on_toggle: super::OnToggle = Rc::new(|_: usize, _: &mut gpui::App| {});
            // 外层 = 搜索面板内容列（定高，模拟 content 区）
            div()
                .w(px(600.))
                .h(px(240.))
                .flex()
                .flex_col()
                .child(results_shell(
                    crate::appearance::theme::theme(),
                    &self.list,
                    &gpui_component::scroll::ScrollbarState::default(),
                    gpui::list(self.list.clone(), move |ix, _window, _cx| {
                        seen.borrow_mut().push(ix);
                        match blocks.get(ix) {
                            Some(block) => {
                                let el = super::render_block(
                                    ix,
                                    block,
                                    None,
                                    &on_open,
                                    &on_toggle,
                                    collapsed,
                                    super::Gutter {
                                        num: 40.,
                                        left: 9.,
                                        right: 9.,
                                    },
                                );
                                div()
                                    .w_full()
                                    .on_children_prepainted({
                                        let heights = heights.clone();
                                        move |bounds, _, _| {
                                            if let Some(b) = bounds.first() {
                                                heights.borrow_mut().push((
                                                    f32::from(b.origin.y),
                                                    f32::from(b.size.height),
                                                ));
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
        heights: &Rc<RefCell<Vec<(f32, f32)>>>,
    ) -> Vec<usize> {
        frame_with(cx, seen, heights, sample_blocks(), false)
    }

    fn frame_with(
        cx: &mut VisualTestContext,
        seen: &Rc<RefCell<Vec<usize>>>,
        heights: &Rc<RefCell<Vec<(f32, f32)>>>,
        blocks: Vec<ResultBlock>,
        collapsed: bool,
    ) -> Vec<usize> {
        seen.borrow_mut().clear();
        heights.borrow_mut().clear();
        let blocks = Rc::new(blocks);
        cx.draw::<Entity<TestView>>(point(px(0.), px(0.)), size(px(600.), px(240.)), |_, cx| {
            cx.new(|_| TestView {
                seen: seen.clone(),
                heights: heights.clone(),
                list: gpui::ListState::new(blocks.len(), gpui::ListAlignment::Top, px(1000.)),
                blocks,
                collapsed,
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
        let heights: Rc<RefCell<Vec<(f32, f32)>>> = Default::default();
        let rows = frame(window, &seen, &heights);
        assert_eq!(rows, vec![0, 1], "两个结果块都必须被构建");
        let hs = heights.borrow().clone();
        assert!(!hs.is_empty(), "块必须完成一次 prepaint（有实际 bounds）");
        assert!(
            hs.iter().all(|(_, h)| *h > 0.),
            "块高度必须 > 0，实测 {hs:?}（0 = 结果区空白）"
        );
    }

    /// 收起锁：收起后该块只剩文件头（高度回落到 `header_h()`），内容行不再
    /// 参与布局——「点收展钮 → 该文件查询内容收起」的行为契约。
    #[gpui::test]
    fn collapsed_block_keeps_only_header(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<usize>>> = Default::default();
        let heights: Rc<RefCell<Vec<(f32, f32)>>> = Default::default();
        let only_first = vec![sample_blocks().remove(0)];

        frame_with(&mut *window, &seen, &heights, only_first.clone(), false);
        let expanded = heights.borrow()[0].1;

        frame_with(&mut *window, &seen, &heights, only_first, true);
        let collapsed = heights.borrow()[0].1;

        assert!(
            (collapsed - super::header_h()).abs() < 2.0,
            "收起后块高应等于文件头高 {}，实测 {collapsed}",
            super::header_h()
        );
        assert!(
            collapsed < expanded - 10.0,
            "收起必须比展开矮（{collapsed} vs {expanded}）"
        );
    }

    /// 间隙锁（2026-10-21 用户问「间隙呢？？」）：Zed 的间隙**来自块内**
    /// `BUFFER_HEADER_PADDING`（4px）而不是列表 gap——所以块在列表里必须紧贴
    /// （gap 0），块高 = 2 行，条上下各留 4px ⇒ 相邻两条之间可视间隙 8px、
    /// 条与自己代码行之间 4px。这里把这三条一起锁住。
    #[gpui::test]
    fn adjacent_headers_keep_eight_px_gap(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<usize>>> = Default::default();
        let heights: Rc<RefCell<Vec<(f32, f32)>>> = Default::default();
        let blocks: Vec<ResultBlock> = sample_blocks()
            .into_iter()
            .map(|mut b| {
                b.segments.clear();
                b
            })
            .collect();

        frame_with(&mut *window, &seen, &heights, blocks, true);
        let hs = heights.borrow().clone();
        assert_eq!(hs.len(), 2, "两个块都要有 bounds，实测 {hs:?}");
        let (top0, h0) = hs[0];
        let (top1, _) = hs[1];
        let item_gap = top1 - (top0 + h0);
        assert!(
            item_gap.abs() < 0.6,
            "列表项之间必须紧贴（间隙来自块内留白），实测 {item_gap}"
        );
        assert!(
            (h0 - super::header_h()).abs() < 0.6,
            "折叠块高应 = 2 行（{}），实测 {h0}",
            super::header_h()
        );
        // 块高 = 2 行、条四周 4px → 条与条的可视间隙 = 8px
        assert_eq!(super::header_h(), (super::line_h() * 2.).round());
        assert_eq!(super::HEADER_PAD, 4.);
        assert_eq!(super::HEADER_PAD * 2., 8.);
    }

    /// 行号列宽锁（2026-10-21 用户「line num 列不够宽，Zed 是动态调整」）：
    /// 宽度 = `max(位数, 4) × ch_advance + 2 × ch_width`（Zed
    /// `gutter_dimensions` + 默认 `min_line_number_digits = 4`）。
    #[gpui::test]
    fn gutter_width_follows_widest_line_number(cx: &mut TestAppContext) {
        fn block_with_line(line: u32) -> Vec<ResultBlock> {
            vec![ResultBlock {
                rel: "a.md".into(),
                path_only: false,
                count: 1,
                segments: vec![vec![ResultLine {
                    line,
                    text: "x".into(),
                    spans: vec![0..1],
                    hit: true,
                    ordinal: Some(0),
                }]],
            }]
        }
        let (three, four, five) = cx.read(|cx| {
            (
                super::gutter_for(&block_with_line(137), cx),
                super::gutter_for(&block_with_line(2219), cx),
                super::gutter_for(&block_with_line(22198), cx),
            )
        });
        assert_eq!(
            three.num, four.num,
            "不足 min_line_number_digits(4) 位按 4 位留宽，不随位数抖动"
        );
        let step = five.num - four.num;
        assert!(
            step > 1.0,
            "5 位行号必须比 4 位更宽（实测 +{step}），否则 22198 会折行"
        );
        // 数字区够放 5 个字符：5 位时 num ≥ 5 × (step 即一个字符的推进宽)
        assert!(
            five.num >= step * 5.0 - 2.0,
            "5 位数要放得下 5 个字符（num={} step={step}）",
            five.num
        );
        assert!(three.left > 0.0 && three.right > 0.0, "左右各留 1ch");
        assert_eq!(super::vidx_digits(u32::MAX), 10);
        assert_eq!(super::vidx_digits(1), 1);
    }

    /// 命中区间必须进 highlight（Zed 的命中底色），且 run 不重叠、按序。
    #[gpui::test]
    fn match_spans_become_highlights(_cx: &mut TestAppContext) {
        let t = crate::appearance::theme::theme();
        let line = "abc openai def";
        let hs = super::match_highlights(line, &[4..10], t);
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
