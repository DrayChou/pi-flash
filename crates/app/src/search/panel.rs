//! 搜索面板（083）：搜索条 + 结果区。Zed `ProjectSearchBar` 的版式移植。
//!
//! 版式（自上而下，Zed render() 逐行对照）：
//!
//! ```text
//! ┌ 搜索条 ───────────────────────────────────────────────────────────┐
//! │ [查询输入框 ····················│ Aa  ab|  .*  ]  [过滤][替换]       │
//! │                                      [1/23 ↻] [︿] [﹀]             │
//! ├ 替换行（替换钮打开时）────────────────────────────────────────────┤
//! │                [替换为 ·················] [换下一个][全部替换]      │
//! ├ 过滤行（过滤钮打开时）────────────────────────────────────────────┤
//! │                [包含 ·······] [排除 ·······] [扫描忽略项]           │
//! └───────────────────────────────────────────────────────────────────┘
//!   结果区：按文件分组的摘录行（等宽等高，命中高亮）
//! ```
//!
//! 与 Zed 的差异（都写在 `docs/模块设计/083-项目搜索.md`）：
//! - Zed 把 query/replace/include/exclude 做成内嵌 `Editor`（带语法高亮、
//!   多行）；PF 复用全应用统一的 `TextInput`（单行、带 IME 与剪贴板），
//!   查询语法高亮对普通用户无价值；
//! - Zed 的计数在右侧 `[︿][1/23][﹀]`，PF 同位同序；
//! - 无 `ToggleFocus`（Esc 聚焦结果）：PF 结果列表点击即开文件，不需要
//!   键盘进入结果区的第二焦点位。

use gpui::prelude::*;
use gpui::{Context, SharedString, Window, div, px, rgb};

use super::engine::SearchOpts;
use super::{Phase, SearchToggle, results};
use crate::Chat;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::ui::kit::{icon_toggle, text_toggle};
use crate::ui::tokens::space;

/// 搜索视图根（content.rs `ContentView::Search` 的落点）。
pub(crate) fn view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    // 首次进入 / 快捷键重复触发：把焦点交给查询框（每帧最多补一次，见
    // 下面的 focus_query 消费点）
    if chat.ws.search.focus_query {
        // 首次开面板的当帧内件还没建（TextInput 懒建），保持请求到下一帧；
        // 内件在树里了再推焦点，避免把 fallback 句柄推上空焦点位
        if let Some(q) = chat.ws.search.query.clone() {
            if q.read(cx).has_inner() {
                chat.ws.search.focus_query = false;
                let h = q.read(cx).focus_handle_in(cx);
                window.focus(&h);
            }
        } else {
            chat.ws.search.focus_query = false;
        }
    }
    div()
        .id("project-search")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .text_color(rgb(t.text))
        .child(search_bar(chat, weak, t))
        .child(results::view(chat, weak, t))
        .into_any_element()
}

/// 搜索条（查询行 + 可选替换行 + 可选过滤行 + 错误行）。
fn search_bar(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let ui = &chat.ws.search;
    let opts = ui.opts;
    let running = ui.phase == Phase::Running;
    let has_matches = ui.matches > 0;

    // ---- 查询行 --------------------------------------------------------
    // Zed 截图首位 = ✕（收起搜索）：PF 对应「关搜索 tab」
    let mut query_row = div()
        .flex()
        .items_center()
        .gap(px(space::SP2))
        .w_full()
        .child(icon_toggle(
            "psq-close",
            "close",
            false,
            weak,
            "search.close",
            false,
            |c, cx| c.close_search_tab(cx),
        ));
    if let Some(q) = ui.query.clone() {
        query_row = query_row.child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(space::SP2))
                // 2026-10-21 用户定案：搜索框**不加 border**（也不加底色/
                // 圆角）——输入框走 flat 档，文字直接坐在面板底上，右侧留
                // 内联开关位。焦点态由光标闪烁表达（Zed 的搜索框同理）。
                .pl(px(space::SP4))
                .pr(px(space::SP1))
                .child(div().flex_1().min_w_0().py(px(4.)).child(q.clone()))
                .child(text_toggle(
                    "psq-aa",
                    "Aa",
                    opts.case_sensitive,
                    weak,
                    "search.case_sensitive",
                    |c, cx| c.search_toggle(SearchToggle::CaseSensitive, cx),
                ))
                .child(text_toggle(
                    "psq-wd",
                    "ab|",
                    opts.whole_word,
                    weak,
                    "search.whole_word",
                    |c, cx| c.search_toggle(SearchToggle::WholeWord, cx),
                ))
                .child(text_toggle(
                    "psq-re",
                    ".*",
                    opts.regex,
                    weak,
                    "search.regex",
                    |c, cx| c.search_toggle(SearchToggle::Regex, cx),
                )),
        );
    }
    // 过滤 / 替换开关（Zed mode_column：过滤在左、替换在中）
    query_row = query_row.child(icon_toggle(
        "psq-filters",
        "sliders-horizontal",
        ui.show_filters,
        weak,
        "search.filters",
        false,
        |c, cx| c.search_toggle(SearchToggle::Filters, cx),
    ));
    query_row = query_row.child(icon_toggle(
        "psq-replace",
        "replace",
        ui.show_replace,
        weak,
        "search.replace",
        false,
        |c, cx| c.search_toggle(SearchToggle::Replace, cx),
    ));
    // 命中计数 + 转圈 + prev/next（Zed matches_column）
    let counter: SharedString = {
        let base = format!("{}/{}", ui.active.map(|a| a + 1).unwrap_or(0), ui.matches);
        if ui.limit {
            format!("{base}+").into()
        } else {
            base.into()
        }
    };
    query_row = query_row.child(
        div()
            .flex()
            .items_center()
            .gap(px(space::SP1))
            .pl(px(space::SP2))
            .border_l_1()
            .border_color(rgb(t.border))
            .child(icon_toggle(
                "psq-prev",
                "chevron-left",
                false,
                weak,
                "search.prev",
                !has_matches,
                |c, cx| c.search_step(false, cx),
            ))
            .child(icon_toggle(
                "psq-next",
                "chevron-right",
                false,
                weak,
                "search.next",
                !has_matches,
                |c, cx| c.search_step(true, cx),
            ))
            .child(
                div()
                    .min_w(px(48.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(space::SP2))
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(if has_matches {
                        t.text_dim
                    } else {
                        t.text_faint
                    }))
                    .child(SharedString::from(counter))
                    .when(running, |d| d.child(crate::ui::spinner(12., t.accent))),
            ),
    );

    let mut col = div()
        .flex_shrink_0()
        .w_full()
        .px(px(space::SP6))
        .pt(px(space::SP4))
        .pb(px(space::SP3))
        .flex()
        .flex_col()
        .gap(px(space::SP2))
        .border_b_1()
        .border_color(rgb(t.border))
        .child(query_row);

    // ---- 替换行（Zed replace_line：左侧留出与查询框对齐的空槽）-------
    if ui.show_replace {
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(space::SP2))
            .w_full()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(px(space::SP3))
                    .pr(px(space::SP1))
                    .border_1()
                    .border_color(rgb(t.border))
                    .rounded(px(crate::ui::tokens::radius::CTRL))
                    .bg(rgb(t.bg_panel))
                    .child(div().py(px(2.)).child(match ui.replace.clone() {
                        Some(e) => e.into_any_element(),
                        None => div().into_any_element(),
                    })),
            );
        row = row
            .child(icon_toggle(
                "psq-replace-next",
                "replace",
                false,
                weak,
                "search.replace_next",
                !has_matches || running,
                |c, cx| c.search_replace_next(cx),
            ))
            .child(icon_toggle(
                "psq-replace-all",
                "replace-all",
                false,
                weak,
                "search.replace_all",
                !has_matches || running,
                |c, cx| c.search_replace_all(cx),
            ));
        col = col.child(row);
    }

    // ---- 过滤行（Zed filter_line：包含 / 排除 / 扫描忽略项）-----------
    if ui.show_filters {
        let input_slot = |input: Option<gpui::Entity<crate::ui::TextInput>>| {
            div()
                .flex_1()
                .min_w_0()
                .pl(px(space::SP3))
                .pr(px(space::SP1))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(px(crate::ui::tokens::radius::CTRL))
                .bg(rgb(t.bg_panel))
                .child(div().py(px(2.)).child(match input {
                    Some(e) => e.into_any_element(),
                    None => div().into_any_element(),
                }))
        };
        let row = div()
            .flex()
            .items_center()
            .gap(px(space::SP2))
            .w_full()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(space::SP2))
                    .child(input_slot(ui.include.clone()))
                    .child(input_slot(ui.exclude.clone())),
            )
            .child(icon_toggle(
                "psq-ignored",
                "eye-off",
                ui.opts.include_ignored,
                weak,
                "search.include_ignored",
                false,
                |c, cx| c.search_toggle(SearchToggle::IncludeIgnored, cx),
            ));
        col = col.child(row);
    }

    // ---- 错误 / 通知行（Zed query_error_line / filter_error_line）------
    if let Some(err) = &ui.error {
        col = col.child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.danger))
                .child(SharedString::from(err.clone())),
        );
    }
    if let Some((msg, bad)) = &ui.notice {
        col = col.child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(if *bad { t.danger } else { t.text_dim }))
                .child(SharedString::from(msg.clone())),
        );
    }
    col.into_any_element()
}

/// 空态文案（结果区无行时由 [`results`] 调用）。
pub(crate) fn empty_hint(phase: Phase, has_query: bool) -> &'static str {
    match phase {
        Phase::Idle if !has_query => tr("search.hint_idle"),
        Phase::Idle => tr("search.hint_idle"),
        Phase::Running => tr("search.searching"),
        Phase::Done => tr("search.no_results"),
    }
}

/// `SearchOpts` 的可读标签（自动化快照用）。
pub(crate) fn opts_labels(o: SearchOpts) -> Vec<&'static str> {
    let mut v = Vec::new();
    if o.case_sensitive {
        v.push("case_sensitive");
    }
    if o.whole_word {
        v.push("whole_word");
    }
    if o.regex {
        v.push("regex");
    }
    if o.include_ignored {
        v.push("include_ignored");
    }
    v
}
