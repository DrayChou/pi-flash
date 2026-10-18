//! Markdown renderer for chat messages and md file preview (pi-web
//! MarkdownBody parity: globals.css `.markdown-body` spec + CodeBlock
//! structure). Streaming-friendly: whole-message re-render.
//!
//! pi-web 规格（app/globals.css + MermaidBlock.tsx CodeBlock）：
//! - 正文 14px / line-height 1.7；块间距 = CSS margin 折叠 max(mb, mt)，
//!   末块不挂 mb（p:last-child 同效）
//! - 标题 600 字重、h1 1.16em / h2 1.08em / h3 0.98em 88% 混色
//! - strong = 700 + 88% 混 accent；em = muted；marker = accent 72% 混 muted
//! - 行内 code = mono 0.92em 等宽盒（拆段渲染）；代码块 = 外框圆角 7 +
//!   头部 + 行号 + 12.5px/1.62，阴影 0 1px 0
//! - 语法高亮：浅色 InspiredGitHub / 深色 VS Dark+（代码构建，pi-web 用
//!   Prism vscDarkPlus）

pub mod attachments;
pub mod render;
pub mod wysiwyg;

use gpui::{
    AnyElement, FontStyle, FontWeight, HighlightStyle, InteractiveText, SharedString, StyledText,
    TextStyle, div, prelude::*, px, relative, rgb, rgba,
};
use pulldown_cmark::{Event, Options, Parser};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::ops::Range;

use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::ScrollAxisExt;
#[cfg(test)] mod real_file_tests;
#[cfg(test)] mod tests;

mod doc;
mod highlight;
mod links;
mod parse;

pub(crate) use doc::*;
pub(crate) use highlight::*;
pub(crate) use links::*;
use parse::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Style {
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Link,
    Strike,
    /// 标题（024 P1）：level 1-6——源码态装饰用（字重 + 行级字号上浮），
    /// 预览渲染走 Heading 块不经过此样式。
    Heading(u8),
    /// 行内公式标记（v57-2）：Run.text 存 LaTeX 源码，渲染期拆段成图
    Math,
    /// 块级公式标记（$$…$$ 可能出现在段落事件流内，多行块 pulldown 也发在
    /// Paragraph 里）：渲染期独立成整行图
    DisplayMath,
}

#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub(crate) text: String,
    pub(crate) style: Style,
    /// `Style::Link` 的跳转目标（autolink = 文本本身），其余样式恒 None。
    /// 渲染层据此接 InteractiveText → cx.open_url。
    pub(crate) url: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum MdBlock {
    Heading { level: u8, runs: Vec<Run> },
    Paragraph { runs: Vec<Run> },
    Code { lang: String, code: String },
    Quote { blocks: Vec<MdBlock> },
    ListItem { depth: usize, marker: String, runs: Vec<Run>, task: Option<bool> },
    Table { head: Vec<Vec<Run>>, rows: Vec<Vec<Vec<Run>>> },
    Image { url: String, alt: Vec<Run>, width: Option<f32> },

    /// 块级公式（$$…$$，v57-2）
    Math { latex: String },
    /// mermaid 图代码块（v57-3）
    Mermaid { source: String },
    Rule,
}

/// 样式栈元素：(样式, 链接目标)。URL 随样式进栈——链接内嵌粗体等嵌套
/// 场景不丢跳转目标，渲染层按 run.url 接点击。
pub(crate) const MONO_FAMILY: &str = "JetBrains Mono";

pub(crate) type StyleEntry = (Style, Option<String>);

/// markdown 基准字号（pi-web: 14px + chat-font-size-offset；本项目以
/// 会话/文件槽位字号为基准做绝对像素差换算）
const BASE: f32 = 14.;

thread_local! {
    /// 当前 markdown 渲染的字体规格：聊天路径（render/render_user）= 会话
    /// 字体；文件预览（render_themed）= markdown 字体。入口设置、构建期
    /// 读取（base_style 等在 Chat::render 内同步执行）；未设置时回退
    /// markdown 槽位。UI 单线程，thread_local 兜底后台路径。
    static MD_SPEC: std::cell::RefCell<Option<crate::services::workspace::FontSpec>> =
        std::cell::RefCell::new(None);
}

fn active_md_spec() -> crate::services::workspace::FontSpec {
    MD_SPEC.with(|c| c.borrow().clone()).unwrap_or_else(crate::appearance::file_font)
}

fn set_md_spec(s: crate::services::workspace::FontSpec) {
    MD_SPEC.with(|c| *c.borrow_mut() = Some(s));
}

fn base_style(size: f32, line_h: f32, color: u32, weight: FontWeight) -> TextStyle {
    // 聊天正文跟随「会话字体」，文件预览跟随「文件字体」（绝对像素差：
    // 生效字号 = 槽位设置值 + (size - BASE)，设计稿基准 14px）。字重必须
    // 显式入参：容器 font_weight 同样传不进 runs（同 font_size 的限制）
    let spec = active_md_spec();
    TextStyle {
        // 容器 .text_color 不会传进 StyledText（自带 base style 覆盖继承），
        // 颜色必须显式入参——h3 混色/引用块 muted 都靠它落到 run 上
        color: rgb(color).into(),
        font_family: spec.family.clone().into(),
        font_size: px(spec.size + (size - BASE)).into(),
        font_weight: weight,
        line_height: relative(line_h),
        ..Default::default()
    }
}

pub(crate) fn highlight(style: Style, t: &Theme) -> Option<HighlightStyle> {
    let h = match style {
        Style::Normal => return None,
        // pi-web strong: 700 字重 + color-mix(text 88%, accent)
        Style::Bold => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            color: Some(rgb(crate::appearance::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // pi-web em: var(--text-muted)
        Style::Italic => HighlightStyle {
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(t.text_muted).into()),
            ..Default::default()
        },
        Style::BoldItalic => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            font_style: Some(FontStyle::Italic),
            color: Some(rgb(crate::appearance::theme::mix_rgb(t.text, t.accent, 0.88)).into()),
            ..Default::default()
        },
        // note: gpui 0.2.2 highlights cannot change font family; 正文的行内 code
        // 走 paragraph_element 拆段成等宽盒，此高亮只是表格/标题内的兜底
        Style::Code => HighlightStyle { background_color: Some(rgb(t.tool_bg).into()), ..Default::default() },
        // pi-web a: 下划线 45% 透明（offset gpui 无对应）
        Style::Link => HighlightStyle {
            color: Some(rgb(t.accent).into()),
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.),
                color: Some(gpui::rgba((t.accent << 8) | 0x73).into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        // 024 P1 标题：正文色 + 字重（字号上浮由 Decorations.line_scale
        // 行级 shaping 承担，行高 uniform 内 cap）
        Style::Heading(_) => HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            color: Some(rgb(t.text).into()),
            ..Default::default()
        },
        Style::Strike => HighlightStyle {
            strikethrough: Some(gpui::StrikethroughStyle { thickness: px(1.), ..Default::default() }),
            ..Default::default()
        },
        // 公式在渲染前已拆段为图片；防御性落到代码风
        Style::Math | Style::DisplayMath => {
            HighlightStyle { background_color: Some(rgb(t.tool_bg).into()), ..Default::default() }
        }
    };
    Some(h)
}

fn styled_text(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> StyledText {
    let mut s = String::new();
    let mut highlights = Vec::new();
    for r in runs {
        let start = s.len();
        s.push_str(&r.text);
        let end = s.len();
        if let Some(h) = highlight(r.style, t) {
            highlights.push((start..end, h));
        }
    }
    StyledText::new(s).with_default_highlights(&base_style(size, line_h, color, weight), highlights)
}

/// 链接 run 的（拼接后字节区间, URL）平行列表，供 InteractiveText 命中。
fn link_ranges(runs: &[Run]) -> (Vec<Range<usize>>, Vec<String>) {
    let mut ranges = Vec::new();
    let mut urls = Vec::new();
    let mut pos = 0usize;
    for r in runs {
        let end = pos + r.text.len();
        if let Some(url) = &r.url {
            ranges.push(pos..end);
            urls.push(url.clone());
        }
        pos = end;
    }
    (ranges, urls)
}

/// runs → 文本元素：含链接 run 时包 InteractiveText（点击 = cx.open_url 调
/// 系统默认浏览器；悬停手型光标由 InteractiveText 自动处理），否则纯
/// StyledText（多数块零开销）。element id 用拼接文本哈希——渲染无状态，
/// 靠内容稳定性保住 InteractiveText 跨帧的 mouse down/up 状态（流式中
/// 正在增长的段落点击可能失效，定稿即恢复）。
fn runs_element(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> AnyElement {
    let text = styled_text(runs, t, size, line_h, color, weight);
    let (ranges, urls) = link_ranges(runs);
    if ranges.is_empty() {
        return text.into_any_element();
    }
    let mut hasher = DefaultHasher::new();
    hasher.write(runs_text(runs).as_bytes());
    let id = gpui::ElementId::named_usize("md-link", hasher.finish() as usize);
    let target = md_target();
    InteractiveText::new(id, text)
        .on_click(ranges, move |ix, _window, cx| {
            let Some(url) = urls.get(ix) else { return };
            // 点击归链接自己消费（本地文件开 tab，其余开浏览器），冒泡到
            // md 预览容器的「点击进编辑」不得再触发
            cx.stop_propagation();
            // href 指向本地文件 → 内置文件标签页（pi-web 拦截 file 链接同型）
            if let Some(path) = as_file_path(url) {
                open_path_with(target.clone(), path, cx);
            } else {
                cx.open_url(url);
            }
        })
        .into_any_element()
}

/// 正文块（段落/表格/代码等）：字号必须挂在容器 div 上——gpui 0.2.2 的
/// StyledText 排版字号取 `window.text_style()`（容器继承链），runs 里的
/// font_size 只决定字族/字重/颜色，对字形尺寸无效。此前 base_style 里算好
/// 的字号对正文/表格/代码块从未生效（一直画容器继承的默认值），只有像
/// 标题、列表标号那样把 text_size 挂在容器上的元素才真正随设置变。
fn sized_text(
    runs: &[Run],
    t: &Theme,
    size: f32,
    line_h: f32,
    color: u32,
    weight: FontWeight,
) -> AnyElement {
    let spec = active_md_spec();
    div()
        .w_full()
        .text_size(px(spec.size + (size - BASE)))
        .line_height(relative(line_h))
        .font_weight(weight)
        .child(runs_element(runs, t, size, line_h, color, weight))
        .into_any_element()
}

/// pi-web 标题字号（em 相对 14px 正文）。
fn size_for_level(level: u8) -> f32 {
    match level {
        1 => BASE * 1.16,
        2 => BASE * 1.08,
        3 => BASE * 0.98,
        _ => BASE,
    }
}

fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// 各块的 CSS margin (mt, mb)，pi-web globals.css .markdown-body 规格。
/// flex 不做外边距折叠——render_blocks 用 max(prev.mb, cur.mt) 复现 CSS
/// 兄弟折叠，末块自然无 mb（= p:last-child { margin-bottom: 0 } 同效，
/// 此前逐块挂 margin 导致块间距系统性偏大：8+6=14 而 CSS 取 8）。
fn block_margins(b: &MdBlock) -> (f32, f32) {
    match b {
        MdBlock::Heading { .. } => (10., 5.),
        MdBlock::Paragraph { .. } => (0., 8.),
        MdBlock::Code { .. } | MdBlock::Mermaid { .. } => (6., 6.),
        MdBlock::Quote { .. } => (6., 6.),
        // 列表容器 ul/ol { margin: 5px 0 8px }
        MdBlock::ListItem { .. } => (5., 8.),
        MdBlock::Table { .. } => (8., 8.),
        // pi-web img { margin: 8px 0 }
        MdBlock::Image { .. } => (8., 8.),
        // katex-display { margin: 0.6em }
        MdBlock::Math { .. } => (8., 8.),
        MdBlock::Rule => (12., 12.),
    }
}

fn render_blocks(blocks: &[MdBlock], depth: usize, t: &Theme, streaming: bool, color: u32) -> gpui::Div {
    let mut col = div().flex().flex_col();
    // 间距 = max(前块 mb, 本块 mt)；首块保留自身 mt（引用块内有 padding，
    // 首元素 margin 不折叠出去，与 CSS 一致）
    let mut prev_mb: Option<f32> = None;
    let mut push = |col: gpui::Div, el: gpui::AnyElement, mt: f32, mb: f32| -> gpui::Div {
        let space = prev_mb.map_or(mt, |p| p.max(mt));
        prev_mb = Some(mb);
        col.child(div().mt(px(space)).child(el))
    };
    // 连续 ListItem 收进列表容器（li 间距 3px 用 gap）
    let mut i = 0;
    while i < blocks.len() {
        if matches!(blocks[i], MdBlock::ListItem { .. }) {
            let mut list = div().flex().flex_col().gap(px(3.));
            while i < blocks.len() && matches!(blocks[i], MdBlock::ListItem { .. }) {
                list = list.child(render_block(&blocks[i], depth, t, streaming, color));
                i += 1;
            }
            col = push(col, list.into_any_element(), 5., 8.);
        } else {
            let (mt, mb) = block_margins(&blocks[i]);
            let el = render_block(&blocks[i], depth, t, streaming, color);
            col = push(col, el, mt, mb);
            i += 1;
        }
    }
    col
}

// ---------------------------------------------------------------------------
// 代码块横向滚动句柄表——Zed Markdown::code_block_scroll_handles 的无状态
// 等价物：渲染函数没有实体可挂句柄，仿 ui::vlist SCROLL_HANDLES 建线程
// 局部表，按代码内容哈希键控（同内容块共享句柄=镜像滚动，罕见且无害）。
// Scrollbar 拖拽写句柄偏移，track_scroll 的滚动容器每帧读同一偏移。
// 哈希键无法枚举回收，超容量整表清空。
// ---------------------------------------------------------------------------
struct CodeScroll {
    handle: gpui::ScrollHandle,
    bar_state: gpui_component::scroll::ScrollbarState,
}

thread_local! {
    static CODE_SCROLLS: std::cell::RefCell<
        std::collections::HashMap<u64, std::rc::Rc<CodeScroll>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

fn code_scroll(hash: u64) -> std::rc::Rc<CodeScroll> {
    CODE_SCROLLS.with(|cell| {
        let mut map = cell.borrow_mut();
        if map.len() > 256 {
            map.clear();
        }
        map.entry(hash)
            .or_insert_with(|| {
                std::rc::Rc::new(CodeScroll {
                    handle: gpui::ScrollHandle::new(),
                    bar_state: gpui_component::scroll::ScrollbarState::default(),
                })
            })
            .clone()
    })
}

/// 代码块：外框圆角 7px + 头部（语言名 / 复制）+ 行号 + 高亮体（字号 =
/// 槽位字号 -1，字体大小设置.md「代码块内容」；pi-web 行高 1.62）。
/// 文档模式代码块（Zed/GitHub 预览形制）：无语言标签/复制/行号 chrome，
/// 圆角框 + 语法高亮 + 横向滚动。
fn render_code_block_doc(lang: &str, code: &str, t: &Theme) -> gpui::AnyElement {
    let code = code.trim_end_matches('\n');
    let body_bg = crate::appearance::theme::mix_rgb(t.bg, t.bg_panel, 0.92);
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for (seg, c) in cached_highlight_segments(code, lang, t.dark).iter() {
        if seg.is_empty() {
            continue;
        }
        let start = text.len();
        text.push_str(seg);
        push_color(&mut highlights, start, text.len(), *c);
    }
    let spec = active_md_spec();
    let base = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.).into(),
        line_height: relative(1.6),
        ..Default::default()
    };
    let hash = hash_str(&format!("doc\0{lang}\0{code}"));
    let scroll = code_scroll(hash);
    div()
        .relative()
        .w_full()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .bg(rgb(body_bg))
        .overflow_hidden()
        .child(
            div()
                .id(SharedString::from(format!("md-code-doc-{hash:016x}")))
                // Zed markdown 代码块同构：滚动容器必须 display:flex——gpui
                // block 子元素被拉伸到容器宽，永远无横向溢出可滚；flex_none
                // 内层让 nowrap 文本按自然宽度溢出成可滚内容
                .flex()
                .w_full()
                .px(px(14.))
                .py(px(12.))
                .text_size(px(spec.size - 1.))
                .line_height(relative(1.6))
                .whitespace_nowrap()
                .overflow_x_scroll()
                // 锁轴：纵向滚轮穿透给外层页面滚动，横向滚轮只滚本块
                .restrict_scroll_to_axis()
                .track_scroll(&scroll.handle)
                .child(
                    div().flex_none().child(
                        StyledText::new(text).with_default_highlights(&base, highlights),
                    ),
                ),
        )
        // 横向滑块：Scrollbar 必须套显式 absolute 细条（用户气泡/导航浮层
        // 的既有配方）——直接挂容器用其自带 Absolute+100% 布局在 gpui 0.2.2
        // 下测不出可见 thumb。内容不超宽时 Scrollbar 自隐藏，无需手动 gate。
        .child(
            div()
                .absolute()
                .left(px(3.))
                .right(px(3.))
                .bottom(px(3.))
                .h(px(8.))
                .child(gpui_component::scroll::Scrollbar::horizontal(
                    &scroll.bar_state,
                    &scroll.handle,
                )),
        )
        .into_any_element()
}

fn render_code_block(lang: &str, code: &str, t: &Theme, streaming: bool) -> gpui::Div {
    let code = code.trim_end_matches('\n');
    let body_bg = crate::appearance::theme::mix_rgb(t.bg, t.bg_panel, 0.92);

    // header
    let copy_code = code.to_string();
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .px(px(10.))
        .py(px(5.))
        .bg(rgb(t.bg_panel))
        .border_b_1()
        .border_color(rgb(t.border))
        .text_size(px(crate::ui::tokens::fixed::MD_LANG_BAR))
        .child(
            div()
                .font_family(MONO_FAMILY)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(if lang.is_empty() {
                    "text".to_string()
                } else {
                    lang.to_string()
                })),
        )
        .child(
            div()
                .id("md-copy")
                .cursor_pointer()
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(t.border))
                .px(px(7.))
                .py(px(2.))
                .text_color(rgb(t.text_muted))
                .hover(|s| s.text_color(rgb(t.text)))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_code.clone()));
                })
                .child(SharedString::from(crate::i18n::tr("action.copy"))),
        );

    // 代码体：行号 gutter 前缀 + 逐行高亮段
    let lines: Vec<&str> = if code.is_empty() {
        vec![""]
    } else {
        code.split('\n').collect()
    };
    // c15: 流式期间纯文本（pi-web 渲染裸 <pre>，不做 Prism 高亮）
    let mut text = String::new();
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    if streaming {
        // 纯文本，无行号无高亮
        text.push_str(code);
    } else {
        let n_digits = lines.len().to_string().len();
        let dim = rgb(t.text_dim);
        // LRU 命中返回 Rc 借用，不再每帧 clone 全部高亮段
        let segs = cached_highlight_segments(code, lang, t.dark);
        let mut seg_ix = 0usize;
        for (i, _) in lines.iter().enumerate() {
            // 行号 gutter
            let start = text.len();
            text.push_str(&format!("{:>w$}  ", i + 1, w = n_digits));
            highlights.push((
                start..text.len(),
                HighlightStyle { color: Some(dim.into()), ..Default::default() },
            ));
            // 该行的高亮段
            while seg_ix < segs.len() {
                let (seg, c) = &segs[seg_ix];
                match seg.find('\n') {
                    Some(pos) => {
                        let (head, _) = seg.split_at(pos);
                        if !head.is_empty() {
                            let start = text.len();
                            text.push_str(head);
                            push_color(&mut highlights, start, text.len(), *c);
                        }
                        seg_ix += 1;
                        break; // 该行结束
                    }
                    None => {
                        if !seg.is_empty() {
                            let start = text.len();
                            text.push_str(seg);
                            push_color(&mut highlights, start, text.len(), *c);
                        }
                        seg_ix += 1;
                    }
                }
            }
            if i + 1 < lines.len() {
                text.push('\n');
            }
        }
    }

    let spec = active_md_spec();
    let base = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.).into(),
        line_height: relative(1.62),
        ..Default::default()
    };
    let hash = hash_str(&format!("chat\0{lang}\0{code}"));
    let scroll = code_scroll(hash);

    div()
        .w_full()
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(7.))
        .overflow_hidden()
        .bg(rgb(body_bg))
        // pi-web box-shadow: 0 1px 0 border 42%（块底一条更深的细线）
        .shadow(vec![gpui::BoxShadow {
            color: gpui::rgba(crate::appearance::theme::border_alpha(t, 0x6b)).into(),
            offset: gpui::point(px(0.), px(1.)),
            blur_radius: px(0.),
            spread_radius: px(0.),
        }])
        .child(header)
        .child(
            div()
                .relative()
                .w_full()
                .child(
                    div()
                        .id(SharedString::from(format!("md-code-body-{hash:016x}")))
                        // Zed 同构：flex 滚动容器 + flex_none 内层（见
                        // render_code_block_doc 注释），块级横向滚动才有
                        // 溢出内容可滚
                        .flex()
                        .w_full()
                        .px(px(13.))
                        .py(px(11.))
                        // 字号挂容器（见 sized_text 注释）：代码块 = 槽位字号 -1
                        .text_size(px(spec.size - 1.))
                        .line_height(relative(1.62))
                        // c15：长行不再裁剪——nowrap + 横向滚动（pi-web <pre> 语义）
                        .whitespace_nowrap()
                        .overflow_x_scroll()
                        // 锁轴：纵向滚轮穿透给外层页面滚动，横向滚轮只滚本块
                        .restrict_scroll_to_axis()
                        .track_scroll(&scroll.handle)
                        .child(
                            div().flex_none().child(
                                StyledText::new(text).with_default_highlights(&base, highlights),
                            ),
                        ),
                )
                // 横向滑块：显式 absolute 细条（同 render_code_block_doc 注释）
                .child(
                    div()
                        .absolute()
                        .left(px(3.))
                        .right(px(3.))
                        .bottom(px(3.))
                        .h(px(8.))
                        .child(gpui_component::scroll::Scrollbar::horizontal(
                            &scroll.bar_state,
                            &scroll.handle,
                        )),
                ),
        )
}

fn push_color(
    highlights: &mut Vec<(std::ops::Range<usize>, HighlightStyle)>,
    start: usize,
    end: usize,
    c: [u8; 3],
) {
    let color = rgb(((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32);
    highlights.push((
        start..end,
        HighlightStyle { color: Some(color.into()), ..Default::default() },
    ));
}

/// 表格：外框圆角 7px、th bg_panel 650 字重、行分隔线、偶数行斑马纹
/// （pi-web .markdown-table-wrap）。
/// 单元格显示宽度权重：ASCII 记 1、CJK/全角记 2（HTML 表 auto layout 的
/// 近似度量）。
fn cell_weight(runs: &[Run]) -> usize {
    runs.iter().map(|r| r.text.chars().map(|c| if (c as u32) > 0x2e80 { 2 } else { 1 }).sum::<usize>()).sum()
}

fn render_table(head: &[Vec<Run>], rows: &[Vec<Vec<Run>>], t: &Theme) -> gpui::Div {
    // 列宽 = 列内最长单元格的显示宽度（钳制 [6,44]），归一化后全表共用
    // 同一组相对宽度——跨行对齐成网格，长单元格换行（pi-web HTML 表
    // auto layout 的近似；flex_1 按内容分配导致每行列位漂移，已废）
    let n_cols = head
        .len()
        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0))
        .max(1);
    const MIN_W: usize = 6;
    const MAX_W: usize = 44;
    let mut weights = vec![MIN_W; n_cols];
    for (ci, cell) in head.iter().enumerate() {
        weights[ci] = weights[ci].max(cell_weight(cell).clamp(MIN_W, MAX_W));
    }
    for row in rows {
        for (ci, cell) in row.iter().enumerate().take(n_cols) {
            weights[ci] = weights[ci].max(cell_weight(cell).clamp(MIN_W, MAX_W));
        }
    }
    let total: f32 = weights.iter().sum::<usize>() as f32;
    let fracs: Vec<f32> = weights.iter().map(|w| *w as f32 / total).collect();

    let cell_div = |frac: f32| {
        div()
            .w(relative(frac))
            .min_w_0()
            .px(px(10.))
            .py(px(6.))
    };

    // 字体大小设置.md：表格头 = 槽位字号，表格内容 = 槽位字号 -1，行高 1.7
    // （字号经 sized_text 挂容器，见其注释）
    let head_cells: Vec<gpui::AnyElement> = head
        .iter()
        .enumerate()
        .map(|(ci, cell)| {
            cell_div(fracs[ci])
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(crate::appearance::theme::mix_rgb(t.text, t.text_muted, 0.88)))
                .child(sized_text(cell, t, BASE, 1.7, crate::appearance::theme::mix_rgb(t.text, t.text_muted, 0.88), FontWeight::SEMIBOLD))
                .into_any_element()
        })
        .collect();
    let head_row = div()
        .flex()
        .bg(rgb(t.bg_panel))
        .border_b_1()
        .border_color(rgb(t.border))
        .children(head_cells);

    let body_rows: Vec<gpui::AnyElement> = rows
        .iter()
        .enumerate()
        .map(|(ri, row)| {
            let mut line = div()
                .flex()
                .border_b_1()
                .border_color(rgb(t.border));
            if ri % 2 == 1 {
                line = line.bg(rgba(t.bg_subtle));
            }
            for (ci, cell) in row.iter().enumerate().take(n_cols) {
                line = line.child(cell_div(fracs[ci]).child(sized_text(cell, t, 13., 1.7, t.text, FontWeight::NORMAL)));
            }
            line.into_any_element()
        })
        .collect();

    div()
        .w_full()
        .mt(px(8.))
        .mb(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(7.))
        .overflow_hidden()
        .child(head_row)
        .children(body_rows)
}

/// 图片：本地文件 gpui img() 直渲染；http/不存在 → alt 文本占位。
/// 相对路径按 md 文件目录解析（IMG_BASE，render_themed 设置）。
fn render_image(url: &str, width: Option<f32>, alt: &[Run], t: &Theme) -> gpui::AnyElement {    let placeholder = || {
        div()
            .w_full()
            .text_color(rgb(t.text_dim))
            .italic()
            .child(SharedString::from(format!("🖼 {}", runs_text(alt))))
            .into_any_element()
    };
    if url.starts_with("http://") || url.starts_with("https://") || url.is_empty() {
        return placeholder();
    }
    let ext = std::path::Path::new(url)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let format = match ext.as_str() {
        "png" => Some(gpui::ImageFormat::Png),
        "jpg" | "jpeg" => Some(gpui::ImageFormat::Jpeg),
        "gif" => Some(gpui::ImageFormat::Gif),
        "bmp" => Some(gpui::ImageFormat::Bmp),
        "svg" => Some(gpui::ImageFormat::Svg),
        "webp" => Some(gpui::ImageFormat::Webp),
        _ => None,
    };
    let path = std::path::Path::new(url);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        resolve_relative_image(path)
    };
    let Some((bytes, format)) = std::fs::read(&path).ok().zip(format) else {
        return placeholder();
    };
    div()
        .w_full()
        .child(
            gpui::img(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
                .max_w_full()
                .when_some(width, |d, w| d.w(px(w.max(24.))))
                .rounded(px(6.)),
        )
        .into_any_element()
}

/// 行内 code 盒（pi-web .markdown-inline-code）：mono（槽位字号 -1.12 =
/// 0.92em 的绝对差换算）+ bg-subtle + 圆角 5 + padding 1px 5px + 70% 边框
/// 描边；颜色恒 --text（引用块内不继承）。
/// gpui 行内 highlight 换不了字体家族，拆盒才能落 mono。
fn inline_code_box(text: &str, t: &Theme) -> AnyElement {
    let spec = active_md_spec();
    let style = TextStyle {
        color: rgb(t.text).into(),
        font_family: MONO_FAMILY.into(),
        font_size: px(spec.size - 1.12).into(),
        line_height: relative(1.5),
        ..Default::default()
    };
    let mut box_div = div()
        .max_w_full()
        .rounded(px(5.))
        .bg(rgba(t.bg_subtle))
        .border_1()
        .border_color(gpui::rgba((t.border << 8) | 0xb3))
        .px(px(5.))
        .py(px(1.))
        // 字号挂容器（见 sized_text 注释）：行内 code = 槽位 -1.12（0.92em）
        .text_size(px(spec.size - 1.12))
        .line_height(relative(1.5));
    // 路径形 code 可点开文件标签页（pi-web 消息链接的行内 code 变体）：
    // on_mouse_down 无需 stateful id，流式增长中也即时可点；非路径 code 盒零改动
    if let Some(path) = as_file_path(text) {
        let target = md_target();
        let accent = t.accent;
        box_div = box_div
            .cursor_pointer()
            .hover(move |s| s.text_color(rgb(accent)))
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                // 点击归 code 自己消费（开文件 tab），不冒泡给 md 预览容器的
                // 「点击进编辑」（on_mouse_down 在容器 on_click 记录按下态
                // 之前发，这里停了容器整条 click 链都不触发）
                cx.stop_propagation();
                open_path_with(target.clone(), path.clone(), cx);
            });
    }
    box_div
        .child(StyledText::new(text.to_string()).with_default_highlights(&style, Vec::new()))
        .into_any_element()
}

/// 行内富段：含 Code/Math 的段落拆 flex-wrap 段（文本段内部自然换行，
/// code = 等宽盒、公式 = 图片）；纯文本段落保持单一 StyledText。
fn paragraph_element(runs: &[Run], t: &Theme, color: u32) -> AnyElement {
    let rich = runs
        .iter()
        .any(|r| matches!(r.style, Style::Code | Style::Math | Style::DisplayMath));
    if !rich {
        return sized_text(runs, t, BASE, 1.7, color, FontWeight::NORMAL);
    }
    let flush =
        |row: gpui::Div, tr: &mut Vec<Run>, t: &Theme, color: u32| -> gpui::Div {
            if !tr.is_empty() {
                let taken = std::mem::take(tr);
                return row.child(div().max_w_full().child(sized_text(&taken, t, BASE, 1.7, color, FontWeight::NORMAL)));
            }
            row
        };
    let mut row = div().w_full().flex().flex_wrap().items_end();
    let mut text_run: Vec<Run> = Vec::new();
    for r in runs {
        match r.style {
            Style::Code => {
                row = flush(row, &mut text_run, t, color);
                row = row.child(inline_code_box(&r.text, t));
            }
            Style::Math => {
                row = flush(row, &mut text_run, t, color);
                row = row.child(crate::editor::markdown::render::math::inline_element(&r.text, t));
            }
            Style::DisplayMath => {
                row = flush(row, &mut text_run, t, color);
                // 块级公式：flex_wrap 下 w_full 独占一行
                row = row.child(div().w_full().child(crate::editor::markdown::render::math::block_element(&r.text, t)));
            }
            _ => text_run.push(r.clone()),
        }
    }
    row = flush(row, &mut text_run, t, color);
    row.into_any_element()
}

fn render_block(b: &MdBlock, depth: usize, t: &Theme, streaming: bool, color: u32) -> AnyElement {
    match b {
        MdBlock::Heading { level, runs } => {
            let doc = doc_mode();
            let spec = active_md_spec();
            // 文档模式 = Zed/GitHub 尺度（h1/h2 放大 + 底部分隔线）；聊天区
            // 维持 pi-web 标题规则（h1 1.16em …）
            let size = if doc {
                doc_size_for_level(*level, spec.size)
            } else {
                size_for_level(*level)
            };
            // pi-web 标题规则：h1/h2/h4-h6 = var(--text)（引用块内也是），
            // h3 = color-mix(text 88%, muted)
            let color = if *level == 3 {
                crate::appearance::theme::mix_rgb(t.text, t.text_muted, 0.88)
            } else {
                t.text
            };
            let text_size = if doc { size } else { spec.size + (size - BASE) };
            let underlined = doc && (*level <= 2);
            div()
                .w_full()
                .font_weight(if doc && *level == 1 { FontWeight::BOLD } else { FontWeight::SEMIBOLD })
                .text_size(px(text_size))
                .line_height(relative(1.35))
                .when(underlined, |d| {
                    d.pb(px(if *level == 1 { 7. } else { 5. }))
                        .border_b_1()
                        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x55)))
                })
                .child(runs_element(runs, t, size, 1.35, color, FontWeight::SEMIBOLD))
                .into_any_element()
        }
        MdBlock::Paragraph { runs } => paragraph_element(runs, t, color),
        MdBlock::Code { code, lang, .. } => {
            if doc_mode() {
                render_code_block_doc(lang, code, t)
            } else {
                render_code_block(lang, code, t, streaming).into_any_element()
            }
        }
        MdBlock::Quote { blocks } => div()
            .w_full()
            .border_l_3()
            .border_color(rgb(crate::appearance::theme::mix_rgb(t.border, t.text_muted, 0.75)))
            .rounded_r(px(6.))
            .bg(rgba(t.bg_subtle))
            .px(px(11.))
            .py(px(6.))
            // pi-web blockquote color: var(--text-muted)——经 color 参数落进
            // 内部段落的 StyledText（此前容器色被子元素硬编码 text 覆盖）
            .child(render_blocks(blocks, depth + 1, t, streaming, t.text_muted))
            .into_any_element(),
        MdBlock::ListItem { depth: d, marker, runs, task } => {
            let task = *task;
            // 悬挂缩进（pi-web ul{padding-left:22px} + li{padding-left:2px}
            // + list-style-position:outside）：marker 落在文字左侧的固定
            // 槽里，文字统一从 24px 起排，嵌套每层 +22px。
            // items_start（非 center）：pi-web outside marker 与内容第一行
            // 对齐——多行 item 时圆点/勾选框必须钉在首行，垂直居中会漂到
            // 整条 item 中间；行盒 = 1.7×字号，文本 marker（"1."）行盒同高
            // 自然落首行
            let slot = div()
                .w(px(24.))
                .flex_shrink_0()
                .flex()
                .items_start()
                .justify_end() // marker 靠槽右缘 = 悬挂在文字左侧
                .pr(px(4.)) // marker 右边到文字的视觉间隙（浏览器 outside marker）
                // 字体大小设置.md「list mark = 设置值」；绝对差换算下
                // 不随外层标题字号复利放大
                .text_size(px(active_md_spec().size))
                .line_height(relative(1.7))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(crate::appearance::theme::mix_rgb(t.accent, t.text_muted, 0.72)));
            let marker_el: AnyElement = match task {
                // c12: 任务项 marker = 14px 复选框（选中 = accent 10% 淡底 +
                // 55% 边框 + accent 对勾，pi-web :checked 规则）；top:0.35em
                // 钉在首行（pi-web input 绝对定位 top 0.35em parity）
                Some(checked) => slot
                    .child(
                        div()
                            .mt(px(5.))
                            .w(px(14.))
                            .h(px(14.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(rgb(if checked {
                                crate::appearance::theme::mix_rgb(t.accent, t.border, 0.55)
                            } else {
                                t.border
                            }))
                            .bg(rgb(if checked {
                                crate::appearance::theme::mix_rgb(t.accent, t.bg, 0.10)
                            } else {
                                t.bg
                            }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(checked, |b| {
                                b.child(
                                    div()
                                        .text_size(px(crate::ui::tokens::fixed::MD_CHECK_GLYPH))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(t.accent))
                                        .child(SharedString::from(tr("action.confirm_mark"))),
                                )
                            }),
                    )
                    .into_any_element(),
                // 无序列表：画 0.45em 实心圆（Chrome list-style disc 尺寸），
                // 首行行盒内垂直居中——"•" 字形在 14px 下只有 ~4px 且偏细
                None if marker == "•" => {
                    let spec = active_md_spec();
                    let dot = (0.45 * spec.size).round();
                    let first_line = 1.7 * spec.size;
                    slot.child(
                        div()
                            .mt(px(((first_line - dot) / 2.).max(0.)))
                            .size(px(dot))
                            .rounded(px(dot / 2.))
                            .bg(rgb(crate::appearance::theme::mix_rgb(t.accent, t.text_muted, 0.72))),
                    )
                    .into_any_element()
                }
                None => slot.child(SharedString::from(marker.clone())).into_any_element(),
            };
            div()
                .flex()
                .pl(px((d * 22) as f32))
                .child(marker_el)
                .child(div().flex_1().min_w_0().child(paragraph_element(runs, t, t.text)))
                .into_any_element()
        }
        MdBlock::Table { head, rows } => render_table(head, rows, t).into_any_element(),
        MdBlock::Image { url, alt, width } => render_image(url, *width, alt, t),
        MdBlock::Math { latex } => crate::editor::markdown::render::math::block_element(latex, t),
        MdBlock::Mermaid { source } => {
            // pi-web MermaidBlock parity：流式期间只显源码；失败回退源码块
            if !streaming {
                if let Some(el) = crate::editor::markdown::render::mermaid::diagram_element(source, t.dark) {
                    return el;
                }
            }
            render_code_block("mermaid", source, t, true).into_any_element()
        }
        MdBlock::Rule => {
            // pi-web hr: 渐变淡出线（linear-gradient 90deg transparent-border-
            // transparent）。gpui 渐变仅两停靠点，用底线 + 两端 18% 遮罩复现
            let base: gpui::Hsla = rgb(t.border).into();
            let bg: gpui::Hsla = rgb(t.bg).into();
            let bg_a0 = gpui::Hsla { a: 0., ..bg };
            div()
                .w_full()
                .h(px(1.))
                .relative()
                .child(div().size_full().bg(base))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(relative(0.18))
                        .h_full()
                        .bg(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(bg, 0.),
                            gpui::linear_color_stop(bg_a0, 1.),
                        )),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .w(relative(0.18))
                        .h_full()
                        .bg(gpui::linear_gradient(
                            90.,
                            gpui::linear_color_stop(bg_a0, 0.),
                            gpui::linear_color_stop(bg, 1.),
                        )),
                )
                .into_any_element()
        }
    }
}

/// pi-web MAX_MARKDOWN_CHARS（v56-3 c16）：超限跳过管线，退纯文本。
/// 只约束聊天气泡；文件预览走 doc_blocks/render_doc_item 虚拟化，不设上限。
const MAX_MARKDOWN_CHARS: usize = 100_000;

/// Render a markdown string as a vertical stack of styled GPUI elements.
/// `streaming`（v56-3 c15）：流式中的消息跳过 syntect 高亮与行号
/// （pi-web CodeBlock：流式期间 Prism 逐 chunk 重分词是最贵开销）。
pub fn render(src: &str, t: &Theme, streaming: bool) -> AnyElement {
    set_md_spec(crate::appearance::session_font());
    render_impl(src, t, streaming, false)
}

/// 用户气泡专用（v57）：HTML 标签不渲染、按原文显示——用户消息是发出
/// 内容的凭证，气泡吞标签会让用户无法核对 agent 实际收到的文本。
pub fn render_user(src: &str, t: &Theme) -> AnyElement {
    set_md_spec(crate::appearance::session_font());
    render_impl(src, t, false, false)
}

fn render_impl(src: &str, t: &Theme, streaming: bool, html: bool) -> AnyElement {
    if src.chars().count() > MAX_MARKDOWN_CHARS {
        return render_oversize(src, t);
    }
    let blocks = cached_blocks(src, html);
    if blocks.is_empty() {
        return div().into_any_element();
    }
    render_blocks(&blocks, 1, t, streaming, t.text).into_any_element()
}

// 解析结果缓存：gpui List 每帧对可见条目重建元素树，pulldown 解析是其中
// 最贵的纯 CPU 段（大 markdown 消息数百 µs～ms 级），滚动/流式时每帧白打。
// key = 源文哈希 + 长度 + html 标志，命中后全等校验防碰撞；线程局部
// VecDeque 当 LRU（元素构建只在主线程）。流式期间末条消息源文每 delta
// 一变，最多占满队头被挤出，不影响命中态。
thread_local! {
    static MD_PARSE_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize, bool), (String, std::rc::Rc<Vec<MdBlock>>))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const MD_PARSE_CACHE_CAP: usize = 128;

/// FNV-1a 按 8 字节块处理（进程内去重键，非加密；命中另有全等校验兜底）
pub(crate) fn hash_str(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325;
    let mut chunks = s.as_bytes().chunks_exact(8);
    for chunk in &mut chunks {
        let mut word = u64::from_le_bytes(chunk.try_into().unwrap());
        for _ in 0..8 {
            h = (h ^ (word & 0xff)).wrapping_mul(0x100000001b3);
            word >>= 8;
        }
    }
    for &byte in chunks.remainder() {
        h = (h ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    h
}

fn cached_blocks(src: &str, html: bool) -> std::rc::Rc<Vec<MdBlock>> {
    let key = (hash_str(src), src.len(), html);
    MD_PARSE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1 .0.as_str() == src {
                let blocks = std::rc::Rc::clone(&entry.1 .1);
                cache.push_back(entry); // 刷新到队尾（LRU）
                return blocks;
            }
        }
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TABLES);
        // c12: 任务列表 + GFM 自动链接；c14: YAML frontmatter 吞掉
        opts.insert(Options::ENABLE_TASKLISTS);
        opts.insert(Options::ENABLE_GFM);
        opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
        // v57-2: $…$ / $$…$$ 数学（pi-web remark-math parity）
        opts.insert(Options::ENABLE_MATH);
        let events: Vec<Event> = Parser::new_ext(src, opts).collect();
        let parsed = std::rc::Rc::new(parse_blocks(&events, html));
        cache.push_back((key, (src.to_string(), std::rc::Rc::clone(&parsed))));
        if cache.len() > MD_PARSE_CACHE_CAP {
            cache.pop_front();
        }
        parsed
    })
}

/// Html/InlineHtml 的字面显示（html=false 路径）：标签原文可见。
fn literal_runs(html_src: &str) -> Vec<Run> {
    vec![Run { text: html_src.to_string(), style: Style::Normal, url: None }]
}

/// 超长消息（pi-web ⚠ Message content is very large）：提示行 + 纯文本
/// 滚动视图（pi-web 展开后的 pre maxHeight 420；省去一次点击）。
fn render_oversize(src: &str, t: &Theme) -> AnyElement {
    let kb = src.len() / 1024;
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_color(rgb(0xca8a04))
                .text_size(px(crate::ui::tokens::fixed::MD_WARN))
                .child(SharedString::from(format!(
                    "⚠ {} ({}KB)",
                    crate::i18n::tr("messages.too_large"),
                    kb
                ))),
        )
        .child(
            div()
                .id("md-oversize")
                .max_h(px(420.))
                .overflow_y_scroll()
                .restrict_scroll_to_axis()
                .font_family(MONO_FAMILY)
                .text_size(px(crate::ui::tokens::fixed::MD_OVERSIZE))
                .line_height(relative(1.5))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(src.to_string())),
        )
        .into_any_element()
}

/// 渲染列钳制的 flex 段（gpui 0.2.2 无带参 flex_grow，直写 style 精修）：
/// basis 0 + 指定 grow 占比 + shrink 0；min_w 用作窄面板下的保底钳制。
pub(crate) fn flex_span(grow: f32, min_w: Option<f32>) -> gpui::Div {
    let mut d = div().flex_basis(px(0.));
    d.style().flex_grow = Some(grow);
    d.style().flex_shrink = Some(0.);
    if let Some(w) = min_w {
        d = d.min_w(px(w));
    }
    d
}
