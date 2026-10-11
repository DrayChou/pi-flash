//! psp 悬浮层 (v54): 项目路径 tooltip（深色，跟随鼠标）、会话 hover 详情卡
//! （侧栏右缘外 8px 锚定，改名/删除流）、⋯ 两级菜单（列表方式/排序方式带
//! 子菜单勾选态）、项目菜单、删除项目确认。全部挂在根渲染，事件坐标定位。
//!
//! 定位统一走 `ui::popup::PopupEdges`（2026-10-11 用户算法：锚点在视口
//! 上半 → 向下弹，反之向上；左半 → 向右弹，反之向左）——列表拉到底时
//! 菜单/详情卡不再溢出下缘，窄窗时不再溢出右缘。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::i18n::{tr, tf};
use crate::services::format::time_ago;
use crate::services::workspace::same_path;
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
    if let Some((path, x, y)) = &chat.proj_tip {
        el = el.child(proj_tip(path, *x, *y, vp, t));
    }
    if chat.editor_menu.is_some() {
        el = el.child(editor_menu_layer(chat, vp, t, cx));
    }
    if let Some(card) = &chat.hover_card {
        // 延迟显示（0.3s）：快速滑过会话列表时不创建视觉，不干扰行悬停
        if card.shown {
            el = el.child(hover_card(chat, vp, t, cx));
        }
    }
    if chat.psp_menu.is_some() {
        el = el.child(menu_layer(chat, vp, t, cx));
    }
    if chat.tree_menu.is_some() {
        el = el.child(tree_menu_layer(chat, vp, t, cx));
    }
    if let Some((path, x, y)) = chat.confirm_prj_del.clone() {
        el = el.child(confirm_project_del(&path, x, y, vp, t, &chat.dialog_focus, cx));
    }
    if let Some(path) = chat.tree_confirm_del.clone() {
        // zed build_removal_prompt 同款：删除打开中的文件时未保存修改并入
        // 删除确认（一次确认 = 删文件 + 弃缓冲）；目录删除统计其子树内
        // 全部打开中的脏 tab（zed 对整个待删集合数 dirty_buffers）
        let dirty_count = chat
            .file_cache
            .iter()
            .filter(|(p, f)| f.dirty && (same_path(p, &path) || p.starts_with(&path)))
            .count();
        el = el.child(tree_delete_confirm(
            &path,
            dirty_count,
            &chat.dialog_focus,
            cx,
        ));
    }
    if chat.tree_del_error.is_some() {
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

/// 会话 hover 详情卡：258px 浅色浮层，标题（点击=打开并改名）、项目/
/// 最后活动/消息数信息行、删除确认流。300ms 离行宽限 + 进卡取消。
/// 锚点 = 侧栏右缘外 8px + 行顶；统一算法决定纵边（行在下半 → 向上弹）。
fn hover_card(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let card = chat.hover_card.clone().expect("hover_card checked by caller");
    let mut found: Option<(String, u64, std::time::SystemTime, String)> = None;
    for g in &chat.projects {
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
    let anchor_x = chat.slp_w + 8.;
    let title_for_rename = title.clone();

    // 统一算法只裁决弹出方向（锚 = 行顶 y）；精准对齐是调用方语义
    //（2026-10-11 二次定夺）：向下弹卡顶 = 行顶、向上弹卡底 = 行底——
    // 卡悬在行的背离侧，行与相邻行都不被遮挡
    let dir = PopupEdges::around((anchor_x, card.y), vp, 0., 0.);
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
        // 卡内点击不落穿（改名/删除按钮依赖）
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        // 进卡取消隐藏 / 离卡进入宽限
        .on_hover(cx.listener(|this, h: &bool, _w, cx| {
            if let Some(card) = this.hover_card.as_mut() {
                card.card_hovered = *h;
                card.hide_at = (!*h).then(std::time::Instant::now);
            }
            cx.notify();
        }));
    // 横向：左半 → 卡左缘贴锚点（侧栏右缘外 8px），反之右缘贴锚点
    el = match (dir.left, dir.right) {
        (Some(l), _) => el.left(l),
        (_, Some(r)) => el.right(r),
        _ => el,
    };
    // 纵向精准对齐：行底 y+h 由 prepaint 钩子实测写入
    el = match (dir.top, dir.bottom) {
        (Some(_), _) => el.top(px(card.y)),
        (_, Some(_)) => el.bottom(px(vp.1 - card.y - card.h)),
        _ => el,
    };
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
                .on_mouse_down(MouseButton::Left, cx.listener(
                    move |this, _: &gpui::MouseDownEvent, window, cx| {
                        let Some(card) = this.hover_card.as_mut() else { return };
                        let path = card.path.clone();
                        let prefill = title_for_rename.clone();
                        let weak_ok = cx.entity().downgrade();
                        let weak_esc = weak_ok.clone();
                        let input = cx.new(|cx| {
                            crate::ui::TextInput::new(cx).select_all_on_focus()
                        });
                        input.update(cx, |ti, cx| ti.set_value(prefill, cx));
                        input.update(cx, |ti, _cx| {
                            let weak_ok = weak_ok.clone();
                            let weak_esc = weak_esc.clone();
                            let path_ok = path.clone();
                            ti.set_on_submit(Box::new(move |v, cx| {
                                let _ = weak_ok.update(cx, |c, cx| {
                                    c.rename_card_commit(&path_ok, v.trim().to_string(), cx)
                                });
                            }));
                            let path_esc = path.clone();
                            ti.set_on_escape(Box::new(move |cx| {
                                let _ = weak_esc.update(cx, |c, cx| {
                                    c.rename_card_cancel(&path_esc, cx)
                                });
                            }));
                        });
                        let handle = input.read(cx).focus_handle();
                        window.focus(&handle);
                        if let Some(card) = this.hover_card.as_mut() {
                            card.renaming = true;
                            card.rename_input = Some(input);
                        }
                        cx.notify();
                    },
                ))
                .child(SharedString::from(title))
                .into_any_element(),
            )
        })
        .child(info_row("folder-closed", SharedString::from(prj_name), t))
        .child(info_row(
            "icon-clock-solid",
            // time_ago 已自带「前」后缀（format.rs 的 tf("{n}天前")）——
            // 这里再拼一次就成「1天前前」
            SharedString::from(time_ago(modified)),
            t,
        ))
        .child(info_row(
            "message-square",
            SharedString::from(tf("{n} 条消息", &[("n", msgs.to_string())])),
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
                        ghost_btn("hc-cancel", tr("取消"), false, cx.listener(
                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                if let Some(c) = this.hover_card.as_mut() {
                                    c.confirming = false;
                                }
                                cx.notify();
                            },
                        ))
                        .into_any_element(),
                        solid_danger_btn("hc-ok", tr("确认"), {
                            let path = path.clone();
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.hover_card = None;
                                this.delete_session(path.clone(), cx);
                            })
                        })
                        .into_any_element(),
                    ]
                } else {
                    vec![ghost_btn("hc-del", tr("删除"), true, cx.listener(
                        |this, _: &gpui::MouseDownEvent, _w, cx| {
                            if let Some(c) = this.hover_card.as_mut() {
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
// ⋯ 两级菜单 + 项目菜单
// ---------------------------------------------------------------------------

type ApplyFn = Box<dyn Fn(&mut Chat, &mut gpui::Context<Chat>)>;

/// menu_item 行距约定（py7×2 + 文本行高），二级菜单行锚定与父卡高度估算
/// 都基于它——与既有实现的 34px 间距一致。
const MENU_ROW: f32 = 34.;

fn menu_layer(
    chat: &mut Chat,
    vp: (f32, f32),
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let (x, y) = match chat.psp_menu.clone().expect("checked by caller") {
        crate::PspMenu::Sort { x, y, .. } => (x, y),
        crate::PspMenu::Project { x, y, .. } => (x, y),
    };
    // 统一弹出定位（2026-10-11）：锚点 = ⋯ 按钮点击点，向下弹距 12px、
    // 向上弹同样让 12px；左右象限决定横向展开方向。
    let parent_edges = PopupEdges::around((x, y), vp, 0., 12.);
    let pop_up = parent_edges.top.is_none();
    // 浮层公共基座（遮挡不穿透 + 点外关闭 + ESC 关闭）
    let weak_layer = weak.clone();
    let mut layer = crate::ui::overlay::layer(false, Some(&chat.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.psp_menu = None;
            cx.notify();
        });
    });
    let mut card = menu_card(parent_edges, t);
    match chat.psp_menu.clone().expect("re-checked") {
        crate::PspMenu::Sort { sub, .. } => {
            let mut view = menu_item("m-list", "icon-organize", tr("列表方式"), false, false);
            view = view.on_hover(cx.listener(|this, h: &bool, _w, cx| {
                if *h {
                    if let Some(crate::PspMenu::Sort { sub, .. }) = this.psp_menu.as_mut() {
                        *sub = Some(0);
                    }
                    cx.notify();
                }
            }));
            let mut sort = menu_item("m-sort", "icon-sort", tr("排序方式"), false, false);
            sort = sort.on_hover(cx.listener(|this, h: &bool, _w, cx| {
                if *h {
                    if let Some(crate::PspMenu::Sort { sub, .. }) = this.psp_menu.as_mut() {
                        *sub = Some(1);
                    }
                    cx.notify();
                }
            }));
            card = card
                .child(view.child(menu_chev(t)))
                .child(sort.child(menu_chev(t)));
            // 二级菜单：锚定父项所在行（行屏幕 y 按父卡贴边方向折算——
            // 向上弹时卡底缘已知，行位 = 卡顶缘 + 4 + ix×行距），展开方向
            // 仍由统一算法按该行所在象限决定。
            if let Some(sub_ix) = sub {
                let card_h = 8. + 2. * MENU_ROW; // 排序菜单固定两行
                let row_y = if pop_up {
                    vp.1 - y + 12. - card_h + 4. + sub_ix as f32 * MENU_ROW
                } else {
                    y + 12. + 4. + sub_ix as f32 * MENU_ROW
                };
                let mut sub_card = menu_card(PopupEdges::around((x + 218., row_y), vp, 0., 0.), t);
                let items: Vec<(&'static str, &str, bool, ApplyFn)> = if sub_ix == 0 {
                    vec![
                        (
                            "icon-grouped",
                            tr("项目分组列表"),
                            chat.list_mode == crate::ListMode::Grouped,
                            Box::new(|c, _cx| c.list_mode = crate::ListMode::Grouped),
                        ),
                        (
                            "icon-flat",
                            tr("最近会话列表"),
                            chat.list_mode == crate::ListMode::Flat,
                            Box::new(|c, _cx| c.list_mode = crate::ListMode::Flat),
                        ),
                    ]
                } else {
                    vec![
                        (
                            "icon-clock-solid",
                            tr("按更新时间排序"),
                            chat.sort_mode == crate::SortMode::Time,
                            Box::new(|c, _cx| c.sort_mode = crate::SortMode::Time),
                        ),
                        (
                            "icon-hand",
                            tr("手动排序"),
                            chat.sort_mode == crate::SortMode::Manual,
                            Box::new(|c, _cx| c.sort_mode = crate::SortMode::Manual),
                        ),
                    ]
                };
                for (i, (icon_name, label, active, apply)) in items.into_iter().enumerate() {
                    let weak = weak.clone();
                    let it = menu_item(
                        SharedString::from(format!("msub-{sub_ix}-{i}")),
                        icon_name,
                        label,
                        false,
                        active,
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = weak.update(cx, |c, cx| {
                            apply(c, cx);
                            c.persist_ui();
                            c.psp_menu = None;
                            cx.notify();
                        });
                    });
                    sub_card = sub_card.child(it);
                }
                layer = layer.child(sub_card);
            }
        }
        crate::PspMenu::Project { path, .. } => {
            card = card
                .child(
                    menu_item(
                        "pm-explorer",
                        "folder-input",
                        tr("在文件浏览器中打开"),
                        false,
                        false,
                    )
                    .on_mouse_down(MouseButton::Left, {
                        let weak = weak.clone();
                        let path = path.clone();
                        move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak.update(cx, |c, cx| {
                                crate::services::shell_open::open_in_explorer(&path);
                                c.psp_menu = None;
                                cx.notify();
                            });
                        }
                    }),
                )
                .child(
                    menu_item("pm-terminal", "icon-terminal-solid", tr("在终端中打开"), false, false)
                        .on_mouse_down(MouseButton::Left, {
                            let weak = weak.clone();
                            let path = path.clone();
                            move |_, window, cx| {
                                cx.stop_propagation();
                                let _ = weak.update(cx, |c, cx| {
                                    c.open_terminal_in(path.clone(), window, cx);
                                    c.dock_panel = crate::DockPanel::Files;
                                    c.persist_ui();
                                    c.psp_menu = None;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    menu_item(
                        "pm-del",
                        "icon-trash-solid",
                        tr("删除项目及所有会话"),
                        true,
                        false,
                    )
                    .on_mouse_down(MouseButton::Left, {
                        let weak = weak.clone();
                        let path = path.clone();
                        move |ev: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
                            let _ = weak.update(cx, |c, cx| {
                                c.psp_menu = None;
                                c.confirm_prj_del = Some((path.clone(), x, y));
                                cx.notify();
                            });
                        }
                    }),
                );
        }
    }
    layer.child(card).into_any_element()
}

/// 统一菜单卡座：absolute 弹层基座，贴边由统一算法的 [`PopupEdges`] 决定。
fn menu_card(edges: PopupEdges, t: &'static Theme) -> gpui::Div {
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

fn menu_chev(t: &'static Theme) -> impl gpui::IntoElement {
    icon("chevron-right", 12., t.text_dim)
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
            c.confirm_prj_del = None;
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
                            "删除项目 {name} 及其所有会话？",
                            &[("name", name)],
                        ))),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(ghost_btn("pd-cancel", tr("取消"), false, cx.listener(
                            |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.confirm_prj_del = None;
                                cx.notify();
                            },
                        )))
                        .child(solid_danger_btn("pd-ok", tr("确认删除"), {
                            let path = path.clone();
                            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                                this.confirm_prj_del = None;
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
    let (x, y) = match chat.tree_menu.clone().expect("checked by caller") {
        crate::TreeMenu::Dir { x, y, .. } | crate::TreeMenu::File { x, y, .. } => (x, y),
    };
    // 与 psp 菜单同参：向下弹让 12px，左右象限决定横向方向
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let weak_layer = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.tree_menu = None;
            cx.notify();
        });
    });
    let mut card = menu_card(edges, t);
    match chat.tree_menu.clone().expect("re-checked") {
        crate::TreeMenu::Dir { path, .. } => {
            card = card
                .child(tree_item(
                    &weak,
                    "ftm-new-file",
                    "file-plus",
                    tr("新建文件"),
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
                    tr("新建目录"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.open_file_new(path.clone(), true, cx)
                    },
                ))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-dir-explorer",
                    "folder-input",
                    tr("在资源管理器中打开"),
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
                    tr("复制路径"),
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
                    tr("@到会话输入框"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| c.tree_at_insert(path.clone(), true, cx)
                    },
                ));
            // 根目录（操作栏名字区右键）不出现 改名/删除——项目根本身不是
            // 可操作对象（改名会拖走整个工作区、删除等于删项目）
            if !same_path(&path, &chat.cwd) {
                card = card
                    .child(menu_sep())
                    .child(tree_item(
                        &weak,
                        "ftm-dir-rename",
                        "pencil",
                        tr("改名"),
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
                        tr("删除"),
                        true,
                        {
                            let path = path.clone();
                            move |c, cx| c.confirm_tree_delete(path.clone(), cx)
                        },
                    ));
            }
        }
        crate::TreeMenu::File { path, .. } => {
            // 新建对齐目录菜单语义：落点 = 文件所在目录（parent 取不到时
            // 兜底项目根）
            card = card
                .child(tree_item(
                    &weak,
                    "ftm-file-new-file",
                    "file-plus",
                    tr("新建文件"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| {
                            let dir = path
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| c.cwd.clone());
                            c.open_file_new(dir, false, cx)
                        }
                    },
                ))
                .child(tree_item(
                    &weak,
                    "ftm-file-new-dir",
                    "folder-plus",
                    tr("新建目录"),
                    false,
                    {
                        let path = path.clone();
                        move |c, cx| {
                            let dir = path
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| c.cwd.clone());
                            c.open_file_new(dir, true, cx)
                        }
                    },
                ))
                .child(menu_sep())
                .child(tree_item(
                    &weak,
                    "ftm-file-explorer",
                    "folder-input",
                    tr("在资源管理器中打开"),
                    false,
                    {
                        let path = path.clone();
                        move |_c, _cx| crate::services::shell_open::reveal_in_explorer(&path)
                    },
                ))
                .child(tree_terminal_item(&weak, "ftm-file-term", &path));
            // 「在浏览器中打开」只对 html 文件出现（设计稿 021）
            let is_html = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "html" | "htm"));
            if is_html {
                card = card.child(tree_item(
                    &weak,
                    "ftm-file-browser",
                    "globe",
                    tr("在浏览器中打开"),
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
                    tr("复制路径"),
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
                    tr("@到会话输入框"),
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
                    tr("改名"),
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
                    tr("删除"),
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

/// 编辑区右键菜单（2026-10-11 用户定案）：编辑块（复制/粘贴/剪切/全选）
/// + 文件动作块（浏览器 html/预览 md/资源管理器/终端 + 改名/删除），
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
    let (path, x, y) = chat.editor_menu.clone().expect("checked by caller");
    let edges = PopupEdges::around((x, y), vp, 0., 12.);
    let weak_layer = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dialog_focus), move |_w, cx| {
        let _ = weak_layer.update(cx, |c, cx| {
            c.editor_menu = None;
            cx.notify();
        });
    });
    let ed = chat
        .file_cache
        .get(&path)
        .and_then(|f| f.editor.clone());
    let mut card = menu_card(edges, t);
    card = card
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-copy",
            "copy",
            tr("复制"),
            || Box::new(gpui_component::input::Copy),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-paste",
            "clipboard",
            tr("粘贴"),
            || Box::new(gpui_component::input::Paste),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-cut",
            "scissors",
            tr("剪切"),
            || Box::new(gpui_component::input::Cut),
        ))
        .child(editor_edit_item(
            &weak,
            &ed,
            "edm-select-all",
            "check",
            tr("全选"),
            || Box::new(gpui_component::input::SelectAll),
        ))
        .child(menu_sep());
    // 「在浏览器中打开」只对 html（同文件树菜单 021）
    let is_html = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "html" | "htm"));
    if is_html {
        card = card.child(editor_item(
            &weak,
            "edm-browser",
            "globe",
            tr("在浏览器中打开"),
            false,
            {
                let path = path.clone();
                move |_c, _cx| crate::services::shell_open::open_in_browser(&path)
            },
        ));
    }
    // 「预览」只对 md 且当前源码态（eye 同一开关：切回渲染态）
    let md_source = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
        && chat
            .file_cache
            .get(&path)
            .is_some_and(|f| f.md_source);
    if md_source {
        card = card.child(editor_item(
            &weak,
            "edm-preview",
            "eye",
            tr("预览"),
            false,
            {
                let path = path.clone();
                move |c, cx| {
                    if let Some(ft) = c.file_cache.get_mut(&path) {
                        ft.md_source = false;
                    }
                    cx.notify();
                }
            },
        ));
    }
    card = card
        .child(editor_item(
            &weak,
            "edm-explorer",
            "folder-input",
            tr("在资源管理器中打开"),
            false,
            {
                let path = path.clone();
                move |_c, _cx| crate::services::shell_open::reveal_in_explorer(&path)
            },
        ))
        .child(editor_terminal_item(&weak, "edm-term", &path))
        .child(menu_sep())
        .child(editor_item(
            &weak,
            "edm-rename",
            "pencil",
            tr("改名"),
            false,
            {
                let path = path.clone();
                move |c, cx| c.open_file_rename(path.clone(), false, true, cx)
            },
        ))
        .child(editor_item(
            &weak,
            "edm-del",
            "trash",
            tr("删除"),
            true,
            {
                let path = path.clone();
                move |c, cx| c.confirm_tree_delete(path.clone(), cx)
            },
        ));
    layer.child(card).into_any_element()
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
                c.editor_menu = None;
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
                c.editor_menu = None;
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
        tr("在终端中打开"),
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
            c.editor_menu = None;
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
                c.tree_menu = None;
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
        tr("在终端中打开"),
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
            c.tree_menu = None;
            cx.notify();
        });
    })
}

/// 菜单分组分隔线（§5.6：1px t.border）。
fn menu_sep() -> gpui::Div {
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
    let msg = chat.tree_del_error.clone().expect("checked by caller");
    let weak_dismiss = weak.clone();
    let weak_ok = weak.clone();
    let layer = crate::ui::overlay::layer(false, Some(&chat.dialog_focus), move |_w, cx| {
        let _ = weak_dismiss.update(cx, |c, cx| {
            c.tree_del_error = None;
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
                    .child(crate::i18n::tf("删除失败：{msg}", &[("msg", msg)])),
            )
            .child(div().flex().justify_end().child(crate::ui::button(
                "del-err-ok",
                crate::i18n::tr("知道了"),
                crate::ui::BtnSize::Md,
                crate::ui::BtnVariant::Secondary,
                false,
                move |_, _, cx| {
                    let _ = weak_ok.update(cx, |c, cx| {
                        c.tree_del_error = None;
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
            "删除 {name}？{n} 个打开文件的未保存更改将丢失（移入回收站）",
            &[("name", name), ("n", dirty_count.to_string())],
        )
    } else {
        crate::i18n::tf("删除 {name}？（移入回收站）", &[("name", name)])
    };
    crate::ui::overlay::confirm(
        focus,
        message,
        move |_w, cx| {
            let _ = weak_cancel.update(cx, |c, cx| {
                c.tree_confirm_del = None;
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
