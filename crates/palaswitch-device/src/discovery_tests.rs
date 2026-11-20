use std::collections::VecDeque;
use std::time::Duration;

use std::cell::RefCell;
use std::rc::Rc;

use palaswitch_hidpp::{LONG_REPORT_LEN, Transport, TransportError};

use super::*;

const RECEIVER: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-lightspeed-discovery.txt");
const WIRED: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-wired-discovery.txt");
const ERRORS: &str =
    include_str!("../../palaswitch-hidpp/tests/fixtures/g502x-plus-lightspeed-errors.txt");

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

/// Replays a capture: checks each write, answers with what followed it. All
/// transports opened for one interface share the same script, as handles to
/// the same device would.
#[derive(Debug, Clone)]
struct Replay {
    script: Rc<RefCell<VecDeque<Exchange>>>,
    inbox: VecDeque<Vec<u8>>,
}

impl Transport for Replay {
    fn write(&mut self, report: &[u8; LONG_REPORT_LEN]) -> Result<(), TransportError> {
        let (expected, replies) = self
            .script
            .borrow_mut()
            .pop_front()
            .expect("unexpected write");
        assert_eq!(report.as_slice(), expected.as_slice());
        self.inbox.extend(replies);
        Ok(())
    }

    fn read(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>, TransportError> {
        Ok(self.inbox.pop_front())
    }
}

fn discover(script: Vec<Exchange>, product_id: u16) -> (Result<Vec<Mouse<Replay>>, Error>, usize) {
    let script = Rc::new(RefCell::new(VecDeque::from(script)));
    let open = || {
        Ok(Replay {
            script: script.clone(),
            inbox: VecDeque::new(),
        })
    };
    let result = probe_interface(open, product_id);
    let left = script.borrow().len();
    (result, left)
}

fn summary(mice: &[Mouse<Replay>]) -> Vec<(String, Connection, u8)> {
    mice.iter()
        .map(|mouse| {
            (
                mouse.name.clone(),
                mouse.connection,
                mouse.info().profile_count,
            )
        })
        .collect()
}

#[test]
fn receiver_discovery_finds_the_mouse_at_index_one() {
    let (result, left) = discover(exchanges(RECEIVER), 0xC547);

    assert_eq!(
        (summary(&result.expect("mice")), left),
        (
            vec![("G502 X PLUS".to_owned(), Connection::Receiver(1), 5)],
            0
        )
    );
}

#[test]
fn wired_discovery_stops_at_index_ff() {
    // The capture ends with a ROM directory read the discovery never sends.
    let mut script = exchanges(WIRED);
    script.pop();
    let (result, left) = discover(script, 0xC095);

    assert_eq!(
        (summary(&result.expect("mice")), left),
        (vec![("G502 X PLUS".to_owned(), Connection::Wired, 5)], 0)
    );
}

#[test]
fn receiver_with_mouse_switched_off_reports_unknown_device() {
    let receiver = exchanges(RECEIVER);
    let errors = exchanges(ERRORS);
    // Index 0xFF, then index 1 with the mouse off, then indices 2 to 6.
    let script = [
        vec![receiver[0].clone(), errors[4].clone()],
        receiver[7..].to_vec(),
    ]
    .concat();
    let (result, left) = discover(script, 0xC547);

    assert_eq!(
        (result.err(), left),
        (Some(Error::Device(DeviceError::Hidpp10(0x08))), 0)
    );
}

#[test]
fn mouse_that_does_not_answer_is_reported_instead_of_empty_indices() {
    // Index 1 gets no answer at all; indices 2 to 6 are empty.
    let receiver = exchanges(RECEIVER);
    let silent = (receiver[1].0.clone(), Vec::new());
    let script = [vec![receiver[0].clone(), silent], receiver[7..].to_vec()].concat();
    let (result, left) = discover(script, 0xC547);

    assert_eq!((result.err(), left), (Some(Error::Timeout), 0));
}
