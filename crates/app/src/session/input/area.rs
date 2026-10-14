//! composer 主输入区：input_area 键路由与装配、外层 wrap、惰性建
//! ComposerInput 实体（ensure_composer）。


use std::rc::Rc;

use gpui::{Context, KeyDownEvent, MouseButton, MouseDownEvent, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::ComposerInput;
use crate::MenuKind;
use crate::{ComposerDown, ComposerTab, ComposerUp, ComposerPaste};
use crate::i18n::tr;
use crate::appearance::theme::theme as T;
use crate::ui::icon;
use super::*;

/// `hero` = 012 新会话页模式：胶囊走正常流（由新会话页内容簇摆位），
/// 不再 0 高 + 绝对定位贴聊天区底。
pub(crate) fn input_area(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    streaming: bool,
    input_focused: bool,
    hero: bool,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let can_queue = !chat.composer.input.is_empty() || !chat.composer.pending_images.is_empty();
    // 新会话默认值（pi selectInitialModelScope parity：settings 默认模型
    // 在 scope 内 → 用它，否则 scope 首个；思考档 pin > per-model > 全局）
    // ——草稿态（无 state）的 pill 显示就落在它身上，进程起来后由 pi 的
    // get_state 接管。只在 fallback 真会被消费时才算：它每帧全量扫模型
    // 目录（逐模型 format! 拼键 + 排序键），挂会话后模型/思考都有 state
    // 答案，扫描是纯浪费（打字每键一帧，最忌每帧 O(目录)）。
    let (model_label, thinking_label, tools_label, ctx_pct, compacting) = {
        let r = chat.rt().read(cx);
        let state_model = r.state.as_ref().and_then(|s| s.model_label());
        let state_thinking = r.state.as_ref().and_then(|s| s.thinking_level.clone());
        let need_draft = (state_model.is_none() && r.pending_model.is_none())
            || (state_thinking.is_none() && r.thinking_override.is_none());
        let (draft_model, draft_thinking) = if need_draft {
            chat.new_session_default()
        } else {
            (None, None)
        };
        let model_label = state_model
            .or_else(|| {
                r.pending_model
                    .as_ref()
                    .map(|(p, id)| chat.model_display_name(p, id))
            })
            .or_else(|| {
                draft_model
                    .as_ref()
                    .map(|(p, id)| chat.model_display_name(p, id))
            })
            .unwrap_or_else(|| tr("model.select_title").to_string());
        let thinking_label = state_thinking
            .or_else(|| r.thinking_override.clone())
            .or_else(|| draft_thinking.clone())
            .unwrap_or_else(|| "auto".to_string());
        (
            model_label,
            thinking_label,
            r.tool_preset_label(),
            r.stats.as_ref().and_then(|s| s.context_percent),
            r.compacting,
        )
    };

    // 输入组件：惰性创建 + 每帧同步占位/值（set_value 同值跳过）
    let composer = ensure_composer(chat, weak, cx);
    // `!` shell 模式（031 pi-web bashMode parity）：去前导空白后以 ! 开头且
    // 无图片附着——边框着色 + 提示行（!! = 输出仅本地）
    let bash_mode = chat.composer.pending_images.is_empty() && chat.composer.input.trim_start().starts_with('!');
    // @ 菜单可能打开：预热/续期文件索引（TTL 内幂等，后台构建）
    if chat.active_menu(cx) == Some(MenuKind::At) {
        chat.ensure_at_index(cx);
    }
    let ph: SharedString = if compacting {
        // 压缩期间输入锁死（下方 on_change/on_submit 丢弃变更），用
        // placeholder 文案告知用户系统在做什么
        tr("composer.compacting_wait2").into()
    } else if streaming {
        tr("composer.hint_esc").into()
    } else {
        tr("composer.hint_shortcuts").into()
    };
    composer.update(cx, |f, _| f.set_placeholder(Some(ph)));
    // 压缩锁：编辑器组件整块不挂载（无焦点/IME/粘贴），显示 placeholder
    // 样式的压缩提示；草稿保留在 chat.composer.input，解锁即恢复
    composer.update(cx, |f, fcx| f.set_read_only(compacting, fcx));
    // 镜像只传借用：长草稿（粘贴的大段日志）下每键一帧的全量 String 拷贝
    // 就是打字延迟
    composer.update(cx, |f, fcx| f.set_value(&chat.composer.input, fcx));
    // 命令名同步给组件：仅 '/' 前缀输入时需要（chip 派生只消费 '/' 前缀），
    // 普通打字每键不再克隆整张命令表；会话进程答案优先，否则启动装载的
    //（项目 skill + 扩展缓存）
    if chat.composer.input.starts_with('/') {
        let names: Vec<String> = chat.slash_commands(cx).iter().map(|c| c.name.clone()).collect();
        composer.update(cx, |f, fcx| f.set_command_names(names, fcx));
    }

    // ---- 胶囊 ----
    let mut capsule = div()
        .id("composer")
        .w_full() // 尺寸由外层锚点统一定（消息列宽 / max 920），/ 菜单与胶囊同宽
        .rounded(px(16.))
        .border_1()
        // 边框不随 agent 运行变色（pi-web 的 streaming 琥珀色已去掉）：
        // 焦点态用 accent；`!` shell 模式用工具绿（pi-web tool-bg parity）
        .border_color(if bash_mode {
            gpui::rgba(crate::appearance::theme::rgba_a(0x22c55e, 0.4))
        } else if input_focused {
            rgb(t.accent)
        } else {
            rgb(t.border)
        })
        .bg(rgb(t.bg))
        .shadow_lg()
        // 鼠标透传修复（psp_overlays 同款方案）：胶囊浮在聊天区上，指针处
        // 双层 hitbox 都命中，事件漏进底下的消息列表（悬停/滚轮/点击穿
        // 透）。occlude 让胶囊自身 bounds 遮挡先绘制的 hitbox；控件行子
        // 元素后绘制不受影响（occlusion 只作用于更早的 hitbox）。
        .occlude()
        .flex()
        .flex_col()
        .pt(px(10.))
        .pb(px(12.)); // 操作栏到下边框的距离
    // 附加图片缩略图（v58 图片粘贴）：56×56 圆角方块，panel 左上角并列、
    // 间隔 5px；右上角悬浮 16px 圆形 X 点击删除（pi-web ImagePreview 布局
    // parity：X 偏移 -4,-4 半嵌在缩略图角上）
    if !chat.composer.pending_images.is_empty() {
        let thumbs: Vec<gpui::AnyElement> = chat
            .composer.pending_images
            .iter()
            .enumerate()
            .map(|(i, img)| {
                let weak_i = weak.clone();
                let weak_open = weak.clone();
                let thumb = img.thumb.clone();
                let thumb_for_open = thumb.clone();
                div()
                    .id(SharedString::from(format!("img-thumb-{i}")))
                    .relative()
                    .size(px(56.))
                    .flex_shrink_0()
                    .child(
                        // 点击看大图（X 是 sibling 不在祖先链，两 handler 互不误触）
                        div()
                            .id(SharedString::from(format!("img-view-{i}")))
                            .size_full()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg_panel))
                            .overflow_hidden()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let Some(image) = thumb_for_open.clone() else { return; };
                                let _ = weak_open.update(cx, |c, cx| {
                                    c.dlg.dialog = Some(crate::Dialog::ImagePreview { image });
                                    cx.notify();
                                });
                            })
                            .child(match thumb {
                                Some(image) => gpui::img(image)
                                    .size_full()
                                    .object_fit(gpui::ObjectFit::Cover)
                                    .into_any_element(),
                                None => div().into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("img-del-{i}")))
                            .absolute()
                            .top(px(-4.))
                            .right(px(-4.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(rgb(t.bg_panel))
                            .border_1()
                            .border_color(rgb(t.border))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                // X 与图片盒的 hitbox 都含点击点（X 叠在其角
                                // 上），且二者非祖先——不拦截会继续派发到图
                                // 片盒的"开大图"handler（实测删除前弹预览）
                                cx.stop_propagation();
                                let _ = weak_i.update(cx, |c, cx| {
                                    if i < c.composer.pending_images.len() {
                                        c.composer.pending_images.remove(i);
                                    }
                                    cx.notify();
                                });
                            })
                            .child(icon("x", 12., t.text_muted)),
                    )
                    .into_any_element()
            })
            .collect();
        capsule = capsule.child(
            div()
                .w_full()
                .pt(px(4.))
                .px(px(12.))
                .flex()
                .flex_wrap()
                .gap(px(5.))
                .children(thumbs),
        );
    }
    // 编辑区：真输入组件（多行 AutoGrow，超出 max 出纵向滚动条）；右键
    // 弹输入菜单（031，事件从组件冒泡上来——vendored 组件右键只做
    // 「无选区时移动光标」，不吞事件）。压缩锁期间整块是提示占位、无
    // 可编辑对象，不弹
    let weak_menu = weak.clone();
    capsule = capsule.child(
        div()
            .id("composer-editor")
            .w_full()
            .min_h(px(60.))
            .pt(px(2.))
            .pb(px(2.))
            .px(px(6.))
            .on_mouse_down(MouseButton::Right, move |ev: &MouseDownEvent, _, cx| {
                if compacting {
                    return;
                }
                cx.stop_propagation();
                let _ = weak_menu.update(cx, |c, cx| {
                    c.dlg.input_menu =
                        Some((f32::from(ev.position.x), f32::from(ev.position.y)));
                    cx.notify();
                });
            })
            .child(composer),
    );
    capsule = wire_composer_keys(capsule, cx);
    // 控件行
    // 锁定期间强制收起胶囊菜单（工具预设/思考），否则菜单会浮在
    // 锁定的控件上还能点
    if compacting && chat.dlg.pill_menu.is_some() {
        chat.dlg.pill_menu = None;
    }
    capsule = capsule.child(composer_bar(
        chat,
        streaming,
        can_queue,
        compacting,
        &model_label,
        &thinking_label,
        &tools_label,
        ctx_pct,
        t,
        cx,
    ));

    // 012 新会话页（hero）胶囊走正常流，由新会话页的内容簇摆位；会话界面
    // 则 0 高 wrapper + 胶囊绝对定位悬浮（聊天消息从胶囊后滚过）。/ 菜单两种
    // 模式同构：挂在测量元素正上方（pi-web：bottom 100% + 8px 间隙）
    // 斜杠与 @ 菜单共用 slash_menu_view（视图内部按 kind 分支渲染）
    let slash_open = chat.active_menu(cx).is_some();
    // 「回到最新」按钮：不贴底且有消息时，悬浮在输入面板顶部上方 20px
    let show_scroll_btn = !chat.rt().read(cx).pager.is_at_bottom()
        && !chat.rt().read(cx).messages.is_empty();
    // 胶囊高度测量槽（导航刻度条 033 居中用）；先克隆引用再拼元素，
    // 避免 &chat.composer.composer_h 与下方 slash_menu_view(&mut chat) 借用冲突
    let composer_h_slot = chat.composer.composer_h.clone();
    let inner = div()
        .relative()
        // 必须撑满父宽：为 `w_full`+`max_w(920)` 的测量元素提供宽度基准，
        // 否则 shrink-to-fit 会把胶囊挤成窄条（会话界面实测踩坑）
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .when(slash_open, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(gpui::relative(1.))
                    .pb(px(8.))
                    // 与胶囊同宽同位：inner 会话界面下是全宽（仅 px(15) 内
                    // 边距），直接铺满会比 920 封顶的胶囊宽一截；居中约束
                    // 与测量元素（同款 max_w 920）对齐后左右边缘贴合胶囊
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(920.))
                            .child(crate::slash_menu_view(chat, weak, t, cx)),
                    ),
            )
        })
        .child(
            // 测量元素只包胶囊本体（不含回底按钮，按钮显隐会
            // 引入 ±52px 抖动）；/ 菜单是 absolute 不占布局
            crate::ui::measure_height(
                "composer-h",
                &composer_h_slot,
                div()
                    .relative()
                    // 外层（会话界面 px(15) / 新会话页内容簇）已扣除内边距，
                    // 此处宽度 == 消息列宽，再以 920 封顶（pi-web 单一内容列宽）
                    .w_full()
                    .max_w(px(920.))
                    .child(capsule),
            ),
        );
    composer_wrap(inner, hero, show_scroll_btn, weak, t)
}

/// 按键路由：组件处理的键（Enter/方向键/粘贴/undo…）到不了这里；这里只
/// 接组件无绑定的 Shift+Enter、已传播的 Escape，以及覆盖绑定截获的
/// ↑/↓/Tab 动作。（自 input_area 拆出，函数行数守卫）
fn wire_composer_keys(
    capsule: gpui::Stateful<gpui::Div>,
    cx: &mut Context<Chat>,
) -> gpui::Stateful<gpui::Div> {
    capsule
        .on_key_down(cx.listener(
            |this, ev: &KeyDownEvent, window, cx| {
                let key = ev.keystroke.key.as_str();                if key == "enter" && ev.keystroke.modifiers.shift {
                    // IME 组合期平台不派发按键（gpui windows events.rs），
                    // 无需组合守卫
                    let streaming = this.rt().read(cx).agent_running;
                    let can_queue =
                        !this.composer.input.is_empty() || !this.composer.pending_images.is_empty();
                    if streaming {
                        if can_queue {
                            this.follow_up_input(cx);
                        }
                    } else if let Some(c) = &this.composer.composer {
                        c.update(cx, |f, fcx| f.insert_newline(window, fcx));
                    }
                    cx.stop_propagation();
                    return;
                }
                if key == "escape" {
                    // 走到这说明组件的 escape 已处理（解除 IME 标记且未清
                    // 草稿）并传播；这里做 app 层语义：菜单取消 / 中止
                    let menu = this.active_menu(cx);
                    if let Some(kind) = menu {
                        if kind == MenuKind::Slash && !this.composer.input.is_empty() {
                            let v = format!("{} ", this.composer.input);
                            this.set_input(v, cx);
                        }
                        // @ 菜单 Esc = 只收起（pi-web parity：token 原样保留，
                        // 下一次输入变化重新打开）
                        this.composer.menu_dismissed = true;
                        this.composer.menu_ix = 0;
                        cx.notify();
                    } else {
                        // `!` 执行中优先中止 bash（rpc abort_bash），其次模型流
                        let bash_running = this.rt().read(cx).bash_running;
                        let streaming = this.rt().read(cx).agent_running;
                        if bash_running {
                            this.abort_bash(cx);
                        } else if streaming {
                            this.abort_stream(cx);
                        }
                    }
                    cx.stop_propagation();
                }
            },
        ))
        .on_action(cx.listener(|this, _: &ComposerUp, window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                this.composer.menu_ix = this.composer.menu_ix.saturating_sub(1);
                this.composer.menu_scroll.scroll_to_item(this.composer.menu_ix);
                cx.notify();
            } else if this.composer.input.is_empty() && !this.composer.history.is_empty() {
                let ix = match this.composer.history_ix {
                    None => this.composer.history.len() - 1,
                    Some(i) => i.saturating_sub(1),
                };
                this.composer.history_ix = Some(ix);
                let v = this.composer.history[ix].clone();
                this.set_input(v, cx);
            } else {
                // 非空多行：上移一行交还组件
                window.dispatch_action(Box::new(gpui_component::input::MoveUp), cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerDown, window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                this.composer.menu_ix = (this.composer.menu_ix + 1).min(items.len() - 1);
                this.composer.menu_scroll.scroll_to_item(this.composer.menu_ix);
                cx.notify();
            } else if this.composer.input.is_empty() {
                if let Some(i) = this.composer.history_ix {
                    if i + 1 < this.composer.history.len() {
                        this.composer.history_ix = Some(i + 1);
                        let v = this.composer.history[i + 1].clone();
                        this.set_input(v, cx);
                    } else {
                        this.composer.history_ix = None;
                        this.set_input(String::new(), cx);
                    }
                }
            } else {
                window.dispatch_action(Box::new(gpui_component::input::MoveDown), cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerTab, _window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                let ix = this.composer.menu_ix.min(items.len() - 1);
                let insert = items[ix].insert.clone();
                this.accept_menu(insert, cx);
            }
        }))
        // Ctrl+V 截获（bind_keys 覆盖组件粘贴绑定）：剪贴板含图 → 附件化；
        // 否则重派发组件 Paste 走原文本粘贴
        .on_action(cx.listener(|this, _: &ComposerPaste, window, cx| {
            if !this.attach_clipboard_image(cx) {
                window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
            }
        }))
}

/// composer 定位包装：会话界面（`hero=false`）= 0 高 wrapper + 胶囊绝对定位
/// 悬浮在聊天区底部之上（聊天消息从胶囊后滚过）；012 新会话页（`hero=true`）
/// = 正常流，由新会话页的内容簇摆位（012 的「下移 10%」）。
fn composer_wrap(
    inner: gpui::Div,
    hero: bool,
    show_scroll_btn: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    if hero {
        return div()
            .id("composer-wrap")
            .relative()
            .w_full()
            .flex_shrink_0()
            .child(inner)
            .into_any_element();
    }
    div()
        .id("composer-wrap")
        .relative()
        .h(px(0.))
        .flex_shrink_0()
        .child(
            div()
                .absolute()
                .bottom(px(20.))
                .left_0()
                .right_0()
                // 与消息列表同一水平内边距（session_list 的 px(15)）→ 胶囊列宽
                // 恒等于消息列宽，窗口缩放时同步收缩，不再靠百分比猜
                .px(px(15.))
                .flex()
                .flex_col()
                .items_center()
                // 按钮与胶囊间距 20px：随输入框增高自动上移，永不叠进面板
                .gap(px(20.))
                .when(show_scroll_btn, |d| {
                    d.child(crate::session::scroll_to_bottom_button(weak, t))
                })
                .child(inner),
        )
        .into_any_element()
}

/// 惰性创建 composer 输入组件（InputState 构造需要 &mut Window，挂在渲染
/// 期；回调经 weak 回写 Chat，不改持有结构）。
fn ensure_composer(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut Context<Chat>,
) -> gpui::Entity<ComposerInput> {
    if let Some(c) = &chat.composer.composer {
        return c.clone();
    }
    let weak_change = weak.clone();
    let weak_submit = weak.clone();
    let weak_chip = weak.clone();
    let c = cx.new(|icx| {
        let mut f = ComposerInput::new(icx);
        f.set_on_change(Rc::new(move |v, cx| {
            let _ = weak_change.update(cx, |chat, cx| {
                if chat.composer.input != v {
                    // enter() 污染签名（原文尾追 \n）：组件 Enter 先插换行
                    // 触发本次 Change，随即 PressEnter → on_submit 用剥 \n 的
                    // 值覆盖回来。此间过滤集未变，不能复位 menu_ix——否则
                    // ↑/↓/悬停选中的条目在 on_submit 读到的恒是 0，Enter 永远
                    // 接受第 0 项（@ 提及表现为「回车插的还是裸查询文本」）
                    let enter_pollution =
                        v.ends_with('\n') && &v[..v.len() - 1] == chat.composer.input;
                    chat.composer.input = v.to_string();
                    if !enter_pollution {
                        // 输入再次变化 = 解除点外/Esc 收起（active_menu 门控的
                        // 声明语义；打字不走 set_input，必须在此复位）
                        chat.composer.menu_dismissed = false;
                        chat.composer.menu_ix = 0;
                        // 过滤集变了回顶部，避免新列表停留在旧滚动深处
                        chat.composer.menu_scroll
                            .set_offset(gpui::point(px(0.), px(0.)));
                    }
                    cx.notify();
                }
            });
        }));
        f.set_on_submit(Rc::new(move |v, cx| {
            let _ = weak_submit.update(cx, |chat, cx| {
                if chat.rt().read(cx).compacting {
                    return;
                }
                // 先落到剥离换行后的 v 再判菜单：组件 enter() 会先插一个
                // 换行并经 Change 污染 chat.composer.input（含空白使菜单判定失败，
                // 回车被误当发送）
                chat.composer.input = v.to_string();
                // chip 激活（命令已确认成胶囊）时 Enter=发送——裸 "/命令"
                // 的输入形态与菜单过滤态相同，必须以 chip 态区分，否则
                // Enter 会走"接受补全"永远发不出去
                let chip = chat
                    .composer.composer
                    .as_ref()
                    .map(|c| c.read(cx).chip_active())
                    .unwrap_or(false);
                let items = chat.menu_items(cx);
                if !chip && chat.active_menu(cx).is_some() && !items.is_empty() {
                    let ix = chat.composer.menu_ix.min(items.len() - 1);
                    let insert = items[ix].insert.clone();
                    chat.accept_menu(insert, cx);
                    return;
                }
                let can_queue = !chat.composer.input.is_empty() || !chat.composer.pending_images.is_empty();
                let streaming = chat.rt().read(cx).agent_running;
                if streaming {
                    if can_queue {
                        chat.steer_input(cx);
                    }
                } else if can_queue {
                    chat.send_input(cx);
                } else {
                    // 空输入回车：不发送，顺手清掉组件自插的换行
                    chat.set_input(String::new(), cx);
                }
            });
        }));
        f.set_on_chip_backspace(Rc::new(move |cx| {
            let _ = weak_chip.update(cx, |chat, cx| {
                chat.set_input(String::new(), cx);
            });
        }));
        f
    });
    chat.composer.composer = Some(c.clone());
    c
}
