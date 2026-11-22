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

/// Replays a capture: checks each write, answers with what followed it.
#[derive(Debug)]
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

fn discover(
    script: Vec<Exchange>,
    product_id: u16,
) -> (Result<Mouse<Replay>, NotFound<Replay>>, usize) {
    let script = Rc::new(RefCell::new(VecDeque::from(script)));
    let transport = Replay {
        script: script.clone(),
        inbox: VecDeque::new(),
    };
    let result = Mouse::find(transport, product_id);
    let left = script.borrow().len();
    (result, left)
}

fn summary(mouse: &Mouse<Replay>) -> (String, Connection, u8) {
    (
        mouse.name.clone(),
        mouse.connection,
        mouse.info().profile_count,
    )
}

fn reason(result: Result<Mouse<Replay>, NotFound<Replay>>) -> (Error, bool) {
    let not_found = result.expect_err("no mouse");
    (not_found.reason, not_found.transport.is_some())
}

#[test]
fn receiver_discovery_keeps_the_mouse_at_index_one() {
    // The capture goes on with indices 2 to 6, which are no longer probed
    // once a device answered.
    let script = exchanges(RECEIVER);
    let not_probed = script.len() - 7;
    let (result, left) = discover(script, 0xC547);

    assert_eq!(
        (summary(&result.expect("mouse")), left),
        (
            ("G502 X PLUS".to_owned(), Connection::Receiver(1), 5),
            not_probed
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
        (summary(&result.expect("mouse")), left),
        (("G502 X PLUS".to_owned(), Connection::Wired, 5), 0)
    );
}

#[test]
fn receiver_with_mouse_switched_off_reports_unknown_device() {
    let receiver = exchanges(RECEIVER);
    let errors = exchanges(ERRORS);
    // Assembled from the discovery and errors captures: index 0xFF, then
    // index 1 with the mouse switched off, then indices 2 to 6.
    let script = [
        vec![receiver[0].clone(), errors[4].clone()],
        receiver[7..].to_vec(),
    ]
    .concat();
    let (result, left) = discover(script, 0xC547);

    assert_eq!(
        (reason(result), left),
        ((Error::Device(DeviceError::Hidpp10(0x08)), true), 0)
    );
}

#[test]
fn mouse_that_does_not_answer_is_reported_instead_of_empty_indices() {
    // Assembled from the discovery capture: index 1 gets no answer at all
    // (its reply removed), indices 2 to 6 are empty.
    let receiver = exchanges(RECEIVER);
    let silent = (receiver[1].0.clone(), Vec::new());
    let script = [vec![receiver[0].clone(), silent], receiver[7..].to_vec()].concat();
    let (result, left) = discover(script, 0xC547);

    assert_eq!((reason(result), left), ((Error::Timeout, true), 0));
}
