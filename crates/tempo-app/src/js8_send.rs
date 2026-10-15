//! JS8's send mechanics, beside the adapter (a CHILD of `engine::js8`, so it reaches the adapter's
//! private verbs): two of JS8Call 3.0.3's transmit options, both off by default as JS8Call ships
//! them, and its typeahead.
//!
//! - **Hold PTT between frames** (`HoldPTT`, JS8_UI/Configuration.cpp:2404): JS8Call's `stopTx2`
//!   leaves PTT up after a frame while frames of the message remain (mainwindow.cpp:2919-2932).
//!   The engine only answers whether to hold; the radio loop owns PTT and holds it.
//! - **Allow Tx frequency changes while transmitting** (`TxQSYAllowed`, Configuration.cpp:2305),
//!   the TX offset's half: off, JS8Call's `changeFreq` refuses a move while a message is queued
//!   for transmit or going out (mainwindow.cpp:5381-5386); on, the move is taken at once, the frame
//!   on the air included: `setFreq` hands the new frequency to the Modulator (mainwindow.cpp:
//!   4212-4217, UI_Constructor.cpp:238-239), which sounds it from the next sample it makes
//!   (Modulator.cpp:217-229). The engine says when and builds the frame's rest; the radio loop has
//!   the sound card play it in place of what it still holds (`js8_retune_on_air`).
//! - **Typeahead** (mainwindow.cpp:3589-3668): the compose box keeps the operator's message while
//!   it goes out and takes more typing; the station frames the box's newest text just before each
//!   frame (`Station::type_ahead`). Nothing new keys by itself: every frame still goes out through
//!   `plan_js8_tx` and its gates.
//!
//! Behaviour, not code: what JS8Call does was read from its source and is done here in Nexus's
//! own shapes.

use super::super::Engine;
use crate::dto::{Js8Typeahead, Tier};
use ::js8::proto::station::TypeaheadError;
use std::sync::{Arc, PoisonError};

/// How far into the rest of a JS8 frame a move of the TX offset lands (`js8_retune_on_air`): 100 ms
/// of the frame at 12 kHz, more than the sound card plays while the rest is built and swapped in, so
/// the samples before the move are the ones the card plays.
pub const JS8_RETUNE_LEAD: usize = 1_200;

/// What the JS8 cockpit says when a move of the TX offset is refused because a message is going
/// out. The sentence shape of the adapter's other refusals.
const JS8_TX_OFFSET_HELD: &str = "TX offset not moved: a JS8 message is waiting to go or going \
     out, and the offset stays put until it has gone, as in JS8Call. To move it while sending, \
     turn on Settings ▸ Digital ▸ JS8 ▸ Allow TX offset changes while sending.";
/// …and why the compose box's typeahead was not taken.
const JS8_TYPEAHEAD_GONE: &str =
    "Nothing of yours is going out to type into: the message has gone, or was stopped.";
const JS8_TYPEAHEAD_LOCKED: &str =
    "A MSG or relay message cannot be changed while it goes out, as in JS8Call.";
const JS8_TYPEAHEAD_STALE: &str =
    "That change was to text already sent; what is still to go is as it was.";

impl Engine {
    /// Whether JS8 refuses a move of the TX offset to `hz` now, and if so says why on the cockpit.
    /// JS8Call refuses an offset change while a message is queued for transmit or going out
    /// (`changeFreq`: `isMessageQueuedForTransmit() && !tx_qsy_allowed()`, mainwindow.cpp:
    /// 5381-5386; queued is `m_transmitting || m_txFrameCount > 0`, :3264-3266) unless "Allow Tx
    /// frequency changes while transmitting" is on. Here: on JS8, the option off, and a frame on
    /// the air (`transmitting`, up from a frame's plan to the boundary that plans nothing more) or
    /// anything in the station's outbox. Leaving the offset where it is changes nothing and is
    /// never refused. Only the TX offset: RX moves, as JS8Call's one slider does not split them,
    /// are Nexus's and transmit nothing.
    pub(crate) fn js8_refuses_tx_offset_move(&mut self, hz: f32) -> bool {
        if self.app.tier() != Tier::Js8 || self.settings.js8_tx_offset_while_sending {
            return false;
        }
        let (lo, hi) = Self::tx_offset_bounds(Tier::Js8);
        if hz.clamp(lo, hi) == self.tx_offset_hz {
            return false;
        }
        let sending = self.app.transmitting() || !self.js8_station.queue().is_empty();
        if sending {
            self.js8_last_error = Some(JS8_TX_OFFSET_HELD.to_string());
        }
        sending
    }

    /// "Allow TX offset changes while sending" on: the TX offset moved while the frame planned for
    /// `slot` is on the air, so that frame moves too, from what it still has to play, as JS8Call's
    /// Modulator takes a new frequency mid-frame. Hands the radio loop what builds the rest of the
    /// frame at the new offset: given how many 12 kHz samples of the frame have not played yet,
    /// that many of the frame's wave, moved [`JS8_RETUNE_LEAD`] into them and its phase run on
    /// unbroken from the samples before (`::js8::phy::modulate_moved`, every earlier move kept).
    /// The lead is what the sound card may play of the frame as it was while the rest is built,
    /// so the move lands on a sample that sounds as the phase assumes. Only a frame keyed on the
    /// operator's offset (a heartbeat or HB ACK on its own 500–1000 Hz spot keeps it), only on
    /// JS8 with TX on, and only into the licence's privileges, which the frame was planned inside:
    /// a move out of them leaves the frame where it is (and the next frame is not planned there).
    /// Each move is handed over once.
    pub fn js8_retune_on_air(
        &mut self,
        slot: u64,
    ) -> Option<impl FnMut(usize) -> Vec<f32> + 'static> {
        if self.app.tier() != Tier::Js8
            || !self.settings.js8_tx_offset_while_sending
            || !self.tx_enabled()
            || !self.tx_allowed()
        {
            return None;
        }
        let to = self.tx_offset_hz();
        let on = self.js8_on_air.as_mut()?;
        if on.slot != slot || !on.follows || on.f0 == to {
            return None;
        }
        on.f0 = to;
        let (speed, moves) = (on.speed, Arc::clone(&on.moves));
        let tones = ::js8::phy::encode_word(&on.word, speed);
        let len = speed.delay_ms() as usize * 12 + 79 * speed.nsps();
        Some(move |left: usize| {
            let at = (len.saturating_sub(left) + JS8_RETUNE_LEAD).min(len);
            let mut moves = moves.lock().unwrap_or_else(PoisonError::into_inner);
            // A move whose sample the card has not reached yet is overtaken by this one.
            moves.retain(|&(k, _)| k == 0 || k < at);
            moves.push((at, to));
            let wave = ::js8::phy::modulate_moved(
                &tones,
                speed,
                &moves,
                tempo_core::tempo_fast::SAMPLE_RATE,
            );
            wave[wave.len().saturating_sub(left)..].to_vec()
        })
    }

    /// Hold PTT between frames: whether the radio loop keeps the transmitter keyed from the end of
    /// the JS8 frame it just sent to the key of the next. JS8Call's `stopTx2` holds while frames
    /// of the message remain (`m_txFrameCount > 0`, mainwindow.cpp:2928). Only with the option
    /// on, while the message on the air has frames to go (`Station::frames_follow`: never across
    /// two messages, never past the last frame), and while the dial is inside the licence's
    /// privileges, which is where the next frame would key. Every stop empties the outbox, so it
    /// ends a hold too.
    pub fn js8_holds_ptt_between_frames(&self) -> bool {
        self.settings.js8_hold_ptt && self.js8_station.frames_follow() && self.tx_allowed()
    }

    /// The compose box's typeahead, JS8Call's: what is still to go of the operator's message going
    /// out, `sent_seen` the characters of it the box had seen go (`Station::type_ahead`). Framed
    /// just before the next frame, which goes out through `plan_js8_tx` and every gate there; keys
    /// nothing by itself. An operator act, as a key press in JS8Call's box is (`eventFilter` →
    /// `resetIdleTimer`, mainwindow.cpp:1368-1374): it restarts the idle count and, taken, the
    /// wall-clock watchdog.
    pub fn js8_typeahead(&mut self, sent_seen: usize, unsent: String) -> Result<(), String> {
        let now_ms = tempo_core::timing::now_unix_ms() as u64;
        self.js8_restart_idle_clock(now_ms);
        let r = self
            .js8_station
            .type_ahead(sent_seen, &unsent)
            .map_err(|e| match e {
                TypeaheadError::NotSending => JS8_TYPEAHEAD_GONE.to_string(),
                TypeaheadError::Locked => JS8_TYPEAHEAD_LOCKED.to_string(),
                TypeaheadError::Stale => JS8_TYPEAHEAD_STALE.to_string(),
                TypeaheadError::Compose(e) => Self::js8_compose_error(e),
            });
        self.js8_after_verb(&r);
        r
    }

    /// What the compose box shows of the operator's message going out: what has gone and what is
    /// still to go, as its frames render. `None` once nothing of the box's is going out.
    pub fn js8_typeahead_state(&self) -> Option<Js8Typeahead> {
        self.js8_station.typeahead().map(|v| Js8Typeahead {
            sent: v.sent,
            unsent: v.unsent,
            locked: v.locked,
        })
    }
}
