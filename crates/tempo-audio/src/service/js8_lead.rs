//! The radio loop's half of JS8's TX delay: a JS8 frame keys JS8Call's `TxDelay` before its period
//! (0.2 s by default, `Engine::js8_tx_delay_ms`), and the RX period before it ends there.
//!
//! JS8Call decides on the 100 ms GUI tick that finds the period `TxDelay` from its end
//! (`prepareSending`, `JS8_UI/mainwindow.cpp:2507-2572`: `time_is_in_tx_delay`, :2512-2516), asks for
//! PTT then, and its Modulator sends silence up to the period and the speed's start delay
//! (`JS8_Mode/Modulator.cpp:86-94`). Its receiver hears its own transmitter from the key, and its
//! decoder rests while it transmits (`isDecodeReady`: "decoder paused during transmit",
//! mainwindow.cpp:1660-1667), so the period before a frame ends at the key.
//!
//! Here, the first tick of the loop inside that last stretch of a JS8 period plans the next period's
//! frame ([`RadioLoop::js8_lead_frame`]). When there is one, that period's boundary is taken then
//! ([`RadioLoop::js8_boundary_slot`]): the boundary block decodes the period just ending from what
//! it has heard of it ([`heard_of_period`]) and keys the frame, which `js8_slot` lays on the slot
//! clock. When there is none, nothing happens: the boundary comes on its tick as before and plans
//! again there. No lead when the loop already holds the transmitter (Hold PTT between frames, the
//! operator's own PTT): JS8Call's PTT is up already then, and a held frame keys on its boundary's
//! tick, its wave on the slot clock all the same. Stop TX and TX Off end a frame inside its lead at
//! once ([`RadioLoop::in_js8_lead`]): it has sent nothing yet, and its period, the next cycle, is
//! the one TX Off turns down. FT8, FT4, FT2, Tempo and every other tier never come here.

use super::RadioLoop;
use tempo_app::engine::Engine;
use tempo_core::tempo_fast;

/// What the loop knows of the JS8 frame keyed in its TX delay.
#[derive(Debug, Default)]
pub(super) struct Js8Lead {
    /// The period whose TX delay was already asked for a frame: once a period, at its first tick.
    asked: Option<u64>,
    /// The period whose frame was planned in its TX delay, so whose boundary the loop took then.
    planned: Option<u64>,
    /// The start of that period, on the loop's clock: the end of the frame's lead.
    start_ms: f64,
}

/// How many 12 kHz samples of the period before boundary `slot` have been heard by `now`: the whole
/// period at the boundary or after it (the boundary block always took exactly that), and what there
/// is so far when the boundary is taken in a JS8 frame's TX delay, so the period's decode starts at
/// the period's own first sample and ends where the receiver stopped hearing it.
pub(super) fn heard_of_period(now: f64, slot: u64, period_secs: f64) -> usize {
    let period = (period_secs * f64::from(tempo_fast::SAMPLE_RATE)) as usize;
    let start_ms = slot.saturating_sub(1) as f64 * period_secs * 1000.0;
    let heard = ((now - start_ms).max(0.0) / 1000.0 * f64::from(tempo_fast::SAMPLE_RATE)) as usize;
    heard.min(period)
}

impl RadioLoop {
    /// The frame for the next JS8 period, planned in that period's TX delay, or `None`. Asked once a
    /// period, on the first tick inside the last `Engine::js8_tx_delay_ms` of the current one, when
    /// this period's boundary has been taken, nothing is keyed or tuning, and the loop may key this
    /// radio. A plan that keys nothing (TX off, nothing queued, a refusal) leaves the boundary to
    /// come on its own tick, where the frame is asked for again, as today.
    pub(super) fn js8_lead_frame(
        &mut self,
        eng: &mut Engine,
        now: f64,
        slot: u64,
        is_tuning: bool,
    ) -> Option<Vec<Vec<f32>>> {
        let next = slot + 1;
        if is_tuning
            || self.holds_tx()
            || !self.may_key()
            || self.last_slot != Some(slot)
            || self.js8_lead.asked == Some(next)
        {
            return None;
        }
        let start_ms = next as f64 * self.cur_slot_secs * 1000.0;
        if now < start_ms - eng.js8_tx_delay_ms() || now >= start_ms {
            return None;
        }
        self.js8_lead.asked = Some(next);
        let waves = eng.poll_tx(next);
        if waves.is_empty() {
            return None;
        }
        self.js8_lead.planned = Some(next);
        self.js8_lead.start_ms = start_ms;
        Some(waves)
    }

    /// The slot the boundary block runs for: the next one, through the rest of the current period,
    /// once the next period's frame was planned in its TX delay ([`Self::js8_lead_frame`]), so its boundary
    /// is taken once, there.
    pub(super) fn js8_boundary_slot(&self, slot: u64) -> u64 {
        if self.js8_lead.planned == Some(slot + 1) {
            slot + 1
        } else {
            slot
        }
    }

    /// Whether the slot over this loop holds is a JS8 frame still inside its lead: keyed in its TX
    /// delay, its period not yet begun. Nothing of it has gone out (silence up to its period), so TX
    /// Off ends it at once, as Stop TX does, instead of letting it finish as an over in flight.
    pub(super) fn in_js8_lead(&self, now: f64) -> bool {
        self.holds_slot_over() && now < self.js8_lead.start_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The period's audio the boundary block decodes: the whole period at its boundary or after
    /// (the same as before the TX delay), what has been heard of it when the boundary is taken in a
    /// frame's TX delay (Normal, 0.2 s early: 14.8 s), at most a period, never negative.
    #[test]
    fn the_period_heard_is_whole_at_its_boundary_and_ends_early_in_a_tx_delay() {
        let slot = 1_000_000;
        let boundary = slot as f64 * 15_000.0;
        assert_eq!(heard_of_period(boundary, slot, 15.0), 180_000);
        assert_eq!(heard_of_period(boundary + 20.0, slot, 15.0), 180_000);
        assert_eq!(heard_of_period(boundary - 200.0, slot, 15.0), 177_600);
        assert_eq!(heard_of_period(boundary - 15_000.0, slot, 15.0), 0);
        assert_eq!(heard_of_period(boundary - 30_000.0, slot, 15.0), 0);
    }
}
