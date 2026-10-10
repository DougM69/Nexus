//! Headphone monitor: a live pass-through of the RX audio the decoder hears to a
//! chosen output device, so the operator can HEAR the band and diagnose levels /
//! RFI. Ships DARK — off by default; a later attended session at the rig verifies
//! latency and levels.
//!
//! The load-bearing invariant is that **the decode path must never degrade**. The
//! capture callback (which feeds the decoder) and the monitor output callback each
//! run on their own real-time audio thread. A mutex shared between them could block
//! the capture thread. So monitoring uses a wait-free single-producer /
//! single-consumer ring ([`SpscRing`]) built from atomics only: the capture thread
//! `push`es and NEVER blocks or allocates, dropping samples on overflow (a monitor
//! glitch) — the decoder is never stalled by monitoring.
//!
//! The pure pieces (the ring, the TX-device guard, the resampler) compile without
//! the `device` feature so they are unit-testable in the headless workspace build;
//! the cpal output stream lives behind `#[cfg(feature = "device")]`.

// The ring is tempo-codec's, below the engine, where the headset's rings share it; named here as
// it always was.
pub use tempo_codec::ring::SpscRing;

/// The TX-device guard, as a pure predicate: `true` when opening the monitor on
/// `monitor_device` would feed the received band into the rig's TX audio device
/// (`audio_out`) and thus transmit it back out. When it returns `true` the monitor
/// must NOT open.
///
/// Two devices collide when their names match case-insensitively, or when BOTH are
/// empty (each meaning "system default output", i.e. the same device). An empty
/// against a named device is treated as distinct — we cannot prove by name that a
/// named device *is* the current system default, so the guard doesn't block it.
/// Resolve an empty device name ("system default output") to the ACTUAL default
/// output device's name, so the TX guard can compare real devices instead of
/// treating "" as unknowable. Returns the input unchanged when it's non-empty or
/// the host can't name a default (guard then falls back to the pure rule).
#[cfg(feature = "device")]
pub fn resolve_output_name(name: &str) -> String {
    use cpal::traits::HostTrait;
    if !name.trim().is_empty() {
        return name.to_string();
    }
    let _guard = crate::device::AUDIO_HOST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Through the crate's one fallible name reader — `to_string()` would panic where this
    // skips. See `device::cpal_device_name`.
    cpal::default_host()
        .default_output_device()
        .and_then(|d| crate::device::cpal_device_name(&d))
        .unwrap_or_default()
}

pub fn monitor_would_transmit(monitor_device: &str, audio_out: &str) -> bool {
    let m = monitor_device.trim();
    let o = audio_out.trim();
    if m.is_empty() && o.is_empty() {
        return true; // both "system default output" → the same device
    }
    if m.is_empty() || o.is_empty() {
        return false;
    }
    m.eq_ignore_ascii_case(o)
}

/// Nearest-neighbour (sample-and-hold) mono resampler driven by an [`SpscRing`],
/// converting the ring's `in_rate` samples to the output device's `out_rate`. Used
/// only when the monitor output device cannot open at the capture rate; at equal
/// rates it pops exactly one sample per output frame (a straight pass-through).
///
/// Pure and allocation-free — it holds only a phase accumulator and the last sample,
/// so it can run in the real-time output callback. Underruns emit silence.
pub struct MonoResampler {
    acc: f32,
    last: f32,
    in_rate: f32,
    out_rate: f32,
}

impl MonoResampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            acc: 0.0,
            last: 0.0,
            in_rate: in_rate.max(1) as f32,
            out_rate: out_rate.max(1) as f32,
        }
    }

    /// The next output-rate mono sample, consuming from `ring` as needed. On
    /// underrun (a needed sample is missing) it yields `0.0` (silence).
    pub fn next(&mut self, ring: &SpscRing) -> f32 {
        self.acc += self.in_rate;
        while self.acc >= self.out_rate {
            self.acc -= self.out_rate;
            self.last = ring.pop().unwrap_or(0.0);
        }
        self.last
    }
}

#[cfg(feature = "device")]
pub use device_monitor::Monitor;

#[cfg(feature = "device")]
mod device_monitor {
    use super::{MonoResampler, SpscRing};
    use crate::device::{dispatch_format, AUDIO_HOST_LOCK};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;

    fn err_fn(e: cpal::Error) {
        eprintln!("tempo-audio: monitor stream error: {e}");
    }

    /// Owns the dark headphone-monitor output stream and the state it shares with
    /// the capture callback. Lives inside [`crate::device::CpalBackend`] on the
    /// radio-loop thread (a cpal `Stream` is `!Send`). Reconfigured in place by
    /// [`Monitor::apply`] — it NEVER touches the capture/TX streams, so toggling the
    /// monitor can't restart or degrade the decode path.
    pub struct Monitor {
        /// The RX samples the capture callback pushes (at `in_rate`, mono).
        ring: Arc<SpscRing>,
        /// Gates the capture-callback push: cleared → the callback skips the ring.
        enabled: Arc<AtomicBool>,
        /// Set while the rig is KEYED. The capture callback treats it as a second gate, so the
        /// monitor goes silent for the duration of a transmission and the operator does not hear
        /// the rig's own delayed MONI over their own voice. Deliberately NOT `enabled`: that is
        /// the operator's setting and must come back unchanged when the over ends.
        tx_mute: Arc<AtomicBool>,
        /// Playback level as `f32` bits, read live by the output callback.
        level_bits: Arc<AtomicU32>,
        /// The capture rate the ring is filled at (the monitor output opens here
        /// when the device supports it; otherwise it resamples).
        in_rate: u32,
        /// The live monitor output stream (`None` = monitor off).
        out_stream: Option<Stream>,
        /// The device name `out_stream` targets ("" = system default), so a device
        /// change rebuilds only the output stream.
        active_device: String,
    }

    impl Monitor {
        pub fn new(
            ring: Arc<SpscRing>,
            enabled: Arc<AtomicBool>,
            tx_mute: Arc<AtomicBool>,
            level_bits: Arc<AtomicU32>,
            in_rate: u32,
        ) -> Self {
            Self {
                ring,
                enabled,
                tx_mute,
                level_bits,
                in_rate,
                out_stream: None,
                active_device: String::new(),
            }
        }

        /// Reconfigure the monitor in place. `enabled` is the guard-resolved decision
        /// (the caller has already refused a TX-device collision). Starts, stops, or
        /// retunes the output stream without disturbing capture. `Err` = the output
        /// device failed to open.
        /// Drop the monitor's output stream, freeing its device. **The caller MUST already
        /// hold [`AUDIO_HOST_LOCK`]** — this deliberately does not take it, because
        /// `std::sync::Mutex` is not reentrant and the whole point is to release every
        /// stream the backend owns inside ONE critical section
        /// (`AudioBackend::release_device`). Use [`Self::apply`] with `enabled: false`
        /// anywhere else; that one locks for you.
        /// Mute or unmute for a transmission. Called from the radio loop on every keying change.
        ///
        /// On the RISING edge the ring is cleared as well as gated: whatever the capture pushed in
        /// the moments before the key went down is band audio from before the over, and playing it
        /// out when the over ends would be a small burst of the past. Cheap — `clear` is two atomic
        /// stores — and it runs on the loop thread, never in an audio callback.
        pub fn set_tx_mute(&mut self, muted: bool) {
            let was = self.tx_mute.swap(muted, Ordering::Release);
            if muted && !was {
                self.ring.clear();
            }
        }

        pub fn release_locked(&mut self) {
            self.enabled.store(false, Ordering::Release);
            self.out_stream = None;
            self.active_device.clear();
            self.ring.clear();
        }
        pub fn apply(&mut self, enabled: bool, device: &str, level: f32) -> Result<(), String> {
            self.level_bits
                .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            if !enabled {
                self.enabled.store(false, Ordering::Release);
                if self.out_stream.is_some() {
                    // Tear the stream down under the host lock (native device-graph
                    // teardown shares the same non-reentrant state as construction).
                    let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                    self.out_stream = None;
                }
                self.active_device.clear();
                self.ring.clear();
                return Ok(());
            }
            if self.out_stream.is_none() || self.active_device != device {
                let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                self.out_stream = None; // drop the old stream (frees the device) first
                self.ring.clear();
                let stream = build_output(device, self.in_rate, &self.ring, &self.level_bits)?;
                self.out_stream = Some(stream);
                self.active_device = device.to_string();
            }
            self.enabled.store(true, Ordering::Release);
            Ok(())
        }
    }

    /// Can [`build_output`] actually build a stream in this sample format?
    ///
    /// ⭐ ONE LIST NOW, not two. This used to be a hand-maintained `matches!` here whose comment
    /// said it "IS `build_output`'s match arms and has to stay in step with them" — and it had
    /// already drifted from `device.rs`, which carried the same four formats with no list at all
    /// and no way for the monitor's picker to know. Both now ask
    /// [`crate::device::supported_device_format`], and both build through the one
    /// `dispatch_format!`, so a format is carried on every path or refused on every path.
    fn buildable_format(f: SampleFormat) -> bool {
        crate::device::supported_device_format(f)
    }

    /// Pick an output config at `want_rate` from what the device offers (so the ring's
    /// samples play straight through with no resampling), else `None`.
    ///
    /// ⚠️ THE FORMAT CHOICE IS THE WHOLE POINT (#8). This took the FIRST range whose rate span
    /// covered `want_rate` with no regard for its sample format, and on ALSA the first format a
    /// rig codec offers is `I8` — which [`build_output`] could not build, so the monitor died with
    /// "unsupported monitor output format: I8" on a card whose audio the DECODE path was opening
    /// happily the whole time. The decode path never hit this because it takes
    /// `default_output_config()`, the card's own PREFERRED format, and never enumerates. Picking
    /// nothing is safe: `build_output` then falls back to that same default config.
    ///
    /// ⭐ AND IT PICKS THE BEST, NOT THE FIRST. Every format is buildable now
    /// ([`crate::device::DeviceSample`]), so a "skip what I cannot build" filter would no longer
    /// skip `I8` at all — it would OPEN in `I8` on a card that also offered `I16`, turning a fixed
    /// bug into an 8-bit monitor. Where there is a choice, [`crate::device::format_quality_rank`]
    /// makes it, and it is our ranking rather than cpal's so it cannot move under a dependency bump.
    fn output_config_at_rate(
        configs: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
        want_rate: u32,
    ) -> Option<cpal::SupportedStreamConfig> {
        configs
            .filter(|range| {
                buildable_format(range.sample_format())
                    && range.min_sample_rate() <= want_rate
                    && want_rate <= range.max_sample_rate()
            })
            .max_by_key(|range| crate::device::format_quality_rank(range.sample_format()))
            // FALLIBLE: cpal 0.18's `with_sample_rate` PANICS out of range. The filter above
            // already guarantees the range covers `want_rate`, so this cannot return None — using
            // the checked form anyway costs nothing and removes a panic path from the open path.
            .and_then(|range| range.try_with_sample_rate(want_rate))
    }

    /// The monitor's output callback, once, for every sample format.
    ///
    /// Wait-free by construction: it pops the capture callback's [`SpscRing`] through a
    /// [`MonoResampler`] that holds only a phase accumulator, so neither audio thread can block the
    /// other. An underrun is silence, never a stall.
    #[allow(clippy::too_many_arguments)]
    fn build_monitor_stream<T: crate::device::DeviceSample + Send + 'static>(
        dev: &cpal::Device,
        config: &cpal::StreamConfig,
        out_ch: usize,
        in_rate: u32,
        out_rate: u32,
        ring: &Arc<SpscRing>,
        level_bits: &Arc<AtomicU32>,
    ) -> Result<Stream, String> {
        let ring_cb = ring.clone();
        let level_cb = level_bits.clone();
        let mut rs = MonoResampler::new(in_rate, out_rate);
        dev.build_output_stream(
            // `StreamConfig` is `Copy` in 0.18 and taken BY VALUE.
            *config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let level = f32::from_bits(level_cb.load(Ordering::Relaxed));
                for frame in data.chunks_mut(out_ch) {
                    // Clamped HERE, on every format including f32: unlike the TX path this is a
                    // product of two operator-set gains and can genuinely exceed unity.
                    let s = (rs.next(&ring_cb) * level).clamp(-1.0, 1.0);
                    let v = T::from_unit(s);
                    for x in frame.iter_mut() {
                        *x = v;
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| e.to_string())
    }

    /// Build and start the monitor output stream on `device_name` ("" = default).
    /// The caller MUST already hold [`AUDIO_HOST_LOCK`]. Opens at `in_rate` when the
    /// device supports it (pure pass-through); otherwise falls back to the device
    /// default rate and nearest-neighbour resamples in the callback.
    ///
    /// A NAMED device that will not resolve is an `Err` (already non-fatal — it gets its
    /// own error lane in `service.rs`), never the system default. This one is a TX-safety
    /// edge, not just honesty: `monitor_would_transmit` guards by comparing NAMES, so a
    /// silent fallback to the system default could land on the device that IS the rig's TX
    /// output and monitor the received band straight back onto the air, with the guard
    /// none the wiser because the name it checked was never the device that opened.
    fn build_output(
        device_name: &str,
        in_rate: u32,
        ring: &Arc<SpscRing>,
        level_bits: &Arc<AtomicU32>,
    ) -> Result<Stream, String> {
        let host = cpal::default_host();
        let name = (!device_name.trim().is_empty()).then_some(device_name);
        let dev = crate::device::resolve_configured(
            || host.output_devices().ok(),
            name,
            host.default_output_device(),
            "monitor output",
        )?;
        let supported = dev
            .supported_output_configs()
            .ok()
            .and_then(|c| output_config_at_rate(c, in_rate))
            .or_else(|| dev.default_output_config().ok())
            .ok_or("no monitor output config")?;
        let out_rate = supported.sample_rate();
        let out_ch = supported.channels() as usize;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.config();

        let stream = dispatch_format!(
            sample_format,
            build_monitor_stream,
            "monitor output",
            &dev,
            &config,
            out_ch.max(1),
            in_rate,
            out_rate,
            ring,
            level_bits,
        )?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(stream)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use cpal::{SupportedBufferSize, SupportedStreamConfigRange};

        fn range(fmt: SampleFormat, lo: u32, hi: u32) -> SupportedStreamConfigRange {
            SupportedStreamConfigRange::new(2, lo, hi, SupportedBufferSize::Unknown, fmt)
        }

        /// ⭐ THE FORMAT-COVERAGE OUTAGE. Every audio path in this crate handles exactly four of
        /// the ten sample formats cpal 0.15 can report, and anything else is refused — the monitor
        /// refuses to open, and on the DECODE path the same gap fails `CpalBackend::open` outright,
        /// which is no audio at all. That is already live (a card whose default config is `U16` or
        /// `F64` gets nothing today), and the cpal 0.18 upgrade widens it: 0.18 adds `I24`/`U24`
        /// and re-ranks `default_*_config()` so `F64`, `U32`, `I24` and `U24` all outrank the `I16`
        /// we do handle. A macOS enumeration fix must not become a Windows and Linux outage, so the
        /// coverage is closed FIRST, on 0.15, for every format that exists there.
        #[test]
        fn every_sample_format_the_host_can_report_is_carried() {
            // The six that were missing. `U16` and `F64` are the plausible ones today; the rest
            // close the class rather than leave it open at a different width.
            for fmt in [
                SampleFormat::I8,
                SampleFormat::U16,
                SampleFormat::U32,
                SampleFormat::I64,
                SampleFormat::U64,
                SampleFormat::F64,
            ] {
                assert!(
                    buildable_format(fmt),
                    "{fmt:?} is a format cpal can hand us, and refusing it is an audio outage"
                );
            }
            // ...and the four that always worked still do — the positive control, since a
            // `buildable_format` that simply returned true would pass the loop above.
            for fmt in [
                SampleFormat::F32,
                SampleFormat::I16,
                SampleFormat::U8,
                SampleFormat::I32,
            ] {
                assert!(buildable_format(fmt), "{fmt:?} regressed");
            }
        }

        /// ⭐ #8 ON THE WIRE: *"unsupported format: I8"* on the very card whose audio the decoder
        /// was already reading. ALSA offers `I8` FIRST, the picker took the first range whose
        /// rate span fitted, and `build_output` then had no arm for it — so the monitor refused to
        /// open a device the decode path opens every session.
        ///
        /// The fix has since moved on twice. `I8` is now BUILDABLE, so "skip what I cannot build"
        /// would let the picker take it — and hand the operator an 8-bit monitor on a card that
        /// also offered `I16`. The rule is now "take the BEST buildable one", which lands on the
        /// same answer here for a better reason.
        #[test]
        fn the_picker_takes_the_best_format_not_the_first() {
            let offered = [
                range(SampleFormat::I8, 8_000, 48_000), // what ALSA offers first
                range(SampleFormat::I16, 8_000, 48_000),
            ];
            let picked = output_config_at_rate(offered.into_iter(), 12_000)
                .expect("a buildable format was on offer");
            assert_eq!(
                picked.sample_format(),
                SampleFormat::I16,
                "the picker must pass over I8 for the better format offered alongside it"
            );
            assert_eq!(picked.sample_rate(), 12_000, "still at the ring's rate");

            // ...and it reaches PAST a mediocre one for the best, not merely past the worst.
            let offered = [
                range(SampleFormat::I8, 8_000, 48_000),
                range(SampleFormat::I16, 8_000, 48_000),
                range(SampleFormat::F32, 8_000, 48_000),
            ];
            assert_eq!(
                output_config_at_rate(offered.into_iter(), 12_000)
                    .unwrap()
                    .sample_format(),
                SampleFormat::F32,
            );

            // I8 ALONE is now opened rather than refused — the coverage half of the change. An
            // 8-bit monitor is poor; silence and an error banner are worse.
            assert_eq!(
                output_config_at_rate([range(SampleFormat::I8, 8_000, 48_000)].into_iter(), 12_000)
                    .unwrap()
                    .sample_format(),
                SampleFormat::I8,
            );
        }

        /// The other direction, and the one that says the filter is a FILTER rather than a
        /// blanket refusal: every format `build_output` handles is still pickable, and the
        /// rate span is still what decides between them.
        #[test]
        fn every_buildable_format_is_still_picked_and_the_rate_span_still_decides() {
            for fmt in [
                SampleFormat::F32,
                SampleFormat::I16,
                SampleFormat::U8,
                SampleFormat::I32,
            ] {
                let picked = output_config_at_rate([range(fmt, 8_000, 48_000)].into_iter(), 12_000)
                    .unwrap_or_else(|| panic!("{fmt:?} is buildable and must be pickable"));
                assert_eq!(picked.sample_format(), fmt);
            }
            // A buildable format whose span does NOT cover the rate is still passed over.
            assert!(
                output_config_at_rate(
                    [range(SampleFormat::I16, 44_100, 48_000)].into_iter(),
                    12_000
                )
                .is_none(),
                "a range that cannot reach the ring's rate is not a candidate"
            );
        }

        /// Nothing USABLE on offer is `None`, NOT a config that would fail to build —
        /// `build_output` then falls back to the card's own default config, which is exactly the
        /// route the decode path takes and the reason rig audio opens at all.
        ///
        /// The format half of this is now unreachable BY CONSTRUCTION on cpal 0.15 — every format
        /// it can report is buildable — so what is left to test is the rate half. The arm still
        /// matters: `SampleFormat` is `#[non_exhaustive]`, and cpal 0.18 adds DSD variants that
        /// cannot be carried by scaling and must never be picked.
        #[test]
        fn nothing_usable_on_offer_yields_none_so_the_default_config_is_used() {
            assert!(
                output_config_at_rate(
                    [range(SampleFormat::I16, 44_100, 48_000)].into_iter(),
                    12_000
                )
                .is_none(),
                "no range reaches the ring's rate → fall back to the card's default config"
            );
            assert!(
                output_config_at_rate(std::iter::empty(), 12_000).is_none(),
                "nothing on offer at all"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "device")]
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    #[cfg(feature = "device")]
    use std::sync::Arc;

    /// A transmission must not be played back to the operator, and what was buffered before it
    /// must not surface afterwards.
    ///
    /// Drives the REAL thing — `Monitor::set_tx_mute` and the atomics the capture callback reads.
    /// The first version of this test did not: it built a `SpscRing`, called `clear()` and checked
    /// the ring was empty, which `SpscRing::clear` already has its own test for and which passes
    /// with this whole feature deleted (kd9taw caught it on #158). A test that cannot fail when the
    /// feature is removed is not evidence of the feature.
    ///
    /// `Monitor` needs no audio device to construct: `new` takes the shared `Arc`s and
    /// `out_stream` starts `None`.
    #[cfg(feature = "device")]
    #[test]
    fn muting_for_a_transmission_closes_the_gate_and_discards_the_audio_that_preceded_it() {
        let ring = Arc::new(SpscRing::new(64));
        let enabled = Arc::new(AtomicBool::new(true));
        let tx_mute = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0.5f32.to_bits()));
        let mut m = Monitor::new(
            ring.clone(),
            enabled.clone(),
            tx_mute.clone(),
            level.clone(),
            48_000,
        );

        for _ in 0..16 {
            assert!(
                ring.push(0.5),
                "fixture: band audio buffered before the over"
            );
        }

        // THE RISING EDGE. The gate the capture callback reads closes, and the audio captured
        // before the key went down goes with it — otherwise it plays out as a burst of the past
        // when the over ends, which is a stranger artefact than the one this fixes.
        m.set_tx_mute(true);
        assert!(tx_mute.load(Ordering::Relaxed), "the gate closed");
        assert_eq!(ring.len(), 0, "and the pre-TX audio went with it");

        // EDGE-TRIGGERED, which is a claim the design makes and therefore has to be held to:
        // muting again mid-over must not wipe anything a second time.
        assert!(ring.push(0.25), "something arrives while still keyed");
        m.set_tx_mute(true);
        assert_eq!(ring.len(), 1, "muting again is not a second clear");

        // AND THE OPERATOR'S OWN SETTING IS UNTOUCHED THROUGHOUT. This is the invariant the whole
        // design rests on — the reason the mute is a second atomic rather than borrowing
        // `enabled` — and nothing held us to it before.
        m.set_tx_mute(false);
        assert!(
            !tx_mute.load(Ordering::Relaxed),
            "the gate reopens when the over ends"
        );
        assert!(
            enabled.load(Ordering::Relaxed),
            "the operator's monitor setting survives a transmission unchanged"
        );
    }
    use super::*;

    #[test]
    fn guard_blocks_same_device_and_both_default() {
        // Both empty = both "system default output" → the same device → blocked.
        assert!(monitor_would_transmit("", ""));
        // Same name (case-insensitive) → blocked.
        assert!(monitor_would_transmit("USB Audio CODEC", "usb audio codec"));
        assert!(monitor_would_transmit(" Speakers ", "Speakers"));
        // Distinct devices → allowed.
        assert!(!monitor_would_transmit("Headphones", "USB Audio CODEC"));
        // One default, one named → cannot prove collision by name → allowed.
        assert!(!monitor_would_transmit("Headphones", ""));
        assert!(!monitor_would_transmit("", "USB Audio CODEC"));
    }

    #[test]
    fn resampler_equal_rate_is_passthrough() {
        let r = SpscRing::new(16);
        r.push_slice(&[0.1, 0.2, 0.3, 0.4]);
        let mut rs = MonoResampler::new(48_000, 48_000);
        // One pop per output sample, in order.
        assert!((rs.next(&r) - 0.1).abs() < 1e-6);
        assert!((rs.next(&r) - 0.2).abs() < 1e-6);
        assert!((rs.next(&r) - 0.3).abs() < 1e-6);
        assert!((rs.next(&r) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn resampler_underrun_is_silence() {
        let r = SpscRing::new(16); // empty
        let mut rs = MonoResampler::new(48_000, 48_000);
        assert_eq!(
            rs.next(&r),
            0.0,
            "no audio available → silence, never a hang"
        );
    }

    #[test]
    fn resampler_downsamples_by_dropping() {
        // 4:1 decimation holds the most-recent of each group of input samples.
        let r = SpscRing::new(16);
        r.push_slice(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        let mut rs = MonoResampler::new(48_000, 12_000);
        assert_eq!(rs.next(&r), 4.0, "consumed 4 inputs, held the last");
        assert_eq!(rs.next(&r), 8.0);
    }
}
