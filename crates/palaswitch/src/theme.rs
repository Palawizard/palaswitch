//! Colours, fonts and dark mode of the settings window. Standard controls
//! keep their Windows look; in dark mode they get the dark visual styles
//! Windows uses for its own windows.

use std::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, HWND, LRESULT, RECT, SIZE};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_WINDOW, COLOR_WINDOWTEXT, CreateFontIndirectW, CreateSolidBrush, DT_LEFT,
    DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawFocusRect, DrawTextW, FW_SEMIBOLD, FillRect,
    GetSysColor, HBRUSH, HDC, HFONT, HGDIOBJ, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::{
    BP_CHECKBOX, CBS_CHECKEDHOT, CBS_CHECKEDNORMAL, CBS_CHECKEDPRESSED, CBS_UNCHECKEDHOT,
    CBS_UNCHECKEDNORMAL, CBS_UNCHECKEDPRESSED, CDDS_PREPAINT, CDIS_FOCUS, CDIS_HOT, CDIS_SELECTED,
    CDRF_DODEFAULT, CDRF_SKIPDEFAULT, CloseThemeData, DrawThemeBackground, GetThemePartSize,
    NMCUSTOMDRAW, OpenThemeData, SetWindowTheme, TS_DRAW,
};
use windows::Win32::UI::HiDpi::SystemParametersInfoForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, NONCLIENTMETRICSW, SPI_GETHIGHCONTRAST, SPI_GETNONCLIENTMETRICS,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageW, SystemParametersInfoW, UISF_HIDEFOCUS,
    WM_QUERYUISTATE,
};
use windows::core::{BOOL, PCWSTR, w};

/// Colours of the window, picked from the Windows settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub high_contrast: bool,
    pub background: COLORREF,
    pub text: COLORREF,
    /// Detail lines; still at least 4.5:1 against the background.
    pub secondary: COLORREF,
    pub field: COLORREF,
    /// Messages about a refused shortcut.
    pub error: COLORREF,
}

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

impl Palette {
    pub fn current() -> Self {
        // SAFETY: plain queries of system colours.
        let system = |index| COLORREF(unsafe { GetSysColor(index) });
        if high_contrast() {
            let text = system(COLOR_WINDOWTEXT);
            return Self {
                dark: false,
                high_contrast: true,
                background: system(COLOR_WINDOW),
                text,
                secondary: text,
                field: system(COLOR_WINDOW),
                error: text,
            };
        }
        if apps_use_dark_theme() {
            Self {
                dark: true,
                high_contrast: false,
                background: rgb(0x20, 0x20, 0x20),
                text: rgb(0xFF, 0xFF, 0xFF),
                secondary: rgb(0xC8, 0xC8, 0xC8),
                field: rgb(0x2D, 0x2D, 0x2D),
                error: rgb(0xFF, 0x99, 0xA4),
            }
        } else {
            Self {
                dark: false,
                high_contrast: false,
                background: system(COLOR_BTNFACE),
                text: system(COLOR_WINDOWTEXT),
                secondary: rgb(0x5C, 0x5C, 0x5C),
                field: system(COLOR_WINDOW),
                error: rgb(0xC4, 0x2B, 0x1C),
            }
        }
    }
}

fn high_contrast() -> bool {
    let mut info = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a HIGHCONTRASTW with its size set, as the call needs.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some((&raw mut info).cast::<c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && info.dwFlags.contains(HCF_HIGHCONTRASTON)
}

/// Whether apps should use the dark theme; light when the setting is missing.
fn apps_use_dark_theme() -> bool {
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` and `size` describe a DWORD buffer that outlives the call.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast::<c_void>()),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 0
}

/// Brushes of a palette, deleted with it.
pub struct Brushes {
    pub background: HBRUSH,
    pub field: HBRUSH,
}

impl Brushes {
    pub fn new(palette: &Palette) -> Self {
        // SAFETY: solid brushes from plain colours, deleted on drop.
        unsafe {
            Self {
                background: CreateSolidBrush(palette.background),
                field: CreateSolidBrush(palette.field),
            }
        }
    }
}

impl Drop for Brushes {
    fn drop(&mut self) {
        for brush in [self.background, self.field] {
            // SAFETY: brushes created by `new` and no longer selected anywhere.
            let _ = unsafe { DeleteObject(HGDIOBJ(brush.0)) };
        }
    }
}

/// Dark or light title bar, as the rest of the window.
pub fn title_bar(hwnd: HWND, dark: bool) {
    let value = BOOL::from(dark);
    // SAFETY: `value` is the BOOL this attribute takes and outlives the call.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const value).cast::<c_void>(),
            size_of::<BOOL>() as u32,
        )
    };
    if let Err(error) = result {
        tracing::debug!(%error, "no dark title bar");
    }
}

/// The kind of standard control, for its dark visual style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    Button,
    Edit,
}

/// Gives a control the dark visual style Windows uses in its own dark
/// windows, or the default one.
pub fn style_control(hwnd: HWND, kind: ControlKind, dark: bool) {
    let name = match (kind, dark) {
        (ControlKind::Button, true) => w!("DarkMode_Explorer"),
        (ControlKind::Edit, true) => w!("DarkMode_CFD"),
        (_, false) => w!("Explorer"),
    };
    // SAFETY: `hwnd` is a control of ours; the name is a static string.
    if let Err(error) = unsafe { SetWindowTheme(hwnd, name, PCWSTR::null()) } {
        tracing::debug!(%error, "could not style a control");
    }
}

/// Fonts of the window at one DPI, deleted with it.
pub struct Fonts {
    pub body: HFONT,
    pub title: HFONT,
}

impl Fonts {
    /// The Windows message font, and a larger semibold one for the title.
    pub fn new(dpi: u32) -> Self {
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        // SAFETY: `metrics` has its size set and outlives the call.
        let read = unsafe {
            SystemParametersInfoForDpi(
                SPI_GETNONCLIENTMETRICS.0,
                metrics.cbSize,
                Some((&raw mut metrics).cast::<c_void>()),
                0,
                dpi,
            )
        };
        if let Err(error) = read {
            tracing::warn!(%error, "no system font metrics");
        }
        let body = metrics.lfMessageFont;
        let mut title = body;
        title.lfHeight = body.lfHeight * 4 / 3;
        title.lfWeight = FW_SEMIBOLD.0 as i32;
        // SAFETY: fonts from filled LOGFONTW values, deleted on drop.
        unsafe {
            Self {
                body: CreateFontIndirectW(&body),
                title: CreateFontIndirectW(&title),
            }
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for font in [self.body, self.title] {
            // SAFETY: fonts created by `new`; the window that used them is
            // gone or has been given new ones.
            let _ = unsafe { DeleteObject(HGDIOBJ(font.0)) };
        }
    }
}

/// Draws a checkbox in dark mode, where the themed checkbox would keep a
/// black label: the themed box, then the label in the palette's colour.
/// Returns the custom draw answer.
pub fn draw_checkbox(draw: &NMCUSTOMDRAW, palette: &Palette, background: HBRUSH) -> LRESULT {
    if !palette.dark || draw.dwDrawStage != CDDS_PREPAINT {
        return LRESULT(CDRF_DODEFAULT as isize);
    }
    let hwnd = draw.hdr.hwndFrom;
    let hdc = draw.hdc;
    let rect = draw.rc;
    // SAFETY: `hwnd` is the checkbox sending this notification and `hdc`
    // its device context for the duration of the paint.
    unsafe {
        FillRect(hdc, &rect, background);
        let checked = SendMessageW(hwnd, BM_GETCHECK, None, None).0 == 1;
        let state = draw.uItemState;
        let state_id = match (
            checked,
            state.contains(CDIS_SELECTED),
            state.contains(CDIS_HOT),
        ) {
            (true, true, _) => CBS_CHECKEDPRESSED,
            (true, false, true) => CBS_CHECKEDHOT,
            (true, false, false) => CBS_CHECKEDNORMAL,
            (false, true, _) => CBS_UNCHECKEDPRESSED,
            (false, false, true) => CBS_UNCHECKEDHOT,
            (false, false, false) => CBS_UNCHECKEDNORMAL,
        };
        let theme = OpenThemeData(Some(hwnd), w!("Button"));
        let size = GetThemePartSize(theme, Some(hdc), BP_CHECKBOX.0, state_id.0, None, TS_DRAW)
            .unwrap_or(SIZE { cx: 13, cy: 13 });
        let top = rect.top + (rect.bottom - rect.top - size.cy) / 2;
        let box_rect = RECT {
            left: rect.left,
            top,
            right: rect.left + size.cx,
            bottom: top + size.cy,
        };
        let _ = DrawThemeBackground(theme, hdc, BP_CHECKBOX.0, state_id.0, &box_rect, None);
        let _ = CloseThemeData(theme);
        draw_label(
            hwnd,
            hdc,
            &rect,
            size.cx,
            palette,
            state.contains(CDIS_FOCUS),
        );
    }
    LRESULT(CDRF_SKIPDEFAULT as isize)
}

/// # Safety
/// `hdc` must be the device context of `hwnd` during its paint.
unsafe fn draw_label(
    hwnd: HWND,
    hdc: HDC,
    rect: &RECT,
    box_width: i32,
    palette: &Palette,
    focused: bool,
) {
    let mut text = [0u16; 128];
    // SAFETY: `text` is a writable buffer whose length is passed along.
    let len = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut text) };
    let mut label = text[..len.max(0) as usize].to_vec();
    let gap = box_width / 3;
    let mut text_rect = RECT {
        left: rect.left + box_width + gap,
        ..*rect
    };
    // SAFETY: `hdc` is valid during the paint, as the caller guarantees.
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, palette.text);
        DrawTextW(
            hdc,
            &mut label,
            &mut text_rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        let ui_state = SendMessageW(hwnd, WM_QUERYUISTATE, None, None).0 as u32;
        if focused && ui_state & UISF_HIDEFOCUS == 0 {
            let _ = DrawFocusRect(hdc, &text_rect);
        }
    }
}
