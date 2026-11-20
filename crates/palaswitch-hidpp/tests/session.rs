mod common;

use std::time::Duration;

use common::{ScriptedTransport, exchanges, line_bytes};
use palaswitch_hidpp::features::{OnboardProfiles, Profile, device_name};
use palaswitch_hidpp::{Config, DeviceError, Error, Session};

const ERRORS: &str = include_str!("fixtures/g502x-plus-lightspeed-errors.txt");
const FOREIGN: &str = include_str!("fixtures/g502x-plus-lightspeed-foreign.txt");
const PROFILES: &str = include_str!("fixtures/g502x-plus-lightspeed-profiles.txt");
const READ: &str = include_str!("fixtures/g502x-plus-lightspeed-read.txt");
const SWITCH: &str = include_str!("fixtures/g502x-plus-lightspeed-switch.txt");

fn config() -> Config {
    Config {
        timeout: Duration::from_millis(10),
        attempts: 3,
        retry_delay: Duration::ZERO,
    }
}

fn session(script: Vec<(Vec<u8>, Vec<Vec<u8>>)>) -> Session<ScriptedTransport> {
    Session::new(ScriptedTransport::new(script), 0x01, config())
}

/// getFeature(0x8100) answered with index 0x0B.
fn onboard_lookup() -> (Vec<u8>, Vec<Vec<u8>>) {
    exchanges(READ).swap_remove(0)
}

#[test]
fn request_skips_response_meant_for_another_program() {
    let (request, own) = exchanges(READ).swap_remove(3);
    let foreign = line_bytes(FOREIGN, 1);
    let mut session = session(vec![(request, [vec![foreign], own].concat())]);

    let response = session.request(0x0B, 4, &[]).expect("response");

    assert_eq!(response.params()[..2], [0x00, 0x01]);
}

#[test]
fn request_fails_with_receiver_error_without_retrying() {
    let (request, replies) = exchanges(ERRORS).swap_remove(0);
    let mut session = Session::new(
        ScriptedTransport::new(vec![(request, replies)]),
        0x02,
        config(),
    );

    assert_eq!(
        session.feature_index(0x8100),
        Err(Error::Device(DeviceError::Hidpp10(0x08)))
    );
}

#[test]
fn request_times_out_when_nothing_answers() {
    let (request, _) = exchanges(READ).swap_remove(3);
    let mut session = session(vec![(request, Vec::new())]);

    assert_eq!(session.request(0x0B, 4, &[]), Err(Error::Timeout));
}

#[test]
fn late_answer_to_an_earlier_request_is_drained_before_writing() {
    // The answer to a getInfo that timed out is still waiting when
    // getCurrentProfile is sent; it must not be read as the new answer.
    let stale = line_bytes(READ, 3);
    let (request, own) = exchanges(READ).swap_remove(3);
    let transport = ScriptedTransport::new(vec![(request, own)]).with_pending(vec![stale]);
    let mut session = Session::new(transport, 0x01, config());

    let response = session.request(0x0B, 4, &[]).expect("response");

    assert_eq!(response.function(), 4);
}

#[test]
fn feature_index_is_cached_after_first_lookup() {
    let mut session = session(vec![onboard_lookup()]);
    session.feature_index(0x8100).expect("first lookup");

    assert_eq!(session.feature_index(0x8100), Ok(0x0B));
}

#[test]
fn ping_returns_protocol_version() {
    let mut session = session(vec![exchanges(PROFILES).swap_remove(1)]);

    assert_eq!(session.ping(), Ok((4, 2)));
}

#[test]
fn device_name_is_read_from_the_device() {
    let script = exchanges(PROFILES).into_iter().take(1).collect::<Vec<_>>();
    let script = [script, exchanges(PROFILES)[2..4].to_vec()].concat();
    let mut session = session(script);

    assert_eq!(device_name(&mut session).as_deref(), Ok("G502 X PLUS"));
}

#[test]
fn profiles_list_enabled_and_disabled_slots() {
    let read = exchanges(READ);
    let profiles_capture = exchanges(PROFILES);
    let script = vec![
        read[0].clone(),
        read[1].clone(),
        profiles_capture[4].clone(),
        profiles_capture[5].clone(),
    ];
    let mut session = session(script);
    let onboard = OnboardProfiles::find(&mut session).expect("feature");
    let info = onboard.info(&mut session).expect("info");

    let enabled: Vec<u16> = onboard
        .profiles(&mut session, info)
        .expect("profiles")
        .iter()
        .filter(|profile| profile.enabled)
        .map(Profile::number)
        .collect();

    assert_eq!((info.profile_count, enabled), (5, vec![1, 2, 3]));
}

#[test]
fn switch_to_profile_two_and_back_succeeds_despite_notification() {
    let mut session = session([vec![onboard_lookup()], exchanges(SWITCH)].concat());
    let onboard = OnboardProfiles::find(&mut session).expect("feature");

    let results = [
        onboard.switch_to(&mut session, 0x0002),
        onboard.switch_to(&mut session, 0x0001),
    ];

    assert_eq!(results, [Ok(()), Ok(())]);
}

#[test]
fn notification_received_during_a_switch_is_kept() {
    let mut session = session([vec![onboard_lookup()], exchanges(SWITCH)[..3].to_vec()].concat());
    let onboard = OnboardProfiles::find(&mut session).expect("feature");
    onboard.switch_to(&mut session, 0x0002).expect("switch");

    let notifications = session.take_notifications();

    assert_eq!(
        notifications
            .iter()
            .map(|frame| (frame.feature_index(), frame.function()))
            .collect::<Vec<_>>(),
        vec![(0x09, 1)]
    );
}

#[test]
fn switch_reports_profile_the_device_kept() {
    // The device acknowledges setCurrentProfile(2) but still reports profile 1.
    let switch = exchanges(SWITCH);
    let script = vec![
        onboard_lookup(),
        switch[0].clone(),
        switch[1].clone(),
        switch[5].clone(),
    ];
    let mut session = session(script);
    let onboard = OnboardProfiles::find(&mut session).expect("feature");

    assert_eq!(
        onboard.switch_to(&mut session, 0x0002),
        Err(Error::ProfileNotApplied {
            requested: 0x0002,
            active: 0x0001
        })
    );
}
