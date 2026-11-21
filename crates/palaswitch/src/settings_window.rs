//! The settings window: created when opened, destroyed when closed.

use std::cell::Cell;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    FillRect, GetMonitorInfoW, HBRUSH, HDC, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, SetBkColor, SetTextColor,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    EM_LIMITTEXT, EM_SETCUEBANNER, NM_CUSTOMDRAW, NMCUSTOMDRAW, NMHDR,
};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CreateWindowExW,
    DefWindowProcW, DestroyWindow, ES_AUTOHSCROLL, GetCursorPos, GetWindowTextW, HMENU, IDC_ARROW,
    IDCANCEL, IsIconic, IsWindow, LoadCursorW, RegisterClassW, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLOREDIT,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_NOTIFY, WM_SETFONT, WNDCLASSW,
    WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::settings_view::{Row, View};
use crate::shortcut_field::{self, FieldEvent};
use crate::text::Texts;
use crate::theme::{self, Brushes, ControlKind, Fonts, Palette};
use crate::win::{self, WindowEvent};

const CLASS_NAME: PCWSTR = w!("palaswitch.settings");
const EN_CHANGE: u32 = 0x0300;

// Control ids: a kind in the high bits, the profile sector in the low ones.
const ID_NAME: u16 = 0x1000;
const ID_FIELD: u16 = 0x2000;
const ID_CLEAR: u16 = 0x3000;
const ID_DETAIL: u16 = 0x4001;
const ID_MESSAGE: u16 = 0x4002;
const ID_START: u16 = 0x4003;
const KIND_MASK: u16 = 0xF000;
/// Static text shown as is: an ampersand in a profile name is not taken for
/// a keyboard accelerator. From WinUser.h.
const SS_NOPREFIX: WINDOW_STYLE = WINDOW_STYLE(0x0080);

// Layout in device-independent pixels.
const MARGIN: i32 = 20;
const CONTENT_WIDTH: i32 = 440;
const NAME_WIDTH: i32 = 170;
const FIELD_WIDTH: i32 = 186;
const GAP: i32 = 8;
const CONTROL_HEIGHT: i32 = 28;
/// A single-line edit draws its text at the top: it gets the height of its
/// text plus a little, centred on the row.
const EDIT_HEIGHT: i32 = 24;
const ROW_STEP: i32 = CONTROL_HEIGHT + GAP;
const LINE_HEIGHT: i32 = 20;
const TITLE_HEIGHT: i32 = 28;

/// What the window tells the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsEvent {
    Rename { sector: u16, text: String },
    Field(FieldEvent),
    Clear(u16),
    StartWithWindows(bool),
    DpiChanged(u32),
    Closed,
}

fn report(event: SettingsEvent) {
    win::dispatch(WindowEvent::Settings(event));
}

fn report_field(event: FieldEvent) {
    report(SettingsEvent::Field(event));
}

/// What the window procedure needs while painting.
#[derive(Clone, Copy)]
struct Paint {
    palette: Palette,
    background: isize,
    field: isize,
}

thread_local! {
    static PAINT: Cell<Option<Paint>> = const { Cell::new(None) };
    static TEXTS: Cell<Option<&'static Texts>> = const { Cell::new(None) };
    /// Set while the app writes into the controls, so that the change
    /// notifications they send are not taken for the user's.
    static UPDATING: Cell<bool> = const { Cell::new(false) };
    /// Set once the window received WM_DESTROY, so that it is not destroyed
    /// a second time from `Drop`.
    static DESTROYED: Cell<bool> = const { Cell::new(false) };
    /// Set while `Drop` destroys the window: the app already knows.
    static SILENT: Cell<bool> = const { Cell::new(false) };
}

#[path = "settings_proc.rs"]
mod window_proc;
use window_proc::window_proc;

struct RowControls {
    sector: u16,
    name: HWND,
    field: HWND,
    clear: HWND,
}

pub struct SettingsWindow {
    hwnd: HWND,
    texts: &'static Texts,
    title: HWND,
    detail: HWND,
    name_header: HWND,
    shortcut_header: HWND,
    rows: Vec<RowControls>,
    message: HWND,
    start: HWND,
    dpi: u32,
    palette: Palette,
    fonts: Fonts,
    brushes: Brushes,
}

impl SettingsWindow {
    pub fn open(
        texts: &'static Texts,
        view: &View,
        start_with_windows: bool,
    ) -> windows::core::Result<Self> {
        TEXTS.with(|cell| cell.set(Some(texts)));
        DESTROYED.with(|cell| cell.set(false));
        // SAFETY: null asks for the module of the running executable.
        let instance = unsafe { GetModuleHandleW(None) }?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: CLASS_NAME,
            // SAFETY: loads the standard arrow cursor shared by the system.
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }?,
            ..Default::default()
        };
        let origin = cursor_work_area().map_or(POINT::default(), |area| POINT {
            x: area.left,
            y: area.top,
        });
        // SAFETY: registers our class once; later calls fail harmlessly
        // because it already exists.
        unsafe { RegisterClassW(&class) };
        // SAFETY: the class is registered; the window is shown once laid out.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_CONTROLPARENT,
                CLASS_NAME,
                &HSTRING::from(texts.settings_title),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                // Created on the monitor of the pointer, so that it is laid
                // out at that monitor's scale from the start.
                origin.x,
                origin.y,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
        }?;
        // SAFETY: `hwnd` was just created.
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        let palette = Palette::current();
        let child = |class: PCWSTR, text: &str, style: WINDOW_STYLE, id: u16| {
            create_child(hwnd, class, text, style, id)
        };
        let title = child(w!("STATIC"), "", SS_NOPREFIX, 0);
        let detail = child(w!("STATIC"), "", SS_NOPREFIX, ID_DETAIL);
        let name_header = child(w!("STATIC"), texts.settings_name_header, SS_NOPREFIX, 0);
        let shortcut_header = child(w!("STATIC"), texts.settings_shortcut_header, SS_NOPREFIX, 0);
        let message = child(w!("STATIC"), "", SS_NOPREFIX, ID_MESSAGE);
        let start = child(
            w!("BUTTON"),
            texts.start_with_windows,
            WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
            ID_START,
        );
        let mut window = Self {
            hwnd,
            texts,
            title,
            detail,
            name_header,
            shortcut_header,
            rows: Vec::new(),
            message,
            start,
            dpi,
            palette,
            fonts: Fonts::new(dpi),
            brushes: Brushes::new(&palette),
        };
        window.set_start_with_windows(start_with_windows);
        window.restyle();
        window.show(view);
        window.place_on_cursor_monitor();
        // SAFETY: shows and focuses our own window.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        window.focus_first();
        win::set_dialog(Some(hwnd));
        Ok(window)
    }

    /// Brings the open window to the front.
    pub fn raise(&self) {
        // SAFETY: our own window, restored first if it was minimised.
        unsafe {
            let show = if IsIconic(self.hwnd).as_bool() {
                SW_RESTORE
            } else {
                SW_SHOW
            };
            let _ = ShowWindow(self.hwnd, show);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    /// Shows `view`; the rows are rebuilt only when the profiles change.
    pub fn show(&mut self, view: &View) {
        set_text(self.title, &view.title);
        set_text(self.detail, &view.detail);
        let same_rows = self.rows.len() == view.rows.len()
            && self
                .rows
                .iter()
                .zip(&view.rows)
                .all(|(controls, row)| controls.sector == row.sector);
        if !same_rows {
            // The field that listened goes away: the app gives the
            // shortcuts back.
            if let Some(sector) = shortcut_field::stop_listening() {
                report_field(FieldEvent::Stopped(sector));
            }
            // SAFETY: reads which control has the keyboard focus.
            let focused = unsafe { GetFocus() };
            let focus_lost = self
                .rows
                .iter()
                .any(|row| [row.name, row.field, row.clear].contains(&focused));
            for controls in self.rows.drain(..) {
                for control in [controls.name, controls.field, controls.clear] {
                    // SAFETY: child controls of ours, not used after.
                    let _ = unsafe { DestroyWindow(control) };
                }
            }
            self.rows = view.rows.iter().map(|row| self.create_row(row)).collect();
            self.apply_fonts_and_styles();
            if focus_lost {
                self.focus_first();
            }
        }
        for (controls, row) in self.rows.iter().zip(&view.rows) {
            self.show_row(controls, row);
        }
        let has_rows = !view.rows.is_empty();
        for header in [self.name_header, self.shortcut_header] {
            set_visible(header, has_rows);
        }
        self.layout();
    }

    fn create_row(&self, row: &Row) -> RowControls {
        let name = create_child(
            self.hwnd,
            w!("EDIT"),
            &row.name,
            WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP,
            ID_NAME | row.sector,
        );
        let cue = HSTRING::from(row.default_name.as_str());
        // SAFETY: `name` is our edit; the cue text outlives the call and the
        // limit is a plain number.
        unsafe {
            SendMessageW(
                name,
                EM_SETCUEBANNER,
                Some(WPARAM(1)),
                Some(LPARAM(cue.as_ptr() as isize)),
            );
            SendMessageW(
                name,
                EM_LIMITTEXT,
                Some(WPARAM(crate::config::MAX_NAME_LEN)),
                None,
            );
        }
        let field = create_child(
            self.hwnd,
            w!("BUTTON"),
            "",
            WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
            ID_FIELD | row.sector,
        );
        shortcut_field::attach(field, self.texts, report_field);
        let clear = create_child(
            self.hwnd,
            w!("BUTTON"),
            self.texts.shortcut_clear,
            WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
            ID_CLEAR | row.sector,
        );
        RowControls {
            sector: row.sector,
            name,
            field,
            clear,
        }
    }

    fn show_row(&self, controls: &RowControls, row: &Row) {
        // The name being typed is left alone.
        // SAFETY: reads which control has the keyboard focus.
        let typing = unsafe { GetFocus() } == controls.name;
        if !typing && window_text(controls.name) != row.name {
            set_text(controls.name, &row.name);
        }
        let shown = row.shortcut.map_or_else(
            || self.texts.shortcut_none.to_owned(),
            |shortcut| shortcut.shown(self.texts),
        );
        if !shortcut_field::is_listening(controls.field) {
            set_text(controls.field, &shown);
        }
        // SAFETY: moves the focus off a button about to be disabled, then
        // enables or disables it; all are controls of ours.
        unsafe {
            if row.shortcut.is_none() && GetFocus() == controls.clear {
                let _ = SetFocus(Some(controls.field));
            }
            let _ = EnableWindow(controls.clear, row.shortcut.is_some());
        }
    }

    /// Shows a line about the last refused shortcut, or nothing.
    pub fn set_message(&self, message: Option<&str>) {
        set_text(self.message, message.unwrap_or_default());
    }

    pub fn set_start_with_windows(&self, checked: bool) {
        // SAFETY: sets the state of our own checkbox.
        unsafe {
            SendMessageW(
                self.start,
                BM_SETCHECK,
                Some(WPARAM(usize::from(checked))),
                None,
            );
        }
    }

    /// Follows a change of the window's DPI: new fonts, new positions.
    pub fn rescale(&mut self, dpi: u32) {
        self.dpi = dpi.max(96);
        // The old fonts are deleted only once no control uses them.
        let old = std::mem::replace(&mut self.fonts, Fonts::new(self.dpi));
        self.apply_fonts_and_styles();
        drop(old);
        self.layout();
    }

    /// Follows a change between light, dark and high contrast.
    pub fn restyle(&mut self) {
        self.palette = Palette::current();
        self.brushes = Brushes::new(&self.palette);
        PAINT.with(|cell| {
            cell.set(Some(Paint {
                palette: self.palette,
                background: self.brushes.background.0 as isize,
                field: self.brushes.field.0 as isize,
            }));
        });
        theme::title_bar(self.hwnd, self.palette.dark);
        self.apply_fonts_and_styles();
        // SAFETY: repaints our own window and its children.
        let _ = unsafe { InvalidateRect(Some(self.hwnd), None, true) };
    }

    fn controls(&self) -> Vec<(HWND, Option<ControlKind>)> {
        let mut controls = vec![
            (self.title, None),
            (self.detail, None),
            (self.name_header, None),
            (self.shortcut_header, None),
            (self.message, None),
            (self.start, Some(ControlKind::Button)),
        ];
        for row in &self.rows {
            controls.push((row.name, Some(ControlKind::Edit)));
            controls.push((row.field, Some(ControlKind::Button)));
            controls.push((row.clear, Some(ControlKind::Button)));
        }
        controls
    }

    fn apply_fonts_and_styles(&self) {
        for (control, kind) in self.controls() {
            let font = if control == self.title {
                self.fonts.title
            } else {
                self.fonts.body
            };
            // SAFETY: sets a font that lives as long as the window's fonts.
            unsafe {
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(font.0 as usize)),
                    Some(LPARAM(1)),
                );
            }
            if let Some(kind) = kind {
                theme::style_control(control, kind, self.palette.dark);
            }
        }
    }

    fn scale(&self, dip: i32) -> i32 {
        dip * self.dpi as i32 / 96
    }

    fn layout(&self) {
        let place = |control: HWND, x: i32, y: i32, width: i32, height: i32| {
            // SAFETY: moves a child control of ours.
            let _ = unsafe {
                SetWindowPos(
                    control,
                    None,
                    self.scale(x),
                    self.scale(y),
                    self.scale(width),
                    self.scale(height),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
        };
        let field_x = MARGIN + NAME_WIDTH + GAP;
        let clear_x = field_x + FIELD_WIDTH + GAP;
        let clear_width = MARGIN + CONTENT_WIDTH - clear_x;
        let mut y = MARGIN;
        place(self.title, MARGIN, y, CONTENT_WIDTH, TITLE_HEIGHT);
        y += TITLE_HEIGHT;
        place(self.detail, MARGIN, y, CONTENT_WIDTH, LINE_HEIGHT);
        y += LINE_HEIGHT + 2 * GAP;
        if !self.rows.is_empty() {
            place(self.name_header, MARGIN, y, NAME_WIDTH, LINE_HEIGHT);
            place(self.shortcut_header, field_x, y, FIELD_WIDTH, LINE_HEIGHT);
            y += LINE_HEIGHT + GAP / 2;
        }
        for row in &self.rows {
            place(
                row.name,
                MARGIN,
                y + (CONTROL_HEIGHT - EDIT_HEIGHT) / 2,
                NAME_WIDTH,
                EDIT_HEIGHT,
            );
            place(row.field, field_x, y, FIELD_WIDTH, CONTROL_HEIGHT);
            place(row.clear, clear_x, y, clear_width, CONTROL_HEIGHT);
            y += ROW_STEP;
        }
        // Two lines: a list of several taken shortcuts wraps.
        place(self.message, MARGIN, y, CONTENT_WIDTH, 2 * LINE_HEIGHT);
        y += 2 * LINE_HEIGHT + GAP;
        place(self.start, MARGIN, y, CONTENT_WIDTH, CONTROL_HEIGHT);
        y += CONTROL_HEIGHT + MARGIN;
        self.resize_client(MARGIN * 2 + CONTENT_WIDTH, y);
    }

    fn resize_client(&self, width: i32, height: i32) {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: self.scale(width),
            bottom: self.scale(height),
        };
        // SAFETY: plain computation on `rect` for our window's styles.
        let _ = unsafe {
            AdjustWindowRectExForDpi(
                &mut rect,
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                false,
                WS_EX_CONTROLPARENT,
                self.dpi,
            )
        };
        // SAFETY: resizes our own window, keeping its position.
        let _ = unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE,
            )
        };
    }

    /// Centres the window on the work area of the monitor under the pointer,
    /// where the tray menu was just used.
    fn place_on_cursor_monitor(&self) {
        let Some(area) = cursor_work_area() else {
            return;
        };
        // SAFETY: plain queries about our window, then a move of it.
        unsafe {
            let mut window = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut window);
            let (width, height) = (window.right - window.left, window.bottom - window.top);
            let x = area.left + (area.right - area.left - width) / 2;
            let y = area.top + (area.bottom - area.top - height) / 2;
            let _ = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
            );
        }
    }

    fn focus_first(&self) {
        let first = self.rows.first().map_or(self.start, |row| row.name);
        // SAFETY: focuses a control of our foreground window.
        let _ = unsafe { SetFocus(Some(first)) };
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        win::set_dialog(None);
        shortcut_field::stop_listening();
        if !DESTROYED.with(Cell::get) {
            SILENT.with(|cell| cell.set(true));
            // SAFETY: destroys our window, which the user did not close; its
            // fonts and brushes are deleted after, with `self`.
            unsafe {
                if IsWindow(Some(self.hwnd)).as_bool() {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            SILENT.with(|cell| cell.set(false));
        }
        PAINT.with(|cell| cell.set(None));
    }
}

/// Work area of the monitor under the pointer.
fn cursor_work_area() -> Option<RECT> {
    let mut cursor = POINT::default();
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: plain queries about the pointer and its monitor, into locals.
    unsafe {
        GetCursorPos(&mut cursor).ok()?;
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info)
            .as_bool()
            .then_some(info.rcWork)
    }
}

fn create_child(parent: HWND, class: PCWSTR, text: &str, style: WINDOW_STYLE, id: u16) -> HWND {
    let extra = if class == w!("EDIT") {
        WS_EX_CLIENTEDGE
    } else {
        WINDOW_EX_STYLE(0)
    };
    // SAFETY: a standard control as child of our window; the id is passed
    // in place of a menu, as child windows take it.
    let created = unsafe {
        CreateWindowExW(
            extra,
            class,
            &HSTRING::from(text),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            Some(parent),
            Some(HMENU(usize::from(id) as *mut _)),
            None,
            None,
        )
    };
    created.unwrap_or_else(|error| {
        tracing::error!(%error, "could not create a control");
        HWND::default()
    })
}

fn set_text(control: HWND, text: &str) {
    UPDATING.with(|cell| cell.set(true));
    // SAFETY: a control of ours; the text outlives the call.
    let _ = unsafe { SetWindowTextW(control, &HSTRING::from(text)) };
    UPDATING.with(|cell| cell.set(false));
}

fn set_visible(control: HWND, visible: bool) {
    let show = if visible {
        SW_SHOW
    } else {
        windows::Win32::UI::WindowsAndMessaging::SW_HIDE
    };
    // SAFETY: shows or hides a control of ours.
    let _ = unsafe { ShowWindow(control, show) };
}

fn window_text(control: HWND) -> String {
    let mut buffer = [0u16; 128];
    // SAFETY: `buffer` is writable and its length is passed along.
    let len = unsafe { GetWindowTextW(control, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}
