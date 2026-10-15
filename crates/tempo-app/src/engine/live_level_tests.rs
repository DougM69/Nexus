//! The cockpit's **Pwr** slider and the RX gain (`set_tx_level` / `set_rx_gain`) set the drive and
//! the capture gain of the radio in use as they move. What comes after them must keep that value: a
//! Settings save of anything, and a save of another radio from its card's **Edit**. Both read the
//! levels back from the active radio's profile (`get_settings` mirrors the profile into the flat
//! fields every save is built on, and `update_radio_profile` mirrors it into the flat fields the
//! radio loop reads), so a profile the narrow edit left behind was the level they put back.

use super::*;
use crate::settings::{RadioProfile, RadioProfilePatch};

/// The FT-710 (radio 0) in use at drive 0.25 and RX gain ×1.5, as a Settings save leaves it.
fn station() -> Engine {
    let mut e = Engine::new("K1ABC", "EN50", 0);
    let mut s = e.settings().clone();
    s.tx_level = 0.25;
    s.rx_gain = 1.5;
    e.apply_settings(s);
    e
}

/// What the Settings window reads, and what every save it makes is built on: `get_settings` hands
/// it the settings with the active radio's profile mirrored into the flat fields.
fn settings_window(e: &Engine) -> Settings {
    let mut s = e.settings().clone();
    s.sync_flat_from_active();
    s
}

/// Save radio `id` from its card's **Edit**, with `edit` made: the per-radio verb, its patch built
/// from that radio's stored config as the window builds it from the form (`radioPatch`).
fn save_radio(e: &mut Engine, id: u32, edit: impl FnOnce(&mut RadioProfile)) {
    let mut p = e
        .settings()
        .radios
        .iter()
        .find(|p| p.id == id)
        .expect("the radio is in the roster")
        .clone();
    edit(&mut p);
    let patch: RadioProfilePatch =
        serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
    e.update_radio_profile(id, patch).unwrap();
}

/// The drive and the RX gain the radio loop applies to the radio in use.
fn live(e: &Engine) -> (f32, f32) {
    (e.settings().tx_level, e.settings().rx_gain)
}

/// ★ The operator's report: Pwr moved in the cockpit, then a Save in Settings of something else.
#[test]
fn a_settings_save_after_a_cockpit_change_keeps_the_drive_and_the_gain() {
    let mut e = station();
    e.set_tx_level(0.81);
    e.set_rx_gain(3.5);
    let mut window = settings_window(&e);
    let shown = (window.tx_level, window.rx_gain);
    // A change to another field, saved: the window's read with that one edit made.
    window.units = "metric".into();
    e.apply_settings(window);
    assert_eq!(
        (shown, live(&e)),
        ((0.81, 3.5), (0.81, 3.5)),
        "Settings shows the drive and gain set last, and a save of another field keeps them"
    );
}

/// With two radios: Pwr moved on the FT-710, then the FT-991A saved from its card's Edit.
#[test]
fn saving_another_radio_after_a_cockpit_change_keeps_the_drive_and_the_gain() {
    let mut e = station();
    let other = e.add_radio();
    save_radio(&mut e, other, |p| {
        p.tx_level = 0.36;
        p.rx_gain = 2.0;
    });
    e.set_tx_level(0.81);
    e.set_rx_gain(3.5);
    save_radio(&mut e, other, |p| {
        p.serial_port = "/dev/cu.usbserial-991A".into();
    });
    let saved = e
        .settings()
        .radios
        .iter()
        .find(|p| p.id == other)
        .map(|p| (p.tx_level, p.rx_gain));
    assert_eq!(
        (live(&e), saved),
        ((0.81, 3.5), Some((0.36, 2.0))),
        "the radio in use keeps the drive and gain set last, and the edited radio keeps its own"
    );
}

/// Control: a level set in Settings after the cockpit's is the one applied, and the one stored.
#[test]
fn a_level_saved_in_settings_after_a_cockpit_change_is_the_one_applied() {
    let mut e = station();
    e.set_tx_level(0.81);
    e.set_rx_gain(3.5);
    let mut window = settings_window(&e);
    window.tx_level = 0.49;
    window.rx_gain = 2.5;
    e.apply_settings(window);
    let stored = e
        .settings()
        .active_profile()
        .map(|p| (p.tx_level, p.rx_gain));
    assert_eq!(
        (live(&e), stored),
        ((0.49, 2.5), Some((0.49, 2.5))),
        "the level saved last is the one the radio loop applies and the radio's profile holds"
    );
}
