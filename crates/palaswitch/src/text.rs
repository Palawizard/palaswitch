//! Every string shown to the user, in French and English.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    French,
    English,
}

const LANG_FRENCH: u16 = 0x0C;

impl Language {
    /// The language named by `PALASWITCH_LANG` (`fr` or `en`), used to see
    /// the other language without changing the one of Windows.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "fr" => Some(Self::French),
            "en" => Some(Self::English),
            _ => None,
        }
    }

    /// French when the Windows UI language is French, English otherwise.
    pub fn from_windows_lang_id(lang_id: u16) -> Self {
        // The primary language is the low 10 bits of a LANGID.
        if lang_id & 0x3FF == LANG_FRENCH {
            Self::French
        } else {
            Self::English
        }
    }
}

/// Texts of one language. `{n}` in a text stands for a profile number.
#[derive(Debug)]
pub struct Texts {
    pub quit: &'static str,
    pub profile: &'static str,
    pub factory_profile: &'static str,
    pub searching: &'static str,
    pub no_mouse: &'static str,
    pub mouse_asleep: &'static str,
    pub switch_failed_title: &'static str,
    pub switch_failed_asleep: &'static str,
    pub switch_failed_no_mouse: &'static str,
    pub switch_failed_other: &'static str,
    pub switch_failed_stopped: &'static str,
    pub settings_reset_title: &'static str,
    pub settings_reset_message: &'static str,
    pub shortcut_taken_title: &'static str,
    pub shortcut_taken_message: &'static str,
    pub shortcut_taken_here: &'static str,
    pub settings_menu: &'static str,
    pub settings_title: &'static str,
    pub settings_no_mouse_hint: &'static str,
    pub settings_unnamed_mouse: &'static str,
    pub settings_active: &'static str,
    pub settings_name_header: &'static str,
    pub settings_shortcut_header: &'static str,
    pub connection_receiver: &'static str,
    pub connection_wired: &'static str,
    pub shortcut_none: &'static str,
    pub shortcut_listening: &'static str,
    pub shortcut_clear: &'static str,
    pub shortcut_needs_modifier: &'static str,
    pub shortcut_win_key: &'static str,
    pub shortcut_used_by: &'static str,
    pub shortcut_taken: &'static str,
    pub start_with_windows: &'static str,
    pub save_failed: &'static str,
    pub autostart_failed: &'static str,
    pub key_ctrl: &'static str,
    pub key_alt: &'static str,
    pub key_shift: &'static str,
    pub key_win: &'static str,
    pub key_keypad: &'static str,
    pub key_space: &'static str,
    pub key_insert: &'static str,
    pub key_delete: &'static str,
    pub key_home: &'static str,
    pub key_end: &'static str,
    pub key_page_up: &'static str,
    pub key_page_down: &'static str,
    pub key_left: &'static str,
    pub key_right: &'static str,
    pub key_up: &'static str,
    pub key_down: &'static str,
}

impl Texts {
    /// `template` with its `{n}` replaced by `number`.
    pub fn numbered(template: &str, number: u16) -> String {
        template.replace("{n}", &number.to_string())
    }
}

pub fn texts(language: Language) -> &'static Texts {
    match language {
        Language::French => &FRENCH,
        Language::English => &ENGLISH,
    }
}

static FRENCH: Texts = Texts {
    quit: "Quitter",
    profile: "Profil {n}",
    factory_profile: "Profil d’usine {n}",
    searching: "Recherche de la souris…",
    no_mouse: "Souris introuvable",
    mouse_asleep: "Souris en veille ou éteinte",
    switch_failed_title: "Profil inchangé",
    switch_failed_asleep: "Bouge ou rallume la souris, puis réessaie.",
    switch_failed_no_mouse: "Souris introuvable. Rebranche-la ou rebranche le récepteur.",
    switch_failed_other: "La souris a refusé le changement. Réessaie.",
    switch_failed_stopped: "palaswitch ne répond plus. Quitte-le et relance-le.",
    settings_reset_title: "Réglages remis à zéro",
    settings_reset_message: "Le fichier de réglages était illisible. L’ancien est gardé à côté, en .bak.",
    shortcut_taken_title: "Raccourci déjà pris",
    shortcut_taken_message: "{list} : déjà utilisé par un autre programme. Choisis-en un autre dans les réglages.",
    shortcut_taken_here: "{list} : déjà utilisé par un autre programme.",
    settings_menu: "Réglages…",
    settings_title: "Réglages de palaswitch",
    settings_no_mouse_hint: "Branche la souris ou son récepteur pour régler ses profils.",
    settings_unnamed_mouse: "Souris sans nom",
    settings_active: "Profil actif : {name}",
    settings_name_header: "Nom affiché",
    settings_shortcut_header: "Raccourci",
    connection_receiver: "Récepteur sans fil",
    connection_wired: "Câble USB",
    shortcut_none: "Aucun",
    shortcut_listening: "Appuie sur les touches…",
    shortcut_clear: "Effacer",
    shortcut_needs_modifier: "Ajoute Ctrl ou Alt : seules F13 à F24 marchent seules.",
    shortcut_win_key: "Les combinaisons avec Win sont réservées à Windows.",
    shortcut_used_by: "Déjà utilisé par {name}.",
    shortcut_taken: "Déjà utilisé par un autre programme.",
    start_with_windows: "Lancer avec Windows",
    save_failed: "Réglages non enregistrés. Réessaie.",
    autostart_failed: "Lancement avec Windows non modifié. Réessaie.",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
    key_shift: "Maj",
    key_win: "Win",
    key_keypad: "Pavé num.",
    key_space: "Espace",
    key_insert: "Inser",
    key_delete: "Suppr",
    key_home: "Début",
    key_end: "Fin",
    key_page_up: "Pg préc",
    key_page_down: "Pg suiv",
    key_left: "Gauche",
    key_right: "Droite",
    key_up: "Haut",
    key_down: "Bas",
};

static ENGLISH: Texts = Texts {
    quit: "Quit",
    profile: "Profile {n}",
    factory_profile: "Factory profile {n}",
    searching: "Looking for the mouse…",
    no_mouse: "Mouse not found",
    mouse_asleep: "Mouse asleep or switched off",
    switch_failed_title: "Profile not changed",
    switch_failed_asleep: "Move the mouse or switch it on, then try again.",
    switch_failed_no_mouse: "Mouse not found. Plug it or its receiver back in.",
    switch_failed_other: "The mouse refused the change. Try again.",
    switch_failed_stopped: "palaswitch stopped responding. Quit it and start it again.",
    settings_reset_title: "Settings reset",
    settings_reset_message: "The settings file could not be read. The old one is kept next to it, as .bak.",
    shortcut_taken_title: "Shortcut already in use",
    shortcut_taken_message: "{list}: already used by another program. Pick another one in the settings.",
    shortcut_taken_here: "{list}: already used by another program.",
    settings_menu: "Settings…",
    settings_title: "palaswitch settings",
    settings_no_mouse_hint: "Plug in the mouse or its receiver to set up its profiles.",
    settings_unnamed_mouse: "Unnamed mouse",
    settings_active: "Active profile: {name}",
    settings_name_header: "Display name",
    settings_shortcut_header: "Shortcut",
    connection_receiver: "Wireless receiver",
    connection_wired: "USB cable",
    shortcut_none: "None",
    shortcut_listening: "Press the keys…",
    shortcut_clear: "Clear",
    shortcut_needs_modifier: "Add Ctrl or Alt: only F13 to F24 work alone.",
    shortcut_win_key: "Win key combinations belong to Windows.",
    shortcut_used_by: "Already used by {name}.",
    shortcut_taken: "Already used by another program.",
    start_with_windows: "Start with Windows",
    save_failed: "Settings not saved. Try again.",
    autostart_failed: "Start with Windows not changed. Try again.",
    key_ctrl: "Ctrl",
    key_alt: "Alt",
    key_shift: "Shift",
    key_win: "Win",
    key_keypad: "Num",
    key_space: "Space",
    key_insert: "Insert",
    key_delete: "Delete",
    key_home: "Home",
    key_end: "End",
    key_page_up: "Page Up",
    key_page_down: "Page Down",
    key_left: "Left",
    key_right: "Right",
    key_up: "Up",
    key_down: "Down",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_france_and_french_canada_give_french() {
        let languages = [0x040C, 0x0C0C].map(Language::from_windows_lang_id);

        assert_eq!(languages, [Language::French; 2]);
    }

    #[test]
    fn the_language_can_be_named() {
        assert_eq!(
            ["fr", " EN ", "de"].map(Language::from_name),
            [Some(Language::French), Some(Language::English), None]
        );
    }

    #[test]
    fn other_languages_give_english() {
        let languages = [0x0409, 0x0407, 0x0411].map(Language::from_windows_lang_id);

        assert_eq!(languages, [Language::English; 3]);
    }

    #[test]
    fn numbered_texts_have_their_number_in_both_languages() {
        for texts in [&FRENCH, &ENGLISH] {
            for template in [texts.profile, texts.factory_profile] {
                let text = Texts::numbered(template, 3);
                assert!(text.contains('3') && !text.contains('{'), "{text}");
            }
        }
    }

    #[test]
    fn list_texts_have_their_list_in_both_languages() {
        for texts in [&FRENCH, &ENGLISH] {
            assert!(texts.shortcut_taken_message.contains("{list}"));
            assert!(texts.shortcut_taken_here.contains("{list}"));
        }
    }

    #[test]
    fn no_text_is_empty() {
        for texts in [&FRENCH, &ENGLISH] {
            let all = [
                texts.quit,
                texts.profile,
                texts.factory_profile,
                texts.searching,
                texts.no_mouse,
                texts.mouse_asleep,
                texts.switch_failed_title,
                texts.switch_failed_asleep,
                texts.switch_failed_no_mouse,
                texts.switch_failed_other,
                texts.switch_failed_stopped,
                texts.settings_reset_title,
                texts.settings_reset_message,
                texts.shortcut_taken_title,
                texts.shortcut_taken_message,
                texts.shortcut_taken_here,
                texts.settings_menu,
                texts.settings_title,
                texts.settings_no_mouse_hint,
                texts.settings_unnamed_mouse,
                texts.settings_active,
                texts.settings_name_header,
                texts.settings_shortcut_header,
                texts.connection_receiver,
                texts.connection_wired,
                texts.shortcut_none,
                texts.shortcut_listening,
                texts.shortcut_clear,
                texts.shortcut_needs_modifier,
                texts.shortcut_win_key,
                texts.shortcut_used_by,
                texts.shortcut_taken,
                texts.start_with_windows,
                texts.save_failed,
                texts.autostart_failed,
                texts.key_ctrl,
                texts.key_alt,
                texts.key_shift,
                texts.key_win,
                texts.key_keypad,
                texts.key_space,
                texts.key_insert,
                texts.key_delete,
                texts.key_home,
                texts.key_end,
                texts.key_page_up,
                texts.key_page_down,
                texts.key_left,
                texts.key_right,
                texts.key_up,
                texts.key_down,
            ];
            assert!(all.iter().all(|text| !text.trim().is_empty()));
        }
    }
}
