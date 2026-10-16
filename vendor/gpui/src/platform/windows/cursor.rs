//! Windows 自绘 grab 手型光标（PF vendored patch，2026-10-16）：
//! Windows 系统光标集（IDC_*）只有指向手 IDC_HAND，没有 grab/grabbing
//! 掌形——OpenHand/ClosedHand 原本落入箭头兜底（util.rs load_cursor 的
//! `_` 分支）。这里用内嵌 SVG（lucide `hand` / `hand-grab`，用户定稿
//! 2026-10-16：项目行悬停 = 伸展小手、拖拽 = 握紧小手）经 gpui 自带的
//! SvgRenderer 光栅化成 32×32 位图，CreateIconIndirect 生成 HCURSOR，
//! 进程级缓存、永不销毁。
//!
//! 可见性处理：光标要在任意底色上可辨，SVG 走双层描边——底层黑粗边 +
//! 顶层白细芯（Windows/Chromium 光标惯例的白身黑边）。

use std::sync::Arc;

use resvg::tiny_skia::Pixmap;
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::HCURSOR;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, BITMAPINFO,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO, HICON};

use crate::svg_renderer::{SvgRenderer, SvgSize};
use crate::{DevicePixels, Size};

/// lucide `hand`（悬停：伸展小手，用户供图 2026-10-16）。
/// 双层描边：底层 4.2 黑边、顶层 1.9 白芯（24 viewBox 下 ≈1.5px 黑边）。
const HAND_OPEN_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round"><g stroke="#000" stroke-width="4.2"><path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8"/><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"/></g><g stroke="#fff" stroke-width="1.9"><path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8"/><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"/></g></svg>"##;

/// lucide `hand-grab`（拖拽：握紧小手，用户供图 2026-10-16）。
const HAND_GRAB_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round"><g stroke="#000" stroke-width="4.2"><path d="M18 11.5V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v1.4"/><path d="M14 10V8a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 9.9V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v5"/><path d="M6 14a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M18 11a2 2 0 1 1 4 0v3a8 8 0 0 1-8 8h-4a8 8 0 0 1-8-8 2 2 0 1 1 4 0"/></g><g stroke="#fff" stroke-width="1.9"><path d="M18 11.5V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v1.4"/><path d="M14 10V8a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 9.9V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v5"/><path d="M6 14a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M18 11a2 2 0 1 1 4 0v3a8 8 0 0 1-8 8h-4a8 8 0 0 1-8-8 2 2 0 1 1 4 0"/></g></svg>"##;

const CURSOR_PX: i32 = 32;
/// 热点 = 食指指尖（lucide hand 指尖约 (7,3)，24→32 放大后取整）。
const HOTSPOT: (u32, u32) = (9, 4);

/// 生成 grab 光标（`closed` = 握紧手）。SVG 解析/光栅化/GDI 任一步失败
/// 返回 None，调用方回退系统箭头。
pub(crate) fn load_hand_cursor(closed: bool) -> Option<HCURSOR> {
    let svg = if closed { HAND_GRAB_SVG } else { HAND_OPEN_SVG };
    let pixmap = SvgRenderer::new(Arc::new(()))
        .render_pixmap(
            svg.as_bytes(),
            SvgSize::Size(Size::new(
                DevicePixels(CURSOR_PX),
                DevicePixels(CURSOR_PX),
            )),
        )
        .ok()?;
    create_cursor(&pixmap, HOTSPOT.0, HOTSPOT.1)
}

/// Pixmap（RGBA 预乘）→ 32bpp BGRA DIB（直通 alpha、自上而下）+ 全零
/// AND 掩码（形状全权交给 alpha）→ CreateIconIndirect。HICON 与 HCURSOR
/// 在现代 Windows 句柄空间互通，直接转用。
fn create_cursor(pixmap: &Pixmap, hot_x: u32, hot_y: u32) -> Option<HCURSOR> {
    let w = pixmap.width() as i32;
    let h = pixmap.height() as i32;
    let mut bgra = Vec::with_capacity(pixmap.data().len());
    for p in pixmap.pixels() {
        let c = p.demultiply();
        bgra.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
    }

    unsafe {
        let hdc = CreateCompatibleDC(None);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>()
            as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = 0; // BI_RGB

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp_color =
            CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());

        let mask_bits = [0u8; 128];
        let hbmp_mask = CreateBitmap(w, h, 1, 1, Some(mask_bits.as_ptr().cast()));

        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: BOOL(1),
            xHotspot: hot_x,
            yHotspot: hot_y,
            hbmMask: hbmp_mask,
            hbmColor: hbmp_color,
        })
        .ok()?;

        let _ = DeleteObject(hbmp_color.into());
        let _ = DeleteObject(hbmp_mask.into());
        let _ = DeleteDC(hdc);
        Some(HCURSOR(icon.0))
    }
}

// HICON → HCURSOR 转换只在两者内层类型一致时成立（windows 0.61 均为指针
// 新类型）；编一道静态断言防上游换句柄表示。
const _: () = assert!(
    std::mem::size_of::<HICON>() == std::mem::size_of::<HCURSOR>()
        && std::mem::align_of::<HICON>() == std::mem::align_of::<HCURSOR>()
);
