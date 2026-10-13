//! Chat 渲染根（P3 自 main.rs 拆出）：impl Render —— 焦点仲裁、pill 弹层、
//! 三段布局装配与 overlay 叠放（设置→弹窗→确认→psp 悬浮→picker→toast→扩展）。

use crate::*;

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

// toast 不加阴影：早前的手工双层黑色 halo 贴片（因 gpui box_shadow 在
// DirectX 下过淡而自绘）在浅色主题下被用户判定为「一圈脏灰边」，已整体
// 移除——toast 只保留 bg_panel + border 的贴片本体。

impl Render for Chat {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 焦点快照（给「agent 结束时窗口没焦点就给点」用）：平台层在每次
        // 激活变化都会 window.refresh() 强制重绘，所以这里的值总是新鲜的
        self.window_active = window.is_window_active();
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if PERF.load(std::sync::atomic::Ordering::Relaxed)
            && !FIRST.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] first frame: {:?}", t0.elapsed());
            }
        }
        // [perf] 仪器：Chat 每秒实际渲染次数（PI_FLASH_PERF=1 开启）。idle
        // 应≈1（光标闪烁）；编辑/拖选时≈事件率；无事件却持续 >30/s = 存在
        // 自持重绘循环（v62 排查文本编辑卡顿的探针，留作常备仪器）。
        static RENDER_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        static RENDER_T: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
        static RENDER_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *RENDER_ON.get_or_init(|| std::env::var("PI_FLASH_PERF").is_ok()) {
            let n = RENDER_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let mut slot = RENDER_T.lock().unwrap();
            let report = match *slot {
                Some(t0) if t0.elapsed().as_secs_f32() >= 1.0 => {
                    Some(n as f32 / t0.elapsed().as_secs_f32())
                }
                Some(_) => None,
                None => {
                    *slot = Some(std::time::Instant::now());
                    None
                }
            };
            if let Some(rate) = report {
                eprintln!("[perf] chat renders/s: {rate:.0}");
                RENDER_N.store(0, std::sync::atomic::Ordering::Relaxed);
                *slot = Some(std::time::Instant::now());
            }
        }
        // 010-启动：揭幕前整幅启动页（黑底、居中 logo、非最大化）；揭幕帧
        // 再按 §4 默认最大化（0.2.2 建窗即 Maximized 不可靠，显式 zoom 是
        // 项目已验证路径——原建窗回调里的 zoom 后移到了这里）。
        if !self.booted {
            startup::mark_splash_painted();
            return startup::splash_view();
        }
        if self.pending_zoom {
            self.pending_zoom = false;
            window.zoom_window();
        }
        // keep terminal focus alive across frames (render focuses chat input
        // otherwise, which would steal it back every redraw)
        let dialog_input = match &self.dialog {
            Some(Dialog::ProviderPicker { input, .. }) | Some(Dialog::SessionSearch { input })
            | Some(Dialog::ProjectPicker { input, .. })
            | Some(Dialog::FileSearch { input, .. }) => {
                Some(input.clone())
            }
            _ => None,
        }
        .or_else(|| {
            // 042：模型菜单的过滤输入同享焦点保持（每帧不被 composer 抢回）；
            // 过滤框未渲染（<10 条）时不抢——菜单退化为纯鼠标操作
            self.model_picker
                .as_ref()
                .filter(|_| self.picker_filter_shown(cx))
                .map(|p| p.input.clone())
        });
        // 详情卡的原地改名输入框也要持有焦点——否则每帧的焦点回收
        // 会把它抢回主输入框，键盘输入进不去
        let card_rename_focus = self
            .hover_card
            .as_ref()
            .and_then(|c| c.rename_input.clone());
        let rename_focus = self.rename_input.clone().or(card_rename_focus);
        if let Some(input) = rename_focus.or(dialog_input) {
            let handle = input.read(cx).focus_handle_in(cx);
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.aux.ext_dialog.is_some() {
            let ext_text = matches!(
                self.aux.ext_dialog.as_ref().map(|r| &r.method),
                Some(
                    pi_link::protocol::ExtUiMethod::Input { .. }
                        | pi_link::protocol::ExtUiMethod::Editor { .. }
                )
            );
            let handle = if ext_text {
                self.aux.ext_input.read(cx).focus_handle()
            } else {
                self.dialog_focus.clone()
            };
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.dialog.is_some()
            // overlay::confirm 三槽（文件树删除/项目删除/删除失败）不占
            // Dialog 枚举，但同样要一打开就持有焦点——Enter 确认 / ESC 取消
            // 才能立即生效（zed window.prompt 同款，键盘可操作）
            || self.tree_confirm_del.is_some()
            || self.confirm_prj_del.is_some()
            || self.tree_del_error.is_some()
        {
            if !self.dialog_focus.is_focused(window) {
                window.focus(&self.dialog_focus);
            }
        } else if let Some(panel) = self.settings.as_ref() {
            // 焦点在面板任意输入框内就不抢（白名单在 v60/v70 两度漏新输入框，
            // 现由 SettingsPanel::focus_within 结构化自检）
            if !panel.read(cx).focus_within(window, cx)
                && !self.dialog_focus.is_focused(window)
            {
                window.focus(&self.dialog_focus);
            }
        } else if self.file_editor_focus(window, cx).is_some() {
            // 023：文件编辑器持有焦点时不回收——否则点进去下一帧就被抢回
            // composer，打字全失效（编辑器此前不在白名单）
        } else if !(self.content_view == crate::ContentView::Term
            && self
                .shown_terminal_focus()
                .is_some_and(|h| h.is_focused(window)))
        {
            // 终端持焦白名单只在终端视图内生效：切回 chat/文件后终端元素
            // 已卸载，但 gpui 的 focus handle 不会随卸载自动 blur，残留持焦
            // 会把每帧的焦点回收永久挡在 composer 之外（打字无处可去）
            //
            // 文件树行内编辑（Zed project_panel `filename_editor`）：编辑
            // 存续期它就是焦点所有者——Zed 里这条由面板列表 `track_focus`
            // 承担（编辑行是面板的一部分，焦点不会跑到别处），pi-flash 的
            // 面板不可聚焦，故在此把它排在 composer 之前。每帧只在「没被
            // 别人夺走」时补焦：抢在级联末级之前让出 composer 回收，编辑行
            // 不再被逐帧抢回——那正是「改名焦点捕捉不到」的根因（首发聚焦
            // 请求见 open_file_new / open_file_rename 的 focus_soon；用户点
            // 别处造成的真夺焦（对话框/终端/编辑器/设置）走前面各分支，本
            // 分支看不到，blur 收场照常触发）。
            match self.tree_edit.as_ref().filter(|_| self.dock_panel == DockPanel::Files) {
                Some(edit) if edit.input.read(cx).has_inner() => {
                    let handle = edit.input.read(cx).focus_handle_in(cx);
                    if !handle.is_focused(window) {
                        window.focus(&handle);
                    }
                }
                // 内件未就绪（打开当帧）：本帧让位，输入框渲染时由 focus_soon
                // 一次性首发（Zed focus_filename_editor 请求同位）
                Some(_) => {}
                None => {
                    // 终端视图兜底：焦点不在**当前显示**的终端上（关/切 tab
                    // 后旧终端元素已卸载但 handle 残留持焦，或焦点被丢到
                    // 根节点）→ 交回显示的那个终端；否则键入无处可去
                    // （双终端「打不进字」的根因）。非终端视图照旧回收给
                    // chat 根。
                    match self
                        .shown_terminal_focus()
                        .filter(|_| self.content_view == crate::ContentView::Term)
                    {
                        Some(h) if !h.is_focused(window) => window.focus(&h),
                        Some(_) => {}
                        None => window.focus(&self.focus_handle(cx)),
                    }
                }
            }
        }
        let t = T();

        let entity = cx.entity();
        let weak = entity.downgrade();
        let weak_for_dialog = weak.clone();

        // popup menu overlay anchored above the composer controls row
        let (thinking_override, preset_key) = {
            let rt = self.rt();
            let r = rt.read(cx);
            (r.thinking_override.clone(), r.tools_preset.clone())
        };
        let pill_menu_el = self.pill_menu.map(|menu| {
            let weak_menu = weak.clone();
            let rows: Vec<(String, String, bool)> = match menu {
                PillMenu::Thinking => [
                    ("auto", tr("thinking.use_pi_default"), thinking_override.is_none()),
                    ("off", tr("thinking.off"), thinking_override.as_deref() == Some("off")),
                    ("minimal", tr("thinking.minimal_reasoning"), thinking_override.as_deref() == Some("minimal")),
                    ("low", tr("thinking.low_reasoning"), thinking_override.as_deref() == Some("low")),
                    ("medium", tr("thinking.medium_reasoning"), thinking_override.as_deref() == Some("medium")),
                    ("high", tr("thinking.high_reasoning"), thinking_override.as_deref() == Some("high")),
                    ("xhigh", tr("thinking.extra_high_reasoning"), thinking_override.as_deref() == Some("xhigh")),
                    ("max", tr("thinking.max_reasoning"), thinking_override.as_deref() == Some("max")),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
                PillMenu::Tools => [
                    ("chat-only", tr("thinking.chat_only"), preset_key == "chat-only"),
                    ("read-only", tr("tools.preset_readonly_count"), preset_key == "read-only"),
                    ("default", tr("tools.preset_full_count"), preset_key == "default"),
                    // full：内置 7 件 + 精确集（-ne + 个人扩展/内置扩展，包零注入）
                    ("full", tr("extensions.builtin_all_desc"), preset_key == "full"),
                    // full+ = full + 本会话选中扩展（默认档；清单按会话保存）
                    ("custom", tr("tools.preset_full_desc"), preset_key == "custom"),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
            };
            let is_thinking = menu == PillMenu::Thinking;
            let items = rows
                .into_iter()
                .map(|(key, desc, active)| {
                    let weak_row = weak_menu.clone();
                    let key_for_rpc = key.clone();
                    div()
                        .id(SharedString::from(format!(
                            "pm-{}-{}",
                            if is_thinking { "t" } else { "w" },
                            key
                        )))
                        .min_h(px(40.))
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak_row.update(cx, |c, cx| {
                                c.pill_menu = None;
                                if is_thinking {
                                    c.set_thinking_level(&key_for_rpc, cx);
                                } else {
                                    c.mc_set_tools_preset(&key_for_rpc, cx);
                                }
                            });
                        })
                        .child(
                            div()
                                .w(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(active.then(|| icon("check", 13., t.accent))),
                        )
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(14.))
                                .font_weight(if active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(rgb(t.text))
                                // full+ 档显示自定义名（其余档沿用内部键）
                                .child(SharedString::from(if key == "custom" {
                                    tr("extensions.full").to_string()
                                } else {
                                    key
                                })),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text_dim))
                                .child(desc),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            // 浮层公共基座：遮挡不穿透 + 点外关闭（ESC 走 composer 的输入框焦点，
            // 这里不抢焦）
            {
                let weak_menu = weak_menu.clone();
                crate::ui::overlay::layer(false, None, move |_w, cx| {
                    let _ = weak_menu.update(cx, |c, cx| {
                        if c.pill_menu.is_some() {
                            c.pill_menu = None;
                            cx.notify();
                        }
                    });
                })
            }
                .child(
                    {
                        // 统一定位（用户定稿）：弹窗与按钮水平居中、底缘距
                        // 按钮顶缘 5px（不遮按钮）；左右钳在视口内
                        let vp = window.viewport_size();
                        let gap = px(5.);
                        let menu_w = px(320.);
                        let (anchor_bottom, anchor_left) = match self.pill_anchor {
                            Some(a) => {
                                let bottom = (vp.height - a.top + gap).max(px(8.));
                                let mut left = a.center_x - menu_w / 2.;
                                if left + menu_w > vp.width - px(8.) {
                                    left = vp.width - menu_w - px(8.);
                                }
                                (bottom, left.max(px(8.)))
                            }
                            None => (px(64.), vp.width - menu_w - px(24.)),
                        };
                        div()
                            .absolute()
                            .bottom(anchor_bottom)
                            .left(anchor_left)
                            .min_w(menu_w)
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg))
                            .shadow_lg()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            // 浮层规则 4：点卡片本身不关（菜单项自己关）
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .children(items)
                    }
                )
                .into_any_element()
        });

        // ---- v54 body: panel-col (topbar-l + dock + statusbar) | content-col
        let entity_for_body = entity.clone();
        let weak_for_body = weak.clone();
        let panes_hidden = self.panes_hidden;
        let slp_dragging = self.slp_drag.is_some();
        let slp_hovered = self.slp_hover;
        let mut body = div()
            .id("app-body")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            // slp 宽度拖拽（resizer 按下时在此跟踪）
            .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
                let _ = weak_for_body.update(cx, |c, cx| {
                    if let Some((start_x, start_w)) = c.slp_drag {
                        c.slp_w = (start_w + f32::from(ev.position.x) - start_x).clamp(300., 500.);
                        cx.notify();
                    }
                });
            })
            .on_mouse_up(
                MouseButton::Left,
                {
                    let weak = weak.clone();
                    move |_, _, cx| {
                        let _ = weak.update(cx, |c, cx| {
                            if c.slp_drag.take().is_some() {
                                c.persist_ui();
                                cx.notify();
                            }
                        });
                    }
                },
            );
        if !panes_hidden {
            // 分隔线响应区（跨线）：panel 内 3px + 内容区 9px（避开滚动条
            // 命中区）；可视线由 content-col 的 border 承担（hover 2px 深线）
            body = body.child(
                div()
                    .id("slp-resizer")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(self.slp_w - 3.))
                    .w(px(12.))
                    .cursor_col_resize()
                    .on_hover(cx.listener(|this, h: &bool, _w, cx| {
                        if this.slp_hover != *h {
                            this.slp_hover = *h;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, cx.listener(
                        |this, ev: &gpui::MouseDownEvent, _w, cx| {
                            if ev.click_count == 2 {
                                this.slp_w = 300.;
                                this.persist_ui();
                                cx.notify();
                            } else {
                                this.slp_drag =
                                    Some((f32::from(ev.position.x), this.slp_w));
                            }
                        },
                    ))
            );
            body = body.child(
                div()
                    .id("panel-col")
                    .w(px(self.slp_w))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .bg(rgb(t.chrome))
                    .child(titlebar::topbar_l(self, cx))
                    .child(function_panel::dock(
                        self,
                        entity_for_body.clone(),
                        &weak,
                        window,
                        cx,
                    ))
                    .child(status_bar::control_bar(self, cx)),
            );
        }
        body = body.child(
            div()
                .id("content-col")
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .relative()
                .bg(rgb(t.bg))
                .when(
                    !panes_hidden && !slp_dragging && !self.slp_hover,
                    |d| {
                        d.border_l_1()
                            .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x99)))
                    },
                )
                // hover/拖拽：2px 深色线替换 1px 常驻线（画在左缘 = 交界，
                // content-col 在 panel-col 之后渲染，z 序安全）
                .when(slp_hovered || slp_dragging, |d| {
                    d.border_l_2()
                        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0xc8)))
                })
                .child(titlebar::topbar_r(self, window, cx))
                .child(content::content_main(
                    self,
                    entity_for_body,
                    &weak,
                    window,
                    cx,
                )),
        );

        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.bg))
            .text_color(rgb(t.text))
            .font_family(crate::appearance::panel_font().family.clone())
            // ComposerPaste 兜底：焦点在对话框等非 composer 输入框时，ctrl-v
            // 截获后走到这里——原样重派发组件 Paste，普通文本粘贴不受影响
            //（composer 胶囊内有更近的 on_action，bubble 最内层先停）
            .on_action(cx.listener(|_, _: &ComposerPaste, window, cx| {
                window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
            }))
            // 023：同款兜底 ×3——composer 绑定在 "Input" 上下文全局劫持了
            // up/down/tab（同深度后注册者优先），焦点在文件编辑器时这四个
            // 动作无人处理=按键全死；bubble 走到根=非 composer，重派发组件
            // 原动作（composer 内层有更近 handler，不会到这里）
            .on_action(cx.listener(|_, _: &ComposerUp, window, cx| {
                window.dispatch_action(Box::new(gpui_component::input::MoveUp), cx);
            }))
            .on_action(cx.listener(|_, _: &ComposerDown, window, cx| {
                window.dispatch_action(Box::new(gpui_component::input::MoveDown), cx);
            }))
            .on_action(cx.listener(|_, _: &ComposerTab, window, cx| {
                window.dispatch_action(Box::new(gpui_component::input::IndentInline), cx);
            }))
            // psp 全局三动作（Ctrl+P / Ctrl+N / Ctrl+K，绑定见 main.rs bind_keys）：
            // 直开项目/新建会话/搜索会话，与 psp 头部三钮同一方法
            .on_action(cx.listener(|this, _: &OpenProjectPicker, _window, cx| {
                let _ = this.open_project_picker(false, cx);
            }))
            .on_action(cx.listener(|this, _: &NewSession, _window, cx| {
                let _ = this.new_session(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSessionSearch, _window, cx| {
                let _ = this.open_session_search(cx);
            }))
            .child(body);

        if let Some(panel) = self.settings.clone() {
            let data = settings::SettingsFormData::snapshot(panel.read(cx), cx);
            root = root.child(settings::render_settings(self, &weak_for_dialog, &data));
        }
        // 弹窗必须挂在 settings **之后**：后渲染者在上——原顺序（dialogs →
        // settings）让设置面板盖住从设置页打开的 ProviderPicker，遮挡层吃掉
        // 全部点击（"看得见点不着"）。弹窗永远是顶层模态，统一放 settings 上。
        root = dialogs::render_dialogs(root, self, &weak_for_dialog, t, cx);
        // 040 卸载扩展确认浮层（叠在设置弹窗之上）
        if let Some(el) = settings::plugins::remove_confirm_overlay(self, cx) {
            root = root.child(el);
        }
        // 042 移除内置商确认浮层（同款机制，叠在设置弹窗之上）
        if let Some(el) = settings::models::remove_provider_confirm_overlay(self, cx) {
            root = root.child(el);
        }
        // psp 悬浮层（tooltip / 详情卡 / 菜单 / 确认）
        root = root.child(function_panel::psp_overlays::psp_overlays(self, window, cx));
        // toolbar pill popup menus
        if let Some(el) = pill_menu_el {
            root = root.child(el);
        }
        // 插件勾选面板：**独立于工具菜单**。千万别再塞回上面那个 if-let ——
        // 插件按钮点击时 pill_menu 是 None，面板会整块不渲染（"点不开"的根因）。
        if let Some(el) = session::plugin_picker::view(self, &weak_for_dialog, window) {
            root = root.child(el);
        }
        // MCP 勾选面板（043）：同上，独立挂载，锚点 = 扩展按钮右边的 MCP 按钮
        if let Some(el) = session::mcp_picker::view(self, &weak_for_dialog, window) {
            root = root.child(el);
        }
        // 模型菜单（042）：同上，独立挂载，锚点 = 输入面板的模型按钮
        if let Some(el) = session::model_picker::view(self, &weak_for_dialog, window, cx) {
            root = root.child(el);
        }
        // status toast（v54: statusbar 无状态文本，改瞬时提示）。2026-10-10
        // 定夺：配色随主题（bg_panel+border+text）、不加阴影（2026-11 用户拍
        // 掉 halo，见上方注释），尺寸自适应 max 600×200、超高滚动、自动换行
        if let Some((msg, _)) = &self.status_toast {
            let text: SharedString = msg.clone().into();
            // 统一弹出定位（2026-10-11）：纵边按锚点象限取自 ui::popup——
            // 顶槽锚点 (y=0) 恒判「上半」向下弹；水平居中是 2026-10-10 定夺
            //（left_0+right_0+justify_center），不走横向象限规则
            let ve = crate::ui::popup::PopupEdges::vertical(
                0.,
                f32::from(window.viewport_size().height),
                44.,
            );
            let mut slot = div().absolute().left_0().right_0().flex().justify_center();
            slot = match (ve.top, ve.bottom) {
                (Some(tp), _) => slot.top(tp),
                (_, Some(bt)) => slot.bottom(bt),
                _ => slot,
            };
            root = root.child(
                slot.child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("status-toast")
                                    .max_w(px(600.))
                                    .max_h(px(200.))
                                    .overflow_y_scroll()
                                    .px(px(20.))
                                    .py(px(7.))
                                    .rounded(px(8.))
                                    .border_2()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.bg_panel))
                                    .text_size(crate::appearance::ui_size(12.))
                                    .text_color(rgb(t.text))
                                    // 非 flex 块级容器：文本按容器宽度自动换行
                                    //（flex 会让文本 item 按最大内容宽撑开不换行）
                                    .child(text),
                            ),
                ),
            );
        }
        // extension notify toast (top-right)。上限与 status toast 对齐
        // （max 600×200、超高滚动、自动换行；2026-10-10 定夺）；不加阴影
        if let Some((message, ty)) = &self.aux.ext_notice {
            let color = match ty {
                1 => 0xfacc15,
                2 => 0xf87171,
                _ => 0x4ade80,
            };
            let text: SharedString = message.clone().into();
            // 统一弹出定位（2026-10-11）：右上槽锚点 (w, 0) → 上半向下弹、
            // 右半向左弹（right 贴边），输出与原 .top(12).right(12) 等价，
            // 但方向由同一算法给出（槽位挪到下方时自动向上弹）
            let vp = window.viewport_size();
            let edge = crate::ui::popup::PopupEdges::around(
                (f32::from(vp.width), 0.),
                (f32::from(vp.width), f32::from(vp.height)),
                12.,
                12.,
            );
            root = root.child(
                edge.apply(div().absolute()).child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("ext-notice")
                                    .max_w(px(600.))
                                    .max_h(px(200.))
                                    .overflow_y_scroll()
                                    .px(px(20.))
                                    .py(px(8.))
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(rgb(color))
                                    .bg(rgb(t.bg_panel))
                                    .flex()
                                    .items_start()
                                    .gap_2()
                                    .child(div().size(px(7.)).rounded_full().mt(px(4.)).bg(rgb(color)))
                                    .child(div().text_size(crate::appearance::ui_size(12.)).text_color(rgb(t.text)).child(text)),
                            ),
                ),
            );
        }
        // blocking extension dialog (select/confirm/input/editor)
        if let Some(req) = &self.aux.ext_dialog {
            root = root.child(render_ext_dialog(self, req.clone(), &weak_for_dialog));
        }
        root
    }
}

