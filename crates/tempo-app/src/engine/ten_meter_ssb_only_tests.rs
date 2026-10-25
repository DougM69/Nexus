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
use tempo_net::tci::server::{KeyKind, Mic, Refusal};
use tempo_net::tci::wire::Word;

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

/// The TCI program the outside-key tests key from.
const PROGRAM: u64 = 1;

/// How a program outside Nexus keys the transmitter.
#[derive(Debug, Clone, Copy)]
enum Outside {
    /// A CAT broker client's `T 1`.
    Broker,
    /// A TCI program's key of the radio's microphone.
    TciMic,
    /// A TCI program's over of its own audio.
    TciOver,
}

/// What the radio reads back over CAT, `None` for nothing read yet, and whether it is plain USB
/// or LSB. Written out, not asked of the gate's own rule.
const READ_BACK: [(Option<&str>, bool); 10] = [
    (Some("USB"), true),
    (Some("LSB"), true),
    (Some("usb"), true),
    (Some("PKTUSB"), false),
    (Some("PKTLSB"), false),
    (Some("DATA-U"), false),
    (Some("USB-D"), false),
    (Some("DIGU"), false),
    (Some("FM"), false),
    (None, false),
];

/// A station as `class` on 10 m Phone at `dial`, picked to the sideband the radio reads back
/// (`rig`), armed, with the CAT broker's and the TCI server's transmit switches on and the
/// program's last `modulation:` that sideband.
fn outside_at(class: &str, dial: f64, rig: Option<&str>) -> Engine {
    let lsb = rig.is_some_and(|w| w.to_ascii_uppercase().contains("LSB") || w == "DATA-L");
    let side = if lsb { "LSB" } else { "USB" };
    let mut e = at(class, "phone", dial, Some(side));
    e.settings.cat_broker_ptt = true;
    e.settings.tci_server_tx = true;
    e.set_tx_enabled(true);
    e.tci_note_modulation(
        PROGRAM,
        &Word::new(&side.to_ascii_lowercase()).expect("a word"),
    );
    if let Some(word) = rig {
        e.observe_rig_mode(word.to_string());
    }
    e
}

/// `how`'s key, and whether it keys the radio: granted, and (for a held key) read as keyed. A
/// broker key granted or latched but not read as keyed comes back as `Owner`, which no case wants.
fn outside_key(e: &mut Engine, how: Outside) -> Result<(), Option<Refusal>> {
    match how {
        // Granted, latched and read as keyed alike: the arbiter refuses it, and the mask on read
        // refuses it too.
        Outside::Broker => {
            let granted = e.broker_ptt(true);
            if granted && e.broker_ptt_active() && e.manual_ptt() {
                Ok(())
            } else if granted || e.broker_ptt_active() {
                Err(Some(Refusal::Owner))
            } else {
                Err(None)
            }
        }
        Outside::TciMic => {
            e.tci_key(PROGRAM, KeyKind::Mic(Mic::Selected), false)
                .map_err(Some)?;
            if e.manual_ptt() {
                Ok(())
            } else {
                Err(None)
            }
        }
        Outside::TciOver => e.tci_key(PROGRAM, KeyKind::Tci, true).map_err(Some),
    }
}

/// ⛔ ON 28.3–28.5 MHz A NOVICE OR A TECHNICIAN MAY BE KEYED BY ANOTHER PROGRAM ONLY IN PLAIN USB OR
/// LSB. Nexus cannot hear what a program sends; in a DATA submode it is soundcard audio, a picture
/// as easily as voice, and 97.307(f)(10) allows only CW and J3E/R3E voice there. So the CAT
/// broker's key, a TCI program's microphone key and its own over are refused there unless the
/// radio reads back plain USB or LSB, and refused when it has read back nothing. Edge by edge,
/// every US class and `Open`, every read-back word, all three keys; General, Advanced, Extra and
/// `Open` are unchanged, and nothing changes off the segment. CONTROLS: a plain-sideband key
/// there is granted, a DATA one off it is granted, and a DATA one for a General on it is granted.
#[test]
fn an_outside_key_on_ten_meter_phone_needs_plain_sideband_for_novice_and_technician() {
    let (mut wrong, mut held_granted, mut held_refused, mut data_granted) = (Vec::new(), 0, 0, 0);
    for class in crate::privileges::US_CLASSES
        .into_iter()
        .chain([crate::settings::LicenseClass::Open])
    {
        let token = format!("{class:?}").to_ascii_lowercase();
        // By hand: the class this rule binds.
        let bound = matches!(token.as_str(), "novice" | "technician");
        for dial in DIALS {
            let held = bound && (28.300..28.500).contains(&dial);
            for (rig, plain) in READ_BACK {
                for how in [Outside::Broker, Outside::TciMic, Outside::TciOver] {
                    let mut e = outside_at(&token, dial, rig);
                    // The licence for that sideband's voice, as Nexus's own PTT judges it.
                    let voice = e.tx_allowed();
                    let want = voice && (plain || !held);
                    let got = outside_key(&mut e, how);
                    if got.is_ok() != want || got == Err(Some(Refusal::Owner)) {
                        wrong.push(format!("{token} {how:?} at {dial} rig {rig:?}: {got:?}"));
                    }
                    // A TCI refusal names this rule, where the licence passed.
                    if voice
                        && !want
                        && !matches!(how, Outside::Broker)
                        && got != Err(Some(Refusal::VoiceOnly))
                    {
                        wrong.push(format!(
                            "{token} {how:?} at {dial} rig {rig:?}: refused as {got:?}"
                        ));
                    }
                    if voice && held {
                        if want {
                            held_granted += 1;
                        } else {
                            held_refused += 1;
                        }
                    }
                    if want && !plain && !held {
                        data_granted += 1;
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "outside keys on 10 m: {wrong:#?}");
    assert!(
        held_granted >= 12 && held_refused >= 30 && data_granted >= 100,
        "the sweep must reach every verdict: {held_granted}/{held_refused}/{data_granted}"
    );
}

/// A data word Nexus itself commands counts as the radio's: an SSTV receiver holding the DATA
/// submode puts the radio in PKTUSB although it reads back USB from before. Refused for a
/// Technician at 28.400; CONTROL: the same station with the hold off keys.
#[test]
fn an_outside_key_is_refused_while_nexus_commands_a_data_word() {
    for hold in [true, false] {
        let mut e = outside_at("technician", 28.400, Some("USB"));
        e.settings.sstv_hold_data_submode = hold;
        e.sstv_armed = hold;
        assert_eq!(
            e.rig_mode_effective() == "PKTUSB",
            hold,
            "precondition: the commanded word"
        );
        assert_eq!(e.broker_ptt(true), !hold, "hold {hold}");
    }
}

/// ⛔ A KEY ALREADY HELD DROPS WHEN THE RADIO LEAVES PLAIN SIDEBAND, and its release is never
/// refused. The broker's and a TCI program's microphone key are masked on read
/// ([`Engine::manual_ptt`]); the program's microphone key ends on the radio loop's next tick, and
/// so does its over. CONTROL: back in plain USB the broker's held key reads keyed again, as every
/// mask on it does.
#[test]
fn an_outside_key_held_drops_when_the_radio_goes_to_data_and_always_unkeys() {
    let mut e = outside_at("technician", 28.400, Some("USB"));
    assert!(
        e.broker_ptt(true) && e.manual_ptt(),
        "precondition: keyed in USB"
    );
    e.observe_rig_mode("PKTUSB".into());
    assert!(!e.manual_ptt(), "the broker's key drops in PKTUSB");
    assert!(!e.broker_ptt(true), "and a re-assert is refused");
    e.observe_rig_mode("USB".into());
    assert!(e.manual_ptt(), "control: plain USB again");
    e.observe_rig_mode("PKTUSB".into());
    assert!(e.broker_ptt(false), "the release is never refused");
    assert!(!e.broker_ptt_active());

    let mut e = outside_at("technician", 28.400, Some("USB"));
    assert_eq!(
        e.tci_key(PROGRAM, KeyKind::Mic(Mic::Selected), false),
        Ok(())
    );
    assert!(e.manual_ptt(), "precondition: the microphone keyed");
    e.observe_rig_mode("DATA-U".into());
    assert!(!e.manual_ptt(), "the program's microphone key drops");
    let _ = e.poll_tci(std::time::Instant::now(), 0.0, 1);
    e.observe_rig_mode("USB".into());
    assert!(!e.manual_ptt(), "and the tick ended it, not held for later");

    let mut e = outside_at("technician", 28.400, Some("USB"));
    assert_eq!(e.tci_key(PROGRAM, KeyKind::Tci, true), Ok(()));
    let now = std::time::Instant::now();
    assert_ne!(
        e.poll_tci(now, 0.0, 1),
        crate::mic::MicTick::Idle,
        "precondition: the over is armed"
    );
    e.observe_rig_mode("PKTUSB".into());
    let _ = e.poll_tci(now, 0.0, 1);
    assert_eq!(
        e.poll_tci(now, 0.0, 1),
        crate::mic::MicTick::Idle,
        "the over ends in PKTUSB"
    );
}

/// The broker's refusal line names the rule in plain words; the TCI server's says the same.
#[test]
fn the_outside_key_refusal_says_why_in_plain_words() {
    for text in [OUTSIDE_KEY_VOICE_ONLY, Refusal::VoiceOnly.text()] {
        for words in [
            "28.3–28.5 MHz",
            "Novice or Technician",
            "plain USB or LSB",
            "DATA",
        ] {
            assert!(text.contains(words), "{text:?} must say {words:?}");
        }
    }
}
