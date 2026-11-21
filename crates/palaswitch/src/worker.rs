//! The mouse thread: owns every HID transport and does all HID++ work, so
//! the interface thread never waits on the mouse.
//!
//! It sleeps until it gets a command: from the interface, from a device
//! change, or from a reader thread when a report arrives.

use std::collections::VecDeque;
use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use palaswitch_device::{Connection, Interface, Mouse, NotFound, ReportSignal};
use palaswitch_hidpp::features::Profile;
use palaswitch_hidpp::{Error, InputReport, SOFTWARE_ID, Transport};

use crate::hardware::{Hardware, HidHardware};
use crate::model::{Failure, MouseState, MouseView};

/// Device changes come in bursts, one per collection; the new device is
/// only opened once they stop for this long.
const SETTLE_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Read the profiles and the active profile again, and look again for a
    /// mouse that was asleep.
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
            .spawn(move || {
                MouseThread::new(HidHardware::default(), sender, receiver, Box::new(notify)).run();
            })?;
        Ok(Self { commands })
    }

    /// Returns false when the mouse thread is gone.
    pub fn send(&self, command: Command) -> bool {
        let sent = self.commands.send(command).is_ok();
        if !sent {
            tracing::error!(?command, "the mouse thread is gone");
        }
        sent
    }
}

struct MouseThread<H: Hardware> {
    hardware: H,
    links: Vec<Link<H::Transport>>,
    commands: Sender<Command>,
    receiver: Receiver<Command>,
    deferred: VecDeque<Command>,
    notify: Box<dyn Fn(Update) + Send>,
    published: Option<MouseState>,
    /// Set once a reader thread asked for a read, until the read starts: a
    /// burst of reports wakes the thread once.
    reports_pending: Arc<AtomicBool>,
}

/// One open HID++ interface.
struct Link<T: Transport> {
    path: CString,
    product_id: u16,
    device: Device<T>,
}

enum Device<T: Transport> {
    Mouse(Box<LinkedMouse<T>>),
    /// Nothing with onboard profiles answered. When the reason is a mouse
    /// that does not answer (asleep or switched off behind a receiver), the
    /// interface is probed again when it sends something or on a refresh.
    Idle {
        transport: T,
        reason: Error,
    },
}

struct LinkedMouse<T: Transport> {
    mouse: Mouse<T>,
    profiles: Vec<Profile>,
    active: Option<u16>,
    unreachable: bool,
}

/// Whether an interface where nothing answered for `reason` may answer
/// later without being plugged again.
fn may_wake(reason: &Error) -> bool {
    Failure::of(reason) == Failure::Asleep
}

/// Whether a link is still at its index after an operation on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kept {
    Yes,
    Removed,
}

impl<H: Hardware> MouseThread<H> {
    fn new(
        hardware: H,
        commands: Sender<Command>,
        receiver: Receiver<Command>,
        notify: Box<dyn Fn(Update) + Send>,
    ) -> Self {
        Self {
            hardware,
            links: Vec::new(),
            commands,
            receiver,
            deferred: VecDeque::new(),
            notify,
            published: None,
            reports_pending: Arc::new(AtomicBool::new(false)),
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
            if command == Command::DevicesChanged {
                self.settle();
            }
            self.handle(command);
            self.publish();
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::DevicesChanged => {
                self.rediscover();
            }
            Command::Refresh => self.refresh(),
            Command::SwitchTo(sector) => self.switch_to(sector),
            Command::Reports => self.read_reports(),
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
    /// still there keep their transport. Returns the interfaces opened, whose
    /// profiles were read on the way.
    fn rediscover(&mut self) -> Vec<CString> {
        self.rediscover_after(self.active_path())
    }

    /// [`Self::rediscover`], then reads the profiles again when the mouse in
    /// use is no longer the one at `before` and was already open.
    fn rediscover_after(&mut self, before: Option<CString>) -> Vec<CString> {
        let present = match self.hardware.interfaces() {
            Ok(present) => present,
            Err(error) => {
                tracing::warn!(%error, "could not list HID devices");
                return Vec::new();
            }
        };
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
        let mut opened = Vec::new();
        for interface in new {
            if let Some(link) = self.open(&interface) {
                opened.push(link.path.clone());
                self.links.push(link);
            }
        }
        let after = self.active_path();
        if after != before
            && let Some(path) = after
            && !opened.contains(&path)
            && let Some(index) = self.active()
        {
            self.read_mouse(index);
        }
        opened
    }

    fn open(&mut self, interface: &Interface) -> Option<Link<H::Transport>> {
        let commands = self.commands.clone();
        let pending = self.reports_pending.clone();
        let signal: ReportSignal = Arc::new(move || {
            if !pending.swap(true, Ordering::AcqRel) {
                let _ = commands.send(Command::Reports);
            }
        });
        let transport = match self.hardware.open(interface, signal) {
            Ok(transport) => transport,
            Err(error) => {
                tracing::warn!(%error, product_id = interface.product_id, "could not open HID++ interface");
                return None;
            }
        };
        Some(Link {
            path: interface.long_path.clone(),
            product_id: interface.product_id,
            device: probe(transport, interface.product_id)?,
        })
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

    fn active_path(&self) -> Option<CString> {
        self.active().map(|index| self.links[index].path.clone())
    }

    /// Lists the interfaces again (an interface that failed to open is
    /// tried again), reads the mouse in use, then probes every interface
    /// where nothing answered. A refresh comes from the user, so it may
    /// also probe interfaces whose traffic does not count as a wake-up.
    fn refresh(&mut self) {
        let fresh = self.rediscover();
        // A mouse opened just now has been read already.
        if let Some(index) = self.active()
            && !fresh.contains(&self.links[index].path)
        {
            self.read_mouse(index);
        }
        let idle: Vec<CString> = self
            .links
            .iter()
            .filter(|link| matches!(link.device, Device::Idle { .. }))
            .map(|link| link.path.clone())
            .collect();
        for path in idle {
            if let Some(index) = self.links.iter().position(|link| link.path == path) {
                self.reprobe(index);
            }
        }
    }

    fn read_mouse(&mut self, index: usize) -> Kept {
        let Device::Mouse(linked) = &mut self.links[index].device else {
            return Kept::Yes;
        };
        match read_profiles(linked) {
            Ok(()) => Kept::Yes,
            Err(error) => {
                tracing::info!(%error, "could not read the profiles");
                self.handle_error(index, &error)
            }
        }
    }

    fn switch_to(&mut self, sector: u16) {
        let Some(index) = self.active() else {
            tracing::warn!(sector, "no mouse to switch profile");
            (self.notify)(Update::SwitchFailed(Failure::NoMouse));
            return;
        };
        let Device::Mouse(linked) = &mut self.links[index].device else {
            return;
        };
        let started = Instant::now();
        let result = linked.mouse.switch_to(sector);
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

    /// Reads what the interfaces sent on their own: profile change
    /// notifications, or a sign of life from a sleeping mouse. Answers to
    /// our own requests never get here: each request reads its answer.
    fn read_reports(&mut self) {
        // Cleared before reading: a report that arrives from now on asks
        // for another read.
        self.reports_pending.store(false, Ordering::Release);
        let mut index = 0;
        while index < self.links.len() {
            if self.read_link(index) == Kept::Removed {
                continue;
            }
            index += 1;
        }
    }

    fn read_link(&mut self, index: usize) -> Kept {
        match &mut self.links[index].device {
            Device::Mouse(linked) => match linked.mouse.poll() {
                Ok(polled) => {
                    for frame in &polled.notifications {
                        if let Some(sector) = linked.mouse.changed_profile(frame) {
                            tracing::info!(sector, "the mouse changed its profile");
                            linked.active = Some(sector);
                        }
                    }
                    if polled.reports > 0 && linked.unreachable {
                        self.read_mouse(index)
                    } else {
                        Kept::Yes
                    }
                }
                Err(error) => self.handle_error(index, &error),
            },
            Device::Idle { transport, reason } => {
                let mut reports = 0;
                loop {
                    match transport.read(Duration::ZERO) {
                        Ok(Some(bytes)) => {
                            // A late answer to our own probe is no sign of life.
                            if InputReport::parse(&bytes).software_id() != Some(SOFTWARE_ID) {
                                reports += 1;
                            }
                        }
                        Ok(None) => break,
                        Err(error) => return self.handle_error(index, &Error::Transport(error)),
                    }
                }
                if reports > 0 && may_wake(reason) {
                    self.reprobe(index)
                } else {
                    Kept::Yes
                }
            }
        }
    }

    /// Looks again for a mouse behind an idle interface.
    fn reprobe(&mut self, index: usize) -> Kept {
        let link = self.links.remove(index);
        let Device::Idle { transport, .. } = link.device else {
            self.links.insert(index, link);
            return Kept::Yes;
        };
        match probe(transport, link.product_id) {
            Some(device) => {
                self.links.insert(index, Link { device, ..link });
                Kept::Yes
            }
            None => {
                self.rediscover();
                Kept::Removed
            }
        }
    }

    fn handle_error(&mut self, index: usize, error: &Error) -> Kept {
        match error {
            Error::Transport(_) => {
                let before = self.active_path();
                let link = self.links.remove(index);
                tracing::info!(%error, product_id = link.product_id, "lost the HID++ interface");
                self.rediscover_after(before);
                Kept::Removed
            }
            _ if Failure::of(error) == Failure::Asleep => {
                if let Device::Mouse(linked) = &mut self.links[index].device {
                    linked.unreachable = true;
                }
                Kept::Yes
            }
            _ => Kept::Yes,
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
            Device::Idle { reason, .. } => may_wake(reason),
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
fn probe<T: Transport>(transport: T, product_id: u16) -> Option<Device<T>> {
    match Mouse::find(transport, product_id) {
        Ok(mouse) => {
            let mut linked = LinkedMouse {
                mouse,
                profiles: Vec::new(),
                active: None,
                unreachable: false,
            };
            match read_profiles(&mut linked) {
                Ok(()) => {}
                Err(error @ Error::Transport(_)) => {
                    tracing::warn!(%error, product_id, "HID++ interface failed while reading");
                    return None;
                }
                Err(error) => {
                    tracing::info!(%error, "could not read the profiles");
                    linked.unreachable = may_wake(&error);
                }
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

fn read_profiles<T: Transport>(linked: &mut LinkedMouse<T>) -> Result<(), Error> {
    let profiles = linked.mouse.profiles()?;
    let active = linked.mouse.current_profile()?;
    linked.profiles = profiles;
    linked.active = Some(active);
    linked.unreachable = false;
    Ok(())
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
