//! The window procedure of the settings window: colours, commands from
//! the controls, DPI changes and closing.

use std::cell::Cell;

use super::*;

fn color_control(hdc: HDC, text: COLORREF, background: COLORREF, brush: isize) -> LRESULT {
    // SAFETY: `hdc` is the device context the control is about to paint with.
    unsafe {
        SetTextColor(hdc, text);
        SetBkColor(hdc, background);
    }
    LRESULT(brush)
}

pub(super) extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let paint = PAINT.with(Cell::get);
    match (message, paint) {
        (WM_ERASEBKGND, Some(paint)) => {
            let mut rect = RECT::default();
            // SAFETY: fills our client area on the device context given.
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
                FillRect(
                    HDC(wparam.0 as *mut _),
                    &rect,
                    HBRUSH(paint.background as *mut _),
                );
            }
            return LRESULT(1);
        }
        (WM_CTLCOLORSTATIC | WM_CTLCOLORBTN, Some(paint)) => {
            // SAFETY: lParam of these messages is the control's handle.
            let id = unsafe {
                windows::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(HWND(lparam.0 as *mut _))
            } as u16;
            let palette = paint.palette;
            let text = match id {
                ID_DETAIL => palette.secondary,
                ID_MESSAGE => palette.error,
                _ => palette.text,
            };
            return color_control(
                HDC(wparam.0 as *mut _),
                text,
                palette.background,
                paint.background,
            );
        }
        (WM_CTLCOLOREDIT, Some(paint)) => {
            let palette = paint.palette;
            return color_control(
                HDC(wparam.0 as *mut _),
                palette.text,
                palette.field,
                paint.field,
            );
        }
        (WM_NOTIFY, Some(paint)) => {
            // SAFETY: for WM_NOTIFY, lParam points to an NMHDR, and to an
            // NMCUSTOMDRAW when the code is NM_CUSTOMDRAW.
            let header = unsafe { &*(lparam.0 as *const NMHDR) };
            if header.code == NM_CUSTOMDRAW && header.idFrom == usize::from(ID_START) {
                // SAFETY: NM_CUSTOMDRAW comes with an NMCUSTOMDRAW, as said above.
                let draw = unsafe { &*(lparam.0 as *const NMCUSTOMDRAW) };
                return theme::draw_checkbox(
                    draw,
                    &paint.palette,
                    HBRUSH(paint.background as *mut _),
                );
            }
        }
        _ => {}
    }
    match message {
        WM_COMMAND => {
            on_command(hwnd, wparam, lparam);
            return LRESULT(0);
        }
        WM_DPICHANGED => {
            // SAFETY: for WM_DPICHANGED, lParam points to the suggested rect.
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            // SAFETY: moves our own window where Windows suggests.
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            report(SettingsEvent::DpiChanged(u32::from(wparam.0 as u16)));
            return LRESULT(0);
        }
        WM_CLOSE => {
            // SAFETY: the user closes our window.
            let _ = unsafe { DestroyWindow(hwnd) };
            return LRESULT(0);
        }
        WM_DESTROY => {
            DESTROYED.with(|cell| cell.set(true));
            if !SILENT.with(Cell::get) {
                report(SettingsEvent::Closed);
            }
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: default handling for every other message.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn on_command(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let id = wparam.0 as u16;
    let code = (wparam.0 >> 16) as u32 & 0xFFFF;
    let control = HWND(lparam.0 as *mut _);
    if i32::from(id) == IDCANCEL.0 {
        // SAFETY: Escape closes our window, as in a dialog.
        let _ = unsafe { DestroyWindow(hwnd) };
        return;
    }
    let sector = id & !KIND_MASK;
    match (id & KIND_MASK, code) {
        (ID_NAME, EN_CHANGE) if !UPDATING.with(Cell::get) => report(SettingsEvent::Rename {
            sector,
            text: window_text(control),
        }),
        (ID_FIELD, BN_CLICKED) => {
            if let Some(texts) = TEXTS.with(Cell::get)
                && shortcut_field::listen(control, sector, texts)
            {
                report_field(FieldEvent::Listening);
            }
        }
        (ID_CLEAR, BN_CLICKED) => report(SettingsEvent::Clear(sector)),
        _ if id == ID_START && code == BN_CLICKED => {
            // SAFETY: reads the state of our own checkbox.
            let checked = unsafe { SendMessageW(control, BM_GETCHECK, None, None) }.0 == 1;
            report(SettingsEvent::StartWithWindows(checked));
        }
        _ => {}
    }
}
