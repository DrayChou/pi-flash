//! 树行内改名/新建（Zed project_panel EditState）的编辑行装配：编辑 chrome
//! （校验边框色 + 消息浮条）、输入槽、新建虚拟行。自 file_tree.rs 内切拆出
//! （check_arch 文件 1500 行上限），纯结构搬移。

use std::path::Path;

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::services::file_icons;
use crate::ui::icon_path;

use super::file_tree::{CONTENT_PAD, INDENT, ROW_H, guide_lines};

/// 编辑态 chrome：边框色（校验错误红 / 警告黄 / 常态 accent）+ 校验消息。
pub(crate) fn edit_chrome(
    e: &crate::function_panel::TreeEdit,
    t: &'static crate::appearance::theme::Theme,
) -> (u32, Option<SharedString>) {
    match &e.validation {
        crate::function_panel::TreeValidation::Error(m) => {
            (t.danger, Some(SharedString::from(m.clone())))
        }
        crate::function_panel::TreeValidation::Warning(m) => (
            crate::appearance::theme::WARN,
            Some(SharedString::from(m.clone())),
        ),
        crate::function_panel::TreeValidation::None => (t.accent, None),
    }
}

/// 校验消息浮条（Zed render 的 validation_color_and_message 同位：行下缘
/// 绝对定位、描边小卡）。
pub(crate) fn validation_note(message: SharedString, color: u32) -> gpui::AnyElement {
    div()
        .absolute()
        .top_full()
        .left(px(-1.))
        .right(px(-1.))
        .py(px(4.))
        .px(px(8.))
        .border_1()
        .border_color(rgb(color))
        .bg(rgb(crate::appearance::theme::theme().bg))
        .text_size(crate::appearance::ui_size(10.5))
        .text_color(rgb(color))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .child(message)
        .into_any_element()
}

/// 行内编辑输入槽：输入框吃满名字区，内部左键 stop_propagation——点击
/// 输入框本身不得触发面板级「点别处提交」。
pub(crate) fn edit_input_slot(input: gpui::Entity<crate::ui::TextInput>) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .ml(px(6.))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(input)
        .into_any_element()
}

/// 行内新建行（Zed NEW_ENTRY_ID 虚拟行）：目录行下方、子项之前，缩进 =
/// 目录深度 + 1；文件图标随键入名实时变化；行带编辑边框 + 校验消息。
/// 无点击动作（行体点击 = 点别处，冒泡到面板收场，同 Zed blur）。
pub(crate) fn tree_edit_row(
    edit: crate::function_panel::TreeEdit,
    depth: usize,
    live_name: &str,
) -> gpui::AnyElement {
    let t = crate::appearance::theme::theme();
    let (border, message) = edit_chrome(&edit, t);
    let icon = if edit.is_dir {
        file_icons::get_generic_folder_icon(false)
    } else if live_name.chars().count() > 2 {
        file_icons::get_icon(Path::new(live_name))
    } else {
        file_icons::get_icon(Path::new(""))
    };
    let mut el = div()
        .relative()
        .w_full()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .overflow_hidden()
        .pr(px(8.))
        .rounded(px(4.))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text))
        .border_1()
        .border_color(rgb(border));
    for g in guide_lines(depth, Some(depth), Some(depth), t.border) {
        el = el.child(g);
    }
    let inner = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .overflow_hidden()
        .ml(px(depth as f32 * INDENT))
        .px(px(CONTENT_PAD))
        .child(
            div()
                .flex_shrink_0()
                .child(icon_path(icon, 14., t.text_dim)),
        )
        .child(edit_input_slot(edit.input.clone()));
    el.child(inner)
        .children(message.map(|m| validation_note(m, border)))
        .into_any_element()
}
