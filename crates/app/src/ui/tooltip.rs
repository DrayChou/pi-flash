//! tooltip 统一入口（UI组件规范 §5.10「悬浮提示」唯一代码落点）。
//!
//! 应用内一切悬停提示（icon 钮、文字、任意可交互元素）一律经由此模块
//! 挂载，禁止直接摸 `gpui_component::tooltip::Tooltip`——弹出机制、提示
//! 卡样式、文案出口（i18n）都在这里收口，改一处全局生效。
//!
//! 机制：gpui 原生 tooltip 手势（`StatefulInteractiveElement::tooltip`，
//! 悬停约 300ms 自动弹出、移开即收），应用层不写 hover 状态机；提示卡
//! 视图 = gpui-component `Tooltip` 默认盘。
//!
//! 用法（tooltip 手势只挂 stateful 元素，`.id()` 必须先行）：
//!
//! ```ignore
//! div().id("psp-new-session")
//!     .with_tip("psp.new_session")   // i18n 键，显示期过 tr 随语言切换
//!     ...
//! ```
//!
//! 会话列表 psp 的项目路径悬浮条/会话详情卡是自绘 overlay（psp_overlays.rs，
//! 「详情卡」语义：可交互、可改名、有宽限期），不是 tooltip，不走这里。

use gpui::prelude::StatefulInteractiveElement;
use gpui::{AnyView, App, SharedString, Window};
use gpui_component::text::Text;

/// 构建提示卡视图（gpui-component Tooltip 默认盘）。样式收口在此。
pub fn tip_view(text: impl Into<Text>, window: &mut Window, cx: &mut App) -> AnyView {
    gpui_component::tooltip::Tooltip::new(text).build(window, cx)
}

/// 给可交互元素挂统一 tooltip。
pub trait WithTip: StatefulInteractiveElement + Sized {
    /// i18n 键：每次显示期过 `i18n::tr`，随语言切换。
    fn with_tip(self, key: &'static str) -> Self {
        // 键翻译留在闭包内（每次弹出重译），语言切换后无需重建元素
        self.tooltip(move |window, cx| tip_view(crate::i18n::tr(key), window, cx))
    }

    /// 动态文案（快捷键拼接等）：直排，不走 i18n。
    // 统一入口的另一半，供动态文案调用点使用；首个调用点落地即摘
    #[allow(dead_code)]
    fn with_tip_text(self, text: impl Into<SharedString>) -> Self {
        let text = text.into();
        // tooltip 闭包是 Fn（每次弹出重建视图），SharedString 廉价克隆
        self.tooltip(move |window, cx| tip_view(text.clone(), window, cx))
    }
}

impl<E: StatefulInteractiveElement> WithTip for E {}
