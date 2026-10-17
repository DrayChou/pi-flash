//! 设置面板公共件 —— pi-web `SettingsUi.tsx` 的 GPUI 复刻（Config* 系列）。
//! 尺寸走 `crate::ui::tokens`（UI 组件规范）；颜色走主题语义色
//! （danger 系见 theme.rs；靛蓝徽标 hsla）。

use gpui::{AnyElement, SharedString, FontWeight, MouseButton, Window, div, px, rgb};

use super::*;

/// 靛蓝小徽标（reasoning "T"、项目 scope）——pi-web rgba(99,102,241,…)。
pub(crate) fn indigo_bg() -> gpui::Hsla {
    gpui::hsla(0.63, 0.86, 0.62, 0.12)
}
pub(crate) fn indigo_fg() -> gpui::Hsla {
    gpui::hsla(0.63, 0.86, 0.62, 0.85)
}

// ---------------------------------------------------------------------------
// buttons
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Btn {
    Primary,
    Secondary,
    Danger,
}

/// ConfigButton：variant × size，点击回调直接拿到 Chat（动作都在 Chat 上）。
/// 尺寸/配色委托 `ui::button`（§5.1 唯一按钮表）：small→MD(28)，default→LG(32)。
pub(crate) fn config_button(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    label: &str,
    variant: Btn,
    small: bool,
    disabled: bool,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let size = if small {
        crate::ui::BtnSize::Md
    } else {
        crate::ui::BtnSize::Lg
    };
    let variant = match variant {
        Btn::Primary => crate::ui::BtnVariant::Primary,
        Btn::Secondary => crate::ui::BtnVariant::Secondary,
        Btn::Danger => crate::ui::BtnVariant::Danger,
    };
    let weak = weak.clone();
    crate::ui::button(
        id,
        label.to_string(),
        size,
        variant,
        disabled,
        move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        },
    )
}

// ---------------------------------------------------------------------------
// switch / dot / tag
// ---------------------------------------------------------------------------

/// 开关三态：关 / 部分选中 / 开。
///
/// 「部分选中」是组头（技能分组、扩展全局）与模型页整商开关的必需态：一组里
/// 只启用了一部分时，开关不能装成全关（会被误读成「一项都没开」），也不该
/// 和全选一模一样——轨道用减淡的 accent，knob 停在开位。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwitchMark {
    Off,
    Mixed,
    On,
}

impl SwitchMark {
    /// 从计数算态：0 = 关，全中 = 开，其余 = 部分。
    pub(crate) fn from_counts(on: usize, total: usize) -> Self {
        if total == 0 || on == 0 {
            Self::Off
        } else if on >= total {
            Self::On
        } else {
            Self::Mixed
        }
    }

    pub(crate) fn is_on(self) -> bool {
        matches!(self, Self::On | Self::Mixed)
    }
}

/// 开关视觉（SW_MD 28×16·knob10，`ui::tokens::switch`）：无 id/无事件，
/// [`switch_base`]（独立开关）与 [`group_switch`]（组头行内）共用。
fn switch_el_mark(mark: SwitchMark) -> gpui::Div {
    use crate::ui::tokens::switch as sw;
    let t = T();
    let lit = mark.is_on();
    // 部分选中：轨道填色减淡（accent 半透明），knob 照旧停在开位
    let track: gpui::Hsla = match mark {
        SwitchMark::Off => rgb(t.bg_selected).into(),
        SwitchMark::On => rgb(t.accent).into(),
        SwitchMark::Mixed => {
            let a: gpui::Hsla = rgb(t.accent).into();
            gpui::hsla(a.h, a.s, a.l, sw::MD_MIXED_ALPHA)
        }
    };
    let border: gpui::Hsla = match mark {
        SwitchMark::Off => rgb(t.border).into(),
        SwitchMark::On => rgb(t.accent).into(),
        SwitchMark::Mixed => {
            let a: gpui::Hsla = rgb(t.accent).into();
            gpui::hsla(a.h, a.s, a.l, sw::MD_MIXED_ALPHA + 0.25)
        }
    };
    div()
        .w(px(sw::MD_W))
        .h(px(sw::MD_H))
        .flex_shrink_0()
        .rounded(px(sw::MD_H / 2.))
        .border_1()
        .border_color(border)
        .bg(track)
        .flex()
        .items_center()
        .child(
            div()
                .ml(px(if lit { sw::MD_ON_ML } else { sw::MD_OFF_ML }))
                .size(px(sw::MD_KNOB))
                .rounded_full()
                .bg(if lit { rgb(t.bg) } else { rgb(t.text_muted) }),
        )
}

fn switch_el(on: bool) -> gpui::Div {
    switch_el_mark(if on { SwitchMark::On } else { SwitchMark::Off })
}

/// 开关视觉唯一入口（独立开关）。`apply` 收到新状态 bool 由调用方自行
/// 解释；Chat 场景走 [`config_switch`]。
pub(crate) fn switch_base(
    id: impl Into<SharedString>,
    on: bool,
    disabled: bool,
    apply: impl Fn(bool, &mut gpui::App) + 'static,
) -> AnyElement {
    let el = switch_el(on).id(id.into());
    if disabled {
        return el.opacity(0.5).into_any_element();
    }
    el.cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            apply(!on, cx);
        })
        .into_any_element()
}

/// ConfigSwitch：设置页行内开关（Chat 回调形态）。
pub(crate) fn config_switch(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    on: bool,
    disabled: bool,
    on_toggle: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let weak = weak.clone();
    switch_base(id, on, disabled, move |_, cx| {
        let _ = weak.update(cx, |c, cx| on_toggle(c, cx));
    })
}

/// 组头小开关行（ConfigSidebarGroupSwitch）：{enabled}/{total} + 开关。
/// 开关尺寸随 2026-10-09 定稿与全局统一为 SW_MD 28×16（原 24×14 小档废除）；
/// 三态（[`SwitchMark`]）——部分启用必须看得出来，不能装成全关。
pub(crate) fn group_switch(
    id: impl Into<SharedString>,
    weak: &gpui::WeakEntity<Chat>,
    count_text: String,
    mark: SwitchMark,
    disabled: bool,
    on_toggle: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    let t = T();
    let row = div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(10.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(count_text)),
        )
        .child({
            let mut sw = switch_el_mark(mark);
            if !disabled {
                sw = sw.cursor_pointer();
            } else {
                sw = sw.opacity(0.5);
            }
            sw
        });
    if disabled {
        return row.into_any_element();
    }
    let weak = weak.clone();
    row.on_mouse_down(MouseButton::Left, move |_, _, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| on_toggle(c, cx));
    })
    .into_any_element()
}

/// 7px 状态点。
pub(crate) fn status_dot(color: u32) -> AnyElement {
    div()
        .size(px(7.))
        .rounded_full()
        .flex_shrink_0()
        .bg(rgb(color))
        .into_any_element()
}

/// scope 徽标：项目 = 靛蓝，其余 = 灰。
pub(crate) fn scope_tag(label: &str, project: bool) -> AnyElement {
    let t = T();
    let dim: gpui::Hsla = rgb(t.text_dim).into();
    div()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(3.))
        .bg(if project { indigo_bg() } else { gpui::hsla(0., 0., 0.5, 0.12) })
        .text_size(crate::appearance::ui_size(10.))
        .text_color(if project { indigo_fg() } else { dim })
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// text blocks
// ---------------------------------------------------------------------------

/// ConfigSectionTitle：11px 600 大写感小标题。
pub(crate) fn section_title(text: &str) -> AnyElement {
    let t = T();
    div()
        .text_size(crate::appearance::ui_size(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// ConfigField：label 上、控件下。
pub(crate) fn field(label: &str, control: impl gpui::IntoElement) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(section_title(label))
        .child(control)
        .into_any_element()
}

/// 说明行（note / describedby 的可见文本）。
pub(crate) fn note(text: &str) -> AnyElement {
    let t = T();
    div()
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

pub(crate) fn error_note(text: &str) -> AnyElement {
    div()
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(T().danger))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// mono 值块（路径 / 命令等）。
pub(crate) fn mono_text(text: String, dim: bool) -> AnyElement {
    let t = T();
    div()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(11.))
        .text_color(if dim { rgb(t.text_dim) } else { rgb(t.text) })
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(SharedString::from(text))
        .into_any_element()
}

/// 复选小块（工具/资源勾选）：14px 方框 + 标签，点击即切换。
// ---------------------------------------------------------------------------
// sidebar pieces
// ---------------------------------------------------------------------------

/// 设置页左列表列宽（Providers / 扩展 / MCP 清单 / 技能）：四页统一 350，
/// 基准 = 扩展页（2026-10-09 用户定稿「全部和扩展的列表宽度对齐」）。
pub(crate) const LIST_W: f32 = 350.;

/// ConfigSidebar 基座（240px 列，bg_panel + 右边框）。
pub(crate) fn sidebar_shell(id: &'static str) -> gpui::Stateful<gpui::Div> {
    let t = T();
    // 不刷底色：列表区与详情区同底（2026-10-10 用户定稿），只留右分隔线
    div()
        .id(id)
        .w(px(240.))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(rgb(t.border))
}

/// ConfigSidebarList：滚动区 + 底部 list-action 槽位由调用方拼。
pub(crate) fn sidebar_list() -> gpui::Stateful<gpui::Div> {
    div()
        .id("cfg-sb-list")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(6.))
        .pt(px(8.))
}

/// ConfigSidebarItem：30px 行（激活 bg_selected + semibold）。返回基座，
/// 调用方 `.child(...)` 拼内容并用 [`sidebar_item_on`] 挂点击。
pub(crate) fn sidebar_item(
    id: impl Into<SharedString>,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    div()
        .id(id.into())
        .h(px(30.))
        .px(px(8.))
        .mb(px(1.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(crate::appearance::ui_size(12.))
        .cursor_pointer()
        .bg(rgb(if active { t.bg_selected } else { t.bg_panel }))
        .font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
        .text_color(rgb(if active { t.text } else { t.text_muted }))
        .hover(|s| s.bg(rgb(t.bg_hover)))
}

/// 侧栏行点击（section 切换 + 清错误）。
pub(crate) fn select_section(
    weak: &gpui::WeakEntity<Chat>,
    section: String,
) -> impl Fn(&mut Window, &mut gpui::App) + 'static {
    let weak = weak.clone();
    move |_, cx| {
        let _ = weak.update(cx, |c, cx| {
            if let Some(st) = c.dlg.settings.clone() {
                st.update(cx, |s, cx| {
                    s.section = section.clone();
                    s.error = None;
                    cx.notify();
                });
            }
        });
    }
}

/// ConfigSidebarGroupLabel：10px 大写组标题，右侧 aside（计数+开关）。
pub(crate) fn group_header(label: &str, aside: Option<AnyElement>) -> AnyElement {
    let t = T();
    div()
        .flex()
        .items_center()
        .px(px(8.))
        .pt(px(8.))
        .pb(px(3.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(crate::appearance::ui_size(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .children(aside)
        .into_any_element()
}

// ---------------------------------------------------------------------------
// detail + footer
// ---------------------------------------------------------------------------

/// ConfigDetail 基座：flex-1 滚动 + 20px 内边距。
pub(crate) fn detail_shell(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .p(px(20.))
        .text_size(crate::appearance::ui_size(12.))
        .flex()
        .flex_col()
        .gap_4()
}

// ---------------------------------------------------------------------------
// rows
// ---------------------------------------------------------------------------

/// set-row：标题+描述在左，控件在右，底分隔线（界面/其他两页单列 rows 版式）。
/// set-row：标题+描述在左，控件在右，底分隔线（界面/其他两页单列 rows 版式）；
/// desc 传空则只渲染标题。
pub(crate) fn set_row(
    title: &str,
    desc: &str,
    control: gpui::AnyElement,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let mut left = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            // 标题与「界面」页 section_label 同款（13 semibold text_muted）
            div()
                .text_size(crate::appearance::ui_size(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(title.to_string())),
        );
    if !desc.is_empty() {
        left = left.child(
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(desc.to_string())),
        );
    }
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .py(px(13.))
        .border_b_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x40)))
        .child(left)
        .child(control)
        .into_any_element()
}

// ---------------------------------------------------------------------------
// range slider（会话内容宽度）
// ---------------------------------------------------------------------------

/// 轨道可用宽（旋钮中心在两端内缩半个旋钮，实际拖动行程 = 本值 - 旋钮径）
const SLIDER_W: f32 = 180.;
/// 元素高 = 命中带高（轨道只有 4px，鼠标却要容易按中）
const SLIDER_H: f32 = 18.;
const SLIDER_TRACK_H: f32 = 4.;
const SLIDER_KNOB: f32 = 12.;

/// 会话内容宽度滑块（pi-web `chatContentWidth` 的 range 控件 parity）。
///
/// 交互：点轨道直接跳值、按住拖动连续改；拖动中只走内存预览
/// （[`crate::services::workspace::preview_chat_content_width`]，每帧一个
/// MouseMove，逐帧写 JSON 没必要），松手才落盘。
///
/// 实现走 `canvas` + 窗口级 `on_mouse_event`（与 `ui/psp_scrollbar.rs` 同
/// 一套机制）：元素级 `on_mouse_move` 只覆盖指针在元素内的移动，拖出 180px
/// 轨道就断，必须用窗口级监听才能在整窗范围跟手。`window.on_mouse_event`
/// 只允许 paint 期注册，故三件套挂在 canvas 的 paint 闭包里、每帧重注册。
pub(crate) fn width_slider(
    id: &'static str,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    weak: &gpui::WeakEntity<Chat>,
) -> AnyElement {
    let t = T();
    let geom = SliderGeom {
        value,
        min,
        max,
        step,
        track: rgb(t.border).into(),
        fill: rgb(t.accent).into(),
        knob: rgb(t.bg_panel).into(),
    };
    let weak = weak.clone();
    div()
        .id(id)
        .relative()
        .w(px(SLIDER_W))
        .h(px(SLIDER_H))
        .flex_shrink_0()
        .cursor_pointer()
        .child(
            gpui::canvas(
                |_, _, _| {},
                move |bounds, _, window, _cx| {
                    geom.paint(bounds, &weak, window);
                },
            )
            .absolute()
            .inset_0(),
        )
        .into_any_element()
}

/// 滑块一帧的绘制参数 + 交互（值映射与事件处理按同一套几何算）。
struct SliderGeom {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    track: gpui::Hsla,
    fill: gpui::Hsla,
    knob: gpui::Hsla,
}

/// 鼠标 x → 值：旋钮中心行程 [bounds.left+knob/2, bounds.right-knob/2] 映射
/// 到 [min, max]，量化到 step 后钳住。
fn value_at(
    bounds: gpui::Bounds<gpui::Pixels>,
    min: f32,
    max: f32,
    step: f32,
    x: gpui::Pixels,
) -> f32 {
    let knob = px(SLIDER_KNOB);
    let x0 = bounds.left() + knob / 2.;
    let x1 = bounds.right() - knob / 2.;
    let frac = f32::from(x - x0) / f32::from(x1 - x0);
    let v = min + frac.clamp(0., 1.) * (max - min);
    ((v / step).round() * step).clamp(min, max)
}

/// 值落地（拖动中）：只写内存预览 + 重绘，落盘留给松手
/// （[`crate::services::workspace::save_app_settings_now`]）。
fn apply_value(weak: &gpui::WeakEntity<Chat>, v: f32, cx: &mut gpui::App) {
    crate::services::workspace::preview_chat_content_width(v.max(0.) as u32);
    let _ = weak.update(cx, |_, cx| cx.notify());
}

thread_local! {
    /// 滑块拖拽中。必须是跨帧状态：`on_mouse_event` 监听在 paint 期每帧重
    /// 注册，闭包里的局部变量活不过一次重绘——拖拽中每次改值都会 notify
    /// 重绘，用局部 Cell 做拖拽标志会在第一帧就自己复位（同
    /// `ui/psp_scrollbar.rs` 的 DRAG 处理）。单窗口应用，thread_local 够用。
    static WIDTH_DRAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl SliderGeom {
    fn paint(
        &self,
        bounds: gpui::Bounds<gpui::Pixels>,
        weak: &gpui::WeakEntity<Chat>,
        window: &mut Window,
    ) {
        let knob = px(SLIDER_KNOB);
        let x0 = bounds.left() + knob / 2.;
        let x1 = bounds.right() - knob / 2.;
        let frac = ((self.value - self.min) / (self.max - self.min)).clamp(0., 1.);
        let knob_x = x0 + (x1 - x0) * frac;
        let mid_y = bounds.center().y;
        let track_bounds = gpui::Bounds::new(
            gpui::point(x0, mid_y - px(SLIDER_TRACK_H) / 2.),
            gpui::size(x1 - x0, px(SLIDER_TRACK_H)),
        );
        let radius = gpui::Corners::all(px(SLIDER_TRACK_H / 2.));
        let plain = |b: gpui::Bounds<gpui::Pixels>, color: gpui::Hsla, r: gpui::Corners<gpui::Pixels>| {
            gpui::quad(
                b,
                r,
                color,
                gpui::Edges::default(),
                gpui::transparent_black(),
                gpui::BorderStyle::default(),
            )
        };
        window.paint_quad(plain(track_bounds, self.track, radius));
        if knob_x > x0 {
            let done = gpui::Bounds::new(
                track_bounds.origin,
                gpui::size(knob_x - x0, px(SLIDER_TRACK_H)),
            );
            window.paint_quad(plain(done, self.fill, radius));
        }
        let knob_bounds = gpui::Bounds::new(
            gpui::point(knob_x - knob / 2., mid_y - knob / 2.),
            gpui::size(knob, knob),
        );
        window.paint_quad(gpui::quad(
            knob_bounds,
            gpui::Corners::all(knob / 2.),
            self.knob,
            gpui::Edges::all(px(1.5)),
            self.fill,
            gpui::BorderStyle::default(),
        ));

        // ---- 鼠标三件套（capture 相；命中才吞事件；窗口级监听每帧重注册）----
        let hit = bounds;
        let (min, max, step) = (self.min, self.max, self.step);
        // 按下：跳值（点轨道即定位）+ 起拖
        {
            let weak = weak.clone();
            window.on_mouse_event(
                move |ev: &gpui::MouseDownEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Capture
                        || ev.button != MouseButton::Left
                        || !hit.contains(&ev.position)
                    {
                        return;
                    }
                    cx.stop_propagation();
                    WIDTH_DRAG.with(|d| d.set(true));
                    apply_value(&weak, value_at(hit, min, max, step, ev.position.x), cx);
                },
            );
        }
        // 移动：拖动中跟手（指针移出 180px 轨道也继续）
        {
            let weak = weak.clone();
            window.on_mouse_event(
                move |ev: &gpui::MouseMoveEvent, phase, _, cx| {
                    if phase != gpui::DispatchPhase::Capture
                        || !WIDTH_DRAG.with(|d| d.get())
                    {
                        return;
                    }
                    if !ev.dragging() {
                        WIDTH_DRAG.with(|d| d.set(false));
                        return;
                    }
                    cx.stop_propagation();
                    apply_value(&weak, value_at(hit, min, max, step, ev.position.x), cx);
                },
            );
        }
        // 松开：一次性落盘（拖动期只写内存预览）
        window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, _, _cx| {
            if phase != gpui::DispatchPhase::Capture || !WIDTH_DRAG.with(|d| d.get()) {
                return;
            }
            WIDTH_DRAG.with(|d| d.set(false));
            crate::services::workspace::save_app_settings_now();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 轨道 180px / 旋钮 12px：可用行程 168px，两端旋钮中心对应 min/max。
    fn track() -> gpui::Bounds<gpui::Pixels> {
        gpui::Bounds::new(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(180.), px(18.)),
        )
    }

    #[test]
    fn width_slider_maps_x_to_quantized_value() {
        // 左端（旋钮中心）= min、右端 = max、中点 = 中值
        assert_eq!(value_at(track(), 560., 1600., 10., px(6.)), 560.);
        assert_eq!(value_at(track(), 560., 1600., 10., px(174.)), 1600.);
        assert_eq!(value_at(track(), 560., 1600., 10., px(90.)), 1080.);
    }

    #[test]
    fn width_slider_clamps_outside_track_and_quantizes() {
        assert_eq!(value_at(track(), 560., 1600., 10., px(-40.)), 560.);
        assert_eq!(value_at(track(), 560., 1600., 10., px(400.)), 1600.);
        // 未落在整档上 → 就近 10px 档（x=12 ≈ 596 → 600）
        assert_eq!(value_at(track(), 560., 1600., 10., px(12.)), 600.);
    }
}
