//! HID++ 2.0 protocol for Logitech devices, independent of the HID transport.
//!
//! This crate builds and reads frames and runs requests over any
//! [`Transport`]. Opening devices and moving bytes is the job of the caller.

pub mod features;
mod frame;
mod report;
mod session;
mod transport;

pub use frame::{FrameError, LONG_REPORT_ID, LONG_REPORT_LEN, LongFrame, SOFTWARE_ID};
pub use report::{DeviceError, InputReport, Reply, SHORT_REPORT_ID, SHORT_REPORT_LEN, classify};
pub use session::{Config, Error, Session};
pub use transport::{Transport, TransportError};
