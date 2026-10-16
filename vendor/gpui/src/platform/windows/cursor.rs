//! Windows 自绘 grab 手型光标（PF vendored patch，2026-10-16；同日返工）：
//! Windows 系统光标集（IDC_*）只有指向手 IDC_HAND，没有 grab/grabbing 掌形
//! ——OpenHand/ClosedHand 原本落入箭头兜底（util.rs load_cursor 的 `_` 分支）。
//! 这里用内嵌 SVG（lucide `hand` / `hand-grab`，用户定稿 2026-10-16：项目行
//! 悬停 = 伸展小手、拖拽 = 握紧小手）经 gpui 自带的 SvgRenderer 光栅化成
//! 32×32 位图，CreateIconIndirect 生成 HCURSOR，进程级缓存、永不销毁。
//!
//! 观感对齐系统光标两条（返工，用户实机反馈「样式古怪、和别的不一致」）：
//! 1. **白身黑边（实心）**：lucide 是描边轮廓画法，只描边得到的是空心线稿
//!    手——棕榈心、指缝全透底，与系统光标（实心白身 + 黑边）不是一个画风。
//!    做法 = 双层描边（底层 4.2 黑粗边 + 顶层 1.9 白细芯）之后，把轮廓围出
//!    的内部透明区洪泛填成不透明白（[`solidify`]）。
//! 2. **不许裁边**：图形本就画到 24 视框的 2..22，4.2 宽黑边再外扩 2.1 →
//!    原位图上下缘被黑边顶满裁平。视框加 3 单位内边距（`-3 -3 30 30`），整手
//!    带边距落进 32×32（图形 footprint ≈26px，与系统指向手占位相当）。

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
/// 视框 = 24 图形 + 3 单位内边距（见文件头）；双层描边：底层 4.2 黑边、
/// 顶层 1.9 白芯（24 视框下 ≈1.2px 黑边），内部再实心化。
const HAND_OPEN_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30" viewBox="-3 -3 30 30" fill="none" stroke-linecap="round" stroke-linejoin="round"><g stroke="#000" stroke-width="4.2"><path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8"/><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"/></g><g stroke="#fff" stroke-width="1.9"><path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8"/><path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15"/></g></svg>"##;

/// lucide `hand-grab`（拖拽：握紧小手，用户供图 2026-10-16）。
const HAND_GRAB_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30" viewBox="-3 -3 30 30" fill="none" stroke-linecap="round" stroke-linejoin="round"><g stroke="#000" stroke-width="4.2"><path d="M18 11.5V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v1.4"/><path d="M14 10V8a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 9.9V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v5"/><path d="M6 14a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M18 11a2 2 0 1 1 4 0v3a8 8 0 0 1-8 8h-4a8 8 0 0 1-8-8 2 2 0 1 1 4 0"/></g><g stroke="#fff" stroke-width="1.9"><path d="M18 11.5V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v1.4"/><path d="M14 10V8a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2"/><path d="M10 9.9V9a2 2 0 0 0-2-2a2 2 0 0 0-2 2v5"/><path d="M6 14a2 2 0 0 0-2-2a2 2 0 0 0-2 2"/><path d="M18 11a2 2 0 1 1 4 0v3a8 8 0 0 1-8 8h-4a8 8 0 0 1-8-8 2 2 0 1 1 4 0"/></g></svg>"##;

const CURSOR_PX: i32 = 32;

/// 热点 = 图形最高一横（指尖）的中点，32×32 位图实测值：伸展手 = 中指
/// 指尖 (16,3)、握紧手 = 拳顶指节 (16,7)。两手 x 一致（拖拽起手时光标不
/// 横向跳），y 各随自己图形顶点（拳比掌矮）。lucide 图形本就左右对称居中
/// 画在视框里，x=16 即图形中线。
const HOTSPOT_OPEN: (u32, u32) = (16, 3);
const HOTSPOT_GRAB: (u32, u32) = (16, 7);

/// 生成 grab 光标（`closed` = 握紧手）。SVG 解析/光栅化/GDI 任一步失败
/// 返回 None，调用方回退系统箭头。
pub(crate) fn load_hand_cursor(closed: bool) -> Option<HCURSOR> {
    let (svg, hot) = if closed {
        (HAND_GRAB_SVG, HOTSPOT_GRAB)
    } else {
        (HAND_OPEN_SVG, HOTSPOT_OPEN)
    };
    let mut pixmap = SvgRenderer::new(Arc::new(()))
        .render_pixmap(
            svg.as_bytes(),
            SvgSize::Size(Size::new(
                DevicePixels(CURSOR_PX),
                DevicePixels(CURSOR_PX),
            )),
        )
        .ok()?;
    solidify(&mut pixmap);
    create_cursor(&pixmap, hot.0, hot.1)
}

/// 实心化（白身）：位图里「轮廓围出的内部」= 从四边洪泛够不着的透明像素
/// ——4 邻域洪泛标记外部可达的透明像素，剩下的透明像素即内部，填成不透明
/// 白（位图是预乘 RGBA，白的预乘值 = (255,255,255,255)）。描边 AA 边缘
/// （alpha ≥ [`ALPHA`]）不参与洪泛，白身与黑边之间仍是平滑过渡。
///
/// 兜底：内部像素数超上限（上游换图后轮廓出现缺口 → 洪泛灌满整图）就整个
/// 放弃填充，保留描边形态——宁可空心也不能出「黑方块」光标。
fn solidify(pixmap: &mut Pixmap) {
    /// 低于此 alpha 视为「透明」（描边 AA 边缘不参与洪泛）。
    const ALPHA: u8 = 110;
    /// 32×32 上手身内部 ≈90px；给到 400 足够宽松，再多必是轮廓漏了。
    const MAX_FILL: usize = 400;

    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let data = pixmap.data_mut();
    let transparent = |ix: usize| data[ix * 4 + 3] < ALPHA;
    let mut outside = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let mut seed = |ix: usize, outside: &mut Vec<bool>, stack: &mut Vec<usize>| {
        if !outside[ix] && data[ix * 4 + 3] < ALPHA {
            outside[ix] = true;
            stack.push(ix);
        }
    };
    for x in 0..w {
        seed(x, &mut outside, &mut stack);
        seed((h - 1) * w + x, &mut outside, &mut stack);
    }
    for y in 0..h {
        seed(y * w, &mut outside, &mut stack);
        seed(y * w + w - 1, &mut outside, &mut stack);
    }
    while let Some(ix) = stack.pop() {
        let (x, y) = (ix % w, ix / w);
        let mut visit = |n: usize, outside: &mut Vec<bool>, stack: &mut Vec<usize>| {
            if !outside[n] && data[n * 4 + 3] < ALPHA {
                outside[n] = true;
                stack.push(n);
            }
        };
        if x > 0 {
            visit(ix - 1, &mut outside, &mut stack);
        }
        if x + 1 < w {
            visit(ix + 1, &mut outside, &mut stack);
        }
        if y > 0 {
            visit(ix - w, &mut outside, &mut stack);
        }
        if y + 1 < h {
            visit(ix + w, &mut outside, &mut stack);
        }
    }
    let interior: Vec<usize> = (0..w * h)
        .filter(|&ix| !outside[ix] && transparent(ix))
        .collect();
    if interior.len() > MAX_FILL {
        return;
    }
    for ix in interior {
        data[ix * 4..ix * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
    }
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
