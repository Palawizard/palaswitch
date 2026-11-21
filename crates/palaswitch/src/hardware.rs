//! Access to the HID++ interfaces of the machine, behind a trait so that
//! the mouse thread can be tested on captures.

use palaswitch_device::{HidApi, HidTransport, Interface, ReportSignal, hidpp_interfaces};
use palaswitch_hidpp::{Transport, TransportError};

/// Lists and opens HID++ interfaces.
pub trait Hardware {
    type Transport: Transport;

    fn interfaces(&mut self) -> Result<Vec<Interface>, String>;

    /// Opens `interface`; `signal` runs on every report it receives.
    fn open(
        &mut self,
        interface: &Interface,
        signal: ReportSignal,
    ) -> Result<Self::Transport, TransportError>;
}

#[derive(Default)]
pub struct HidHardware {
    api: Option<HidApi>,
}

impl Hardware for HidHardware {
    type Transport = HidTransport;

    fn interfaces(&mut self) -> Result<Vec<Interface>, String> {
        let api = match self.api.as_mut() {
            // The list is fixed when HidApi is created: refresh it each time.
            Some(api) => {
                api.refresh_devices().map_err(|error| error.to_string())?;
                api
            }
            None => self
                .api
                .insert(HidApi::new().map_err(|error| error.to_string())?),
        };
        Ok(hidpp_interfaces(api))
    }

    fn open(
        &mut self,
        interface: &Interface,
        signal: ReportSignal,
    ) -> Result<HidTransport, TransportError> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| TransportError("hidapi is not started".to_owned()))?;
        HidTransport::open_with_signal(api, interface, signal)
    }
}
