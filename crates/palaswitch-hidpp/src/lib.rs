//! HID++ 2.0 protocol for Logitech devices, independent of the HID transport.
//!
//! This crate only builds and reads frames. Opening devices and moving bytes
//! is the job of the caller.

mod frame;

pub use frame::{FrameError, LONG_REPORT_ID, LONG_REPORT_LEN, LongFrame, SOFTWARE_ID};
