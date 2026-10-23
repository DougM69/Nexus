//! The Remote FT permit's slot on the Engine (`Engine::remote_transmit`), and the poll the radio
//! loop and FT planning and commit run on every tick. The verbs that set the permit went with the
//! old Remote page, so the slot is only ever cleared now and the state's `txArmed` reads false; the
//! poll's live work is a streamed session's own hold on the transmitter (`remote_stream.rs`).
use super::Engine;
use std::time::Instant;

impl Engine {
    pub fn remote_ft_tx_owned(&self) -> bool {
        self.tx_enabled()
            && self
                .remote_transmit
                .as_ref()
                .is_some_and(|p| p.valid(Instant::now()))
    }

    /// Called by the real radio loop and at both FT planning and commit. A
    /// disconnected host cannot leave the station waiting for another request
    /// to notice expiry. Native halt supplies the existing flush/unkey signals.
    /// A streamed session's presence and held PTT are polled here too, on the
    /// same tick (`remote_stream.rs`).
    pub fn poll_remote_transmit(&mut self, now: Instant) -> bool {
        let halted = if self.remote_transmit.as_ref().is_some_and(|p| !p.valid(now)) {
            self.halt_tx();
            true
        } else {
            false
        };
        self.poll_remote_stream(now) || halted
    }
}
