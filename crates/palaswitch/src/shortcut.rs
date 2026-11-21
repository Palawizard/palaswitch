//! Keyboard shortcuts: what the settings file stores, what the user sees,
//! and what is registered with Windows.

use std::fmt;
use std::str::FromStr;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};

use crate::text::Texts;

/// Modifier keys held with the key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

/// How a key is shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Label {
    /// The same everywhere: a letter, a digit, a function key.
    Plain(&'static str),
    /// A key of the numeric keypad, shown after the keypad prefix.
    Keypad(&'static str),
    /// A key whose name depends on the language.
    Named(Name),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Name {
    Space,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
}

impl Name {
    fn shown(self, texts: &Texts) -> &'static str {
        match self {
            Self::Space => texts.key_space,
            Self::Insert => texts.key_insert,
            Self::Delete => texts.key_delete,
            Self::Home => texts.key_home,
            Self::End => texts.key_end,
            Self::PageUp => texts.key_page_up,
            Self::PageDown => texts.key_page_down,
            Self::Left => texts.key_left,
            Self::Right => texts.key_right,
            Self::Up => texts.key_up,
            Self::Down => texts.key_down,
        }
    }
}

/// A key that can end a shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// Windows virtual-key code.
    vk: u16,
    code: Code,
    /// Name in the settings file.
    id: &'static str,
    label: Label,
}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.vk.hash(state);
    }
}

macro_rules! plain {
    ($vk:expr, $code:ident, $id:literal) => {
        Key {
            vk: $vk,
            code: Code::$code,
            id: $id,
            label: Label::Plain($id),
        }
    };
}

macro_rules! keypad {
    ($vk:expr, $code:ident, $id:literal, $shown:literal) => {
        Key {
            vk: $vk,
            code: Code::$code,
            id: $id,
            label: Label::Keypad($shown),
        }
    };
}

macro_rules! named {
    ($vk:expr, $code:ident, $id:literal, $name:ident) => {
        Key {
            vk: $vk,
            code: Code::$code,
            id: $id,
            label: Label::Named(Name::$name),
        }
    };
}

/// Every key a shortcut can end with.
static KEYS: &[Key] = &[
    plain!(0x41, KeyA, "A"),
    plain!(0x42, KeyB, "B"),
    plain!(0x43, KeyC, "C"),
    plain!(0x44, KeyD, "D"),
    plain!(0x45, KeyE, "E"),
    plain!(0x46, KeyF, "F"),
    plain!(0x47, KeyG, "G"),
    plain!(0x48, KeyH, "H"),
    plain!(0x49, KeyI, "I"),
    plain!(0x4A, KeyJ, "J"),
    plain!(0x4B, KeyK, "K"),
    plain!(0x4C, KeyL, "L"),
    plain!(0x4D, KeyM, "M"),
    plain!(0x4E, KeyN, "N"),
    plain!(0x4F, KeyO, "O"),
    plain!(0x50, KeyP, "P"),
    plain!(0x51, KeyQ, "Q"),
    plain!(0x52, KeyR, "R"),
    plain!(0x53, KeyS, "S"),
    plain!(0x54, KeyT, "T"),
    plain!(0x55, KeyU, "U"),
    plain!(0x56, KeyV, "V"),
    plain!(0x57, KeyW, "W"),
    plain!(0x58, KeyX, "X"),
    plain!(0x59, KeyY, "Y"),
    plain!(0x5A, KeyZ, "Z"),
    plain!(0x30, Digit0, "0"),
    plain!(0x31, Digit1, "1"),
    plain!(0x32, Digit2, "2"),
    plain!(0x33, Digit3, "3"),
    plain!(0x34, Digit4, "4"),
    plain!(0x35, Digit5, "5"),
    plain!(0x36, Digit6, "6"),
    plain!(0x37, Digit7, "7"),
    plain!(0x38, Digit8, "8"),
    plain!(0x39, Digit9, "9"),
    plain!(0x70, F1, "F1"),
    plain!(0x71, F2, "F2"),
    plain!(0x72, F3, "F3"),
    plain!(0x73, F4, "F4"),
    plain!(0x74, F5, "F5"),
    plain!(0x75, F6, "F6"),
    plain!(0x76, F7, "F7"),
    plain!(0x77, F8, "F8"),
    plain!(0x78, F9, "F9"),
    plain!(0x79, F10, "F10"),
    plain!(0x7A, F11, "F11"),
    plain!(0x7B, F12, "F12"),
    plain!(0x7C, F13, "F13"),
    plain!(0x7D, F14, "F14"),
    plain!(0x7E, F15, "F15"),
    plain!(0x7F, F16, "F16"),
    plain!(0x80, F17, "F17"),
    plain!(0x81, F18, "F18"),
    plain!(0x82, F19, "F19"),
    plain!(0x83, F20, "F20"),
    plain!(0x84, F21, "F21"),
    plain!(0x85, F22, "F22"),
    plain!(0x86, F23, "F23"),
    plain!(0x87, F24, "F24"),
    keypad!(0x60, Numpad0, "Num0", "0"),
    keypad!(0x61, Numpad1, "Num1", "1"),
    keypad!(0x62, Numpad2, "Num2", "2"),
    keypad!(0x63, Numpad3, "Num3", "3"),
    keypad!(0x64, Numpad4, "Num4", "4"),
    keypad!(0x65, Numpad5, "Num5", "5"),
    keypad!(0x66, Numpad6, "Num6", "6"),
    keypad!(0x67, Numpad7, "Num7", "7"),
    keypad!(0x68, Numpad8, "Num8", "8"),
    keypad!(0x69, Numpad9, "Num9", "9"),
    keypad!(0x6A, NumpadMultiply, "NumMultiply", "*"),
    keypad!(0x6B, NumpadAdd, "NumAdd", "+"),
    keypad!(0x6D, NumpadSubtract, "NumSubtract", "-"),
    keypad!(0x6E, NumpadDecimal, "NumDecimal", "."),
    keypad!(0x6F, NumpadDivide, "NumDivide", "/"),
    named!(0x20, Space, "Space", Space),
    named!(0x2D, Insert, "Insert", Insert),
    named!(0x2E, Delete, "Delete", Delete),
    named!(0x24, Home, "Home", Home),
    named!(0x23, End, "End", End),
    named!(0x21, PageUp, "PageUp", PageUp),
    named!(0x22, PageDown, "PageDown", PageDown),
    named!(0x25, ArrowLeft, "Left", Left),
    named!(0x27, ArrowRight, "Right", Right),
    named!(0x26, ArrowUp, "Up", Up),
    named!(0x28, ArrowDown, "Down", Down),
];

impl Key {
    /// The key for a Windows virtual-key code, if shortcuts can use it.
    pub fn vk(self) -> u16 {
        self.vk
    }

    pub fn from_vk(vk: u16) -> Option<Self> {
        KEYS.iter().copied().find(|key| key.vk == vk)
    }

    fn from_id(id: &str) -> Option<Self> {
        KEYS.iter()
            .copied()
            .find(|key| key.id.eq_ignore_ascii_case(id))
    }

    /// F13 to F24 exist on no common keyboard, so they never collide with
    /// typing: they may be used without a modifier.
    fn is_spare_function_key(self) -> bool {
        (0x7C..=0x87).contains(&self.vk)
    }

    fn shown(self, texts: &Texts) -> String {
        match self.label {
            Label::Plain(name) => name.to_owned(),
            Label::Keypad(name) => format!("{} {name}", texts.key_keypad),
            Label::Named(name) => name.shown(texts).to_owned(),
        }
    }
}

/// A key with its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub mods: Mods,
    pub key: Key,
}

/// Why a shortcut is refused before even asking Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A key alone would fire while typing.
    NeedsModifier,
    /// Windows keeps the Win key combinations for itself.
    WinKey,
    /// Windows uses it everywhere: taking it would break it in every app.
    Reserved,
    /// Ctrl+Alt is also AltGr: on the active keyboard layout, it types a
    /// character that no app could get any more.
    AltGr,
}

/// Combinations Windows uses in every app, which a global shortcut would
/// take away from them, in the settings file form.
const RESERVED: &[&str] = &["Alt+F4", "Alt+Space", "Ctrl+Alt+Delete"];

impl Shortcut {
    pub fn check(self) -> Result<Self, Refusal> {
        if self.mods.win {
            return Err(Refusal::WinKey);
        }
        let only_shift = !self.mods.ctrl && !self.mods.alt;
        if only_shift && !self.key.is_spare_function_key() {
            return Err(Refusal::NeedsModifier);
        }
        if RESERVED.contains(&self.to_string().as_str()) {
            return Err(Refusal::Reserved);
        }
        Ok(self)
    }

    /// [`Self::check`], and also refuses a Ctrl+Alt shortcut for which
    /// `types_with_altgr` says the keyboard types a character.
    pub fn check_typed(self, types_with_altgr: impl Fn(Self) -> bool) -> Result<Self, Refusal> {
        let shortcut = self.check()?;
        if shortcut.mods.ctrl && shortcut.mods.alt && types_with_altgr(shortcut) {
            return Err(Refusal::AltGr);
        }
        Ok(shortcut)
    }

    /// As shown to the user, in the language of `texts`.
    pub fn shown(self, texts: &Texts) -> String {
        let mut parts = mods_shown(self.mods, texts);
        parts.push(self.key.shown(texts));
        parts.join("+")
    }

    pub fn hotkey(self) -> HotKey {
        let mut mods = Modifiers::empty();
        mods.set(Modifiers::CONTROL, self.mods.ctrl);
        mods.set(Modifiers::ALT, self.mods.alt);
        mods.set(Modifiers::SHIFT, self.mods.shift);
        mods.set(Modifiers::META, self.mods.win);
        HotKey::new(Some(mods), self.key.code)
    }
}

/// Modifier names in the order Windows shows them.
pub fn mods_shown(mods: Mods, texts: &Texts) -> Vec<String> {
    [
        (mods.ctrl, texts.key_ctrl),
        (mods.alt, texts.key_alt),
        (mods.shift, texts.key_shift),
        (mods.win, texts.key_win),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .map(|(_, name)| name.to_owned())
    .collect()
}

/// The settings file form: modifiers then the key, joined by `+`, such as
/// `Ctrl+Alt+F1`.
impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (held, name) in [
            (self.mods.ctrl, "Ctrl"),
            (self.mods.alt, "Alt"),
            (self.mods.shift, "Shift"),
            (self.mods.win, "Win"),
        ] {
            if held {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(self.key.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a shortcut: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl FromStr for Shortcut {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || ParseError(text.to_owned());
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = Key::from_id(parts.pop().ok_or_else(error)?).ok_or_else(error)?;
        let mut mods = Mods::default();
        for part in parts {
            let held = match part.to_ascii_lowercase().as_str() {
                "ctrl" => &mut mods.ctrl,
                "alt" => &mut mods.alt,
                "shift" => &mut mods.shift,
                "win" => &mut mods.win,
                _ => return Err(error()),
            };
            if *held {
                return Err(error());
            }
            *held = true;
        }
        Ok(Self { mods, key })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{Language, texts};

    fn shortcut(text: &str) -> Shortcut {
        text.parse().expect("shortcut")
    }

    #[test]
    fn settings_form_reads_back_the_same() {
        for text in [
            "Ctrl+Alt+F1",
            "Ctrl+Shift+Num1",
            "Alt+PageDown",
            "F13",
            "Ctrl+Space",
        ] {
            assert_eq!(shortcut(text).to_string(), text);
        }
    }

    #[test]
    fn parsing_ignores_case_and_spaces() {
        assert_eq!(shortcut(" ctrl + alt + f1 "), shortcut("Ctrl+Alt+F1"));
    }

    #[test]
    fn parsing_refuses_unknown_keys_and_repeated_modifiers() {
        for text in [
            "",
            "Ctrl+",
            "Ctrl+Ctrl+A",
            "Hyper+A",
            "Ctrl+Tab",
            "Ctrl+Alt",
        ] {
            assert!(text.parse::<Shortcut>().is_err(), "{text}");
        }
    }

    #[test]
    fn shown_in_the_language_of_windows() {
        let shortcut = shortcut("Ctrl+Shift+Num1");

        assert_eq!(
            [
                shortcut.shown(texts(Language::French)),
                shortcut.shown(texts(Language::English))
            ],
            ["Ctrl+Maj+Pavé num. 1", "Ctrl+Shift+Num 1"]
        );
    }

    #[test]
    fn keys_without_ctrl_or_alt_are_refused_except_spare_function_keys() {
        let checks = [
            "A",
            "Shift+A",
            "Shift+F1",
            "F13",
            "Alt+A",
            "Ctrl+Win+A",
            "Alt+F4",
            "Alt+Space",
            "Ctrl+Alt+Delete",
        ]
        .map(|text| shortcut(text).check().map(|_| ()));

        assert_eq!(
            checks,
            [
                Err(Refusal::NeedsModifier),
                Err(Refusal::NeedsModifier),
                Err(Refusal::NeedsModifier),
                Ok(()),
                Ok(()),
                Err(Refusal::WinKey),
                Err(Refusal::Reserved),
                Err(Refusal::Reserved),
                Err(Refusal::Reserved)
            ]
        );
    }

    #[test]
    fn ctrl_alt_keys_that_type_with_altgr_are_refused() {
        // Stands for a French layout, where AltGr+0 types @ and AltGr+F1
        // types nothing.
        let types = |shortcut: Shortcut| shortcut.key.id == "0";

        let checks = ["Ctrl+Alt+0", "Ctrl+Alt+F1", "Ctrl+0", "Alt+0"]
            .map(|text| shortcut(text).check_typed(types).map(|_| ()));

        assert_eq!(checks, [Err(Refusal::AltGr), Ok(()), Ok(()), Ok(())]);
    }

    #[test]
    fn every_key_has_a_distinct_code_and_name() {
        for (index, key) in KEYS.iter().enumerate() {
            for other in &KEYS[index + 1..] {
                assert!(key.vk != other.vk && key.code != other.code && key.id != other.id);
            }
            assert_eq!(Key::from_vk(key.vk), Some(*key));
        }
    }

    #[test]
    fn hotkeys_of_two_profiles_differ() {
        assert_ne!(
            shortcut("Ctrl+Alt+F1").hotkey().id(),
            shortcut("Ctrl+Alt+F2").hotkey().id()
        );
    }
}
