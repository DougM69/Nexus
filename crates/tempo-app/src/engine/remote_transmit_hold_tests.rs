//! ⛔ A SIGN-IN AUTO-GRANTED DEVICE HOLDS TRANSMIT (the 2026-10-23 sign-in decision). A device the
//! owner's account signed in and was auto-granted control to may connect, operate the receiver and
//! hold station control, but no transmit ARM starts while the service holds it — through every arm
//! path. The hold itself keys, unkeys and disarms nothing: Stop and the off half of the TX-enable
//! latch are taken at once, held or not. The service decides the hold FAIL-CLOSED (a sign-in
//! controller is held unless positively waived) and sets it here
//! ([`Engine::set_remote_transmit_held`]); these drive the engine's own gates against the flag.
//!
//! This is ADDITIVE to the clock-repair hold ([`super::clock_repair_hold_tests`]) — never a
//! weakening of any transmit guard. Every start is tried twice, held and then not, so a refusal
//! here is the hold's and not some other gate's: the second try is the control.

use super::*;

/// An FT8 station with its identity set, TX off.
fn ft8() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e
}

/// A station in `section`, armed as entering a manual section arms it.
fn station_in(section: &str) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_operating_mode(section, false);
    assert!(e.tx_enabled(), "premise: entering {section} arms TX");
    e
}

/// ★ TX On is refused while the device is held, and arms once the hold is lifted.
#[test]
fn tx_on_is_refused_while_held_and_arms_after() {
    let mut e = ft8();
    assert!(!e.tx_enabled(), "premise: TX off");
    e.set_remote_transmit_held(true);
    e.set_tx_enabled(true);
    assert!(!e.tx_enabled(), "TX On while the sign-in hold stands");

    e.set_remote_transmit_held(false);
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "control: TX On once the hold is lifted");
}

/// ★ Entering a manual section does not arm its latch while held, and arms once the hold is off:
/// FT, Phone, CW and the keyboard modes all arm through `set_tx_enabled`, which this refuses.
#[test]
fn a_manual_section_does_not_arm_while_held() {
    for section in ["phone", "cw", "rtty", "keyboard"] {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_remote_transmit_held(true);
        e.set_operating_mode(section, false);
        assert!(!e.tx_enabled(), "{section}: the latch stays off while held");

        e.set_remote_transmit_held(false);
        e.set_operating_mode("qso-monitor", false);
        e.set_operating_mode(section, false);
        assert!(
            e.tx_enabled(),
            "control: {section} arms once the hold is off"
        );
    }
}

/// ★ An FT8 run cannot be started while held (Call CQ funnels through `set_tx_enabled`), and starts
/// once the hold is off.
#[test]
fn an_ft8_run_cannot_start_while_held() {
    let mut e = ft8();
    e.set_remote_transmit_held(true);
    e.start_cq(None).unwrap();
    assert!(!e.tx_enabled(), "no FT8 run arms while held");

    e.set_remote_transmit_held(false);
    e.start_cq(None).unwrap();
    assert!(e.tx_enabled(), "control: the run arms once the hold is off");
}

/// ★ Tune is refused while held — the one keying path that bypasses the TX-enable latch.
#[test]
fn tune_is_refused_while_held() {
    let mut e = ft8();
    e.set_remote_transmit_held(true);
    e.set_tune(true);
    assert!(!e.tuning(), "Tune while the sign-in hold stands");

    e.set_remote_transmit_held(false);
    e.set_tune(true);
    assert!(e.tuning(), "control: Tune once the hold is off");
}

/// ★ A CW send self-arms the latch, so it is refused by the hold's own gate in `send_cw`, queues
/// nothing, and leaves the latch off; it goes once the hold is off.
#[test]
fn a_cw_send_is_refused_while_held_and_arms_nothing() {
    let mut e = station_in("cw");
    e.set_tx_enabled(false);
    e.set_remote_transmit_held(true);
    e.send_cw("CQ TEST");
    assert_eq!(e.tx_owner(), None, "nothing queued");
    assert!(!e.tx_enabled(), "nothing armed");
    assert_eq!(e.cw_keyer_error.as_deref(), Some(REMOTE_HOLD_DROPPED));

    e.set_remote_transmit_held(false);
    e.send_cw("CQ TEST");
    assert_eq!(
        e.tx_owner(),
        Some(TxOwner::Cw),
        "control: it goes once the hold is off"
    );
}

/// ★ Voice-memory playback keys the rig without the latch, so the hold refuses the request and
/// queues nothing; it is accepted once the hold is off.
#[test]
fn voice_memory_is_refused_while_held() {
    let mut e = station_in("phone");
    e.set_remote_transmit_held(true);
    assert!(!e.request_voice_mem(1), "voice memory while held");
    assert_eq!(e.take_voice_mem(), None, "nothing queued for the radio");

    e.set_remote_transmit_held(false);
    assert!(
        e.request_voice_mem(1),
        "control: accepted once the hold is off"
    );
    assert_eq!(e.take_voice_mem(), Some(VoiceMemCmd::Play(1)));
}

/// ★ STOP IS NEVER HELD. Inside the window, an armed station's Stop (`halt_tx`) disarms at once,
/// exactly as it does with no hold. The hold refuses a start; it never refuses a stop.
#[test]
fn stop_is_taken_at_once_while_held() {
    let mut e = station_in("phone");
    assert!(e.tx_enabled(), "premise: armed");
    e.set_remote_transmit_held(true);
    e.halt_tx();
    assert!(!e.tx_enabled(), "Stop disarms the latch while held");
    assert_eq!(e.tx_owner(), None, "and nothing is keyed");
    assert!(e.remote_transmit_held(), "the hold itself still stands");
}

/// ★ THE OFF HALF OF THE SWITCH IS NEVER HELD. `set_tx_enabled(false)` disarms at once while held;
/// only an arm (`true`) is refused.
#[test]
fn disarming_is_honoured_while_held() {
    let mut e = station_in("phone");
    assert!(e.tx_enabled(), "premise: armed");
    e.set_remote_transmit_held(true);
    e.set_tx_enabled(false);
    assert!(!e.tx_enabled(), "TX Off is honoured while held");
}
