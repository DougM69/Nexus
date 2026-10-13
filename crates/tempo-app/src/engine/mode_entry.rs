//! Resolve an operating-section destination without changing the live station.
//!
//! Both the native setter and future Remote radio transactions must use this
//! decision. Preparation cannot bank a memory, arm TX, change a decoder or leave
//! a target for the radio loop to apply after a Remote command has expired.
//! This is the section/frequency decision only, not a CAT permission or a claim
//! that a hardware operation has completed.

use super::Engine;
use crate::settings::{KeyboardModem, OperatingMode};

#[derive(Debug, PartialEq)]
pub(super) struct ModeEntry {
    pub mode: OperatingMode,
    /// The Keyboard section's modem this entry selects: PSK for "keyboard", JTTY for "jtty", none
    /// for any other section, which leaves the modem as it was.
    pub modem: Option<KeyboardModem>,
    pub frequency: Option<(f64, String)>,
}

impl Engine {
    pub(super) fn prepare_mode_entry(&self, mode: &str, follow_frequency: bool) -> ModeEntry {
        let (mode, modem) = match mode.to_ascii_lowercase().as_str() {
            "phone" => (OperatingMode::Phone, None),
            "cw" => (OperatingMode::Cw, None),
            "rtty" => (OperatingMode::Rtty, None),
            "keyboard" => (OperatingMode::Keyboard, Some(KeyboardModem::Psk)),
            "jtty" => (OperatingMode::Keyboard, Some(KeyboardModem::Jtty)),
            _ => (OperatingMode::Digital, None),
        };
        // JTTY keeps no dial memory: it lands on WSJT-X's JTTY frequency for the band, as
        // WSJT-X does when the mode or band changes (see `jtty_channel`).
        if modem == Some(KeyboardModem::Jtty) {
            let frequency = (follow_frequency && self.sat_dial_owner.is_none())
                .then(|| self.jtty_channel(&self.settings.band))
                .flatten();
            return ModeEntry {
                mode,
                modem,
                frequency,
            };
        }
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
            modem,
            frequency,
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

    /// ⭐ THE KEYBOARD SECTION'S MODEM PICKS ITS BAND PLAN. PSK and JTTY are one section, and
    /// the modem the entry names is what differs: entering JTTY lands on WSJT-X's JTTY frequency
    /// for the band (14.090 on 20 m), entering PSK on the PSK31 watering hole (14.070), and a
    /// band picked by name lands on the plan of the modem it names (or, named by no mode, of the
    /// section the station is in).
    #[test]
    fn the_keyboard_modem_picks_the_band_plan() {
        let mut e = station();
        e.set_frequency(14.074, "20m", "USB");
        e.set_operating_mode("jtty", true);
        assert_eq!(
            (e.settings().operating_mode, e.settings().keyboard_modem),
            (OperatingMode::Keyboard, KeyboardModem::Jtty)
        );
        assert!(e.jtty_section());
        assert_eq!(e.settings().dial_mhz, 14.090, "JTTY's 20 m frequency");
        e.set_operating_mode("keyboard", true);
        assert_eq!(e.settings().keyboard_modem, KeyboardModem::Psk);
        assert!(!e.jtty_section());
        assert_eq!(
            e.settings().dial_mhz,
            14.070,
            "control: PSK31's 20 m watering hole"
        );
        e.set_operating_mode("jtty", true);
        assert_eq!(e.settings().dial_mhz, 14.090);
        e.pick_band("40m", Some("jtty"));
        assert_eq!(e.settings().dial_mhz, 7.078, "a JTTY pick by name");
        e.pick_band("15m", None);
        assert_eq!(
            e.settings().dial_mhz,
            21.090,
            "a pick naming no mode: the station's modem"
        );
        e.pick_band("20m", Some("keyboard"));
        assert_eq!(e.settings().dial_mhz, 14.070, "a PSK pick by name");
        e.set_operating_mode("phone", false);
        assert_eq!(
            e.settings().keyboard_modem,
            KeyboardModem::Jtty,
            "another section leaves the modem as it was"
        );
        assert!(!e.jtty_section());
    }

    /// JTTY keeps no dial memory, and above all never writes a JTTY dial into PSK's: both modems
    /// are the Keyboard section, and its (band, section) cell is PSK's. PSK on 14.0712, JTTY
    /// tuned off its frequency, then PSK again comes back to 14.0712, and JTTY to 14.090.
    #[test]
    fn a_jtty_dial_never_becomes_psks_memory() {
        let mut e = station();
        e.set_operating_mode("keyboard", true);
        e.set_frequency(14.0712, "20m", "USB");
        e.set_operating_mode("jtty", true);
        assert_eq!(e.settings().dial_mhz, 14.090);
        e.set_frequency(14.0915, "20m", "USB");
        e.set_operating_mode("keyboard", true);
        assert_eq!(e.settings().dial_mhz, 14.0712, "PSK's own dial");
        e.set_operating_mode("jtty", true);
        assert_eq!(
            e.settings().dial_mhz,
            14.090,
            "WSJT-X's frequency, not a memory"
        );
        e.set_frequency(14.0915, "20m", "USB");
        e.set_operating_mode("phone", true);
        e.set_operating_mode("keyboard", true);
        assert_eq!(
            e.settings().dial_mhz,
            14.0712,
            "through another section too"
        );
    }

    /// The JTTY frequency is taken only where the operator's class may run data there, judged
    /// where WSJT-X puts JTTY's transmit offset (1500 Hz up): a Technician has no 20 m data, so
    /// the dial stays put, and has 10 m data, so 28.090 is taken.
    #[test]
    fn a_jtty_frequency_outside_the_class_leaves_the_dial_put() {
        use crate::settings::LicenseClass;
        let mut e = station();
        e.settings.license_class = LicenseClass::Technician;
        e.set_frequency(14.074, "20m", "USB");
        e.set_operating_mode("jtty", true);
        assert_eq!(e.settings().dial_mhz, 14.074, "no Technician data on 20 m");
        e.set_frequency(28.074, "10m", "USB");
        e.set_operating_mode("phone", false);
        e.set_operating_mode("jtty", true);
        assert_eq!(
            e.settings().dial_mhz,
            28.090,
            "control: 10 m data is a Technician's"
        );
    }

    /// The modem is section state, kept across a Settings save as the section is: a whole-struct
    /// save from a panel loaded while the station was in PSK does not move it out of JTTY.
    #[test]
    fn a_settings_save_keeps_the_live_modem() {
        let mut e = station();
        e.set_operating_mode("keyboard", false);
        let stale = e.settings().clone();
        e.set_operating_mode("jtty", false);
        e.apply_settings(stale);
        assert_eq!(e.settings().keyboard_modem, KeyboardModem::Jtty);
        assert!(e.jtty_section());
    }
}
