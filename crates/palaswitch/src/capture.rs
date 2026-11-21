//! Typing a shortcut into its field: modifiers show as they are held, the
//! first other key completes the shortcut.

use crate::shortcut::{Key, Mods, Shortcut};

const VK_TAB: u16 = 0x09;
const VK_ESCAPE: u16 = 0x1B;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Win,
}

fn modifier(vk: u16) -> Option<Modifier> {
    match vk {
        0x11 | 0xA2 | 0xA3 => Some(Modifier::Ctrl),
        0x12 | 0xA4 | 0xA5 => Some(Modifier::Alt),
        0x10 | 0xA0 | 0xA1 => Some(Modifier::Shift),
        0x5B | 0x5C => Some(Modifier::Win),
        _ => None,
    }
}

/// What the field shows or does after a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Still listening; these modifiers are held.
    Holding(Mods),
    /// A key completed the shortcut.
    Done(Shortcut),
    /// Escape: keep the previous shortcut.
    Cancelled,
    /// Tab: keep the previous shortcut and let the focus move on.
    Left,
    /// A key a shortcut cannot use; still listening.
    Ignored,
}

/// The field while it listens for a shortcut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capture {
    held: Mods,
}

impl Capture {
    pub fn key_down(&mut self, vk: u16) -> Step {
        if let Some(modifier) = modifier(vk) {
            self.set(modifier, true);
            return Step::Holding(self.held);
        }
        match vk {
            VK_ESCAPE => Step::Cancelled,
            VK_TAB => Step::Left,
            _ => match Key::from_vk(vk) {
                Some(key) => Step::Done(Shortcut {
                    mods: self.held,
                    key,
                }),
                None => Step::Ignored,
            },
        }
    }

    pub fn key_up(&mut self, vk: u16) -> Step {
        match modifier(vk) {
            Some(modifier) => {
                self.set(modifier, false);
                Step::Holding(self.held)
            }
            None => Step::Ignored,
        }
    }

    fn set(&mut self, modifier: Modifier, held: bool) {
        let slot = match modifier {
            Modifier::Ctrl => &mut self.held.ctrl,
            Modifier::Alt => &mut self.held.alt,
            Modifier::Shift => &mut self.held.shift,
            Modifier::Win => &mut self.held.win,
        };
        *slot = held;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VK_LCONTROL: u16 = 0xA2;
    const VK_MENU: u16 = 0x12;
    const VK_F1: u16 = 0x70;
    const VK_CAPITAL: u16 = 0x14;

    #[test]
    fn modifiers_show_as_they_are_held_then_a_key_completes() {
        let mut capture = Capture::default();

        let steps = [
            capture.key_down(VK_LCONTROL),
            capture.key_down(VK_MENU),
            capture.key_down(VK_F1),
        ];

        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        let ctrl_alt = Mods { alt: true, ..ctrl };
        assert_eq!(steps[0], Step::Holding(ctrl));
        assert_eq!(steps[1], Step::Holding(ctrl_alt));
        let Step::Done(shortcut) = steps[2] else {
            panic!("not done: {:?}", steps[2]);
        };
        assert_eq!(shortcut.to_string(), "Ctrl+Alt+F1");
    }

    #[test]
    fn released_modifiers_no_longer_count() {
        let mut capture = Capture::default();
        capture.key_down(VK_LCONTROL);
        capture.key_up(VK_LCONTROL);

        let step = capture.key_down(VK_F1);

        let Step::Done(shortcut) = step else {
            panic!("not done: {step:?}");
        };
        assert_eq!(shortcut.to_string(), "F1");
    }

    #[test]
    fn escape_cancels_tab_leaves_and_other_keys_are_ignored() {
        let mut capture = Capture::default();
        capture.key_down(VK_LCONTROL);

        assert_eq!(
            [
                capture.key_down(VK_CAPITAL),
                capture.key_down(VK_ESCAPE),
                capture.key_down(VK_TAB)
            ],
            [Step::Ignored, Step::Cancelled, Step::Left]
        );
    }
}
