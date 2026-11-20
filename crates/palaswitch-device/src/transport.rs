use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};
use palaswitch_hidpp::{LONG_REPORT_LEN, Transport, TransportError};

use crate::interface::Interface;

const READ_BUFFER_LEN: usize = 64;

type Incoming = Result<Vec<u8>, String>;

/// HID++ transport over hidapi.
///
/// Writes go through their own handle to the long collection. One thread per
/// collection blocks on its own read handle and forwards every report, so the
/// process does no work while the device is quiet. A thread ends when the
/// device goes away or once this transport is dropped and a report arrives.
pub struct HidTransport {
    writer: HidDevice,
    incoming: Receiver<Incoming>,
}

impl std::fmt::Debug for HidTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HidTransport").finish_non_exhaustive()
    }
}

impl HidTransport {
    pub fn open(api: &HidApi, interface: &Interface) -> Result<Self, TransportError> {
        let open = |path: &std::ffi::CStr| {
            api.open_path(path)
                .map_err(|error| TransportError(error.to_string()))
        };
        let writer = open(&interface.long_path)?;
        let (sender, incoming) = mpsc::channel();
        let mut readers = vec![("long", open(&interface.long_path)?)];
        if let Some(short_path) = &interface.short_path {
            readers.push(("short", open(short_path)?));
        }
        for (name, reader) in readers {
            let sender = sender.clone();
            thread::Builder::new()
                .name(format!("hidpp-{name}-reader"))
                .spawn(move || forward_reports(&reader, &sender))
                .map_err(|error| TransportError(error.to_string()))?;
        }
        Ok(Self { writer, incoming })
    }
}

fn forward_reports(reader: &HidDevice, sender: &mpsc::Sender<Incoming>) {
    let mut buffer = [0u8; READ_BUFFER_LEN];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {}
            Ok(len) => {
                if sender.send(Ok(buffer[..len].to_vec())).is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(error.to_string()));
                return;
            }
        }
    }
}

impl Transport for HidTransport {
    fn write(&mut self, report: &[u8; LONG_REPORT_LEN]) -> Result<(), TransportError> {
        self.writer
            .write(report)
            .map(|_| ())
            .map_err(|error| TransportError(error.to_string()))
    }

    fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, TransportError> {
        let incoming = if timeout.is_zero() {
            match self.incoming.try_recv() {
                Ok(incoming) => incoming,
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => return Err(reader_stopped()),
            }
        } else {
            match self.incoming.recv_timeout(timeout) {
                Ok(incoming) => incoming,
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => return Err(reader_stopped()),
            }
        };
        incoming.map(Some).map_err(TransportError)
    }
}

fn reader_stopped() -> TransportError {
    TransportError("the HID read threads stopped".to_owned())
}
