//! Decisions of the command line tool that do not need a mouse.

use palaswitch_device::Skipped;
use palaswitch_hidpp::Error;
use palaswitch_hidpp::features::Profile;

/// Most switches one `stress` run may ask for.
pub const MAX_STRESS_COUNT: usize = 10_000;
/// Longest `sniff` run, in seconds.
pub const MAX_SNIFF_SECS: u64 = 3_600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Missing,
    Disabled,
}

/// The profile `set <number>` may switch to.
pub fn choose_profile(profiles: &[Profile], number: u16) -> Result<Profile, Choice> {
    let profile = profiles
        .iter()
        .find(|profile| profile.number() == number)
        .ok_or(Choice::Missing)?;
    if profile.enabled {
        Ok(*profile)
    } else {
        Err(Choice::Disabled)
    }
}

/// Enabled profiles in the order `stress` visits them: starting with the one
/// after the active profile, so that the first switch changes something.
pub fn stress_cycle(profiles: &[Profile], active: u16) -> Vec<Profile> {
    let enabled: Vec<Profile> = profiles
        .iter()
        .copied()
        .filter(|profile| profile.enabled)
        .collect();
    let start = enabled
        .iter()
        .position(|profile| profile.sector == active)
        .map_or(0, |position| position + 1);
    let mut cycle = enabled;
    if !cycle.is_empty() {
        let start = start % cycle.len();
        cycle.rotate_left(start);
    }
    cycle
}

pub fn describe_profile(sector: u16) -> String {
    let profile = Profile {
        sector,
        enabled: true,
    };
    if profile.is_rom() {
        format!("factory profile {}", profile.number())
    } else {
        format!("profile {}", profile.number())
    }
}

/// Whether the reason no mouse was found is most likely a mouse that sleeps
/// or is switched off, among the interfaces `product_id` selects.
pub fn mouse_may_be_asleep(skipped: &[Skipped], product_id: Option<u16>) -> bool {
    skipped
        .iter()
        .filter(|skip| product_id.is_none_or(|pid| skip.product_id == pid))
        .any(|skip| match &skip.reason {
            Error::Device(error) => error.is_unreachable(),
            Error::Timeout => true,
            _ => false,
        })
}

pub fn parse_bounded<T>(text: &str, max: T) -> Result<T, String>
where
    T: std::str::FromStr + PartialOrd + Copy + std::fmt::Display + From<u8>,
{
    let value: T = text.parse().map_err(|_| format!("not a number: {text}"))?;
    if value < T::from(1) || value > max {
        return Err(format!("{text} is out of range (1 to {max})"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use palaswitch_hidpp::DeviceError;
    use palaswitch_hidpp::features::parse_directory_chunk;

    use super::*;

    const PROFILES: &str =
        include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-lightspeed-profiles.txt");

    /// Profile slots of the G502 X PLUS, from the two directory reads of the
    /// capture: 1 to 3 enabled, 4 and 5 disabled.
    fn captured_profiles() -> Vec<Profile> {
        let directory_reads: Vec<Vec<u8>> = PROFILES
            .lines()
            .filter(|line| line.starts_with("RX 11 01 0B 5E 00"))
            .map(|line| {
                line.split_whitespace()
                    .skip(1)
                    .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
                    .collect()
            })
            .collect();
        let mut profiles = Vec::new();
        for read in &directory_reads[..2] {
            if parse_directory_chunk(&read[4..], &mut profiles).is_none() {
                break;
            }
        }
        profiles
    }

    #[test]
    fn captured_directory_has_five_slots() {
        assert_eq!(captured_profiles().len(), 5);
    }

    #[test]
    fn set_accepts_an_enabled_profile() {
        assert_eq!(
            choose_profile(&captured_profiles(), 2).map(|profile| profile.sector),
            Ok(0x0002)
        );
    }

    #[test]
    fn set_refuses_a_disabled_profile() {
        assert_eq!(
            choose_profile(&captured_profiles(), 4),
            Err(Choice::Disabled)
        );
    }

    #[test]
    fn set_refuses_a_missing_profile() {
        assert_eq!(
            choose_profile(&captured_profiles(), 9),
            Err(Choice::Missing)
        );
    }

    #[test]
    fn stress_starts_after_the_active_profile() {
        let order: Vec<u16> = stress_cycle(&captured_profiles(), 0x0001)
            .iter()
            .map(Profile::number)
            .collect();

        assert_eq!(order, vec![2, 3, 1]);
    }

    #[test]
    fn stress_wraps_around_from_the_last_enabled_profile() {
        let order: Vec<u16> = stress_cycle(&captured_profiles(), 0x0003)
            .iter()
            .map(Profile::number)
            .collect();

        assert_eq!(order, vec![1, 2, 3]);
    }

    #[test]
    fn factory_profiles_are_named_as_such() {
        assert_eq!(
            [describe_profile(0x0001), describe_profile(0x0101)],
            ["profile 1".to_owned(), "factory profile 1".to_owned()]
        );
    }

    #[test]
    fn unknown_device_behind_the_selected_receiver_means_asleep() {
        let skipped = [Skipped {
            product_id: 0xC547,
            reason: Error::Device(DeviceError::Hidpp10(0x08)),
        }];

        assert_eq!(
            [
                mouse_may_be_asleep(&skipped, None),
                mouse_may_be_asleep(&skipped, Some(0xC547)),
                mouse_may_be_asleep(&skipped, Some(0xC095)),
            ],
            [true, true, false]
        );
    }

    #[test]
    fn counts_outside_the_bounds_are_refused() {
        assert_eq!(
            [
                parse_bounded("0", MAX_STRESS_COUNT).is_err(),
                parse_bounded("10001", MAX_STRESS_COUNT).is_err(),
                parse_bounded("200", MAX_STRESS_COUNT).is_ok(),
            ],
            [true, true, true]
        );
    }
}
