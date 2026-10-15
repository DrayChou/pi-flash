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
