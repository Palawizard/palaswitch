//! Global shortcuts of the profiles, registered with Windows.

use std::collections::{BTreeMap, HashMap};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{Error, GlobalHotKeyManager};

use crate::config::Settings;
use crate::shortcut::Shortcut;

/// Why Windows did not take a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// Another program, or Windows itself, already uses it.
    Taken,
    /// Windows refused it for another reason (details in the log).
    Refused,
}

/// The shortcuts registered for the profiles. Lives on the interface thread,
/// whose message loop receives them.
pub struct Hotkeys {
    manager: GlobalHotKeyManager,
    /// Registered hotkeys, by hotkey id, with the profile sector they select.
    registered: HashMap<u32, (HotKey, u16)>,
}

impl Hotkeys {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            manager: GlobalHotKeyManager::new()?,
            registered: HashMap::new(),
        })
    }

    /// Registers the shortcuts of `settings` in place of the previous ones.
    /// Returns the profiles whose shortcut Windows refused.
    pub fn apply(&mut self, settings: &Settings) -> BTreeMap<u16, Conflict> {
        for (_, (hotkey, _)) in self.registered.drain() {
            if let Err(error) = self.manager.unregister(hotkey) {
                tracing::warn!(%error, "could not release a shortcut");
            }
        }
        let mut conflicts = BTreeMap::new();
        for (&sector, profile) in &settings.profiles {
            let Some(shortcut) = profile.shortcut else {
                continue;
            };
            match self.register(shortcut) {
                Ok(hotkey) => {
                    self.registered.insert(hotkey.id(), (hotkey, sector));
                }
                Err(conflict) => {
                    conflicts.insert(sector, conflict);
                }
            }
        }
        conflicts
    }

    /// Asks Windows for `shortcut` alone, to tell the user at once whether
    /// it can be used. Registers it if so.
    fn register(&self, shortcut: Shortcut) -> Result<HotKey, Conflict> {
        let hotkey = shortcut.hotkey();
        match self.manager.register(hotkey) {
            Ok(()) => {
                tracing::info!(%shortcut, "shortcut registered");
                Ok(hotkey)
            }
            Err(Error::AlreadyRegistered(_)) => {
                tracing::warn!(%shortcut, "shortcut already used by another program");
                Err(Conflict::Taken)
            }
            Err(error) => {
                tracing::warn!(%error, %shortcut, "shortcut refused");
                Err(Conflict::Refused)
            }
        }
    }

    /// The profile sector a pressed hotkey selects.
    pub fn sector(&self, hotkey_id: u32) -> Option<u16> {
        self.registered.get(&hotkey_id).map(|&(_, sector)| sector)
    }
}
