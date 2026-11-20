//! HID++ 2.0 features used by palaswitch: device name (0x0005) and onboard
//! profiles (0x8100).

use crate::session::{Error, Session};
use crate::transport::Transport;

pub const DEVICE_NAME: u16 = 0x0005;
pub const ONBOARD_PROFILES: u16 = 0x8100;

const NAME_GET_COUNT: u8 = 0;
const NAME_GET_NAME: u8 = 1;
const MAX_NAME_LEN: usize = 64;

const PROFILES_GET_INFO: u8 = 0;
const PROFILES_SET_MODE: u8 = 1;
const PROFILES_GET_MODE: u8 = 2;
const PROFILES_SET_CURRENT: u8 = 3;
const PROFILES_GET_CURRENT: u8 = 4;
const PROFILES_MEMORY_READ: u8 = 5;

/// Function of the change notification sent when the active profile changes
/// on the device itself.
pub const PROFILE_CHANGED_EVENT: u8 = 0;

const DIRECTORY_SECTOR: u16 = 0x0000;
/// Directory of the factory profiles, read when the user directory is empty.
const ROM_DIRECTORY_SECTOR: u16 = 0x0100;
const ROM_SECTOR_BASE: u16 = 0x0100;
const DIRECTORY_END: u16 = 0xFFFF;
const MEMORY_CHUNK: usize = 16;
const DIRECTORY_ENTRY_LEN: usize = 4;

/// Name the device reports, such as "G502 X PLUS".
pub fn device_name<T: Transport>(session: &mut Session<T>) -> Result<String, Error> {
    let index = session.feature_index(DEVICE_NAME)?;
    let len = usize::from(session.request(index, NAME_GET_COUNT, &[])?.params()[0]);
    if len > MAX_NAME_LEN {
        return Err(Error::Malformed("device name longer than 64 bytes"));
    }
    let mut name = Vec::with_capacity(len);
    while name.len() < len {
        let offset = u8::try_from(name.len()).map_err(|_| Error::Malformed("name offset"))?;
        let chunk = session.request(index, NAME_GET_NAME, &[offset])?;
        let wanted = (len - name.len()).min(chunk.params().len());
        name.extend_from_slice(&chunk.params()[..wanted]);
    }
    Ok(String::from_utf8_lossy(&name)
        .trim_end_matches('\0')
        .to_owned())
}

/// Who decides the active profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The device uses the profiles stored in its memory.
    Onboard,
    /// Software on the computer drives the device.
    Host,
    Other(u8),
}

impl Mode {
    fn from_byte(byte: u8) -> Self {
        match byte {
            1 => Self::Onboard,
            2 => Self::Host,
            other => Self::Other(other),
        }
    }
}

/// Answer to getInfo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfilesInfo {
    pub profile_count: u8,
    pub rom_profile_count: u8,
}

/// One profile slot of the onboard memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// Memory sector of the profile, the value setCurrentProfile takes.
    pub sector: u16,
    pub enabled: bool,
}

impl Profile {
    /// Number shown to the user: 1 for the first slot, of user or factory
    /// profiles.
    pub fn number(&self) -> u16 {
        self.sector & 0x00FF
    }

    /// Whether this is a read-only factory profile.
    pub fn is_rom(&self) -> bool {
        self.sector & 0xFF00 == ROM_SECTOR_BASE
    }
}

/// Reads entries of the profile directory (4 bytes each: sector, big endian,
/// then an enabled flag). Returns `None` once the end marker is reached.
pub fn parse_directory_chunk(chunk: &[u8], profiles: &mut Vec<Profile>) -> Option<()> {
    for entry in chunk.as_chunks::<DIRECTORY_ENTRY_LEN>().0 {
        let sector = u16::from_be_bytes([entry[0], entry[1]]);
        if sector == DIRECTORY_END {
            return None;
        }
        profiles.push(Profile {
            sector,
            enabled: entry[2] != 0,
        });
    }
    Some(())
}

/// Feature 0x8100 of one device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnboardProfiles {
    feature_index: u8,
}

impl OnboardProfiles {
    pub fn find<T: Transport>(session: &mut Session<T>) -> Result<Self, Error> {
        Ok(Self {
            feature_index: session.feature_index(ONBOARD_PROFILES)?,
        })
    }

    pub fn feature_index(&self) -> u8 {
        self.feature_index
    }

    pub fn info<T: Transport>(&self, session: &mut Session<T>) -> Result<ProfilesInfo, Error> {
        let response = session.request(self.feature_index, PROFILES_GET_INFO, &[])?;
        let params = response.params();
        Ok(ProfilesInfo {
            profile_count: params[3],
            rom_profile_count: params[4],
        })
    }

    pub fn mode<T: Transport>(&self, session: &mut Session<T>) -> Result<Mode, Error> {
        let response = session.request(self.feature_index, PROFILES_GET_MODE, &[])?;
        Ok(Mode::from_byte(response.params()[0]))
    }

    pub fn set_onboard_mode<T: Transport>(&self, session: &mut Session<T>) -> Result<(), Error> {
        session.request(self.feature_index, PROFILES_SET_MODE, &[1])?;
        Ok(())
    }

    /// Sector of the active profile.
    pub fn current<T: Transport>(&self, session: &mut Session<T>) -> Result<u16, Error> {
        let response = session.request(self.feature_index, PROFILES_GET_CURRENT, &[])?;
        let params = response.params();
        Ok(u16::from_be_bytes([params[0], params[1]]))
    }

    /// Profile slots listed in the onboard memory directory, at most
    /// `profile_count` of them. A device whose user directory was never
    /// written lists its factory profiles instead.
    pub fn profiles<T: Transport>(
        &self,
        session: &mut Session<T>,
        info: ProfilesInfo,
    ) -> Result<Vec<Profile>, Error> {
        let max = usize::from(info.profile_count);
        let profiles = self.read_directory(session, DIRECTORY_SECTOR, max)?;
        if profiles.is_empty() {
            return self.read_directory(session, ROM_DIRECTORY_SECTOR, max);
        }
        Ok(profiles)
    }

    fn read_directory<T: Transport>(
        &self,
        session: &mut Session<T>,
        sector: u16,
        max: usize,
    ) -> Result<Vec<Profile>, Error> {
        let mut profiles = Vec::new();
        let mut offset: u16 = 0;
        while profiles.len() < max {
            let chunk = self.memory_read(session, sector, offset)?;
            if parse_directory_chunk(&chunk, &mut profiles).is_none() {
                break;
            }
            offset += MEMORY_CHUNK as u16;
        }
        profiles.truncate(max);
        Ok(profiles)
    }

    /// Makes `sector` the active profile, then reads the active profile back
    /// to make sure the device applied it.
    pub fn switch_to<T: Transport>(
        &self,
        session: &mut Session<T>,
        sector: u16,
    ) -> Result<(), Error> {
        self.set_onboard_mode(session)?;
        session.request(
            self.feature_index,
            PROFILES_SET_CURRENT,
            &sector.to_be_bytes(),
        )?;
        let active = self.current(session)?;
        if active != sector {
            return Err(Error::ProfileNotApplied {
                requested: sector,
                active,
            });
        }
        Ok(())
    }

    fn memory_read<T: Transport>(
        &self,
        session: &mut Session<T>,
        sector: u16,
        offset: u16,
    ) -> Result<[u8; MEMORY_CHUNK], Error> {
        let [sector_hi, sector_lo] = sector.to_be_bytes();
        let [offset_hi, offset_lo] = offset.to_be_bytes();
        let response = session.request(
            self.feature_index,
            PROFILES_MEMORY_READ,
            &[sector_hi, sector_lo, offset_hi, offset_lo],
        )?;
        Ok(*response.params())
    }
}
