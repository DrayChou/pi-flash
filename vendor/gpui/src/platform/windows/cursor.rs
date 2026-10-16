//! Windows 自绘 grab 手型光标（PF vendored patch，2026-10-16；图形 v2 白色版）：
//! Windows 系统光标集（IDC_*）只有指向手 IDC_HAND，没有 grab/grabbing 掌形
//! ——OpenHand/ClosedHand 原本落入箭头兜底（util.rs load_cursor 的 `_` 分支）。
//! 图形 = 用户供图手掌路径（iconfont 1024 视框，伸展掌 / 握拳各一条），渲染 =
//! **白填充 + 黑描边**（`fill="#fff" stroke="#000"`，Windows 箭头同款配方：
//! 白身保深色底可辨、黑线保浅色底可辨——纯白版在浅色面板上边线发灰看不清，
//! 用户定稿加黑线），gpui 自带 SvgRenderer 光栅化成 32×32，CreateIconIndirect
//! 生成 HCURSOR，进程级缓存、永不销毁、任一步失败回退系统箭头。

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

/// 伸展小手（悬停态，用户供图 2026-10-16：实心路径白填充 + 黑描边）。
const HAND_OPEN_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024"><path fill="#fff" stroke="#000" stroke-width="24" stroke-linejoin="round" d="M870.4 204.8c-18.6368 0-36.1472 5.0176-51.2 13.7728l0-64.9728c0-56.4736-45.9264-102.4-102.4-102.4-21.0944 0-40.6528 6.4-56.9856 17.3568-14.0288-39.8848-52.0192-68.5568-96.6144-68.5568s-82.6368 28.672-96.6144 68.5568c-16.2816-10.9568-35.8912-17.3568-56.9856-17.3568-56.4736 0-102.4 45.9264-102.4 102.4l0 377.4976-68.9152-119.4496c-13.3632-24.32-35.1744-41.6256-61.3888-48.7936-25.5488-6.9632-52.1216-3.2768-74.8544 10.3424-46.4384 27.8528-64.1536 90.8288-39.424 140.3904 1.536 3.1232 34.2016 70.0416 136.192 273.92 48.0256 96 100.7104 164.6592 156.6208 203.9808 43.8784 30.8736 74.1888 32.4608 79.8208 32.4608l256 0c43.5712 0 84.0704-14.1824 120.4224-42.0864 34.1504-26.2656 63.7952-64.256 88.064-112.8448 47.8208-95.6416 73.1136-227.9424 73.1136-382.6688l0-179.2c0-56.4736-45.9264-102.4-102.4-102.4zM921.6 486.4c0 146.7904-23.3984 271.1552-67.6864 359.7312-28.8768 57.7536-80.5888 126.6688-162.7136 126.6688l-255.488 0c-1.9968-0.1536-23.552-2.56-56.064-26.88-32.4096-24.2688-82.176-75.3664-135.0656-181.248-103.7824-207.5648-135.68-272.9472-135.9872-273.5616-0.0512-0.1024-0.0512-0.1536-0.1024-0.2048-12.8512-25.7536-3.7376-59.4944 19.9168-73.6768 10.6496-6.4 23.0912-8.0896 35.072-4.864 12.7488 3.4816 23.4496 12.0832 30.0544 24.1664 0.1024 0.1536 0.2048 0.3584 0.3072 0.512l79.9232 138.496c16.3328 29.8496 34.7136 42.3936 54.6304 37.3248 19.968-5.0688 30.0544-25.0368 30.0544-59.2384l0-400.0256c0-28.2112 22.9888-51.2 51.2-51.2s51.2 22.9888 51.2 51.2l0 332.8c0 14.1312 11.4688 25.6 25.6 25.6s25.6-11.4688 25.6-25.6l0-384c0-28.2112 22.9888-51.2 51.2-51.2s51.2 22.9888 51.2 51.2l0 384c0 14.1312 11.4688 25.6 25.6 25.6s25.6-11.4688 25.6-25.6l0-332.8c0-28.2112 22.9888-51.2 51.2-51.2s51.2 22.9888 51.2 51.2l0 384c0 14.1312 11.4688 25.6 25.6 25.6s25.6-11.4688 25.6-25.6l0-230.4c0-28.2112 22.9888-51.2 51.2-51.2s51.2 22.9888 51.2 51.2l0 179.2z"/></svg>"##;

/// 握紧小手（拖拽态，用户供图 2026-10-16：实心路径白填充 + 黑描边）。
const HAND_GRAB_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024"><path fill="#fff" stroke="#000" stroke-width="24" stroke-linejoin="round" d="M853.333333 213.333333a127.317333 127.317333 0 0 0-47.616 9.386667 126.336 126.336 0 0 0-185.813333-77.653333 127.274667 127.274667 0 0 0-215.808 0A126.592 126.592 0 0 0 341.333333 128a128 128 0 0 0-128 128v46.933333A213.717333 213.717333 0 0 0 42.666667 512v42.666667a384.426667 384.426667 0 0 0 384 384h170.666666a384.426667 384.426667 0 0 0 384-384V341.333333a128 128 0 0 0-128-128z m42.666667 341.333334a299.050667 299.050667 0 0 1-298.666667 298.666666h-170.666666a299.050667 299.050667 0 0 1-298.666667-298.666666v-42.666667a128.256 128.256 0 0 1 85.333333-120.704V426.666667a42.666667 42.666667 0 0 0 85.333334 0V256a42.666667 42.666667 0 0 1 85.333333 0v128a42.666667 42.666667 0 0 0 85.333333 0V213.333333a42.666667 42.666667 0 0 1 85.333334 0v170.666667a42.666667 42.666667 0 0 0 85.333333 0V256a42.666667 42.666667 0 0 1 85.333333 0v128a42.666667 42.666667 0 0 0 85.333334 0V341.333333a42.666667 42.666667 0 0 1 85.333333 0z"/></svg>"##;

const CURSOR_PX: i32 = 32;

/// 热点 = 图形最高一横（指尖）的中点，32×32 位图实测：伸展手 (17,0)、
/// 握拳 (16,2)。x 都落在图形中线附近，拖拽起手光标不横向跳。
const HOTSPOT_OPEN: (u32, u32) = (17, 0);
const HOTSPOT_GRAB: (u32, u32) = (16, 2);

/// 生成 grab 光标（`closed` = 握紧手）。SVG 解析/光栅化/GDI 任一步失败
/// 返回 None，调用方回退系统箭头。
pub(crate) fn load_hand_cursor(closed: bool) -> Option<HCURSOR> {
    let (svg, hot) = if closed {
        (HAND_GRAB_SVG, HOTSPOT_GRAB)
    } else {
        (HAND_OPEN_SVG, HOTSPOT_OPEN)
    };
    let pixmap = SvgRenderer::new(Arc::new(()))
        .render_pixmap(
            svg.as_bytes(),
            SvgSize::Size(Size::new(
                DevicePixels(CURSOR_PX),
                DevicePixels(CURSOR_PX),
            )),
        )
        .ok()?;
    create_cursor(&pixmap, hot.0, hot.1)
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
