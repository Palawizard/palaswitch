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

#[derive(Debug)]
pub struct Texts {
    pub quit: &'static str,
}

pub fn texts(language: Language) -> &'static Texts {
    match language {
        Language::French => &FRENCH,
        Language::English => &ENGLISH,
    }
}

static FRENCH: Texts = Texts { quit: "Quitter" };

static ENGLISH: Texts = Texts { quit: "Quit" };

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
}
