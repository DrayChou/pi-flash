//! 设置「其他」页 (v54): pi 版本 / 启动恢复 / 加载时间窗口 / 自动折叠时间 /
//! 会话显示数 / 提示音 / 工作区数据目录。set-row 版式（标题+描述左，控件右）。
//! 展示思考 / 文件树 Git 标识已挪至「界面」页。

use super::*;

pub(crate) fn mc_misc_view(
    _chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let mut col = div().id("mc-misc").flex().flex_col();

    // 启动恢复 switch
    let restore_on = crate::services::workspace::startup_restore();
    col = col.child(set_row(
        tr("settings.restore_title"),
        tr("settings.restore_hint"),
        switch(
            "misc-restore",
            restore_on,
            {
                let weak = weak.clone();
                move |on, cx| {
                    let _ = weak.update(cx, |_c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.restore = Some(on);
                        crate::services::workspace::save_app_settings(&s);
                        cx.notify();
                    });
                }
            },
            t,
        ),
        t,
    ));

    // 加载时间窗口（7/14/30 天档位）：启动按最近活动清单加载窗口内会话
    let days = crate::services::workspace::load_window_days();
    col = col.child(set_row(
        tr("settings.load_time_window"),
        tr("settings.load_window_hint"),
        {
            let weak = weak.clone();
            window_row(days, &weak, t)
        },
        t,
    ));

    // 自动折叠时间（1/3/7 天档位）：启动时项目内会话全部早于该阈值 → 项目
    // 默认折叠（内部按小时算）。置于「会话显示数」上方（两者都是列表行为）
    let collapse_h = crate::services::workspace::auto_collapse_hours();
    col = col.child(set_row(
        tr("settings.auto_collapse_time"),
        tr("settings.auto_collapse_hint"),
        {
            let weak = weak.clone();
            collapse_row(collapse_h, &weak, t)
        },
        t,
    ));

    // 会话显示数（3-10）：各项目默认显示的会话数量（psp 初始页大小）
    let sess_n = crate::services::workspace::session_display_count() as u64;
    col = col.child(set_row(
        tr("settings.session_display_count"),
        tr("settings.session_count_hint"),
        stepper(
            "misc-sess",
            sess_n,
            3,
            10,
            |v| v.to_string(),
            || crate::services::workspace::session_display_count() as u64,
            |v| {
                let mut s = crate::services::workspace::app_settings();
                s.session_display_count = Some(v);
                crate::services::workspace::save_app_settings(&s);
            },
            weak,
            t,
        ),
        t,
    ));

    // 自动清理会话（052）：−/值/+ 步进，范围 [0,100]，默认 3；0 = 关闭；
    // 最后活动超出天数的非置顶会话启动时移入回收站。置于「提示音」前（052 定）
    col = col.child(set_row(
        tr("settings.auto_clean_title"),
        tr("settings.auto_clean_hint"),
        clean_stepper(weak, t),
        t,
    ));

    // 提示音 switch（v60 从界面页挪入）
    let sound_on = crate::services::workspace::load_sound_pref();
    col = col.child(set_row(
        tr("settings.notification_sound"),
        tr("settings.sound_hint"),
        switch(
            "misc-sound",
            sound_on,
            {
                let weak = weak.clone();
                move |on, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        crate::services::workspace::save_sound_pref(on);
                        c.broadcast_sound_on(on, cx);
                        if on {
                            crate::services::workspace::play_notify_sound();
                        }
                        cx.notify();
                    });
                }
            },
            t,
        ),
        t,
    ));

    // 自动更新 switch（081：默认 on——启动后台检查 release，on=下载 staging
    // 待重启，off=只在新会话页提示新版本号 ↑ 可点击开 release 页）
    let auto_on = crate::services::workspace::auto_update();
    col = col.child(set_row(
        tr("about.auto_update"),
        tr("settings.auto_update_hint"),
        switch(
            "misc-auto-update",
            auto_on,
            {
                let weak = weak.clone();
                move |on, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.auto_update = Some(on);
                        crate::services::workspace::save_app_settings(&s);
                        // 关→开：立即补一次检查（Idle 才发，避免与进行中的流程叠加）
                        if on && matches!(c.aux.update, crate::services::updater::UpdateState::Idle) {
                            c.spawn_update_check(cx);
                        }
                        cx.notify();
                    });
                }
            },
            t,
        ),
        t,
    ));

    // 工作区数据目录：pf 自有目录（~/.pi-flash）在上、pi 目录（~/.pi/agent）
    // 在下，上下两行；每行右侧「打开」按钮（系统文件浏览器打开，右对齐）
    let pf_dir = pi_link::paths::dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    col = col.child(set_row(
        tr("settings.data_dir"),
        tr("settings.memory_dir"),
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(dir_row("pf", &pf_dir, t))
            .child(dir_row("pi", &format!("{home}\\.pi\\agent"), t))
            .into_any_element(),
        t,
    ));

    col.into_any_element()
}

/// 档位：7/14/30 天一排（仿语言按钮，active 描边）。
fn window_row(
    days: u64,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    const CHOICES: [(u64, &str); 3] = [
        (7, "misc.window_7d"),
        (14, "misc.window_14d"),
        (30, "misc.window_30d"),
    ];
    div()
        .flex()
        .gap(px(8.))
        .children(CHOICES.iter().map(|(v, key)| {
            let active = days == *v;
            let weak_item = weak.clone();
            let value = *v;
            let label = tr(*key);
            div()
                .id(SharedString::from(format!("misc-window-{v}")))
                .w(px(72.))
                .h(px(30.))
                .px(px(10.))
                .rounded(px(5.))
                .border_1()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                // §5.4：选项文字统一 text_muted，选中只加粗不变色
                // （选中感由 accent 描边 + bg_selected 承担）
                .text_color(rgb(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |_c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.load_window_days = Some(value);
                        crate::services::workspace::save_app_settings(&s);
                        cx.notify();
                    });
                })
                .child(SharedString::from(label.to_string()))
                .into_any_element()
        }))
        .into_any_element()
}

/// 档位：1/3/7 天一排（仿语言按钮，active 描边）——内部按小时（24/72/168）
/// 存取与比较，档位值即三档小时数。
fn collapse_row(
    hours: u64,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    const CHOICES: [(u64, &str); 3] = [
        (24, "misc.collapse_1d"),
        (72, "misc.collapse_3d"),
        (168, "misc.collapse_7d"),
    ];
    div()
        .flex()
        .gap(px(8.))
        .children(CHOICES.iter().map(|(v, key)| {
            let active = hours == *v;
            let label = tr(*key);
            let weak_item = weak.clone();
            let value = *v;
            div()
                .id(SharedString::from(format!("misc-collapse-{v}")))
                .w(px(72.))
                .h(px(30.))
                .px(px(10.))
                .rounded(px(5.))
                .border_1()
                .border_color(if active { rgb(t.accent) } else { rgb(t.border) })
                .bg(if active { rgb(t.bg_selected) } else { rgb(t.bg_panel) })
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                // §5.4：选项文字统一 text_muted，选中只加粗不变色
                // （选中感由 accent 描边 + bg_selected 承担）
                .text_color(rgb(t.text_muted))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |_c, cx| {
                        let mut s = crate::services::workspace::app_settings();
                        s.auto_collapse_hours = Some(value);
                        crate::services::workspace::save_app_settings(&s);
                        cx.notify();
                    });
                })
                .child(SharedString::from(label.to_string()))
                .into_any_element()
        }))
        .into_any_element()
}

/// 步进控件（通用版）：−/+ 两个 24px 档位按钮夹居中数值，到边界后对应按钮
/// 置灰禁点。`render` 生成中间显示（"10" / "3 天" / "关闭"），`cur` 在点击
/// 时重读当前值（不吃渲染快照陈值），夹回 [min, max] 后交 `apply` 落盘。
/// 现用于会话显示数（3-10）与自动清理天数（0-100，052）。
fn stepper(
    id_prefix: &'static str,
    value: u64,
    min: u64,
    max: u64,
    render: impl Fn(u64) -> String + 'static,
    cur: impl Fn() -> u64 + Clone + 'static,
    apply: impl Fn(u64) + Clone + 'static,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(step_btn(
            format!("{id_prefix}-dec"),
            "−",
            value > min,
            -1,
            min,
            max,
            cur.clone(),
            apply.clone(),
            weak,
            t,
        ))
        .child(
            div()
                .min_w(px(28.))
                .flex()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                // §5.4：与页内其他文字同色，层级只由加粗表达
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(render(value))),
        )
        .child(step_btn(
            format!("{id_prefix}-inc"),
            "+",
            value < max,
            1,
            min,
            max,
            cur,
            apply,
            weak,
            t,
        ))
        .into_any_element()
}

/// 052 自动清理天数步进：[0,100]，默认 3；0 显示「关闭」（= 不自动清理）。
fn clean_stepper(
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    stepper(
        "misc-clean",
        crate::services::workspace::auto_clean_days(),
        0,
        100,
        |v| {
            if v == 0 {
                tr("misc.auto_clean_off").to_string()
            } else {
                crate::i18n::tf("misc.auto_clean_days", &[("n", v.to_string())])
            }
        },
        crate::services::workspace::auto_clean_days,
        |v| {
            let mut s = crate::services::workspace::app_settings();
            s.auto_clean_days = Some(v);
            crate::services::workspace::save_app_settings(&s);
        },
        weak,
        t,
    )
}

/// 步进按钮：enabled=false 时置灰且不挂点击；点击把 cur() + delta 夹回
/// [min, max] 后交 apply 落盘（weak.update 只为拿 Chat 的 notify）。
fn step_btn(
    id: String,
    label: &str,
    enabled: bool,
    delta: i64,
    min: u64,
    max: u64,
    cur: impl Fn() -> u64 + 'static,
    apply: impl Fn(u64) + 'static,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let base = div()
        .id(SharedString::from(id))
        .size(px(24.))
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .flex()
        .items_center()
        .justify_center()
        .text_size(crate::appearance::ui_size(13.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(if enabled { t.text_dim } else { t.text_muted }));
    let btn = if enabled {
        let weak_click = weak.clone();
        base.cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_click.update(cx, |_c, cx| {
                    apply(((cur() as i64) + delta).clamp(min as i64, max as i64) as u64);
                    cx.notify();
                });
            })
    } else {
        base
    };
    btn.child(SharedString::from(label.to_string()))
        .into_any_element()
}

/// 目录行：mono 路径占满左侧，「打开」按钮右对齐（系统文件浏览器打开）。
fn dir_row(key: &str, path: &str, t: &'static crate::appearance::theme::Theme) -> gpui::AnyElement {
    let open_path = std::path::PathBuf::from(path);
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().flex_1().min_w_0().truncate().child(mono_value(path)))
        .child(
            div()
                .id(SharedString::from(format!("misc-open-{key}")))
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                // 文件树标题栏「在资源管理器中打开」同款 icon
                .child(crate::ui::icon("folder-input", 16., t.text_dim))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    crate::services::shell_open::open_in_explorer(&open_path);
                }),
        )
        .into_any_element()
}

fn mono_value(text: &str) -> gpui::AnyElement {
    let t = T();
    div()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t.text_dim))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// 开关（原 34×19 私有实现 → 委托 [`switch_base`]，SW_MD 28×16 全局统一）。
fn switch(
    id: &'static str,
    on: bool,
    apply: impl Fn(bool, &mut gpui::App) + 'static,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let _ = t; // 配色统一走 theme 语义，入参保留兼容既有调用点
    switch_base(id, on, false, apply)
}

