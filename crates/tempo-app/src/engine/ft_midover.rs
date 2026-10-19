//! WSJT-X's mid-over message switch for FT8 and FT4 (`m_restart`), in the engine's half: what the
//! over on the air is, what the sequencer would send now, and the bookkeeping of sending it from
//! here on. The radio loop says when (`tempo_audio`'s `RadioLoop::ft_midover_switch`) and its
//! audio route replaces what it still holds (`AudioBackend::retune_tx`).
//!
//! WSJT-X 3.0.2, `widgets/mainwindow.cpp`:
//! - **When.** `m_restart` is set only while transmitting (`if(m_transmitting) m_restart=true;`):
//!   at the end of `processMessage` (:9446), which `auto_sequence` calls for a decode that moves
//!   the QSO (:7542-7560), and on a Tx-slot button click (`on_txb1_clicked`..`on_txb6_clicked`,
//!   :8751, :8768, :8776, :8784, :8802, :8816). `m_transmitting` runs from PTT (:8214) to the
//!   stop at the window's end (:8569).
//! - **What.** The next GUI tick regenerates the tones and the wave for the new `m_ntx` (:7880,
//!   FT8 :7952-7962, FT4 :7988-7997) and writes a second `Tx` row to ALL.TXT and the display
//!   (:8087-8091), stamped with the wall clock (`write_all`, :15252). The Modulator is not
//!   restarted: it reads the new wave from the sample it had reached (`foxcom_.wave[m_ic]`,
//!   `Modulator/Modulator.cpp:329`), so the over keeps its start and its end.
//! - **No time limit.** Nothing in `processMessage`, `guiUpdate` or the Modulator bounds when the
//!   switch may happen: a late one sends the start of one message and the rest of another.
//!
//! Nexus switches when the message the sequencer would send next differs from the one on the
//! air, which is when WSJT-X's regenerated tones differ.

use super::{Engine, Mode, QsoState, Tier, TxGateStamp, TxPlan, TxWaveform};

/// The FT8/FT4 over [`Engine::commit_tx`] handed to the radio loop last: what is on the air while
/// the loop holds its slot over, and how it was built.
pub(crate) struct FtOnAir {
    slot: u64,
    tier: Tier,
    kind: modes::ModeKind,
    text: String,
    f0: f32,
    wave_len: usize,
    stamp: TxGateStamp,
}

/// What [`Engine::ft_midover_switch`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum FtSwitch {
    /// The rest of the over now sends `to` in place of `from`.
    Switched { from: String, to: String },
    /// The sequencer moved on to `to`, and the audio route could not replace what it still holds:
    /// the over goes out as `from`, and `to` waits for the next over, as before.
    NotReplaced { from: String, to: String },
}

impl Engine {
    /// Remember the over `commit_tx` is handing to the radio loop (FT8 and FT4).
    pub(super) fn note_ft_on_air(&mut self, plan: &TxPlan, wave_len: usize) {
        // A new over answers every switch asked for before it.
        self.ft_restart = false;
        self.ft_on_air = match &plan.waveform {
            TxWaveform::Native { kind, text, f0 } if matches!(plan.tier, Tier::Ft8 | Tier::Ft4) => {
                Some(FtOnAir {
                    slot: plan.slot,
                    tier: plan.tier,
                    kind: *kind,
                    text: text.clone(),
                    f0: *f0,
                    wave_len,
                    stamp: plan.stamp.clone(),
                })
            }
            _ => None,
        };
    }

    /// A Tx-slot click asked for the over on the air to switch (`m_restart`); the radio loop
    /// takes it every tick and acts on it only while an FT8/FT4 over is on the air.
    pub fn take_ft_restart(&mut self) -> bool {
        std::mem::take(&mut self.ft_restart)
    }

    /// The text the QSO sequencer would send in `slot` if asked now, as [`Engine::plan_tx`] picks
    /// it: the one over owed to a partner the run has left, else the station's own. Changes
    /// nothing. `None` outside a QSO, or with nothing to send.
    pub fn ft_next_over(&self, slot: u64) -> Option<String> {
        let Mode::Qso { station, .. } = &self.mode else {
            return None;
        };
        match self
            .recent_partner
            .as_ref()
            .filter(|p| slot <= p.until_slot)
            .and_then(|p| p.owed.as_ref())
        {
            Some(owed) => Some(owed.to_text()),
            None => Some(station.outgoing_rv()?.0.to_text()),
        }
    }

    /// WSJT-X's mid-over switch: when the over this station keyed in `slot` (the live slot) is on
    /// the air and the QSO sequencer would now send something else, build that message's over and
    /// hand `retune` what replaces the rest of the queued one (`AudioBackend::retune_tx`'s
    /// argument: given how many samples have not played and how many before them the route needs,
    /// those samples of the new over). The over is slot-positioned and the new one is as long, so
    /// it keeps its start, its end and its PTT. When `retune` takes it, the rest of the over is
    /// sent as the sequencer sends an over at its key, with a second `Tx` row in ALL.TXT stamped
    /// `now_ms`; when it cannot, nothing is touched and the new message waits for the next over.
    ///
    /// Only while every reason not to transmit at all, which `plan_tx` checks before it picks an
    /// over, still passes, the live slot is still this station's, and the dial, sideband, TX
    /// offset, operating mode and licence class are those the over was planned for: an over
    /// that would not be planned now is never rebuilt. Stop TX and TX Off end here too.
    pub fn ft_midover_switch(
        &mut self,
        slot: u64,
        now_ms: f64,
        retune: impl FnOnce(&mut dyn FnMut(usize, usize) -> Vec<f32>) -> bool,
    ) -> Option<FtSwitch> {
        let tier = self.app.tier();
        let on = self.ft_on_air.as_ref()?;
        if on.slot != slot || on.tier != tier {
            return None;
        }
        self.poll_remote_transmit(std::time::Instant::now());
        if !self.slot_tx_armed()
            || self.tuning
            || self.cw_sending()
            || !self.tx_allowed()
            || self.clock_repair_holds_tx()
            || self.structured_tx_ready(true).is_err()
            || slot % 2 != self.tx_parity
        {
            return None;
        }
        let on = self.ft_on_air.as_ref()?;
        let mut stamp = self.tx_gate_stamp();
        stamp.generation = on.stamp.generation;
        if stamp != on.stamp {
            return None;
        }
        let to = self.ft_next_over(slot)?;
        if to == on.text {
            return None;
        }
        let from = on.text.clone();
        let wave = TxWaveform::Native {
            kind: on.kind,
            text: to.clone(),
            f0: on.f0,
        }
        .build();
        if wave.is_empty() || wave.len() != on.wave_len {
            tempo_core::applog::info(
                "tx",
                &format!(
                    "over not switched mid-air: {to:?} built {} samples where {from:?} has {}; \
                     the over goes out as {from:?}",
                    wave.len(),
                    on.wave_len
                ),
            );
            return Some(FtSwitch::NotReplaced { from, to });
        }
        // `left` counts back from the end of the over, which is where what the route holds ends,
        // so the new over's last samples are the ones that replace it, tone for tone in place.
        let mut tail = |left: usize, before: usize| {
            let want = left + before;
            let mut rest = vec![0.0; want.saturating_sub(wave.len())];
            rest.extend_from_slice(&wave[wave.len().saturating_sub(want)..]);
            rest
        };
        if !retune(&mut tail) {
            tempo_core::applog::info(
                "tx",
                &format!(
                    "over not switched mid-air: the audio route cannot replace what it has \
                     already been handed (or nothing is left to play); the over goes out as \
                     {from:?} and {to:?} waits for the next over"
                ),
            );
            return Some(FtSwitch::NotReplaced { from, to });
        }
        self.take_ft_over(slot);
        self.record_own_tx(to.clone());
        if self.settings.write_all_txt {
            let mode = format!("{tier:?}").to_uppercase();
            self.station
                .all_txt_pending
                .push(crate::alltxt::all_txt_line(
                    // WSJT-X stamps the second row with the wall clock (`write_all`), which says
                    // when in the over the switch was made.
                    (now_ms.max(0.0) / 1000.0) as u64,
                    self.settings.dial_mhz,
                    true,
                    &mode,
                    0,
                    0.0,
                    self.tx_offset_hz,
                    &to,
                ));
            let len = self.station.all_txt_pending.len();
            if len > 5000 {
                self.station.all_txt_pending.drain(0..len - 5000);
            }
        }
        let period_ms = self.active_slot_secs() * 1000.0;
        tempo_core::applog::info(
            "tx",
            &format!(
                "over switched mid-air {:.0} ms into the slot: {from:?} -> {to:?}",
                now_ms - (now_ms / period_ms).floor() * period_ms
            ),
        );
        if let Some(on) = self.ft_on_air.as_mut() {
            on.text = to.clone();
        }
        Some(FtSwitch::Switched { from, to })
    }

    /// The sequencer's bookkeeping for the over now on the air, as `plan_tx`'s QSO arm does it at
    /// a key: an owed over is spent, or the station's message is counted as sent. Counted once
    /// per over: a message of the step the over was keyed for (its report re-read from the
    /// partner's newer decode) was counted at the key; a new step (`tx_count` 0 after the
    /// partner's advance, or a Tx-slot click) is counted here. Its "Disable Tx after sending
    /// 73" is armed as at a key, and the radio loop drops TX once the over has played.
    fn take_ft_over(&mut self, slot: u64) {
        if self
            .recent_partner
            .as_ref()
            .is_some_and(|p| slot > p.until_slot)
        {
            self.recent_partner = None;
        }
        if self
            .recent_partner
            .as_mut()
            .and_then(|p| p.owed.take())
            .is_some()
        {
            self.recent_partner = None;
            self.cq_pause_until = None;
            return;
        }
        let Mode::Qso { station, .. } = &mut self.mode else {
            return;
        };
        self.cq_pause_until = None;
        if station.tx_count == 0 {
            station.after_tx();
            if station.state == QsoState::Done
                && !self.cq_running
                && self.settings.disable_tx_after_73
            {
                self.pending_tx_disable = true;
            }
        }
    }
}
