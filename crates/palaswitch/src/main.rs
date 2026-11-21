#![windows_subsystem = "windows"]

mod logs;
mod text;

use std::error::Error;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage,
};

use text::{Language, texts};

const ICON_SIZE: u32 = 32;

fn main() -> Result<(), Box<dyn Error>> {
    logs::init(logs::default_dir().as_deref());
    std::panic::set_hook(Box::new(|info| tracing::error!(%info, "panic")));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");

    // SAFETY: no arguments, only reads the user's language setting.
    let language = Language::from_windows_lang_id(unsafe { GetUserDefaultUILanguage() });
    let texts = texts(language);

    let quit = MenuItem::new(texts.quit, true, None);
    let menu = Menu::new();
    menu.append(&quit)?;

    let quit_id = quit.id().clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            // SAFETY: the handler runs inside the message loop of this thread.
            unsafe { PostQuitMessage(0) };
        }
    }));

    let _tray = TrayIconBuilder::new()
        .with_tooltip("palaswitch")
        .with_icon(placeholder_icon()?)
        .with_menu(Box::new(menu))
        .build()?;

    run_message_loop();
    Ok(())
}

fn run_message_loop() {
    let mut message = MSG::default();
    // SAFETY: `message` lives for the whole loop; GetMessageW returns 0 on
    // WM_QUIT and -1 on error, both of which end the loop.
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        // SAFETY: `message` was just filled by GetMessageW.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Plain disc until the real profile icons exist.
fn placeholder_icon() -> Result<Icon, tray_icon::BadIcon> {
    let center = (ICON_SIZE as f32 - 1.0) / 2.0;
    let radius = ICON_SIZE as f32 / 2.0 - 1.0;
    let mut rgba = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let distance = (x as f32 - center).hypot(y as f32 - center);
            let alpha = if distance <= radius { 0xFF } else { 0x00 };
            rgba.extend_from_slice(&[0x2B, 0x6C, 0xE0, alpha]);
        }
    }
    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE)
}
