use super::*;

const GITHUB_URL: &str = "https://github.com/danielbill/pi-flash";

pub(crate) fn mc_about_view() -> gpui::AnyElement {
    let t = T();
    let mut col = div().id("mc-about").flex().flex_col();

    col = col.child(set_row(tr("作者"), "", value_el(tr("不正常赛博捅讯社").to_string(), true), t));

    col = col.child(set_row(
        tr("PF版本"),
        "",
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(icon("logo-marks", 16., t.accent))
            .child(value_el(format!("Pi-Flash v{}", env!("CARGO_PKG_VERSION")), true))
            .into_any_element(),
        t,
    ));

    col = col.child(set_row(
        tr("内核版本"),
        "",
        value_el(
            format!(
                "Pi v{}",
                pi_link::vendor::vendored_version().unwrap_or_default()
            ),
            false,
        ),
        t,
    ));

    col = col.child(set_row(
        "GitHub",
        "",
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

    col = col.child(
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .pt(px(16.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_muted))
                    .child(tr("说明")),
            )
            .child(note(tr(
                "windows上Pi的极速Desktop + 文件管理编辑 + markdown增强",
            )))
            .child(note(tr("Pi-Flash内核将保持轻巧，专注速度体验"))),
    );

    col.into_any_element()
}

fn value_el(text: String, bright: bool) -> gpui::AnyElement {
    let t = T();
    div()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(if bright { t.text } else { t.text_dim }))
        .child(SharedString::from(text))
        .into_any_element()
}
