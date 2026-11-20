mod common;

use common::line_bytes;
use palaswitch_hidpp::{DeviceError, InputReport, LongFrame, Reply, classify};

const ERRORS: &str = include_str!("fixtures/g502x-plus-lightspeed-errors.txt");
const FOREIGN: &str = include_str!("fixtures/g502x-plus-lightspeed-foreign.txt");
const READ: &str = include_str!("fixtures/g502x-plus-lightspeed-read.txt");
const SWITCH: &str = include_str!("fixtures/g502x-plus-lightspeed-switch.txt");

fn reply(request_line: (&str, usize), report_line: (&str, usize)) -> Reply {
    let request = LongFrame::parse(&line_bytes(request_line.0, request_line.1)).expect("request");
    let report = InputReport::parse(&line_bytes(report_line.0, report_line.1));
    classify(&request, &report)
}

#[test]
fn receiver_error_for_empty_index_fails_the_request() {
    assert_eq!(
        reply((ERRORS, 0), (ERRORS, 1)),
        Reply::Failed(DeviceError::Hidpp10(0x08))
    );
}

#[test]
fn receiver_error_for_empty_index_means_unreachable() {
    assert!(DeviceError::Hidpp10(0x08).is_unreachable());
}

#[test]
fn device_error_for_unknown_function_fails_the_request() {
    assert_eq!(
        reply((ERRORS, 2), (ERRORS, 3)),
        Reply::Failed(DeviceError::Hidpp20(0x07))
    );
}

#[test]
fn invalid_argument_is_not_retryable() {
    let Reply::Failed(error) = reply((ERRORS, 4), (ERRORS, 5)) else {
        panic!("expected an error");
    };

    assert!(!error.is_retryable());
}

#[test]
fn response_meant_for_another_program_is_unrelated() {
    // Our getCurrentProfile request against the same response sent to software ID 0x0D.
    assert_eq!(reply((READ, 6), (FOREIGN, 1)), Reply::Unrelated);
}

#[test]
fn notification_is_unrelated_to_a_pending_request() {
    assert_eq!(reply((SWITCH, 2), (SWITCH, 4)), Reply::Unrelated);
}

#[test]
fn own_response_answers_the_request() {
    let Reply::Response(frame) = reply((READ, 6), (READ, 7)) else {
        panic!("expected a response");
    };

    assert_eq!(frame.params()[..2], [0x00, 0x01]);
}

#[test]
fn receiver_error_is_not_read_as_a_long_frame() {
    assert!(matches!(
        InputReport::parse(&line_bytes(ERRORS, 1)),
        InputReport::Error { .. }
    ));
}
