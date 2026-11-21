//! What the tray shows for the state of the mouse, without any Windows API.

use palaswitch_device::Connection;
use palaswitch_hidpp::Error;
use palaswitch_hidpp::features::Profile;

use crate::config::Settings;
use crate::text::Texts;

/// The mouse as the mouse thread last saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseState {
    /// First search since the app started.
    Searching,
    /// No mouse with onboard profiles answers. `asleep` when a receiver is
    /// there but its mouse does not answer.
    Absent {
        asleep: bool,
    },
    Present(MouseView),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseView {
    pub name: String,
    pub connection: Connection,
    pub profiles: Vec<Profile>,
    /// Sector of the active profile, when it could be read.
    pub active: Option<u16>,
    /// The last request got no answer: asleep, switched off or out of range.
    pub unreachable: bool,
}

/// Why a profile change did not happen, as the user can act on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Asleep,
    NoMouse,
    Refused,
    /// The mouse thread is gone; only a restart helps.
    Stopped,
}

impl Failure {
    pub fn of(error: &Error) -> Self {
        match error {
            Error::Device(device) if device.is_unreachable() => Self::Asleep,
            Error::Timeout => Self::Asleep,
            Error::Transport(_) => Self::NoMouse,
            _ => Self::Refused,
        }
    }

    pub fn message(self, texts: &Texts) -> &'static str {
        match self {
            Self::Asleep => texts.switch_failed_asleep,
            Self::NoMouse => texts.switch_failed_no_mouse,
            Self::Refused => texts.switch_failed_other,
            Self::Stopped => texts.switch_failed_stopped,
        }
    }
}

/// What the tray icon draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Profile(u16),
    /// A mouse is there but its active profile is not known yet.
    Unknown,
    NoMouse,
}

impl IconKind {
    pub fn of(state: &MouseState) -> Self {
        match state {
            MouseState::Searching => Self::Unknown,
            MouseState::Absent { .. } => Self::NoMouse,
            MouseState::Present(view) => match view.active {
                Some(sector) => Self::Profile(profile_number(sector)),
                None => Self::Unknown,
            },
        }
    }
}

/// Number shown for a profile sector: 1 for the first slot.
fn profile_number(sector: u16) -> u16 {
    Profile {
        sector,
        enabled: true,
    }
    .number()
}

/// One profile entry of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuProfile {
    pub sector: u16,
    pub label: String,
    pub checked: bool,
}

/// The part of the tray menu that depends on the mouse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// Line shown greyed out above the profiles.
    pub status: Option<&'static str>,
    pub profiles: Vec<MenuProfile>,
}

impl MenuModel {
    /// Enabled profiles, plus the active one if the device made a disabled
    /// profile active, under the names chosen in `settings`.
    pub fn of(state: &MouseState, texts: &Texts, settings: &Settings) -> Self {
        match state {
            MouseState::Searching => Self::status_only(texts.searching),
            MouseState::Absent { asleep: true } => Self::status_only(texts.mouse_asleep),
            MouseState::Absent { asleep: false } => Self::status_only(texts.no_mouse),
            MouseState::Present(view) => Self {
                status: view.unreachable.then_some(texts.mouse_asleep),
                profiles: view
                    .profiles
                    .iter()
                    .filter(|profile| profile.enabled || view.active == Some(profile.sector))
                    .map(|profile| MenuProfile {
                        sector: profile.sector,
                        label: profile_label(profile, texts, settings),
                        checked: view.active == Some(profile.sector),
                    })
                    .collect(),
            },
        }
    }

    fn status_only(status: &'static str) -> Self {
        Self {
            status: Some(status),
            profiles: Vec::new(),
        }
    }
}

/// The name the user gave the profile, or "Profile n".
pub fn profile_label(profile: &Profile, texts: &Texts, settings: &Settings) -> String {
    if let Some(name) = settings
        .profiles
        .get(&profile.sector)
        .and_then(|chosen| chosen.name.as_deref())
        .filter(|name| !name.trim().is_empty())
    {
        return name.to_owned();
    }
    let template = if profile.is_rom() {
        texts.factory_profile
    } else {
        texts.profile
    };
    Texts::numbered(template, profile.number())
}

/// Tooltip of the tray icon: the app name, then what the menu would say.
pub fn tooltip(state: &MouseState, texts: &Texts, settings: &Settings) -> String {
    let menu = MenuModel::of(state, texts, settings);
    let detail = menu
        .profiles
        .iter()
        .find(|profile| profile.checked)
        .map(|profile| profile.label.as_str())
        .or(menu.status)
        .unwrap_or_default();
    if detail.is_empty() {
        "palaswitch".to_owned()
    } else {
        format!("palaswitch\n{detail}")
    }
}

#[cfg(test)]
mod tests {
    use palaswitch_hidpp::{DeviceError, TransportError};

    use super::*;
    use crate::text::{Language, texts};

    /// The profiles of the G502 X PLUS as `palaswitch-cli list` read them
    /// on 2026-11-20: five slots, the first three enabled.
    fn g502_profiles() -> Vec<Profile> {
        (1..=5)
            .map(|sector| Profile {
                sector,
                enabled: sector <= 3,
            })
            .collect()
    }

    fn present(active: Option<u16>, unreachable: bool) -> MouseState {
        MouseState::Present(MouseView {
            name: "G502 X PLUS".to_owned(),
            connection: Connection::Receiver(1),
            profiles: g502_profiles(),
            active,
            unreachable,
        })
    }

    fn labels(model: &MenuModel) -> Vec<(&str, bool)> {
        model
            .profiles
            .iter()
            .map(|profile| (profile.label.as_str(), profile.checked))
            .collect()
    }

    #[test]
    fn menu_lists_enabled_profiles_with_the_active_one_checked() {
        let model = MenuModel::of(
            &present(Some(2), false),
            texts(Language::French),
            &Settings::default(),
        );

        assert_eq!(model.status, None);
        assert_eq!(
            labels(&model),
            [("Profil 1", false), ("Profil 2", true), ("Profil 3", false)]
        );
    }

    #[test]
    fn menu_keeps_a_disabled_profile_the_device_made_active() {
        let model = MenuModel::of(
            &present(Some(5), false),
            texts(Language::English),
            &Settings::default(),
        );

        assert_eq!(
            labels(&model),
            [
                ("Profile 1", false),
                ("Profile 2", false),
                ("Profile 3", false),
                ("Profile 5", true)
            ]
        );
    }

    #[test]
    fn menu_uses_the_names_chosen_in_the_settings() {
        let settings = crate::config::parse(
            "[[profile]]
sector = 2
name = \"Jeu\"
[[profile]]
sector = 3
name = \"  \"",
        )
        .expect("settings");

        let model = MenuModel::of(&present(Some(2), false), texts(Language::French), &settings);

        assert_eq!(
            labels(&model),
            [("Profil 1", false), ("Jeu", true), ("Profil 3", false)]
        );
    }

    #[test]
    fn menu_names_factory_profiles() {
        let state = MouseState::Present(MouseView {
            name: String::new(),
            connection: Connection::Wired,
            profiles: vec![Profile {
                sector: 0x0101,
                enabled: true,
            }],
            active: Some(0x0101),
            unreachable: false,
        });

        let model = MenuModel::of(&state, texts(Language::French), &Settings::default());

        assert_eq!(labels(&model), [("Profil d’usine 1", true)]);
        assert_eq!(IconKind::of(&state), IconKind::Profile(1));
    }

    #[test]
    fn sleeping_mouse_keeps_its_profiles_under_a_hint() {
        let texts = texts(Language::French);

        let model = MenuModel::of(&present(Some(1), true), texts, &Settings::default());

        assert_eq!(model.status, Some(texts.mouse_asleep));
        assert_eq!(model.profiles.len(), 3);
    }

    #[test]
    fn menu_without_mouse_only_says_why() {
        let texts = texts(Language::French);
        let states = [
            MouseState::Searching,
            MouseState::Absent { asleep: false },
            MouseState::Absent { asleep: true },
        ];

        let models = states.map(|state| MenuModel::of(&state, texts, &Settings::default()));

        assert!(models.iter().all(|model| model.profiles.is_empty()));
        assert_eq!(
            models.map(|model| model.status),
            [
                Some(texts.searching),
                Some(texts.no_mouse),
                Some(texts.mouse_asleep)
            ]
        );
    }

    #[test]
    fn icon_shows_the_profile_number_or_no_mouse() {
        let states = [
            present(Some(3), false),
            present(None, false),
            MouseState::Searching,
            MouseState::Absent { asleep: true },
        ];

        assert_eq!(
            states.map(|state| IconKind::of(&state)),
            [
                IconKind::Profile(3),
                IconKind::Unknown,
                IconKind::Unknown,
                IconKind::NoMouse
            ]
        );
    }

    #[test]
    fn tooltip_names_the_active_profile_or_the_problem() {
        let texts = texts(Language::English);

        assert_eq!(
            [
                tooltip(&present(Some(2), false), texts, &Settings::default()),
                tooltip(
                    &MouseState::Absent { asleep: false },
                    texts,
                    &Settings::default()
                ),
                tooltip(&present(None, false), texts, &Settings::default()),
            ],
            [
                "palaswitch\nProfile 2",
                "palaswitch\nMouse not found",
                "palaswitch"
            ]
        );
    }

    #[test]
    fn failures_say_what_to_do() {
        let errors = [
            Error::Device(DeviceError::Hidpp10(0x08)),
            Error::Timeout,
            Error::Transport(TransportError("gone".to_owned())),
            Error::ProfileNotApplied {
                requested: 2,
                active: 1,
            },
        ];

        assert_eq!(
            errors.map(|error| Failure::of(&error)),
            [
                Failure::Asleep,
                Failure::Asleep,
                Failure::NoMouse,
                Failure::Refused
            ]
        );
    }
}
