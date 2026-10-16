//! psp 悬浮层 (v54): 项目路径 tooltip（深色，跟随鼠标）、会话右键详情卡
//! （锚右键点、点外即收，改名/删除流）、⋯ 两级菜单（列表方式/排序方式带
//! 子菜单勾选态）、项目菜单、删除项目确认。全部挂在根渲染，事件坐标定位。
//!
//! 定位统一走 `ui::popup::PopupEdges`（2026-10-11 用户算法：锚点在视口
//! 上半 → 向下弹，反之向上；左半 → 向右弹，反之向左）——列表拉到底时
//! 菜单/详情卡不再溢出下缘，窄窗时不再溢出右缘。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::i18n::{tr, tf};
use crate::services::format::time_ago;
use crate::services::workspace::{same_path, same_ws};
use crate::appearance::theme::{Theme, danger_wash, theme as T};
use crate::ui::icon;
use crate::ui::popup::PopupEdges;

/// 根渲染挂载的 psp 悬浮层合集（tooltip / 详情卡 / 菜单 / 确认）。
pub(crate) fn psp_overlays(
    chat: &mut Chat,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let vp = window.viewport_size();
    let vp = (f32::from(vp.width), f32::from(vp.height));
    // 全窗口定位层：absolute inset_0 铺满 root，悬浮子元素相对它定位，
    // 且不参与流布局（0 高容器会把 absolute 子元素裁掉——菜单曾因此不可见）。
    // **父层不得 occlude**：inset_0 的层一挡全窗口 hit-test 都断（行悬停
    // 背景闪烁的根因）；各浮层在自己的 bounds 内各自 occlude。
    let mut el = div()
        .id("psp-overlays")
        .absolute()
        .inset_0();
    if let Some((path, x, y)) = &chat.dlg.proj_tip {
        el = el.child(proj_tip(path, *x, *y, vp, t));
    }
    if chat.dlg.editor_menu.is_some() {
        el = el.child(editor_menu_layer(chat, vp, t, cx));
    }
    // 输入面板右键菜单（031：编辑块 撤销/恢复/全选 + 剪切/复制/粘贴/删除）
    if chat.dlg.input_menu.is_some() {
        el = el.child(input_menu_layer(chat, vp, t, cx));
    }
    // 消息内容区右键菜单（单项「复制」= 复制当前消息划选）
    if chat.dlg.msg_menu.is_some() {
        el = el.child(msg_menu_layer(chat, vp, t, cx));
    }
    // 终端右键菜单（复制/粘贴/全选）；tab 已关则不再挂（防挂到空气里）
    if let Some((tab_id, x, y)) = chat.dlg.term_menu
        && chat.ws.terminals.iter().any(|t| t.id == tab_id)
    {
        el = el.child(term_menu_layer(chat, tab_id, x, y, vp, t, cx));
    }
    if chat.dlg.hover_card.is_some() {
        // 右键详情卡：打开即渲染（无悬停延迟），点卡外/再右键别行收换
        el = el.child(hover_card(chat, vp, t, cx));
    }
    if chat.dlg.tree_menu.is_some() {
        el = el.child(tree_menu_layer(chat, vp, t, cx));
    }
    // 文件树标题项目下拉（树根跨项目快切）：标题左键 / files.project_menu
    if chat.dlg.tree_proj_menu.is_some() {
        el = el.child(tree_proj_menu_layer(chat, vp, t, cx));
    }
    // topbar 标签右键菜单（zed tab_bar 菜单移植；tab 已关不再挂——防挂到
    // 空气里，顺带收槽位）
    if let Some((target, x, y)) = chat.dlg.tab_menu {
        let alive = match target {
            crate::TabMenuTarget::Session => true,
            crate::TabMenuTarget::Panel(ix) => ix < chat.ws.panel_tabs.len(),
        };
        if alive {
            el = el.child(super::tab_menu::layer(chat, target, x, y, vp, t, cx));
        } else {
            chat.dlg.tab_menu = None;
        }
    }
    if let Some((path, x, y)) = chat.dlg.confirm_prj_del.clone() {
        el = el.child(confirm_project_del(&path, x, y, vp, t, &chat.dlg.dialog_focus, cx));
    }
    if let Some(path) = chat.dlg.tree_confirm_del.clone() {
        // zed build_removal_prompt 同款：删除打开中的文件时未保存修改并入
        // 删除确认（一次确认 = 删文件 + 弃缓冲）；目录删除统计其子树内
        // 全部打开中的脏 tab（zed 对整个待删集合数 dirty_buffers）
        let dirty_count = chat
            .ws.file_cache
            .iter()
            .filter(|(p, f)| f.dirty && (same_path(p, &path) || p.starts_with(&path)))
            .count();
        el = el.child(tree_delete_confirm(
            &path,
            dirty_count,
            &chat.dlg.dialog_focus,
            cx,
        ));
    }
    if chat.dlg.tree_del_error.is_some() {
        el = el.child(tree_del_error_layer(chat, t, cx));
    }
    el.into_any_element()
}

/// 项目行 tooltip：短名 + 全路径（mono），随主题（与 toast 同族：bg_panel
/// + border + text），跟随鼠标。锚点 = 光标，统一算法：光标在右半 → 贴
/// 光标左侧弹，下半 → 贴光标上方弹。t 从外层传入（psp_overlays 已取）。
fn proj_tip(
    path: &std::path::PathBuf,
    x: f32,
    y: f32,
    vp: (f32, f32),
    t: &'static crate::appearance::theme::Theme,
) -> impl gpui::IntoElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let full = path.to_string_lossy().to_string();
    PopupEdges::around((x, y), vp, 14., 16.)
        .apply(
            div()
                .id("psp-proj-tip")
                .absolute()
                .max_w(px(300.))
                .px(px(11.))
                .py(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(t.bg_panel))
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(1.)),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.5))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(name)),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(10.5))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(full)),
        )
}

/// 会话右键详情卡：258px 浅色浮层，标题（点击=打开并改名）、项目/
/// 最后活动/消息数信息行、删除确认流。2026-10-15 定夺：悬停自动弹窗撤销，
/// 改右键召唤——锚点 = 右键点，统一算法四象限（左缘对齐鼠标 x、上/下方
/// 12px 让出光标防误点）；点卡外即收（右键别行 = 换卡）。
fn hover_card(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let card = chat.dlg.hover_card.clone().expect("hover_card checked by caller");
    let mut found: Option<(String, u64, std::time::SystemTime, String)> = None;
    for g in &chat.psp.projects {
        if let Some(info) = g.sessions.iter().find(|s| same_path(&s.path, &card.path)) {
            let title = info
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| {
                    if info.preview.is_empty() {
                        "(empty)".to_string()
                    } else {
                        info.preview.clone()
                    }
                });
            found = Some((g.name.clone(), info.message_count, info.modified, title));
            break;
        }
    }
    let Some((prj_name, msgs, modified, title)) = found else {
        return div().into_any_element();
    };
    let path = card.path.clone();
    let confirming = card.confirming;
    // 统一算法四象限：左缘对齐鼠标 x、上/下方让 12px（光标不压卡，防误点）
    let dir = PopupEdges::around((card.x, card.y), vp, 0., 12.);
    let mut el = div()
        .id("psp-hcard")
        .absolute()
        .w(px(258.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded(px(12.))
        .shadow_lg()
        // 遮挡只作用于卡片自身 bounds（父层 occlude 会断全窗口 hit-test）
        .occlude()
        .px(px(13.))
        .pt(px(11.))
        .pb(px(9.))
        .flex()
        .flex_col()
        // 卡内点击不落穿（改名/删除按钮依赖；左右键都拦——右键穿下去会
        // 在身下的行上再开一张卡）
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Right, |_, _, cx| {
            cx.stop_propagation();
        });
    // 点卡外即收（上下文卡语义）。用 capture 阶段的 `on_mouse_down_out`
    //（capture 只判「点不在卡 bounds 内」，本次点击照常冒泡给下层 = 点哪
    // 去哪），不铺全屏 catcher：铺了会遮挡整窗 hit-test。右键别行 = 收旧
    // 开新（行的右键处理器同帧置新卡）
    {
        let weak = cx.entity().downgrade();
        el = el.on_mouse_down_out(move |_, _w, cx| {
            let _ = weak.update(cx, |c, cx| c.hover_card_rename_dismiss(cx));
        });
    }
    el = dir.apply(el);
    el = el
        .children(if card.renaming {
            // 设计稿：标题原地变输入框（Enter 提交 / Esc 取消）
            card.rename_input.clone().map(|inp| {
                div().mt(px(2.)).child(inp).into_any_element()
            })
        } else {
            Some(
            div()
                .id("hc-name")
                .text_size(crate::appearance::ui_size(13.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .overflow_hidden()
                // 标题最多三行（2026-11 定夺）：够放下长会话名，再多就截断
                // 加省略号；line_clamp 自带 overflow_hidden
                .line_clamp(3)
                .text_ellipsis()
                .cursor_pointer()
                .px(px(5.))
                .py(px(1.))
                .mx(px(-5.))
                .my(px(-1.))
                .rounded(px(6.))
                .hover(|s| s.bg(rgb(t.bg_hover)))
                // 点击标题 = 原地进入改名（不切换会话）
                // 点击标题 = 原地进入改名（不切换会话）；构建/聚焦细节在
                // Chat::hover_card_begin_rename（与自动化入口共用）
                .on_mouse_down(MouseButton::Left, cx.listener(
                    move |this, _: &gpui::MouseDownEvent, _window, cx| {
                        this.hover_card_begin_rename(cx);
                    },
                ))
                .child(SharedString::from(title))
                .into_any_element(),
            )
        })
        .child(info_row("folder-closed", SharedString::from(prj_name), t))
        .child(info_row(
            "icon-clock-solid",
            // time_ago 已自带「前」后缀（format.rs 的 tf("time.d_ago")）——
            // 这里再拼一次就成「1天前前」
            SharedString::from(time_ago(modified)),
            t,
        ))
        .child(info_row(
            "message-square",
            SharedString::from(tf("psp.messages_count", &[("n", msgs.to_string())])),
            t,
        ))
        .child(
            div()
                .h(px(1.))
                .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
                .mx(px(-13.))
                .mt(px(9.))
                .mb(px(7.)),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(6.))
                .children(if confirming {
                    // 原地换 取消/确认
                    vec![
                        ghost_btn("hc-cancel", tr("action.cancel"), false, cx.listener(
                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                if let Some(c) = this.dlg.hover_card.as_mut() {
                                    c.confirming = false;
                                }
                                cx.notify();
                            },
                        ))
                        .into_any_element(),
                        solid_danger_btn("hc-ok", tr("action.confirm"), {
                            let path = path.clone();
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.dlg.hover_card = None;
                                this.delete_session(path.clone(), cx);
                            })
                        })
                        .into_any_element(),
                    ]
                } else {
                    vec![ghost_btn("hc-del", tr("action.delete"), true, cx.listener(
                        |this, _: &gpui::MouseDownEvent, _w, cx| {
                            if let Some(c) = this.dlg.hover_card.as_mut() {
                                c.confirming = true;
                            }
                            cx.notify();
                        },
                    ))
                    .into_any_element()]
                }),
        );
    let _ = &mut el;
    el.into_any_element()
}

fn info_row(icon_name: &'static str, text: SharedString, t: &'static Theme) -> impl gpui::IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mt(px(7.5))
        .text_size(crate::appearance::ui_size(12.5))
        .text_color(rgb(t.text))
        .child(icon(icon_name, 15., t.text_muted))
        .child(text)
}

fn ghost_btn(
    id: &'static str,
    label: &str,
    danger: bool,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    crate::ui::button(
        id,
        label.to_string(),
        crate::ui::BtnSize::Sm,
        if danger {
            crate::ui::BtnVariant::Danger
        } else {
            crate::ui::BtnVariant::Secondary
        },
        false,
        handler,
    )
}

fn solid_danger_btn(
    id: &'static str,
    label: &str,
    handler: impl Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl gpui::IntoElement {
    // §5.1 唯一按钮表：实心红不存在，破坏性主行动 = Danger 描边款
    crate::ui::button(
        id,
        label.to_string(),
        crate::ui::BtnSize::Sm,
        crate::ui::BtnVariant::Danger,
        false,
        handler,
    )
}

// ---------------------------------------------------------------------------
// psp 菜单（⋯ 项目菜单已于 020-1 撤除：浏览器/终端入口在文件树操作栏，
// 删除由项目行垃圾桶直弹确认卡）
// ---------------------------------------------------------------------------

/// 统一菜单卡座：absolute 弹层基座，贴边由统一算法的 [`PopupEdges`] 决定。
pub(crate) fn menu_card(edges: PopupEdges, t: &'static Theme) -> gpui::Div {
    edges.apply(
        div()
            .absolute()
            .min_w(px(200.))
            .p(px(4.))
            .bg(rgb(t.bg))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
            .rounded(px(8.))
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            }),
    )
}

fn menu_item<S: Into<SharedString>>(
    id: S,
    icon_name: &'static str,
    label: &str,
    danger: bool,
    checked: bool,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .py(px(7.))
        .rounded(px(6.))
        .text_size(crate::appearance::ui_size(12.))
        .text_color(if danger { rgb(t.danger) } else { rgb(t.text) })
        .cursor_pointer()
        .hover(|s| {
            if danger {
                s.bg(gpui::rgba(danger_wash(t)))
            } else {
                s.bg(rgb(t.bg_hover))
            }
        })
        .child(icon(icon_name, 14., if danger { t.danger } else { t.text_muted }))
        .child(div().flex_1().child(SharedString::from(label.to_string())))
        .children(checked.then(|| icon("check", 12., t.accent)))
}

/// 删除项目确认浮层（锚定菜单点击处，统一算法定向）。
fn confirm_project_del(
    path: &std::path::PathBuf,
    x: f32,
    y: f32,
    vp: (f32, f32),
    t: &'static Theme,
    focus: &gpui::FocusHandle,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let path = path.clone();
    // 浮层公共基座（遮挡不穿透 + 点外关闭 + ESC 关闭）
    let weak = cx.entity().downgrade();
    crate::ui::overlay::layer(false, Some(focus), move |_w, cx| {
        let _ = weak.update(cx, |c, cx| {
            c.dlg.confirm_prj_del = None;
            cx.notify();
        });
    })
        .child(
            PopupEdges::around((x, y), vp, 0., 0.).apply(
                div()
                    .absolute()
                    .min_w(px(380.))
                    .p(px(16.))
                    .bg(rgb(t.bg))
                    .border_1()
                    .border_color(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x73)))
                    .rounded(px(10.))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    }),
            )
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(12.5))
                        .text_color(rgb(t.text))
                        .child(SharedString::from(tf(
                            "psp.delete_project_confirm",
                            &[("name", name)],
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(ghost_btn("pd-cancel", tr("action.cancel"), false, cx.listener(
                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.dlg.confirm_prj_del = None;
                                cx.notify();
                            },
                        )))
                        .child(solid_danger_btn("pd-ok", tr("psp.confirm_delete"), {
                            let path = path.clone();
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.dlg.confirm_prj_del = None;
                                this.delete_project(path.clone(), cx);
                            })
                        })),
                ),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 021 文件树右键菜单（目录/文件两套条目）+ 删除确认
// ---------------------------------------------------------------------------

/// 文件树右键菜单：条目按设计稿 021——目录=新建/资源管理器/终端/复制/
/// @/改名/删除，文件=新建(落点=所在目录)/资源管理器/终端/浏览器(html)/
/// 复制/@/改名/删除。
/// 复用 psp 菜单的 menu_card/menu_item（同一 theme 样式族），锚点 =
/// 右键事件坐标，弹出方向走统一算法。
fn tree_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = match chat.dlg.tree_menu.clone().expect("checked by caller") {
        crate::function_panel::TreeMenu::Dir { x, y, .. } | crate::function_panel::TreeMenu::File { x, y, .. } => (x, y),
    };
    // 与 psp 菜单同参：向下弹让 12px，左右象限决定横向方向
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let weak_layer = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.dlg.tree_menu = None;
            cx.notify();
        });
    });
    let mut card = menu_card(edges, t);
    match chat.dlg.tree_menu.clone().expect("re-checked") {
        crate::function_panel::TreeMenu::Dir { path, .. } => {
            card = card
                .child(tree_item(
                    &weak,
                    "ftm-new-file",
                    "file-plus",
                    tr("file_view.new_file"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.open_file_new(path.clone(), false, cx)
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-new-dir",
                    "folder-plus",
                    tr("file_tree.new_folder"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.open_file_new(path.clone(), true, cx)
                    },
                ))
                .child(tree_pin_item(&weak, "ftm-dir-pin", &path, chat))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-dir-explorer",
                    "folder-input",
                    tr("file_tree.reveal_in_explorer"),
                    false,
                    {
                        let path = path.clone();
                        move |_c, _cx| crate::services::shell_open::reveal_in_explorer(&path)
                    },
                ))
                .child(tree_terminal_item(&weak, "ftm-dir-term", &path))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-dir-copy",
                    "copy",
                    tr("file_tree.copy_path"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_copy_path(path.clone(), cx)
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-dir-at",
                    "at-sign",
                    tr("file_tree.insert_at_mention"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_at_insert(path.clone(), true, cx)
                    },
                ));
            // 根目录（操作栏名字区右键）不出现 改名/删除——项目根本身不是
            // 可操作对象（改名会拖走整个工作区、删除等于删项目）
            if !same_path(&path, &chat.ws.cwd) {
                card = card
                    .child(menu_sep())
                    .child(tree_item(
                        &weak,
                        "ftm-dir-rename",
                        "pencil",
                        tr("file_tree.rename"),
                        false,
                        {
                            let path = path.clone();
                            move |c, cx| c.open_file_rename(path.clone(), true, false, cx)
                        },
                    ))
                    .child(tree_item(
                        &weak,
                        "ftm-dir-del",
                        "trash",
                        tr("action.delete"),
                        true,
                        {
                            let path = path.clone();
                            move |c, cx| c.confirm_tree_delete(path.clone(), cx)
                        },
                    ));
            }
        }
        crate::function_panel::TreeMenu::File { path, .. } => {
            // 新建对齐目录菜单语义：落点 = 文件所在目录（parent 取不到时
            // 兜底项目根）
            card = card
                .child(tree_item(
                    &weak,
                    "ftm-file-new-file",
                    "file-plus",
                    tr("file_view.new_file"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| {
                            let dir = path
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| c.ws.cwd.clone());
                            c.open_file_new(dir, false, cx)
                        }
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-file-new-dir",
                    "folder-plus",
                    tr("file_tree.new_folder"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| {
                            let dir = path
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| c.ws.cwd.clone());
                            c.open_file_new(dir, true, cx)
                        }
                    },
                ))
                .child(tree_pin_item(&weak, "ftm-file-pin", &path, chat))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-file-explorer",
                    "folder-input",
                    tr("file_tree.reveal_in_explorer"),
                    false,
                    {
                        let path = path.clone();
                        move |_c, _cx| crate::services::shell_open::reveal_in_explorer(&path)
                    },
                ))
                .child(tree_terminal_item(&weak, "ftm-file-term", &path));
            // 「在浏览器中打开」只对 html 页文件出现（设计稿 021）——判据
            // 与编辑器右键菜单同一处（editor/kind.rs `browser_openable`）
            if crate::editor::kind::browser_openable(&path) {
                card = card.child(tree_item(
                    &weak,
                    "ftm-file-browser",
                    "globe",
                    tr("file_tree.open_in_browser"),
                    false,
                    {
                        let path = path.clone();
                        move |_c, _cx| crate::services::shell_open::open_in_browser(&path)
                    },
                ));
            }
            card = card
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-file-copy",
                    "copy",
                    tr("file_tree.copy_path"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_copy_path(path.clone(), cx)
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-file-at",
                    "at-sign",
                    tr("file_tree.insert_at_mention"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_at_insert(path.clone(), false, cx)
                    },
                ))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-file-rename",
                    "pencil",
                    tr("file_tree.rename"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.open_file_rename(path.clone(), false, false, cx)
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-file-del",
                    "trash",
                    tr("action.delete"),
                    true,
                    {
                        let path = path.clone();
                        move |c, cx| c.confirm_tree_delete(path.clone(), cx)
                    },
                ));
        }
    }
    layer.child(card).into_any_element()
}

/// 文件树标题项目下拉（树根跨项目快切）：一览 psp.projects（会话列表里
/// 的全部项目，当前项目勾选），↑/↓ 高亮 + Enter 切换、鼠标点击直切。
///
/// 键盘路径 = overlay::confirm 同款：render 焦点仲裁把 dialog_focus 交给
/// 下拉层（track_focus），layer 的 on_key_down 接 ↑/↓/Enter（ESC 由
/// overlay::layer 自带）。锚点 = 标题 paint 期 bounds（layout.ft_title_
/// bounds）：贴标题下缘左对齐（列表盖在树体上，dropdown 语义）；下方放
/// 不下才贴标题上缘向上弹。切换语义见 tree_proj_menu_pick（活跃会话不动）。
fn tree_proj_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let menu = chat.dlg.tree_proj_menu.clone().expect("checked by caller");
    // 标题 bounds 未量到（首帧 / 面板刚好收起）→ 收槽位不挂（防挂到空气
    // 里，tab_menu 同款守卫）
    let Some(tb) = chat.layout.ft_title_bounds.borrow().as_ref().map(|b| {
        (
            f32::from(b.origin.x),
            f32::from(b.origin.y),
            f32::from(b.size.width),
            f32::from(b.size.height),
        )
    }) else {
        chat.dlg.tree_proj_menu = None;
        return div().into_any_element();
    };
    // (名字, 路径, 是否当前)：当前态渲染期判定一次，行闭包只吃快照
    let items: Vec<(String, std::path::PathBuf, bool)> = chat
        .psp
        .projects
        .iter()
        .map(|g| {
            (
                g.name.clone(),
                g.path.clone(),
                same_ws(&g.path.to_string_lossy(), &chat.ws.cwd.to_string_lossy()),
            )
        })
        .collect();
    let weak_layer = weak.clone();
    let weak_nav = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.dlg.tree_proj_menu = None;
            cx.notify();
        });
    })
    // ↑/↓ 移高亮、Enter 切换（裸修饰键才接管；ESC 落到 layer 自己的
    // escape 分支）。无输入实体参与派发，apply 不需要 defer 出派发周期
    .on_key_down(move |ev: &gpui::KeyDownEvent, _w, cx| {
        let k = &ev.keystroke;
        let delta = match k.key.as_str() {
            "up" if !k.modifiers.modified() => Some(-1i32),
            "down" if !k.modifiers.modified() => Some(1),
            _ => None,
        };
        if let Some(delta) = delta {
            cx.stop_propagation();
            let _ = weak_nav.update(cx, |c, cx| c.tree_proj_menu_move(delta, cx));
            return;
        }
        if k.key == "enter" && !k.modifiers.modified() {
            cx.stop_propagation();
            let _ = weak_nav.update(cx, |c, cx| c.tree_proj_menu_apply(cx));
        }
    });
    let row_h = 28.;
    let max_h = 320.;
    // 估算高度（行数 × 行高 + 上下 p4）判下方放不放得下；超高滚动兜底
    let est_h = ((items.len() as f32) * row_h + 8.).min(max_h);
    let below = tb.1 + tb.3 + 4.;
    let fits_below = below + est_h <= vp.1 - 8.;
    let sel_bg = gpui::rgba((t.text << 8) | 0x1a);
    let mut card = div()
        .id("ftp-card")
        .absolute()
        .left(px(tb.0))
        .w(px(240.))
        .max_h(px(max_h))
        .overflow_y_scroll()
        .p(px(4.))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded(px(8.))
        .shadow_lg()
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        });
    card = if fits_below {
        card.top(px(below))
    } else {
        card.bottom(px(vp.1 - tb.1 + 4.))
    };
    for (ix, (name, _path, current)) in items.into_iter().enumerate() {
        let row_weak = weak.clone();
        let selected = menu.sel == ix;
        card = card.child(
            div()
                .id(SharedString::from(format!("ftp-{ix}")))
                .h(px(row_h))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(6.))
                .overflow_hidden()
                .text_size(crate::appearance::ui_size(11.5))
                // 键盘高亮 = 10% 前景薄纱常驻（psp 会话行/tree 选中同色系）
                .when(selected, |d| d.bg(sel_bg).hover(move |s| s.bg(sel_bg)))
                .when(!selected, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
                .child(icon("folder", 14., t.text_dim))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(rgb(if current { t.accent } else { t.text }))
                        .child(SharedString::from(name)),
                )
                .children(current.then(|| icon("check", 12., t.accent)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = row_weak.update(cx, |c, cx| {
                        c.tree_proj_menu_pick(ix, cx);
                    });
                }),
        );
    }
    layer.child(card).into_any_element()
}

/// 编辑区右键菜单（2026-10-11 用户定案）：编辑块（复制/粘贴/剪切/全选）
/// + 文件动作块（由 editor/kind.rs 的 FileTypeHandler 声明：浏览器/预览/
/// 资源管理器/终端 + 改名/删除，本渲染器零类型特判），
/// 样式与文件树菜单同套（menu_card/menu_item/menu_sep）。编辑四项
/// 先把焦点还给编辑器再派发 Input 动作（菜单打开不改变焦点归属，
/// 但点击可能让焦点漂移，保险起见每次归位）。
fn editor_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (path, x, y) = chat.dlg.editor_menu.clone().expect("checked by caller");
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let weak_layer = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.dlg.editor_menu = None;
            cx.notify();
        });
    });
    let ed = chat
        .ws.file_cache
        .get(&path)
        .and_then(|f| f.editor.clone());
    let mut card = menu_card(edges, t);
    card = card
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-copy",
            "copy",
            tr("action.copy"),
            || Box::new(gpui_component::input::Copy),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-paste",
            "clipboard",
            tr("action.paste"),
            || Box::new(gpui_component::input::Paste),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-cut",
            "scissors",
            tr("action.cut"),
            || Box::new(gpui_component::input::Cut),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-select-all",
            "check",
            tr("action.select_all"),
            || Box::new(gpui_component::input::SelectAll),
        ))
        .child(menu_sep());
    // 文件动作块：条目与顺序由 kind handler 声明（含分隔线）——默认骨架 =
    // 资源管理器/终端 + 改名/删除；md 源码态带预览项；「在浏览器中打开」
    // 是路径判据（html/htm，同文件树菜单），保留在动作块首位
    let source_mode = chat.ws.file_cache.get(&path).is_some_and(|f| f.md_source);
    if crate::editor::kind::browser_openable(&path) {
        card = card.child(editor_item(
            &weak,
            "edm-browser",
            "globe",
            tr("file_tree.open_in_browser"),
            false,
            {
                let path = path.clone();
                move |_c, _cx| crate::services::shell_open::open_in_browser(&path)
            },
        ));
    }
    for action in crate::editor::kind::kind_of(&path).menu_file_actions(source_mode) {
        card = match action {
            crate::editor::kind::MenuAction::PreviewToggle => card.child(editor_item(
                &weak,
                "edm-preview",
                "eye",
                tr("file_view.preview"),
                false,
                {
                    let path = path.clone();
                    move |c, cx| {
                        if let Some(ft) = c.ws.file_cache.get_mut(&path) {
                            ft.md_source = false;
                        }
                        cx.notify();
                    }
                },
            )),
            crate::editor::kind::MenuAction::Explorer => card.child(editor_item(
                &weak,
                "edm-explorer",
                "folder-input",
                tr("file_tree.reveal_in_explorer"),
                false,
                {
                    let path = path.clone();
                    move |_c, _cx| crate::services::shell_open::reveal_in_explorer(&path)
                },
            )),
            crate::editor::kind::MenuAction::Terminal => {
                card.child(editor_terminal_item(&weak, "edm-term", &path))
            }
            crate::editor::kind::MenuAction::Sep => card.child(menu_sep()),
            crate::editor::kind::MenuAction::Rename => card.child(editor_item(
                &weak,
                "edm-rename",
                "pencil",
                tr("file_tree.rename"),
                false,
                {
                    let path = path.clone();
                    move |c, cx| c.open_file_rename(path.clone(), false, true, cx)
                },
            )),
            crate::editor::kind::MenuAction::Delete => card.child(editor_item(
                &weak,
                "edm-del",
                "trash",
                tr("action.delete"),
                true,
                {
                    let path = path.clone();
                    move |c, cx| c.confirm_tree_delete(path.clone(), cx)
                },
            )),
        };
    }
    layer.child(card).into_any_element()
}

/// 输入面板右键菜单（031 用户定案）：纯编辑块——撤销/恢复/全选 + 剪切/
/// 复制/粘贴/删除，样式与编辑区菜单同套（menu_card/menu_item/menu_sep，
/// 不出快捷键列：Ctrl+Z/Y/A/X/C/V 系统本就支持，菜单只是等价入口）。
/// 动作先把焦点还给 composer 输入组件再派发 Input 动作（同编辑区菜单
/// 编辑四项的接线）；粘贴派发 app 的 ComposerPaste，与 Ctrl+V 完全同
/// 路径（剪贴板含图先附件化）。
fn input_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = chat.dlg.input_menu.expect("checked by caller");
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let weak_layer = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.dlg.input_menu = None;
            cx.notify();
        });
    });
    let card = menu_card(edges, t)
        .child(composer_edit_item(
            &weak,
            "cim-undo",
            "undo-2",
            tr("action.undo"),
            || Box::new(gpui_component::input::Undo),
        ))
        .child(composer_edit_item(
            &weak,
            "cim-redo",
            "redo-2",
            tr("action.redo"),
            || Box::new(gpui_component::input::Redo),
        ))
        .child(composer_edit_item(
            &weak,
            "cim-select-all",
            "check",
            tr("action.select_all"),
            || Box::new(gpui_component::input::SelectAll),
        ))
        .child(menu_sep())
        .child(composer_edit_item(
            &weak,
            "cim-cut",
            "scissors",
            tr("action.cut"),
            || Box::new(gpui_component::input::Cut),
        ))
        .child(composer_edit_item(
            &weak,
            "cim-copy",
            "copy",
            tr("action.copy"),
            || Box::new(gpui_component::input::Copy),
        ))
        .child(composer_paste_item(&weak, "cim-paste"))
        .child(composer_edit_item(
            &weak,
            "cim-delete",
            "trash",
            tr("action.delete"),
            || Box::new(gpui_component::input::Delete),
        ));
    layer.child(card).into_any_element()
}

/// 消息内容区右键菜单：单项「复制」= 复制当前消息划选（样式同输入面板
/// 右键菜单 menu_card/menu_item 套件）。文案取打开菜单当帧的划选快照——
/// 菜单开着时没有会清选区的路径（TextView 已 vendor 补丁为仅左键起选/
/// 清除，点菜单项本身有 stop_propagation），快照即点击时的真实选区；
/// 无划选时快照为空，点击自然空转。
fn msg_menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = chat.dlg.msg_menu.expect("checked by caller");
    let copied = gpui_component::text::take_last_selected_text(cx);
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let weak_layer = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.dlg.msg_menu = None;
            cx.notify();
        });
    });
    let copied_click = copied.clone();
    let card = menu_card(edges, t).child(
        menu_item("msg-copy", "copy", tr("action.copy"), false, false).on_mouse_down(
            MouseButton::Left,
            move |_, _, cx| {
                cx.stop_propagation();
                if let Some(text) = copied_click.clone() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
                let _ = weak.update(cx, |c, cx| {
                    c.dlg.msg_menu = None;
                    cx.notify();
                });
            },
        ),
    );
    layer.child(card).into_any_element()
}

/// 输入菜单动作项接线：点击 = 焦点归位 composer 输入组件 + 派发 Input
/// 动作 + 收菜单（同 editor_edit_item，目标换成 composer 门面内的
/// InputState——菜单打开不改变焦点归属，但点击可能让焦点漂移，保险
/// 每次归位，收起后键盘接着在输入框里用）。
fn composer_edit_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    icon_name: &'static str,
    label: &str,
    make: fn() -> Box<dyn gpui::Action>,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    menu_item(SharedString::from(id.to_string()), icon_name, label, false, false)
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                if let Some(comp) = &c.composer.composer {
                    let handle = comp.read(cx).focus_handle_in(cx);
                    window.focus(&handle);
                    window.dispatch_action(make(), cx);
                }
                c.dlg.input_menu = None;
                cx.notify();
            });
        })
}

/// 输入菜单「粘贴」项：派发 app 的 ComposerPaste 而非组件 Paste——
/// Ctrl+V 的完整路径（剪贴板含图先附件化，纯文本再走组件粘贴）。
fn composer_paste_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    menu_item(
        SharedString::from(id.to_string()),
        "clipboard",
        tr("action.paste"),
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            if let Some(comp) = &c.composer.composer {
                let handle = comp.read(cx).focus_handle_in(cx);
                window.focus(&handle);
                window.dispatch_action(Box::new(crate::ComposerPaste), cx);
            }
            c.dlg.input_menu = None;
            cx.notify();
        });
    })
}

/// 终端右键菜单（复制 / 粘贴 / 全选）：动作作用于右键那一个终端 tab
/// （不依赖 active_terminal），完成后焦点还给终端——菜单是终端的附属
/// UI，收起后键盘要能接着用。
fn term_menu_layer(
    chat: &mut Chat,
    tab_id: usize,
    x: f32,
    y: f32,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let weak_layer = weak.clone();
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let has_sel = chat
        .ws.terminals
        .iter()
        .find(|t| t.id == tab_id)
        .is_some_and(|t| t.selection.is_some());
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.dlg.term_menu = None;
            cx.notify();
        });
    });
    let card = menu_card(edges, t)
        .child(term_menu_item(
            &weak,
            "tm-copy",
            "copy",
            tr("action.copy"),
            has_sel,
            tab_id,
            |c, tab, cx| {
                c.term_copy(tab, cx);
            },
        ))
        .child(term_menu_item(
            &weak,
            "tm-paste",
            "clipboard",
            tr("action.paste"),
            true,
            tab_id,
            |c, tab, cx| {
                c.term_paste(tab, cx);
            },
        ))
        .child(term_menu_item(
            &weak,
            "tm-select-all",
            "check",
            tr("action.select_all"),
            true,
            tab_id,
            |c, tab, cx| {
                c.term_select_all(tab, cx);
            },
        ));
    layer.child(card).into_any_element()
}

/// 终端菜单动作项：点击 = 应用动作（作用于该 tab）+ 收菜单 + 焦点回终端。
fn term_menu_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &'static str,
    icon_name: &'static str,
    label: &str,
    enabled: bool,
    tab_id: usize,
    apply: fn(&mut Chat, usize, &mut gpui::Context<Chat>),
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let item = menu_item(SharedString::from(id.to_string()), icon_name, label, false, false)
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                apply(c, tab_id, cx);
                c.dlg.term_menu = None;
                // 焦点回终端：菜单收起后键盘接着用（与打开菜单时的聚焦对称）
                if let Some(tab) = c.ws.terminals.iter().find(|t| t.id == tab_id) {
                    let f = tab.focus.clone();
                    window.focus(&f);
                }
                cx.notify();
            });
        });
    if enabled { item } else { item.opacity(0.45) }
}

/// 编辑菜单动作项接线：点击 = 应用动作 + 收**编辑菜单**（tree_item 写死
/// 收 tree_menu，编辑菜单复用它会导致菜单不消、挡住改名框/删除确认）。
fn editor_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    icon_name: &'static str,
    label: &str,
    danger: bool,
    apply: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    menu_item(SharedString::from(id.to_string()), icon_name, label, danger, false)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                apply(c, cx);
                c.dlg.editor_menu = None;
                cx.notify();
            });
        })
}

/// 编辑动作项（复制/粘贴/剪切/全选）：先归位焦点再派发 Input 动作。
fn editor_edit_item(
    weak: &gpui::WeakEntity<Chat>,
    ed: &Option<gpui::Entity<gpui_component::input::InputState>>,
    id: &'static str,
    icon_name: &'static str,
    label: &str,
    make: fn() -> Box<dyn gpui::Action>,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let ed = ed.clone();
    menu_item(SharedString::from(id.to_string()), icon_name, label, false, false)
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                if let Some(ed) = &ed {
                    ed.update(cx, |st, scx| st.focus(window, scx));
                    window.dispatch_action(make(), cx);
                }
                c.dlg.editor_menu = None;
                cx.notify();
            });
        })
}

/// 「在终端中打开」：文件 = 其所在目录（同文件树菜单语义）。
fn editor_terminal_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    path: &std::path::PathBuf,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let path = path.clone();
    menu_item(
        SharedString::from(id.to_string()),
        "icon-terminal-solid",
        tr("file_tree.open_in_terminal"),
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            let cwd = path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| path.clone());
            c.open_terminal_in(cwd, window, cx);
            c.dlg.editor_menu = None;
            cx.notify();
        });
    })
}

/// 菜单项公共接线：点击 = 应用动作 + 收菜单（同 psp 菜单项语义）。
fn tree_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    icon_name: &'static str,
    label: &str,
    danger: bool,
    apply: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    menu_item(SharedString::from(id.to_string()), icon_name, label, danger, false)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| {
                apply(c, cx);
                c.dlg.tree_menu = None;
                cx.notify();
            });
        })
}

/// 「在终端中打开」要走 window（open_terminal_in 建终端 tab），单独接线。
fn tree_terminal_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    path: &std::path::PathBuf,
) -> gpui::Stateful<gpui::Div> {
    let weak = weak.clone();
    let path = path.clone();
    menu_item(
        SharedString::from(id.to_string()),
        "icon-terminal-solid",
        tr("file_tree.open_in_terminal"),
        false,
        false,
    )
    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
        cx.stop_propagation();
        let _ = weak.update(cx, |c, cx| {
            // 文件 = 其所在目录；目录 = 自身（psp 项目菜单同语义）
            let cwd = if path.is_dir() {
                path.clone()
            } else {
                path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.clone())
            };
            c.open_terminal_in(cwd, window, cx);
            c.dlg.tree_menu = None;
            cx.notify();
        });
    })
}

/// 021 右键菜单「Pin/取消固定」项：已在收藏夹 = 取消固定，反之加入
/// （toggle 一条写路 tree_pin_toggle）。
fn tree_pin_item(
    weak: &gpui::WeakEntity<Chat>,
    id: &str,
    path: &std::path::PathBuf,
    chat: &Chat,
) -> gpui::Stateful<gpui::Div> {
    let pinned = chat.ws.pinned.iter().any(|p| same_path(p, path));
    tree_item(
        weak,
        id,
        "pin",
        if pinned {
            tr("file_tree.unpin")
        } else {
            tr("file_tree.pin")
        },
        false,
        {
            let path = path.clone();
            move |c, cx| c.tree_pin_toggle(path.clone(), cx)
        },
    )
}

/// 菜单分组分隔线（§5.6：1px t.border）。
pub(crate) fn menu_sep() -> gpui::Div {
    div()
        .h(px(1.))
        .my(px(4.))
        .bg(gpui::rgba(crate::appearance::theme::border_alpha(T(), 0x66)))
}

/// 删除失败持久弹层：trash Err 必看（状态栏 toast 一闪容易漏）。
fn tree_del_error_layer(
    chat: &mut Chat,
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let msg = chat.dlg.tree_del_error.clone().expect("checked by caller");
    let weak_dismiss = weak.clone();
    let weak_ok = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_dismiss.update(cx, |c, cx| {
            c.dlg.tree_del_error = None;
            cx.notify();
        });
    });
    let card = crate::ui::overlay::stop_click(
        div()
            .min_w(px(320.))
            .p(px(14.))
            .bg(rgb(t.bg))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x73)))
            .rounded(px(10.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.5))
                    .text_color(rgb(t.text))
                    .child(crate::i18n::tf("extensions.remove_failed_msg", &[("msg", msg)])),
            )
            .child(div().flex().justify_end().child(crate::ui::button(
                "del-err-ok",
                crate::i18n::tr("extensions.ok"),
                crate::ui::BtnSize::Md,
                crate::ui::BtnVariant::Secondary,
                false,
                move |_, _, cx| {
                    let _ = weak_ok.update(cx, |c, cx| {
                        c.dlg.tree_del_error = None;
                        cx.notify();
                    });
                },
            ))),
    );
    layer
        .flex()
        .items_center()
        .justify_center()
        .child(card)
        .into_any_element()
}

/// 删除确认：统一 overlay::confirm（自动居中 + 取消/确认两键）。
fn tree_delete_confirm(
    path: &std::path::PathBuf,
    dirty_count: usize,
    focus: &gpui::FocusHandle,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let weak_cancel = weak.clone();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // zed 同款：打开中的脏文件删除时明确「未保存更改将丢失」，一次确认
    // = 删文件 + 弃缓冲（discard_file_tab 强关，不再二次确认）；目录删除
    // 报其子树内全部受影响的脏 tab 数
    let message = if dirty_count > 0 {
        crate::i18n::tf(
            "tree.delete_confirm_dirty",
            &[("name", name), ("n", dirty_count.to_string())],
        )
    } else {
        crate::i18n::tf("tree.delete_confirm", &[("name", name)])
    };
    crate::ui::overlay::confirm(
        focus,
        message,
        move |_w, cx| {
            let _ = weak_cancel.update(cx, |c, cx| {
                c.dlg.tree_confirm_del = None;
                cx.notify();
            });
        },
        // 确认走统一方法（取槽→清槽→commit），自动化 op file.delete_confirm
        // 也调它——回调里不再自持路径副本
        move |_w, cx| {
            let _ = weak.update(cx, |c, cx| c.tree_delete_confirm_commit(cx));
        },
    )
}
