//! The tray icon and its menu, on the interface thread.

use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;

use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::config::{Reset, Settings};
use crate::gdi::SystemFont;
use crate::hotkeys::{Conflict, Hotkeys};
use crate::icon::picture;
use crate::model::{Failure, IconKind, MenuModel, MouseState, menu_text, tooltip};
use crate::settings_window::SettingsWindow;
use crate::text::Texts;
use crate::win;
use crate::worker::{Command, Update, Worker};

#[path = "tray_settings.rs"]
mod settings;

/// What the menu and the icon report, sent to the interface thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    Menu(MenuId),
    /// A registered shortcut was pressed.
    Hotkey(u32),
    /// The pointer came over the icon: the menu may open soon.
    PointerOver,
}

pub struct App {
    texts: &'static Texts,
    worker: Worker,
    updates: Receiver<Update>,
    events: Receiver<UiEvent>,
    tray: TrayIcon,
    menu: Menu,
    status: MenuItem,
    status_shown: bool,
    settings_item: MenuItem,
    quit: MenuItem,
    profiles: Vec<(u16, CheckMenuItem)>,
    state: MouseState,
    light_taskbar: bool,
    drawn: Option<(IconKind, bool, usize)>,
    settings: Settings,
    hotkeys: Option<Hotkeys>,
    conflicts: BTreeMap<u16, Conflict>,
    window: Option<SettingsWindow>,
    /// Set when the settings file could not be read and is still in place:
    /// it is not written over until the next start.
    keep_file: Option<KeepFile>,
}

/// Why the settings file is left alone until the next start.
#[derive(Debug, Clone, Copy)]
struct KeepFile {
    /// Not valid and could not be set aside, rather than unreadable.
    invalid: bool,
}

/// What the app starts with besides the mouse.
pub struct Startup {
    pub texts: &'static Texts,
    pub settings: Settings,
    /// Set when the settings file could not be used.
    pub reset: Option<Reset>,
}

impl App {
    pub fn new(
        startup: Startup,
        worker: Worker,
        updates: Receiver<Update>,
        events: Receiver<UiEvent>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let Startup {
            texts,
            settings,
            reset,
        } = startup;
        let status = MenuItem::new("", false, None);
        let settings_item = MenuItem::new(texts.settings_menu, true, None);
        let quit = MenuItem::new(texts.quit, true, None);
        let menu = Menu::new();
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&settings_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit)?;
        let state = MouseState::Searching;
        let light_taskbar = win::light_taskbar();
        let size = win::small_icon_size();
        let kind = IconKind::of(&state);
        let tray = TrayIconBuilder::new()
            .with_tooltip(tooltip(&state, texts, &settings))
            .with_icon(draw(kind, size, light_taskbar)?)
            .with_menu(Box::new(menu.clone()))
            .build()?;
        if !win::check_tray_icon(tray.window_handle()) {
            tracing::error!("the tray icon is not where notifications are sent");
        }
        let mut app = Self {
            texts,
            worker,
            updates,
            events,
            tray,
            menu,
            status,
            status_shown: false,
            settings_item,
            quit,
            profiles: Vec::new(),
            state,
            light_taskbar,
            drawn: Some((kind, light_taskbar, size)),
            settings,
            hotkeys: Hotkeys::new()
                .inspect_err(|error| tracing::error!(%error, "cannot register shortcuts"))
                .ok(),
            conflicts: BTreeMap::new(),
            window: None,
            keep_file: reset.as_ref().and_then(|reset| match reset {
                Reset::Unreadable { .. } => Some(KeepFile { invalid: false }),
                Reset::Invalid { kept_at: None, .. } => Some(KeepFile { invalid: true }),
                Reset::Invalid {
                    kept_at: Some(_), ..
                } => None,
            }),
        };
        app.show_state();
        if let Some(reset) = &reset {
            let message = match reset {
                Reset::Unreadable { .. } => texts.settings_unreadable_message,
                Reset::Invalid { kept_at: None, .. } => texts.settings_invalid_kept_message,
                Reset::Invalid {
                    kept_at: Some(_), ..
                } => texts.settings_reset_message,
            };
            let title = if reset.file_left_in_place() {
                texts.settings_unreadable_title
            } else {
                texts.settings_reset_title
            };
            win::notify(app.tray.window_handle(), title, message);
        }
        app.register_shortcuts();
        app.tell_taken_shortcuts();
        let has_shortcuts = app.settings.profiles.values().any(|p| p.shortcut.is_some());
        if app.hotkeys.is_none() && has_shortcuts {
            win::notify(
                app.tray.window_handle(),
                texts.shortcut_taken_title,
                texts.shortcut_unavailable,
            );
        }
        app.keep_start_with_windows();
        Ok(app)
    }

    /// Registers the shortcuts of the settings.
    fn register_shortcuts(&mut self) {
        if let Some(hotkeys) = self.hotkeys.as_mut() {
            self.conflicts = hotkeys.apply(&self.settings);
        }
    }

    /// Shortcuts of the settings that another program already holds.
    fn shortcuts_in(&self, kind: Conflict) -> Vec<String> {
        self.conflicts
            .iter()
            .filter(|(_, conflict)| **conflict == kind)
            .filter_map(|(sector, _)| {
                let shortcut = self.settings.profiles.get(sector)?.shortcut?;
                Some(shortcut.shown(self.texts))
            })
            .collect()
    }

    /// What to say about the shortcuts Windows did not register, with
    /// `taken` as the text for those another program holds; `None` when
    /// all were registered.
    fn conflicts_text(&self, taken: &str) -> Option<String> {
        let lines: Vec<String> = [
            (Conflict::Taken, taken),
            (Conflict::Refused, self.texts.shortcut_refused_list),
        ]
        .into_iter()
        .filter_map(|(kind, template)| {
            let list = self.shortcuts_in(kind);
            (!list.is_empty()).then(|| template.replace("{list}", &list.join(", ")))
        })
        .collect();
        (!lines.is_empty()).then(|| lines.join(" "))
    }

    /// Says which shortcuts Windows did not register, at start-up.
    fn tell_taken_shortcuts(&self) {
        if let Some(text) = self.conflicts_text(self.texts.shortcut_taken_message) {
            win::notify(
                self.tray.window_handle(),
                self.texts.shortcut_taken_title,
                &text,
            );
        }
    }

    pub fn handle(&mut self, event: win::WindowEvent) {
        match event {
            win::WindowEvent::Wake => {
                while let Ok(update) = self.updates.try_recv() {
                    self.on_update(update);
                }
                while let Ok(event) = self.events.try_recv() {
                    self.on_ui_event(event);
                }
            }
            win::WindowEvent::DevicesChanged => {
                self.worker.send(Command::DevicesChanged);
            }
            win::WindowEvent::ThemeChanged => {
                self.light_taskbar = win::light_taskbar();
                self.show_state();
                self.restyle_settings_window();
            }
            win::WindowEvent::AnotherInstance => {
                tracing::info!("launched again while running");
                self.open_settings();
            }
            win::WindowEvent::Settings(event) => self.on_settings(event),
        }
    }

    fn on_update(&mut self, update: Update) {
        match update {
            Update::State(state) => {
                self.state = state;
                self.show_state();
                self.refresh_settings_window();
            }
            Update::SwitchFailed(failure) => win::notify(
                self.tray.window_handle(),
                self.texts.switch_failed_title,
                failure.message(self.texts),
            ),
        }
    }

    fn on_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::PointerOver => {
                self.worker.send(Command::Refresh);
            }
            UiEvent::Hotkey(id) => {
                let sector = self.hotkeys.as_ref().and_then(|hotkeys| hotkeys.sector(id));
                if let Some(sector) = sector {
                    tracing::info!(sector, "shortcut pressed");
                    if !self.worker.send(Command::SwitchTo(sector)) {
                        self.on_update(Update::SwitchFailed(Failure::Stopped));
                    }
                }
            }
            UiEvent::Menu(id) if id == *self.quit.id() => win::quit(),
            UiEvent::Menu(id) if id == *self.settings_item.id() => self.open_settings(),
            UiEvent::Menu(id) => {
                let sector = self
                    .profiles
                    .iter()
                    .find(|(_, item)| *item.id() == id)
                    .map(|(sector, _)| *sector);
                // The menu ticks or unticks an item when it is clicked; the
                // tick follows the mouse instead.
                self.show_state();
                match sector {
                    Some(sector) => {
                        if !self.worker.send(Command::SwitchTo(sector)) {
                            self.on_update(Update::SwitchFailed(Failure::Stopped));
                        }
                    }
                    None => tracing::info!(?id, "menu entry no longer shown"),
                }
            }
        }
    }

    fn show_state(&mut self) {
        let model = MenuModel::of(&self.state, self.texts, &self.settings);
        self.show_status(model.status);
        self.show_profiles(&model);
        let kind = IconKind::of(&self.state);
        let size = win::small_icon_size();
        if self.drawn != Some((kind, self.light_taskbar, size)) {
            match draw(kind, size, self.light_taskbar).map(|icon| self.tray.set_icon(Some(icon))) {
                Ok(Ok(())) => self.drawn = Some((kind, self.light_taskbar, size)),
                Ok(Err(error)) => tracing::warn!(%error, "could not change the tray icon"),
                Err(error) => tracing::warn!(%error, "could not draw the tray icon"),
            }
        }
        if let Err(error) =
            self.tray
                .set_tooltip(Some(tooltip(&self.state, self.texts, &self.settings)))
        {
            tracing::warn!(%error, "could not change the tooltip");
        }
    }

    fn show_status(&mut self, status: Option<&str>) {
        match status {
            Some(text) => {
                self.status.set_text(text);
                if !self.status_shown {
                    self.status_shown = self.menu.prepend(&self.status).is_ok();
                }
            }
            None if self.status_shown => {
                self.status_shown = self.menu.remove(&self.status).is_err();
            }
            None => {}
        }
    }

    fn show_profiles(&mut self, model: &MenuModel) {
        let same_entries = self.profiles.len() == model.profiles.len()
            && self
                .profiles
                .iter()
                .zip(&model.profiles)
                .all(|((sector, item), entry)| {
                    *sector == entry.sector && item.text() == menu_text(&entry.label)
                });
        if !same_entries {
            for (_, item) in self.profiles.drain(..) {
                let _ = self.menu.remove(&item);
            }
            let first = usize::from(self.status_shown);
            for (offset, entry) in model.profiles.iter().enumerate() {
                let item = CheckMenuItem::new(menu_text(&entry.label), true, entry.checked, None);
                if let Err(error) = self.menu.insert(&item, first + offset) {
                    tracing::warn!(%error, "could not add a profile to the menu");
                }
                self.profiles.push((entry.sector, item));
            }
        }
        for ((_, item), entry) in self.profiles.iter().zip(&model.profiles) {
            item.set_checked(entry.checked);
        }
    }
}

fn draw(kind: IconKind, size: usize, light_taskbar: bool) -> Result<Icon, tray_icon::BadIcon> {
    let rgba = picture(kind, size, light_taskbar, &SystemFont);
    let side = u32::try_from(size).unwrap_or(16);
    Icon::from_rgba(rgba, side, side)
}
