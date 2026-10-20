//! ⛔ A SPOT WORKED FROM AFAR ARMS NOTHING AT THE SHACK, AND ITS HINT SAYS SO.
//!
//! A computer driving this station works a spot through `work_spot` with `afar` (the command lane
//! wrote it), and the engine enters the spot's section without arming Enable TX: from afar only the
//! wired starts arm, under the stream's transmit rules. The Work still moves the hint every window
//! follows into that cockpit (`workTick`, `workView`). The station's own window, open at the shack,
//! followed it and re-asserted the section from its screen, `set_operating_mode` with
//! `{ mode, followFreq: false }` and no `afar`, which arms the latch for Phone, CW, RTTY and PSK
//! wherever nothing holds it: the authority the Work withheld, given back by the shack's screen.
//!
//! The hint now carries where the Work came from (`workAfar`, the key the window reads), and the
//! window opens that cockpit without the re-assert. The window's half is
//! `ui/src/App.workFromAfar.test.tsx`; the two meet at that command and those arguments, which the
//! first test here shows arm the latch.

use super::*;
use serde_json::{json, Value};

/// A station with every HF privilege, at launch: in Digital, TX off, nothing held.
fn station() -> Engine {
    let mut e = Engine::new("K1ABC", "EN50", 0);
    e.set_license_class("extra");
    e
}

/// The manual sections a spot can open, each at a 20 m spot.
const MANUAL: [(&str, f64); 4] = [
    ("cw", 14.025),
    ("phone", 14.250),
    ("rtty", 14.080),
    ("keyboard", 14.070),
];

/// The hint as the window reads it, off the wire: `(workTick, workView, workAfar)`.
fn hint(e: &Engine) -> (Value, Value, Value) {
    let wire = serde_json::to_value(e.snapshot()).unwrap();
    (
        wire["workTick"].clone(),
        wire["workView"].clone(),
        wire["workAfar"].clone(),
    )
}

/// `lib.rs` `work_spot` from afar: the lane wrote `afar`, so `afar.is_none()` is false.
fn work_from_afar(e: &mut Engine, mode: &str, mhz: f64) {
    e.work_spot_tiered_with_arming(None, mode, mhz, "20m", None, false);
}

/// `lib.rs` `set_operating_mode` from the shack's own screen, `{ mode, followFreq: false }` and no
/// `afar`: what the window's re-assert of a followed hint sends.
fn shack_screen_reasserts(e: &mut Engine, mode: &str) {
    e.set_operating_mode_with_arming(mode, false, true);
}

/// The Work from afar arms nothing and its hint says where it came from, in every manual section and
/// in Digital.
#[test]
fn a_spot_worked_from_afar_says_so_in_the_hint_and_arms_nothing() {
    let mut rows = Vec::new();
    for (mode, mhz) in MANUAL.into_iter().chain([("digital", 14.074)]) {
        let mut e = station();
        work_from_afar(&mut e, mode, mhz);
        rows.push((mode, e.tx_enabled(), hint(&e)));
    }
    assert_eq!(
        rows,
        MANUAL
            .into_iter()
            .chain([("digital", 14.074)])
            .map(|(mode, _)| (mode, false, (json!(1), json!(mode), json!(true))))
            .collect::<Vec<_>>(),
        "(section, TX armed by the Work from afar, (workTick, workView, workAfar))"
    );
}

/// The premise of the window's half, and why it must not send the re-assert: after a Work from
/// afar left the latch down, the shack screen's `set_operating_mode` for the followed hint, with
/// exactly the arguments the window sends, arms it in every manual section.
#[test]
fn the_shack_screens_reassert_arms_what_a_work_from_afar_left_down() {
    let mut rows = Vec::new();
    for (mode, mhz) in MANUAL {
        let mut e = station();
        work_from_afar(&mut e, mode, mhz);
        let after_work = e.tx_enabled();
        shack_screen_reasserts(&mut e, mode);
        rows.push((mode, after_work, e.tx_enabled()));
    }
    assert_eq!(
        rows,
        MANUAL.map(|(mode, _)| (mode, false, true)),
        "(section, TX after the Work from afar, TX after the shack screen's re-assert)"
    );
}

/// CONTROL: a Work at the station (`lib.rs` `work_spot` with no `afar`) arms as it always has, and
/// its hint carries no `workAfar`, so the window follows it with its re-assert as before.
#[test]
fn the_stations_own_work_arms_as_before_and_its_hint_names_no_afar() {
    let mut rows = Vec::new();
    for (mode, mhz) in MANUAL {
        let mut e = station();
        e.work_spot_tiered(None, mode, mhz, "20m", None);
        rows.push((mode, e.tx_enabled(), hint(&e)));
    }
    assert_eq!(
        rows,
        MANUAL
            .map(|(mode, _)| (mode, true, (json!(1), json!(mode), Value::Null)))
            .to_vec(),
        "(section, TX armed by the station's own Work, (workTick, workView, workAfar))"
    );
}

/// The mark belongs to the one Work: the station's own next Work clears it, and a Work from afar
/// after that sets it again.
#[test]
fn each_work_says_where_it_came_from() {
    let mut e = station();
    work_from_afar(&mut e, "cw", 14.025);
    let afar = hint(&e);
    e.work_spot_tiered(None, "phone", 14.250, "20m", None);
    let own = hint(&e);
    work_from_afar(&mut e, "cw", 14.030);
    assert_eq!(
        (afar, own, hint(&e)),
        (
            (json!(1), json!("cw"), json!(true)),
            (json!(2), json!("phone"), Value::Null),
            (json!(3), json!("cw"), json!(true)),
        ),
        "(from afar, the station's own, from afar again)"
    );
}

/// CONTROL: a Stop still holds against both Works and against the shack screen's own re-assert.
#[test]
fn a_stop_holds_against_either_work() {
    let mut e = station();
    e.set_operating_mode("cw", true);
    assert!(
        e.tx_enabled(),
        "control: entering CW with nothing stopped arms TX"
    );
    e.halt_tx();
    e.work_spot_tiered(None, "phone", 14.250, "20m", None);
    let own = e.tx_enabled();
    work_from_afar(&mut e, "cw", 14.025);
    let afar = e.tx_enabled();
    shack_screen_reasserts(&mut e, "cw");
    assert_eq!(
        (own, afar, e.tx_enabled()),
        (false, false, false),
        "(the station's own Work, the Work from afar, the shack screen's re-assert): TX after a Stop"
    );
}
