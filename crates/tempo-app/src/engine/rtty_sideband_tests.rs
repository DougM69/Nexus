//! AFSK RTTY on either sideband keeps the standard on-air sense: mark HIGH on RF, space 170 Hz
//! below it. On LSB the mark is the lower audio tone (RF = dial − audio); on USB the audio maps
//! to RF the other way up (RF = dial + audio), so the tones swap. Reverse inverts whichever sense
//! that is, for an upside-down station.

use super::*;

fn station(side: &str, reverse: bool) -> Engine {
    let mut e = Engine::new("K1ABC", "EN50", 0);
    e.set_license_class("extra");
    let mut s = e.settings().clone();
    s.rtty_afsk_sideband = side.into();
    s.rtty_reverse = reverse;
    e.apply_settings(s);
    e.set_operating_mode("rtty", false);
    e
}

/// (mark, space) on RF, in Hz from the dial: the tone pair both TX sites in the radio loop build
/// (`tone_pair(rtty_center_hz, rtty_shift_hz, rtty_reverse)`), placed on the side of the dial the
/// commanded mode puts the audio.
fn emitted(e: &Engine) -> (f32, f32) {
    let (mark, space) = tempo_core::rtty::tone_pair(
        e.rtty_center_hz(),
        e.rtty_shift_hz() as f32,
        e.rtty_reverse(),
    );
    match e.rig_mode_effective().as_str() {
        "PKTUSB" => (mark, space),
        "PKTLSB" => (-mark, -space),
        other => panic!("AFSK RTTY commanded {other}"),
    }
}

#[test]
fn afsk_puts_mark_high_on_rf_on_either_sideband() {
    for (side, mode, rf) in [
        ("lsb", "PKTLSB", (-2125.0, -2295.0)),
        ("usb", "PKTUSB", (2295.0, 2125.0)),
    ] {
        let e = station(side, false);
        assert_eq!(e.rig_mode_effective(), mode, "{side}");
        assert_eq!(emitted(&e), rf, "{side}: (mark, space) on RF from the dial");
        assert!(rf.0 > rf.1, "{side}: mark is the higher RF");
        // The waterfall cursors are the same pair, and the privilege gate judges the same side.
        let st = e.rtty_state();
        assert_eq!(
            (st.mark_hz, st.space_hz),
            (rf.0.abs(), rf.1.abs()),
            "{side} cursors"
        );
        assert_eq!(
            e.rtty_afsk_usb_side(),
            Some(side == "usb"),
            "{side} gate side"
        );
    }
}

/// LSB + Reverse sends the audio pair USB sends, and USB + Reverse sends LSB's: Reverse inverts
/// the on-air sense on both sides, so mark lands LOW on RF.
#[test]
fn reverse_composes_with_the_sideband() {
    let audio = |e: &Engine| {
        tempo_core::rtty::tone_pair(
            e.rtty_center_hz(),
            e.rtty_shift_hz() as f32,
            e.rtty_reverse(),
        )
    };
    assert_eq!(audio(&station("lsb", true)), audio(&station("usb", false)));
    assert_eq!(audio(&station("usb", true)), audio(&station("lsb", false)));
    assert_eq!(audio(&station("lsb", true)), (2295.0, 2125.0));
    assert_eq!(audio(&station("usb", true)), (2125.0, 2295.0));
    for side in ["lsb", "usb"] {
        let (mark, space) = emitted(&station(side, true));
        assert!(
            mark < space,
            "{side} + Reverse: mark LOW on RF, got {mark} vs {space}"
        );
    }
}

/// True FSK ignores the AFSK sideband: the rig's own RTTY mode, and the operator's Reverse alone.
#[test]
fn fsk_ignores_the_afsk_sideband() {
    for side in ["lsb", "usb"] {
        let mut e = station(side, false);
        let mut s = e.settings().clone();
        s.rtty_backend = "fsk".into();
        e.apply_settings(s);
        assert_eq!(e.rig_mode_effective(), "RTTY", "{side}");
        assert!(!e.rtty_reverse(), "{side}");
    }
}
