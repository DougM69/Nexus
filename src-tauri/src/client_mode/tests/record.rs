//! The voice keyer's recorder and the held press, which share this computer's microphone.
use super::*;

/// ⛔ Record is refused while a press is held, as the PTT is refused while recording: the recorder
/// and the over would take turns at the microphone's one ring, each getting part of the voice. It
/// is refused by name (`pressHeld`), and once the PTT is let go, Record starts. CONTROL: the press
/// went first, and its hold reached the road.
#[test]
fn a_recording_is_refused_while_a_press_is_held() {
    use crate::lan_client::audio_road::StandIn;
    let (_shack, end, client, s, dir) = present_for(Duration::from_secs(5));
    client.record_from(StandIn::new());
    assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
    eventually("the press's first hold", || end.presses().len() == 1);
    let recording = ask(&s.main, "start_voice_recording", json!({}));
    assert!(ask(&s.main, "set_ptt", json!({"on": false})).is_ok());
    let after = ask(&s.main, "start_voice_recording", json!({}));
    let _ = ask(&s.main, "cancel_voice_recording", json!({}));
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        recording,
        refused("pressHeld"),
        "Record started while a press was held"
    );
    assert!(after.is_ok(), "Record once the PTT was let go: {after:?}");
}
