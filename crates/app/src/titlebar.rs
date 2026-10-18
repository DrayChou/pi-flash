//! topbar 三段 (016): 左段在 panel-col 内（收放钮；手机遥控钮已隐藏——
//! 2026-10-10 定，060 管线保留），右段在 content-col 内（中段标签栏 +
//! 窗口控制钮）。中段标签栏 =
//! 置顶会话 tab（第一位，无 ×；行尾 ⋯ 菜单 2026-10-11 撤销，三入口改
//! session tab 下方功能栏 session::session_fn_bar）+ 自由标签区（终端/文件，
//! 023 规则），
//! 激活 tab Obsidian 卡片融底、条上不画横线。设置钮下放 018 状态栏、+ 已删
//! （016 定案）。整条是客户区自绘标题栏——空段挂 `WindowControlArea::Drag`
//! 命中盒，三个窗口钮注册 Min/Max/Close 命中盒（zed platform_title_bar
//! parity）。收起态面板全隐，收放钮跳到右段起点（竖线镜像位）。

use gpui::{MouseButton, SharedString, Window, div, prelude::*, px, rgb};
use std::path::PathBuf;
use gpui::WindowControlArea;

use crate::Chat;
use crate::ContentView;
use crate::TabKey;
use crate::i18n::tr;
use crate::appearance::theme::theme as T;
use crate::services::file_icons;
use crate::ui::tokens::align;

/// tab 拖拽载荷（zed DraggedTab 单 pane 版，2026-10-14）：`ix` 供悬停
/// 插入侧指示判向；`key` 供 drop 落位（稳定键，防中途列表漂移错位）。
/// TypeId 与树拖拽 TreeDrag 互不串扰。会话 tab 不接 on_drag（固定段
/// 禁拖，016 定案）。
#[derive(Clone)]
pub(crate) struct TabDrag {
    pub(crate) ix: usize,
    pub(crate) key: TabKey,
}

/// tab 拖拽元信息（content_tab 入参）：key + ghost 数据一次传齐，
/// 避免 content_tab 参数继续膨胀。
pub(crate) struct TabDragMeta {
    pub(crate) key: TabKey,
    pub(crate) path: Option<PathBuf>,
}

/// tab 拖拽幽灵（gpui 把 `cx.active_drag.view` 画在光标处）：icon + 标
/// 题的浮起小签，zed tab 自克隆的极简版（等高、卡片底、浮影）。
pub(crate) struct TabDragGhost {
    pub(crate) label: String,
    pub(crate) icon: Option<&'static str>,
    pub(crate) path: Option<PathBuf>,
}

impl gpui::Render for TabDragGhost {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let t = T();
        // 图标：终端/更新日志固定 icon；文件按后缀取类型 icon
        let icon: Option<&'static str> = self
            .icon
            .or_else(|| self.path.as_deref().map(file_icons::get_icon));
        div()
            .flex()
            .items_center()
            .gap(px(9.))
            .h(px(HEIGHT - 4.))
            .max_w(px(300.))
            .px(px(12.))
            .rounded(px(7.))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
            .bg(rgb(t.bg))
            .shadow_md()
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(t.text))
            .children(icon.map(|n| crate::ui::icon(n, 15., t.text_muted)))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(self.label.clone())),
            )
    }
}

/// topbar 高（两段同高；窗口控制钮高度跟随；2026-10-10 定夺 36→38，
/// 2026-10-13 定夺 38→40 与 statusbar 等高）。
pub(crate) const HEIGHT: f32 = 40.;

/// Caption-button glyph font (Win11; MDL2 covers Win10).
pub const CAPTION_FONT: &str = "Segoe Fluent Icons";

/// 左段（015 功能面板侧）：收放钮贴左 = `tokens::align::TOGGLE_ML`（图标
/// 视觉线 ICON_LINE 与 psp 列表图标对齐，2026-10-13 定稿）+ 拖拽填充。原
/// 右端手机遥控钮已隐藏（2026-10-10 用户定夺；重开走 git 历史找回 wx_btn）。
pub(crate) fn topbar_l(_chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> impl gpui::IntoElement {
    let t = T();
    div()
        .id("topbar-l")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(t.chrome))
        .child(div().ml(px(align::TOGGLE_ML)).child(icon_btn(
            "panes-toggle",
            "panel-left",
            tr("titlebar.toggle_sidebar"),
            cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                this.toggle_panes(cx);
            }),
        )))
        // drag filler: the empty middle is the drag region (HTCAPTION —
        // platform handles move + double-click-zoom)
        .child(
            div()
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
}

/// 右段：中段标签栏 + 窗口控制。
pub(crate) fn topbar_r(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    let maximized = window.is_maximized();

    let mut bar = div()
        .id("topbar-r")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .relative()
        .flex()
        .items_center()
        .bg(rgb(t.chrome));

    // 收起态：收放钮跳到右段起点（4px 等距，竖线镜像位）
    if chat.layout.panes_hidden {
        bar = bar
            .child(
                div()
                    .ml(px(4.))
                    .mr(px(2.))
                    .child(icon_btn(
                        "panes-toggle-r",
                        "panel-left",
                        tr("titlebar.expand_sidebar"),
                        cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                            this.toggle_panes(cx);
                        }),
                    )),
            )
            .child(div().w(px(1.)).h(px(18.)).bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c))).mx(px(4.)));
    }

    // 016 中段标签栏：置顶会话 tab + 自由标签区（终端/文件），激活 tab
    // 卡片融底（Obsidian 式，条上不画横线）
    bar = bar.child(tab_strip(chat, cx));

    // 标签区右缘到最小化钮左缘固定 20px（016 2026-10-14 定案），仍是
    // 窗口拖拽命中区
    bar = bar.child(
        div()
            .w(px(20.))
            .flex_shrink_0()
            .h_full()
            .window_control_area(WindowControlArea::Drag),
    );

    // 016 右段：仅窗口控制钮（设置下放 018 状态栏、+ 删除）
    // v55: min/max switch to SVG icons via HoverIcon; close keeps red-bg exemption
    for (area, icon_name) in [
        (WindowControlArea::Min, "minus"),
        (WindowControlArea::Max, if maximized { "restore" } else { "square" }),
    ] {
        bar = bar.child(caption_button(area, icon_name, t));
    }
    bar.child(caption_button(
        WindowControlArea::Close,
        "\u{E8BB}",
        t,
    ))
}

/// 30×30 图标钮（topbar 通用；hover chrome-hover）。钮盒/图标走 align 令牌。
fn icon_btn(
    id: &'static str,
    icon_name: &'static str,
    _tip: &'static str,
    handler: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    let t = T();
    div()
        .id(id)
        .size(px(align::TOGGLE_BTN))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .text_color(rgb(t.text_muted))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, handler)
        .child(crate::ui::icon_hover(icon_name, align::TOGGLE_ICON, t.text_muted))
}

/// 内容区 tab（Obsidian 式）：激活 = 凸起卡片（bg 色、顶圆角、下缘融入
/// 内容区、× 可见）；非激活 = 平铺文字。`ml` = 距分隔线/前一 tab 的间距。


/// 中段标签栏（016）：置顶会话 tab（固定段，第一位，距条左缘 10px）+
/// 自由标签区（终端/文件，023 规则：脏点/冲突标记在 tab 行尾）。
///
/// 标签区伸展到最小化钮左侧 20px（flex_1 + min_w_0；016 2026-10-14 定案
/// 撤 75% 限宽，回 023 的 Zed parity）；自由标签区独立横向滑动，会话 tab
/// 不参与滚动（2026-10-14 bug 修复：超多标签时旧实现把会话 tab 一起滚
/// 走——它必须恒定可见）；右走流内 20px spacer（滚到底也有留白）。
fn tab_strip(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let mut strip = div()
        .id("topbar-tabs")
        .flex_1()
        .min_w_0()
        .pl(px(10.))
        .h_full()
        .flex()
        .items_end();
    // 固定段：置顶会话 tab（滚动区外，永不滚走）
    strip = strip.child(session_tab(chat, cx));
    // 自由标签区：独立横向滑动，起点紧贴会话 tab（tab 自带 ml 4px）
    let mut tabs_host = div()
        .id("topbar-tabs-scroll")
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .items_end()
        .overflow_x_scroll();
    for (ix, tab) in chat.ws.panel_tabs.iter().enumerate() {
        let (label, path, is_file, tab_icon, tab_view): (
            SharedString,
            Option<PathBuf>,
            bool,
            Option<&'static str>,
            ContentView,
        ) = match tab {
            crate::PanelTab::Term(id) => {
                let title = chat
                    .ws.terminals
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| {
                        t.cwd
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                (title.into(), None, false, Some("icon-terminal-solid"), ContentView::Term)
            }
            crate::PanelTab::File(p) => (
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".into())
                    .into(),
                Some(p.clone()),
                true,
                None,
                ContentView::File,
            ),
            // 081 更新日志 tab（新版本首启自动打开）
            crate::PanelTab::Changelog => (
                tr("about.changelog").into(),
                None,
                false,
                Some("history"),
                ContentView::Changelog,
            ),
        };
        // 文件 tab 行尾标记（023）：冲突 ! > 脏点。zed parity（DiskState::
        // Deleted）：文件已从磁盘消失不标 !——删除线是唯一标记，脏点照常
        // （脏的已删缓冲仍显示为已修改）
        let deleted = path
            .as_ref()
            .and_then(|p| chat.ws.file_cache.get(p))
            .is_some_and(|f| f.conflict == Some(crate::FileConflict::Deleted));
        let badge: Option<gpui::AnyElement> = path.as_ref().and_then(|p| {
            let f = chat.ws.file_cache.get(p)?;
            Some(
                if f.conflict == Some(crate::FileConflict::Changed) {
                    div()
                        .flex_shrink_0()
                        .text_size(crate::appearance::ui_size(11.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(0xf87171))
                        .child(tr("status.alert"))
                } else if f.dirty {
                    div().size(px(7.)).rounded_full().flex_shrink_0().bg(rgb(0xe0a562))
                } else {
                    return None;
                }
                .into_any_element(),
            )
        });
        let active = chat.layout.content_view == tab_view && chat.ws.active_panel_tab == Some(ix);
        tabs_host = tabs_host.child(content_tab(
            if is_file { "ctab-file" } else { "ctab-term" },
            ix,
            label,
            active,
            4.,
            tab_icon,
            badge,
            deleted,
            TabDragMeta { key: tab.key(), path },
            tab_view,
            cx,
        ));
    }
    // 流内右留白（滚到底也有 20px）
    tabs_host = tabs_host.child(div().w(px(20.)).flex_shrink_0());
    // 拖到标签区空白（tab 右侧余量）= 移到末尾（zed tab bar 空白 drop 同
    // 语义）；drop 派发给最深 hover 目标（bubble + stop_propagation），
    // tab 上的 drop 不会重复进这里
    tabs_host = tabs_host.on_drop::<TabDrag>(cx.listener(
        move |this, drag: &TabDrag, _, cx| {
            let last = this.ws.panel_tabs.len().saturating_sub(1);
            this.move_panel_tab(&drag.key, last, cx);
        },
    ));
    strip.child(tabs_host).into_any_element()
}

/// 置顶会话 tab（016）：中段第一位固定段（距条左缘 10px，不随自由标签
/// 滚动——2026-10-14 定案），无 × 无 ⋯（2026-10-11
/// 定夺：⋯ 菜单三入口改到会话 tab 下方的功能栏，见 session::session_fn_bar），
/// label = 会话标题（≤30 字截断不变），前置 bot-message-square icon。激活 =
/// 会话视图（Obsidian 卡片融底）；点击回会话视图。
fn session_tab(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> gpui::AnyElement {
    let t = T();
    let active = chat.layout.content_view == ContentView::Chat;
    let title: SharedString = chat.session_title(cx).into();
    let switch = cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
        this.set_content_view(crate::ContentView::Chat);
        cx.notify();
    });
    // 右键 = 标签菜单（zed 移植）：会话 tab 恒存不可关，菜单无 Close、
    // Close Left 恒灰（见 psp_overlays::tab_menu_layer）
    let ctx_menu = cx.listener(
        move |this, ev: &gpui::MouseDownEvent, _w, cx| {
            this.dlg.tab_menu = Some((
                crate::TabMenuTarget::Session,
                f32::from(ev.position.x),
                f32::from(ev.position.y),
            ));
            cx.notify();
        },
    );
    tab_shell(
        "ctab-session",
        Some("bot-message-square"),
        title,
        active,
        0.,
        t,
        switch,
        None,
        None,
        false,
    )
    .on_mouse_down(MouseButton::Right, ctx_menu)
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn content_tab(
    id: &'static str,
    ix: usize,
    label: SharedString,
    active: bool,
    ml: f32,
    tab_icon: Option<&'static str>,
    badge: Option<gpui::AnyElement>,
    strike: bool,
    drag_meta: TabDragMeta,
    // 082 三期后视图由 activate_content_tab 从 panel_tabs[ix] 推导，调用
    // 方传入的 view 仅作签名兼容保留
    _view: ContentView,
    cx: &mut gpui::Context<Chat>,
) -> impl gpui::IntoElement {
    let t = T();
    let close = cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
        // 文件 tab：脏缓冲走确认弹窗；干净直关（含内容区回退）。逻辑收拢
        // 在 editor/tabs.rs close_tab_at——与标签右键菜单 Close 同一方法
        this.close_tab_at(ix, cx);
        cx.notify();
    });
    let switch = cx.listener(move |this, _: &gpui::MouseDownEvent, window, cx| {
        // 切 tab 只改状态，不自动聚焦内容区（082 三期：终端不再抢键盘；
        // 键入目标由内容区点击决定）。与 Ctrl+←/→ 同一条路径：
        // activate_content_tab（tabs.rs）内含视图真变化时的 window.blur()
        // 截断旧焦点所有者（避免卸载元素句柄残留持焦、回切时「复活」）。
        // 终端 = 点终端本体才持焦（term_view 的 mouse_down）；文件 = 显式
        // 打开手势（树点开/eye 切源码/链接跳转）才走 pending_focus_file。
        this.activate_content_tab(ix, window, cx);
    });
    // 行尾 ×（仅激活态由 tab_shell 渲染）——脏 tab 也要能关（关时走
    // FileDirty 确认弹窗）
    let close_x = div()
        .id(SharedString::from(format!("{id}-x")))
        .flex_shrink_0()
        .size(px(20.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .text_color(rgb(t.text_dim))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |ev, w, cx| {
            cx.stop_propagation();
            close(ev, w, cx);
        })
        .child(crate::ui::icon_hover("x", 12., t.text_dim))
        .into_any_element();
    // 右键 = 标签菜单（zed 移植）：Close/Others/Left/Right/All，锚点 =
    // 事件坐标
    let ctx_menu = cx.listener(move |this, ev: &gpui::MouseDownEvent, _w, cx| {
        this.dlg.tab_menu = Some((
            crate::TabMenuTarget::Panel(ix),
            f32::from(ev.position.x),
            f32::from(ev.position.y),
        ));
        cx.notify();
    });
    // zed pane tab 拖拽换位（2026-10-14）：on_drag 发起（ghost = icon+
    // 标题浮起小签）+ drag_over 插入侧指示 + on_drop 落位（按稳定 key
    // 现查 from，防拖拽中列表漂移）。会话 tab 不接线（固定段禁拖）。
    let ghost_label = label.to_string();
    let ghost_path = drag_meta.path.clone();
    tab_shell(
        id,
        tab_icon,
        label,
        active,
        ml,
        t,
        switch,
        Some(close_x),
        badge,
        strike,
    )
    .on_mouse_down(MouseButton::Right, ctx_menu)
    .on_drag(
        TabDrag { ix, key: drag_meta.key },
        move |_, _, _, cx| {
            cx.new(|_| TabDragGhost {
                label: ghost_label.clone(),
                icon: tab_icon,
                path: ghost_path.clone(),
            })
        },
    )
    .drag_over::<TabDrag>(move |mut style, drag, _, _| {
        // zed 插入侧指示：目标在源右 = 源插到目标前（左缘亮条）；目标在
        // 源左 = 插到目标后（右缘亮条）；原地不高亮
        if ix == drag.ix {
            return style;
        }
        style.background = Some(rgb(t.bg_hover).into());
        style.border_color = Some(rgb(t.accent).into());
        if ix > drag.ix {
            style.border_widths.left = Some(gpui::AbsoluteLength::Pixels(px(2.)));
        } else {
            style.border_widths.right = Some(gpui::AbsoluteLength::Pixels(px(2.)));
        }
        style
    })
    .on_drop::<TabDrag>(cx.listener(move |this, drag: &TabDrag, _, cx| {
        this.move_panel_tab(&drag.key, ix, cx);
    }))
}

#[allow(clippy::too_many_arguments)]
fn tab_shell(
    id: &'static str,
    icon_name: Option<&'static str>,
    label: SharedString,
    active: bool,
    ml: f32,
    t: &'static crate::appearance::theme::Theme,
    switch: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
    trailing: Option<gpui::AnyElement>,
    badge: Option<gpui::AnyElement>,
    strike: bool,
) -> gpui::Stateful<gpui::Div> {
    let mut tab = div()
        .id(id)
        .ml(px(ml))
        .flex()
        .items_center()
        .gap(px(9.))
        .text_size(crate::appearance::ui_size(12.))
        .cursor_pointer()
        .when(active, |d| {
            // 连体态：bg 填充、下缘融入下方内容区（host 已 items_end 贴
            // 底）；023① 激活/背景等高；023④ 激活 tab 限宽 300px（超长
            // 省略号截断）
            d.max_w(px(300.))
                .h(px(HEIGHT - 4.))
                .mb(px(-1.))
                .bg(rgb(t.bg))
                // 上沿不画线（2026-10-10 定夺）：只留左右两道与内容区分隔
                .border_l_1()
                .border_r_1()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
                .rounded_tl(px(9.))
                .rounded_tr(px(9.))
                .pl(px(12.))
                // 行尾有交互钮（文件/终端 tab 的 ×，自带 20px 命中区）让位
                // 5px；无行尾钮（会话 tab，⋯ 已撤 2026-10-11）右距对齐左距
                .pr(px(if trailing.is_some() { 5. } else { 12. }))
                .text_color(rgb(t.text))
        })
        .when(!active, |d| {
            // 023④ 未激活 tab 默认缩至 120px
            d.max_w(px(120.))
                .h(px(HEIGHT - 4.))
                .mb(px(-1.))
                .pl(px(12.))
                .pr(px(12.))
                .text_color(rgb(t.text_muted))
                .hover(|s| s.text_color(rgb(t.text)))
        })
        .on_mouse_down(MouseButton::Left, switch);
    tab = tab
        // 前置 icon（会话 tab = bot-message-square；终端 tab = terminal；
        // 文件 tab 无）。flex_shrink_0 必须显式：非激活 tab 限宽 100px 溢出
        // 时默认 shrink 会把 svg 连带宽一起压扁（icon 缩成团的 bug）
        .children(icon_name.map(|n| {
            div().flex_shrink_0().child(crate::ui::icon(n, 15., t.text_muted))
        }))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                // zed tab parity：文件已从磁盘消失（DiskState::Deleted）标删除线
                .when(strike, |d| d.line_through())
                .child(label),
        )
        // 行尾标记（023 文件 tab：脏点/冲突!）；行尾交互钮（文件/终端 tab
        // = × 关闭，会话 tab = ⋯ 菜单）仅激活态显示
        .children(badge)
        .when(active, |d| d.children(trailing));
    tab
}

fn caption_button(
    area: WindowControlArea,
    icon_name: &'static str,
    t: &crate::appearance::theme::Theme,
) -> impl gpui::IntoElement {
    // 关闭钮豁免：红底白字（v54 语义，glyph 渲染）；min/max 走 HoverIcon
    // 统一动效（无底色，图标自身上抬+放大，v55 全局规格）
    if area == WindowControlArea::Close {
        return div()
            .id(SharedString::from(format!("wb-{}", area as u8)))
            .w(px(42.))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .font_family(CAPTION_FONT)
            // §2：随界面字号缩放（原 text_sm 裸字号；默认档下与旧 14px 等值）
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_muted))
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(t.danger_hover)).text_color(rgb(0xffffff)))
            .window_control_area(area)
            .child(SharedString::from("\u{E8BB}"))
            .into_any_element();
    }
    div()
        .id(SharedString::from(format!("wb-{}", area as u8)))
        .w(px(42.))
        .h(px(HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.text_color(rgb(t.text)))
        .window_control_area(area)
        .child(crate::ui::icon_hover(icon_name, 13., t.text_muted))
        .into_any_element()
}
