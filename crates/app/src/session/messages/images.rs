//! 消息内图片：结果图配对、按容器缩放、格式头探测（PNG/JPEG/GIF/
//! BMP/WebP）与 thread_local 解码缓存（LRU，cap 64）。

use super::*;


use gpui::{SharedString, div, prelude::*, px, relative, rgb, rgba};

use crate::i18n::tr;
use crate::appearance::theme;


/// pi-web PairedResult：maxHeight 400 滚动、空结果斜体 0.6、错误红字。
pub(super) fn paired_result(
    result: &str,
    is_error: bool,
    t: &theme::Theme,
    msg_ix: usize,
    content_index: usize,
) -> gpui::AnyElement {
    let empty = result_is_empty(result);
    let text = if empty {
        tr("messages.no_output").to_string()
    } else {
        result.to_string()
    };
    div()
        .id(SharedString::from(format!("tres-{msg_ix}-{content_index}")))
        .max_h(px(400.))
        .overflow_y_scroll()
        .border_t_1()
        .border_color(gpui::rgba(if is_error {
            theme::rgba_a(0xf87171, 0.3)
        } else {
            theme::rgba_a(0x22c55e, 0.15)
        }))
        .bg(if is_error {
            gpui::rgba(theme::rgba_a(0xf87171, 0.04))
        } else {
            rgba(t.bg_subtle)
        })
        .px(px(10.))
        .py(px(8.))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::sess_size(-2.))
        .line_height(relative(1.5))
        .when(empty, |d| d.italic().text_color(rgb(t.text_dim)).opacity(0.6))
        .when(!empty && is_error, |d| d.text_color(rgb(0xf87171)))
        .when(!empty && !is_error, |d| d.text_color(rgb(t.text_muted)))
        .child(SharedString::from(text))
        .into_any_element()
}

/// pi-web ResultImages parity（v56-2 c11）：flex wrap、图片
/// maxWidth 720/maxHeight 520、圆角 6、1px 边框；10MB 上限、
/// png/jpeg/webp/gif/bmp 白名单（pi-web 另含 avif，gpui 无解码器跳过）。
pub(super) fn result_images(
    images: &[pi_link::protocol::ImageData],
    is_error: bool,
    t: &theme::Theme,
) -> gpui::AnyElement {
    const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
    let mut wrap = div()
        .flex()
        .flex_wrap()
        .gap(px(8.))
        .p(px(10.))
        .bg(rgb(t.bg))
        .border_t_1()
        .border_color(gpui::rgba(if is_error {
            theme::rgba_a(0xf87171, 0.3)
        } else {
            theme::rgba_a(0x22c55e, 0.15)
        }));
    for data in images {
        let Some(format) = mime_to_image_format(&data.mime) else {
            continue;
        };
        let Ok(image) = decode_image_cached(&data.data, format) else {
            continue;
        };
        if image.bytes.len() > MAX_IMAGE_BYTES {
            continue;
        }
        wrap = wrap.child(
            fitted_image(image, 720., 520.)
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border)),
        );
    }
    wrap.into_any_element()
}

/// 图片绘制框按 max 约束等比缩放（结果图与大图预览共用）。
///
/// gpui `Img` 在 `request_layout` 里把 style 的 w/h 从原始像素**分别**定死，
/// `max_w`/`max_h` 于是各自独立裁剪一维（宽图被 520 高度压扁），再叠加默认
/// `ObjectFit::Contain` 就出现上下/左右留白。这里先解析自然像素尺寸自己算
/// 缩放比，把 w/h 一起定成同比例的实际绘制框，元素盒 = 图像，无空白。
/// 尺寸解析不出来（SVG 等）时退回 gpui 原生行为。
pub(crate) fn fitted_image(
    image: std::sync::Arc<gpui::Image>,
    max_w: f32,
    max_h: f32,
) -> gpui::Img {
    let mut el = gpui::img(image.clone());
    if let Some((w, h)) = image_natural_size(&image.bytes, image.format) {
        let (dw, dh) = fit_box(w, h, max_w, max_h);
        if dw >= 1.0 && dh >= 1.0 {
            el = el.w(px(dw)).h(px(dh));
        }
    } else {
        el = el.max_w(px(max_w)).max_h(px(max_h));
    }
    el
}

/// 等比缩放到 (max_w, max_h) 框内；不放大小图（与 gpui 原生像素尺寸一致）。
pub(super) fn fit_box(w: u32, h: u32, max_w: f32, max_h: f32) -> (f32, f32) {
    if w == 0 || h == 0 {
        return (max_w, max_h);
    }
    let scale = (max_w / w as f32).min(max_h / h as f32).min(1.0);
    (
        (w as f32 * scale).round().max(1.0),
        (h as f32 * scale).round().max(1.0),
    )
}

/// 编码字节的像素尺寸（PNG/JPEG/GIF/BMP/WebP）；SVG 等返回 None。
pub(super) fn image_natural_size(bytes: &[u8], format: gpui::ImageFormat) -> Option<(u32, u32)> {
    // 注意：try_into 的目标数组长度由 from_*_bytes 反推，必须显式给中间类型
    // （写成 u32::from_le_bytes(slice) 会被推成 [u8;4] 而恒失败）
    fn le16(b: &[u8], at: usize) -> Option<u32> {
        let v: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
        Some(u32::from(u16::from_le_bytes(v)))
    }
    fn le24(b: &[u8], at: usize) -> Option<u32> {
        let v: [u8; 3] = b.get(at..at + 3)?.try_into().ok()?;
        Some(u32::from_le_bytes([v[0], v[1], v[2], 0]))
    }
    fn le32(b: &[u8], at: usize) -> Option<u32> {
        let v: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_le_bytes(v))
    }
    fn be32(b: &[u8], at: usize) -> Option<u32> {
        let v: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_be_bytes(v))
    }
    fn ok(w: u32, h: u32) -> Option<(u32, u32)> {
        (w > 0 && h > 0).then_some((w, h))
    }

    match format {
        gpui::ImageFormat::Png => {
            (bytes.len() >= 24 && bytes.starts_with(&[0x89, b'P', b'N', b'G']))
                .then(|| ok(be32(bytes, 16)?, be32(bytes, 20)?))
                .flatten()
        }
        gpui::ImageFormat::Jpeg => {
            if !bytes.starts_with(&[0xFF, 0xD8]) {
                return None;
            }
            let mut i = 2usize;
            while i + 3 < bytes.len() {
                if bytes[i] != 0xFF {
                    i += 1;
                    continue;
                }
                let marker = bytes[i + 1];
                i += 2;
                // 填充字节 FF FF …
                if marker == 0xFF {
                    i -= 1;
                    continue;
                }
                // SOF0..SOF15（排除 DHT=C4 / JPG=C8 / DAC=CC），无长度字段
                let is_sof = (0xC0..=0xCF).contains(&marker)
                    && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
                if is_sof {
                    // 段体：len(2) + precision(1) + height(2) + width(2)
                    return ok(be16(bytes, i + 5)?, be16(bytes, i + 3)?);
                }
                if marker == 0xD8 || marker == 0xD9 {
                    return None;
                }
                let seg_len = be16(bytes, i)? as usize;
                if seg_len < 2 {
                    return None;
                }
                i += seg_len;
            }
            None
        }
        gpui::ImageFormat::Gif => {
            (bytes.len() >= 10 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")))
                .then(|| ok(le16(bytes, 6)?, le16(bytes, 8)?))
                .flatten()
        }
        gpui::ImageFormat::Bmp => {
            (bytes.len() >= 26 && bytes.starts_with(b"BM")).then(|| {
                let w = le32(bytes, 18)? as i32;
                let h = le32(bytes, 22)? as i32;
                ok(w.unsigned_abs(), h.unsigned_abs())
            })
            .flatten()
        }
        gpui::ImageFormat::Webp => {
            if bytes.len() < 30 || !bytes.starts_with(b"RIFF") || !bytes[8..].starts_with(b"WEBP")
            {
                return None;
            }
            match &bytes[12..16] {
                b"VP8 " => ok(le16(bytes, 26)? & 0x3FFF, le16(bytes, 28)? & 0x3FFF),
                b"VP8X" => ok(le24(bytes, 24)? + 1, le24(bytes, 27)? + 1),
                b"VP8L" => {
                    let b = bytes.get(21..25)?;
                    let bits = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    ok((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?)))
}

pub(crate) fn mime_to_image_format(mime: &str) -> Option<gpui::ImageFormat> {
    match mime {
        "image/png" => Some(gpui::ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Some(gpui::ImageFormat::Jpeg),
        "image/gif" => Some(gpui::ImageFormat::Gif),
        "image/bmp" => Some(gpui::ImageFormat::Bmp),
        "image/webp" => Some(gpui::ImageFormat::Webp),
        "image/svg+xml" => Some(gpui::ImageFormat::Svg),
        _ => None,
    }
}

fn decode_image_data(data: &str) -> Result<Vec<u8>, ()> {
    use base64::Engine as _;
    let trimmed = data.trim();
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(trimmed))
        .map_err(|_| ())
}

// 解码缓存：List 每帧重建元素树，同一张 base64 图每帧重走 base64 解码 +
// `Image::from_bytes` 的内容哈希（几百 KB 数据 = ms 级），滚动/流式时纯浪费。
// key = (哈希, 长度, 格式)，命中全等校验防碰撞；线程局部 VecDeque 当 LRU。
// 预算按字节计（源串 + 解码后字节）：条目数上限只防碎片，真正封顶的是
// 字节预算——大图会话里 64 张 × 几 MB 的 base64 原文 + 解码位图曾可达
// 数百 MB（内存泄漏观感的元凶之一）。
thread_local! {
    static IMAGE_DECODE_CACHE: std::cell::RefCell<
        std::collections::VecDeque<
            ((u64, usize, u8), (String, Result<std::sync::Arc<gpui::Image>, ()>)),
        >,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const IMAGE_DECODE_CACHE_CAP: usize = 64;
/// 字节预算（128MB）：大图工具结果场景的合理上限；超预算从头驱逐（LRU）
const IMAGE_DECODE_CACHE_BUDGET_BYTES: usize = 128 * 1024 * 1024;

/// base64 → [`gpui::Image`]（带每进程缓存；Err 结果同样入缓存，坏数据不反复重试）
pub(crate) fn decode_image_cached(
    data: &str,
    format: gpui::ImageFormat,
) -> Result<std::sync::Arc<gpui::Image>, ()> {
    let key = (crate::editor::markdown::hash_str(data), data.len(), format as u8);
    IMAGE_DECODE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1 .0.as_str() == data {
                let image = entry.1 .1.clone();
                cache.push_back(entry); // 刷新到队尾（LRU）
                return image;
            }
        }
        let decoded = decode_image_data(data)
            .map(|bytes| std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)));
        cache.push_back((key, (data.to_string(), decoded.clone())));
        while cache.len() > IMAGE_DECODE_CACHE_CAP
            || cache.iter().map(|(_, v)| v.0.len() + v.1.as_ref().map(|i| i.bytes.len()).unwrap_or(0)).sum::<usize>()
                > IMAGE_DECODE_CACHE_BUDGET_BYTES
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        decoded
    })
}
