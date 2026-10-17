//! 60 m allows only USB phone, and the lock says so when the pick is all that stands in the way.
//!
//! Every US class may talk on 60 m in upper sideband only: LSB is refused there, and so are AM,
//! FM and every word that is not one sideband (`privileges::lsb_phone_allowed`,
//! `privileges::wide_phone_allowed`). Those verdicts are not this file's business and nothing here
//! moves one. What it pins is what the operator is TOLD:
//!
//! - `phoneUsbOnly` — the snapshot says the rule holds at the transmit carrier, so the Phone
//!   cockpit greys its LSB, AM and FM picks there;
//! - `phoneModeRefused` — the snapshot says the lock is the MODE alone, exactly when a USB pick
//!   on the same dial would key, so the cockpit names the rule, not the frequency;
//! - the words the engine's own refusals carry on the paths that key from the Phone section:
//!   the voice keyer, the radio's ATU, an SSTV send and an SSTV picture dropped while it waited.
//!
//! A frequency USB is refused on too keeps "this frequency is outside your license privileges".
//! The snapshot fields are read by their wire names, so a missing field reads as false.

use super::tx_gate_table::station;
use super::*;

const USB_ONLY: &str = "TX locked — 60 m allows only USB phone. Pick USB or AUTO.";
const FREQUENCY: &str = "TX locked — this frequency is outside your license privileges";
const SSTV_USB_ONLY: &str = "SSTV not sent: 60 m allows only USB phone, so the picture that \
     was waiting was dropped, not held for later. Pick USB or AUTO and send it again.";
const SSTV_FREQUENCY: &str = "SSTV not sent: this frequency is outside your license privileges, \
     so the picture that was waiting was dropped, not held for later. Send it again from inside \
     them.";

/// The four channel carriers (1.5 kHz below each centre) and a carrier in the 5351.5–5366.5 kHz
/// segment: USB keys on each for a General or an Extra.
const USB_KEYS: [f64; 5] = [5.3305, 5.3465, 5.3600, 5.3715, 5.4035];
/// 60 m dials whose USB passband is in no channel and not in the segment: refused for the
/// frequency, whatever the pick.
const OFF_CHANNEL: [f64; 2] = [5.3400, 5.3900];

/// A Phone station as `class` at `dial` on `band` with the cockpit's pick `pick` (`None` = AUTO),
/// built through the operator's verbs in the cockpit's order: the tune first, THEN the pick,
/// because a band change drops the pick. An IC-7300, which has XIT.
fn phone_at(class: &str, band: &str, dial: f64, pick: Option<&str>) -> Engine {
    let mut e = station(3073, class, "phone", dial, band, "USB");
    e.request_sideband_override(pick);
    assert!(
        (e.settings.dial_mhz - dial).abs() < 1e-9,
        "precondition: the tune landed on {dial}"
    );
    assert_eq!(
        e.sideband_override().as_deref(),
        pick,
        "precondition: the pick is in force"
    );
    e
}

/// What the cockpit reads: (`phoneUsbOnly`, `phoneModeRefused`).
fn told(e: &Engine) -> (bool, bool) {
    let radio = serde_json::to_value(e.snapshot().radio).expect("the snapshot serializes");
    let flag = |key: &str| {
        radio
            .get(key)
            .map(|v| v.as_bool().expect("a bool"))
            .unwrap_or(false)
    };
    (flag("phoneUsbOnly"), flag("phoneModeRefused"))
}

#[test]
fn a_pick_the_60_m_rule_refuses_is_named_where_usb_would_key() {
    let mut wrong = Vec::new();
    for class in ["general", "extra"] {
        for dial in USB_KEYS {
            // The control first: USB keys here, so the frequency is not the reason.
            let usb = phone_at(class, "60m", dial, Some("USB"));
            if !usb.tx_allowed() || told(&usb) != (true, false) {
                wrong.push(format!(
                    "{class} {dial:.4} USB: allowed={} told={:?}",
                    usb.tx_allowed(),
                    told(&usb)
                ));
            }
            for pick in ["LSB", "AM", "FM"] {
                let e = phone_at(class, "60m", dial, Some(pick));
                if e.tx_allowed() || told(&e) != (true, true) {
                    wrong.push(format!(
                        "{class} {dial:.4} {pick}: allowed={} told={:?}",
                        e.tx_allowed(),
                        told(&e)
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "60 m must say USB only exactly where USB keys: {wrong:#?}"
    );
}

#[test]
fn a_frequency_usb_is_refused_on_too_keeps_the_frequency_sentence() {
    let mut wrong = Vec::new();
    let picks = [None, Some("USB"), Some("LSB"), Some("AM"), Some("FM")];
    for class in ["general", "extra"] {
        for dial in OFF_CHANNEL {
            for pick in picks {
                let e = phone_at(class, "60m", dial, pick);
                if e.tx_allowed() || told(&e) != (true, false) {
                    wrong.push(format!(
                        "{class} {dial:.4} {pick:?}: allowed={} told={:?}",
                        e.tx_allowed(),
                        told(&e)
                    ));
                }
            }
        }
    }
    // A Technician has no 60 m phone at all, so USB is refused too: the frequency sentence.
    for dial in USB_KEYS {
        for pick in picks {
            let e = phone_at("technician", "60m", dial, pick);
            if e.tx_allowed() || told(&e) != (true, false) {
                wrong.push(format!(
                    "technician {dial:.4} {pick:?}: allowed={} told={:?}",
                    e.tx_allowed(),
                    told(&e)
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a refusal USB shares is the frequency's: {wrong:#?}"
    );
}

#[test]
fn the_rule_is_60_m_and_us_classes_only() {
    let mut wrong = Vec::new();
    // Open is trusted on 60 m, as the gate trusts it: nothing greyed, nothing locked.
    for pick in ["USB", "LSB", "AM", "FM"] {
        let e = phone_at("open", "60m", 5.3600, Some(pick));
        if !e.tx_allowed() || told(&e) != (false, false) {
            wrong.push(format!(
                "open 5.3600 {pick}: allowed={} told={:?}",
                e.tx_allowed(),
                told(&e)
            ));
        }
    }
    // Every other band keeps every pick: 80 m and 40 m LSB, the 20 m AM calling frequency, the
    // 10 m FM calling frequency.
    for (band, dial, pick) in [
        ("80m", 3.885, "LSB"),
        ("40m", 7.250, "LSB"),
        ("20m", 14.286, "AM"),
        ("10m", 29.600, "FM"),
    ] {
        for class in ["general", "extra"] {
            let e = phone_at(class, band, dial, Some(pick));
            if !e.tx_allowed() || told(&e) != (false, false) {
                wrong.push(format!(
                    "{class} {band} {dial:.4} {pick}: allowed={} told={:?}",
                    e.tx_allowed(),
                    told(&e)
                ));
            }
        }
    }
    // A pick refused for its side off 60 m is not this rule: LSB just above Extra's 20 m phone
    // edge puts its passband below 14.150, where USB would key. The frequency sentence stays.
    let e = phone_at("extra", "20m", 14.152, Some("LSB"));
    let usb = phone_at("extra", "20m", 14.152, Some("USB"));
    if e.tx_allowed() || !usb.tx_allowed() || told(&e) != (false, false) {
        wrong.push(format!(
            "extra 14.1520 LSB: allowed={} (USB {}) told={:?}",
            e.tx_allowed(),
            usb.tx_allowed(),
            told(&e)
        ));
    }
    assert!(wrong.is_empty(), "only 60 m and US classes: {wrong:#?}");
}

#[test]
fn the_rule_is_named_in_the_phone_section_only() {
    // CW on a 60 m channel carrier is refused: the rule puts the CW carrier on the channel
    // centre. USB phone would key on that dial, but CW is not phone, so the frequency sentence
    // stays.
    let mut e = station(3073, "general", "cw", 5.3305, "60m", "USB");
    assert!(
        !e.tx_allowed(),
        "precondition: CW off the channel centre is refused"
    );
    assert_eq!(told(&e), (true, false), "CW at 5.3305");
    e.observe_rig_tuner(Some(true), true);
    assert_eq!(
        e.atu_tune_gate().err().as_deref(),
        Some(FREQUENCY),
        "the ATU in CW"
    );
}

#[test]
fn the_rule_is_named_only_where_usb_keys_on_the_frequency_xit_moves_the_transmitter_to() {
    // In the segment, +1 kHz XIT keeps a USB passband inside it: the pick is still the reason.
    let mut e = phone_at("general", "60m", 5.3600, Some("AM"));
    e.request_xit(1_000);
    assert!(
        (e.tx_emission_mhz() - 5.3610).abs() < 1e-9,
        "precondition: the IC-7300 takes the XIT"
    );
    assert_eq!(
        (e.tx_allowed(), told(&e)),
        (false, (true, true)),
        "AM at 5.3600 with +1 kHz XIT"
    );
    // On a channel carrier, +3 kHz XIT takes even a USB passband out of the channel: the
    // frequency is the reason, though the dial alone would have keyed USB.
    let mut e = phone_at("general", "60m", 5.3305, Some("AM"));
    e.request_xit(3_000);
    assert_eq!(
        (e.tx_allowed(), told(&e)),
        (false, (true, false)),
        "AM at 5.3305 with +3 kHz XIT"
    );
}

#[test]
fn the_engine_s_own_refusals_name_the_rule() {
    let said = |r: Result<(), String>| r.err();
    let tuner = |e: &mut Engine| e.observe_rig_tuner(Some(true), true);
    let mut wrong = Vec::new();
    for (dial, pick, words) in [
        (5.3600, "AM", USB_ONLY),
        (5.3305, "FM", USB_ONLY),
        (5.3465, "LSB", USB_ONLY),
        (5.3400, "AM", FREQUENCY),
        (5.3900, "LSB", FREQUENCY),
    ] {
        let want = Some(words.to_string());
        let mut e = phone_at("general", "60m", dial, Some(pick));
        assert!(e.tx_enabled(), "precondition: entering Phone arms TX");
        let voice = said(e.send_voice(vec![0.0; 1_200]));
        tuner(&mut e);
        let atu = said(e.atu_tune_gate());
        let sstv = said(e.sstv_tx_gate());
        for (path, got) in [("voice keyer", voice), ("ATU", atu), ("SSTV send", sstv)] {
            if got != want {
                wrong.push(format!("{dial:.4} {pick} {path}: {got:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "the refusal's words: {wrong:#?}");
}

#[test]
fn an_sstv_picture_dropped_by_the_60_m_rule_says_why() {
    // Queued under a USB pick, then the pick becomes FM while it waits: the rule dropped it.
    let mut e = phone_at("general", "60m", 5.3600, Some("USB"));
    e.sstv_send(vec![0.0; 24_000], "PD-120".into())
        .expect("precondition: USB at 5.3600 sends");
    e.request_sideband_override(Some("FM"));
    assert!(
        e.poll_sstv_tx().is_none(),
        "nothing keys under an FM pick on 60 m"
    );
    assert!(!e.sstv_sending(), "dropped, not held");
    assert_eq!(e.sstv_tx_notice(), Some(SSTV_USB_ONLY));

    // The control: queued at 5.3600, then the dial leaves the segment while the pick stays USB.
    let mut e = phone_at("general", "60m", 5.3600, Some("USB"));
    e.sstv_send(vec![0.0; 24_000], "PD-120".into())
        .expect("precondition: USB at 5.3600 sends");
    e.set_frequency(5.3400, "60m", "USB");
    assert_eq!(
        e.sideband_override().as_deref(),
        Some("USB"),
        "precondition"
    );
    assert!(
        e.poll_sstv_tx().is_none(),
        "nothing keys between the channels"
    );
    assert_eq!(e.sstv_tx_notice(), Some(SSTV_FREQUENCY));
}
