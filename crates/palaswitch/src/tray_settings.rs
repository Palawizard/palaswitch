//! What the app does with the settings window and the changes made in it.

use std::time::Instant;

use super::App;
use crate::autostart::RUN_KEY;
use crate::config::{self, Settings};
use crate::hotkeys::Conflict;
use crate::model::{MouseState, profile_label};
use crate::settings_view::{Saved, ShortcutProblem, View, clear_shortcut, give_shortcut, rename};
use crate::settings_window::{SettingsEvent, SettingsWindow};
use crate::shortcut::Shortcut;
use crate::shortcut_field::FieldEvent;
use crate::text::Texts;
use crate::worker::Command;

impl App {
    pub(super) fn open_settings(&mut self) {
        if let Some(window) = &self.window {
            window.raise();
            return;
        }
        let started = Instant::now();
        let view = View::of(&self.state, &self.settings, self.texts);
        let start_with_windows = std::env::current_exe().is_ok_and(|exe| RUN_KEY.is_enabled(&exe));
        match SettingsWindow::open(self.texts, &view, start_with_windows) {
            Ok(window) => {
                tracing::info!(
                    elapsed_ms = started.elapsed().as_millis(),
                    "settings window open"
                );
                self.window = Some(window);
                self.worker.send(Command::Refresh);
                if let Some(text) = self.conflicts_text(self.texts.shortcut_taken_here) {
                    self.set_message(Some(&text));
                }
            }
            Err(error) => tracing::error!(%error, "could not open the settings window"),
        }
    }

    pub(super) fn refresh_settings_window(&mut self) {
        let view = View::of(&self.state, &self.settings, self.texts);
        if let Some(window) = self.window.as_mut() {
            window.show(&view);
        }
    }

    pub(super) fn restyle_settings_window(&mut self) {
        if let Some(window) = self.window.as_mut() {
            window.restyle();
        }
    }

    pub(super) fn on_settings(&mut self, event: SettingsEvent) {
        match event {
            SettingsEvent::Rename { sector, text } => {
                let renamed = rename(&self.settings, sector, &text);
                if renamed != self.settings {
                    self.settings = renamed;
                    let saved = self.save();
                    self.set_message(saved.message(self.texts));
                    self.show_state();
                    self.refresh_settings_window();
                }
            }
            SettingsEvent::Field(FieldEvent::Listening) => {
                // Keys must reach the field, even a shortcut of the app.
                if let Some(hotkeys) = self.hotkeys.as_mut() {
                    hotkeys.apply(&Settings::default());
                }
                self.set_message(None);
            }
            SettingsEvent::Field(FieldEvent::Typed(sector, shortcut)) => {
                self.give(sector, shortcut);
            }
            SettingsEvent::Field(FieldEvent::Stopped(_)) => {
                self.register_shortcuts();
                self.refresh_settings_window();
            }
            SettingsEvent::Clear(sector) => {
                self.settings = clear_shortcut(&self.settings, sector);
                let saved = self.save();
                self.register_shortcuts();
                self.set_message(saved.message(self.texts));
                self.refresh_settings_window();
            }
            SettingsEvent::StartWithWindows(enabled) => self.start_with_windows(enabled),
            SettingsEvent::DpiChanged(dpi) => {
                if let Some(window) = self.window.as_mut() {
                    window.rescale(dpi);
                }
            }
            SettingsEvent::Closed => {
                self.window = None;
                self.register_shortcuts();
            }
        }
    }

    /// Gives `shortcut` to `sector` if no profile and no other program has
    /// it; otherwise says why and keeps the previous one.
    fn give(&mut self, sector: u16, shortcut: Shortcut) {
        let given = give_shortcut(
            &self.settings,
            sector,
            shortcut,
            |other| profile_name(&self.state, &self.settings, self.texts, other),
            crate::win::types_with_altgr,
        );
        let message = match given {
            Err(problem) => {
                self.register_shortcuts();
                Some(problem.message(self.texts))
            }
            Ok(_) if self.hotkeys.is_none() => Some(self.texts.shortcut_unavailable.to_owned()),
            Ok(settings) => {
                let conflicts = self
                    .hotkeys
                    .as_mut()
                    .map(|hotkeys| hotkeys.apply(&settings))
                    .unwrap_or_default();
                let problem = match conflicts.get(&sector) {
                    Some(Conflict::Taken) => Some(ShortcutProblem::Taken),
                    Some(Conflict::Refused) => Some(ShortcutProblem::RefusedByWindows),
                    None => None,
                };
                if let Some(problem) = problem {
                    self.register_shortcuts();
                    Some(problem.message(self.texts))
                } else {
                    self.settings = settings;
                    self.conflicts = conflicts;
                    self.save().message(self.texts).map(str::to_owned)
                }
            }
        };
        self.set_message(message.as_deref());
        self.refresh_settings_window();
    }

    /// When the settings say to start with Windows, makes the Run key point
    /// to this executable, which may have moved since the box was ticked.
    pub(super) fn keep_start_with_windows(&self) {
        if !self.settings.start_with_windows {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        if let Err(error) = RUN_KEY.restore(&exe) {
            tracing::warn!(%error, "could not restore start with Windows");
        }
    }

    fn start_with_windows(&mut self, enabled: bool) {
        let result = std::env::current_exe()
            .map_err(|error| error.to_string())
            .and_then(|exe| {
                RUN_KEY
                    .set(&exe, enabled)
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => {
                self.settings.start_with_windows = enabled;
                let saved = self.save();
                self.set_message(saved.message(self.texts));
            }
            Err(error) => {
                tracing::warn!(error, enabled, "could not change start with Windows");
                self.set_message(Some(self.texts.autostart_failed));
                if let Some(window) = &self.window {
                    window.set_start_with_windows(!enabled);
                }
            }
        }
    }

    fn save(&self) -> Saved {
        let Some(path) = config::default_path() else {
            return Saved::Failed;
        };
        if let Some(keep) = self.keep_file {
            tracing::warn!("settings not saved: the file could not be used at start-up");
            return Saved::Blocked {
                invalid: keep.invalid,
            };
        }
        match config::save(&path, &self.settings) {
            Ok(()) => Saved::Written,
            Err(error) => {
                tracing::warn!(%error, "could not save the settings");
                Saved::Failed
            }
        }
    }

    fn set_message(&self, message: Option<&str>) {
        if let Some(window) = &self.window {
            window.set_message(message);
        }
    }
}

/// The name shown for the profile at `sector`.
fn profile_name(state: &MouseState, settings: &Settings, texts: &Texts, sector: u16) -> String {
    let profile = match state {
        MouseState::Present(view) => view.profiles.iter().find(|p| p.sector == sector).copied(),
        _ => None,
    };
    let profile = profile.unwrap_or(palaswitch_hidpp::features::Profile {
        sector,
        enabled: true,
    });
    profile_label(&profile, texts, settings)
}
