//! gpui 解码位图的延迟驱逐。`gpui::img(Arc<Image>)` 首次渲染后，解码位图
//! （宽×高×4 字节）钉在 `App::loading_assets` 里——fetch_asset 只进不出，
//! gpui 侧唯一出口是显式 `ImageSource::remove_asset`，而 PF 侧各图片缓存
//! （消息图片解码 / mermaid / math / markdown 本地图）驱逐条目时 gpui 侧
//! 并不跟进，长跑下位图只增不减（每张 1080p 截图 ≈ 8MB）。
//!
//! 约定：PF 侧缓存驱逐条目时把源 `Arc<Image>` 投进 [`queue_evicted`]，
//! 渲染帧首（Chat::render）统一 [`drain`]，从 gpui 资产表按源哈希精确摘除，
//! 解码位图随之释放。被驱逐的图若其实还在屏上（字节预算压力下的误逐），
//! 下一帧 use_asset 会重新解码回填——代价是偶发一帧空白，正确性无损。

use std::sync::{Arc, Mutex};

static PENDING: Mutex<Vec<Arc<gpui::Image>>> = Mutex::new(Vec::new());

/// 缓存驱逐点调用：该图在 PF 侧已无条目，gpui 侧解码位图待放。
pub(crate) fn queue_evicted(image: Arc<gpui::Image>) {
    if let Ok(mut q) = PENDING.lock() {
        q.push(image);
    }
}

/// 渲染帧首调用：把队列里的图从 gpui 资产表摘除（同帧元素构建在 drain
/// 之后进行，仍在屏的图会重新解码入表，顺序无害）。
pub(crate) fn drain(cx: &mut gpui::App) {
    let drained = match PENDING.lock() {
        Ok(mut q) => std::mem::take(&mut *q),
        Err(_) => return,
    };
    for image in drained {
        gpui::ImageSource::from(image).remove_asset(cx);
    }
}

/// 探针（automation snapshot）：待驱逐队列长度。
pub(crate) fn pending_len() -> usize {
    PENDING.lock().map(|q| q.len()).unwrap_or(0)
}
