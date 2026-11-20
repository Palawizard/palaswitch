use palaswitch_hidpp::{FrameError, LongFrame, SOFTWARE_ID};

const CAPTURE: &str = include_str!("fixtures/g502x-plus-lightspeed-read.txt");

/// Frames of the capture, in order, as (direction, bytes).
fn captured_frames() -> Vec<(&'static str, Vec<u8>)> {
    CAPTURE
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let direction = parts.next().expect("direction");
            let bytes = parts
                .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte"))
                .collect();
            (direction, bytes)
        })
        .collect()
}

fn captured(index: usize) -> Vec<u8> {
    captured_frames().swap_remove(index).1
}

#[test]
fn root_get_feature_request_matches_captured_bytes() {
    let request = LongFrame::request(0x01, 0x00, 0, &[0x81, 0x00]).expect("valid request");

    assert_eq!(request.to_bytes().as_slice(), captured(0).as_slice());
}

#[test]
fn get_current_profile_request_matches_captured_bytes() {
    let request = LongFrame::request(0x01, 0x0B, 4, &[]).expect("valid request");

    assert_eq!(request.to_bytes().as_slice(), captured(6).as_slice());
}

#[test]
fn get_current_profile_response_carries_profile_one() {
    let response = LongFrame::parse(&captured(7)).expect("valid frame");

    assert_eq!(
        (
            response.device_index,
            response.feature_index,
            response.function,
            response.software_id,
            &response.params[..2],
        ),
        (0x01, 0x0B, 4, SOFTWARE_ID, &[0x00, 0x01][..]),
    );
}

#[test]
fn every_captured_frame_survives_parse_and_encode() {
    for (_, bytes) in captured_frames() {
        let frame = LongFrame::parse(&bytes).expect("valid frame");

        assert_eq!(frame.to_bytes().as_slice(), bytes.as_slice());
    }
}

#[test]
fn parse_rejects_short_report() {
    assert_eq!(
        LongFrame::parse(&captured(1)[..7]),
        Err(FrameError::WrongLength(7))
    );
}

#[test]
fn request_rejects_function_wider_than_four_bits() {
    assert_eq!(
        LongFrame::request(0x01, 0x0B, 0x10, &[]),
        Err(FrameError::FunctionOutOfRange(0x10))
    );
}
