//! Two things a phone operator watches beside the transmit meters, neither of which is one:
//! the radio's own PA (finals) TEMPERATURE, and its SSB TRANSMIT BANDWIDTH step.
//!
//! The radio loop owns the CAT link, so it is the only thing that knows whether THIS radio
//! answers either at all (`tempo_audio::txextras` holds the per-radio commands). It tells the
//! engine what it read; the engine holds it for the snapshot and queues the operator's pick for
//! the loop. Nothing here keys, unkeys or gates a transmission.
//!
//! - **Temperature** is a reading, not a transmit meter: it is read while receiving, so it is
//!   NOT blanked on unkey the way [`Engine::clear_rig_tx_meters`] blanks the bars. It goes on a
//!   radio switch and a CAT drop, with the radio it described.
//! - **Transmit bandwidth** is the radio's own step list, in the radio's own words. A pick is
//!   refused while the station is on the air: the loop also never writes it while keyed, but a
//!   pick that waited silently for the unkey would change the over after the one the operator
//!   was listening to.

use super::Engine;
use crate::dto::RadioStatus;

/// The coldest and hottest readings believed. A PA heatsink sits between room temperature and
/// roughly 100 °C; anything outside this is a misparse or a radio reporting "no sensor", and is
/// dropped rather than shown.
const PA_TEMP_MIN_C: f32 = -40.0;
const PA_TEMP_MAX_C: f32 = 150.0;

/// The engine's copy of the two readings and the operator's pending pick.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TxExtras {
    /// °C, as last read. `None` = never read, or the radio has none.
    pa_temp_c: Option<f32>,
    /// The radio's own TEMP meter position, 0..1, for a radio that reports a meter and no
    /// degrees (the FTDX101's `RM9`). Never converted to degrees: no scale is published.
    pa_temp_meter: Option<f32>,
    /// The radio's own steps, as the loop declared them. Empty = no selector.
    tx_bw_steps: Vec<String>,
    /// Index into `tx_bw_steps` the radio last read back as.
    tx_bw: Option<usize>,
    /// The operator's pick, waiting for the loop's next write.
    tx_bw_request: Option<usize>,
    /// May the meter offer PROC on this radio's link — see [`Engine::observe_rig_proc_switch`].
    proc_switch: bool,
}

impl TxExtras {
    pub(super) fn fill_snapshot(&self, r: &mut RadioStatus) {
        r.pa_temp_c = self.pa_temp_c;
        r.pa_temp_meter = self.pa_temp_meter;
        r.tx_proc_switch = self.proc_switch;
        // The selector shows only once the radio has ANSWERED a read: a list with no current
        // step would draw a control that cannot say where it is.
        if self.tx_bw.is_some() {
            r.tx_bw_steps = self.tx_bw_steps.clone();
            r.tx_bw = self.tx_bw.and_then(|i| self.tx_bw_steps.get(i).cloned());
        } else {
            r.tx_bw_steps = Vec::new();
            r.tx_bw = None;
        }
    }
}

impl Engine {
    /// The radio's PA temperature, °C, as the loop read it. An implausible value is dropped
    /// (the last good reading stays): see [`PA_TEMP_MIN_C`].
    pub fn observe_rig_pa_temp_c(&mut self, celsius: f32) {
        if celsius.is_finite() && (PA_TEMP_MIN_C..=PA_TEMP_MAX_C).contains(&celsius) {
            self.tx_extras.pa_temp_c = Some(celsius);
        }
    }

    /// The radio's transmit-bandwidth steps and which one it is on (`None` = the read missed;
    /// the last answer stays). A different step list (another radio) drops the old answer and
    /// any pending pick, which named a step of the old list.
    pub fn observe_rig_tx_bw(&mut self, steps: &[&str], current: Option<usize>) {
        let x = &mut self.tx_extras;
        if x.tx_bw_steps
            .iter()
            .map(String::as_str)
            .ne(steps.iter().copied())
        {
            x.tx_bw_steps = steps.iter().map(|s| s.to_string()).collect();
            x.tx_bw = None;
            x.tx_bw_request = None;
        }
        if let Some(i) = current.filter(|&i| i < steps.len()) {
            x.tx_bw = Some(i);
        }
    }

    /// The radio's own TEMP meter position, 0..1, for a radio that reports no degrees.
    pub fn observe_rig_pa_temp_meter(&mut self, frac: f32) {
        if frac.is_finite() && (0.0..=1.0).contains(&frac) {
            self.tx_extras.pa_temp_meter = Some(frac);
        }
    }

    /// The radio loop gave up reading the meter-only temperature: its last reading goes with it,
    /// rather than stand on screen as if it were current.
    pub fn forget_rig_pa_temp_meter(&mut self) {
        self.tx_extras.pa_temp_meter = None;
    }

    /// Whether the TX meters may offer the speech-processor switch on this radio: only where the
    /// loop knows the switch's write is the radio maker's own command (the native Icom link's
    /// `16 44`). Elsewhere the processor stays on the chain's own COMP control.
    pub fn observe_rig_proc_switch(&mut self, offered: bool) {
        self.tx_extras.proc_switch = offered;
    }

    /// Forget the temperature alone: the stream that carried it has gone (a FlexRadio's meter
    /// stream), while whatever else the loop reads stays.
    pub fn clear_rig_pa_temp(&mut self) {
        self.tx_extras.pa_temp_c = None;
        self.tx_extras.pa_temp_meter = None;
    }

    /// Forget what the radio LOOP reads over CAT — the meter-only temperature and the
    /// transmit-bandwidth answer and pick — when the loop moves to another radio or its link
    /// drops. A FlexRadio's degrees come from its meter stream, which withdraws them itself.
    pub fn clear_rig_cat_extras(&mut self) {
        let x = &mut self.tx_extras;
        x.pa_temp_meter = None;
        x.tx_bw_steps.clear();
        x.tx_bw = None;
        x.tx_bw_request = None;
        x.proc_switch = false;
    }

    /// Forget both readings and any pending pick: the radio they describe is gone (a switch, or
    /// a CAT drop that may come back as another radio).
    pub fn clear_rig_tx_extras(&mut self) {
        self.tx_extras = super::tx_extras::TxExtras::default();
    }

    /// The operator picks a transmit-bandwidth step by its name on THIS radio.
    ///
    /// Refused (and nothing queued) when the radio has offered no steps, when the name is not
    /// one of them, or while the station is transmitting — Nexus's own over or the radio keyed
    /// from its own mic.
    pub fn set_tx_bw(&mut self, step: &str) -> Result<(), String> {
        if self.tx_extras.tx_bw.is_none() {
            return Err(
                "This radio doesn't report its transmit bandwidth over CAT — set it on the radio"
                    .to_string(),
            );
        }
        let Some(i) = self.tx_extras.tx_bw_steps.iter().position(|s| s == step) else {
            return Err(format!(
                "{step} is not a transmit bandwidth this radio offers"
            ));
        };
        if self.tx_owner().is_some() || self.rig_keyed {
            return Err(
                "Not while transmitting — change the transmit bandwidth between overs".to_string(),
            );
        }
        self.remote_actuation.revoke();
        self.tx_extras.tx_bw_request = Some(i);
        Ok(())
    }

    /// The pending pick, for the radio loop to write. Taken once.
    pub fn take_tx_bw_request(&mut self) -> Option<usize> {
        self.tx_extras.tx_bw_request.take()
    }
}
