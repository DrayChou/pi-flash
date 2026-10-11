//! §5.1 文字按钮 —— 全项目唯一按钮实现（docs/UI设计/UI组件规范.md）。
//!
//! 三档高（SM24/MD28/LG32）× 三变体（Primary/Secondary/Danger），尺寸取
//! [`super::tokens::btn`]，圆角 R_CTRL(6)，Danger 走 theme 危险系
//! （红字 + 35% 红描边，hover 12% 红洗底）。存量手搓按钮一律收敛到这里。

use gpui::{AnyElement, IntoElement, InteractiveElement, ParentElement, Styled, SharedString, FontWeight, MouseButton, div, px, rgb};

use super::tokens::{btn, radius};

#[derive(Clone, Copy, PartialEq)]
pub enum BtnSize {
    /// h24 · px8 · ui(11)：行内/git 面板等紧凑位
    Sm,
    /// h28 · px12 · ui(12)：默认（弹窗按钮等）
    Md,
    /// h32 · px14 · ui(12)：设置页表单主按钮
    Lg,
}

#[derive(Clone, Copy, PartialEq)]
pub enum BtnVariant {
    /// accent 底白字 Semibold，hover accent_hover
    Primary,
    /// 透明底 + t.border 描边，hover bg_hover
    Secondary,
    /// 红字 + 35% 红描边，hover 红 12% 洗底
    Danger,
}

/// 样式化按钮基座（空内容）：高级调用方自拼 children（图标+文字等）后
/// 自挂 `on_mouse_down`；普通文字按钮用 [`button`]。
pub fn button_base(
    id: impl Into<SharedString>,
    size: BtnSize,
    variant: BtnVariant,
) -> gpui::Stateful<gpui::Div> {
    let t = crate::theme::theme();
    let (h, hpad, ts) = match size {
        BtnSize::Sm => (btn::SM_H, btn::SM_PX, 11.),
        BtnSize::Md => (btn::MD_H, btn::MD_PX, 12.),
        BtnSize::Lg => (btn::LG_H, btn::LG_PX, 12.),
    };
    let el = div()
        .id(id.into())
        .h(px(h))
        .px(px(hpad))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::CTRL))
        .text_size(crate::appearance::ui_size(ts));
    let el = match variant {
        BtnVariant::Primary => el
            .border_1()
            .border_color(rgb(t.accent))
            .bg(rgb(t.accent))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(t.accent_contrast)),
        BtnVariant::Secondary => el
            .border_1()
            .border_color(rgb(t.border))
            .text_color(rgb(t.text)),
        BtnVariant::Danger => el
            .border_1()
            .border_color(gpui::rgba(crate::theme::danger_alpha(t, 0x59)))
            .text_color(rgb(t.danger)),
    };
    el.cursor_pointer().hover(move |s| match variant {
        BtnVariant::Primary => s.bg(rgb(t.accent_hover)),
        BtnVariant::Danger => s.bg(gpui::rgba(crate::theme::danger_wash(t))),
        BtnVariant::Secondary => s.bg(rgb(t.bg_hover)),
    })
}

/// 文字按钮（三档 × 三变体）。`on_click` 收
/// `(&MouseDownEvent, &mut Window, &mut App)`——Chat 场景在闭包里
/// `weak.update` 即可，与 `cx.listener` 产物直接兼容。
pub fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    size: BtnSize,
    variant: BtnVariant,
    disabled: bool,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let el = button_base(id, size, variant).child(label.into());
    if disabled {
        return el.opacity(0.5).into_any_element();
    }
    el.on_mouse_down(MouseButton::Left, on_click)
        .into_any_element()
}

/// §5.2 图标按钮尺寸：方形槽 Sm22/Md28/Lg32，内图标 14/16/18。
/// Sm/Lg 为规范预留档（存量收敛时启用），当前只有 Md 有消费方。
#[allow(dead_code)]
pub enum IconBtnSize {
    /// 22 槽 · 14 图标：tab 关闭 ×、行内 ⋯ 触发钮
    Sm,
    /// 28 槽 · 16 图标：行操作钮、弹窗关闭 ×
    Md,
    /// 32 槽 · 18 图标：topbar、psp 标题行操作
    Lg,
}

/// §5.2 图标按钮：方形、无边框、圆角 6（R_CTRL），hover bg_hover，
/// 禁用整体 opacity 0.5 且不挂点击。
pub fn icon_button(
    id: impl Into<SharedString>,
    icon: &'static str,
    size: IconBtnSize,
    disabled: bool,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let t = crate::theme::theme();
    let (slot, ix) = match size {
        IconBtnSize::Sm => (22., 14.),
        IconBtnSize::Md => (28., 16.),
        IconBtnSize::Lg => (32., 18.),
    };
    let el = div()
        .id(id.into())
        .size(px(slot))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::CTRL))
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .child(crate::ui::icon(icon, ix, t.text));
    if disabled {
        return el.opacity(0.5).into_any_element();
    }
    el.cursor_pointer()
        .on_mouse_down(MouseButton::Left, on_click)
        .into_any_element()
}
