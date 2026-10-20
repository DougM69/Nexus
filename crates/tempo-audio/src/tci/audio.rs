//! The TCI session's audio on Nexus's side, on the session's own thread. Receive is here: the
//! transceiver's audio at the decoders' rate, for the radio loop in place of the sound card, and for
//! the waterfall and Remote Listen. The transmit half (Nexus's audio, pulled by the server's
//! TX_CHRONO) is a later change: until it is installed every key that takes Nexus's audio is refused
//! ([`super::TciDaemon::set_tx_route`]), and the session has nothing of ours to answer a chrono with
//! but silence.
//!
//! ## Receive
//! A TCI server streams receive audio as binary frames, float32 stereo at 48 kHz unless told
//! otherwise (TCI 2.0 §3.4, §4.3), once the session has asked with `audio_start` (the radio's
//! `tci_audio`). Channel 0 is the real audio in both the one- and the two-channel stream (in DIGU and
//! DIGL the second channel is the quadrature one), and it is what WSJT-X takes too
//! (`TCITransceiver.cpp:909-985`). The codec has already made a sample that is not a number silence.
//! Channel 0 is resampled to the modem's 12 kHz with the capture path's own resampler (built again
//! when the stream's rate changes), and the operator's RX gain is applied with the sound card's own
//! clamp, so the slider means one thing on every route. Then it goes two ways:
//! - to the radio loop, which takes what arrived since its last tick ([`Receive::take`]). At most
//!   [`RING_CAP`] samples are kept, the oldest dropped first: a loop that stops reading (a radio the
//!   monitor holds, or one whose receive went back to the sound card) loses audio, never memory;
//! - to the receive tap's ring, while the radio loop has handed one over ([`Receive::set_tap`]). This
//!   thread is that ring's one producer; the waterfall's FFT and Remote Listen drain it on their own
//!   thread (`crate::rxtap`), so no CAT wait in the radio loop (a TCI write waits for the server's
//!   report) can stall them. The ring is held weakly: once the tap has moved on to another source,
//!   nothing more is written.
//!
//! Nexus's own design, not a port.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Instant;

use tempo_net::tci::frame::Block;
use tempo_net::tci::session::Streams;

use crate::capture_resample::CaptureResampler;
use crate::monitor::SpscRing;

/// The decoders' rate: what [`Receive::take`] hands the radio loop and the tap is fed at.
pub const MODEM_RATE: u32 = 12_000;

/// About ten seconds at [`MODEM_RATE`], as the Flex client keeps for its DAX receive audio.
pub(crate) const RING_CAP: usize = 120_000;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the session's thread and the radio loop share about receive audio.
pub struct Receive {
    /// Modem-rate audio since the last take, RX gain applied.
    ring: Mutex<Vec<f32>>,
    /// When the last receive block for our transceiver arrived: the radio loop's receive floor.
    last_block: Mutex<Option<Instant>>,
    /// The operator's RX gain, as the bits of an `f32`.
    gain: AtomicU32,
    /// The receive tap's ring while TCI audio is what the station hears.
    tap: Mutex<Weak<SpscRing>>,
}

impl Default for Receive {
    fn default() -> Receive {
        Receive {
            ring: Mutex::new(Vec::new()),
            last_block: Mutex::new(None),
            gain: AtomicU32::new(1.0f32.to_bits()),
            tap: Mutex::new(Weak::new()),
        }
    }
}

impl Receive {
    /// The modem-rate receive audio since the last call.
    pub fn take(&self) -> Vec<f32> {
        std::mem::take(&mut *lock(&self.ring))
    }

    /// When the last receive block for this transceiver arrived, if one has.
    pub fn last_block(&self) -> Option<Instant> {
        *lock(&self.last_block)
    }

    /// The operator's RX gain, applied to every block from the next one on.
    pub fn set_gain(&self, gain: f32) {
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    /// Feed `ring` (the receive tap's) from the next block on, or stop feeding any.
    pub fn set_tap(&self, ring: Option<&Arc<SpscRing>>) {
        *lock(&self.tap) = ring.map_or_else(Weak::new, Arc::downgrade);
    }

    /// One block's modem-rate audio: kept for the radio loop and written to the tap.
    fn push(&self, samples: &[f32]) {
        {
            let mut ring = lock(&self.ring);
            ring.extend_from_slice(samples);
            if ring.len() > RING_CAP {
                let excess = ring.len() - RING_CAP;
                ring.drain(..excess);
            }
        }
        let tap = lock(&self.tap).upgrade();
        if let Some(tap) = tap {
            tap.push_slice(samples);
        }
    }
}

/// The [`Streams`] a TCI session is given: its receive audio into [`Receive`].
pub struct SessionAudio {
    receive: Arc<Receive>,
    /// The stream's rate, and the resampler from it to [`MODEM_RATE`].
    resampler: Option<(u32, CaptureResampler)>,
}

impl SessionAudio {
    pub fn new(receive: Arc<Receive>) -> SessionAudio {
        SessionAudio {
            receive,
            resampler: None,
        }
    }
}

impl Streams for SessionAudio {
    fn receive(&mut self, block: &Block<'_>) {
        *lock(&self.receive.last_block) = Some(Instant::now());
        let rate = block.header.sample_rate;
        let resampler = match &mut self.resampler {
            Some((r, resampler)) if *r == rate => resampler,
            slot => {
                &mut slot
                    .insert((rate, CaptureResampler::new(rate, MODEM_RATE)))
                    .1
            }
        };
        let channel: Vec<f32> = block.channel(0).collect();
        let mut out = resampler.process(&channel);
        if out.is_empty() {
            return;
        }
        let gain =
            crate::device::clamp_rx_gain(f32::from_bits(self.receive.gain.load(Ordering::Relaxed)));
        if gain != 1.0 {
            for s in &mut out {
                *s *= gain;
            }
        }
        self.receive.push(&out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_net::tci::frame::{
        decode, encode_block, BlockHeader, BlockKind, Channels, Frame, LengthMeaning, SampleFormat,
    };
    use tempo_net::tci::Trx;

    /// A receive block's bytes: `frames` of a 1 kHz tone at `amp` on channel 0, its negative on
    /// channel 1, at `rate`.
    fn block_bytes(rate: u32, frames: usize, amp: f32) -> Vec<u8> {
        let header = BlockHeader {
            trx: Trx::new(0).unwrap(),
            kind: BlockKind::RxAudio,
            sample_rate: rate,
            format: SampleFormat::Float32,
            channels: Channels::Two,
        };
        let samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let v = amp * (std::f32::consts::TAU * 1_000.0 * i as f32 / rate as f32).sin();
                [v, -v]
            })
            .collect();
        encode_block(&header, LengthMeaning::Values, &samples).unwrap()
    }

    fn feed(audio: &mut SessionAudio, bytes: &[u8]) {
        match decode(bytes).unwrap() {
            Frame::Block(block) => audio.receive(&block),
            _ => panic!("not a block"),
        }
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Channel 0 at 48 kHz arrives at the modem's 12 kHz, a quarter as many samples, with the
    /// tone's level kept (the second channel is never mixed in: it would cancel the tone here).
    #[test]
    fn channel_zero_arrives_at_the_modem_rate() {
        let receive = Arc::new(Receive::default());
        let mut audio = SessionAudio::new(receive.clone());
        assert_eq!(receive.last_block(), None);
        for _ in 0..10 {
            feed(&mut audio, &block_bytes(48_000, 2_048, 0.5));
        }
        let got = receive.take();
        let fed = 10 * 2_048 / 4;
        assert!(
            got.len() <= fed && got.len() + 64 >= fed,
            "{} samples for {fed}",
            got.len()
        );
        // Past the resampler's first window, the tone at its own level.
        let p = peak(&got[200..]);
        assert!((0.45..=0.55).contains(&p), "peak {p}");
        assert!(receive.last_block().is_some());
        assert!(receive.take().is_empty(), "each sample is taken once");
    }

    /// The operator's RX gain is applied as the sound card applies it (its clamp included), and a
    /// rate change builds the resampler again rather than smearing two rates.
    #[test]
    fn the_rx_gain_and_a_new_rate_apply_from_the_next_block() {
        let receive = Arc::new(Receive::default());
        let mut audio = SessionAudio::new(receive.clone());
        receive.set_gain(2.0);
        for _ in 0..10 {
            feed(&mut audio, &block_bytes(48_000, 2_048, 0.25));
        }
        let p = peak(&receive.take()[200..]);
        assert!((0.45..=0.55).contains(&p), "gain 2: peak {p}");
        receive.set_gain(50.0);
        for _ in 0..10 {
            feed(&mut audio, &block_bytes(24_000, 1_024, 0.05));
        }
        let got = receive.take();
        assert!(
            got.len() <= 5_120 && got.len() + 64 >= 5_120,
            "24 kHz: {} samples",
            got.len()
        );
        let p = peak(&got[200..]);
        assert!((0.36..=0.44).contains(&p), "gain clamped to 8: peak {p}");
    }

    /// What the radio loop does not take is bounded: the oldest goes first.
    #[test]
    fn audio_nobody_takes_is_bounded() {
        let receive = Arc::new(Receive::default());
        let mut audio = SessionAudio::new(receive.clone());
        for _ in 0..1_200 {
            feed(&mut audio, &block_bytes(48_000, 2_048, 0.1));
        }
        assert_eq!(receive.take().len(), RING_CAP);
    }

    /// The tap's ring gets the same audio while it is given, and nothing once the tap has let it
    /// go, or once another ring is given.
    #[test]
    fn the_tap_is_fed_only_while_it_holds_its_ring() {
        let receive = Arc::new(Receive::default());
        let mut audio = SessionAudio::new(receive.clone());
        let ring = Arc::new(SpscRing::new(1 << 16));
        receive.set_tap(Some(&ring));
        feed(&mut audio, &block_bytes(48_000, 2_048, 0.1));
        let taken = receive.take();
        assert!(!taken.is_empty());
        assert_eq!(ring.len(), taken.len(), "the tap got what the loop got");
        let before = ring.len();
        receive.set_tap(None);
        feed(&mut audio, &block_bytes(48_000, 2_048, 0.1));
        assert_eq!(ring.len(), before, "no tap: nothing written");
        receive.set_tap(Some(&ring));
        let weak = Arc::downgrade(&ring);
        drop(ring);
        assert!(weak.upgrade().is_none());
        feed(&mut audio, &block_bytes(48_000, 2_048, 0.1));
        assert!(!receive.take().is_empty(), "the loop's audio goes on");
    }
}
