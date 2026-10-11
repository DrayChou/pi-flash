//! pi-flash — desktop shell for the pi coding agent.
//!
//! v54 shell: topbar 两段（左=收放钮 chrome / 右=内容 tabs+设置+窗口控制）·
//! psp 项目+会话一体列表 · 内容区 chat/term/md 状态机 · statusbar 仅面板段。
//! Layout values come from docs/UI设计/主界面UI设计-2.html (mist tokens).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::StreamExt;
use gpui::{
    actions, App, Application, Context, FocusHandle, Focusable, KeyBinding, KeyDownEvent,
    MouseButton, ParentElement,
    Render, SharedString, Styled, WindowOptions, div, prelude::*, px, rgb,
};
use pi_link::protocol::Command;
use pi_link::sessions::{SessionInfo, list_sessions_for_cwd, read_tail_messages};

mod agent_session;
mod automation;
mod appearance;
mod assets;
mod content;
mod dialogs;
mod editor;
mod ext_ui;
mod function_panel;
mod i18n;
mod models_config;
mod remote_control;
mod startup;
mod services;
mod session;
mod settings;
mod status_bar;
mod titlebar;
mod terminal;
mod top_panels;
mod ui;
mod shell;
pub(crate) use shell::chat::Chat;
pub(crate) use shell::types::*;
pub(crate) use ui::ComposerInput;
pub(crate) use session::menu::slash_menu_view;

// composer 覆盖动作（注册为 "Input" 上下文绑定，见 run() 里 bind_keys）：
// ↑/↓ 在菜单态导航补全、空输入态回溯历史，非空多行重新派发组件 MoveUp/
// MoveDown；Tab 在菜单态接受补全；Ctrl+V 截获图片粘贴（composer 附件化，
// 其余输入框经根节点兜底重派发组件 Paste）。组件默认的这些键由此被截获。
actions!(app, [ComposerUp, ComposerDown, ComposerTab, ComposerPaste, FileSave]);
use i18n::tr;
use models_config::EnabledState;
use crate::appearance::theme::theme as T;
use services::format::*;
use services::git::*;
use services::workspace::*;
use session::messages::{Msg, msgs_from_tail};
use session::runtime::SessionRuntime;
use terminal::TerminalTab;
use ui::TextInput;
use ui::icon;
use ext_ui::render_ext_dialog;

static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static PERF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

/// Rows rendered in the model picker (and the keyboard-selection range);
/// further matches are reachable by typing to filter.
pub(crate) const MODEL_PICKER_ROWS: usize = 12;


fn main() {
    let _ = T0.set(std::time::Instant::now());
    // 启动第一步（010-启动.md §4/§10）：建 ~/.pi-flash/、旧 pi-flash-*.json 搬家、
    // recents 首启种子。必须早于任何读盘者（workspace 记忆 / recents / 会话扫描器
    // 都是进程级惰性单例）。
    startup::boot();
    // [perf] 仪器总开关：PI_FLASH_PERF=1 才开（1452 RENDER_ON 同款门控）。
    // 此处曾被无条件 store(true)，每次启动都往 stderr 打 [perf] 行，release 烟测必炸。
    PERF.store(
        std::env::var("PI_FLASH_PERF").is_ok(),
        std::sync::atomic::Ordering::Relaxed,
    );
    // theme: PI_FLASH_THEME (dev override) > app_settings.json（pi-flash 专
    // 属配置，绝不碰 pi 的 settings.json——pi 的 settings schema 有自己的
    // theme 键，写入未知主题名会让 pi 每次启动报错）
    if std::env::var("PI_FLASH_THEME").ok().and_then(|n| crate::appearance::theme::set_by_name(&n).then_some(())).is_none() {
        if let Some(name) = app_settings().theme {
            crate::appearance::theme::set_by_name(&name);
        }
    }
    // language: persisted workspace-memory preference；无偏好默认 zh-CN。
    // set_lang 内同步 gpui-component 的 rust_i18n locale（搜索条文案/tooltip）
    i18n::set_lang(load_lang_pref().unwrap_or(0));
    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            // JetBrains Mono 随二进制打包：全应用唯一内置 mono（会话区 mono
            // + 终端网格；三档字重 + Italic 覆盖 400/600/700 与斜体）。
            // 注册失败仅回退系统字体，不致命。
            cx.text_system().add_fonts(vec![
                std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice()),
                std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-SemiBold.ttf").as_slice()),
                std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf").as_slice()),
                std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-Italic.ttf").as_slice()),
            ])
            .expect("embedded mono fonts are valid TTF");
            // 系统字体目录（设置页字体下拉数据源，字母序；一次性枚举）
            crate::appearance::init_font_catalog(cx);
            // 面板字号 → 全局 UI 缩放（ui_size 基准）
            crate::appearance::sync_ui_scale();
            // gpui-component (widget library powering TextInput): global
            // init + token mapping from the active app theme
            gpui_component::init(cx);
            // composer 覆盖绑定：同深度("Input")后注册者优先，必须排在
            // gpui_component::init 之后（其绑定含 up/down/tab→组件移动/
            // 缩进）。被截获的键由 composer 的 on_action 处理（菜单导航/
            // 历史回溯/补全接受），非空多行时重新派发 MoveUp/MoveDown。
            cx.bind_keys([
                KeyBinding::new("up", ComposerUp, Some("Input")),
                KeyBinding::new("down", ComposerDown, Some("Input")),
                KeyBinding::new("tab", ComposerTab, Some("Input")),
                // 图片粘贴：同深度后注册者优先（覆盖组件 ctrl-v→Paste），
                // composer 内层处理图片附件，其余输入框由根节点兜底重派发
                KeyBinding::new("ctrl-v", ComposerPaste, Some("Input")),
                KeyBinding::new("cmd-v", ComposerPaste, Some("Input")),
                // 023 fileView：编辑器里 Ctrl+S 保存（composer 等其他 Input
                // 上下文内无人监听该 action，自然空转）
                KeyBinding::new("ctrl-s", FileSave, Some("Input")),
                KeyBinding::new("cmd-s", FileSave, Some("Input")),
            ]);
            appearance::sync_gpui_tokens(cx);
            // startup restore (§4)：每次启动默认最大化（位置不持久化——
            // gpui Windows 的外框/客户区坐标在存取间不对称，每个周期漂移
            // 一个边框宽）。取消最大化后的尺寸仍保存，供会话内还原参考。
            // 010-启动：先以普通窗口呈现启动页（不全屏），揭幕后由 render
            // 的 pending_zoom 补最大化——0.2.2 建窗即 Maximized 的延迟处理
            // 实测不生效，显式 zoom 是项目已验证路径（从建窗回调后移）。
            let _restored = get_window_state();
            let bounds = gpui::Bounds::centered(None, gpui::size(px(1180.), px(760.)), cx);
            let window_bounds = gpui::WindowBounds::Windowed(bounds);
            let automation_chat: std::sync::OnceLock<gpui::WeakEntity<Chat>> =
                std::sync::OnceLock::new();
            let window_handle = cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    // 最小宽度 900px（逻辑像素，gpui 按 scale_factor 换算再交给
                    // WM_GETMINMAXINFO）：窗口再缩也不至于把聊天列挤到不可用。
                    // 高度不设底（0 = 只保留边框开销），需要时再单独限。
                    window_min_size: Some(gpui::size(px(900.), px(0.))),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("pi-flash".into()),
                        // client-side title bar (v54 topbar 两段, window
                        // control hitboxes registered by titlebar.rs)
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    // gpui-component widgets require their Root as the window
                    // root view (renders their context-menu/popover layers)
                    let chat = cx.new(Chat::new);
                    let weak = chat.downgrade();
                    let _ = automation_chat.set(weak.clone());
                    // persist window bounds + shell layout on close so the
                    // startup restore layer has data (§4)
                    window.on_window_should_close(cx, move |window, cx| {
                        let b = window.bounds();
                        save_window_state(&WindowState {
                            x: f64::from(b.origin.x),
                            y: f64::from(b.origin.y),
                            w: f64::from(b.size.width),
                            h: f64::from(b.size.height),
                            maximized: window.is_maximized(),
                        });
                        if let Some(chat) = weak.upgrade() {
                            chat.update(cx, |chat, _cx| chat.persist_ui());
                        }
                        true
                    });
                    cx.new(|cx| gpui_component::Root::new(chat.into(), window, cx))
                },
            )
            .unwrap();
            // UI 自动化服务（pi-flash-2kq）：PI_FLASH_AUTOMATION=<port|auto|1>
            // 显式开启，缺省关闭——agent 调试用，不抢真实屏幕/鼠标。
            if let (Some(weak), Ok(spec)) = (
                automation_chat.get(),
                std::env::var("PI_FLASH_AUTOMATION"),
            ) {
                if let Err(e) =
                    automation::start(cx, window_handle.into(), weak.clone(), &spec)
                {
                    eprintln!("pi-flash automation: {e}");
                }
            }
            cx.activate(true);
        });
}
