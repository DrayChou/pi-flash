//! pulldown-cmark 解析层：inline 样式栈、链接切分、块事件折叠
//! （parse_blocks/parse_block）与表格单元格切分。产 MdBlock，纯函数。

use pulldown_cmark::{CodeBlockKind, Event, Tag, TagEnd};
use super::*;

fn style_push(styles: &mut Vec<StyleEntry>, tag: &Tag) {
    let (base, base_url) = styles
        .last()
        .map(|(s, u)| (*s, u.clone()))
        .unwrap_or((Style::Normal, None));
    let next = match tag {
        Tag::Strong => (
            match base {
                Style::Italic | Style::BoldItalic => Style::BoldItalic,
                _ => Style::Bold,
            },
            base_url,
        ),
        Tag::Emphasis => (
            match base {
                Style::Bold | Style::BoldItalic => Style::BoldItalic,
                _ => Style::Italic,
            },
            base_url,
        ),
        Tag::Link { dest_url, .. } => (Style::Link, Some(dest_url.to_string())),
        Tag::Strikethrough => (Style::Strike, base_url),
        _ => (base, base_url),
    };
    styles.push(next);
}

fn style_pop(styles: &mut Vec<StyleEntry>, end: &TagEnd) {
    match end {
        TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link | TagEnd::Strikethrough => {
            styles.pop();
        }
        _ => {}
    }
}

/// GFM autolink-literal parity (pi-web remark-gfm；pulldown 0.13 无此扩展)：
/// 把普通文本按裸 http(s) URL 切成 Link run。尾部标点剥到 URL 外。
fn split_links(text: &str) -> Vec<(String, bool)> {
    const HTTPS: &str = "https://";
    const HTTP: &str = "http://";
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut rest = text;
    loop {
        let start = [HTTPS, HTTP]
            .iter()
            .filter_map(|sc| rest.find(sc))
            .min();
        let Some(start) = start else { break };
        // 前缀须是词边界（避免 "foohttps://" 误切）
        let boundary_ok = start == 0
            || !rest[..start]
                .chars()
                .next_back()
                .map(|c| c.is_alphanumeric())
                .unwrap_or(false);
        if !boundary_ok {
            // 跳过这个伪起点，从下一个字符继续找
            let (head, tail) = rest.split_at(start + 1);
            out.push((head.to_string(), false));
            rest = tail;
            continue;
        }
        // URL 止于空白或尖括号；尾部标点剥出
        let candidate = &rest[start..];
        let end = candidate
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
            .unwrap_or(candidate.len());
        let mut url = &candidate[..end];
        while url
            .chars()
            .next_back()
            .map(|c| "!.,;:?\")'".contains(c))
            .unwrap_or(false)
        {
            url = &url[..url.len() - 1];
        }
        if url.len() > HTTPS.len() {
            if start > 0 {
                out.push((rest[..start].to_string(), false));
            }
            out.push((url.to_string(), true));
            rest = &rest[start + url.len()..];
        } else {
            // 裸 scheme 无主体——原样保留
            let (head, tail) = rest.split_at(start + 1);
            out.push((head.to_string(), false));
            rest = tail;
        }
    }
    if out.is_empty() {
        vec![(text.to_string(), false)]
    } else {
        out.push((rest.to_string(), false));
        out
    }
}

/// 块级边界事件：列表项内容里遇到它就得停手（块级 `Start` 走递归解析、
/// 块级 `End` 只跳过标记）。**行内事件不在此列**——`Code`/强调/链接/
/// 软换行/行内公式都由 [`collect_inline`] 消费。
///
/// ⚠️ 含 `TagEnd::Item`：列表项内容用 [`collect_inline_peek`]（不消费终止
/// 事件），终止符原样留给外层 while 判定。
fn is_block_boundary(e: &Event) -> bool {
    matches!(
        e,
        Event::Start(
            Tag::Paragraph
                | Tag::List(_)
                | Tag::CodeBlock(_)
                | Tag::BlockQuote(_)
                | Tag::Table(_)
                | Tag::Image { .. }
                | Tag::HtmlBlock
        ) | Event::End(
            TagEnd::Item
                | TagEnd::Paragraph
                | TagEnd::List(_)
                | TagEnd::CodeBlock
                | TagEnd::BlockQuote(_)
                | TagEnd::Table
                | TagEnd::Image
                | TagEnd::HtmlBlock
        )
    )
}

/// Collect inline runs until `is_end` matches (**consuming** the end event) —
/// existing callers（段落 / 表格单元 / 图片 alt）的口径。
fn collect_inline(
    events: &[Event],
    i: &mut usize,
    is_end: &dyn Fn(&Event) -> bool,
    html: bool,
) -> Vec<Run> {
    let runs = collect_inline_peek(events, i, is_end, html);
    if *i < events.len() && is_end(&events[*i]) {
        *i += 1;
    }
    runs
}

/// 同 [`collect_inline`]，但命中 `is_end` 时**不**消费终止事件（光标留在它上面）。
/// 列表项内容用这个：终止符 `End(TagEnd::Item)` 必须留给外层 while 判定，
/// 吃掉它会把下一项并进本项。
fn collect_inline_peek(
    events: &[Event],
    i: &mut usize,
    is_end: &dyn Fn(&Event) -> bool,
    html: bool,
) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut styles: Vec<StyleEntry> = Vec::new();
    let mut text = String::new();
    let mut cur = Style::Normal;
    let mut cur_url: Option<String> = None;

    fn flush(text: &mut String, cur: Style, cur_url: &Option<String>, runs: &mut Vec<Run>) {
        if text.is_empty() {
            return;
        }
        let taken = std::mem::take(text);
        if cur == Style::Normal {
            // GFM 自动链接：普通文本里的裸 URL 提为 Link run（目标 = 文本本身）
            for (seg, is_link) in split_links(&taken) {
                if !seg.is_empty() {
                    let url = is_link.then(|| seg.clone());
                    runs.push(Run {
                        text: seg,
                        style: if is_link { Style::Link } else { Style::Normal },
                        url,
                    });
                }
            }
        } else {
            runs.push(Run { text: taken, style: cur, url: cur_url.clone() });
        }
    }

    while *i < events.len() {
        match &events[*i] {
            e if is_end(e) => {
                flush(&mut text, cur, &cur_url, &mut runs);
                return runs;
            }
            Event::Text(t) => {
                let (s, u) = styles
                    .last()
                    .map(|(s, u)| (*s, u.clone()))
                    .unwrap_or((Style::Normal, None));
                if s != cur || u != cur_url {
                    flush(&mut text, cur, &cur_url, &mut runs);
                    cur = s;
                    cur_url = u;
                }
                text.push_str(t);
            }
            Event::InlineHtml(h) => {
                // v57-1: 行内 HTML；html=false（用户气泡）时标签原文可见。
                // 开/闭标签跨事件维持样式栈（配对标签样式不丢）
                flush(&mut text, cur, &cur_url, &mut runs);
                if html {
                    use crate::editor::markdown::render::html::InlineHtmlEffect as E;
                    match crate::editor::markdown::render::html::fragment_effect(h) {
                        E::StylePush(st, url) => styles.push((st, url)),
                        E::StylePop => {
                            styles.pop();
                        }
                        E::Runs(rs) => runs.extend(rs),
                    }
                    let (s, u) = styles
                        .last()
                        .map(|(s, u)| (*s, u.clone()))
                        .unwrap_or((Style::Normal, None));
                    cur = s;
                    cur_url = u;
                } else {
                    runs.extend(literal_runs(h));
                    cur = Style::Normal;
                    cur_url = None;
                    styles.clear();
                }
            }
            Event::InlineMath(tex) => {
                // v57-2: 行内公式标记 run（渲染期拆段成图）
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run { text: tex.to_string(), style: Style::Math, url: None });
            }
            Event::DisplayMath(tex) => {
                // 多行 $$…$$ 的 DisplayMath 事件发在段落流内（实测），同用标记
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run { text: tex.to_string(), style: Style::DisplayMath, url: None });
            }
            Event::Code(c) => {
                flush(&mut text, cur, &cur_url, &mut runs);
                runs.push(Run { text: c.to_string(), style: Style::Code, url: None });
            }
            // html=false 即用户气泡路径（render_user 唯一调用方）：段内软
            // 换行保留 \n（pi-web parity：.markdown-user-message p 的
            // white-space:pre-wrap——用户分次回车的多行按原文折行，而非
            // CommonMark 默认折叠成空格）
            Event::SoftBreak => text.push(if html { ' ' } else { '\n' }),
            Event::HardBreak => text.push('\n'),
            Event::Start(tag) => style_push(&mut styles, tag),
            Event::End(end) => style_pop(&mut styles, end),
            _ => {}
        }
        *i += 1;
    }
    flush(&mut text, cur, &cur_url, &mut runs);
    runs
}

// ---------------------------------------------------------------------------
// block parsing
// ---------------------------------------------------------------------------

/// HTML 块内容三路分发：doc 模式 img 提取（html_block_doc）/ 安全子集映射
/// （html::blocks）/ 用户气泡标签原文。裸 `Event::Html` 与
/// `Start(Tag::HtmlBlock)` 包裹层共用。
fn html_block_dispatch(raw: &str, html: bool) -> Vec<MdBlock> {
    if html && doc_mode() {
        html_block_doc(raw)
    } else if html {
        crate::editor::markdown::render::html::blocks(raw)
    } else {
        vec![MdBlock::Paragraph { runs: literal_runs(raw) }]
    }
}

pub(super) fn parse_blocks(events: &[Event], html: bool) -> Vec<MdBlock> {
    let mut out: Vec<MdBlock> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::HtmlBlock) => {
                // pulldown 0.10+ 把块级 HTML 包进 Start/End(HtmlBlock)。此前
                // 落进 parse_block 的兜底「跳到容器尾」，内部 Event::Html 整段
                // 被吞——README 头部 <p><img></p>+tagline 就这样消失（图片
                // 不显示的真根因，v57-1/doc 模式两条 HTML 路径都没被走到）。
                i += 1;
                let mut raw = String::new();
                while i < events.len() && !matches!(events[i], Event::End(TagEnd::HtmlBlock)) {
                    if let Event::Html(h) | Event::InlineHtml(h) = &events[i] {
                        raw.push_str(h);
                    }
                    i += 1;
                }
                i += 1; // End(HtmlBlock)
                out.extend(html_block_dispatch(&raw, html));
            }
            Event::Start(tag) => {
                let tag = tag.clone();
                i += 1;
                parse_block(events, &mut i, tag, &mut out, 0, html);
            }
            Event::Rule => {
                out.push(MdBlock::Rule);
                i += 1;
            }
            Event::Text(t) => {
                out.push(MdBlock::Paragraph {
                    runs: vec![Run { text: t.to_string(), style: Style::Normal, url: None }],
                });
                i += 1;
            }
            Event::Html(h) => {
                out.extend(html_block_dispatch(h, html));
                i += 1;
            }
            Event::DisplayMath(tex) => {
                // v57-2: 块级公式（$$…$$）
                out.push(MdBlock::Math { latex: tex.to_string() });
                i += 1;
            }
            Event::InlineHtml(h) => {
                if html {
                    let runs = crate::editor::markdown::render::html::inline_runs(h);
                    if !runs.is_empty() {
                        out.push(MdBlock::Paragraph { runs });
                    }
                } else {
                    out.push(MdBlock::Paragraph { runs: literal_runs(h) });
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// True when the paragraph from `start` holds nothing but whitespace and a
/// single image — those render as a real image block (pi-web img: block).
fn solo_image(events: &[Event], start: usize) -> Option<String> {
    let mut url: Option<String> = None;
    let mut depth = 0usize;
    let mut i = start;
    while i < events.len() {
        match &events[i] {
            Event::End(TagEnd::Paragraph) => break,
            Event::Start(Tag::Image { dest_url, .. }) => {
                if depth == 0 {
                    if url.is_some() {
                        return None;
                    }
                    url = Some(dest_url.to_string());
                }
                depth += 1;
            }
            Event::End(TagEnd::Image) => depth -= 1,
            _ if depth == 0 => {
                if let Event::Text(t) = &events[i] {
                    if t.trim().is_empty() {
                        i += 1;
                        continue;
                    }
                }
                return None;
            }
            _ => {}
        }
        i += 1;
    }
    url
}

fn parse_block(
    events: &[Event],
    i: &mut usize,
    tag: Tag,
    out: &mut Vec<MdBlock>,
    depth: usize,
    html: bool,
) {
    match tag {
        Tag::Paragraph => {
            if let Some(url) = solo_image(events, *i) {
                // 独立图片段落：定位 Image，收 alt，越过 End(Paragraph)
                while !matches!(events[*i], Event::Start(Tag::Image { .. })) {
                    *i += 1;
                }
                if let Event::Start(Tag::Image { .. }) = &events[*i] {
                    *i += 1;
                    let alt = collect_inline(events, i, &|e| {
                        matches!(e, Event::End(TagEnd::Image))
}, html);
                    out.push(MdBlock::Image { url, alt, width: None });
                }
                while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Paragraph)) {
                    *i += 1;
                }
                *i += 1;
            } else {
                let runs = collect_inline(events, i, &|e| {
                    matches!(e, Event::End(TagEnd::Paragraph))
}, html);
                out.push(MdBlock::Paragraph { runs });
            }
        }
        Tag::Heading { level, .. } => {
            let runs = collect_inline(events, i, &|e| {
                matches!(e, Event::End(TagEnd::Heading(_)))
}, html);
            out.push(MdBlock::Heading { level: level as u8, runs });
        }
        Tag::BlockQuote(_) => {
            let mut inner = Vec::new();
            while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::BlockQuote(_))) {
                if let Event::Start(t) = &events[*i] {
                    let t = t.clone();
                    *i += 1;
                    parse_block(events, i, t, &mut inner, depth + 1, html);
                } else {
                    *i += 1;
                }
            }
            *i += 1; // End(BlockQuote)
            out.push(MdBlock::Quote { blocks: inner });
        }
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                CodeBlockKind::Fenced(l) => l.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
            let mut code = String::new();
            while *i < events.len() {
                match &events[*i] {
                    Event::Text(t) => code.push_str(t),
                    Event::End(TagEnd::CodeBlock) => {
                        *i += 1;
                        break;
                    }
                    _ => {}
                }
                *i += 1;
            }
            if lang.trim() == "mermaid" {
                // v57-3: mermaid 图（渲染期流式回退源码）
                out.push(MdBlock::Mermaid { source: code });
            } else {
                out.push(MdBlock::Code { lang, code });
            }
        }
        Tag::Table(_) => {
            let mut head: Vec<Vec<Run>> = Vec::new();
            let mut rows: Vec<Vec<Vec<Run>>> = Vec::new();
            while *i < events.len() {
                match &events[*i] {
                    Event::End(TagEnd::Table) => {
                        *i += 1;
                        break;
                    }
                    Event::Start(Tag::TableHead) => {
                        *i += 1;
                        head = parse_table_cells(
                            events,
                            i,
                            &|e| matches!(e, Event::End(TagEnd::TableHead)),
                            html,
                        );
                    }
                    Event::Start(Tag::TableRow) => {
                        *i += 1;
                        let row = parse_table_cells(
                            events,
                            i,
                            &|e| matches!(e, Event::End(TagEnd::TableRow)),
                            html,
                        );
                        rows.push(row);
                    }
                    _ => *i += 1,
                }
            }
            out.push(MdBlock::Table { head, rows });
        }
        Tag::Image { dest_url, .. } => {
            let alt = collect_inline(events, i, &|e| matches!(e, Event::End(TagEnd::Image)), html);
            out.push(MdBlock::Image { url: dest_url.to_string(), alt, width: None });
        }
        Tag::List(start) => {
            let ordered = start.is_some();
            let mut n = start.unwrap_or(1);
            while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::List(_))) {
                if let Event::Start(Tag::Item) = &events[*i] {
                    *i += 1;
                    let marker = if ordered {
                        format!("{n}.")
                    } else {
                        "•".to_string()
                    };
                    n += 1;
                    let mut runs: Vec<Run> = Vec::new();
                    let mut nested = Vec::new();
                    let mut task: Option<bool> = None;
                    while *i < events.len() && !matches!(events[*i], Event::End(TagEnd::Item)) {
                        match &events[*i] {
                            // c12: 任务列表复选框（pi-web 自绘 14px accent 对勾）
                            Event::TaskListMarker(checked) => {
                                task = Some(*checked);
                                *i += 1;
                            }
                            // 只接**块级** Start（段落 / 嵌套列表 / 代码块 / 引用 /
                            // 表格 / 图片 / HTML 块）。行内 Start（Link / Strong /
                            // Emphasis / Strikethrough）必须落到下面的
                            // collect_inline_peek —— 旧写法 `Event::Start(inner_tag)`
                            // 把行内开标签也抢走（内层 `_ => {}` 静默空转），
                            // 于是列表里的链接/强调丢了样式。
                            Event::Start(inner_tag) if is_block_boundary(&events[*i]) => {
                                let inner_tag = inner_tag.clone();
                                *i += 1;
                                match &inner_tag {
                                    Tag::Paragraph => {
                                        let r = collect_inline(events, i, &|e| {
                                            matches!(e, Event::End(TagEnd::Paragraph))
}, html);
                                        if !runs.is_empty() {
                                            if let Some(last) = runs.last_mut() {
                                                last.text.push(' ');
                                            }
                                        }
                                        runs.extend(r);
                                    }
                                    Tag::List(_) => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1, html);
                                    }
                                    Tag::CodeBlock(_) | Tag::BlockQuote(_) | Tag::Table(_) => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1, html);
                                    }
                                    Tag::Image { .. } => {
                                        parse_block(events, i, inner_tag, &mut nested, depth + 1, html);
                                    }
                                    Tag::HtmlBlock => {
                                        // 列表项内包裹式 HTML 块：收拢 chunks 走
                                        // 统一分发（i 已过 Start，此处到 End 为止）
                                        let mut raw = String::new();
                                        while *i < events.len()
                                            && !matches!(events[*i], Event::End(TagEnd::HtmlBlock))
                                        {
                                            if let Event::Html(h) | Event::InlineHtml(h) =
                                                &events[*i]
                                            {
                                                raw.push_str(h);
                                            }
                                            *i += 1;
                                        }
                                        *i += 1; // End(HtmlBlock)
                                        nested.extend(html_block_dispatch(&raw, html));
                                    }
                                    _ => {}
                                }
                            }
                            // 块级收尾标记（End(Paragraph)/End(CodeBlock)…）：其内容
                            // 已由上面的 Start 臂消费，这里只跳过标记本身。
                            // End(Item) 不在此列——它是本项终止符，交给外层 while。
                            Event::End(_) if is_block_boundary(&events[*i]) => {
                                *i += 1;
                            }
                            // 其余全是**行内**事件（Text / Code / 强调 / 链接 /
                            // 软换行 / 行内公式…）：一律交给 collect_inline_peek，
                            // 与段落同源。⚠️ 此前这里只有 `Event::Text` 一个分支，
                            // 其余全被 `_ => *i += 1` **静默吞掉**——紧凑列表
                            // （tight list，CommonMark 默认不带 <p>）里的 `code`、
                            // **粗体**、链接全部不渲染（用户实测：整条列表只剩
                            // 「：、、」）。
                            _ => runs.extend(collect_inline_peek(
                                events,
                                i,
                                &|e| is_block_boundary(e),
                                html,
                            )),
                        }
                    }
                    *i += 1; // End(Item)
                    out.push(MdBlock::ListItem { depth, marker, runs, task });
                    out.extend(nested);
                } else {
                    *i += 1;
                }
            }
            *i += 1; // End(List)
        }
        _ => {
            // unknown/unsupported container: skip to its end (depth-naive but fine
            // for the subset markdown chat needs)
            let mut d = 1usize;
            while *i < events.len() && d > 0 {
                match &events[*i] {
                    Event::Start(_) => d += 1,
                    Event::End(_) => d -= 1,
                    _ => {}
                }
                *i += 1;
            }
        }
    }
}

/// Parse `Start(TableCell) … End(TableCell)` sequences until `is_end` fires
/// (the row/head End event), one `Vec<Run>` per cell.
fn parse_table_cells(
    events: &[Event],
    i: &mut usize,
    is_end: &dyn Fn(&Event) -> bool,
    html: bool,
) -> Vec<Vec<Run>> {
    let mut cells: Vec<Vec<Run>> = Vec::new();
    while *i < events.len() {
        if is_end(&events[*i]) {
            *i += 1;
            break;
        }
        if matches!(events[*i], Event::Start(Tag::TableCell)) {
            *i += 1;
            let runs =
                collect_inline(events, i, &|e| matches!(e, Event::End(TagEnd::TableCell)), html);
            cells.push(runs);
        } else {
            *i += 1;
        }
    }
    cells
}
