mod common;

use std::time::Duration;

use common::{ScriptedTransport, exchanges, line_bytes};
use palaswitch_hidpp::features::{OnboardProfiles, device_name};
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
    // Assembled from two captures: the getCurrentProfile answer sent to
    // software ID 0x0D (profile 1, from the foreign capture) arrives before
    // ours (profile 2, from the switch capture).
    let (request, _) = exchanges(READ).swap_remove(3);
    let foreign = line_bytes(FOREIGN, 1);
    let own = line_bytes(SWITCH, 6);
    let mut session = session(vec![(request, vec![foreign, own])]);

    let response = session.request(0x0B, 4, &[]).expect("response");

    assert_eq!(response.params()[..2], [0x00, 0x02]);
}

#[test]
fn response_meant_for_another_program_is_not_kept_as_notification() {
    // Assembled: the getInfo answer sent to software ID 0x0D has function 0,
    // like a profile change notification, and arrives before our own answer.
    let (request, own) = exchanges(READ).swap_remove(1);
    let foreign = line_bytes(FOREIGN, 3);
    let mut session = session(vec![(request, [vec![foreign], own].concat())]);
    session.request(0x0B, 0, &[]).expect("response");

    assert!(session.take_notifications().is_empty());
}

#[test]
fn notification_from_another_device_index_is_not_kept() {
    // Assembled: a session for index 2 sees the notification index 1 sent
    // in the switch capture, then the receiver's error for index 2.
    let notification = line_bytes(SWITCH, 4);
    let (request, replies) = exchanges(ERRORS).swap_remove(0);
    let transport =
        ScriptedTransport::new(vec![(request, replies)]).with_pending(vec![notification]);
    let mut session = Session::new(transport, 0x02, config());
    let _ = session.feature_index(0x8100);

    assert!(session.take_notifications().is_empty());
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
    // Assembled from two captures to stand for a late answer: the
    // getCurrentProfile answer saying profile 2 (switch capture) is already
    // waiting when getCurrentProfile is sent; the answer to this request says
    // profile 1 (read capture).
    let stale = line_bytes(SWITCH, 6);
    let (request, own) = exchanges(READ).swap_remove(3);
    let transport = ScriptedTransport::new(vec![(request, own)]).with_pending(vec![stale]);
    let mut session = Session::new(transport, 0x01, config());

    let response = session.request(0x0B, 4, &[]).expect("response");

    assert_eq!(response.params()[..2], [0x00, 0x01]);
}

#[test]
fn notification_arriving_before_the_response_is_kept() {
    // Assembled: the notification of the switch capture inserted before the
    // getCurrentProfile answer of the read capture.
    let (request, own) = exchanges(READ).swap_remove(3);
    let notification = line_bytes(SWITCH, 4);
    let mut session = session(vec![(request, [vec![notification], own].concat())]);
    session.request(0x0B, 4, &[]).expect("response");

    let notifications = session.take_notifications();

    assert_eq!(notifications.len(), 1);
}

#[test]
fn switched_off_mouse_fails_at_once_without_retrying() {
    let (request, replies) = exchanges(ERRORS).swap_remove(4);
    let mut session = session(vec![(request, replies)]);

    let result = session.ping();

    assert_eq!(
        (result, session.into_transport().writes),
        (Err(Error::Device(DeviceError::Hidpp10(0x08))), 1)
    );
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

    let slots: Vec<(u16, bool)> = onboard
        .profiles(&mut session, info)
        .expect("profiles")
        .iter()
        .map(|profile| (profile.number(), profile.enabled))
        .collect();

    assert_eq!(
        slots,
        vec![(1, true), (2, true), (3, true), (4, false), (5, false)]
    );
}

#[test]
fn empty_user_directory_falls_back_to_factory_profiles() {
    let read = exchanges(READ);
    let profiles_capture = exchanges(PROFILES);
    // Assembled: an unwritten memory area reads as sixteen 0xFF bytes, so a
    // directory that was never written starts with the end marker 0xFFFF.
    // This mouse has a user directory; the reply is the answer the same
    // function gave for an unwritten area (profile 1, offset 0xA0).
    let empty = profiles_capture[6].1.clone();
    let script = vec![
        read[0].clone(),
        read[1].clone(),
        (profiles_capture[4].0.clone(), empty),
        profiles_capture[11].clone(),
    ];
    let mut session = session(script);
    let onboard = OnboardProfiles::find(&mut session).expect("feature");
    let info = onboard.info(&mut session).expect("info");

    let slots: Vec<(u16, bool)> = onboard
        .profiles(&mut session, info)
        .expect("profiles")
        .iter()
        .map(|profile| (profile.sector, profile.is_rom()))
        .collect();

    assert_eq!(slots, vec![(0x0101, true), (0x0102, true)]);
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
    // Assembled to stand for a device that keeps its profile: the answers to
    // setOnboardMode and setCurrentProfile(2), then the getCurrentProfile
    // answer saying profile 1, taken from the switch back to profile 1.
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
