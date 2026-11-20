use std::fmt;

use crate::frame::{LONG_REPORT_ID, LONG_REPORT_LEN, LongFrame};

/// Report ID of a short HID++ report.
pub const SHORT_REPORT_ID: u8 = 0x10;

/// Length in bytes of a short HID++ report, report ID included.
pub const SHORT_REPORT_LEN: usize = 7;

const HIDPP10_ERROR_SUB_ID: u8 = 0x8F;
const HIDPP20_ERROR_FEATURE_INDEX: u8 = 0xFF;

/// An error the device or the receiver sent back instead of a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    /// HID++ 2.0 error, sent by the device itself.
    Hidpp20(u8),
    /// HID++ 1.0 error, sent by the receiver, for instance when the device is
    /// asleep, out of range or not paired at that index.
    Hidpp10(u8),
}

impl DeviceError {
    /// Whether sending the same request again may succeed.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Hidpp20(0x08) | Self::Hidpp10(0x04 | 0x07 | 0x09)
        )
    }

    /// Whether the receiver says no device answers at this index right now.
    pub fn is_unreachable(self) -> bool {
        matches!(self, Self::Hidpp10(0x04 | 0x08 | 0x09))
    }

    fn name(self) -> &'static str {
        match self {
            Self::Hidpp20(code) => match code {
                0x01 => "unknown error",
                0x02 => "invalid argument",
                0x03 => "out of range",
                0x04 => "hardware error",
                0x05 => "internal error",
                0x06 => "invalid feature index",
                0x07 => "invalid function",
                0x08 => "busy",
                0x09 => "unsupported",
                _ => "unlisted error",
            },
            Self::Hidpp10(code) => match code {
                0x01 => "invalid sub ID",
                0x02 => "invalid address",
                0x03 => "invalid value",
                0x04 => "connection failed",
                0x05 => "too many devices",
                0x06 => "already exists",
                0x07 => "busy",
                0x08 => "unknown device",
                0x09 => "resource error",
                0x0A => "request unavailable",
                0x0B => "invalid parameter value",
                0x0C => "wrong PIN code",
                _ => "unlisted error",
            },
        }
    }
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (version, code) = match self {
            Self::Hidpp20(code) => ("HID++ 2.0", code),
            Self::Hidpp10(code) => ("HID++ 1.0", code),
        };
        write!(f, "{version} error {code:#04X} ({})", self.name())
    }
}

impl std::error::Error for DeviceError {}

/// Any report read from a HID++ collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputReport {
    /// Response or notification.
    Long(LongFrame),
    Error {
        device_index: u8,
        feature_index: u8,
        /// Function and software ID byte of the failed request.
        function_byte: u8,
        error: DeviceError,
    },
    /// Anything else: other short reports, unknown report IDs, wrong lengths.
    Other,
}

impl InputReport {
    pub fn parse(bytes: &[u8]) -> Self {
        match (bytes.first(), bytes.len()) {
            (Some(&LONG_REPORT_ID), LONG_REPORT_LEN) if bytes[2] == HIDPP20_ERROR_FEATURE_INDEX => {
                Self::Error {
                    device_index: bytes[1],
                    feature_index: bytes[3],
                    function_byte: bytes[4],
                    error: DeviceError::Hidpp20(bytes[5]),
                }
            }
            (Some(&LONG_REPORT_ID), LONG_REPORT_LEN) => match LongFrame::parse(bytes) {
                Ok(frame) => Self::Long(frame),
                Err(_) => Self::Other,
            },
            (Some(&SHORT_REPORT_ID), SHORT_REPORT_LEN) if bytes[2] == HIDPP10_ERROR_SUB_ID => {
                Self::Error {
                    device_index: bytes[1],
                    feature_index: bytes[3],
                    function_byte: bytes[4],
                    error: DeviceError::Hidpp10(bytes[5]),
                }
            }
            _ => Self::Other,
        }
    }
}

/// What an input report means for a request waiting for its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Response(LongFrame),
    Failed(DeviceError),
    /// Not about this request: a notification, or traffic of another program.
    Unrelated,
}

/// Matches `report` against `request` on device index, feature index,
/// function and software ID.
pub fn classify(request: &LongFrame, report: &InputReport) -> Reply {
    let function_byte = (request.function() << 4) | request.software_id();
    match *report {
        InputReport::Long(frame)
            if frame.device_index() == request.device_index()
                && frame.feature_index() == request.feature_index()
                && frame.function() == request.function()
                && frame.software_id() == request.software_id() =>
        {
            Reply::Response(frame)
        }
        InputReport::Error {
            device_index,
            feature_index,
            function_byte: failed,
            error,
        } if device_index == request.device_index()
            && feature_index == request.feature_index()
            && failed == function_byte =>
        {
            Reply::Failed(error)
        }
        _ => Reply::Unrelated,
    }
}
