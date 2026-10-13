//! statusbar (v54): 36px，只在面板段（panel-col 内），三个面板 tab
//! （psp / 文件树 / Git）46px 宽 icon 即标签。激活 tab = nav 色连体卡
//! （顶无边、底圆角、accent 色），与 dock 背景连成一体。右端 = 设置钮
//! （016 下放，原手机图标位；手机遥控迁 topbar 左段）。收起态整个隐藏
//! （panel-col 随 panes_hidden 不渲染）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::DockPanel;
use crate::services::workspace::{save_ui_state, ui_state};
use crate::appearance::theme::theme as T;
use crate::ui::tokens::align;

const HEIGHT: f32 = 36.;

pub(crate) fn control_bar(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> impl gpui::IntoElement {
    let t = T();
    let active = chat.layout.dock_panel;

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
                .children(tabs(active, t, cx)),
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

fn tabs(
    active: DockPanel,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> Vec<impl gpui::IntoElement> {
    [
        (DockPanel::Sessions, "messages-square", "psp-tab"),
        (DockPanel::Files, "folder-tree", "files-tab"),
        // git tab 隐藏（2026-10-10 用户定夺：PF 定位傻瓜化，git 面板暂缓设计；
        // 恢复走 git 历史找回本行）
        // (DockPanel::Git, "git-branch", "git-tab"),
    ]
    .iter()
    .map(|(panel, icon_name, id)| {
        let on = *panel == active;
        let panel = *panel;
        div()
            .id(SharedString::from(*id))
            .w(px(46.))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(if on { rgb(t.accent) } else { rgb(t.text_muted) })
            .when(on, |d| {
                // 连体卡：nav 底色 + 边框（顶无边）+ 底圆角 + 顶穿 1px
                d.bg(rgb(t.nav))
                    .border_1()
                    .border_t_0()
                    .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
                    .rounded_bl(px(9.))
                    .rounded_br(px(9.))
                    .mt(px(-1.))
            })
            .when(!on, |d| {
                d.hover(|s| s.bg(rgb(t.bg_hover)).rounded_bl(px(8.)).rounded_br(px(8.)))
            })
            .on_mouse_down(MouseButton::Left, cx.listener(
                move |this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.layout.dock_panel = panel;
                    if panel == DockPanel::Git {
                        this.refresh_git();
                        this.refresh_git_log();
                    }
                    // 2026-10-13 用户定夺：功能面板切换与主窗口（内容区）解绑——
                    // 旧的「会话 tab=chat / files tab=浏览区」绑定过时，面板归
                    // 面板，内容区归内容区（titlebar 的 tab 条管内容区）。
                    this.persist_ui();
                    cx.notify();
                },
            ))
            .child(crate::ui::icon_hover(icon_name, 16., if on { t.accent } else { t.text_muted }))
    })
    .collect()
}

/// Persist the current shell layout (panel/width/modes/collapse).
impl Chat {
    pub(crate) fn persist_ui(&mut self) {
        let mut st = ui_state();
        st.panel = self.layout.dock_panel.as_str().to_string();
        st.slp_w = self.layout.slp_w;
        st.panes_hidden = self.layout.panes_hidden;
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
