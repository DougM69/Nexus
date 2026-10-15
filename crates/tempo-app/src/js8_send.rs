//! JS8's send mechanics, beside the adapter (a CHILD of `engine::js8`, so it reaches the adapter's
//! private verbs): two of JS8Call 3.0.3's transmit options, both off by default as JS8Call ships
//! them.
//!
//! - **Hold PTT between frames** (`HoldPTT`, JS8_UI/Configuration.cpp:2404): JS8Call's `stopTx2`
//!   leaves PTT up after a frame while frames of the message remain (mainwindow.cpp:2919-2932).
//!   The engine only answers whether to hold; the radio loop owns PTT and holds it.
//! - **Allow Tx frequency changes while transmitting** (`TxQSYAllowed`, Configuration.cpp:2305),
//!   the TX offset's half: off, JS8Call's `changeFreq` refuses a move while a message is queued
//!   for transmit or going out (mainwindow.cpp:5381-5386).
//!
//! Behaviour, not code: what JS8Call does was read from its source and is done here in Nexus's
//! own shapes.

use super::super::Engine;
use crate::dto::Tier;

/// What the JS8 cockpit says when a move of the TX offset is refused because a message is going
/// out. The sentence shape of the adapter's other refusals.
const JS8_TX_OFFSET_HELD: &str = "TX offset not moved: a JS8 message is waiting to go or going \
     out, and the offset stays put until it has gone, as in JS8Call. To move it while sending, \
     turn on Settings ▸ Digital ▸ JS8 ▸ Allow TX offset changes while sending.";

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
}
