//! 会话查询面板（013 的弹窗版 → 083 同版式面板，2026-10-21 用户定案）。
//!
//! **只查询**：没有替换 / 包含排除过滤 / 正则 / 大小写 / 整词——一个查询框
//! 加结果列表，其余版式与项目搜索（[`super::results`]）逐项一致：
//! - 组头复用 [`super::results::card_header`]（块 p4、内条 rounded4 + border、
//!   28×28 收展钮、标题 MEDIUM + 副标题 muted）；
//! - 正文单色（`file_font()` 同源）、命中底色走 [`super::results::match_highlights`]；
//! - 结果区两侧留白（`pl 8 / pr 18`）、竖向滚动条常显。
//!
//! 分组：一次命中 = 一条会话消息的片段（before/match/after）；按会话文件分组，
//! 组头 = 会话名（无名字用预览）+ 相对时间，点击组头跳到该会话首条命中。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, StyledText, WeakEntity, div, px, rgb};
use std::rc::Rc;

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::kit::icon_toggle;
use crate::ui::tokens::space;
use crate::{ContentView, PanelTab};

use super::results::{HeaderCallbacks, card_header, match_highlights};

/// 会话查询的防抖（沿用 013 弹窗版的 300ms）
const DEBOUNCE_MS: u64 = 300;

impl Chat {
    /// Ctrl+K / psp「搜索会话」钮：打开会话查询面板（单例 tab），已开则
    /// 聚焦查询框。**替换掉原来的 `Dialog::SessionSearch` 弹窗**
    /// （2026-10-21 用户定案：照文件查询的界面改造成面板，只查询）。
    pub(crate) fn open_session_search_panel(&mut self, cx: &mut Context<Self>) {
        self.ensure_session_input(cx);
        // 焦点当帧兑现（同项目搜索：`focus_soon` 在输入框自己那帧 focus）
        if let Some(q) = self.psp.session_query.clone() {
            q.update(cx, |input, cx| input.focus_soon(cx));
        }
        let existing = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::SessionSearch));
        let ix = match existing {
            Some(ix) => ix,
            None => {
                self.ws.panel_tabs.push(PanelTab::SessionSearch);
                self.ws.panel_tabs.len() - 1
            }
        };
        self.activate_panel_tab(ix, cx);
        self.set_content_view(ContentView::SessionSearch);
        cx.notify();
    }

    /// 关会话查询 tab（同项目搜索：状态保留，再开还是上次的查询）。
    pub(crate) fn close_session_search_tab(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::SessionSearch))
        else {
            return;
        };
        self.psp.search_gen += 1; // 作废在跑的批次
        self.close_panel_tab(ix, cx);
        if self.layout.content_view == ContentView::SessionSearch {
            self.fallback_after_closes(cx);
        }
        cx.notify();
    }

    /// 懒建查询框（`TextInput::new` 不需要 Window，可在动作里建）。
    pub(crate) fn ensure_session_input(&mut self, cx: &mut Context<Self>) {
        if self.psp.session_query.is_some() {
            return;
        }
        let weak = cx.weak_entity();
        let weak_esc = cx.weak_entity();
        let input = cx.new(|cx| {
            crate::ui::TextInput::new(cx)
                // 与项目搜索同一条定案：搜索框**不加 border**（flat 档）
                .flat()
                .placeholder(tr("session_search.placeholder"))
                .on_change(Box::new(move |q: &str, cx: &mut gpui::App| {
                    let _ = weak.update(cx, |c, cx| c.session_search_changed(q, cx));
                }))
                // Esc = 收起会话查询面板（defer 出帧再关：关 tab 会 drop 正在
                // 派发事件的输入框实体——与项目搜索同一条）
                .on_escape(Box::new(move |cx: &mut gpui::App| {
                    let weak = weak_esc.clone();
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |c, cx| c.close_session_search_tab(cx));
                    });
                }))
        });
        self.psp.session_query = Some(input);
    }

    /// 查询词变化：代际 +1、防抖 300ms 后后台扫本项目会话（013 原逻辑，
    /// 陈旧响应按代际丢弃）。
    pub(crate) fn session_search_changed(&mut self, q: &str, cx: &mut Context<Self>) {
        self.psp.search_gen += 1;
        let epoch = self.psp.search_gen;
        let needle = q.trim().to_string();
        self.psp.search_needle = needle.clone();
        self.psp.search_hits.clear();
        self.psp.search_truncated = false;
        // 结果集换了 → 收起状态作废（旧块对不上新结果），滚动回顶
        self.psp.session_collapsed.clear();
        self.psp
            .session_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        if needle.is_empty() {
            self.psp.search_running = false;
            cx.notify();
            return;
        }
        self.psp.search_running = true;
        // 无项目态：没有可搜的项目分组目录
        let Some(cwd) = self.ws.cwd.clone() else {
            self.psp.search_running = false;
            cx.notify();
            return;
        };
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(DEBOUNCE_MS))
                .await;
            let cancelled = weak
                .update(cx, |c, _| c.psp.search_gen != epoch)
                .unwrap_or(true);
            if cancelled {
                return;
            }
            let needle2 = needle.clone();
            let resp = cx
                .background_spawn(async move {
                    pi_link::sessions::search_sessions_for_cwd(&cwd.to_string_lossy(), &needle2)
                })
                .await;
            let _ = weak.update(cx, |c, cx| {
                if c.psp.search_gen != epoch {
                    return;
                }
                c.psp.search_running = false;
                c.psp.search_truncated = resp.truncated;
                c.psp.search_hits = resp.hits;
                cx.notify();
            });
        })
        .detach();
    }

    /// 收展一个会话块（键 = 会话文件路径）。
    pub(crate) fn session_search_toggle_block(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.psp.session_collapsed.remove(key) {
            self.psp.session_collapsed.insert(key.to_string());
        }
        cx.notify();
    }
}

/// 一行片段的高度（与项目搜索行高同源）
fn line_h() -> f32 {
    (crate::appearance::file_font().size * 1.5).round()
}

/// 视图根（`ContentView::SessionSearch` 的落点）：查询条 + 结果区。
pub(crate) fn view(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    div()
        .id("session-search")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .text_color(rgb(t.text))
        .child(bar(chat, weak, t))
        .child(results(chat, weak, t))
        .into_any_element()
}

/// 查询条：✕ + 无边框查询框 + 命中计数（Zed 同款，无替换/过滤/开关列）。
fn bar(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let hits = chat.psp.search_hits.len();
    let running = chat.psp.search_running;
    let truncated = chat.psp.search_truncated;
    let query = chat.psp.session_query.clone();
    let counter: SharedString = if truncated {
        format!("{hits}+").into()
    } else {
        format!("{hits}").into()
    };
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(space::SP2))
        .w_full()
        .child(icon_toggle(
            "ssq-close",
            "close",
            false,
            weak,
            "search.close",
            false,
            |c, cx| c.close_session_search_tab(cx),
        ));
    if let Some(q) = query {
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                // 搜索框不加 border（与项目搜索同一条定案）
                .pl(px(space::SP4))
                .pr(px(space::SP1))
                .child(div().flex_1().min_w_0().py(px(4.)).child(q)),
        );
    }
    row = row.child(
        div()
            .flex()
            .items_center()
            .gap(px(space::SP2))
            .pl(px(space::SP2))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(if hits > 0 { t.text_dim } else { t.text_faint }))
            .child(SharedString::from(counter))
            .when(running, |d| d.child(crate::ui::spinner(12., t.accent))),
    );
    div()
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
        .child(row)
        .into_any_element()
}

/// 结果区：空态（标题 + 提示）或「会话块」列表。
fn results(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    if chat.psp.search_hits.is_empty() {
        let (title, hint): (SharedString, SharedString) =
            if !chat.psp.search_needle.is_empty() && !chat.psp.search_running {
                (
                    tr("session_search.no_match").into(),
                    SharedString::from(tr("session_search.no_match_hint")),
                )
            } else if chat.psp.search_running {
                (tr("session_search.searching").into(), "".into())
            } else {
                (
                    tr("session_search.title").into(),
                    SharedString::from(tr("session_search.hint")),
                )
            };
        let mut col = div().flex().flex_col().gap(px(4.)).child(
            div()
                .text_size(px(16.))
                .text_color(rgb(t.text))
                .child(title),
        );
        if !hint.is_empty() {
            // Zed landing 的副标题档：14px muted，下留 8px
            col = col.child(
                div()
                    .text_size(px(14.))
                    .text_color(rgb(t.text_muted))
                    .mb(px(8.))
                    .child(hint),
            );
        }
        return div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(rgb(t.bg))
            .child(col)
            .into_any_element();
    }

    // 按会话分组（命中已按会话成段返回，相邻即同组）
    let groups = group_hits(&chat.psp.search_hits);
    let collapsed = &chat.psp.session_collapsed;
    let weak_open = weak.clone();
    let weak_toggle = weak.clone();
    let mut body = div().flex().flex_col().w_full();
    for (gix, group) in groups.iter().enumerate() {
        let key = group.key.clone();
        let collapsed_here = collapsed.contains(&group.key);
        let first_ts = group.hits.first().and_then(|h| h.ts);
        let path_open = group.path.clone();
        let weak_row_open = weak_open.clone();
        let open: Rc<dyn Fn(&mut gpui::App)> = Rc::new(move |cx| {
            let path = path_open.clone();
            let _ = weak_row_open.update(cx, |c, cx| c.jump_to_hit(path, first_ts, cx));
        });
        let key_toggle = key.clone();
        let weak_t = weak_toggle.clone();
        let toggle: Rc<dyn Fn(&mut gpui::App)> = Rc::new(move |cx| {
            let k = key_toggle.clone();
            let _ = weak_t.update(cx, |c, cx| c.session_search_toggle_block(&k, cx));
        });
        body = body.child(card_header(
            format!("ss-{gix}"),
            group.title.clone(),
            group.sub.clone(),
            None,
            collapsed_here,
            HeaderCallbacks { open, toggle },
            t,
        ));
        if collapsed_here {
            continue;
        }
        for hit in group.hits.iter() {
            body = body.child(snippet_row(hit, weak, t));
        }
    }

    // 两侧留白 + 右侧让出滚动条（与项目搜索同一条）
    let mut shell = div()
        .id("session-search-results")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .relative()
        .pt(px(1.))
        .pl(px(8.))
        .pr(px(18.))
        .child(
            div()
                .id("session-search-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&chat.psp.session_scroll)
                .child(body),
        );
    if chat.psp.session_scroll.max_offset().height > px(0.) {
        shell = shell.child(
            div()
                .absolute()
                .top(px(4.))
                .bottom(px(4.))
                .right(px(4.))
                .w(px(8.))
                .child(gpui_component::scroll::Scrollbar::vertical(
                    &chat.psp.session_sb_state,
                    &chat.psp.session_scroll.clone(),
                )),
        );
    }
    shell.into_any_element()
}

/// 一条命中行：单色正文 + 命中底色（无行号列——会话片段不是文件行）。
fn snippet_row(
    hit: &pi_link::sessions::SearchHit,
    weak: &WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
    let font = crate::appearance::file_font();
    let text = format!("{}{}{}", hit.before, hit.match_text, hit.after);
    let start = hit.before.len();
    let span = start..(start + hit.match_text.len()).min(text.len());
    let highlights = match_highlights(&text, &[span], t);
    let path = hit.session_path.clone();
    let ts = hit.ts;
    let weak = weak.clone();
    div()
        .id(SharedString::from(format!(
            "ssh-{}-{}",
            path.to_string_lossy(),
            hit.ts.unwrap_or_default()
        )))
        .min_h(px(line_h()))
        .w_full()
        .flex()
        .items_start()
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let path = path.clone();
            let _ = weak.update(cx, |c, cx| c.jump_to_hit(path, ts, cx));
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(px(8.))
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .line_height(gpui::relative(1.5))
                .text_color(rgb(t.text_muted))
                .child(StyledText::new(SharedString::from(text)).with_highlights(highlights)),
        )
        .into_any_element()
}

/// 一个会话块：组头 + 该会话的命中行。
struct SessionGroup<'a> {
    key: String,
    path: std::path::PathBuf,
    title: SharedString,
    sub: SharedString,
    hits: Vec<&'a pi_link::sessions::SearchHit>,
}

fn group_hits(hits: &[pi_link::sessions::SearchHit]) -> Vec<SessionGroup<'_>> {
    let mut out: Vec<SessionGroup<'_>> = Vec::new();
    for hit in hits {
        let key = hit.session_path.to_string_lossy().to_string();
        if out.last().map(|g| g.key.as_str()) != Some(key.as_str()) {
            let title: SharedString = hit
                .session_name
                .clone()
                .or_else(|| (!hit.preview.is_empty()).then(|| hit.preview.clone()))
                .unwrap_or_else(|| {
                    hit.session_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| key.clone())
                })
                .into();
            let sub: SharedString = crate::services::format::time_ago(hit.modified).into();
            out.push(SessionGroup {
                key,
                path: hit.session_path.clone(),
                title,
                sub,
                hits: Vec::new(),
            });
        }
        if let Some(g) = out.last_mut() {
            g.hits.push(hit);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_link::sessions::SearchHit;
    use std::time::SystemTime;

    fn hit(path: &str, name: Option<&str>, ts: i64) -> SearchHit {
        SearchHit {
            session_path: std::path::PathBuf::from(path),
            session_name: name.map(str::to_string),
            preview: String::new(),
            modified: SystemTime::UNIX_EPOCH,
            ts: Some(ts),
            role: "user".into(),
            before: "aaa ".into(),
            match_text: "needle".into(),
            after: " bbb".into(),
        }
    }

    /// 相邻同会话的命中归一块，换会话才开新块（组头 = 会话名 + 时间）。
    #[test]
    fn groups_adjacent_hits_by_session() {
        let hits = vec![
            hit("a.jsonl", Some("会话A"), 1),
            hit("a.jsonl", None, 2),
            hit("b.jsonl", None, 3),
        ];
        let groups = group_hits(&hits);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].hits.len(), 2, "同会话两条命中进同一块");
        assert_eq!(groups[0].title.as_ref(), "会话A");
        assert_eq!(groups[1].hits.len(), 1);
        // 无名字无预览 → 回落到文件名（stem）
        assert_eq!(groups[1].title.as_ref(), "b");
    }
}
