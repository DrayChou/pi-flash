mod format;
mod inline;
mod node;
mod style;
mod text_view;
mod utils;

use std::sync::Arc;

use gpui::App;
pub use style::*;
pub use text_view::*;

pub(crate) fn init(cx: &mut App) {
    text_view::init(cx);
}

/// App-wide link click handler for TextView links. When set, link clicks are
/// routed to it instead of `cx.open_url`, so host apps can intercept local
/// file links (open in-app editor) and keep http(s) on the system browser.
pub type TextViewLinkHandler = Arc<dyn Fn(&str, &mut App) + Send + Sync>;

struct TextViewLinkHandlerGlobal(Option<TextViewLinkHandler>);

impl gpui::Global for TextViewLinkHandlerGlobal {}

/// Install the app-wide link click handler (call once at startup).
pub fn set_link_handler(handler: TextViewLinkHandler, cx: &mut App) {
    cx.set_global(TextViewLinkHandlerGlobal(Some(handler)));
}

pub(crate) fn dispatch_link(url: &str, cx: &mut App) {
    match cx
        .try_global::<TextViewLinkHandlerGlobal>()
        .and_then(|g| g.0.clone())
    {
        Some(handler) => handler(url, cx),
        None => cx.open_url(url),
    }
}

/// App-wide detector that promotes inline code spans to links. When set,
/// TextView code-span marks whose text matches (e.g. a local file path) are
/// rendered as clickable links (code background kept) that route through the
/// link handler. Pure function, called during render.
pub type TextViewPathDetector = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

struct TextViewPathDetectorGlobal(Option<TextViewPathDetector>);

impl gpui::Global for TextViewPathDetectorGlobal {}

/// Install the app-wide code-span → link detector (call once at startup).
pub fn set_path_link_detector(detector: TextViewPathDetector, cx: &mut App) {
    cx.set_global(TextViewPathDetectorGlobal(Some(detector)));
}

pub(crate) fn detect_path_link(text: &str, cx: &App) -> Option<String> {
    let detector = cx
        .try_global::<TextViewPathDetectorGlobal>()
        .and_then(|g| g.0.clone())?;
    detector(text)
}
