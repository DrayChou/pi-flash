//! 设置「关于」页（009-06）：应用说明与基础互动信息。纯静态 set-row
//! 版式（作者 / PF版本 / 内核版本 / GitHub），唯一交互是 GitHub 链接
//! 点击 = cx.open_url（plugins 页 pi.dev 链接同款）。「说明」两行文案
//! 独占整行块——右控列放不下长句。

use super::*;

/// GitHub 主页（009-06；点击 = 系统浏览器打开）。
const GITHUB_URL: &str = "https://github.com/danielbill/pi-flash";

pub(crate) fn mc_about_view() -> gpui::AnyElement {
    let t = T();
    let mut col = div().id("mc-about").flex().flex_col();

    // 作者
    col = col.child(set_row(
        tr("作者"),
        tr("出品与维护"),
        value_el(tr("不正常赛博捅讯社").to_string(), true),
        t,
    ));

    // PF版本：icon Pi-Flash v…
    col = col.child(set_row(
        tr("PF版本"),
        tr("当前安装的 Pi-Flash 版本"),
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(icon("logo-marks", 16., t.accent))
            .child(value_el(format!("Pi-Flash v{}", env!("CARGO_PKG_VERSION")), true))
            .into_any_element(),
        t,
    ));

    // 内核版本：内置 pi-coding-agent（vendored）
    col = col.child(set_row(
        tr("内核版本"),
        tr("vendored pi-coding-agent"),
        value_el(
            format!(
                "Pi v{}",
                pi_link::vendor::vendored_version().unwrap_or_default()
            ),
            false,
        ),
        t,
    ));

    // GitHub：url 可点击
    col = col.child(set_row(
        "GitHub",
        tr("项目主页（点击打开）"),
        div()
            .id("about-github")
            .font_family(crate::editor::markdown::MONO_FAMILY)
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(t.accent))
            .underline()
            .cursor_pointer()
            .hover(|s| s.opacity(0.75))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
                cx.open_url(GITHUB_URL);
            })
            .child(SharedString::from(GITHUB_URL))
            .into_any_element(),
        t,
    ));

    // 说明：两行文案独占整行块
    col = col.child(
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .pt(px(16.))
            .child(section_title(tr("说明")))
            .child(note(tr("windows上Pi的极速Desktop + 文件管理编辑 + markdown增强")))
            .child(note(tr("Pi-Flash内核将保持轻巧，专注速度体验"))),
    );

    col.into_any_element()
}

/// 右侧值文本（mono 12px；bright=text 色用于强调值，否则 text_dim）。
fn value_el(text: String, bright: bool) -> gpui::AnyElement {
    let t = T();
    div()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(if bright { t.text } else { t.text_dim }))
        .child(SharedString::from(text))
        .into_any_element()
}
