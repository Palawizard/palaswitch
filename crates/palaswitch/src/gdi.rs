//! Draws text with the Windows UI font, for the tray icon.

use std::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FW_BOLD, GdiFlush,
    HDC, HGDIOBJ, OUT_TT_PRECIS, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::core::w;

use crate::icon::{Coverage, TextRenderer};

/// Font size to start from, as a share of the icon size; it shrinks until
/// the text fits.
const START_EM: f32 = 1.5;
const SHRINK: f32 = 0.92;

/// Bold Segoe UI, grey anti-aliasing (no ClearType colour fringes).
pub struct SystemFont;

impl TextRenderer for SystemFont {
    fn render(&self, text: &str, size: usize) -> Coverage {
        let mut em = size as f32 * START_EM;
        while em >= 4.0 {
            match draw(text, em.round() as i32, size * 2).and_then(|canvas| canvas.centred(size)) {
                Some(coverage) => return coverage,
                None => em *= SHRINK,
            }
        }
        tracing::warn!(text, size, "could not draw the icon text");
        Coverage::empty(size, size)
    }
}

/// Text centred on a `canvas` pixels square bitmap, white on black, read
/// back as grey levels.
fn draw(text: &str, em: i32, canvas: usize) -> Option<Coverage> {
    let side = i32::try_from(canvas).ok()?;
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: side,
            // Negative height: rows top to bottom.
            biHeight: -side,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut text: Vec<u16> = text.encode_utf16().collect();
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: a memory DC needs no window; it is deleted below.
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return None;
    }
    let dc = DeleteDcOnDrop(dc);
    // SAFETY: `info` describes a 32-bit top-down bitmap and `bits` receives
    // its pixels, which stay valid until the bitmap is deleted.
    let bitmap =
        unsafe { CreateDIBSection(Some(dc.0), &info, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    let bitmap = DeleteObjectOnDrop(bitmap.into());
    // SAFETY: plain font creation from constant arguments.
    let font = unsafe {
        CreateFontW(
            -em,
            0,
            0,
            0,
            FW_BOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            0,
            w!("Segoe UI"),
        )
    };
    if font.is_invalid() {
        return None;
    }
    let font = DeleteObjectOnDrop(font.into());
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: side,
        bottom: side,
    };
    // SAFETY: `dc`, `bitmap` and `font` are valid until the end of this
    // function; the previous selections are put back before they are
    // deleted.
    let levels = unsafe {
        let old_bitmap = SelectObject(dc.0, bitmap.0);
        let old_font = SelectObject(dc.0, font.0);
        SetTextColor(dc.0, COLORREF(0x00FF_FFFF));
        SetBkMode(dc.0, TRANSPARENT);
        let drawn = DrawTextW(
            dc.0,
            &mut text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let _ = GdiFlush();
        let pixels = std::slice::from_raw_parts(bits.cast::<[u8; 4]>(), canvas * canvas);
        // Blue, green, red, unused: the text is grey, so green says it all.
        let levels = pixels.iter().map(|pixel| pixel[1]).collect();
        SelectObject(dc.0, old_font);
        SelectObject(dc.0, old_bitmap);
        (drawn != 0).then_some(levels)
    }?;
    Some(Coverage {
        width: canvas,
        height: canvas,
        levels,
    })
}

struct DeleteDcOnDrop(HDC);

impl Drop for DeleteDcOnDrop {
    fn drop(&mut self) {
        // SAFETY: the DC was created by CreateCompatibleDC and is not used after.
        let _ = unsafe { DeleteDC(self.0) };
    }
}

struct DeleteObjectOnDrop(HGDIOBJ);

impl Drop for DeleteObjectOnDrop {
    fn drop(&mut self) {
        // SAFETY: the object is no longer selected in any DC.
        let _ = unsafe { DeleteObject(self.0) };
    }
}
