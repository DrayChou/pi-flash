//! 子代理面板（044-1 §1）——当前会话子代理 catalog 的聚合视图。
//!
//! 入口 = 会话操作栏按钮（`session_fn_bar`，运行中带计数徽标）；点开为
//! 锚在操作栏正下方的浮层卡片（2026-10-19 三改：二改的停靠版会下推消息区，
//! 用户不要；浮层但**不挂全屏外点拦截**——那会把 composer 模型下拉等点击
//! 一起吃掉，是第三批修掉的雷）。✕/再点按钮关。事实源 = 父会话 JSONL 的
//! `subagent/descriptor`（[`pi_link::subagent_catalog`]），整行点击 = 以正式
//! tab 打开子会话并收面板。
//!
//! 轮询常驻（2026-10-19 用户拍板：关面板只是隐藏，徽标必须一直准）——打开
//! 期 1s 增量读、收起期 5s 慢速轮询；会话激活即建缓存（open=false），徽标
//! 不依赖「先开过一次面板」。
//!
//! 状态色与 messages/cards.rs 的 subagent 卡片同源（运行黄 / 完成绿 /
//! 失败红），面板不另起一套。

use std::path::PathBuf;

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use pi_link::subagent_catalog::{
    CatalogRow, StatusKind, merge_rows, read_catalog, row_duration_secs,
};

/// 打开期轮询间隔（044-1 §1.4：1s 增量读）。
const POLL_TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// 收起期轮询间隔（关面板只是隐藏；5s 对徽标够新鲜）。
const CLOSED_POLL_TICK: std::time::Duration = std::time::Duration::from_secs(5);

/// 状态色（照抄 cards.rs 的 subagent 卡片；中止橙按 044-1 §1.3 交互稿）。
const C_RUNNING: u32 = 0xeab308;
const C_DONE: u32 = 0x22c55e;
const C_ERROR: u32 = 0xf87171;
const C_ABORTED: u32 = 0xf97316;

/// 面板宿主状态：持 catalog 缓存（§1.4 的 offset+rows）；`open` 只控渲染。
///
/// `open` 与 Option 分开：徽标随时读缓存，面板收起不清数据。`polling`
/// 守卫常驻轮询任务只拉起一份——轮询从会话激活跑到 Chat 消亡，收起不清
/// 缓存也不停轮（间隔放慢），换会话在 poll 内自愈归零重扫。
#[derive(Clone)]
pub(crate) struct SubagentPanel {
    /// 父会话文件（descriptor 的落盘处）；没落盘的会话无 catalog。
    pub file: Option<PathBuf>,
    /// 下一轮增量续读的字节偏移。
    pub offset: u64,
    /// 累积的 descriptor 投影（同 childId 末条覆盖）。
    pub rows: Vec<CatalogRow>,
    /// 面板是否展开（只控渲染；收起轮询放慢到 [`CLOSED_POLL_TICK`]）。
    pub open: bool,
    /// 常驻轮询任务是否在跑（防 toggle/切换重复 spawn）。
    pub polling: bool,
}

impl SubagentPanel {
    /// 运行中计数（phase ∈ provisioning/active）——操作栏徽标数它。
    fn running_count(&self) -> usize {
        self.rows.iter().filter(|r| r.in_flight()).count()
    }
}

impl Chat {
    /// 切换子代理面板：首次打开（或换会话后）拉一次全量，之后只翻 open。
    /// 关面板不丢缓存不停轮（徽标还要用）。
    pub(crate) fn toggle_subagent_panel(&mut self, cx: &mut gpui::Context<Self>) {
        let file = self.rt().read(cx).file.clone();
        let already = self
            .dlg
            .subagent_panel
            .as_ref()
            .is_some_and(|p| p.file == file);
        match self.dlg.subagent_panel.as_mut() {
            // 同会话已有缓存：只翻开关
            Some(p) if already => p.open = !p.open,
            // 首次 / 换了会话：重拉全量；polling 继承（旧任务还活着就别再 spawn）
            _ => {
                let (rows, offset) = match file.as_deref() {
                    Some(f) => {
                        let scan = read_catalog(f, 0);
                        (scan.rows, scan.next_offset)
                    }
                    None => (Vec::new(), 0),
                };
                let polling = self.dlg.subagent_panel.as_ref().is_some_and(|p| p.polling);
                self.dlg.subagent_panel = Some(SubagentPanel {
                    file,
                    offset,
                    rows,
                    open: true,
                    polling,
                });
            }
        }
        ensure_poll(self, cx);
        cx.notify();
    }

    /// 会话激活时常备面板缓存（open=false，只出数据不出浮层）——徽标从切换
    /// 起就准，不依赖「先开过一次面板」。switch_pool_rt 调；空结构起步，首轮
    /// poll 全量扫，不拖慢切换热路径。
    pub(crate) fn ensure_subagent_panel(&mut self, cx: &mut gpui::Context<Self>) {
        let file = self.rt().read(cx).file.clone();
        let already = self
            .dlg
            .subagent_panel
            .as_ref()
            .is_some_and(|p| p.file == file);
        if !already {
            let polling = self.dlg.subagent_panel.as_ref().is_some_and(|p| p.polling);
            self.dlg.subagent_panel = Some(SubagentPanel {
                file,
                offset: 0,
                rows: Vec::new(),
                open: false,
                polling,
            });
        }
        ensure_poll(self, cx);
    }

    /// 一轮增量轮询：只读上次 offset 之后的新增字节，合并后重绘。
    /// 返回下一轮间隔（开 1s / 关 5s）；None = 面板没了（Chat 消亡）→ 任务退场。
    /// 会话切换在这里自愈：缓存挂的还是旧会话 → 归零重扫新会话（open 保持）。
    fn subagent_panel_poll(&mut self, cx: &mut gpui::Context<Self>) -> Option<std::time::Duration> {
        let current = self.rt().read(cx).file.clone();
        let panel = self.dlg.subagent_panel.as_mut()?;
        if panel.file != current {
            panel.file = current;
            panel.offset = 0;
            panel.rows.clear();
        }
        let tick = if panel.open {
            POLL_TICK
        } else {
            CLOSED_POLL_TICK
        };
        let Some(file) = panel.file.clone() else {
            // 会话还没落盘：没东西可读，慢速等文件出现
            return Some(tick);
        };
        let scan = read_catalog(&file, panel.offset);
        if scan.next_offset != panel.offset {
            panel.offset = scan.next_offset;
            merge_rows(&mut panel.rows, scan.rows);
            cx.notify();
        }
        Some(tick)
    }

    /// 整行点击：子会话是普通 pi 会话，直接 open_session（只读检视 + 可续跑），
    /// 并收起面板（缓存留着，徽标仍显示）。
    pub(crate) fn open_subagent_child(&mut self, path: PathBuf, cx: &mut gpui::Context<Self>) {
        if let Some(p) = self.dlg.subagent_panel.as_mut() {
            p.open = false;
        }
        self.open_session(path, false, cx);
        cx.notify();
    }
}

/// 拉起常驻轮询（幂等：`polling` 已在跑则不动）。
fn ensure_poll(chat: &mut Chat, cx: &mut gpui::Context<Chat>) {
    if let Some(p) = chat.dlg.subagent_panel.as_mut() {
        if p.polling {
            return;
        }
        p.polling = true;
    }
    spawn_poll(cx);
}

/// 常驻轮询任务：间隔由上轮 poll 的返回值决定（开 1s / 关 5s，同 psp_drag
/// 看门狗范式）；poll 返回 None（Chat 消亡）才退场。
fn spawn_poll(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let mut tick = POLL_TICK;
        loop {
            cx.background_executor().timer(tick).await;
            let next = this
                .update(cx, |chat, cx| chat.subagent_panel_poll(cx))
                .unwrap_or(None);
            let Some(next_tick) = next else { break };
            tick = next_tick;
        }
    })
    .detach();
}

/// 操作栏按钮的运行中计数徽标（044-1 §1.3：N = phase ∈ provisioning/active）。
/// 无运行中不渲染徽标。
pub(crate) fn running_badge(chat: &Chat) -> Option<gpui::AnyElement> {
    let t = T();
    let n = chat
        .dlg
        .subagent_panel
        .as_ref()
        .map(SubagentPanel::running_count)?;
    if n == 0 {
        return None;
    }
    Some(
        div()
            .min_w(px(16.))
            .h(px(16.))
            .px(px(4.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(t.accent))
            .text_size(crate::appearance::ui_size(10.))
            .text_color(rgb(t.accent_contrast))
            .child(SharedString::from(n.to_string()))
            .into_any_element(),
    )
}

/// 面板本体：浮层（044-1 §1.1 三改——二改的停靠版会把消息区下推，用户
/// 不要）。`absolute` 挂 chat-wrap（relative）顶左 = 操作栏正下方悬浮，
/// 不推挤消息区；**不挂全屏外点拦截**（那会把 composer 模型下拉等点击一起
/// 吃掉，见 buglist 第三批），关 = ✕ 或再点操作栏按钮。
/// `open=false` → None（不占渲染）。
pub(crate) fn subagent_panel_element(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    content_h: f32,
) -> Option<gpui::AnyElement> {
    let t = T();
    let panel = chat.dlg.subagent_panel.as_ref()?;
    // open 门（不可省）：缓存常驻（徽标轮询用），没这道门面板会永远渲染
    if !panel.open {
        return None;
    }

    // 宽 = 设置列表标准宽（LIST_W=400，082 定稿，与二改前浮层同源）；
    // 最大高 = 内容区上半，矮窗口保底 160px
    let card_max_h = (content_h / 2.).max(160.);

    let mut card = div()
        .id("subagent-panel-card")
        .absolute()
        .top_0()
        .left_0()
        .w(px(crate::settings::LIST_W))
        .max_h(px(card_max_h))
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .shadow_lg()
        .overflow_hidden();

    // 头部：标题 + 关闭 ✕（规则 5：有可见内容的卡片挂 ×）
    card = card.child(panel_header(weak));

    // 行列表（滚动）
    let mut list = div()
        .id("subagent-panel-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(6.))
        .py(px(6.));
    if panel.rows.is_empty() {
        list = list.child(
            div()
                .p(px(16.))
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_dim))
                .child(tr("subagent.empty")),
        );
    } else {
        for row in &panel.rows {
            list = list.child(panel_row(weak, row));
        }
    }
    card = card.child(list);

    Some(card.into_any_element())
}

/// 头部行：标题 + ✕ 关闭。
fn panel_header(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let weak_close = weak.clone();
    div()
        .id("subagent-panel-head")
        .flex()
        .items_center()
        .px(px(12.))
        .py(px(8.))
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(tr("subagent.panel")),
        )
        .child(
            div()
                .id("subagent-panel-x")
                .size(px(20.))
                .rounded(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_close.update(cx, |c, cx| {
                        // 只收浮层，不丢缓存（徽标还要用）
                        if let Some(p) = c.dlg.subagent_panel.as_mut() {
                            p.open = false;
                        }
                        cx.notify();
                    });
                })
                .child("✕"),
        )
        .into_any_element()
}

/// 一行子代理：状态点 + label（退 childId）+ `provider·d{depth}` + 耗时，
/// 整行点击 = 打开子会话。
fn panel_row(weak: &gpui::WeakEntity<Chat>, row: &CatalogRow) -> gpui::AnyElement {
    let t = T();
    let (kind_c, kind_text) = status_of(row);
    let meta = format!("{}·d{}", row.provider, row.depth);
    let time = row_duration_secs(row)
        .map(|s| format!("{s}s"))
        .unwrap_or_else(|| "—".to_string());
    let open = row.session_file.clone().map(PathBuf::from);

    let mut el = div()
        .id(SharedString::from(format!("subagent-row-{}", row.child_id)))
        .h(px(30.))
        .px(px(8.))
        .mb(px(1.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t.text_muted))
        .hover(|s| s.bg(rgb(t.bg_hover)));
    if open.is_some() {
        let weak_row = weak.clone();
        el = el
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_row.update(cx, |c, cx| {
                    if let Some(p) = open.clone() {
                        c.open_subagent_child(p, cx);
                    }
                });
            });
    }
    el.child(crate::settings::widgets::status_dot(kind_c))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(row.display_label().to_string())),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(meta)),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(time)),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(kind_c))
                .child(SharedString::from(kind_text)),
        )
        .into_any_element()
}

/// 行状态（色 + 文案）：终态优先分码，缺 stop_reason 时按 phase 兜底。
fn status_of(row: &CatalogRow) -> (u32, &'static str) {
    match row.status_kind() {
        StatusKind::Running => (C_RUNNING, tr("subagent.running")),
        StatusKind::Done => (C_DONE, tr("subagent.done")),
        StatusKind::Aborted => (C_ABORTED, tr("subagent.aborted")),
        StatusKind::MaxTurns => (C_ABORTED, tr("subagent.max_turns")),
        StatusKind::Error => (C_ERROR, tr("subagent.failed")),
    }
}
