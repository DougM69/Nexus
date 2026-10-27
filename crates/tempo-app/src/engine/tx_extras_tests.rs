//! The radio's PA temperature and its SSB transmit-bandwidth step as the snapshot carries them
//! ([`super::tx_extras`]): shown only where a reading arrived, kept across an unkey, refused
//! while transmitting.

use super::*;

fn engine() -> Engine {
    Engine::new("K1ABC", "EN50", 0)
}

const ICOM_TBW: [&str; 3] = ["WIDE", "MID", "NAR"];

#[test]
fn a_radio_that_reports_no_temperature_puts_none_in_the_snapshot() {
    let e = engine();
    let r = e.snapshot().radio;
    assert_eq!(r.pa_temp_c, None);
    let json = serde_json::to_string(&r).unwrap();
    assert!(
        !json.contains("paTempC"),
        "an absent reading stays out of the JSON"
    );
    assert!(!json.contains("txBwSteps"));
    assert!(!json.contains("\"txBw\""));
}

#[test]
fn a_temperature_reading_reaches_the_snapshot_in_celsius_and_survives_the_unkey() {
    let mut e = engine();
    e.observe_rig_pa_temp_c(41.5);
    assert_eq!(e.snapshot().radio.pa_temp_c, Some(41.5));
    // The transmit meters blank on unkey; the temperature is not one of them.
    e.clear_rig_tx_meters();
    assert_eq!(e.snapshot().radio.pa_temp_c, Some(41.5));
    let json = serde_json::to_string(&e.snapshot().radio).unwrap();
    assert!(json.contains("\"paTempC\":41.5"), "{json}");
}

#[test]
fn an_implausible_temperature_is_dropped_and_the_last_good_one_kept() {
    let mut e = engine();
    e.observe_rig_pa_temp_c(38.0);
    for bad in [f32::NAN, f32::INFINITY, -41.0, 151.0, 255.0] {
        e.observe_rig_pa_temp_c(bad);
        assert_eq!(
            e.snapshot().radio.pa_temp_c,
            Some(38.0),
            "{bad} was believed"
        );
    }
}

#[test]
fn a_radio_switch_or_cat_drop_forgets_both() {
    let mut e = engine();
    e.observe_rig_pa_temp_c(50.0);
    e.observe_rig_tx_bw(&ICOM_TBW, Some(1));
    e.clear_rig_tx_extras();
    let r = e.snapshot().radio;
    assert_eq!(r.pa_temp_c, None);
    assert!(r.tx_bw_steps.is_empty());
    assert_eq!(r.tx_bw, None);
}

#[test]
fn the_selector_appears_only_once_the_radio_has_answered_a_read() {
    let mut e = engine();
    // Declared but not yet answered: no selector, because it could not say where it is.
    e.observe_rig_tx_bw(&ICOM_TBW, None);
    assert!(e.snapshot().radio.tx_bw_steps.is_empty());
    e.observe_rig_tx_bw(&ICOM_TBW, Some(2));
    let r = e.snapshot().radio;
    assert_eq!(r.tx_bw_steps, vec!["WIDE", "MID", "NAR"]);
    assert_eq!(r.tx_bw.as_deref(), Some("NAR"));
    // A missed read keeps the last answer rather than hiding the control.
    e.observe_rig_tx_bw(&ICOM_TBW, None);
    assert_eq!(e.snapshot().radio.tx_bw.as_deref(), Some("NAR"));
}

#[test]
fn a_pick_is_queued_once_by_its_index_on_this_radio() {
    let mut e = engine();
    e.observe_rig_tx_bw(&ICOM_TBW, Some(0));
    e.set_tx_bw("MID").unwrap();
    assert_eq!(e.take_tx_bw_request(), Some(1));
    assert_eq!(e.take_tx_bw_request(), None, "taken once");
}

#[test]
fn a_pick_is_refused_without_steps_or_for_a_step_the_radio_lacks() {
    let mut e = engine();
    assert!(e.set_tx_bw("WIDE").is_err(), "no radio answer yet");
    e.observe_rig_tx_bw(&ICOM_TBW, Some(0));
    assert!(e.set_tx_bw("ULTRA").is_err());
    assert_eq!(e.take_tx_bw_request(), None);
}

#[test]
fn a_pick_is_refused_while_the_radio_is_keyed() {
    let mut e = engine();
    e.observe_rig_tx_bw(&ICOM_TBW, Some(0));
    e.observe_rig_ptt(true);
    let err = e.set_tx_bw("NAR").unwrap_err();
    assert!(err.contains("transmitting"), "{err}");
    assert_eq!(
        e.take_tx_bw_request(),
        None,
        "nothing queued for after the over"
    );
    e.observe_rig_ptt(false);
    e.set_tx_bw("NAR").unwrap();
    assert_eq!(e.take_tx_bw_request(), Some(2));
}

#[test]
fn another_radios_step_list_drops_the_old_answer_and_pick() {
    let mut e = engine();
    e.observe_rig_tx_bw(&ICOM_TBW, Some(0));
    e.set_tx_bw("MID").unwrap();
    e.observe_rig_tx_bw(&["50-3050", "100-2900"], None);
    assert_eq!(e.take_tx_bw_request(), None);
    assert!(
        e.snapshot().radio.tx_bw_steps.is_empty(),
        "not answered on the new list yet"
    );
}

#[test]
fn a_meter_only_temperature_is_carried_as_a_fraction_never_as_degrees() {
    let mut e = engine();
    e.observe_rig_pa_temp_meter(0.4);
    let r = e.snapshot().radio;
    assert_eq!(r.pa_temp_meter, Some(0.4));
    assert_eq!(r.pa_temp_c, None, "no degree scale is published for it");
    e.observe_rig_pa_temp_meter(1.5);
    assert_eq!(
        e.snapshot().radio.pa_temp_meter,
        Some(0.4),
        "off the meter: dropped"
    );
    e.clear_rig_pa_temp();
    assert_eq!(e.snapshot().radio.pa_temp_meter, None);
}
