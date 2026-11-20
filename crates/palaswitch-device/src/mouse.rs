use hidapi::HidApi;
use palaswitch_hidpp::features::{self, Mode, OnboardProfiles, Profile, ProfilesInfo};
use palaswitch_hidpp::{Config, Error, LongFrame, Session};

use crate::interface::{Collection, Interface, LOGITECH_VENDOR_ID, group_hidpp_collections};
use crate::transport::HidTransport;

/// Device index of a device plugged in directly (cable).
const DIRECT_INDEX: u8 = 0xFF;
/// Device indices a receiver can pair.
const RECEIVER_INDICES: std::ops::RangeInclusive<u8> = 1..=6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    /// USB cable, or any device answering at index 0xFF.
    Wired,
    /// Through a wireless receiver, at this device index.
    Receiver(u8),
}

/// A Logitech device with onboard profiles, ready for requests.
#[derive(Debug)]
pub struct Mouse {
    pub name: String,
    pub product_id: u16,
    pub connection: Connection,
    session: Session<HidTransport>,
    onboard: OnboardProfiles,
    info: ProfilesInfo,
}

impl Mouse {
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
        match open_interface(api, &interface) {
            Ok(found) => mice.extend(found),
            Err(reason) => skipped.push(Skipped {
                product_id: interface.product_id,
                reason,
            }),
        }
    }
    (mice, skipped)
}

fn open_interface(api: &HidApi, interface: &Interface) -> Result<Vec<Mouse>, Error> {
    let mut mice = Vec::new();
    let mut last_error = Error::Malformed("no device index answered");
    let mut spare: Option<HidTransport> = None;
    for index in std::iter::once(DIRECT_INDEX).chain(RECEIVER_INDICES) {
        let transport = match spare.take() {
            Some(transport) => transport,
            None => HidTransport::open(api, interface)?,
        };
        // A sleeping wireless mouse can take over half a second to answer its
        // first request; empty indices and receivers answer at once.
        let mut session = Session::new(transport, index, Config::default());
        match probe(&mut session) {
            Ok((onboard, info, name)) => {
                tracing::info!(
                    name,
                    index,
                    product_id = interface.product_id,
                    "found device"
                );
                mice.push(Mouse {
                    name,
                    product_id: interface.product_id,
                    connection: if index == DIRECT_INDEX {
                        Connection::Wired
                    } else {
                        Connection::Receiver(index)
                    },
                    session,
                    onboard,
                    info,
                });
                if index == DIRECT_INDEX {
                    break;
                }
            }
            Err(error) => {
                tracing::debug!(%error, index, product_id = interface.product_id, "no device");
                last_error = error;
                spare = Some(session.into_transport());
            }
        }
    }
    if mice.is_empty() {
        Err(last_error)
    } else {
        Ok(mice)
    }
}

fn probe(
    session: &mut Session<HidTransport>,
) -> Result<(OnboardProfiles, ProfilesInfo, String), Error> {
    session.ping()?;
    let onboard = OnboardProfiles::find(session)?;
    let info = onboard.info(session)?;
    let name = features::device_name(session).unwrap_or_default();
    Ok((onboard, info, name))
}
