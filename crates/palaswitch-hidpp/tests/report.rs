mod common;

use common::line_bytes;
use palaswitch_hidpp::{DeviceError, InputReport, LongFrame, Reply, classify, should_retry};

const ERRORS: &str = include_str!("fixtures/g502x-plus-lightspeed-errors.txt");
const FOREIGN: &str = include_str!("fixtures/g502x-plus-lightspeed-foreign.txt");
const READ: &str = include_str!("fixtures/g502x-plus-lightspeed-read.txt");
const SWITCH: &str = include_str!("fixtures/g502x-plus-lightspeed-switch.txt");
const WIRED: &str = include_str!("fixtures/g502x-plus-wired-discovery.txt");

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

#[test]
fn receiver_rejects_hidpp20_ping_at_its_own_index() {
    assert_eq!(
        reply((ERRORS, 6), (ERRORS, 7)),
        Reply::Failed(DeviceError::Hidpp10(0x01))
    );
}

#[test]
fn receiver_rejecting_ping_does_not_mean_unreachable() {
    assert!(!DeviceError::Hidpp10(0x01).is_unreachable());
}

#[test]
fn receiver_error_for_another_function_is_unrelated() {
    // getCurrentProfile waiting at index 1, against the receiver error that
    // answered setCurrentProfile at the same index with the mouse switched off.
    assert_eq!(reply((READ, 6), (ERRORS, 11)), Reply::Unrelated);
}

#[test]
fn receiver_error_for_another_device_index_is_unrelated() {
    // getFeature(0x8100) at index 1, against the error for the same request at index 2.
    assert_eq!(reply((READ, 0), (ERRORS, 1)), Reply::Unrelated);
}

#[test]
fn busy_and_unreachable_errors_are_retried_until_the_last_try() {
    let retryable = [
        DeviceError::Hidpp20(0x08),
        DeviceError::Hidpp10(0x04),
        DeviceError::Hidpp10(0x07),
        DeviceError::Hidpp10(0x09),
    ];

    let decisions: Vec<[bool; 3]> = retryable
        .iter()
        .map(|&error| [1, 2, 3].map(|attempt| should_retry(error, attempt, 3)))
        .collect();

    assert_eq!(decisions, vec![[true, true, false]; 4]);
}

#[test]
fn unknown_device_and_invalid_requests_are_not_retried() {
    let final_errors = [
        DeviceError::Hidpp10(0x08),
        DeviceError::Hidpp10(0x01),
        DeviceError::Hidpp20(0x02),
        DeviceError::Hidpp20(0x07),
    ];

    assert!(final_errors.iter().all(|&error| !should_retry(error, 1, 3)));
}

#[test]
fn response_to_another_function_of_the_same_feature_is_unrelated() {
    // getCurrentProfile against the getOnboardMode answer.
    assert_eq!(reply((READ, 6), (READ, 5)), Reply::Unrelated);
}

#[test]
fn response_from_another_feature_is_unrelated() {
    // getFeature (root, feature index 0x00) against the getInfo answer (0x0B).
    assert_eq!(reply((READ, 0), (READ, 3)), Reply::Unrelated);
}

#[test]
fn same_response_for_another_device_index_is_unrelated() {
    // getFeature(0x8100) sent to the cable (index 0xFF) against the answer
    // the receiver gave for index 1.
    assert_eq!(reply((WIRED, 2), (READ, 1)), Reply::Unrelated);
}

#[test]
fn receiver_error_for_another_feature_is_unrelated() {
    // setOnboardMode (feature index 0x0B) against the receiver error for a
    // ping (root, 0x00) at the same index.
    assert_eq!(reply((SWITCH, 0), (ERRORS, 9)), Reply::Unrelated);
}
