//! On 28.3–28.5 MHz a Novice and a Technician may send only CW and single-sideband voice.
//!
//! "A station having a control operator holding a Novice Class operator license, a Technician
//! Class operator license, or a Technician Plus Class operator license may only transmit a CW
//! emission using the international Morse code or phone emissions J3E and R3E" (47 CFR
//! 97.307(f)(10), the 28.3–28.5 MHz segment). So there, for those two classes, AM and FM are
//! refused (`privileges::wide_phone_allowed`), and so is an SSTV picture, which is an image
//! (J3F) and not voice, though it rides the same USB passband (`Engine::sstv_tx_gate`,
//! `Engine::poll_sstv_tx`). General, Advanced and Extra keep every 10 m phone emission.
//!
//! The table is judged edge by edge, a hair either side of each end of the segment, for every
//! class and every emission the Phone and CW sections key; the refusals' words are pinned after
//! it, because "outside your license privileges" is wrong where the frequency is fine and the
//! emission is not.

use super::tx_gate_table::station;
use super::*;

const SSTV_TEN_METER: &str = "SSTV not sent: on 28.3–28.5 MHz a Novice or Technician license \
     allows only CW and single-sideband voice, not pictures.";
const SSTV_TEN_METER_DROPPED: &str = "SSTV not sent: on 28.3–28.5 MHz a Novice or Technician \
     license allows only CW and single-sideband voice, not pictures, so the picture that was \
     waiting was dropped, not held for later.";
const PHONE_TEN_METER: &str = "TX locked — on 28.3–28.5 MHz a Novice or Technician license \
     allows only CW and single-sideband voice. Pick USB, LSB or AUTO.";
const FREQUENCY: &str = "TX locked — this frequency is outside your license privileges";

/// The dials: a hair under the segment, its floor, its middle, its top and a hair over it.
const DIALS: [f64; 5] = [28.299_999, 28.300, 28.400, 28.500, 28.500_001];

/// What a class may key at each of [`DIALS`], per emission, as `T`/`F` per dial.
struct Row {
    cw: &'static str,
    usb: &'static str,
    lsb: &'static str,
    am: &'static str,
    fm: &'static str,
    sstv: &'static str,
}

/// Novice and Technician: CW 28.0–28.5, phone 28.3–28.5 in single sideband only, no pictures.
/// A USB passband from a hair under 28.3 starts outside the segment, an LSB one from 28.3 ends
/// below it, and every phone emission keyed from 28.5 or above is outside it.
const NOVICE_TECHNICIAN: Row = Row {
    cw: "TTTFF",
    usb: "FTTFF",
    lsb: "FFTFF",
    am: "FFFFF",
    fm: "FFFFF",
    sstv: "FFFFF",
};

/// General, Advanced and Extra: CW to 29.7, phone 28.3–29.7 in every emission. AM from 28.3
/// puts its lower sideband under the segment, as LSB does; FM is judged at its carrier and the
/// band's USB passband.
const GENERAL_UP: Row = Row {
    cw: "TTTTT",
    usb: "FTTTT",
    lsb: "FFTTT",
    am: "FFTTT",
    fm: "FTTTT",
    sstv: "FTTTT",
};

/// A station as `class` on 10 m at `dial` in `section`, with the Phone cockpit's pick `pick`
/// (`None` = AUTO), tuned first and picked after, as the cockpit does it. An IC-7300.
fn at(class: &str, section: &str, dial: f64, pick: Option<&str>) -> Engine {
    let mut e = station(3073, class, section, dial, "10m", "USB");
    if section == "phone" {
        e.request_sideband_override(pick);
        assert_eq!(
            e.sideband_override().as_deref(),
            pick,
            "precondition: the pick"
        );
    }
    assert!(
        (e.settings.dial_mhz - dial).abs() < 1e-9,
        "precondition: the tune landed on {dial}"
    );
    e
}

#[test]
fn ten_meter_phone_and_pictures_edge_by_edge_for_every_class() {
    let mut wrong = Vec::new();
    for (class, row) in [
        ("novice", NOVICE_TECHNICIAN),
        ("technician", NOVICE_TECHNICIAN),
        ("general", GENERAL_UP),
        ("advanced", GENERAL_UP),
        ("extra", GENERAL_UP),
    ] {
        for (i, dial) in DIALS.into_iter().enumerate() {
            let want = |cells: &str| cells.as_bytes()[i] == b'T';
            let mut got = vec![
                ("CW", want(row.cw), at(class, "cw", dial, None).tx_allowed()),
                (
                    "SSTV",
                    want(row.sstv),
                    at(class, "phone", dial, Some("USB")).sstv_tx_gate().is_ok(),
                ),
            ];
            for (pick, cells) in [
                ("USB", row.usb),
                ("LSB", row.lsb),
                ("AM", row.am),
                ("FM", row.fm),
            ] {
                let e = at(class, "phone", dial, Some(pick));
                got.push((pick, want(cells), e.tx_allowed()));
            }
            for (emission, want, got) in got {
                if want != got {
                    wrong.push(format!(
                        "{class} {emission} at {dial}: want {want}, got {got}"
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "the 10 m table: {wrong:#?}");
}

#[test]
fn the_refusals_name_the_ten_meter_rule_where_the_frequency_is_not_the_reason() {
    let said = |r: Result<(), String>| r.err();
    let mut wrong = Vec::new();
    for class in ["novice", "technician"] {
        // A picture under a USB pick, where USB voice keys: the rule is the reason.
        for dial in [28.300, 28.400, 28.497] {
            let e = at(class, "phone", dial, Some("USB"));
            assert!(e.tx_allowed(), "precondition: USB voice keys at {dial}");
            let got = said(e.sstv_tx_gate());
            if got.as_deref() != Some(SSTV_TEN_METER) {
                wrong.push(format!("{class} SSTV at {dial}: {got:?}"));
            }
        }
        // AM and FM where either sideband keys: the rule, from the voice keyer and an SSTV send.
        for dial in [28.303, 28.400, 28.497] {
            for pick in ["AM", "FM"] {
                let mut e = at(class, "phone", dial, Some(pick));
                let voice = said(e.send_voice(vec![0.0; 1_200]));
                let sstv = said(e.sstv_tx_gate());
                for (path, got) in [("voice keyer", voice), ("SSTV send", sstv)] {
                    if got.as_deref() != Some(PHONE_TEN_METER) {
                        wrong.push(format!("{class} {pick} at {dial} {path}: {got:?}"));
                    }
                }
            }
        }
        // Off the segment the frequency is the reason, whatever the emission.
        for (dial, pick) in [(28.299_999, "USB"), (28.500, "USB"), (28.300, "AM")] {
            let mut e = at(class, "phone", dial, Some(pick));
            let voice = said(e.send_voice(vec![0.0; 1_200]));
            let sstv = said(e.sstv_tx_gate());
            for (path, got) in [("voice keyer", voice), ("SSTV send", sstv)] {
                if got.as_deref() != Some(FREQUENCY) {
                    wrong.push(format!("{class} {pick} at {dial} {path}: {got:?}"));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "the refusal's words: {wrong:#?}");
}

#[test]
fn a_waiting_picture_the_ten_meter_rule_refuses_is_dropped_and_says_why() {
    // Queued as a General at 28.400, then the class becomes Technician while it waits.
    let mut e = at("general", "phone", 28.400, Some("USB"));
    e.sstv_send(vec![0.0; 24_000], "PD-120".into())
        .expect("precondition: a General sends a picture at 28.400");
    e.set_license_class("technician");
    assert!(e.poll_sstv_tx().is_none(), "nothing keys");
    assert!(!e.sstv_sending(), "dropped, not held");
    assert_eq!(e.sstv_tx_notice(), Some(SSTV_TEN_METER_DROPPED));
    // …and it is not held for later: back to General, nothing is waiting.
    e.set_license_class("general");
    assert!(e.poll_sstv_tx().is_none(), "dropped, not held for later");

    // The control: a General's picture at 28.400 keys.
    let mut e = at("general", "phone", 28.400, Some("USB"));
    e.sstv_send(vec![0.0; 24_000], "PD-120".into())
        .expect("precondition: a General sends a picture at 28.400");
    assert!(e.poll_sstv_tx().is_some(), "a General's picture keys");
}
