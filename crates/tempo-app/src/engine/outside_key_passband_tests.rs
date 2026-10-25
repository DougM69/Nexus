//! A program outside Nexus is judged across the whole audio passband it could send.
//!
//! A CAT broker client or a TCI program keys the transmitter with audio Nexus never hears, at an
//! offset Nexus does not know: a data program puts its signal anywhere from 0 to about 3 kHz
//! above the carrier on an upper sideband (below it on a lower one). Judging the carrier, or
//! Nexus's own frame at Nexus's own TX offset, let a program tuned just under an edge put its
//! signal inside the next segment (operator ruling, 2026-10-25: the whole passband must lie inside
//! the class's privileges for the mode, every class, every band edge). The Phone section judges
//! its own phone passband, CW the carrier, as for Nexus's own transmissions, which keep their own
//! judgement.

use super::tx_gate_table::station;
use super::*;
use tempo_net::tci::server::{KeyKind, Mic};
use tempo_net::tci::wire::Word;

/// Nexus's own TX offset in these tests: low, so Nexus's own data frame (300–350 Hz) sits well
/// inside wherever the program's 3 kHz passband does not, and the two judgements can disagree.
const OWN_OFFSET_HZ: f32 = 300.0;

/// One edge, swept from the side the passband reaches it from.
struct Edge {
    class: &'static str,
    section: &'static str,
    band: &'static str,
    /// The section's sideband (the commanded word follows it).
    sideband: &'static str,
    /// The privilege edge, MHz.
    edge: f64,
    /// kHz from the edge, on the side away from it, and what each key gets there, as `T`/`F`:
    /// the outside key's verdict, and Nexus's own (the control).
    sweep: &'static [(f64, char, char)],
}

/// An upper-sideband passband under an edge: 5 kHz under keys; 3 kHz under the passband's top
/// reaches the edge, which is outside, as a segment's top is; 2 and 0.1 kHz cross it. Nexus's own
/// frame keys until 0.1 kHz.
const UNDER: &[(f64, char, char)] = &[
    (5.0, 'T', 'T'),
    (3.0, 'F', 'T'),
    (2.0, 'F', 'T'),
    (0.1, 'F', 'F'),
];

/// A lower-sideband passband over an edge: 3 kHz over, the passband's bottom is the edge, which
/// is inside; 2.9 kHz and closer it crosses.
const OVER: &[(f64, char, char)] = &[
    (5.0, 'T', 'T'),
    (3.0, 'T', 'T'),
    (2.9, 'F', 'T'),
    (2.0, 'F', 'T'),
    (0.1, 'F', 'F'),
];

/// Phone: the gate's own 2.8 kHz phone passband, for the program and for Nexus alike.
const PHONE_UNDER: &[(f64, char, char)] = &[
    (5.0, 'T', 'T'),
    (3.0, 'T', 'T'),
    (2.0, 'F', 'F'),
    (0.1, 'F', 'F'),
];

/// CW: the carrier, for the program and for Nexus alike.
const CW_INSIDE: &[(f64, char, char)] = &[
    (5.0, 'T', 'T'),
    (3.0, 'T', 'T'),
    (2.0, 'T', 'T'),
    (0.1, 'T', 'T'),
    (-0.1, 'F', 'F'),
];

const EDGES: &[Edge] = &[
    // Technician: data below 28.300, voice only above it.
    Edge {
        class: "technician",
        section: "digital",
        band: "10m",
        sideband: "USB",
        edge: 28.300,
        sweep: UNDER,
    },
    // General: the data segment's top on 20 m.
    Edge {
        class: "general",
        section: "digital",
        band: "20m",
        sideband: "USB",
        edge: 14.150,
        sweep: UNDER,
    },
    // Extra: CW and data end at 3.600, phone above.
    Edge {
        class: "extra",
        section: "digital",
        band: "80m",
        sideband: "USB",
        edge: 3.600,
        sweep: UNDER,
    },
    // The top of 6 m.
    Edge {
        class: "extra",
        section: "digital",
        band: "6m",
        sideband: "USB",
        edge: 54.000,
        sweep: UNDER,
    },
    // LSB mirror: CW only below 50.100, every mode above.
    Edge {
        class: "technician",
        section: "digital",
        band: "6m",
        sideband: "LSB",
        edge: 50.100,
        sweep: OVER,
    },
    // LSB mirror: a General's data floor on 20 m, Extra-only below it.
    Edge {
        class: "general",
        section: "digital",
        band: "20m",
        sideband: "LSB",
        edge: 14.025,
        sweep: OVER,
    },
    // Keyboard (PSK31) under a General's 20 m data top: the program's passband, not PSK's tone.
    Edge {
        class: "general",
        section: "keyboard",
        band: "20m",
        sideband: "USB",
        edge: 14.150,
        sweep: UNDER,
    },
    // Phone at the top of 20 m: the gate's own phone passband, unchanged.
    Edge {
        class: "extra",
        section: "phone",
        band: "20m",
        sideband: "USB",
        edge: 14.350,
        sweep: PHONE_UNDER,
    },
    // CW from a General's 20 m floor: the carrier, unchanged.
    Edge {
        class: "general",
        section: "cw",
        band: "20m",
        sideband: "USB",
        edge: 14.025,
        sweep: CW_INSIDE,
    },
];

/// The TCI program the Phone rows key from.
const PROGRAM: u64 = 1;

/// A station as `class` at `dial`, armed, Nexus's own TX offset [`OWN_OFFSET_HZ`], with the CAT
/// broker's and the TCI server's transmit switches on and the TCI program's last `modulation:`
/// the section's sideband.
fn armed(class: &str, section: &str, band: &str, sideband: &str, dial: f64) -> Engine {
    let mut e = station(3073, class, section, dial, band, sideband);
    assert!(
        (e.settings.dial_mhz - dial).abs() < 1e-9,
        "precondition: tuned to {dial}"
    );
    e.set_tx_offset(OWN_OFFSET_HZ);
    e.settings.cat_broker_ptt = true;
    e.settings.tci_server_tx = true;
    e.set_tx_enabled(true);
    e.tci_note_modulation(
        PROGRAM,
        &Word::new(&sideband.to_ascii_lowercase()).expect("a word"),
    );
    e
}

/// The broker's key, granted and read as keyed; and its release, never refused.
fn broker_keys(e: &mut Engine) -> bool {
    let keyed = e.broker_ptt(true) && e.broker_ptt_active() && e.manual_ptt();
    assert!(e.broker_ptt(false), "the release is never refused");
    assert!(!e.manual_ptt(), "and it unkeys");
    keyed
}

/// ⛔ AN OUTSIDE KEY IS ALLOWED ONLY WHERE THE PROGRAM'S WHOLE PASSBAND IS INSIDE THE LICENCE.
/// Edge by edge for several classes, from the side the passband reaches each edge: in Digital the
/// broker's key is judged across 0–3 kHz on the commanded side, so a dial up to 3 kHz from the
/// edge is refused where Nexus's own frame keys. CONTROLS: Nexus's own judgement at every dial is
/// unchanged (its own frame at 300 Hz); the Phone section judges the program as Nexus's own voice,
/// TCI microphone key and over included; CW judges the carrier.
#[test]
fn an_outside_key_is_judged_across_the_programs_whole_passband_at_every_edge() {
    let (mut wrong, mut discriminating) = (Vec::new(), 0);
    for edge in EDGES {
        let lsb = edge.sideband == "LSB";
        for &(khz, outside, own) in edge.sweep {
            // Away from the edge: under it on USB, over it on LSB; CW sits over its floor.
            let away = if lsb || edge.section == "cw" {
                1.0
            } else {
                -1.0
            };
            let dial = edge.edge + away * khz / 1000.0;
            let at = || armed(edge.class, edge.section, edge.band, edge.sideband, dial);
            let (want_outside, want_own) = (outside == 'T', own == 'T');
            let label = format!(
                "{} {} {} at {dial:.4} ({khz} kHz from {})",
                edge.class, edge.section, edge.sideband, edge.edge
            );
            let mut e = at();
            if e.tx_allowed() != want_own {
                wrong.push(format!("{label}: Nexus's own key {}", e.tx_allowed()));
            }
            if e.outside_key_passband_allowed() != want_outside {
                wrong.push(format!("{label}: the passband verdict"));
            }
            if broker_keys(&mut e) != want_outside {
                wrong.push(format!("{label}: the broker's key"));
            }
            if edge.section == "phone" {
                let mut e = at();
                let mic = e
                    .tci_key(PROGRAM, KeyKind::Mic(Mic::Selected), false)
                    .is_ok()
                    && e.manual_ptt();
                if mic != want_outside {
                    wrong.push(format!("{label}: the TCI microphone key {mic}"));
                }
                let mut e = at();
                let over = e.tci_key(PROGRAM, KeyKind::Tci, true).is_ok();
                if over != want_outside {
                    wrong.push(format!("{label}: the TCI over {over}"));
                }
            }
            discriminating += usize::from(want_own && !want_outside);
        }
    }
    assert!(wrong.is_empty(), "outside keys at the edges: {wrong:#?}");
    assert_eq!(
        discriminating, 14,
        "the sweep must reach the dials where the two judgements differ"
    );
}

/// ⛔ A HELD OUTSIDE KEY DROPS WHEN A RETUNE CARRIES THE PASSBAND ACROSS AN EDGE, and its release
/// is never refused. A General's broker key in Digital at 14.140 is held; a retune to 14.148
/// leaves Nexus's own frame inside the data segment, so only the program's passband crosses
/// 14.150. The key is still latched, so the mask on read is what drops it.
#[test]
fn a_held_outside_key_drops_when_a_retune_carries_the_passband_across_an_edge() {
    let mut e = armed("general", "digital", "20m", "USB", 14.140);
    assert!(
        e.broker_ptt(true) && e.manual_ptt(),
        "precondition: the broker keys at 14.140"
    );
    e.set_frequency(14.148, "20m", "USB");
    assert!(
        e.tx_allowed(),
        "precondition: Nexus's own frame keys at 14.148"
    );
    assert!(
        e.broker_ptt_active(),
        "precondition: the retune leaves the key latched"
    );
    assert!(!e.manual_ptt(), "the broker's held key drops");
    assert!(!e.broker_ptt(true), "and a re-assert is refused");
    assert!(e.broker_ptt(false), "the release is never refused");
    assert!(!e.broker_ptt_active());
}

/// The broker's refusal names the edge and how far the program's audio could reach, on the side
/// the radio sends it: up from the carrier on USB, down on LSB. None where the passband fits.
#[test]
fn the_passband_refusal_says_where_the_programs_audio_could_reach() {
    let e = armed("general", "digital", "20m", "USB", 14.148);
    let why = e.outside_key_passband_refusal().expect("refused at 14.148");
    for words in [
        "outside your license privileges at the band edge",
        "the program's audio could reach 14.1510 MHz",
    ] {
        assert!(why.contains(words), "{why:?} must say {words:?}");
    }
    let e = armed("general", "digital", "20m", "LSB", 14.027);
    let why = e.outside_key_passband_refusal().expect("refused at 14.027");
    assert!(why.contains("could reach 14.0240 MHz"), "{why:?}");
    let e = armed("general", "digital", "20m", "USB", 14.140);
    assert_eq!(e.outside_key_passband_refusal(), None, "control: inside");
}

/// The four 60 m channels' dials, each a channel's carrier, 1.5 kHz under its centre.
const SIXTY_CHANNEL_DIALS: [f64; 4] = [5.3305, 5.3465, 5.3715, 5.4035];

/// ⛔ ON A 60 M CHANNEL AN OUTSIDE KEY IS JUDGED AT THE CHANNEL, as before the passband rule
/// (operator ruling, 2026-10-25): a channel is 2.8 kHz wide, so a 3 kHz passband never fits one
/// and the rule would refuse every outside data key there. A General's or an Extra's broker key
/// in Digital at each channel's dial keys as Nexus's own does; between channels it is still
/// refused. The 5.3515–5.3665 segment keeps the passband rule: 3 kHz under its top refused where
/// Nexus's own frame keys, 4 kHz under it granted.
#[test]
fn on_a_60m_channel_an_outside_key_is_judged_at_the_channel() {
    let mut wrong = Vec::new();
    for class in ["general", "extra"] {
        for dial in SIXTY_CHANNEL_DIALS {
            let mut e = armed(class, "digital", "60m", "USB", dial);
            if !(e.tx_allowed() && e.outside_key_passband_allowed() && broker_keys(&mut e)) {
                wrong.push(format!("{class} at the channel dial {dial}: refused"));
            }
        }
        // Off every channel: between the 5.3320 and 5.3480 channels, and a hair under a carrier.
        for dial in [5.3400, 5.3300] {
            let mut e = armed(class, "digital", "60m", "USB", dial);
            if e.outside_key_passband_allowed() || broker_keys(&mut e) {
                wrong.push(format!("{class} off-channel at {dial}: granted"));
            }
        }
        // The segment's top, by the passband: 5.3635 reaches 5.3665, the segment's top.
        let mut e = armed(class, "digital", "60m", "USB", 5.3635);
        if !e.tx_allowed() || e.outside_key_passband_allowed() || broker_keys(&mut e) {
            wrong.push(format!(
                "{class} at 5.3635 under the segment's top: not the passband"
            ));
        }
        let mut e = armed(class, "digital", "60m", "USB", 5.3625);
        if !(e.outside_key_passband_allowed() && broker_keys(&mut e)) {
            wrong.push(format!("{class} at 5.3625 inside the segment: refused"));
        }
    }
    // CONTROL: a 20 m edge point keeps the passband rule.
    let mut e = armed("general", "digital", "20m", "USB", 14.148);
    if !e.tx_allowed() || e.outside_key_passband_allowed() || broker_keys(&mut e) {
        wrong.push("general at 14.148: not the passband".into());
    }
    assert!(wrong.is_empty(), "60 m outside keys: {wrong:#?}");
}
