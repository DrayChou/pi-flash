//! Windows 自绘 grab 手型光标（PF vendored patch，2026-10-16；图形 v2 白色版）：
//! Windows 系统光标集（IDC_*）只有指向手 IDC_HAND，没有 grab/grabbing 掌形
//! ——OpenHand/ClosedHand 原本落入箭头兜底（util.rs load_cursor 的 `_` 分支）。
//! 图形 = 用户供图 v2「白色版」实心手掌路径（iconfont 1024 视框，伸展掌 /
//! 握拳各一条），渲染就一层：**简单填充白色**（`fill="#fff"`，ZCode 同款做法，
//! 不描边不改形），gpui 自带 SvgRenderer 光栅化成 32×32，CreateIconIndirect
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

/// 伸展小手（悬停态，用户供图 v2 白色版 2026-10-16，实心路径原样白填充）。
const HAND_OPEN_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024"><path fill="#fff" d="M824.515155 243.164159c-16.249079 0-31.582299 4.389984-44.734854 12.033058l0-56.767911c0-49.345871-40.10337-89.468684-89.468684-89.468684-18.438954 0-35.519981 5.591345-49.800219 15.155165-12.236696-34.83539-45.433772-59.890019-84.405365-59.890019-38.968523 0-72.189136 25.054629-84.420715 59.890019-14.21884-9.56382-31.343869-15.155165-49.7818-15.155165-49.343825 0-89.467661 40.122813-89.467661 89.468684l0 329.831031-60.22157-104.365976c-11.664667-21.25407-30.735002-36.369326-53.626361-42.637076-22.321378-6.072299-45.541219-2.863211-65.399499 9.041934-40.582277 24.332175-56.048527 79.356372-34.444487 122.669854 1.333368 2.708692 29.879518 61.160965 118.979812 239.312434 41.960671 83.877339 88.004333 143.902434 136.845715 178.239474 38.334073 26.956955 64.831564 28.353768 69.746504 28.353768l223.674269 0c38.047548 0 73.438593-12.362562 105.236809-36.785811 29.794583-22.935362 55.745628-56.136531 76.933183-98.596576 41.765219-83.532485 63.869656-199.123107 63.869656-334.333578L914.029888 332.587817c0-49.342802-40.148395-89.469708-89.467661-89.469708L824.515155 243.164159zM869.250008 489.203808c0 128.264269-20.444635 236.908688-59.150168 314.276776-25.255197 50.504254-70.422909 110.702288-142.157697 110.702288l-223.234247 0c-1.726318-0.130983-20.576641-2.228761-48.974411-23.502273-28.308743-21.186532-71.795163-65.835428-118.017904-158.318772-90.669022-181.384094-118.541837-238.506069-118.803803-239.050468-0.041956-0.089028-0.041956-0.130983-0.085958-0.176009-11.228739-22.499433-3.276626-51.988048 17.388019-64.36903 9.304923-5.593392 20.180622-7.057743 30.645974-4.239558 11.138688 3.036149 20.488637 10.552334 26.253944 21.100574 0.10847 0.130983 0.198521 0.326435 0.284479 0.458441l69.829392 121.009028c14.264888 26.079982 30.319539 37.004799 47.730071 32.593326 17.452487-4.414543 26.253944-21.889543 26.253944-51.770084L377.211643 198.429305c0-24.64019 20.09671-44.734854 44.732807-44.734854 24.642237 0 44.738947 20.094664 44.738947 44.734854l0 290.774503c0 12.342096 10.021238 22.367427 22.366404 22.367427 12.341073 0 22.36538-10.025331 22.36538-22.367427L511.415181 153.694451c0-24.641213 20.09671-44.73383 44.735877-44.73383 24.638143 0 44.735877 20.092617 44.735877 44.73383l0 335.509357c0 12.342096 10.002818 22.367427 22.36538 22.367427 12.363585 0 22.367427-10.025331 22.367427-22.367427L645.619742 198.429305c0-24.64019 20.098757-44.734854 44.73383-44.734854 24.63712 0 44.735877 20.094664 44.735877 44.734854l0 335.509357c0 12.319583 10.001795 22.367427 22.367427 22.367427 12.363585 0 22.366404-10.047844 22.366404-22.367427L779.82328 332.632843c0-24.641213 20.09671-44.73383 44.735877-44.73383 24.635073 0 44.731784 20.092617 44.731784 44.73383l0 156.570965L869.250008 489.203808z"/></svg>"##;

/// 握紧小手（拖拽态，用户供图 v2 白色版 2026-10-16，实心路径原样白填充）。
const HAND_GRAB_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024"><path fill="#fff" d="M853.333333 213.333333a127.317333 127.317333 0 0 0-47.616 9.386667 126.336 126.336 0 0 0-185.813333-77.653333 127.274667 127.274667 0 0 0-215.808 0A126.592 126.592 0 0 0 341.333333 128a128 128 0 0 0-128 128v46.933333A213.717333 213.717333 0 0 0 42.666667 512v42.666667a384.426667 384.426667 0 0 0 384 384h170.666666a384.426667 384.426667 0 0 0 384-384V341.333333a128 128 0 0 0-128-128z m42.666667 341.333334a299.050667 299.050667 0 0 1-298.666667 298.666666h-170.666666a299.050667 299.050667 0 0 1-298.666667-298.666666v-42.666667a128.256 128.256 0 0 1 85.333333-120.704V426.666667a42.666667 42.666667 0 0 0 85.333334 0V256a42.666667 42.666667 0 0 1 85.333333 0v128a42.666667 42.666667 0 0 0 85.333333 0V213.333333a42.666667 42.666667 0 0 1 85.333334 0v170.666667a42.666667 42.666667 0 0 0 85.333333 0V256a42.666667 42.666667 0 0 1 85.333333 0v128a42.666667 42.666667 0 0 0 85.333334 0V341.333333a42.666667 42.666667 0 0 1 85.333333 0z"/></svg>"##;

const CURSOR_PX: i32 = 32;

/// 热点 = 图形最高一横（指尖）的中点，32×32 位图实测：伸展手 (17,2)、
/// 握拳 (16,3)。x 都落在图形中线附近，拖拽起手光标不横向跳。
const HOTSPOT_OPEN: (u32, u32) = (17, 2);
const HOTSPOT_GRAB: (u32, u32) = (16, 3);

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
