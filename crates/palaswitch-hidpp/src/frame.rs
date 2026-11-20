use std::fmt;

/// Report ID of a long HID++ report.
pub const LONG_REPORT_ID: u8 = 0x11;

/// Length in bytes of a long HID++ report, report ID included.
pub const LONG_REPORT_LEN: usize = 20;

/// Software ID put in every request we send. Other values are already used by
/// the device firmware or by other Logitech tools, so responses carrying ours
/// can only be answers to our own requests.
pub const SOFTWARE_ID: u8 = 0x0E;

const PARAMS_LEN: usize = LONG_REPORT_LEN - 4;

/// A long HID++ 2.0 report: request, response or notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LongFrame {
    pub device_index: u8,
    pub feature_index: u8,
    pub function: u8,
    pub software_id: u8,
    pub params: [u8; PARAMS_LEN],
}

/// Why bytes could not be turned into a frame, or a frame could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    WrongLength(usize),
    WrongReportId(u8),
    FunctionOutOfRange(u8),
    TooManyParams(usize),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength(len) => {
                write!(f, "expected {LONG_REPORT_LEN} bytes, got {len}")
            }
            Self::WrongReportId(id) => {
                write!(f, "expected report ID {LONG_REPORT_ID:#04X}, got {id:#04X}")
            }
            Self::FunctionOutOfRange(function) => {
                write!(f, "function {function:#04X} does not fit in 4 bits")
            }
            Self::TooManyParams(len) => {
                write!(f, "{len} parameter bytes, at most {PARAMS_LEN} fit")
            }
        }
    }
}

impl std::error::Error for FrameError {}

impl LongFrame {
    /// Builds a request for `function` of the feature at `feature_index`,
    /// tagged with our [`SOFTWARE_ID`].
    pub fn request(
        device_index: u8,
        feature_index: u8,
        function: u8,
        params: &[u8],
    ) -> Result<Self, FrameError> {
        if function > 0x0F {
            return Err(FrameError::FunctionOutOfRange(function));
        }
        if params.len() > PARAMS_LEN {
            return Err(FrameError::TooManyParams(params.len()));
        }
        let mut padded = [0; PARAMS_LEN];
        padded[..params.len()].copy_from_slice(params);
        Ok(Self {
            device_index,
            feature_index,
            function,
            software_id: SOFTWARE_ID,
            params: padded,
        })
    }

    /// Reads a long report as received from the HID transport.
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        if bytes.len() != LONG_REPORT_LEN {
            return Err(FrameError::WrongLength(bytes.len()));
        }
        if bytes[0] != LONG_REPORT_ID {
            return Err(FrameError::WrongReportId(bytes[0]));
        }
        let mut params = [0; PARAMS_LEN];
        params.copy_from_slice(&bytes[4..]);
        Ok(Self {
            device_index: bytes[1],
            feature_index: bytes[2],
            function: bytes[3] >> 4,
            software_id: bytes[3] & 0x0F,
            params,
        })
    }

    /// Bytes to write to the HID transport, report ID first.
    pub fn to_bytes(&self) -> [u8; LONG_REPORT_LEN] {
        let mut bytes = [0; LONG_REPORT_LEN];
        bytes[0] = LONG_REPORT_ID;
        bytes[1] = self.device_index;
        bytes[2] = self.feature_index;
        bytes[3] = (self.function << 4) | (self.software_id & 0x0F);
        bytes[4..].copy_from_slice(&self.params);
        bytes
    }
}
