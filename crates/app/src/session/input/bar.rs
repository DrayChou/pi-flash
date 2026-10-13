//! composer 控件行：左 = 图片/工具预设，右 = 用量环/模型/思考/发送停止；
//! 附插件与 MCP 两个胶囊按钮。BAR_FONT 单点在此。



use gpui::{Context, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::PillMenu;
use crate::i18n::tr;
use crate::ui::{icon, icon_hover};
use super::*;

/// 操作栏字体大小（工具预设/模型/思考统一；单点改这里）。
/// inputpanel 操作栏文字 = 面板设置值（字体大小设置.md §1）
const BAR_FONT: f32 = 12.;

/// 悬停标志更新 + 淡出状态机（乱序免疫）：只按 (was_open, open) 转移，

/// 扩展按钮（040 定稿 + 2026-10 改版）：工具胶囊右边，**仅「full+」档激活**。
/// 点击开合扩展菜单（勾选即生效，无确认按钮）；非 full+ 档置灰不可点。
fn plugin_button(
    chat: &mut Chat,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let ui = crate::appearance::ui_size;
    // 激活条件只有一个：**当前档是「full+」**（内部键 custom）。运行中也能
    // 点开勾选，清单在下一轮发送前重绑生效（pending_ext_sources 链路）。
    let (is_custom, n, open) = {
        let r = chat.rt().read(cx);
        (
            r.tool_preset_key() == "custom",
            r.ext_sources.len(),
            chat.plugin_picker.is_some(),
        )
    };
    let active = is_custom;
    let label = if n > 0 {
        format!("{}({n})", crate::i18n::tr("settings.tab.extensions"))
    } else {
        crate::i18n::tr("settings.tab.extensions").to_string()
    };
    let color = if !active {
        t.text_faint
    } else if open {
        t.accent
    } else {
        t.text_muted
    };
    let mut el = div()
        .id("plugins-menu")
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(5.))
        .rounded(px(8.))
        .text_size(ui(BAR_FONT))
        .text_color(rgb(color))
        .child(icon_hover("plug", 13., color))
        .child(SharedString::from(label));
    if active {
        el = el
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _w, cx| {
                    this.toggle_plugin_menu_at(event.position, cx);
                }),
            );
    }
    // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
    el = el.child(crate::PillBtns::tracker(&chat.pill_btn.ext));
    el.into_any_element()
}

/// MCP 按钮（043 定稿）：扩展按钮右边的兄弟节点。**不设档位守卫**——
/// 扩展只在 full+ 档挂载，MCP 各档都装载；全局 mcp.json 无条目时整个
/// 不渲染。点击开合 MCP 勾选菜单（勾选即生效，写盘 + 运行中标记重绑）。
fn mcp_button(
    chat: &mut Chat,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let ui = crate::appearance::ui_size;
    let has_global = chat
        .mcp_servers
        .iter()
        .any(|s| s.scope == pi_link::mcp::Scope::Global);
    if !has_global {
        return div().into_any_element();
    }
    let (n, open) = (
        chat.mcp_servers
            .iter()
            .filter(|s| s.scope == pi_link::mcp::Scope::Global && s.enabled)
            .count(),
        chat.mcp_picker.is_some(),
    );
    let label = if n > 0 {
        format!("{}({n})", crate::i18n::tr("dock.mcp"))
    } else {
        crate::i18n::tr("dock.mcp").to_string()
    };
    let color = if open { t.accent } else { t.text_muted };
    let mut el = div()
        .id("mcp-menu")
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(5.))
        .rounded(px(8.))
        .text_size(ui(BAR_FONT))
        .text_color(rgb(color))
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &gpui::MouseDownEvent, _w, cx| {
                this.toggle_mcp_menu_at(event.position, cx);
            }),
        )
        .child(icon_hover("server", 13., color))
        .child(SharedString::from(label));
    // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
    el = el.child(crate::PillBtns::tracker(&chat.pill_btn.mcp));
    el.into_any_element()
}

/// 控件行：左 = 图片 + 工具预设；右 = 上下文环 + 模型 + 思考 + 发送/停止。
#[allow(clippy::too_many_arguments)]
pub(super) fn composer_bar(
    chat: &mut Chat,
    streaming: bool,
    can_queue: bool,
    // 压缩锁：整行控件禁用（图片/工具预设/模型/思考/发送）
    locked: bool,
    model_label: &str,
    thinking_label: &str,
    tools_label: &str,
    ctx_pct: Option<f64>,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    // `!` bash 执行中与模型流共用停止按钮形态（031）
    let bash_running = chat.rt().read(cx).bash_running;
    let running = streaming || bash_running;
    // `!` shell 模式（与 input_area 边框同判定）：左组控件整组换成提示
    let bash_mode = chat.pending_images.is_empty() && chat.input.trim_start().starts_with('!');
    let bash_excluded = chat.input.trim_start().starts_with("!!");
    let thinking_open = chat.pill_menu == Some(PillMenu::Thinking);
    let tools_open = chat.pill_menu == Some(PillMenu::Tools);
    // 工具预设 = spawn 参数（--tools/--no-tools），换它要重绑会话进程 +
    // 整表重读：运行中换会把当前这一轮掐掉、并让「等待模型响应」掉回屏底。
    // 所以 agent 跑动期间置灰禁止（思考强度/模型都是实时 RPC，不动消息表，照旧可用）。
    let tools_locked = locked || streaming;
    let mut bar = div()
        .flex()
        .items_center()
        .px(px(10.))
        .child(bar_left_group(
            chat,
            locked,
            tools_locked,
            tools_open,
            tools_label,
            bash_mode,
            bash_excluded,
            t,
            cx,
        ));
    // 右侧：环 + 模型 + 思考 + 发送，按钮间距统一 10px；操作栏左右
    // padding 10px = 发送钮距胶囊边框 10px
    let ctx_tip = ctx_tip_element(chat, t, cx);
    let mut right = div().ml_auto().flex().items_center().gap(px(10.)).child(
        // 上下文用量环：track 在下、实际比例的主题色弧在上（弧 SVG 按百分比
        // 运行时生成，icons/ring-p{1-99}；100% 走静态 ring-100）。悬浮显示
        // 用量详情，挪开淡出（ctx_tip_* 状态机）
        div()
            .id("ctx-ring")
            .relative()
            .p(px(4.))
            .m(px(-4.)) // 命中区扩到 26px，视觉位置不变
            .cursor_pointer()
            .rounded_full()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_hover(cx.listener(|this, h: &bool, _w, cx| ctx_tip_hover(this, true, *h, cx)))
            .child(
                // 同心双环：track 在下、进度弧在上（svg 是 flex 行内子元素
                // 会并排——必须各自绝对定位铺满后居中才叠成同心）
                div()
                    .relative()
                    .size(px(18.))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::ui::icon("ring-track", 18., t.bg_selected)),
                    )
                    .children(ctx_pct.map(|p| {
                        let path = if p >= 100. {
                            "icons/ring-100.svg".to_string()
                        } else {
                            format!("icons/ring-p{}.svg", p.round().clamp(1., 99.) as u8)
                        };
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui::svg()
                                    .path(SharedString::from(path))
                                    .text_color(rgb(t.accent))
                                    .size(px(18.)),
                            )
                    }))
                    // 悬浮详情浮层（absolute 越界，浮层随 hover/淡出态挂载）
                    .children(ctx_tip),
            ),
    );
    // 模型 ∨
    right = right.child(
        div()
            .id("open-model-select")
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(8.))
            .text_size(crate::appearance::ui_size(BAR_FONT))
            .text_color(rgb(if locked { t.text_faint } else { t.text_muted }))
            .when(!locked, |d| d.cursor_pointer())
            .hover(move |s| {
                if locked { s } else { s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)) }
            })
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, event: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    // 042：模型菜单锚定本按钮（原居中 ModelSelect 弹窗退役）；
                    // 目录缺失时 ensure_models_requested 借同 cwd 活进程补拉，
                    // 草稿无进程也照常弹出（列表来自 Chat 共享目录）
                    this.toggle_model_menu_at(event.position, cx);
                },
            ))
            .child(SharedString::from(model_label.to_string()))
            .child(icon("chevron-down", 10., if locked { t.text_faint } else { t.text_dim }))
            // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
            .child(crate::PillBtns::tracker(&chat.pill_btn.model)),
    );
    // 思考强度 ∨
    right = right.child(
        div()
            .id("thinking-menu")
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(8.))
            .text_size(crate::appearance::ui_size(BAR_FONT))
            .text_color(rgb(if locked {
                t.text_faint
            } else if thinking_open {
                t.accent
            } else {
                t.text_muted
            }))
            .when(!locked, |d| d.cursor_pointer())
            .hover(move |s| {
                if locked { s } else { s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)) }
            })
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, event: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    this.pill_anchor = Some(crate::PillBtns::anchor(&this.pill_btn.thinking, event.position));
                    this.pill_menu = match this.pill_menu {
                        Some(PillMenu::Thinking) => None,
                        _ => Some(PillMenu::Thinking),
                    };
                    cx.notify();
                },
            ))
            .child(icon_hover(
                "lightbulb",
                13.,
                if locked {
                    t.text_faint
                } else if thinking_open {
                    t.accent
                } else {
                    t.text_muted
                },
            ))
            .child(SharedString::from(thinking_label.to_string()))
            .child(icon("chevron-down", 10., if locked { t.text_faint } else { t.text_dim }))
            // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
            .child(crate::PillBtns::tracker(&chat.pill_btn.thinking)),
    );
    // 圆形发送 ↑（运行中变停止：主题色圆角方块+对比色停止块）；用户定位：
    // 左移 5px、上移 8px
    right = right.child(
        div()
            .id("send")
            .mr(px(2.)) // 右边距=栏padding10+2=12（与下边距对齐）
            .mt(px(-8.))
            .size(px(36.)) // 发送/停止共用直径（用户微调处）
            .rounded(if running { px(9.) } else { px(18.) })
            .flex()
            .items_center()
            .justify_center()
            .when(!locked, |d| d.cursor_pointer())
            .bg(rgb(if locked {
                // 压缩锁：恒灰，不给"可点"的暗示
                t.bg_selected
            } else if running {
                t.accent // 停止态外圈=主题色（用户定稿，非深色）
            } else if can_queue {
                t.accent
            } else {
                t.bg_selected
            }))
            .hover(move |s| if locked { s } else { s.opacity(0.9) })
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, _: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    // 与按钮渲染同源：agent_running（事件驱动），快照
                    // is_streaming 恒 false 会把"停止"点成"发送"；
                    // `!` bash 执行中点它 = abort_bash
                    let bash_running = this.rt().read(cx).bash_running;
                    if bash_running {
                        this.abort_bash(cx);
                    } else if this.rt().read(cx).agent_running {
                        this.abort_stream(cx);
                    } else {
                        this.send_input(cx);
                    }
                },
            ))
            .child(if running {
                // 停止方块
                div()
                    .size(px(12.))
                    .rounded(px(2.5))
                    .bg(rgb(t.accent_contrast))
                    .into_any_element()
            } else {
                icon("arrow-up", 15., t.accent_contrast)
            }),
    );
    bar = bar.child(right);
    bar
}

/// 左组：`!` shell 模式提示，或 图片 + 工具预设 + 扩展/MCP 胶囊。
/// （自 composer_bar 拆出，函数行数守卫。）
#[allow(clippy::too_many_arguments)]
fn bar_left_group(
    chat: &mut Chat,
    locked: bool,
    tools_locked: bool,
    tools_open: bool,
    tools_label: &str,
    bash_mode: bool,
    bash_excluded: bool,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    if bash_mode {
            // `!` shell 模式：左组换成模式提示（覆盖图片/工具/插件位，
            // 不额外占一行；右侧环/模型/思考/发送照旧）
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(8.))
                .h(px(28.))
                .child(icon("terminal", 13., t.text_dim))
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(BAR_FONT))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(if bash_excluded {
                            tr("composer.shell_local_only")
                        } else {
                            tr("composer.shell_to_model")
                        })),
                )
                .into_any_element()
        } else {
        // 左侧：图片 + 工具预设（内容裸宽，组内间距 10px，与右侧一致）
        div().flex().items_center().gap(px(10.)).child(
            // 图片
            div()
                .id("attach-image")
                .pl(px(8.)) // 图片与正文首行左对齐：正文=编辑行6+组件12=18，图片=栏10+8=18
                .flex()
                .items_center()
                .text_color(rgb(if locked { t.text_faint } else { t.text_muted }))
                .when(!locked, |d| d.cursor_pointer())
                .hover(move |s| {
                    if locked { s } else { s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)) }
                })
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                        if this.rt().read(cx).compacting {
                            return;
                        }
                        this.attach_images(cx);
                    },
                ))
                .child(icon_hover("image", 15., if locked { t.text_faint } else { t.text_muted })),
            )
            .child(
            // 工具预设「默认∨」
            div()
                .id("tools-menu")
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(5.))
                .rounded(px(8.))
                .text_size(crate::appearance::ui_size(BAR_FONT))
                .text_color(rgb(if tools_locked {
                    t.text_faint
                } else if tools_open {
                    t.accent
                } else {
                    t.text_muted
                }))
                .when(!tools_locked, |d| d.cursor_pointer())
                .hover(move |s| {
                    if tools_locked {
                        s
                    } else {
                        s.bg(rgb(t.bg_hover)).text_color(rgb(t.text))
                    }
                })
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, event: &gpui::MouseDownEvent, _w, cx| {
                        if this.rt().read(cx).compacting {
                            return;
                        }
                        if this.rt().read(cx).agent_running {
                            // 置灰之外再给一句说明：工具预设换不了是因为要重绑进程
                            this.set_status(
                                crate::i18n::tr("compaction.preset_locked").to_string(),
                                cx,
                            );
                            return;
                        }
                        this.pill_anchor = Some(crate::PillBtns::anchor(&this.pill_btn.tools, event.position));
                        this.pill_menu = match this.pill_menu {
                            Some(PillMenu::Tools) => None,
                            _ => Some(PillMenu::Tools),
                        };
                        cx.notify();
                    },
                ))
                .child(icon_hover("wrench", 13., if tools_open { t.accent } else { t.text_muted }))
                .child(SharedString::from(tools_label.to_string()))
                .child(icon("chevron-down", 10., t.text_dim))
                // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
                .child(crate::PillBtns::tracker(&chat.pill_btn.tools)),
            )
            // 扩展按钮（040 定稿）：**工具胶囊的兄弟节点**，不是子节点——
            // 塞进胶囊里点它会冒泡触发工具菜单（用户踩过）。仅「full+」档
            // 激活，点开扩展菜单（勾选即生效，单列已勾置顶）。
            .child(plugin_button(chat, t, cx))
            // MCP 按钮（043）：扩展按钮右边的兄弟节点（无条目时自隐藏）
            .child(mcp_button(chat, t, cx))
            .into_any_element()
    }
}

