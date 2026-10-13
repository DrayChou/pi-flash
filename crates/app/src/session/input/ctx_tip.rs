//! composer 上下文用量：环悬停状态机、详情卡与用量面板（v58 响应式）。



use gpui::{AnimationExt, Context, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::i18n::tr;
use crate::services::format::fmt_thousand;

/// 悬停标志更新 + 淡出状态机（乱序免疫）：只按 (was_open, open) 转移，
/// 环→面板交接时两个 hover 事件无论先后都收敛到正确状态。双面全空才
/// 开始淡出（200ms 定时器到期清除，守卫时刻戳防旧定时器误杀新一轮）。
pub(super) fn ctx_tip_hover(
    chat: &mut Chat,
    ring: bool,
    entered: bool,
    cx: &mut Context<Chat>,
) {
    let was_open = chat.dlg.ctx_tip_ring_hover || chat.dlg.ctx_tip_panel_hover;
    if ring {
        chat.dlg.ctx_tip_ring_hover = entered;
    } else {
        chat.dlg.ctx_tip_panel_hover = entered;
    }
    let open = chat.dlg.ctx_tip_ring_hover || chat.dlg.ctx_tip_panel_hover;
    match (was_open, open) {
        (false, true) => {
            chat.dlg.ctx_tip_closing = None;
            cx.notify();
        }
        (true, false) => {
            let t = std::time::Instant::now();
            chat.dlg.ctx_tip_closing = Some(t);
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let _ = this.update(cx, |c, cx| {
                    if c.dlg.ctx_tip_closing == Some(t) {
                        c.dlg.ctx_tip_closing = None;
                        cx.notify();
                    }
                });
            })
            .detach();
            cx.notify();
        }
        _ => {}
    }
}

/// 悬浮详情浮层（挂在 ctx-ring 容器内，absolute 越界浮在聊天区上）：
/// Open 常显；Closing 包一层 160ms 透明度淡出，定时器随后卸载。wrapper
/// 下方 12px 隐形尾迹压住环顶 2px，环→面板悬停交接无死区。
pub(super) fn ctx_tip_element(
    chat: &mut Chat,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let visible =
        chat.dlg.ctx_tip_ring_hover || chat.dlg.ctx_tip_panel_hover || chat.dlg.ctx_tip_closing.is_some();
    if !visible {
        return None;
    }
    let (stats, compacting, can_compact) = {
        let r = chat.rt().read(cx);
        (r.stats.clone(), r.compacting, r.can_compact())
    };
    let closing = chat.dlg.ctx_tip_closing.is_some();
    let wrap = div()
        .id("ctx-tip")
        .absolute()
        .bottom(px(24.)) // 环命中区 26px，压顶 2px 防接缝死区
        // 相对圆环居中：向左/右对称扩张 + flex 居中。手算 left 偏移要跟着
        // 环的 padding(4)+负 margin(-4) 一起走，对称扩张则天然以环为轴，
        // 环宽变了也不用改常数。±200 > 320/2 保证面板不被压缩。
        .left(px(-200.))
        .right(px(-200.))
        .pb(px(12.))
        .flex()
        .flex_row()
        .justify_center()
        // hover 挂整层：面板与环之间的空档也属于浮层，鼠标横穿不闪断
        .on_hover(cx.listener(|this, h: &bool, _w, cx| ctx_tip_hover(this, false, *h, cx)))
        // mousedown 拦截只挂面板本体（点面板不穿透到消息列表）；空档区域
        // 无 mousedown handler，点击照常落到下层消息
        .child(
            div()
                .w(px(320.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(ctx_usage_panel(stats, compacting, can_compact, t, cx)),
        );
    let el = if closing {
        wrap.with_animation(
            "ctx-tip-out",
            gpui::Animation::new(std::time::Duration::from_millis(160)),
            |el, delta| el.opacity(1. - delta),
        )
        .into_any_element()
    } else {
        wrap.into_any_element()
    };
    // vendored gpui 的 Style::paint 把边框画在子元素之后——面板挂在胶囊
    // 内部会被胶囊聚焦边框压线（实测横穿面板）；deferred 推迟到整帧末尾
    // 绘制（布局仍在原位，锚定/hover 关系不变，弹层同机制）
    Some(gpui::deferred(el).into_any_element())
}

/// 用量详情面板本体（pi-web session-info-popover 简化版——上下文比例 /
/// token 分项 / 费用 / 缓存命中率）。
fn ctx_usage_panel(
    stats: Option<pi_link::protocol::SessionStats>,
    compacting: bool,
    can_compact: bool,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    // 上下文用量弹窗：全部文字 = 面板设置值，title 与正文同号
    // （字体大小设置.md §1）
    let row = |label: &str, value: String| -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(label.to_string())),
            )
            .child(
                div()
                    .ml_auto()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(value)),
            )
            .into_any_element()
    };
    let section = |title: &str, rows: Vec<gpui::AnyElement>| -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(title.to_string())),
            )
            .children(rows)
            .into_any_element()
    };

    let body: Vec<gpui::AnyElement> = match stats {
        None => vec![
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(tr("stats.no_usage"))
                .into_any_element(),
        ],
        Some(s) => {
            let fmt_k = |n: u64| -> String { format!("{}K", (n as f64 / 1000.).round() as u64) };
            let ctx_rows = vec![
                row(
                    tr("stats.share"),
                    s.context_percent
                        .map(|p| format!("{p:.1}%"))
                        .unwrap_or_else(|| "\u{2014}".into()),
                ),
                row(
                    tr("stats.context_tokens"),
                    match (s.context_tokens, s.context_window) {
                        (Some(t), Some(w)) => format!("{} / {}", fmt_k(t), fmt_k(w)),
                        (Some(t), None) => fmt_k(t),
                        _ => "\u{2014}".into(),
                    },
                ),
            ];
            let token_rows = vec![
                row(tr("stats.input"), fmt_thousand(s.input)),
                row(tr("stats.output"), fmt_thousand(s.output)),
                row(tr("stats.cache_read"), fmt_thousand(s.cache_read)),
                row(tr("stats.cache_write"), fmt_thousand(s.cache_write)),
                row(tr("tokens.total"), fmt_thousand(s.tokens_total)),
            ];
            let mut cost_rows = vec![row(tr("stats.total_cost"), format!("${:.4}", s.cost))];
            if let Some(r) = s.cache_hit_rate() {
                cost_rows.push(row(tr("stats.cache_hit_rate"), format!("{:.1}%", r * 100.)));
            }
            vec![
                section(tr("stats.context_label"), ctx_rows),
                section(tr("stats.tokens_total"), token_rows),
                section(tr("stats.cost"), cost_rows),
            ]
        }
    };

    // 「压 缩」字间插窄空格（gpui 无 letter_spacing，字符串层处理；
    // 「压缩中…」多字，保持原样不插）
    let compact_label = {
        let raw = if compacting { tr("status.compacting") } else { tr("action.compact") };
        let mut cs = raw.chars();
        match (cs.next(), cs.next(), cs.next()) {
            (Some(a), Some(b), None) => format!("{a}\u{2009}{b}"),
            _ => raw.to_string(),
        }
    };

    div()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .shadow_lg()
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .children(body)
        .child(
            // 底部手动压缩按钮：无门槛（点不点由用户决定），描边按钮
            // 无常态底色（hover 才出 bg_hover）；固定高 34 + items_center
            // 保证文字在框内垂直居中。compacting 仅防连点。
            div()
                .id("ctx-compact")
                .h(px(34.))
                .w_full()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border))
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.))
                .when(can_compact, |s| s.cursor_pointer())
                .text_color(rgb(if compacting {
                    t.text_muted
                } else if can_compact {
                    t.accent
                } else {
                    t.text_faint
                }))
                .hover(move |s| if can_compact { s.bg(rgb(t.bg_hover)) } else { s })
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                        // 双重守卫：按钮禁用态之外再查一次 runtime（agent
                        // 跑动中 compact 会中止当前 turn）
                        if this.rt().read(cx).can_compact() {
                            this.rt().update(cx, |r, cx| r.compact(cx));
                        }
                    },
                ))
                .child(SharedString::from(compact_label)),
        )
}
