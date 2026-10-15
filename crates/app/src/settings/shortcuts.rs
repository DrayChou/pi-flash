//! 设置「快捷键」页（082）：全局 / 文件树两组快捷键说明，set-row 版式
//! （说明左、键帽右）。键帽来源分两类：
//! - 有 action 的五项：keymap 反查（`Kbd::binding_for_action`，单一事实
//!   源，与按钮 tooltip 键位 chip 同源，改绑定自动跟随）；
//! - Ctrl+←/→ 与 F2：走渲染根按键兜底、**故意没有 action**（None-context
//!   绑定会把输入框词移动抢掉，见 shell/render.rs），键串静态解析。

use super::*;
use gpui::Keystroke;
use gpui_component::Kbd;

pub(crate) fn mc_shortcuts_view(window: &gpui::Window) -> gpui::AnyElement {
    let t = T();
    let mut col = div().id("mc-shortcuts").flex().flex_col();

    col = col.child(section_label(tr("shortcuts.group_global")));
    for (title, desc, kbd) in [
        (
            tr("psp.open_project"),
            "",
            Kbd::binding_for_action(&crate::OpenProjectPicker, None, window),
        ),
        (
            tr("psp.new_session"),
            "",
            Kbd::binding_for_action(&crate::NewSession, None, window),
        ),
        (
            tr("psp.search_sessions"),
            "",
            Kbd::binding_for_action(&crate::OpenSessionSearch, None, window),
        ),
        (
            tr("psp.maximize_session_list"),
            "",
            Kbd::binding_for_action(&crate::MaximizeSessionList, None, window),
        ),
        (
            tr("file_tree.maximize"),
            "",
            Kbd::binding_for_action(&crate::MaximizeFileTree, None, window),
        ),
        (
            tr("shortcuts.view_left"),
            tr("shortcuts.view_left_hint"),
            Some(bare_key("ctrl-left")),
        ),
        (
            tr("shortcuts.view_right"),
            tr("shortcuts.view_right_hint"),
            Some(bare_key("ctrl-right")),
        ),
    ] {
        col = col.child(kbd_row(title, desc, kbd, t));
    }

    col = col.child(section_label(tr("shortcuts.group_file_tree")));
    col = col.child(kbd_row(
        tr("file_tree.rename"),
        tr("shortcuts.rename_hint"),
        Some(bare_key("f2")),
        t,
    ));

    col.into_any_element()
}

/// 无 action 的键（渲染根按键兜底三键）：静态解析键串为 Kbd 键帽。
fn bare_key(spec: &str) -> gpui_component::Kbd {
    gpui_component::Kbd::new(Keystroke::parse(spec).expect("static key spec parses"))
}

/// 快捷键行：set_row（说明左、控件右），右侧 = Kbd 键帽。
fn kbd_row(
    title: &str,
    desc: &str,
    kbd: Option<gpui_component::Kbd>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    set_row(
        title,
        desc,
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .children(kbd)
            .into_any_element(),
        t,
    )
}

fn section_label(text: &str) -> gpui::AnyElement {
    let t = T();
    div()
        .mt(px(20.))
        .mb(px(8.))
        .text_size(crate::appearance::ui_size(13.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_muted))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}
