//! Resolve an operating-section destination without changing the live station.
//!
//! Both the native setter and future Remote radio transactions must use this
//! decision. Preparation cannot bank a memory, arm TX, change a decoder or leave
//! a target for the radio loop to apply after a Remote command has expired.
//! This is the section/frequency decision only, not a CAT permission or a claim
//! that a hardware operation has completed.
//!
//! One entry keeps the dial instead of homing it: Phone entered from the SSTV
//! screen ([`Engine::prepare_mode_entry_keeping_dial`]), whose pictures already
//! go out in the phone segment.

use super::Engine;
use crate::settings::OperatingMode;

#[derive(Debug, PartialEq)]
pub(super) struct ModeEntry {
    pub mode: OperatingMode,
    pub frequency: Option<(f64, String)>,
    /// The entry keeps the dial where it is (`frequency` is then `None`). The setter
    /// carries the operator's residency on that dial into the section entered.
    pub kept_dial: bool,
}

impl Engine {
    pub(super) fn prepare_mode_entry(&self, mode: &str, follow_frequency: bool) -> ModeEntry {
        let mode = match mode.to_ascii_lowercase().as_str() {
            "phone" => OperatingMode::Phone,
            "cw" => OperatingMode::Cw,
            "rtty" => OperatingMode::Rtty,
            "keyboard" => OperatingMode::Keyboard,
            _ => OperatingMode::Digital,
        };
        let frequency = if follow_frequency && self.sat_dial_owner.is_none() {
            let band = &self.settings.band;
            // The setter banks the outgoing residency before restoring a cell.
            // On same-mode entry that would replace the cell being read. Peek
            // at that exact replacement without consuming or banking it here.
            let remembered = match self.dial_residency.as_ref() {
                Some(r) if !r.band.is_empty() && &r.band == band && r.mode == mode => {
                    self.resolve_dial_memory(band, mode, r.dial_mhz, r.sideband.clone())
                }
                _ => self.recall_dial_memory(band, mode),
            };
            remembered.or_else(|| self.mode_home(mode))
        } else {
            None
        };
        ModeEntry {
            mode,
            frequency,
            kept_dial: false,
        }
    }

    /// The follow entry for a section entered from a screen that already rides it: today only
    /// Phone from the SSTV screen. The operator's rule (2026-10-20): "when on sstv and changing
    /// to phone, it should not switch to phone frequency until I tell it to". The dial stays
    /// where it is whenever the section may transmit there, however it got there (a channel
    /// pick, the rig's own knob, the dial the app launched on), and the operator moves it
    /// themselves: by tuning, a band or memory pick, or a spot.
    ///
    /// A dial the section may NOT transmit on homes exactly as [`Self::prepare_mode_entry`]
    /// homes it, through the remembered dial or the segment default: the 40 m image channel
    /// 7.171 sits below a General's phone segment, and 28.680 outside a Technician's 10 m one.
    /// Legality is [`Engine::emission_allowed`], the model the dial memory's restore and the
    /// transmit gate share, so a kept dial is one the gate will key. A held pass moves nothing
    /// here either: keeping is no QSY, and the home stands down for the pass already.
    pub(super) fn prepare_mode_entry_keeping_dial(&self, mode: &str) -> ModeEntry {
        let entry = self.prepare_mode_entry(mode, true);
        let s = &self.settings;
        if !self.emission_allowed(entry.mode, s.dial_mhz, &s.sideband) {
            return entry;
        }
        ModeEntry {
            mode: entry.mode,
            frequency: None,
            kept_dial: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn station() -> Engine {
        Engine::with_settings(Settings::default())
    }

    #[test]
    fn preparation_does_not_bank_the_dial_or_change_any_live_control() {
        let mut e = station();
        e.set_operating_mode("phone", false);
        e.set_frequency(14.240, "20m", "USB");
        e.set_tx_enabled(false);
        e.take_immediate_retune();
        let settings = serde_json::to_value(e.settings()).unwrap();
        let generation = e.tx_gate_gen;
        let actuation = e.remote_actuation_context_generation();
        let memories = e.freq_memory.clone();
        let residency = e.dial_residency.as_ref().unwrap().dial_mhz;

        for _ in 0..3 {
            let entry = e.prepare_mode_entry("cw", true);
            assert_eq!(entry.mode, OperatingMode::Cw);
            assert_eq!(entry.frequency.as_ref().unwrap().0, 14.030);
        }

        assert_eq!(serde_json::to_value(e.settings()).unwrap(), settings);
        assert_eq!(e.tx_gate_gen, generation);
        assert_eq!(e.remote_actuation_context_generation(), actuation);
        assert_eq!(e.freq_memory, memories);
        assert_eq!(e.dial_residency.as_ref().unwrap().dial_mhz, residency);
        assert!(!e.tx_enabled());
        assert!(!e.take_immediate_retune());

        // Positive control: executing the native gesture does bank the outgoing
        // dial, assert the new mode and retain native manual-mode arming.
        e.set_operating_mode("cw", true);
        assert_eq!(e.settings().dial_mhz, 14.030);
        assert_eq!(
            e.freq_memory[&("20m".into(), OperatingMode::Phone)].0,
            14.240
        );
        assert!(e.tx_enabled());
        assert!(e.take_immediate_retune());
    }

    #[test]
    fn same_mode_entry_uses_the_pending_residency_instead_of_an_older_cell() {
        let mut e = station();
        e.set_operating_mode("phone", false);
        e.set_frequency(14.240, "20m", "USB");
        e.bank_dial_memory();
        e.set_frequency(14.285, "20m", "USB");
        let entry = e.prepare_mode_entry("phone", true);
        assert_eq!(entry.frequency, Some((14.285, "USB".into())));
        assert_eq!(
            e.freq_memory[&("20m".into(), OperatingMode::Phone)].0,
            14.240
        );
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().dial_mhz, 14.285);
    }

    #[test]
    fn preparation_and_native_entry_restore_the_same_mode_memories() {
        let mut e = station();
        for (mode, dial, sideband) in [
            ("phone", 14.240, "USB"),
            ("cw", 14.055, "USB"),
            ("rtty", 14.090, "LSB"),
            ("keyboard", 14.071, "USB"),
            ("digital", 14.074, "USB"),
        ] {
            e.set_operating_mode(mode, false);
            e.set_frequency(dial, "20m", sideband);
        }
        for (mode, expected) in [
            ("phone", 14.240),
            ("cw", 14.055),
            ("rtty", 14.090),
            ("keyboard", 14.071),
            ("digital", 14.074),
        ] {
            let entry = e.prepare_mode_entry(mode, true);
            assert_eq!(entry.frequency.as_ref().unwrap().0, expected, "{mode}");
            e.set_operating_mode(mode, true);
            assert_eq!(e.settings().operating_mode, entry.mode);
            assert_eq!(e.settings().dial_mhz, expected, "{mode}");
            assert_eq!(e.settings().sideband, entry.frequency.unwrap().1);
        }
    }

    #[test]
    fn no_follow_or_a_held_pass_never_prepares_a_frequency_change() {
        let mut e = station();
        e.set_frequency(14.240, "20m", "LSB");
        assert!(e.prepare_mode_entry("digital", false).frequency.is_none());
        e.sat_dial_owner = Some(("test-pass".into(), 0));
        assert!(e.prepare_mode_entry("digital", true).frequency.is_none());
        e.set_operating_mode("digital", true);
        assert_eq!(e.settings().dial_mhz, 14.240);
        assert_eq!(e.settings().sideband, "LSB");
        assert!(e.sat_dial_owner.is_some());
    }

    #[test]
    fn a_pending_cell_that_lost_privilege_replaces_an_older_valid_cell() {
        use crate::settings::LicenseClass;
        let mut e = station();
        e.settings.license_class = LicenseClass::Extra;
        e.set_operating_mode("phone", false);
        e.set_frequency(14.240, "20m", "USB");
        e.bank_dial_memory();
        e.set_frequency(14.160, "20m", "USB");
        e.settings.license_class = LicenseClass::General;

        // Native entry banks 14.160 OVER 14.240, refuses that remembered
        // emission under the new class, then uses the General phone home.
        let entry = e.prepare_mode_entry("phone", true);
        assert_eq!(entry.frequency, Some((14.225, "USB".into())));
        assert_eq!(
            e.freq_memory[&("20m".into(), OperatingMode::Phone)].0,
            14.240
        );
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().dial_mhz, 14.225);
    }

    #[test]
    fn a_knob_residency_gets_its_sideband_from_the_existing_band_policy() {
        let mut e = station();
        e.set_operating_mode("phone", false);
        e.set_frequency(7.200, "40m", "USB");
        e.rig_dial_seen = true;
        e.observe_rig_freq(7_250_000);
        assert!(e.dial_residency.as_ref().unwrap().sideband.is_none());
        let entry = e.prepare_mode_entry("phone", true);
        assert_eq!(entry.frequency, Some((7.250, "LSB".into())));
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().dial_mhz, 7.250);
        assert_eq!(e.settings().sideband, "LSB");
    }

    #[test]
    fn digital_entry_still_corrects_an_outgoing_lsb_without_homing() {
        let mut e = station();
        e.set_operating_mode("rtty", false);
        e.set_frequency(14.090, "20m", "LSB");
        assert!(e.tx_enabled());
        let entry = e.prepare_mode_entry("digital", false);
        assert!(entry.frequency.is_none());
        assert_eq!(e.settings().sideband, "LSB");
        assert!(
            e.tx_enabled(),
            "preparation cannot lower the local latch either"
        );
        e.set_operating_mode("digital", false);
        assert_eq!(e.settings().dial_mhz, 14.090);
        assert_eq!(e.settings().sideband, "USB");
        assert!(!e.tx_enabled());
    }

    #[test]
    fn an_off_band_residency_does_not_become_a_recall_cell() {
        let mut e = station();
        e.set_operating_mode("phone", false);
        e.set_frequency(5.000, "", "USB");
        let entry = e.prepare_mode_entry("phone", true);
        assert!(entry.frequency.is_none());
        assert!(e.dial_residency.is_some());
        e.set_operating_mode("phone", true);
        assert_eq!(e.settings().dial_mhz, 5.000);
        assert!(!e
            .freq_memory
            .contains_key(&(String::new(), OperatingMode::Phone)));
    }

    // ── The Phone tab on the SSTV screen ────────────────────────────────────────────────────
    //
    // The operator, 2026-10-20: "when on sstv and changing to phone, it should not switch to
    // phone frequency until I tell it to". The SSTV screen claims no section on entry, so the
    // station may still be in FT8's when the operator gets there (the rig's own knob, a launch),
    // and the ordinary Phone tab then homed to the segment default.

    /// A General on 20 m FT8, where an operator usually comes to the SSTV screen from.
    fn general_on_ft8() -> Engine {
        let mut e = Engine::new("K1ABC", "EN50", 0);
        e.set_license_class("general");
        e.set_operating_mode("digital", false);
        e.set_frequency(14.074, "20m", "USB");
        e
    }

    /// The rig's own VFO knob, after the CAT link's first word.
    fn knob(e: &mut Engine, hz: u64) {
        e.rig_dial_seen = true;
        e.observe_rig_freq(hz);
    }

    /// The Phone tab pressed on the SSTV screen, as the app sends it (`keepDial`)…
    fn phone_tab_on_the_sstv_screen(e: &mut Engine, arm_manual: bool) {
        e.set_operating_mode_keeping_dial("phone", arm_manual);
    }

    /// …and the entry it prepares.
    fn prepared_on_the_sstv_screen(e: &Engine) -> ModeEntry {
        e.prepare_mode_entry_keeping_dial("phone")
    }

    fn assert_dial(e: &Engine, mhz: f64, why: &str) {
        let dial = e.settings().dial_mhz;
        assert!((dial - mhz).abs() < 1e-9, "{why}: the dial is {dial}");
    }

    #[test]
    fn a_channel_pick_then_the_phone_tab_keeps_the_dial_with_no_qsy() {
        let mut e = general_on_ft8();
        e.sstv_tune(14.230, "20m", "USB");
        assert_eq!(
            prepared_on_the_sstv_screen(&e).frequency,
            None,
            "no QSY prepared, not even to the dial it is already on"
        );
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_eq!(e.settings().operating_mode, OperatingMode::Phone);
        assert_dial(&e, 14.230, "the 20 m image channel stays");
    }

    #[test]
    fn an_image_frequency_tuned_on_the_rig_survives_the_phone_tab() {
        let mut e = general_on_ft8();
        knob(&mut e, 14_230_000);
        assert_eq!(
            e.settings().operating_mode,
            OperatingMode::Digital,
            "premise: the SSTV screen claimed no section, so this is still FT8's"
        );
        assert_eq!(
            prepared_on_the_sstv_screen(&e).frequency,
            None,
            "no QSY prepared from the knob's 14.230"
        );
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_eq!(e.settings().operating_mode, OperatingMode::Phone);
        assert_dial(
            &e,
            14.230,
            "the knob's 14.230 stays, not the 14.225 segment default",
        );
    }

    #[test]
    fn the_dial_the_app_launched_on_survives_the_phone_tab() {
        let mut e = Engine::with_settings(Settings {
            band: "20m".into(),
            dial_mhz: 14.230,
            sideband: "USB".into(),
            operating_mode: OperatingMode::Digital,
            license_class: crate::settings::LicenseClass::General,
            ..Settings::default()
        });
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_eq!(e.settings().operating_mode, OperatingMode::Phone);
        assert_dial(&e, 14.230, "launched on the SSTV screen on 14.230");
    }

    #[test]
    fn a_dial_phone_may_not_transmit_on_still_homes_from_the_sstv_screen() {
        // 40 m SSTV, 7.171 LSB, sits below a General's 7.175 phone edge.
        let mut e = general_on_ft8();
        e.sstv_tune(7.171, "40m", "LSB");
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_dial(
            &e,
            7.1778,
            "General, 40 m image channel: the phone home clear of the edge",
        );
        assert_eq!(e.settings().sideband, "LSB");
        // 10 m SSTV, 28.680, is outside a Technician's 28.300-28.500.
        let mut e = Engine::new("K1ABC", "EN50", 0);
        e.set_license_class("technician");
        e.sstv_tune(28.680, "10m", "USB");
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_dial(
            &e,
            28.300,
            "Technician, 10 m image channel: the 10 m phone home",
        );
        // A visit to the SSTV screen that tuned nothing: FT8's 14.074 is no phone frequency.
        let mut e = general_on_ft8();
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_dial(&e, 14.225, "FT8's 14.074: the phone segment default");
        // CW's 14.040 likewise, with a remembered Phone dial: the memory wins, as always.
        let mut e = general_on_ft8();
        e.set_operating_mode("phone", true);
        e.set_frequency(14.240, "20m", "USB");
        e.set_operating_mode("cw", true);
        e.set_frequency(14.040, "20m", "USB");
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_dial(&e, 14.240, "CW's 14.040: the Phone memory");
    }

    #[test]
    fn every_other_phone_entry_still_homes_as_before() {
        // The knob's 14.230 again, but the Phone tab pressed on the FT8 screen.
        let mut e = general_on_ft8();
        knob(&mut e, 14_230_000);
        e.set_operating_mode("phone", true);
        assert_dial(&e, 14.225, "FT8 screen, knob on 14.230");
        let mut e = general_on_ft8();
        e.set_operating_mode("cw", true);
        e.set_frequency(14.040, "20m", "USB");
        e.set_operating_mode("phone", true);
        assert_dial(&e, 14.225, "CW on 14.040");
        let mut e = general_on_ft8();
        e.set_operating_mode("phone", true);
        assert_dial(&e, 14.225, "FT8 on 14.074");
        let mut e = general_on_ft8();
        e.set_operating_mode("rtty", true);
        assert!(
            e.settings().dial_mhz < 14.150,
            "premise: RTTY's watering hole"
        );
        e.set_operating_mode("phone", true);
        assert_dial(&e, 14.225, "RTTY");
        // Phone → CW drops to the CW activity default, then to CW's own memory.
        let mut e = general_on_ft8();
        e.set_operating_mode("phone", true);
        e.set_operating_mode("cw", true);
        assert_dial(&e, 14.030, "Phone → CW, nothing remembered");
        e.set_frequency(14.045, "20m", "USB");
        e.set_operating_mode("phone", true);
        e.set_operating_mode("cw", true);
        assert_dial(&e, 14.045, "Phone → CW, CW's memory");
    }

    #[test]
    fn a_held_pass_still_owns_the_dial_from_the_sstv_screen() {
        // A dial Phone may not transmit on homes, and the home stands down for the pass.
        let mut e = general_on_ft8();
        e.set_frequency(14.040, "20m", "USB");
        e.sat_dial_owner = Some(("test-pass".into(), 0));
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_dial(&e, 14.040, "the pass keeps its dial");
        assert!(e.sat_dial_owner.is_some());
    }

    #[test]
    fn after_the_sstv_screen_phone_returns_where_the_operator_left_it() {
        for how in ["a channel pick", "the rig's knob"] {
            let mut e = general_on_ft8();
            if how == "a channel pick" {
                e.sstv_tune(14.230, "20m", "USB");
            } else {
                knob(&mut e, 14_230_000);
            }
            phone_tab_on_the_sstv_screen(&mut e, true);
            e.set_operating_mode("cw", true);
            e.set_operating_mode("phone", true);
            assert_dial(&e, 14.230, how);
            assert_eq!(
                e.freq_memory[&("20m".into(), OperatingMode::Phone)].0,
                14.230,
                "{how}: the Phone cell"
            );
        }
        // A tune in Phone after the SSTV screen is the memory from then on.
        let mut e = general_on_ft8();
        e.sstv_tune(14.230, "20m", "USB");
        phone_tab_on_the_sstv_screen(&mut e, true);
        e.set_frequency(14.245, "20m", "USB");
        e.set_operating_mode("cw", true);
        e.set_operating_mode("phone", true);
        assert_dial(&e, 14.245, "the tune made in Phone");
    }

    #[test]
    fn a_dial_nobody_tuned_is_kept_but_never_remembered_as_the_operators() {
        // Launched on 14.230. Kept; but the restart rule stands: a dial of unknown provenance
        // (it may be last session's machinery channel) is never handed to Phone's memory.
        let mut e = Engine::with_settings(Settings {
            band: "20m".into(),
            dial_mhz: 14.230,
            sideband: "USB".into(),
            operating_mode: OperatingMode::Digital,
            license_class: crate::settings::LicenseClass::General,
            ..Settings::default()
        });
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_dial(&e, 14.230, "launched: kept");
        e.set_operating_mode("cw", true);
        e.set_operating_mode("phone", true);
        assert_dial(&e, 14.225, "launched: not remembered");
        // The CAT link's first word, mid-session, while the operator's 40 m dial still stands.
        let mut e = general_on_ft8();
        e.set_frequency(7.074, "40m", "USB");
        e.observe_rig_freq(14_230_000);
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_dial(&e, 14.230, "the link's first word: kept");
        e.set_operating_mode("cw", true);
        e.set_operating_mode("phone", true);
        assert_dial(&e, 14.225, "the link's first word: not remembered");
    }

    #[test]
    fn a_kept_iss_downlink_is_never_remembered_as_the_operators_phone_dial() {
        let mut e = Engine::new("K1ABC", "EN50", 0);
        e.set_license_class("extra");
        e.set_operating_mode("phone", false);
        e.set_frequency(146.520, "2m", "FM"); // the operator's own 2 m phone dial
        e.set_operating_mode("digital", true);
        e.tune_channel(145.800, "2m", "FM"); // the ISS pass on the SSTV screen: the app's channel
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert_dial(&e, 145.800, "kept: Phone may transmit on 2 m");
        e.set_operating_mode("cw", true);
        e.set_operating_mode("phone", true);
        assert_dial(
            &e,
            146.520,
            "the operator's own 2 m dial, never the ISS downlink",
        );
    }

    #[test]
    fn the_phone_tab_on_the_sstv_screen_arms_as_every_entry_arms() {
        let mut e = general_on_ft8();
        knob(&mut e, 14_230_000);
        assert!(!e.tx_enabled(), "premise: FT8 is disarmed");
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert!(e.tx_enabled(), "nothing stopped: Phone arms");
        let mut e = general_on_ft8();
        knob(&mut e, 14_230_000);
        e.halt_tx();
        phone_tab_on_the_sstv_screen(&mut e, true);
        assert!(!e.tx_enabled(), "after Stop TX: still off");
        let mut e = general_on_ft8();
        knob(&mut e, 14_230_000);
        phone_tab_on_the_sstv_screen(&mut e, false);
        assert!(!e.tx_enabled(), "from afar: nothing arms");
        assert_dial(&e, 14.230, "from afar the dial stays too");
    }
}
