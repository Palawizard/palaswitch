//! Win32 plumbing of the tray app: a hidden window that receives device and
//! theme changes and wake-ups from other threads, the single-instance lock,
//! failure notifications and a few system settings.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;

use windows::Win32::Devices::HumanInterfaceDevice::GUID_DEVINTERFACE_HID;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_INFO, NIIF_WARNING, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE,
    DBT_DEVTYP_DEVICEINTERFACE, DEV_BROADCAST_DEVICEINTERFACE_W, DEVICE_NOTIFY_WINDOW_HANDLE,
    DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW,
    GetWindowThreadProcessId, HDEVNOTIFY, IsDialogMessageW, MSG, PBT_APMRESUMEAUTOMATIC,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterDeviceNotificationW, SM_CXSMICON,
    TranslateMessage, UnregisterDeviceNotification, WM_APP, WM_CLOSE, WM_DEVICECHANGE,
    WM_POWERBROADCAST, WM_SETTINGCHANGE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::settings_window::SettingsEvent;

const CLASS_NAME: PCWSTR = w!("palaswitch.hidden");
const INSTANCE_MUTEX: PCWSTR = w!("Local\\palaswitch.instance");
const WM_APP_WAKE: u32 = WM_APP + 1;
const WM_APP_DEVICES: u32 = WM_APP + 2;
const WM_APP_THEME: u32 = WM_APP + 3;
const WM_APP_ANOTHER_INSTANCE: u32 = WM_APP + 4;
/// `uID` of our tray icon. tray-icon 0.26.1 (pinned in Cargo.toml) takes it
/// from a process-wide counter starting at 1, which `TrayIconBuilder::new`
/// uses once for the icon's own id before `build` takes the next value for
/// the `uID`. [`check_tray_icon`] says at start-up if this ever changes.
const TRAY_ICON_ID: u32 = 2;

/// What the hidden window and the settings window tell the interface thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowEvent {
    /// Another thread has something waiting for the interface thread.
    Wake,
    DevicesChanged,
    /// The taskbar may have switched between light and dark.
    ThemeChanged,
    /// The app was launched again while running.
    AnotherInstance,
    Settings(SettingsEvent),
}

type Handler = Box<dyn FnMut(WindowEvent)>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = RefCell::new(None);
    /// Events that arrived while the handler was running, handled once it
    /// returns.
    static PENDING: RefCell<VecDeque<WindowEvent>> = const { RefCell::new(VecDeque::new()) };
    /// The settings window, whose controls the message loop moves between
    /// with Tab, as in a dialog.
    static DIALOG: Cell<isize> = const { Cell::new(0) };
}

pub fn set_dialog(hwnd: Option<HWND>) {
    DIALOG.with(|cell| cell.set(hwnd.map_or(0, |hwnd| hwnd.0 as isize)));
}

/// Top-level window that is never shown: unlike a message-only window, it
/// gets broadcasts such as theme changes, and another instance can find it.
pub struct HiddenWindow {
    hwnd: HWND,
    devices: Option<HDEVNOTIFY>,
}

impl HiddenWindow {
    /// Creates the window on the current thread; `handler` runs on it for
    /// each event, from the message loop.
    pub fn create(handler: impl FnMut(WindowEvent) + 'static) -> windows::core::Result<Self> {
        HANDLER.with(|cell| *cell.borrow_mut() = Some(Box::new(handler)));
        // SAFETY: null asks for the module of the running executable.
        let instance = unsafe { GetModuleHandleW(None) }?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        // SAFETY: `class` points to a static name and a valid window procedure.
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(windows::core::Error::from_thread());
        }
        // SAFETY: the class was registered above; the window has no parent
        // and is never shown.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                CLASS_NAME,
                w!("palaswitch"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }?;
        let devices = register_hid_notifications(hwnd);
        Ok(Self { hwnd, devices })
    }

    /// Lets any thread wake the interface thread.
    pub fn doorbell(&self) -> Doorbell {
        Doorbell(self.hwnd.0 as isize)
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        if let Some(devices) = self.devices.take() {
            // SAFETY: the handle came from RegisterDeviceNotificationW.
            let _ = unsafe { UnregisterDeviceNotification(devices) };
        }
        // SAFETY: our window, destroyed from the thread that created it.
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

fn register_hid_notifications(hwnd: HWND) -> Option<HDEVNOTIFY> {
    let filter = DEV_BROADCAST_DEVICEINTERFACE_W {
        dbcc_size: size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
        dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
        dbcc_classguid: GUID_DEVINTERFACE_HID,
        ..Default::default()
    };
    // SAFETY: `filter` is a filled device interface filter that outlives
    // the call; `hwnd` is our window.
    let result = unsafe {
        RegisterDeviceNotificationW(
            HANDLE(hwnd.0),
            (&raw const filter).cast::<c_void>(),
            DEVICE_NOTIFY_WINDOW_HANDLE,
        )
    };
    match result {
        Ok(handle) => Some(handle),
        Err(error) => {
            tracing::error!(%error, "cannot follow HID device changes");
            None
        }
    }
}

/// Wakes the interface thread from any thread. A wake-up sent after the
/// window is gone is lost, which only happens while the app quits.
#[derive(Debug, Clone, Copy)]
pub struct Doorbell(isize);

impl Doorbell {
    pub fn ring(self) {
        // SAFETY: PostMessageW may be called from any thread and fails
        // without harm on a window that no longer exists.
        let _ = unsafe {
            PostMessageW(
                Some(HWND(self.0 as *mut c_void)),
                WM_APP_WAKE,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let event = match message {
        WM_APP_WAKE => Some(WindowEvent::Wake),
        WM_APP_DEVICES => Some(WindowEvent::DevicesChanged),
        WM_APP_THEME => Some(WindowEvent::ThemeChanged),
        WM_APP_ANOTHER_INSTANCE => Some(WindowEvent::AnotherInstance),
        // Device and theme changes are sent, not posted: answer at once and
        // handle them from the queue.
        WM_DEVICECHANGE
            if matches!(
                wparam.0 as u32,
                DBT_DEVICEARRIVAL | DBT_DEVICEREMOVECOMPLETE
            ) =>
        {
            post(hwnd, WM_APP_DEVICES);
            return LRESULT(1);
        }
        // Back from sleep: devices may have come or gone meanwhile.
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_APMRESUMEAUTOMATIC => {
            post(hwnd, WM_APP_DEVICES);
            return LRESULT(1);
        }
        WM_SETTINGCHANGE if is_color_set_change(lparam) => {
            post(hwnd, WM_APP_THEME);
            None
        }
        _ => None,
    };
    if let Some(event) = event {
        dispatch(event);
        return LRESULT(0);
    }
    if message == WM_CLOSE {
        // Asked to close by another program (taskkill, an installer).
        quit();
        return LRESULT(0);
    }
    // SAFETY: default handling for every other message.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Runs the handler for `event`. A message loop inside the handler (a menu)
/// can dispatch another event meanwhile: it is queued and handled right
/// after.
pub fn dispatch(event: WindowEvent) {
    PENDING.with(|pending| pending.borrow_mut().push_back(event));
    HANDLER.with(|cell| {
        let Ok(mut handler) = cell.try_borrow_mut() else {
            return;
        };
        while let Some(event) = PENDING.with(|pending| pending.borrow_mut().pop_front()) {
            if let Some(handler) = handler.as_mut() {
                handler(event);
            }
        }
    });
}

/// Ends the message loop.
pub fn quit() {
    // SAFETY: only posts WM_QUIT to the calling thread, the interface thread.
    unsafe { PostQuitMessage(0) };
}

fn post(hwnd: HWND, message: u32) {
    // SAFETY: posting to our own window.
    let _ = unsafe { PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(0)) };
}

/// WM_SETTINGCHANGE carries "ImmersiveColorSet" when the theme colours change.
fn is_color_set_change(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    // SAFETY: for WM_SETTINGCHANGE a non-null lParam is a null-terminated
    // UTF-16 string that lives for the duration of the message.
    let area = unsafe { PCWSTR(lparam.0 as *const u16).to_string() };
    area.is_ok_and(|area| area == "ImmersiveColorSet")
}

/// Runs the message loop until WM_QUIT.
pub fn run_message_loop() {
    let mut message = MSG::default();
    // SAFETY: `message` lives for the whole loop; GetMessageW returns 0 on
    // WM_QUIT and -1 on error, both of which end the loop.
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        let dialog = DIALOG.with(Cell::get);
        if dialog != 0 && is_dialog_message(dialog, &message) {
            continue;
        }
        // SAFETY: `message` was just filled by GetMessageW.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Lets the settings window move the focus with Tab, as a dialog does.
fn is_dialog_message(dialog: isize, message: &MSG) -> bool {
    // SAFETY: `dialog` is our settings window while it exists (set_dialog
    // clears it when the window goes); `message` was filled by GetMessageW.
    unsafe { IsDialogMessageW(HWND(dialog as *mut c_void), message) }.as_bool()
}

/// Held while this process is the running instance.
pub struct InstanceLock(HANDLE);

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Takes the single-instance lock, or tells the running instance and
/// returns `None`.
pub fn lock_single_instance() -> windows::core::Result<Option<InstanceLock>> {
    // SAFETY: a named mutex with default security; the handle is closed by
    // InstanceLock or right below.
    let mutex = unsafe { CreateMutexW(None, false, INSTANCE_MUTEX) }?;
    // SAFETY: reads the error code CreateMutexW left for this thread.
    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return Ok(Some(InstanceLock(mutex)));
    }
    // SAFETY: the handle came from CreateMutexW and is not used after.
    let _ = unsafe { CloseHandle(mutex) };
    // The running instance may not have its window yet; then nothing is
    // told, which only matters for the settings window it would open.
    // SAFETY: looks up a window by class name only.
    if let Ok(running) = unsafe { FindWindowW(CLASS_NAME, PCWSTR::null()) } {
        let mut process = 0;
        // SAFETY: reads the process of a window and lets that process bring
        // its settings window to the front, which Windows refuses otherwise
        // to a program the user did not just click.
        unsafe {
            GetWindowThreadProcessId(running, Some(&mut process));
            let _ = AllowSetForegroundWindow(process);
        }
        post(running, WM_APP_ANOTHER_INSTANCE);
    }
    Ok(None)
}

/// Shows a Windows notification from the tray icon whose window is
/// `tray_window`.
pub fn notify(tray_window: *mut c_void, title: &str, message: &str) {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND(tray_window),
        uID: TRAY_ICON_ID,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_WARNING,
        ..Default::default()
    };
    copy_truncated(&mut data.szInfoTitle, title);
    copy_truncated(&mut data.szInfo, message);
    // SAFETY: `data` is a filled NOTIFYICONDATAW for an icon of this process.
    if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
        tracing::warn!(title, "could not show the notification");
    }
}

/// Whether Windows knows our tray icon under the window and `uID` that
/// [`notify`] uses.
pub fn check_tray_icon(tray_window: *mut c_void) -> bool {
    let data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HWND(tray_window),
        uID: TRAY_ICON_ID,
        ..Default::default()
    };
    // SAFETY: `data` is a filled NOTIFYICONDATAW that changes nothing.
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool()
}

/// Copies `text` into a fixed UTF-16 buffer, leaving room for the final null.
fn copy_truncated(buffer: &mut [u16], text: &str) {
    let room = buffer.len() - 1;
    for (slot, unit) in buffer.iter_mut().zip(text.encode_utf16().take(room)) {
        *slot = unit;
    }
}

/// Whether the taskbar uses the light theme; dark when the setting is missing.
pub fn light_taskbar() -> bool {
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: `value` and `size` describe a DWORD buffer that outlives the call.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast::<c_void>()),
            Some(&mut size),
        )
    };
    status.is_ok() && value != 0
}

/// Side of a tray icon in pixels, at the system scale.
pub fn small_icon_size() -> usize {
    // SAFETY: plain queries without pointers.
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) };
    usize::try_from(size)
        .ok()
        .filter(|&size| size > 0)
        .unwrap_or(16)
}

pub fn ui_language_id() -> u16 {
    // SAFETY: no arguments, only reads the user's language setting.
    unsafe { GetUserDefaultUILanguage() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_text_is_cut_to_fit_with_its_final_null() {
        let mut buffer = [0u16; 4];

        copy_truncated(&mut buffer, "Profil");

        assert_eq!(String::from_utf16_lossy(&buffer[..3]), "Pro");
        assert_eq!(buffer[3], 0);
    }
}
