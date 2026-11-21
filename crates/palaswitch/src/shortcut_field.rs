//! The shortcut field of a profile: a standard push button that, once
//! pressed, listens to the keyboard until a shortcut is typed.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    DLGC_WANTALLKEYS, GetNextDlgTabItem, GetParent, SetWindowTextW, WM_CHAR, WM_GETDLGCODE,
    WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_NCDESTROY, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::HSTRING;

use crate::capture::{Capture, Step};
use crate::shortcut::{Mods, mods_shown};
use crate::text::Texts;

const SUBCLASS_ID: usize = 1;

/// The field listening for a shortcut, if any.
#[derive(Debug, Clone, Copy)]
struct Listening {
    field: isize,
    sector: u16,
    capture: Capture,
}

thread_local! {
    static LISTENING: Cell<Option<Listening>> = const { Cell::new(None) };
    /// Set once a shortcut is typed, until its key is released: the
    /// character it produces must not reach the button (a beep, or the
    /// system menu after Alt+Space).
    static SWALLOW_CHARS: Cell<bool> = const { Cell::new(false) };
}

/// What a field tells its window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldEvent {
    /// The field listens: shortcuts of the app must let keys through.
    Listening,
    Typed(u16, crate::shortcut::Shortcut),
    /// Listening stopped without a shortcut: the field shows its value again.
    Stopped(u16),
}

/// Makes `field` a shortcut field; `texts` gives what it shows while
/// listening, `report` receives its events.
pub fn attach(field: HWND, texts: &'static Texts, report: fn(FieldEvent)) {
    let data = Box::into_raw(Box::new(FieldData { texts, report })) as usize;
    // SAFETY: `field` is a button of ours; `data` is freed on WM_NCDESTROY.
    let attached = unsafe { SetWindowSubclass(field, Some(field_proc), SUBCLASS_ID, data) };
    if !attached.as_bool() {
        // SAFETY: the box was not handed to Windows.
        drop(unsafe { Box::from_raw(data as *mut FieldData) });
        tracing::error!("could not make a shortcut field");
    }
}

struct FieldData {
    texts: &'static Texts,
    report: fn(FieldEvent),
}

/// Starts listening on `field` for the profile at `sector`.
pub fn listen(field: HWND, sector: u16, texts: &Texts) -> bool {
    if is_listening(field) {
        return false;
    }
    stop_listening();
    LISTENING.with(|cell| {
        cell.set(Some(Listening {
            field: field.0 as isize,
            sector,
            capture: Capture::holding(held_modifiers()),
        }));
    });
    set_text(field, texts.shortcut_listening);
    true
}

/// Modifiers down right now, as read from this thread's keyboard state.
fn held_modifiers() -> Mods {
    // SAFETY: reads the key state of this thread's input.
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    Mods {
        ctrl: down(VK_CONTROL.0),
        alt: down(VK_MENU.0),
        shift: down(VK_SHIFT.0),
        win: down(VK_LWIN.0) || down(VK_RWIN.0),
    }
}

pub fn is_listening(field: HWND) -> bool {
    LISTENING.with(|cell| {
        cell.get()
            .is_some_and(|listening| listening.field == field.0 as isize)
    })
}

/// Stops listening, if a field was. Returns the profile it listened for.
pub fn stop_listening() -> Option<u16> {
    LISTENING
        .with(|cell| cell.take())
        .map(|listening| listening.sector)
}

fn set_text(field: HWND, text: &str) {
    // SAFETY: `field` is a control of ours; the text outlives the call.
    let _ = unsafe { SetWindowTextW(field, &HSTRING::from(text)) };
}

extern "system" fn field_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        if is_listening(hwnd) {
            stop_listening();
        }
        // SAFETY: removes our own subclass, then frees its data once.
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(field_proc), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut FieldData));
            return DefSubclassProc(hwnd, message, wparam, lparam);
        }
    }
    // SAFETY: `data` is the FieldData given in `attach`, alive until the
    // WM_NCDESTROY handled above.
    let field = unsafe { &*(data as *const FieldData) };
    let listening = LISTENING
        .with(Cell::get)
        .filter(|listening| listening.field == hwnd.0 as isize);
    let Some(mut listening) = listening else {
        if SWALLOW_CHARS.with(Cell::get) {
            match message {
                WM_CHAR | WM_SYSCHAR => return LRESULT(0),
                WM_KEYUP | WM_SYSKEYUP => SWALLOW_CHARS.with(|cell| cell.set(false)),
                _ => {}
            }
        }
        // SAFETY: default handling of a field that is not listening.
        return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    };
    let vk = wparam.0 as u16;
    let step = match message {
        WM_GETDLGCODE => return LRESULT(DLGC_WANTALLKEYS as isize),
        WM_KEYDOWN | WM_SYSKEYDOWN => listening.capture.key_down(vk),
        WM_KEYUP | WM_SYSKEYUP => listening.capture.key_up(vk),
        // Typed characters would beep or press the button.
        WM_CHAR | WM_SYSCHAR => return LRESULT(0),
        WM_KILLFOCUS => {
            stop_listening();
            (field.report)(FieldEvent::Stopped(listening.sector));
            // SAFETY: the button still needs to redraw without its focus.
            return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        }
        // SAFETY: anything else is handled as by any button.
        _ => return unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    };
    match step {
        Step::Holding(mods) => {
            LISTENING.with(|cell| cell.set(Some(listening)));
            let mut parts = mods_shown(mods, field.texts);
            let text = if parts.is_empty() {
                field.texts.shortcut_listening.to_owned()
            } else {
                parts.push("…".to_owned());
                parts.join("+")
            };
            set_text(hwnd, &text);
        }
        Step::Done(shortcut) => {
            stop_listening();
            SWALLOW_CHARS.with(|cell| cell.set(true));
            (field.report)(FieldEvent::Typed(listening.sector, shortcut));
        }
        Step::Cancelled => {
            stop_listening();
            (field.report)(FieldEvent::Stopped(listening.sector));
        }
        Step::Left => {
            stop_listening();
            (field.report)(FieldEvent::Stopped(listening.sector));
            // SAFETY: reads the Shift key state of this thread's input.
            let backwards = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
            // SAFETY: moves the focus to the next or previous control of
            // our window, as Tab does; the window is not a dialog, so the
            // dialog manager would not do it.
            unsafe {
                if let Ok(parent) = GetParent(hwnd)
                    && let Ok(next) = GetNextDlgTabItem(parent, Some(hwnd), backwards)
                {
                    let _ = SetFocus(Some(next));
                }
            }
        }
        Step::Ignored => {
            LISTENING.with(|cell| cell.set(Some(listening)));
        }
    }
    LRESULT(0)
}
