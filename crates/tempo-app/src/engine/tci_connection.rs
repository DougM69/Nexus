//! A radio on the TCI connection, on the engine: what Nexus itself may transmit through an SDR
//! program's TCI server (ExpertSDR3, Thetis, piHPSDR), and what a click on a spot in that
//! program's panadapter may do. The session (`tempo_net::tci::session`) and the CAT daemon
//! (`tempo_audio::tci`) refuse at the key everything the server cannot take: a key before
//! `ready;`, while the program forbids transmit, above the drive Nexus set, a digital over on the
//! program's microphone, Nexus's audio with no route for it. This module holds the station's side:
//! what the connection asks of the engine, and what an outside program's click may never do.
//!
//! ## What the connection allows
//!
//! ⭐ EVERY SECTION TRANSMITS ON THE TCI CONNECTION, the Digital section's slot-timed modes (FT8,
//! FT4, FT2, Tempo Fast and Deep, Q65, FST4, FST4W, MSK144, JT65, WSPR and JS8) included, behind
//! every gate any connection has: TX armed, a Stop's hold, a watchdog trip, the licence on the
//! emitted RF, identity at the keying boundary. The connection adds no refusal of its own
//! ([`Engine::connection_tx_refusal`] is `None` on it, in every section). With Audio over TCI on, a
//! slot over goes to the SDR program as Nexus's own audio, which the program pulls as it sends, so
//! the mid-over message switch replaces only what it has not pulled yet; with Audio over TCI off,
//! the over keys the program's own input and plays on the sound card, as on any CAT radio.
//!
//! ⚠️ NEEDS-BENCH. An over's start on TCI is the program's first pull after the key, which no bench
//! has timed yet, and the switch on the TCI route has met only the simulator. The release waits for
//! the operator's bench of FT8 and FT4 over TCI. Should the bench refuse it, holding the Digital
//! section again is one arm of `connection_tx_refusal_as`, which already judges the section asked.
//!
//! The keys are the session's to refuse: Phone at the shack keys the program's own microphone and
//! Tune its own carrier; with Audio over TCI on, Nexus's audio (the slot-timed modes, RTTY, PSK,
//! SSTV, APRS, the voice keyer) goes out only once the program has taken Nexus's audio stream, and
//! never on its microphone; CW through the program's own keyer is not built yet. Nothing here
//! repeats those rules, and nothing here admits what they refuse. Programs' overs through Nexus's
//! own TCI server are a separate rule (`engine/tci.rs`): their slot-timed modes stay refused there.
//!
//! ## A click on a spot in the program's panadapter
//!
//! When the operator clicks a spot on the SDR program's panadapter, the program tells its clients
//! (`clicked_on_spot`, `rx_clicked_on_spot`) and the session hands the click on. It arrives from
//! outside, so [`Engine::tci_spot_clicked`] tunes and does nothing else: the dial goes to the
//! spot, where Nexus's own spot click puts it, in the section the operator is in, on the TCI radio
//! itself. It never starts a QSO, never arms TX, never clears a Stop's hold, a watchdog trip or a
//! band change's hold, never starts or restarts the watchdog clock, never changes the section, and
//! never moves the dial while anything is on the air. Nor does it ask the window to open a cockpit,
//! as a Work click does: the window's entry to a Phone, CW, RTTY or PSK cockpit arms TX where
//! nothing holds it. A band change it makes halts what a band change halts.

use super::Engine;

/// What a click on a spot in the SDR program's panadapter could not do, for the connection log.
const SPOT_NOT_ON_TCI: &str = "spot click ignored: the radio in use is not on the TCI connection";
const SPOT_ON_AIR: &str =
    "spot click ignored: Nexus never moves the dial for one while anything is on the air";
const SPOT_NO_CALL: &str = "spot click ignored: it names no callsign";
const SPOT_OFF_BAND: &str = "spot click ignored: its frequency is on no amateur band";

/// The longest callsign a click may name, as a cluster writes them (`VP2V/K1ABC/P` is 12).
const SPOT_CALL_MAX: usize = 20;

impl Engine {
    /// The SDR program's operator clicked the spot `call` at `hz` on its panadapter: tune the TCI
    /// radio to it, as Nexus's own spot click places the dial, and nothing else (the module
    /// header). `Err` says why the click changed nothing, for the caller to log.
    pub fn tci_spot_clicked(&mut self, call: &str, hz: i64) -> Result<(), &'static str> {
        if !crate::settings::rig_conn_is_tci(&self.settings.rig_conn) {
            return Err(SPOT_NOT_ON_TCI);
        }
        self.tune_to_clicked_spot(call, hz, "a spot clicked in the SDR program")
    }

    /// Tune to the spot `call` at `hz` that the operator clicked, where Nexus's own spot click
    /// places the dial, and do nothing else (the module header's list); `what` names the click for
    /// the log. A program's spot clicked on Nexus's band map comes here too
    /// (`Engine::tci_program_spot_clicked`).
    pub(super) fn tune_to_clicked_spot(
        &mut self,
        call: &str,
        hz: i64,
        what: &str,
    ) -> Result<(), &'static str> {
        if self.on_air() {
            return Err(SPOT_ON_AIR);
        }
        let call = call.trim().to_ascii_uppercase();
        if call.len() > SPOT_CALL_MAX || !tempo_core::message::looks_like_call(&call) {
            return Err(SPOT_NO_CALL);
        }
        let spot_mhz = hz as f64 / 1e6;
        let band = crate::bandplan::band_for_dial(spot_mhz).ok_or(SPOT_OFF_BAND)?;
        // The radio in use, whose panadapter or band map it is: the band's routing never hands the
        // click to another.
        self.route_target = Some(None);
        // A soundcard CW keyer radiates a pitch off its dial: the dial sits a pitch off the spot.
        let dial = spot_mhz - self.soundcard_cw_pitch_offset_mhz(spot_mhz);
        self.set_frequency(dial, band, "USB");
        tempo_core::applog::info(
            "cat",
            &format!("TCI: tuned to {call} at {spot_mhz:.6} MHz, {what}"),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::Tier;
    use crate::engine::now_unix_secs;
    use crate::settings::{RadioProfile, Settings, ICOM_LAN_TX_REFUSED};

    /// A General's station at launch on one radio, the TCI connection's own model (7), on `conn`:
    /// "tci", or "serial" as the control. TX is off and nothing holds it. The CAT share may key, so
    /// a program's PTT through it is a path too.
    fn on(conn: &str) -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let profile = RadioProfile {
            id: 0,
            rig_model: 7,
            rig_conn: conn.into(),
            ..Default::default()
        };
        e.apply_restored_settings(Settings {
            // The station's identity, which the FT paths validate at the keying boundary.
            mycall: "W9XYZ".into(),
            mygrid: "EN61".into(),
            rig_model: 7,
            rig_conn: conn.into(),
            cat_broker_ptt: true,
            radios: vec![profile],
            active_radio: 0,
            ..Settings::default()
        });
        e.set_license_class("general");
        assert!(!e.tx_enabled(), "premise: TX is off at launch");
        e
    }

    /// [`on`], in `section` at `mhz` on 20 m, TX armed by the operator.
    fn at(conn: &str, section: &str, mhz: f64) -> Engine {
        let mut e = on(conn);
        e.set_operating_mode(section, false);
        e.set_frequency(mhz, "20m", "USB");
        e.set_tx_enabled_by_operator(true);
        e
    }

    /// [`on`] at `tier`, its CQ running as Call CQ starts it (TX armed).
    fn cq_run(conn: &str, tier: Tier) -> Engine {
        let mut e = on(conn);
        e.set_tier(tier);
        e.set_mode("qso-run").expect("Call CQ");
        assert!(e.tx_enabled(), "premise: Call CQ arms TX");
        e
    }

    /// The first JS8 slot after now.
    fn next_js8_slot(e: &Engine) -> u64 {
        let period_ms = u64::from(e.js8_tx_speed().period_s()) * 1000;
        tempo_core::timing::now_unix_ms() as u64 / period_ms + 1
    }

    /// ⭐ THE SLOT-TIMED MODES TRANSMIT ON THE TCI CONNECTION AS ON ANY OTHER (NEEDS-BENCH, the
    /// module header). In the Digital section the verdict, the snapshot's lock and reason, Call CQ
    /// on FT8 and FT4 (the slot plans the CQ), a JS8 message (its slot plans it) and Tune go ahead
    /// on TCI exactly as on the same radio's serial connection. CONTROL: on the Icom network
    /// connection every one of them is refused, in that connection's own words, so the paths
    /// measured here are the ones a connection's refusal stops. Every path is measured before
    /// anything is asserted, so a red names them all.
    #[test]
    fn the_slot_timed_modes_transmit_on_the_tci_connection_as_on_serial() {
        let mut wrong = Vec::new();
        for (conn, refused) in [
            ("serial", None),
            ("tci", None),
            ("icomlan", Some(ICOM_LAN_TX_REFUSED)),
        ] {
            let goes = refused.is_none();
            let mut check = |what: &str, got: String, want: String| {
                if got != want {
                    wrong.push(format!("{conn}: {what}: got {got}, want {want}"));
                }
            };

            let e = at(conn, "digital", 14.074);
            check(
                "the verdict",
                format!("{}", e.tx_allowed()),
                format!("{goes}"),
            );
            let radio = e.snapshot().radio;
            check(
                "the snapshot's lock",
                format!("{}", radio.tx_allowed),
                format!("{goes}"),
            );
            check(
                "the snapshot's reason",
                format!("{:?}", radio.tx_refusal),
                format!("{refused:?}"),
            );
            check(
                "the connection's reason",
                format!("{:?}", e.connection_tx_refusal()),
                format!("{refused:?}"),
            );

            // FT8 and FT4: Call CQ arms, and the slot plans the CQ.
            for tier in [Tier::Ft8, Tier::Ft4] {
                let mut e = cq_run(conn, tier);
                check(
                    &format!("{tier:?}: the CQ keys"),
                    format!("{}", !e.poll_tx(0).is_empty()),
                    format!("{goes}"),
                );
            }

            // JS8: the message's slot plans it.
            let mut e = on(conn);
            e.js8_enter();
            e.set_tx_offset(1500.0);
            e.set_tx_enabled_by_operator(true);
            e.js8_send(None, "HELLO FROM TCI".into()).expect("queues");
            let slot = next_js8_slot(&e);
            check(
                "JS8: the slot keys",
                format!("{}", e.plan_tx(slot).is_some()),
                format!("{goes}"),
            );

            // Tune in the Digital section.
            let mut e = at(conn, "digital", 14.074);
            e.set_tune(true);
            check(
                "Digital: Tune",
                format!("{}", e.tuning()),
                format!("{goes}"),
            );
        }
        assert!(
            wrong.is_empty(),
            "the slot-timed modes on TCI:\n{}",
            wrong.join("\n")
        );
    }

    /// The JS8 cockpit's line, when its queue is dropped because the connection refuses the
    /// section, names that connection, as CW's notice does: the Icom network's words. The TCI
    /// connection drops nothing, and outside the licence the line names the licence there, as on
    /// any connection. CONTROLS: on a serial radio nothing is refused, and outside the licence the
    /// line names it.
    #[test]
    fn the_js8_line_names_the_connection_not_the_licence() {
        let line = |conn: &str, mhz: f64| {
            let mut e = on(conn);
            e.js8_enter();
            e.set_frequency(mhz, "20m", "USB");
            e.set_tx_offset(1500.0);
            e.set_tx_enabled_by_operator(true);
            e.js8_send(None, "HELLO FROM TCI".into()).expect("queues");
            e.js8_tick(tempo_core::timing::now_unix_ms() as u64);
            e.js8_state().last_error
        };
        assert_eq!(
            line("tci", 14.078),
            None,
            "the TCI connection drops nothing"
        );
        assert_eq!(
            line("icomlan", 14.078).as_deref(),
            Some(ICOM_LAN_TX_REFUSED)
        );
        assert_eq!(line("serial", 14.078), None, "control: nothing refused");
        // A General's 20 m data starts at 14.025.
        for conn in ["serial", "tci"] {
            let outside = line(conn, 14.010).unwrap_or_default();
            assert!(
                outside.contains("license privileges"),
                "{conn}: outside the licence the line names it: {outside:?}"
            );
        }
    }

    /// ⭐ AND EVERY OTHER SECTION: on the TCI connection every other section's transmit path goes
    /// ahead in the engine exactly as on the serial one (the TCI session refuses at the key what it
    /// cannot carry yet, in its own words), and nothing names the TCI connection there.
    #[test]
    fn every_other_section_transmits_on_the_tci_connection_as_on_serial() {
        let mut wrong = Vec::new();
        for conn in ["serial", "tci"] {
            let mut check = |what: &str, ok: bool| {
                if !ok {
                    wrong.push(format!("{conn}: {what}"));
                }
            };
            let said = |r: Result<(), String>| r.err();

            for (section, mhz) in [
                ("phone", 14.250),
                ("cw", 14.030),
                ("rtty", 14.085),
                ("keyboard", 14.070),
            ] {
                let e = at(conn, section, mhz);
                check(&format!("{section}: the verdict"), e.tx_allowed());
                check(
                    &format!("{section}: no refusal"),
                    e.snapshot().radio.tx_refusal.is_none(),
                );
            }

            // Phone: PTT, Tune, the voice keyer, SSTV, a program's PTT through the CAT share.
            let mut e = at(conn, "phone", 14.250);
            e.set_ptt(true);
            check("Phone: PTT", e.manual_ptt());
            let mut e = at(conn, "phone", 14.250);
            e.set_tune(true);
            check("Phone: Tune", e.tuning());
            let mut e = at(conn, "phone", 14.250);
            check(
                "Phone: voice keyer",
                said(e.send_voice(vec![0.1, 0.2])).is_none(),
            );
            let mut e = at(conn, "phone", 14.230);
            check(
                "Phone: SSTV",
                said(e.sstv_send(vec![0.0f32; 12_000], "PD-120".into())).is_none(),
            );
            let mut e = at(conn, "phone", 14.250);
            check("Phone: CAT share PTT", e.broker_ptt(true));
            let mut e = at(conn, "phone", 14.250);
            check(
                "Phone: APRS",
                said(e.aprs_beacon(41.9, -87.6, '/', '>', "test", &[])).is_none(),
            );

            // CW, RTTY, PSK.
            let mut e = at(conn, "cw", 14.030);
            e.send_cw("TEST");
            check("CW: keyed", e.poll_cw_one().is_some());
            check("CW: no notice", e.cw_keyer_error().is_none());
            let mut e = at(conn, "rtty", 14.085);
            check("RTTY", said(e.rtty_send_text("CQ")).is_none());
            let mut e = at(conn, "keyboard", 14.070);
            check("PSK", said(e.psk_send_text("CQ")).is_none());
        }
        assert!(
            wrong.is_empty(),
            "refused on the TCI connection:\n{}",
            wrong.join("\n")
        );
    }

    /// ⛔ NO GATE GIVES WAY TO THE TCI CONNECTION. On it, FT8 in the Digital section keys nothing
    /// after Stop TX, with TX Off, after a watchdog trip, outside the licence (in the licence's
    /// words, not the connection's) or without the station's grid. In Phone the licence still
    /// refuses, TX Off still refuses, and a Stop's hold and a watchdog trip still keep the section
    /// from arming. The Icom network connection keeps its own refusal, in every section. CONTROLS:
    /// the same CQ with nothing in the way keys, inside the licence and with the grid; the same
    /// Phone station inside the licence keys.
    #[test]
    fn every_gate_still_holds_on_the_tci_connection() {
        type Gate = fn(&mut Engine);
        let gates: [(&str, Gate, bool); 4] = [
            ("nothing in the way", |_| {}, true),
            ("Stop TX", |e| e.halt_tx(), false),
            ("TX Off", |e| e.set_tx_enabled_by_operator(false), false),
            ("a watchdog trip", |e| e.trip_tx_watchdog_for_test(), false),
        ];
        let mut wrong = Vec::new();
        for (what, gate, keys) in gates {
            let mut e = cq_run("tci", Tier::Ft8);
            gate(&mut e);
            if e.poll_tx(0).is_empty() == keys {
                wrong.push(format!("FT8 on TCI, {what}: the CQ keys {}", !keys));
            }
        }
        // The licence: a General's 20 m data starts at 14.025.
        for (mhz, inside) in [(14.080, true), (14.010, false)] {
            let mut e = on("tci");
            e.set_tier(Tier::Ft8);
            e.set_frequency(mhz, "20m", "USB");
            e.set_mode("qso-run").expect("Call CQ");
            assert!(
                (e.settings.dial_mhz - mhz).abs() < 1e-9,
                "premise: the dial at {mhz}"
            );
            let (allowed, refusal) = (e.tx_allowed(), e.snapshot().radio.tx_refusal);
            let keyed = !e.poll_tx(0).is_empty();
            if (allowed, keyed, refusal.as_deref()) != (inside, inside, None) {
                wrong.push(format!(
                    "FT8 on TCI at {mhz}: the verdict {allowed}, keyed {keyed}, a connection's \
                     words {refusal:?}"
                ));
            }
        }
        // Identity at the keying boundary: no CQ without the station's grid.
        for (grid, keys) in [("EN61", true), ("", false)] {
            let mut e = on("tci");
            e.set_tier(Tier::Ft8);
            e.settings.mygrid = grid.into();
            let keyed = e.set_mode("qso-run").is_ok() && !e.poll_tx(0).is_empty();
            if keyed != keys {
                wrong.push(format!("FT8 on TCI, grid {grid:?}: the CQ keys {keyed}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));

        let mut e = at("tci", "phone", 14.200);
        assert!(!e.tx_allowed(), "a General's phone below 14.225");
        assert_eq!(
            e.snapshot().radio.tx_refusal,
            None,
            "the licence's words, not the connection's"
        );
        e.set_ptt(true);
        assert!(!e.manual_ptt(), "PTT outside the licence");

        let mut e = at("tci", "phone", 14.250);
        e.set_ptt(true);
        assert!(e.manual_ptt(), "control: inside the licence PTT keys");

        let mut e = at("tci", "phone", 14.250);
        e.set_tx_enabled_by_operator(false);
        e.set_ptt(true);
        assert!(!e.manual_ptt(), "PTT with TX off");

        let mut e = at("tci", "phone", 14.250);
        e.halt_tx();
        e.set_operating_mode("cw", false);
        e.set_operating_mode("phone", false);
        assert!(!e.tx_enabled(), "a Stop's hold on the TCI connection");

        let mut e = at("tci", "phone", 14.250);
        e.trip_tx_watchdog_for_test();
        e.set_operating_mode("cw", false);
        e.set_operating_mode("phone", false);
        assert!(!e.tx_enabled(), "a watchdog trip on the TCI connection");

        for section in ["digital", "phone", "cw"] {
            let mut e = on("icomlan");
            e.set_operating_mode(section, false);
            assert_eq!(
                e.connection_tx_refusal(),
                Some(ICOM_LAN_TX_REFUSED),
                "the Icom network connection in {section}"
            );
        }
    }

    /// The refusal reads the radio in use, now: across a serial radio, a TCI one and one on the
    /// Icom network, in Digital and in Phone, only the Icom network radio is refused, and a switch
    /// away from it clears its words from the snapshot.
    #[test]
    fn the_refusal_follows_the_radio_in_use() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        let radio = |id: u32, conn: &str| RadioProfile {
            id,
            rig_model: 7,
            rig_conn: conn.into(),
            ..Default::default()
        };
        e.apply_restored_settings(Settings {
            mycall: "W9XYZ".into(),
            mygrid: "EN61".into(),
            rig_model: 7,
            rig_conn: "serial".into(),
            radios: vec![radio(0, "serial"), radio(1, "tci"), radio(2, "icomlan")],
            active_radio: 0,
            ..Settings::default()
        });
        e.set_operating_mode("digital", false);
        let mut seen = Vec::new();
        let mut note = |e: &Engine, step: &str| {
            seen.push((step.to_string(), e.snapshot().radio.tx_refusal.is_some()));
        };
        note(&e, "serial, Digital");
        e.set_active_radio(1);
        note(&e, "TCI, Digital");
        e.set_operating_mode("phone", false);
        note(&e, "TCI, Phone");
        e.set_active_radio(2);
        note(&e, "Icom network, Phone");
        e.set_operating_mode("digital", false);
        note(&e, "Icom network, Digital");
        e.set_active_radio(1);
        note(&e, "TCI, Digital again");
        e.set_active_radio(0);
        note(&e, "serial, Digital again");
        let want = [
            ("serial, Digital", false),
            ("TCI, Digital", false),
            ("TCI, Phone", false),
            ("Icom network, Phone", true),
            ("Icom network, Digital", true),
            ("TCI, Digital again", false),
            ("serial, Digital again", false),
        ]
        .map(|(s, r)| (s.to_string(), r));
        assert_eq!(seen, want);
    }

    /// ⛔ THE GATE JUDGES THE SECTION ASKED, NOT THE ONE THE STATION IS IN, on the TCI connection
    /// as on the serial one. A General in Phone at 14.074 asked as Digital gets the data carrier's
    /// answer, and in Digital at 14.250 asked as Phone the phone answer: inside the privileges,
    /// both. CONTROLS: the same stations asked as the section they are in are refused (a General's
    /// phone starts at 14.225, its data ends at 14.150), so each answer is the section asked.
    #[test]
    fn the_gate_judges_the_section_asked_not_the_section_the_station_is_in() {
        use crate::settings::OperatingMode::{Cw, Digital, Phone};
        let mut seen = Vec::new();
        for conn in ["serial", "tci"] {
            // A General's data carrier above 14.074 is inside the privileges, and so is CW there.
            let e = at(conn, "phone", 14.074);
            seen.push((conn, "in Phone, asked as Digital", e.tx_allowed_as(Digital)));
            seen.push((conn, "in Phone, asked as Phone", e.tx_allowed_as(Phone)));
            let e = at(conn, "digital", 14.074);
            seen.push((conn, "in Digital, asked as CW", e.tx_allowed_as(Cw)));
            // A General's phone at 14.250 is inside the privileges.
            let e = at(conn, "digital", 14.250);
            seen.push((conn, "in Digital, asked as Phone", e.tx_allowed_as(Phone)));
            seen.push((
                conn,
                "in Digital, asked as Digital",
                e.tx_allowed_as(Digital),
            ));
        }
        let want = [
            ("serial", "in Phone, asked as Digital", true),
            ("serial", "in Phone, asked as Phone", false),
            ("serial", "in Digital, asked as CW", true),
            ("serial", "in Digital, asked as Phone", true),
            ("serial", "in Digital, asked as Digital", false),
            ("tci", "in Phone, asked as Digital", true),
            ("tci", "in Phone, asked as Phone", false),
            ("tci", "in Digital, asked as CW", true),
            ("tci", "in Digital, asked as Phone", true),
            ("tci", "in Digital, asked as Digital", false),
        ];
        assert_eq!(seen, want);
    }

    /// ⛔ THE LIFT IS NEXUS'S OWN: with the station's radio on the TCI connection, in the Digital
    /// section, a program's over through Nexus's own TCI server is still refused as slot-timed (the
    /// server's rule, `engine/tci.rs`), and its `tx_enable` says so. CONTROL: the same program's
    /// over in Phone is admitted, once it asked a voice mode (a program that asked none is refused
    /// there).
    #[test]
    fn programs_slot_timed_overs_through_nexus_own_tci_server_stay_refused_on_a_tci_radio() {
        use tempo_net::tci::server::{KeyKind, Refusal};
        use tempo_net::tci::wire::Word;
        let mut e = at("tci", "digital", 14.074);
        e.settings.tci_server_tx = true;
        assert!(e.tx_allowed(), "premise: Nexus's own FT8 may transmit here");
        assert_eq!(e.tci_key(1, KeyKind::Tci, true), Err(Refusal::SlotTimed));
        assert!(!e.tci_tx_enable(1, true), "told tx_enable true");
        assert!(!e.tci_over_armed(), "a program's over armed");
        let mut e = at("tci", "phone", 14.250);
        e.settings.tci_server_tx = true;
        e.tci_note_modulation(1, &Word::new("usb").expect("a mode word"));
        assert_eq!(e.tci_key(1, KeyKind::Tci, true), Ok(()), "control: Phone");
    }

    // ── A click on a spot in the SDR program's panadapter ─────────────────────────────────────

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Holds {
        tx_enabled: bool,
        stop_hold: bool,
        watchdog: bool,
        context_hold: bool,
        watchdog_start: Option<u64>,
    }

    fn holds(e: &Engine) -> Holds {
        Holds {
            tx_enabled: e.tx_enabled,
            stop_hold: e.tx_stop_hold,
            watchdog: e.tx_watchdog,
            context_hold: e.broker_context_hold,
            watchdog_start: e.tx_watchdog_start,
        }
    }

    /// What a click may do to them: lower TX (a band change's halt), set a hold, never the reverse,
    /// and never touch the watchdog clock.
    fn arms_or_clears(before: Holds, after: Holds) -> bool {
        (!before.tx_enabled && after.tx_enabled)
            || (before.stop_hold && !after.stop_hold)
            || (before.watchdog && !after.watchdog)
            || (before.context_hold && !after.context_hold)
            || before.watchdog_start != after.watchdog_start
    }

    type State = (&'static str, Box<dyn Fn() -> Engine>);

    /// The states a click can find a radio on `conn` in.
    fn states(conn: &'static str) -> Vec<State> {
        vec![
            ("launch, TX off", Box::new(move || on(conn))),
            (
                "Digital, a CQ run armed",
                Box::new(move || cq_run(conn, Tier::Ft8)),
            ),
            ("Phone, armed", Box::new(move || at(conn, "phone", 14.250))),
            (
                "Phone, armed, the watchdog clock running",
                Box::new(move || {
                    let mut e = at(conn, "phone", 14.250);
                    e.tx_watchdog_start = Some(now_unix_secs() - 30);
                    e
                }),
            ),
            (
                "after Stop TX",
                Box::new(move || {
                    let mut e = at(conn, "phone", 14.250);
                    e.halt_tx();
                    e
                }),
            ),
            (
                "after a watchdog trip",
                Box::new(move || {
                    let mut e = at(conn, "phone", 14.250);
                    e.trip_tx_watchdog_for_test();
                    e
                }),
            ),
            (
                "after a band change",
                Box::new(move || {
                    let mut e = at(conn, "phone", 14.250);
                    e.set_frequency(7.200, "40m", "LSB");
                    e
                }),
            ),
            ("CW, armed", Box::new(move || at(conn, "cw", 14.030))),
        ]
    }

    /// The clicks: on the band the radio is on, and on another.
    const CLICKS: [(&str, i64); 2] = [("K1ABC", 14_025_000), ("VP2V/K1ABC", 7_030_000)];

    /// A click, as a verb on the engine.
    type Click = fn(&mut Engine, &str, i64) -> Result<(), &'static str>;

    /// Every click from every state of a radio on each of `conns`: what each moved that a click
    /// may not move, and how many of how many tuned to their spot.
    fn census(conns: &[&'static str], click: Click) -> (Vec<String>, usize, usize) {
        let mut moved = Vec::new();
        let (mut tuned, mut clicks) = (0, 0);
        for &conn in conns {
            for (state, build) in states(conn) {
                for (call, hz) in CLICKS {
                    clicks += 1;
                    let mut e = build();
                    let (before, section, tick) =
                        (holds(&e), e.settings.operating_mode, e.snapshot().work_tick);
                    let r = click(&mut e, call, hz);
                    let after = holds(&e);
                    let what = format!("{conn}, {state}: {call} at {hz} Hz ({r:?})");
                    if arms_or_clears(before, after) {
                        moved.push(format!("{what}: {before:?} -> {after:?}"));
                    }
                    if let Some(dx) = e.qso_dxcall() {
                        moved.push(format!("{what}: a QSO with {dx} started"));
                    }
                    if e.settings.operating_mode != section {
                        moved.push(format!(
                            "{what}: the section moved to {:?}",
                            e.settings.operating_mode
                        ));
                    }
                    if e.snapshot().work_tick != tick {
                        moved.push(format!("{what}: the window was asked to open a cockpit"));
                    }
                    tuned += usize::from(
                        r.is_ok() && (e.settings.dial_mhz - hz as f64 / 1e6).abs() < 1e-9,
                    );
                }
            }
        }
        (moved, tuned, clicks)
    }

    /// ⛔ A CLICK ON A SPOT TUNES AND NEVER TRANSMITS. From every state and for every click: no QSO
    /// starts, TX is never armed, no Stop's hold, watchdog trip or band change's hold is cleared,
    /// the watchdog clock never moves, the section stays, and the window is not asked to open a
    /// cockpit. CONTROL: the click did tune, every time, to the spot.
    #[test]
    fn a_click_on_a_spot_in_the_sdr_panadapter_tunes_and_never_transmits() {
        let (moved, tuned, clicks) = census(&["tci"], Engine::tci_spot_clicked);
        assert!(
            moved.is_empty(),
            "a spot click moved them:\n{}",
            moved.join("\n")
        );
        assert_eq!(tuned, clicks, "control: every click tuned to its spot");
    }

    /// A click on a program's spot, with programs' spots on so the programs are told of it: told
    /// once when it tuned, never when it was refused.
    fn program_click(e: &mut Engine, call: &str, hz: i64) -> Result<(), &'static str> {
        e.settings.tci_server = true;
        e.settings.tci_server_spots = true;
        let r = e.tci_program_spot_clicked(call, hz);
        assert_eq!(
            e.take_tci_spot_clicks().len(),
            usize::from(r.is_ok()),
            "{call}: told once it tuned ({r:?})"
        );
        r
    }

    /// ⛔ A CLICK ON A PROGRAM'S SPOT ON NEXUS'S BAND MAP ONLY TUNES, the same census, on a radio
    /// on any connection: Nexus's own TCI server serves whichever radio is in use. It never calls
    /// the station, arms, starts a QSO, clears a hold, moves the section or opens a cockpit, and
    /// telling the programs of it changes none of that. CONTROL: every click tuned to its spot.
    #[test]
    fn a_click_on_a_programs_spot_on_the_band_map_only_tunes() {
        let (moved, tuned, clicks) = census(&["tci", "serial"], program_click);
        assert!(
            moved.is_empty(),
            "a click on a program's spot moved them:\n{}",
            moved.join("\n")
        );
        assert_eq!(tuned, clicks, "control: every click tuned to its spot");
        assert_eq!(clicks, 32, "control: 8 states, 2 clicks, 2 connections");
    }

    /// ⛔ NOT WHILE ANYTHING IS ON THE AIR, NOT OFF THE BANDS, NOT WITHOUT A CALLSIGN: a click on a
    /// program's spot is refused as a click in an SDR program's panadapter is, and the dial stays.
    /// CONTROL: the same click with nothing in the way tunes.
    #[test]
    fn a_click_on_a_programs_spot_is_refused_where_a_panadapter_click_is() {
        type Setup = fn(&mut Engine);
        let cases: [(&str, Setup, &str, i64, &str); 4] = [
            (
                "PTT held",
                |e| e.set_ptt(true),
                "K1ABC",
                14_025_000,
                SPOT_ON_AIR,
            ),
            (
                "tuning",
                |e| e.set_tune(true),
                "K1ABC",
                14_025_000,
                SPOT_ON_AIR,
            ),
            ("off the bands", |_| {}, "K1ABC", 15_000_000, SPOT_OFF_BAND),
            ("not a call", |_| {}, "HELLO", 14_025_000, SPOT_NO_CALL),
        ];
        let mut wrong = Vec::new();
        for (what, setup, call, hz, why) in cases {
            let mut e = at("serial", "phone", 14.250);
            setup(&mut e);
            let dial = e.settings.dial_mhz;
            let r = e.tci_program_spot_clicked(call, hz);
            if r != Err(why) || e.settings.dial_mhz != dial {
                wrong.push(format!(
                    "{what}: {r:?}, the dial {dial} -> {}",
                    e.settings.dial_mhz
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        let mut e = at("serial", "phone", 14.250);
        assert_eq!(
            e.tci_program_spot_clicked("k1abc", 14_225_000),
            Ok(()),
            "control"
        );
        assert!(
            (e.settings.dial_mhz - 14.225).abs() < 1e-9,
            "control: tuned"
        );
    }

    /// The click on a TCI radio in CW with the soundcard keyer: the dial sits a pitch below the
    /// spot on the upper side, where Nexus's own spot click puts it, so the signal lands on the
    /// station.
    #[test]
    fn a_click_in_soundcard_cw_puts_the_dial_a_pitch_off_the_spot() {
        let mut e = at("tci", "cw", 14.030);
        e.settings.cw_keyer = crate::settings::CwKeyerBackend::Soundcard;
        let pitch = f64::from(e.cw_pitch_hz()) / 1e6;
        assert!(pitch > 0.0, "premise: a CW pitch");
        assert_eq!(e.tci_spot_clicked("K1ABC", 14_025_000), Ok(()));
        assert!(
            (e.settings.dial_mhz - (14.025 - pitch)).abs() < 1e-9,
            "the dial: {} MHz",
            e.settings.dial_mhz
        );
    }

    /// The click goes to the radio whose panadapter it is: a second radio the band is routed to
    /// does not take it. CONTROL: the operator's own QSY to the same band is routed there.
    #[test]
    fn a_click_stays_on_the_tci_radio_whatever_the_band_routing_says() {
        let two_radios = || {
            let mut e = Engine::new("W9XYZ", "EN61", 0);
            let tci = RadioProfile {
                id: 0,
                rig_model: 7,
                rig_conn: "tci".into(),
                bands: vec!["20m".into()],
                ..Default::default()
            };
            let other = RadioProfile {
                id: 1,
                rig_model: 1042,
                rig_conn: "serial".into(),
                bands: vec!["40m".into()],
                ..Default::default()
            };
            e.apply_restored_settings(Settings {
                mycall: "W9XYZ".into(),
                mygrid: "EN61".into(),
                rig_model: 7,
                rig_conn: "tci".into(),
                radios: vec![tci, other],
                active_radio: 0,
                ..Settings::default()
            });
            e.set_operating_mode("cw", false);
            e.set_frequency(14.030, "20m", "USB");
            assert_eq!(e.settings.active_radio, 0, "premise: on the TCI radio");
            e
        };
        let mut control = two_radios();
        control.set_frequency(7.030, "40m", "USB");
        assert_eq!(
            control.settings.active_radio, 1,
            "control: the operator's own QSY to 40 m is routed to the other radio"
        );
        let mut e = two_radios();
        assert_eq!(e.tci_spot_clicked("K1ABC", 7_030_000), Ok(()));
        assert_eq!(
            (e.settings.active_radio, e.settings.band.as_str()),
            (0, "40m"),
            "the click left the TCI radio"
        );
    }

    /// ⛔ NOT WHILE ANYTHING IS ON THE AIR, NOT OFF THE TCI CONNECTION, NOT OFF THE BANDS, NOT
    /// WITHOUT A CALLSIGN: each such click is refused, says why, and leaves the dial where it was.
    /// CONTROL: the same click, with nothing in the way, tunes.
    #[test]
    fn a_click_that_cannot_be_a_spot_on_this_radio_changes_nothing() {
        type Setup = fn(&mut Engine);
        let cases: [(&str, Setup, &str, i64, &str); 9] = [
            (
                "tuning",
                |e| e.set_tune(true),
                "K1ABC",
                14_025_000,
                SPOT_ON_AIR,
            ),
            (
                "PTT held",
                |e| e.set_ptt(true),
                "K1ABC",
                14_025_000,
                SPOT_ON_AIR,
            ),
            (
                "on a serial radio",
                |e| e.settings.rig_conn = "serial".into(),
                "K1ABC",
                14_025_000,
                SPOT_NOT_ON_TCI,
            ),
            ("off the bands", |_| {}, "K1ABC", 15_000_000, SPOT_OFF_BAND),
            (
                "a negative frequency",
                |_| {},
                "K1ABC",
                -14_025_000,
                SPOT_OFF_BAND,
            ),
            ("no call", |_| {}, " ", 14_025_000, SPOT_NO_CALL),
            ("not a call", |_| {}, "HELLO", 14_025_000, SPOT_NO_CALL),
            ("a command", |_| {}, "TRX:0,TRUE", 14_025_000, SPOT_NO_CALL),
            (
                "too long",
                |_| {},
                "K1ABC/AAAAAAAAAAAAAAAAAAAAAAAA",
                14_025_000,
                SPOT_NO_CALL,
            ),
        ];
        let mut wrong = Vec::new();
        for (what, setup, call, hz, why) in cases {
            let mut e = at("tci", "phone", 14.250);
            setup(&mut e);
            assert_eq!(
                e.on_air(),
                why == SPOT_ON_AIR,
                "premise: {what} puts something on the air, or nothing"
            );
            let dial = e.settings.dial_mhz;
            let r = e.tci_spot_clicked(call, hz);
            if r != Err(why) || e.settings.dial_mhz != dial {
                wrong.push(format!(
                    "{what}: {r:?}, the dial {dial} -> {}",
                    e.settings.dial_mhz
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        let mut e = at("tci", "phone", 14.250);
        assert_eq!(e.tci_spot_clicked("k1abc", 14_225_000), Ok(()), "control");
        assert!(
            (e.settings.dial_mhz - 14.225).abs() < 1e-9,
            "control: tuned"
        );
    }
}
