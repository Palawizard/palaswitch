#![windows_subsystem = "windows"]

mod gdi;
mod icon;
mod logs;
mod model;
mod text;
mod tray;
mod win;
mod worker;

use std::cell::RefCell;
use std::error::Error;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc;

use tray_icon::menu::MenuEvent;
use tray_icon::{MouseButtonState, TrayIconEvent};

use text::{Language, texts};
use tray::{App, UiEvent};
use win::HiddenWindow;
use worker::Worker;

fn main() -> ExitCode {
    // Taken before the log file is opened, so that a second launch never
    // writes to the file of the running instance.
    let instance = match win::lock_single_instance() {
        Ok(Some(instance)) => instance,
        Ok(None) => return ExitCode::SUCCESS,
        Err(_) => return ExitCode::FAILURE,
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
    let texts = texts(Language::from_windows_lang_id(win::ui_language_id()));

    let app: Rc<RefCell<Option<App>>> = Rc::new(RefCell::new(None));
    let window = {
        let app = app.clone();
        HiddenWindow::create(move |event| {
            if let Some(app) = app.borrow_mut().as_mut() {
                app.handle(event);
            }
        })?
    };
    let doorbell = window.doorbell();

    let (ui_sender, ui_events) = mpsc::channel();
    let menu_sender = ui_sender.clone();
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

    let (update_sender, updates) = mpsc::channel();
    let worker = Worker::spawn(move |update| {
        let _ = update_sender.send(update);
        doorbell.ring();
    })?;

    *app.borrow_mut() = Some(App::new(texts, worker, updates, ui_events)?);
    win::run_message_loop();

    // Dropping the app removes the icon at once instead of leaving it until
    // the pointer passes over it.
    app.borrow_mut().take();
    drop(window);
    Ok(())
}
