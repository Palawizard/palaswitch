use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use palaswitch_hidpp::LONG_REPORT_LEN;

use super::*;

const RECEIVER: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-lightspeed-discovery.txt");
const PROFILES: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-lightspeed-profiles.txt");
const READ: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-lightspeed-read.txt");
const ERRORS: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-lightspeed-errors.txt");
const SWITCH: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-lightspeed-switch.txt");
const WIRED: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-wired-discovery.txt");

const RECEIVER_PATH: &str = "receiver";
const WIRED_PATH: &str = "wired";

type Exchange = (Vec<u8>, Vec<Vec<u8>>);

/// Exchanges of a capture: each `TX` line with the `RX`/`RXS` lines after it.
fn exchanges(capture: &str) -> Vec<Exchange> {
    let mut exchanges: Vec<Exchange> = Vec::new();
    for line in capture
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
    {
        let mut parts = line.split_whitespace();
        let direction = parts.next().expect("direction");
        let bytes: Vec<u8> = parts
            .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
            .collect();
        if direction == "TX" {
            exchanges.push((bytes, Vec::new()));
        } else {
            exchanges.last_mut().expect("TX first").1.push(bytes);
        }
    }
    exchanges
}

/// The receiver finds the mouse at index 1, then the profiles are read:
/// the directory (profiles 1 to 3 enabled) and the active profile (1).
fn receiver_finds_mouse() -> Vec<Exchange> {
    [exchanges(RECEIVER)[..7].to_vec(), receiver_reads_profiles()].concat()
}

fn receiver_reads_profiles() -> Vec<Exchange> {
    let profiles = exchanges(PROFILES);
    vec![
        profiles[4].clone(),
        profiles[5].clone(),
        exchanges(READ)[3].clone(),
    ]
}

/// Index 0xFF is the receiver, index 1 answers "unknown device" (mouse in
/// deep sleep), indices 2 to 6 are empty.
fn receiver_mouse_asleep() -> Vec<Exchange> {
    let receiver = exchanges(RECEIVER);
    [
        vec![receiver[0].clone(), exchanges(ERRORS)[4].clone()],
        receiver[7..].to_vec(),
    ]
    .concat()
}

/// Shared by every transport opened on one interface, as the device is.
#[derive(Default)]
struct Line {
    script: VecDeque<Exchange>,
    inbox: VecDeque<Vec<u8>>,
    /// Reads and writes fail, as on a device being unplugged.
    broken: bool,
}

/// Replays captures. A write past the end of its script gets no answer,
/// as a mouse that does not answer.
struct Replay(Rc<RefCell<Line>>);

impl Transport for Replay {
    fn write(&mut self, report: &[u8; LONG_REPORT_LEN]) -> Result<(), TransportError> {
        let mut line = self.0.borrow_mut();
        if line.broken {
            return Err(TransportError("unplugged".to_owned()));
        }
        if let Some((expected, replies)) = line.script.pop_front() {
            assert_eq!(report.as_slice(), expected.as_slice());
            line.inbox.extend(replies);
        }
        Ok(())
    }

    fn read(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>, TransportError> {
        let mut line = self.0.borrow_mut();
        if line.broken {
            return Err(TransportError("unplugged".to_owned()));
        }
        Ok(line.inbox.pop_front())
    }
}

type Lines = HashMap<&'static str, Rc<RefCell<Line>>>;

#[derive(Default)]
struct FakeHardware {
    present: Rc<RefCell<Vec<&'static str>>>,
    lines: Rc<RefCell<Lines>>,
}

impl FakeHardware {
    fn plug(&self, path: &'static str, script: Vec<Exchange>) {
        self.present.borrow_mut().push(path);
        self.script(path, script);
    }

    fn unplug(&self, path: &'static str) {
        self.present.borrow_mut().retain(|present| *present != path);
    }

    fn script(&self, path: &'static str, script: Vec<Exchange>) {
        let mut lines = self.lines.borrow_mut();
        let line = lines.entry(path).or_default();
        line.borrow_mut().script.extend(script);
    }

    fn receive(&self, path: &'static str, report: Vec<u8>) {
        self.lines.borrow()[path]
            .borrow_mut()
            .inbox
            .push_back(report);
    }

    fn break_line(&self, path: &'static str) {
        self.lines.borrow()[path].borrow_mut().broken = true;
    }

    fn unanswered(&self, path: &'static str) -> usize {
        self.lines.borrow()[path].borrow().script.len()
    }

    fn handle(&self) -> Self {
        Self {
            present: self.present.clone(),
            lines: self.lines.clone(),
        }
    }
}

impl Hardware for FakeHardware {
    type Transport = Replay;

    fn interfaces(&mut self) -> Result<Vec<Interface>, String> {
        Ok(self
            .present
            .borrow()
            .iter()
            .map(|path| Interface {
                product_id: if *path == WIRED_PATH { 0xC095 } else { 0xC547 },
                product: String::new(),
                long_path: CString::new(*path).expect("path"),
                short_path: None,
            })
            .collect())
    }

    fn open(
        &mut self,
        interface: &Interface,
        _signal: ReportSignal,
    ) -> Result<Replay, TransportError> {
        let path = interface.long_path.to_str().expect("path");
        Ok(Replay(self.lines.borrow()[path].clone()))
    }
}

fn thread(hardware: &FakeHardware) -> (MouseThread<FakeHardware>, Receiver<Update>) {
    let (commands, receiver) = mpsc::channel();
    let (updates, received) = mpsc::channel();
    let notify = Box::new(move |update| {
        let _ = updates.send(update);
    });
    (
        MouseThread::new(hardware.handle(), commands, receiver, notify),
        received,
    )
}

fn summary(state: &MouseState) -> Option<(Connection, Option<u16>, bool)> {
    match state {
        MouseState::Present(view) => Some((view.connection, view.active, view.unreachable)),
        _ => None,
    }
}

#[test]
fn receiver_mouse_is_found_with_its_profiles() {
    let hardware = FakeHardware::default();
    hardware.plug(RECEIVER_PATH, receiver_finds_mouse());
    let (mut thread, _) = thread(&hardware);

    thread.rediscover();

    let MouseState::Present(view) = thread.state() else {
        panic!("no mouse");
    };
    assert_eq!(
        (view.name.as_str(), view.connection, view.active),
        ("G502 X PLUS", Connection::Receiver(1), Some(1))
    );
    assert_eq!(view.profiles.iter().filter(|p| p.enabled).count(), 3);
    assert_eq!(hardware.unanswered(RECEIVER_PATH), 0);
}

#[test]
fn sleeping_mouse_is_found_again_on_refresh() {
    let hardware = FakeHardware::default();
    hardware.plug(RECEIVER_PATH, receiver_mouse_asleep());
    let (mut thread, _) = thread(&hardware);
    thread.rediscover();
    let asleep = thread.state();
    hardware.script(RECEIVER_PATH, receiver_finds_mouse());

    thread.handle(Command::Refresh);

    assert_eq!(asleep, MouseState::Absent { asleep: true });
    assert_eq!(
        summary(&thread.state()),
        Some((Connection::Receiver(1), Some(1), false))
    );
    assert_eq!(hardware.unanswered(RECEIVER_PATH), 0);
}

#[test]
fn sleeping_mouse_is_found_again_when_the_receiver_sends_something() {
    // Any report will do; this one is a real notification of the mouse.
    let report = exchanges(SWITCH)[1].1[1].clone();
    let hardware = FakeHardware::default();
    hardware.plug(RECEIVER_PATH, receiver_mouse_asleep());
    let (mut thread, _) = thread(&hardware);
    thread.rediscover();
    hardware.script(RECEIVER_PATH, receiver_finds_mouse());
    hardware.receive(RECEIVER_PATH, report);

    thread.handle(Command::Reports);

    assert_eq!(
        summary(&thread.state()),
        Some((Connection::Receiver(1), Some(1), false))
    );
}

#[test]
fn quiet_receiver_is_not_probed_again_on_its_own_answers() {
    let hardware = FakeHardware::default();
    hardware.plug(RECEIVER_PATH, receiver_mouse_asleep());
    let (mut thread, _) = thread(&hardware);
    thread.rediscover();
    hardware.script(RECEIVER_PATH, receiver_finds_mouse());
    let waiting = hardware.unanswered(RECEIVER_PATH);

    thread.handle(Command::Reports);

    assert_eq!(thread.state(), MouseState::Absent { asleep: true });
    assert_eq!(hardware.unanswered(RECEIVER_PATH), waiting);
}

#[test]
fn cable_is_preferred_and_receiver_read_again_once_unplugged() {
    // Assembled: the receiver and cable discoveries at the same time. The
    // cable capture has no user directory read, so the cable mouse does not
    // answer it and its profile stays unknown.
    let hardware = FakeHardware::default();
    hardware.plug(RECEIVER_PATH, receiver_finds_mouse());
    hardware.plug(WIRED_PATH, exchanges(WIRED)[..6].to_vec());
    let (mut thread, _) = thread(&hardware);
    thread.rediscover();
    let both = summary(&thread.state());
    hardware.script(RECEIVER_PATH, receiver_reads_profiles());
    hardware.unplug(WIRED_PATH);

    thread.handle(Command::DevicesChanged);

    assert_eq!(both, Some((Connection::Wired, None, true)));
    assert_eq!(
        summary(&thread.state()),
        Some((Connection::Receiver(1), Some(1), false))
    );
    assert_eq!(hardware.unanswered(RECEIVER_PATH), 0);
}

#[test]
fn receiver_is_read_again_when_the_cable_fails_before_windows_says_it_is_gone() {
    let hardware = FakeHardware::default();
    hardware.plug(RECEIVER_PATH, receiver_finds_mouse());
    hardware.plug(WIRED_PATH, exchanges(WIRED)[..6].to_vec());
    let (mut thread, _) = thread(&hardware);
    thread.rediscover();
    hardware.script(RECEIVER_PATH, receiver_reads_profiles());
    hardware.break_line(WIRED_PATH);

    thread.handle(Command::Reports);

    assert_eq!(
        summary(&thread.state()),
        Some((Connection::Receiver(1), Some(1), false))
    );
    assert_eq!(hardware.unanswered(RECEIVER_PATH), 0);
}

#[test]
fn switch_on_a_sleeping_mouse_says_to_wake_it() {
    let hardware = FakeHardware::default();
    hardware.plug(RECEIVER_PATH, receiver_finds_mouse());
    let (mut thread, updates) = thread(&hardware);
    thread.rediscover();
    // Assembled: setOnboardMode answered (switch capture), then
    // setCurrentProfile(2) answered "unknown device" (errors capture).
    hardware.script(
        RECEIVER_PATH,
        vec![exchanges(SWITCH)[0].clone(), exchanges(ERRORS)[5].clone()],
    );

    thread.handle(Command::SwitchTo(2));

    assert_eq!(
        updates.try_iter().collect::<Vec<_>>(),
        [Update::SwitchFailed(Failure::Asleep)]
    );
    assert_eq!(
        summary(&thread.state()),
        Some((Connection::Receiver(1), Some(1), true))
    );
}

#[test]
fn switch_without_a_mouse_says_so() {
    let hardware = FakeHardware::default();
    let (mut thread, updates) = thread(&hardware);
    thread.rediscover();

    thread.handle(Command::SwitchTo(2));

    assert_eq!(
        updates.try_iter().collect::<Vec<_>>(),
        [Update::SwitchFailed(Failure::NoMouse)]
    );
    assert_eq!(thread.state(), MouseState::Absent { asleep: false });
}
