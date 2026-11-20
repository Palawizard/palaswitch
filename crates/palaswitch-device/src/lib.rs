//! Finds Logitech HID++ devices with hidapi and talks to them through
//! `palaswitch-hidpp`.

mod interface;
mod mouse;
mod transport;

pub use hidapi::HidApi;
pub use interface::{Collection, Interface, group_hidpp_collections};
pub use mouse::{Connection, Mouse, Skipped, find_mice, hidpp_interfaces};
pub use transport::HidTransport;
