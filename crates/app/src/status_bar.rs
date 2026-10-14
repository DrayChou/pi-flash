//! statusbar (v54): 40px，只在面板段（panel-col 内）。020-1 起面板 tab
//! 只剩「项目」一个（46px 宽 briefcase icon 常亮连体卡，顶无边、底圆角、
//! accent 色，与 dock 背景连成一体；文件树已并入整合面板，tab 位留给后续
//! 新面板）。右端 = 设置钮（016 下放）。收起态整个隐藏（panel-col 随
//! panes_hidden 不渲染）。

use gpui::{MouseButton, div, prelude::*, px, rgb};

use crate::Chat;
use crate::services::workspace::{save_ui_state, ui_state};
use crate::appearance::theme::theme as T;
use crate::ui::tokens::align;

const HEIGHT: f32 = 40.;

pub(crate) fn control_bar(
    _chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();

    let mut bar = div()
        .id("status-bar")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(t.chrome))
        // 上沿不画线（2026-10-10 定夺）：与内容区之间不放 border_t
        // 右缘不画线：slp|内容分隔线 = content-col 的 border_l 一条全高线，
        // statusbar 再画 border_r 会与其相邻成 2px 双线（设计稿 ::before 在
        // 浏览器亚像素下糊成一条，gpui 锐利像素下显形）
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                // 首 tab 卡片左缘落在图标对齐线（§7 / tokens::align，与上方
                // psp 列表图标同线；2026-10-13 定夺）
                .ml(px(align::ICON_LINE))
                // 020-1：面板 tab 只剩「项目」一个（briefcase 图标；文件树
                // tab 隐藏——文件树已并入上方整合面板）。tab 位保留，后续
                // 新面板在此续 tab。
                .child(project_tab(t)),
        );
    bar = bar.child(div().flex_1());

    // 016：设置钮下放状态栏右槽（原手机图标位；手机遥控迁 topbar 左段）。
    // 图标右缘对齐上方列表右线（容器 LIST_PX + 行 pr LIST_PX = 2×LIST_PX，
    // 2026-10-13 定夺「向上对齐」）——右对齐 + pr 直取线位，勿改回居中
    // （居中是 15px，会比右线多伸出 5px）
    bar = bar.child(
        div()
            .id("statusbar-settings")
            .w(px(46.))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_end()
            .pr(px(align::LIST_PX * 2.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, cx.listener(
                move |this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.open_settings(0, cx);
                },
            ))
            .child(crate::ui::icon_hover("sliders-horizontal", 16., t.text_muted)),
    );
    bar
}

/// 「项目」tab（020-1）：常亮连体卡（nav 底 + 顶无边 + 底圆角 + accent
/// 图标），与上方 dock 连成一体。整合面板常驻后无可切换对象，暂为纯
/// 指示位；后续新面板在此续可点 tab（切换走 layout.dock_panel）。
fn project_tab(t: &'static crate::appearance::theme::Theme) -> impl gpui::IntoElement {
    div()
        .id("project-tab")
        .w(px(46.))
        // tab 高 = bar 高 −4（2026-10-13 定夺，上下两 bar 同规则）
        .h(px(HEIGHT - 4.))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(t.nav))
        .border_1()
        .border_t_0()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded_bl(px(9.))
        .rounded_br(px(9.))
        // 连体卡顶贴 dock（mt = −(bar−tab) = −4 时卡片顶缘 y=0）
        .mt(px(-4.))
        .child(crate::ui::icon("icon-project", 16., t.accent))
}

/// Persist the current shell layout (panel/width/modes/collapse).
impl Chat {
    pub(crate) fn persist_ui(&mut self) {
        let mut st = ui_state();
        st.panel = self.layout.dock_panel.as_str().to_string();
        st.slp_w = self.layout.slp_w;
        st.panes_hidden = self.layout.panes_hidden;
        st.dock_split = Some(self.layout.dock_split);
        st.list_mode = match self.psp.list_mode {
            crate::ListMode::Flat => "flat".to_string(),
            crate::ListMode::Grouped => "grouped".to_string(),
        };
        st.sort_mode = match self.psp.sort_mode {
            crate::SortMode::Manual => "manual".to_string(),
            crate::SortMode::Time => "time".to_string(),
        };
        st.collapsed = self.psp.collapsed_keys.iter().cloned().collect();
        st.tree_changes_only = self.ws.changes_only;
        save_ui_state(&st);
    }

    pub(crate) fn toggle_panes(&mut self, cx: &mut gpui::Context<Self>) {
        self.layout.panes_hidden = !self.layout.panes_hidden;
        self.persist_ui();
        cx.notify();
    }
}
