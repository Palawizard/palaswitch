#![windows_subsystem = "windows"]

mod autostart;
mod capture;
mod config;
mod gdi;
mod hardware;
mod hotkeys;
mod icon;
mod logs;
mod model;
mod settings_view;
mod settings_window;
mod shortcut;
mod shortcut_field;
mod text;
mod theme;
mod tray;
mod updates;
mod win;
mod worker;

use std::cell::RefCell;
use std::error::Error;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc;

use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use tray_icon::menu::MenuEvent;
use tray_icon::{MouseButtonState, TrayIconEvent};

use text::{Language, texts};
use tray::{App, Startup, UiEvent};
use win::HiddenWindow;
use worker::Worker;

fn main() -> ExitCode {
    updates::run_installer_hooks();
    // Taken before the log file is opened, so that a second launch never
    // writes to the file of the running instance.
    let instance = match win::lock_single_instance() {
        Ok(Some(instance)) => instance,
        Ok(None) => return ExitCode::SUCCESS,
        Err(error) => {
            logs::init(logs::default_dir().as_deref());
            tracing::error!(%error, "could not take the single-instance lock");
            return ExitCode::FAILURE;
        }
    };
    logs::init(logs::default_dir().as_deref());
    std::panic::set_hook(Box::new(|info| tracing::error!(%info, "panic")));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting");
    let result = run();
    drop(instance);
    match result {
        Ok(()) => {
            tracing::info!("quit");
            ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(%error, "could not start");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let language = std::env::var("PALASWITCH_LANG")
        .ok()
        .and_then(|name| Language::from_name(&name))
        .unwrap_or_else(|| Language::from_windows_lang_id(win::ui_language_id()));
    let texts = texts(language);
    let (settings, reset) = match config::default_path() {
        Some(path) => config::load(&path),
        None => (config::Settings::default(), None),
    };

    let app: Rc<RefCell<Option<App>>> = Rc::new(RefCell::new(None));
    let window = {
        let app = app.clone();
        HiddenWindow::create(move |event| {
            // Busy only while the app is dropped at the end: nothing left to do.
            if let Ok(mut app) = app.try_borrow_mut()
                && let Some(app) = app.as_mut()
            {
                app.handle(event);
            }
        })?
    };
    let doorbell = window.doorbell();

    let (ui_sender, ui_events) = mpsc::channel();
    let menu_sender = ui_sender.clone();
    let hotkey_sender = ui_sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = menu_sender.send(UiEvent::Menu(event.id));
        doorbell.ring();
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        // The pointer coming over the icon is not always reported: a press
        // on the icon, which comes before the menu opens, also counts.
        let menu_coming = matches!(
            event,
            TrayIconEvent::Enter { .. }
                | TrayIconEvent::Click {
                    button_state: MouseButtonState::Down,
                    ..
                }
        );
        if menu_coming {
            let _ = ui_sender.send(UiEvent::PointerOver);
            doorbell.ring();
        }
    }));

    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed {
            let _ = hotkey_sender.send(UiEvent::Hotkey(event.id));
            doorbell.ring();
        }
    }));

    let (update_sender, updates) = mpsc::channel();
    let worker = Worker::spawn(move |update| {
        let _ = update_sender.send(update);
        doorbell.ring();
    })?;

    let startup = Startup {
        texts,
        settings,
        reset,
    };
    *app.borrow_mut() = Some(App::new(startup, worker, updates, ui_events)?);
    win::run_message_loop();

    // Dropping the app removes the icon at once instead of leaving it until
    // the pointer passes over it. It is taken out of the cell first: while
    // it drops, its settings window may still report to the handler, which
    // must find the cell free.
    let quitting = app.borrow_mut().take();
    drop(quitting);
    drop(window);
    Ok(())
}
