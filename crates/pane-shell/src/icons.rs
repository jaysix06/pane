//! File icons as RGBA pixels, from the shell (the same icons Explorer shows).

use std::path::Path;

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HGDIOBJ,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::core::PCWSTR;

use crate::win::wide;

/// An icon in straight (not premultiplied) RGBA.
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The shell's icon for `path` at about `size` pixels. Needs COM on the
/// calling thread ([`crate::init_com`]).
pub fn icon_rgba(path: &Path, size: u32) -> Option<Icon> {
    let name = wide(path);
    // SAFETY: COM calls on this thread's apartment; the bitmap is deleted below.
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).ok()?;
        let side = size as i32;
        let hbitmap = factory
            .GetImage(SIZE { cx: side, cy: side }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
            .ok()?;
        let result = bitmap_pixels(hbitmap);
        let _ = DeleteObject(HGDIOBJ(hbitmap.0));
        result
    }
}

unsafe fn bitmap_pixels(hbitmap: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<Icon> {
    let mut bm = BITMAP::default();
    // SAFETY: `bm` is sized for a BITMAP.
    let got = unsafe { GetObjectW(HGDIOBJ(hbitmap.0), size_of::<BITMAP>() as i32, Some((&raw mut bm).cast())) };
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    // SAFETY: a memory DC for GetDIBits, deleted after; the buffer is w*h*4.
    let rows = unsafe {
        let dc = CreateCompatibleDC(None);
        let rows = GetDIBits(dc, hbitmap, 0, h, Some(bgra.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        rows
    };
    if rows == 0 {
        return None;
    }
    // Premultiplied BGRA to straight RGBA. Icons without alpha (all zero)
    // are opaque.
    let any_alpha = bgra.chunks_exact(4).any(|p| p[3] != 0);
    for p in bgra.chunks_exact_mut(4) {
        let (b, g, r, a) = (p[0], p[1], p[2], if any_alpha { p[3] } else { 255 });
        let un = |c: u8| if a == 0 || a == 255 { c } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        p[0] = un(r);
        p[1] = un(g);
        p[2] = un(b);
        p[3] = a;
    }
    Some(Icon { width: w, height: h, rgba: bgra })
}
