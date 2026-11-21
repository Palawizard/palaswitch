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
            ];
            assert!(all.iter().all(|text| !text.trim().is_empty()));
        }
    }
}
