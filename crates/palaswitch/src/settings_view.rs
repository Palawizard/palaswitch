//! What the settings window shows and what each change does to the
//! settings, without any Windows API.

use palaswitch_device::Connection;

use crate::config::{MAX_NAME_LEN, MAX_SECTOR, ProfileSettings, Settings};
use crate::model::{MouseState, profile_label};
use crate::shortcut::{Refusal, Shortcut};
use crate::text::Texts;

/// One profile line of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub sector: u16,
    /// Shown greyed in the empty name field: "Profile 1".
    pub default_name: String,
    /// The name the user chose, or empty.
    pub name: String,
    pub shortcut: Option<Shortcut>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// Mouse name, or why there is none.
    pub title: String,
    /// Connection and active profile, or what to do.
    pub detail: String,
    pub rows: Vec<Row>,
}

impl View {
    pub fn of(state: &MouseState, settings: &Settings, texts: &Texts) -> Self {
        let MouseState::Present(view) = state else {
            let title = match state {
                MouseState::Absent { asleep: true } => texts.mouse_asleep,
                MouseState::Searching => texts.searching,
                _ => texts.no_mouse,
            };
            return Self {
                title: title.to_owned(),
                detail: texts.settings_no_mouse_hint.to_owned(),
                rows: Vec::new(),
            };
        };
        let connection = match view.connection {
            Connection::Wired => texts.connection_wired,
            Connection::Receiver(_) => texts.connection_receiver,
        };
        let active = view
            .active
            .and_then(|sector| {
                view.profiles
                    .iter()
                    .find(|profile| profile.sector == sector)
            })
            .map(|profile| profile_label(profile, texts, settings));
        let detail = match active {
            Some(active) => format!(
                "{connection} · {}",
                texts.settings_active.replace("{name}", &active)
            ),
            None => connection.to_owned(),
        };
        let rows = view
            .profiles
            .iter()
            .filter(|profile| profile.enabled || view.active == Some(profile.sector))
            .filter(|profile| profile.sector <= MAX_SECTOR)
            .map(|profile| {
                let chosen = settings.profiles.get(&profile.sector);
                Row {
                    sector: profile.sector,
                    default_name: profile_label(profile, texts, &Settings::default()),
                    name: chosen
                        .and_then(|chosen| chosen.name.clone())
                        .unwrap_or_default(),
                    shortcut: chosen.and_then(|chosen| chosen.shortcut),
                }
            })
            .collect();
        let title = if view.name.is_empty() {
            texts.settings_unnamed_mouse.to_owned()
        } else {
            view.name.clone()
        };
        Self {
            title,
            detail,
            rows,
        }
    }
}

/// `settings` with the name of `sector` set to `text`; an empty or blank
/// text goes back to the default name. Text past the length limit is cut.
pub fn rename(settings: &Settings, sector: u16, text: &str) -> Settings {
    let name: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_LEN)
        .collect();
    let name = name.trim_end().to_owned();
    let mut settings = settings.clone();
    let profile = settings.profiles.entry(sector).or_default();
    profile.name = (!name.is_empty()).then_some(name);
    drop_if_empty(&mut settings, sector);
    settings
}

/// Why a typed shortcut cannot be given to a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutProblem {
    Refused(Refusal),
    /// Another profile has it; its shown name.
    UsedBy(String),
    /// Windows says another program holds it.
    Taken,
    /// Windows refused it for another reason.
    RefusedByWindows,
}

impl ShortcutProblem {
    pub fn message(&self, texts: &Texts) -> String {
        match self {
            Self::Refused(Refusal::NeedsModifier) => texts.shortcut_needs_modifier.to_owned(),
            Self::Refused(Refusal::WinKey) => texts.shortcut_win_key.to_owned(),
            Self::Refused(Refusal::Reserved) => texts.shortcut_reserved.to_owned(),
            Self::UsedBy(name) => texts.shortcut_used_by.replace("{name}", name),
            Self::Taken => texts.shortcut_taken.to_owned(),
            Self::RefusedByWindows => texts.shortcut_windows_refused.to_owned(),
        }
    }
}

/// `settings` with `shortcut` given to `sector`, unless it is refused or
/// already belongs to another profile. `names` gives the shown name of a
/// sector.
pub fn give_shortcut(
    settings: &Settings,
    sector: u16,
    shortcut: Shortcut,
    names: impl Fn(u16) -> String,
) -> Result<Settings, ShortcutProblem> {
    let shortcut = shortcut.check().map_err(ShortcutProblem::Refused)?;
    if let Some((&other, _)) = settings
        .profiles
        .iter()
        .find(|&(&other, profile)| other != sector && profile.shortcut == Some(shortcut))
    {
        return Err(ShortcutProblem::UsedBy(names(other)));
    }
    let mut settings = settings.clone();
    settings.profiles.entry(sector).or_default().shortcut = Some(shortcut);
    Ok(settings)
}

pub fn clear_shortcut(settings: &Settings, sector: u16) -> Settings {
    let mut settings = settings.clone();
    if let Some(profile) = settings.profiles.get_mut(&sector) {
        profile.shortcut = None;
    }
    drop_if_empty(&mut settings, sector);
    settings
}

fn drop_if_empty(settings: &mut Settings, sector: u16) {
    if settings.profiles.get(&sector) == Some(&ProfileSettings::default()) {
        settings.profiles.remove(&sector);
    }
}

#[cfg(test)]
mod tests {
    use palaswitch_hidpp::features::Profile;

    use super::*;
    use crate::model::MouseView;
    use crate::text::{Language, texts};

    /// The profiles of the G502 X PLUS as `palaswitch-cli list` read them
    /// on 2026-11-20: five slots, the first three enabled.
    fn present(active: Option<u16>) -> MouseState {
        MouseState::Present(MouseView {
            name: "G502 X PLUS".to_owned(),
            connection: Connection::Receiver(1),
            profiles: (1..=5)
                .map(|sector| Profile {
                    sector,
                    enabled: sector <= 3,
                })
                .collect(),
            active,
            unreachable: false,
        })
    }

    fn shortcut(text: &str) -> Shortcut {
        text.parse().expect("shortcut")
    }

    fn named(sector: u16, name: &str) -> Settings {
        rename(&Settings::default(), sector, name)
    }

    #[test]
    fn rows_list_enabled_profiles_with_their_settings() {
        let settings = give_shortcut(&named(2, "Jeu"), 2, shortcut("Ctrl+Alt+F2"), |_| {
            String::new()
        })
        .expect("given");

        let view = View::of(&present(Some(2)), &settings, texts(Language::French));

        assert_eq!(view.title, "G502 X PLUS");
        assert_eq!(view.detail, "Récepteur sans fil · Profil actif : Jeu");
        assert_eq!(
            view.rows
                .iter()
                .map(|row| (row.default_name.as_str(), row.name.as_str(), row.shortcut))
                .collect::<Vec<_>>(),
            [
                ("Profil 1", "", None),
                ("Profil 2", "Jeu", Some(shortcut("Ctrl+Alt+F2"))),
                ("Profil 3", "", None)
            ]
        );
    }

    #[test]
    fn without_a_mouse_the_window_says_what_to_do() {
        let texts = texts(Language::English);

        let view = View::of(
            &MouseState::Absent { asleep: false },
            &Settings::default(),
            texts,
        );

        assert_eq!(
            (view.title.as_str(), view.detail.as_str(), view.rows.len()),
            (texts.no_mouse, texts.settings_no_mouse_hint, 0)
        );
    }

    #[test]
    fn renaming_trims_cuts_and_forgets_blank_names() {
        let long = "x".repeat(MAX_NAME_LEN + 5);

        let renamed = rename(&Settings::default(), 1, &format!("  {long}"));
        let blanked = rename(&renamed, 1, "   ");

        assert_eq!(
            renamed.profiles[&1]
                .name
                .as_ref()
                .map(|name| name.chars().count()),
            Some(MAX_NAME_LEN)
        );
        assert_eq!(blanked, Settings::default());
    }

    #[test]
    fn a_shortcut_of_another_profile_is_refused_with_its_name() {
        let settings = give_shortcut(&Settings::default(), 1, shortcut("Ctrl+Alt+F1"), |_| {
            String::new()
        })
        .expect("given");

        let result = give_shortcut(&settings, 2, shortcut("Ctrl+Alt+F1"), |sector| {
            format!("Profil {sector}")
        });

        assert_eq!(result, Err(ShortcutProblem::UsedBy("Profil 1".to_owned())));
    }

    #[test]
    fn a_profile_can_take_its_own_shortcut_again_and_lose_it() {
        let names = |_| String::new();
        let first =
            give_shortcut(&Settings::default(), 1, shortcut("Ctrl+Alt+F1"), names).expect("given");

        let again = give_shortcut(&first, 1, shortcut("Ctrl+Alt+F1"), names);
        let cleared = clear_shortcut(&first, 1);

        assert_eq!(again, Ok(first));
        assert_eq!(cleared, Settings::default());
    }

    #[test]
    fn refusals_have_their_own_message() {
        let texts = texts(Language::French);

        let messages = [
            ShortcutProblem::Refused(Refusal::NeedsModifier),
            ShortcutProblem::Refused(Refusal::WinKey),
            ShortcutProblem::UsedBy("Jeu".to_owned()),
            ShortcutProblem::Taken,
            ShortcutProblem::Refused(Refusal::Reserved),
            ShortcutProblem::RefusedByWindows,
        ]
        .map(|problem| problem.message(texts));

        assert_eq!(messages[2], "Déjà utilisé par Jeu.");
        assert!(messages.iter().all(|message| !message.contains('{')));
    }
}
