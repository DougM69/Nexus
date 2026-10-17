//! JS8's TX delay, the engine's half: how early a JS8 frame keys and how late it may still start.
//!
//! JS8Call 3.0.3 asks for PTT `TxDelay` before a frame's period begins (`prepareSending`,
//! `JS8_UI/mainwindow.cpp:2507-2572`: `time_is_in_tx_delay`, :2512-2516), 0.2 s by default
//! (`JS8_UI/Configuration.cpp:2275`), 0.1 to 0.5 s on its spin box (`JS8_UI/Configuration.ui`,
//! `sbTxDelay`: "Minimum delay between assertion of PTT and start of Tx audio."). A frame it has
//! not keyed by then it still starts while the period is under its late threshold (:2533-2546),
//! and never after. The radio loop keys and lays the frame on the slot clock
//! (`tempo_audio::js8_slot`); the engine says how early and how late.
//!
//! Behaviour, not code: what JS8Call does was read from its source and is done here in Nexus's
//! own shapes.

use super::super::Engine;
use modes::Js8Speed;

/// The shortest TX delay JS8Call's spin box takes, in ms (`sbTxDelay` minimum 0.1 s).
pub const JS8_TX_DELAY_MIN_MS: u32 = 100;
/// …and the longest (`sbTxDelay` maximum 0.5 s).
pub const JS8_TX_DELAY_MAX_MS: u32 = 500;

/// How far into its period, in ms, JS8Call still starts a frame it has not keyed in its TX delay:
/// the period's dead air (the period less the 79 symbols) less the TX delay, all of it at Slow and
/// Normal, three quarters at Fast, half at JS8 40 and JS8 60 (`lateThreshold`,
/// mainwindow.cpp:2533-2546: `computeRatio` less `txDelay / period`, times 0.75 for
/// `JS8CallFast`, 0.5 for `JS8CallTurbo` and `JS8CallUltra`; a start keys while
/// `fraction_of_tx_slot < lateThreshold`). At 0.2 s: Slow 4.52 s, Normal 2.16 s, Fast 1.425 s,
/// JS8 40 0.925 s, JS8 60 0.636 s.
pub fn js8_late_start_ms(speed: Js8Speed, tx_delay_ms: f64) -> f64 {
    let period_ms = f64::from(speed.period_s()) * 1000.0;
    let symbols_ms = (79 * speed.nsps()) as f64 / 12.0;
    let share = match speed {
        Js8Speed::Fast => 0.75,
        Js8Speed::Turbo | Js8Speed::Ultra => 0.5,
        Js8Speed::Slow | Js8Speed::Normal => 1.0,
    };
    (period_ms - symbols_ms - tx_delay_ms) * share
}

impl Engine {
    /// JS8Call's TX delay from Settings, in ms: how long before its period a JS8 frame keys. A
    /// value outside JS8Call's 0.1 to 0.5 s (a hand-edited settings file) is taken as the nearer end.
    pub fn js8_tx_delay_ms(&self) -> f64 {
        f64::from(
            self.settings
                .js8_tx_delay_ms
                .clamp(JS8_TX_DELAY_MIN_MS, JS8_TX_DELAY_MAX_MS),
        )
    }

    /// How far into its period a JS8 frame at the TX speed may still start ([`js8_late_start_ms`]
    /// with this station's TX delay).
    pub fn js8_late_start_ms(&self) -> f64 {
        js8_late_start_ms(self.js8_tx_speed(), self.js8_tx_delay_ms())
    }

    /// How far into its period a JS8 frame at the TX speed has sent its last tone, in ms: the
    /// speed's start delay and its 79 symbols, JS8Call's `txDuration`
    /// (`JS8_Mode/JS8Submode.cpp:74`), past which JS8Call's GUI tick stops TX (`prepareSending`,
    /// `JS8_UI/mainwindow.cpp:2575-2576`, then `stopTx`, :2681-2682). Slow 25.78 s, Normal 13.14 s,
    /// Fast 8.1 s, JS8 40 4.05 s, JS8 60 2.628 s.
    pub fn js8_frame_end_ms(&self) -> f64 {
        let speed = self.js8_tx_speed();
        f64::from(speed.delay_ms()) + (79 * speed.nsps()) as f64 / 12.0
    }

    /// The radio answered a JS8 frame's key `into_ms` into the frame's period, at or after the end
    /// of its last tone, `limit_ms` into it (inside the period or after it): nothing of the frame
    /// was left to send, and the loop sent nothing (and unkeyed the radio, unless it already held
    /// it). Says so on the JS8 cockpit's own line and in the log. TX stays on and the rest of the
    /// message goes out in the next periods, as an FT over the radio keys too late is dropped and
    /// TX kept on.
    pub fn js8_note_late_key(&mut self, into_ms: f64, limit_ms: f64) {
        let why = format!(
            "JS8 frame not sent: the radio keyed {:.1} s into the frame's period, after its last \
             tone had ended ({:.1} s in), so none of it was left to send. TX stays on, and the \
             rest of the message goes out in the next periods. A slow CAT link holds up the key: \
             check the CAT link and Split operation in Settings.",
            into_ms / 1000.0,
            limit_ms / 1000.0
        );
        tempo_core::applog::info("tx", &why);
        self.js8_last_error = Some(why);
        self.set_transmitting(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    /// JS8Call's late threshold at its default TX delay, speed by speed: Slow 4.52 s, Normal 2.16 s,
    /// Fast 1.425 s, JS8 40 0.925 s, JS8 60 0.636 s (mainwindow.cpp:2533-2546), and at the spin
    /// box's longest, 0.5 s.
    #[test]
    fn the_late_start_is_js8calls_threshold_at_every_speed() {
        let at = |speed, delay| (js8_late_start_ms(speed, delay) * 1000.0).round() / 1000.0;
        assert_eq!(
            Js8Speed::ALL.map(|s| at(s, 200.0)),
            [4_520.0, 2_160.0, 1_425.0, 925.0, 636.0]
        );
        assert_eq!(
            Js8Speed::ALL.map(|s| at(s, 500.0)),
            [4_220.0, 1_860.0, 1_200.0, 775.0, 486.0]
        );
    }

    /// The TX delay is JS8Call's: 0.2 s by default, a stored value inside 0.1 to 0.5 s as stored,
    /// one outside it the nearer end; the late start follows it and the TX speed.
    #[test]
    fn the_tx_delay_is_the_setting_inside_js8calls_range() {
        let mut e = Engine::with_settings(Settings::default());
        e.js8_enter();
        assert_eq!(e.js8_tx_delay_ms(), 200.0, "JS8Call's default TxDelay");
        assert_eq!(e.js8_late_start_ms(), 2_160.0, "Normal at 0.2 s");
        for (stored, taken) in [
            (100, 100.0),
            (300, 300.0),
            (500, 500.0),
            (0, 100.0),
            (5_000, 500.0),
        ] {
            e.settings.js8_tx_delay_ms = stored;
            assert_eq!(e.js8_tx_delay_ms(), taken, "stored {stored} ms");
        }
        e.settings.js8_tx_delay_ms = 500;
        e.js8_set_speed(2).unwrap();
        assert_eq!(e.js8_late_start_ms(), 1_200.0, "Fast at 0.5 s");
    }

    /// Where a frame's last tone ends at each speed: JS8Call's `txDuration`, the start delay and 79
    /// symbols (JS8Submode.cpp:74), whatever the TX delay.
    #[test]
    fn the_frames_last_tone_is_js8calls_tx_duration_at_every_speed() {
        let mut e = Engine::with_settings(Settings::default());
        e.js8_enter();
        for delay in [200, 500] {
            e.settings.js8_tx_delay_ms = delay;
            let ends = Js8Speed::ALL.map(|s| {
                e.js8_set_speed(s.index()).unwrap();
                e.js8_frame_end_ms()
            });
            assert_eq!(
                ends,
                [25_780.0, 13_140.0, 8_100.0, 4_050.0, 2_628.0],
                "TX delay {delay} ms"
            );
        }
    }

    /// A key the radio answered after its frame's last tone had ended, inside the period or after
    /// it, says so on the cockpit's line, with how far into the period it came and where the last
    /// tone ended, keeps TX on, and shows nothing as going out.
    #[test]
    fn a_late_key_says_why_and_keeps_tx_on() {
        let mut e = Engine::with_settings(Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            ..Default::default()
        });
        e.js8_enter();
        e.set_tx_enabled(true);
        e.js8_note_late_key(14_037.0, 13_140.0);
        let state = e.js8_state();
        let line = state.last_error.expect("the cockpit says why");
        assert!(
            line.starts_with(
                "JS8 frame not sent: the radio keyed 14.0 s into the frame's period, after its \
                 last tone had ended (13.1 s in)"
            ),
            "{line}"
        );
        assert!(e.tx_enabled(), "TX stays on");
    }
}
