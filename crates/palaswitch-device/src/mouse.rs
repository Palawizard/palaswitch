use hidapi::HidApi;
use palaswitch_hidpp::features::{self, Mode, OnboardProfiles, Profile, ProfilesInfo};
use palaswitch_hidpp::{Config, DeviceError, Error, LongFrame, Session, Transport};

use crate::interface::{Collection, Interface, LOGITECH_VENDOR_ID, group_hidpp_collections};
use crate::transport::HidTransport;

/// Device index of a device plugged in directly (cable).
const DIRECT_INDEX: u8 = 0xFF;
/// Device indices a receiver can pair.
const RECEIVER_INDICES: std::ops::RangeInclusive<u8> = 1..=6;
/// What a receiver answers for an index with nothing paired, or with the
/// mouse switched off or in deep sleep.
const EMPTY_INDEX: Error = Error::Device(DeviceError::Hidpp10(0x08));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    /// USB cable, or any device answering at index 0xFF.
    Wired,
    /// Through a wireless receiver, at this device index.
    Receiver(u8),
}

/// A Logitech device with onboard profiles, ready for requests.
#[derive(Debug)]
pub struct Mouse<T = HidTransport> {
    pub name: String,
    pub product_id: u16,
    pub connection: Connection,
    session: Session<T>,
    onboard: OnboardProfiles,
    info: ProfilesInfo,
}

impl<T: Transport> Mouse<T> {
    pub fn info(&self) -> ProfilesInfo {
        self.info
    }

    pub fn profiles(&mut self) -> Result<Vec<Profile>, Error> {
        self.onboard.profiles(&mut self.session, self.info)
    }

    pub fn mode(&mut self) -> Result<Mode, Error> {
        self.onboard.mode(&mut self.session)
    }

    /// Sector of the active profile.
    pub fn current_profile(&mut self) -> Result<u16, Error> {
        self.onboard.current(&mut self.session)
    }

    pub fn switch_to(&mut self, sector: u16) -> Result<(), Error> {
        self.onboard.switch_to(&mut self.session, sector)
    }

    /// Notifications received so far, oldest first.
    pub fn take_notifications(&mut self) -> Vec<LongFrame> {
        self.session.take_notifications()
    }

    /// Reads what has arrived without sending anything, and returns the
    /// notifications received so far, oldest first.
    pub fn poll_notifications(&mut self) -> Result<Vec<LongFrame>, Error> {
        self.session.poll_notifications()
    }

    /// Sector of the new active profile when `frame` says the profile
    /// changed on the device itself.
    pub fn changed_profile(&self, frame: &LongFrame) -> Option<u16> {
        self.onboard.changed_profile(frame)
    }

    /// Feature index of onboard profiles, to recognize its notifications.
    pub fn onboard_feature_index(&self) -> u8 {
        self.onboard.feature_index()
    }
}

/// Why no mouse could be opened on an interface.
#[derive(Debug)]
pub struct Skipped {
    pub product_id: u16,
    pub reason: Error,
}

/// Every HID++ interface currently present.
pub fn hidpp_interfaces(api: &HidApi) -> Vec<Interface> {
    let collections: Vec<Collection> = api
        .device_list()
        .filter(|info| info.vendor_id() == LOGITECH_VENDOR_ID)
        .map(|info| Collection {
            product_id: info.product_id(),
            usage_page: info.usage_page(),
            usage: info.usage(),
            path: info.path().to_owned(),
            product: info.product_string().unwrap_or_default().to_owned(),
        })
        .collect();
    group_hidpp_collections(&collections)
}

/// Opens every connected device that has onboard profiles. Interfaces where
/// none answers are reported in the second list.
pub fn find_mice(api: &HidApi) -> (Vec<Mouse>, Vec<Skipped>) {
    let mut mice = Vec::new();
    let mut skipped = Vec::new();
    for interface in hidpp_interfaces(api) {
        let found = HidTransport::open(api, &interface)
            .map_err(Error::Transport)
            .and_then(|transport| {
                Mouse::find(transport, interface.product_id).map_err(|not_found| not_found.reason)
            });
        match found {
            Ok(mouse) => mice.push(mouse),
            Err(reason) => skipped.push(Skipped {
                product_id: interface.product_id,
                reason,
            }),
        }
    }
    (mice, skipped)
}

/// No device with onboard profiles answered behind an interface.
#[derive(Debug)]
pub struct NotFound<T> {
    pub reason: Error,
    /// The transport, to try again later without opening the interface
    /// again. `None` when the transport itself failed.
    pub transport: Option<T>,
}

impl<T: Transport> Mouse<T> {
    /// Looks for a device with onboard profiles behind one interface: index
    /// 0xFF first, which is the device itself when it is plugged in directly.
    /// A receiver answers 0xFF with a HID++ 1.0 error; only then are the
    /// paired indices 1 to 6 tried, and the first device found is kept, so
    /// that each interface has a single transport.
    pub fn find(transport: T, product_id: u16) -> Result<Self, NotFound<T>> {
        let mut session = Session::new(transport, DIRECT_INDEX, Config::default());
        match probe(&mut session) {
            Ok((onboard, info, name)) => {
                tracing::info!(name, product_id, "found device plugged in directly");
                return Ok(Mouse {
                    name,
                    product_id,
                    connection: Connection::Wired,
                    session,
                    onboard,
                    info,
                });
            }
            Err(Error::Device(DeviceError::Hidpp10(code))) => {
                tracing::debug!(code, product_id, "receiver, trying paired devices");
            }
            Err(reason) => return Err(not_found(reason, session)),
        }

        let mut reason: Option<Error> = None;
        for index in RECEIVER_INDICES {
            let mut indexed = Session::new(session.into_transport(), index, Config::default());
            match probe(&mut indexed) {
                Ok((onboard, info, name)) => {
                    tracing::info!(name, index, product_id, "found device behind receiver");
                    return Ok(Mouse {
                        name,
                        product_id,
                        connection: Connection::Receiver(index),
                        session: indexed,
                        onboard,
                        info,
                    });
                }
                Err(error @ Error::Transport(_)) => {
                    return Err(NotFound {
                        reason: error,
                        transport: None,
                    });
                }
                Err(error) => {
                    tracing::debug!(%error, index, product_id, "no device");
                    reason = Some(more_telling(reason, error));
                    session = indexed;
                }
            }
        }
        let reason = reason.unwrap_or(Error::Malformed("no device index answered"));
        Err(not_found(reason, session))
    }
}

fn not_found<T: Transport>(reason: Error, session: Session<T>) -> NotFound<T> {
    let transport = match reason {
        Error::Transport(_) => None,
        _ => Some(session.into_transport()),
    };
    NotFound { reason, transport }
}

/// Keeps the error that says the most about why nothing answered: an empty
/// index (unknown device) only matters when every index is empty.
fn more_telling(kept: Option<Error>, new: Error) -> Error {
    match kept {
        Some(kept) if kept != EMPTY_INDEX => kept,
        _ => new,
    }
}

// A sleeping wireless mouse can take up to two seconds to answer its first
// request, so probing keeps the normal timeout; empty indices and receivers
// answer at once.
fn probe<T: Transport>(
    session: &mut Session<T>,
) -> Result<(OnboardProfiles, ProfilesInfo, String), Error> {
    session.ping()?;
    let onboard = OnboardProfiles::find(session)?;
    let info = onboard.info(session)?;
    let name = features::device_name(session).unwrap_or_else(|error| {
        tracing::debug!(%error, "could not read the device name");
        String::new()
    });
    Ok((onboard, info, name))
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
