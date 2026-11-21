use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::{Duration, Instant};

use crate::frame::{FrameError, LongFrame};
use crate::report::{DeviceError, InputReport, Reply, classify};
use crate::transport::{Transport, TransportError};

const ROOT_FEATURE_INDEX: u8 = 0x00;
const ROOT_GET_FEATURE: u8 = 0;
const ROOT_GET_PROTOCOL_VERSION: u8 = 1;
const PING_BYTE: u8 = 0xAA;

/// How long and how often a request is tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Time allowed for one answer.
    pub timeout: Duration,
    /// Number of tries when the device answers with a retryable error.
    pub attempts: u8,
    /// Pause before trying again.
    pub retry_delay: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(4),
            attempts: 3,
            retry_delay: Duration::from_millis(50),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Frame(FrameError),
    Transport(TransportError),
    /// No answer within the configured timeout.
    Timeout,
    Device(DeviceError),
    /// The device does not have this HID++ 2.0 feature.
    FeatureNotSupported(u16),
    /// A response did not have the expected shape.
    Malformed(&'static str),
    /// The device accepted the profile change but reports another profile.
    ProfileNotApplied {
        requested: u16,
        active: u16,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(error) => write!(f, "{error}"),
            Self::Transport(error) => write!(f, "{error}"),
            Self::Timeout => write!(f, "the device did not answer in time"),
            Self::Device(error) => write!(f, "{error}"),
            Self::FeatureNotSupported(feature) => {
                write!(f, "the device has no HID++ feature {feature:#06X}")
            }
            Self::Malformed(what) => write!(f, "malformed response: {what}"),
            Self::ProfileNotApplied { requested, active } => write!(
                f,
                "asked for profile {requested:#06X} but profile {active:#06X} is active"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<FrameError> for Error {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

impl From<TransportError> for Error {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

/// Whether a request that failed with `error` on try number `attempt`
/// (counted from 1) is sent again, given `attempts` tries in all.
pub fn should_retry(error: DeviceError, attempt: u8, attempts: u8) -> bool {
    error.is_retryable() && attempt < attempts
}

/// Requests to one HID++ 2.0 device (one device index) over a transport.
#[derive(Debug)]
pub struct Session<T> {
    transport: T,
    device_index: u8,
    config: Config,
    features: HashMap<u16, u8>,
    notifications: VecDeque<LongFrame>,
}

impl<T: Transport> Session<T> {
    pub fn new(transport: T, device_index: u8, config: Config) -> Self {
        Self {
            transport,
            device_index,
            config,
            features: HashMap::new(),
            notifications: VecDeque::new(),
        }
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    /// Sends `function` of the feature at `feature_index` and waits for its
    /// response. Reports that do not answer this request are skipped;
    /// notifications among them are kept for [`Self::take_notifications`].
    pub fn request(
        &mut self,
        feature_index: u8,
        function: u8,
        params: &[u8],
    ) -> Result<LongFrame, Error> {
        let request = LongFrame::request(self.device_index, feature_index, function, params)?;
        let mut attempt = 1;
        loop {
            self.drain()?;
            self.transport.write(&request.to_bytes())?;
            match self.await_reply(&request)? {
                Ok(response) => return Ok(response),
                Err(error) if should_retry(error, attempt, self.config.attempts) => {
                    tracing::debug!(%error, attempt, "retrying HID++ request");
                    attempt += 1;
                    std::thread::sleep(self.config.retry_delay);
                }
                Err(error) => return Err(Error::Device(error)),
            }
        }
    }

    /// Index of `feature_id` on this device, cached after the first lookup.
    pub fn feature_index(&mut self, feature_id: u16) -> Result<u8, Error> {
        if let Some(&index) = self.features.get(&feature_id) {
            return Ok(index);
        }
        let response = self.request(
            ROOT_FEATURE_INDEX,
            ROOT_GET_FEATURE,
            &feature_id.to_be_bytes(),
        )?;
        match response.params()[0] {
            0 => Err(Error::FeatureNotSupported(feature_id)),
            index => {
                self.features.insert(feature_id, index);
                Ok(index)
            }
        }
    }

    /// Asks for the HID++ protocol version, which also wakes a sleeping
    /// wireless device. Returns (major, minor).
    pub fn ping(&mut self) -> Result<(u8, u8), Error> {
        let response = self.request(
            ROOT_FEATURE_INDEX,
            ROOT_GET_PROTOCOL_VERSION,
            &[0, 0, PING_BYTE],
        )?;
        let params = response.params();
        if params[2] != PING_BYTE {
            return Err(Error::Malformed("ping byte not echoed"));
        }
        Ok((params[0], params[1]))
    }

    /// Notifications received from this device since the last call, oldest first.
    pub fn take_notifications(&mut self) -> Vec<LongFrame> {
        self.notifications.drain(..).collect()
    }

    /// Reads what has already arrived without sending anything, then returns
    /// the notifications received since the last call.
    pub fn poll_notifications(&mut self) -> Result<Vec<LongFrame>, Error> {
        self.drain()?;
        Ok(self.take_notifications())
    }

    /// Reads what has already arrived, so that a late answer to an earlier
    /// request cannot be taken for the next one.
    fn drain(&mut self) -> Result<(), Error> {
        while let Some(bytes) = self.transport.read(Duration::ZERO)? {
            self.keep_if_notification(&InputReport::parse(&bytes));
        }
        Ok(())
    }

    fn await_reply(
        &mut self,
        request: &LongFrame,
    ) -> Result<Result<LongFrame, DeviceError>, Error> {
        let deadline = Instant::now() + self.config.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let Some(bytes) = self.transport.read(remaining)? else {
                return Err(Error::Timeout);
            };
            let report = InputReport::parse(&bytes);
            match classify(request, &report) {
                Reply::Response(frame) => return Ok(Ok(frame)),
                Reply::Failed(error) => return Ok(Err(error)),
                Reply::Unrelated => {
                    tracing::trace!(?bytes, "skipping unrelated HID++ report");
                    self.keep_if_notification(&report);
                }
            }
        }
    }

    fn keep_if_notification(&mut self, report: &InputReport) {
        if let InputReport::Long(frame) = report
            && frame.software_id() == 0
            && frame.device_index() == self.device_index
        {
            self.notifications.push_back(*frame);
        }
    }
}
