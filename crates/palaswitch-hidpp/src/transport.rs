use std::fmt;
use std::time::Duration;

use crate::frame::LONG_REPORT_LEN;

/// Moves HID++ reports to and from one device interface.
///
/// `read` must return input reports from every HID++ collection of the
/// interface (short and long), in arrival order.
pub trait Transport {
    fn write(&mut self, report: &[u8; LONG_REPORT_LEN]) -> Result<(), TransportError>;

    /// Next input report, or `None` once `timeout` has elapsed without one.
    /// A zero timeout only returns what has already arrived.
    fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, TransportError>;
}

/// The transport failed, usually because the device was unplugged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError(pub String);

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HID transport failed: {}", self.0)
    }
}

impl std::error::Error for TransportError {}
