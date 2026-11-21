//! Every string shown to the user, in French and English.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    French,
    English,
}

const LANG_FRENCH: u16 = 0x0C;

impl Language {
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
