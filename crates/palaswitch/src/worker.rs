//! The mouse thread: owns every HID transport and does all HID++ work, so
//! the interface thread never waits on the mouse.
//!
//! It sleeps until it gets a command: from the interface, from a device
//! change, or from a reader thread when a report arrives.

use std::collections::VecDeque;
use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use palaswitch_device::{
    Connection, HidApi, HidTransport, Interface, Mouse, NotFound, hidpp_interfaces,
};
use palaswitch_hidpp::Error;
use palaswitch_hidpp::features::Profile;

use crate::model::{Failure, MouseState, MouseView};

/// Device changes come in bursts, one per collection; the new device is
/// only opened once they stop for this long.
const SETTLE_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Read the profiles and the active profile again.
    Refresh,
    /// Make the profile at this sector active.
    SwitchTo(u16),
    /// A HID device came or went.
    DevicesChanged,
    /// A reader thread forwarded a report.
    Reports,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    State(MouseState),
    SwitchFailed(Failure),
}

/// Handle to the mouse thread. The thread ends with the process.
#[derive(Debug, Clone)]
pub struct Worker {
    commands: Sender<Command>,
}

impl Worker {
    /// Starts the thread; `notify` runs on it for each update.
    pub fn spawn(notify: impl Fn(Update) + Send + 'static) -> std::io::Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let sender = commands.clone();
        thread::Builder::new()
            .name("mouse".to_owned())
            .spawn(move || MouseThread::new(sender, receiver, Box::new(notify)).run())?;
        Ok(Self { commands })
    }

    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            tracing::error!(?command, "the mouse thread is gone");
        }
    }
}

struct MouseThread {
    api: Option<HidApi>,
    links: Vec<Link>,
    commands: Sender<Command>,
    receiver: Receiver<Command>,
    deferred: VecDeque<Command>,
    notify: Box<dyn Fn(Update) + Send>,
    published: Option<MouseState>,
}

/// One open HID++ interface.
struct Link {
    path: CString,
    product_id: u16,
    /// Reports forwarded by its reader threads so far.
    reports: Arc<AtomicU64>,
    /// Value of `reports` once our own requests were answered: anything
    /// above it came from the device or another program.
    seen: u64,
    device: Device,
}

enum Device {
    Mouse(Box<LinkedMouse>),
    /// Nothing with onboard profiles answered, often a receiver whose mouse
    /// sleeps; probed again when the interface sends something.
    Idle {
        transport: HidTransport,
        reason: Error,
    },
}

struct LinkedMouse {
    mouse: Mouse,
    profiles: Vec<Profile>,
    active: Option<u16>,
    unreachable: bool,
}

impl Link {
    fn caught_up(&mut self) {
        self.seen = self.reports.load(Ordering::Acquire);
    }

    fn has_news(&self) -> bool {
        self.reports.load(Ordering::Acquire) != self.seen
    }
}

impl MouseThread {
    fn new(
        commands: Sender<Command>,
        receiver: Receiver<Command>,
        notify: Box<dyn Fn(Update) + Send>,
    ) -> Self {
        Self {
            api: None,
            links: Vec::new(),
            commands,
            receiver,
            deferred: VecDeque::new(),
            notify,
            published: None,
        }
    }

    fn run(mut self) {
        self.rediscover();
        self.publish();
        loop {
            let command = match self.deferred.pop_front() {
                Some(command) => command,
                None => match self.receiver.recv() {
                    Ok(command) => command,
                    Err(_) => return,
                },
            };
            match command {
                Command::DevicesChanged => {
                    self.settle();
                    self.rediscover();
                }
                Command::Refresh => self.refresh(),
                Command::SwitchTo(sector) => self.switch_to(sector),
                Command::Reports => self.read_reports(),
            }
            self.publish();
        }
    }

    /// Waits until device changes stop, keeping other commands for later.
    fn settle(&mut self) {
        loop {
            match self.receiver.recv_timeout(SETTLE_DELAY) {
                Ok(Command::DevicesChanged) => {}
                Ok(other) => self.deferred.push_back(other),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Drops the interfaces that went away and opens the new ones; the ones
    /// still there keep their transport.
    fn rediscover(&mut self) {
        let api = match self.api.as_mut() {
            Some(api) => match api.refresh_devices() {
                Ok(()) => api,
                Err(error) => {
                    tracing::warn!(%error, "could not list HID devices");
                    return;
                }
            },
            None => match HidApi::new() {
                Ok(api) => self.api.insert(api),
                Err(error) => {
                    tracing::warn!(%error, "could not start hidapi");
                    return;
                }
            },
        };
        let present = hidpp_interfaces(api);
        self.links.retain(|link| {
            let kept = present
                .iter()
                .any(|interface| interface.long_path == link.path);
            if !kept {
                tracing::info!(product_id = link.product_id, "HID++ interface gone");
            }
            kept
        });
        let new: Vec<Interface> = present
            .into_iter()
            .filter(|interface| {
                !self
                    .links
                    .iter()
                    .any(|link| link.path == interface.long_path)
            })
            .collect();
        for interface in new {
            if let Some(link) = self.open(&interface) {
                self.links.push(link);
            }
        }
    }

    fn open(&self, interface: &Interface) -> Option<Link> {
        let api = self.api.as_ref()?;
        let reports = Arc::new(AtomicU64::new(0));
        let signal = {
            let reports = reports.clone();
            let commands = self.commands.clone();
            Arc::new(move || {
                reports.fetch_add(1, Ordering::AcqRel);
                let _ = commands.send(Command::Reports);
            })
        };
        let transport = match HidTransport::open_with_signal(api, interface, signal) {
            Ok(transport) => transport,
            Err(error) => {
                tracing::warn!(%error, product_id = interface.product_id, "could not open HID++ interface");
                return None;
            }
        };
        let mut link = Link {
            path: interface.long_path.clone(),
            product_id: interface.product_id,
            reports,
            seen: 0,
            device: probe(transport, interface.product_id)?,
        };
        link.caught_up();
        Some(link)
    }

    /// Index of the link whose mouse the menu acts on: a mouse on a cable
    /// first, as it is the one in use when both are there.
    fn active(&self) -> Option<usize> {
        let mice = || {
            self.links
                .iter()
                .enumerate()
                .filter_map(|(index, link)| match &link.device {
                    Device::Mouse(linked) => Some((index, linked.mouse.connection)),
                    Device::Idle { .. } => None,
                })
        };
        mice()
            .find(|(_, connection)| *connection == Connection::Wired)
            .or_else(|| mice().next())
            .map(|(index, _)| index)
    }

    fn refresh(&mut self) {
        if let Some(index) = self.active() {
            self.read_mouse(index);
        }
    }

    fn read_mouse(&mut self, index: usize) {
        let link = &mut self.links[index];
        let Device::Mouse(linked) = &mut link.device else {
            return;
        };
        let result = read_profiles(linked);
        link.caught_up();
        if let Err(error) = result {
            tracing::info!(%error, "could not read the profiles");
            self.handle_error(index, &error);
        }
    }

    fn switch_to(&mut self, sector: u16) {
        let Some(index) = self.active() else {
            tracing::warn!(sector, "no mouse to switch profile");
            (self.notify)(Update::SwitchFailed(Failure::NoMouse));
            return;
        };
        let link = &mut self.links[index];
        let Device::Mouse(linked) = &mut link.device else {
            return;
        };
        let started = Instant::now();
        let result = linked.mouse.switch_to(sector);
        link.seen = link.reports.load(Ordering::Acquire);
        let elapsed_ms = started.elapsed().as_millis();
        match result {
            Ok(()) => {
                tracing::info!(sector, elapsed_ms, "profile switched");
                linked.active = Some(sector);
                linked.unreachable = false;
            }
            Err(error) => {
                tracing::warn!(%error, sector, elapsed_ms, "profile switch failed");
                if let Error::ProfileNotApplied { active, .. } = error {
                    linked.active = Some(active);
                }
                (self.notify)(Update::SwitchFailed(Failure::of(&error)));
                self.handle_error(index, &error);
            }
        }
    }

    /// Reads what the interfaces sent since our last requests: profile
    /// change notifications, or a sign of life from a sleeping mouse.
    fn read_reports(&mut self) {
        let mut index = 0;
        while index < self.links.len() {
            if self.links[index].has_news() {
                self.links[index].caught_up();
                if !self.read_link(index) {
                    continue;
                }
            }
            index += 1;
        }
    }

    /// Returns false when the link was removed.
    fn read_link(&mut self, index: usize) -> bool {
        match &mut self.links[index].device {
            Device::Mouse(linked) => match linked.mouse.poll_notifications() {
                Ok(frames) => {
                    for frame in &frames {
                        if let Some(sector) = linked.mouse.changed_profile(frame) {
                            tracing::info!(sector, "the mouse changed its profile");
                            linked.active = Some(sector);
                        }
                    }
                    if linked.unreachable {
                        self.read_mouse(index);
                    }
                    index < self.links.len()
                }
                Err(error) => {
                    let removed = matches!(error, Error::Transport(_));
                    self.handle_error(index, &error);
                    !removed
                }
            },
            Device::Idle { .. } => {
                let link = self.links.remove(index);
                let Device::Idle { transport, .. } = link.device else {
                    unreachable!("matched as idle above");
                };
                match probe(transport, link.product_id) {
                    Some(device) => {
                        let mut link = Link { device, ..link };
                        link.caught_up();
                        self.links.insert(index, link);
                        true
                    }
                    None => false,
                }
            }
        }
    }

    fn handle_error(&mut self, index: usize, error: &Error) {
        match error {
            Error::Transport(_) => {
                let link = self.links.remove(index);
                tracing::info!(%error, product_id = link.product_id, "lost the HID++ interface");
                self.rediscover();
            }
            Error::Device(device) if device.is_unreachable() => self.mark_unreachable(index),
            Error::Timeout => self.mark_unreachable(index),
            _ => {}
        }
    }

    fn mark_unreachable(&mut self, index: usize) {
        if let Device::Mouse(linked) = &mut self.links[index].device {
            linked.unreachable = true;
        }
    }

    fn state(&self) -> MouseState {
        if let Some(index) = self.active()
            && let Device::Mouse(linked) = &self.links[index].device
        {
            return MouseState::Present(MouseView {
                name: linked.mouse.name.clone(),
                connection: linked.mouse.connection,
                profiles: linked.profiles.clone(),
                active: linked.active,
                unreachable: linked.unreachable,
            });
        }
        let asleep = self.links.iter().any(|link| match &link.device {
            Device::Idle { reason, .. } => Failure::of(reason) == Failure::Asleep,
            Device::Mouse(_) => false,
        });
        MouseState::Absent { asleep }
    }

    fn publish(&mut self) {
        let state = self.state();
        if self.published.as_ref() != Some(&state) {
            tracing::info!(?state, "mouse state");
            self.published = Some(state.clone());
            (self.notify)(Update::State(state));
        }
    }
}

/// Looks for a mouse behind a freshly opened or idle transport. `None` when
/// the transport failed.
fn probe(transport: HidTransport, product_id: u16) -> Option<Device> {
    match Mouse::find(transport, product_id) {
        Ok(mouse) => {
            let mut linked = LinkedMouse {
                mouse,
                profiles: Vec::new(),
                active: None,
                unreachable: false,
            };
            if let Err(error) = read_profiles(&mut linked) {
                tracing::info!(%error, "could not read the profiles");
                linked.unreachable = Failure::of(&error) == Failure::Asleep;
            }
            Some(Device::Mouse(Box::new(linked)))
        }
        Err(NotFound {
            reason,
            transport: Some(transport),
        }) => {
            tracing::info!(%reason, product_id, "no mouse answers on this interface");
            Some(Device::Idle { transport, reason })
        }
        Err(NotFound {
            reason,
            transport: None,
        }) => {
            tracing::warn!(%reason, product_id, "HID++ interface failed while probing");
            None
        }
    }
}

fn read_profiles(linked: &mut LinkedMouse) -> Result<(), Error> {
    let profiles = linked.mouse.profiles()?;
    let active = linked.mouse.current_profile()?;
    linked.profiles = profiles;
    linked.active = Some(active);
    linked.unreachable = false;
    Ok(())
}
