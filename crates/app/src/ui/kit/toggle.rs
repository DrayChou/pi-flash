//! 图标开关钮（`icon_toggle` / `text_toggle`）：24×24 方形槽 + 14 图标，可选
//! 「按下」态。尺寸见 `docs/UI设计/UI组件规范.md` §5.2（图标按钮方形槽 =
//! btn::SM_H）。
//!
//! 与 `switch`（轨道式开关）分工：开关用于设置页表单行，本件用于**工具条**
//! 上语义为「模式」的一组按钮（搜索条的大小写/整词/正则、过滤、替换）。

use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, WeakEntity, div, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::ui::tooltip::WithTip as _;

/// 按下态底色：accent 16% 洗底（Zed `IconButton::toggle_state` 的视觉等价物）。
fn active_wash() -> gpui::Hsla {
    let a: gpui::Hsla = rgb(T().accent).into();
    gpui::hsla(a.h, a.s, a.l, 0.16)
}

/// 图标开关钮。`active` = 按下态（accent 洗底 + accent 图标）；`tip` 是 i18n 键。
pub(crate) fn icon_toggle(
    id: impl Into<SharedString>,
    icon_name: &'static str,
    active: bool,
    weak: &WeakEntity<Chat>,
    tip: &'static str,
    disabled: bool,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    use crate::ui::tokens::{btn, icon as icon_tok, radius};
    let t = T();
    let fg = if disabled {
        t.text_dim
    } else if active {
        t.accent
    } else {
        t.text_muted
    };
    let weak = weak.clone();
    let base = div()
        .id(id.into())
        .size(px(btn::SM_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::CTRL))
        .bg(if active {
            gpui::Fill::from(active_wash())
        } else {
            gpui::Fill::from(gpui::rgba(0x00000000))
        })
        .child(crate::ui::icon(icon_name, icon_tok::SM, fg));
    if disabled {
        return base.opacity(0.4).into_any_element();
    }
    base.cursor_pointer()
        .hover(move |s| {
            s.bg(if active {
                gpui::Fill::from(active_wash())
            } else {
                gpui::Fill::from(rgb(t.bg_hover))
            })
        })
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        })
        .with_tip(tip)
        .into_any_element()
}

/// 文字开关钮：整词匹配这类没有现成图标语义的开关，用短文字标签做槽
/// （Zed `IconName::WholeWord` 的等价物，不为一个大写「ab」再造 svg 资产）。
pub(crate) fn text_toggle(
    id: impl Into<SharedString>,
    label: &'static str,
    active: bool,
    weak: &WeakEntity<Chat>,
    tip: &'static str,
    on_click: impl Fn(&mut Chat, &mut Context<Chat>) + 'static,
) -> AnyElement {
    use crate::ui::tokens::{btn, radius};
    let t = T();
    let weak = weak.clone();
    div()
        .id(id.into())
        .h(px(btn::SM_H))
        .px(px(6.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::CTRL))
        .bg(if active {
            gpui::Fill::from(active_wash())
        } else {
            gpui::Fill::from(gpui::rgba(0x00000000))
        })
        .text_size(crate::appearance::ui_size(11.))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(if active {
            rgb(t.accent)
        } else {
            rgb(t.text_muted)
        })
        .child(SharedString::from(label))
        .cursor_pointer()
        .hover(move |s| {
            s.bg(if active {
                gpui::Fill::from(active_wash())
            } else {
                gpui::Fill::from(rgb(t.bg_hover))
            })
        })
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| on_click(c, cx));
        })
        .with_tip(tip)
        .into_any_element()
}
