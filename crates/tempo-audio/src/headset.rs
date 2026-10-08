//! This computer's headset, for a shack it drives over its own network (client mode): the shack's
//! receive audio and a short tone out, the operator's microphone in, on the devices chosen under
//! This computer ▸ Audio. Nothing here knows a transport, a press or a radio: the audio road
//! (src-tauri's `lan_client::audio_road`) fills it and drains it.
//!
//! ## Wait-free between the road and the sound card
//!
//! The output and the input each run on cpal's own realtime thread, and share with the rest of
//! Nexus only atomics and [`SpscRing`]s ([`Rings`]): no callback takes a lock, allocates or waits,
//! as on the station's own sound-card path. The threads that write the output or read the input
//! take a lock among THEMSELVES ([`Rings::play`], [`Rings::sound`], [`Rings::take_mic`]), so each
//! ring keeps one producer and one consumer, and a callback never takes it.
//!
//! ## What is heard: the hosted page's rules, at the LAN's cushion
//!
//! - **A fixed cushion** ([`CUSHION_MS`]): playback starts once that much is waiting, and again the
//!   same way whenever it runs dry. Past the cushion and 100 ms more ([`CEILING_MS`]) the oldest
//!   audio goes, back down to the cushion: a drop, never a speed-up or a slow-down.
//! - **A quiet bed in a known gap** ([`BED_LEVEL`]): while listening with nothing to play, a noise
//!   some 8 dB under a quiet band, as the page plays, so a gap never sounds like a dead link and
//!   never bursts.
//! - **The page's level** ([`GAIN_DB`]): the station sets its receive level for its decoders, some
//!   40 dB under a normal listen, so it is lifted by the page's default volume and held under
//!   [`PEAK`].
//! - **M9: muted, never ducked** ([`Rings::mute`]), while the operator's over may be on the air:
//!   silence, and what was waiting is dropped, so nothing from before the over plays after it.
//! - **The tone** ([`Rings::sound`]) plays over all of it, muted or not. It is this computer's own
//!   sound and goes only to the output: the microphone ring is the input's alone, so the tone can
//!   never reach the microphone path.
//!
//! ## The microphone
//!
//! The input folds its channels to mono by averaging, as every capture callback does (a lane
//! picked by level shreds a signal), and pushes device-rate samples to the microphone ring; its
//! peak is kept for the meter ([`Rings::peak`]). The reader resamples to the encoder's rate.
//!
//! ## Open only while wanted (`Headset`, with the `device` feature)
//!
//! One thread of the headset's own opens and holds the devices (a cpal stream lives and dies on
//! one thread), each direction only while something wants it: the output while a road listens or
//! a sound plays, the input while a press is live or the meter is shown. Each want is a deadline
//! its user renews, so a direction nobody renews closes by itself. A named device that is not
//! there is said so, never swapped for the system default.
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::monitor::SpscRing;

/// The cushion on the LAN: playback starts once this much is waiting.
pub const CUSHION_MS: u32 = 60;
/// Past this much waiting, the oldest audio goes, back down to the cushion.
pub const CEILING_MS: u32 = CUSHION_MS + 100;
/// The bed, in the station's own scale before the gain: the hosted page's, about -68 dBFS RMS.
pub const BED_LEVEL: f32 = 0.0006;
/// The station's level lifted by the hosted page's default volume.
pub const GAIN_DB: f32 = 24.0;
/// The loudest sample the headset is ever given, -0.9 dBFS.
pub const PEAK: f32 = 0.9;
/// Each ring: half a second at the highest rate a headset is likely to run, 192 kHz.
const RING: usize = 96_000;

/// The gain [`GAIN_DB`] as a factor.
pub fn gain() -> f32 {
    10f32.powf(GAIN_DB / 20.0)
}

fn lock(mutex: &Mutex<()>) -> MutexGuard<'_, ()> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What the road's thread and the headset's callbacks share: three rings and the atomics around
/// them.
pub struct Rings {
    /// The shack's audio, lifted, at the output's rate.
    play: SpscRing,
    /// This computer's own sounds, at the output's rate.
    tone: SpscRing,
    /// The microphone, mono, at the input's rate.
    mic: SpscRing,
    /// The producers' and the reader's own locks, never a callback's (see the module header).
    play_writer: Mutex<()>,
    tone_writer: Mutex<()>,
    mic_reader: Mutex<()>,
    /// M9: the operator's over may be on the air.
    muted: AtomicBool,
    /// A road is listening: a gap plays the bed rather than silence.
    listening: AtomicBool,
    /// The open output's and input's rates, 0 while closed.
    out_rate: AtomicU32,
    in_rate: AtomicU32,
    /// The microphone's peak since the meter last read it, as `f32` bits.
    peak: AtomicU32,
    /// Moves each time a direction opens, so a reader at the old rate starts again.
    opened: AtomicU64,
    /// Samples of the shack's audio the output had no room for.
    dropped: AtomicU64,
}

impl Default for Rings {
    fn default() -> Self {
        Self {
            play: SpscRing::new(RING),
            tone: SpscRing::new(RING),
            mic: SpscRing::new(RING),
            play_writer: Mutex::new(()),
            tone_writer: Mutex::new(()),
            mic_reader: Mutex::new(()),
            muted: AtomicBool::new(false),
            listening: AtomicBool::new(false),
            out_rate: AtomicU32::new(0),
            in_rate: AtomicU32::new(0),
            peak: AtomicU32::new(0),
            opened: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        }
    }
}

impl Rings {
    /// Rings with no device behind them, as if the output and the input were open at these rates:
    /// for a test standing in for a headset.
    pub fn stand_in(out_rate: u32, in_rate: u32) -> Self {
        let rings = Self::default();
        rings.out_rate.store(out_rate, Ordering::Release);
        rings.in_rate.store(in_rate, Ordering::Release);
        rings
    }

    /// The output's rate, while it is open.
    pub fn out_rate(&self) -> Option<u32> {
        Some(self.out_rate.load(Ordering::Acquire)).filter(|rate| *rate > 0)
    }

    /// The input's rate, while it is open.
    pub fn in_rate(&self) -> Option<u32> {
        Some(self.in_rate.load(Ordering::Acquire)).filter(|rate| *rate > 0)
    }

    /// How many times a direction has opened: a reader that saw another number resamples afresh.
    pub fn opened(&self) -> u64 {
        self.opened.load(Ordering::Acquire)
    }

    /// The shack's audio, at the output's rate: lifted by [`gain`], held under [`PEAK`] and queued.
    /// Nothing is queued while muted (M9), and what will not fit is dropped and counted.
    pub fn play(&self, samples: &[f32]) {
        let _writer = lock(&self.play_writer);
        if self.muted.load(Ordering::Acquire) {
            return;
        }
        let lift = gain();
        let mut taken = 0;
        for sample in samples {
            if !self.play.push((sample * lift).clamp(-PEAK, PEAK)) {
                break;
            }
            taken += 1;
        }
        self.dropped
            .fetch_add((samples.len() - taken) as u64, Ordering::Relaxed);
    }

    /// One of this computer's own sounds, at the output's rate: played whether muted or not, and
    /// only ever to the output.
    pub fn sound(&self, samples: &[f32]) {
        let _writer = lock(&self.tone_writer);
        self.tone.push_slice(samples);
    }

    /// Everything the microphone captured since the last take, at the input's rate.
    pub fn take_mic(&self) -> Vec<f32> {
        let _reader = lock(&self.mic_reader);
        std::iter::from_fn(|| self.mic.pop()).collect()
    }

    /// The microphone's loudest sample since the last call, `0.0` to `1.0`.
    pub fn peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::AcqRel))
    }

    /// M9: muted while the operator's over may be on the air. Never ducked: muted is silence.
    pub fn mute(&self, on: bool) {
        self.muted.store(on, Ordering::Release);
    }

    pub fn muted(&self) -> bool {
        self.muted.load(Ordering::Acquire)
    }

    /// A road is listening, so a gap plays the bed.
    pub fn listen(&self, on: bool) {
        self.listening.store(on, Ordering::Release);
    }

    /// How much of the shack's audio is waiting to play, in samples at the output's rate.
    pub fn waiting(&self) -> usize {
        self.play.len()
    }

    /// Samples of the shack's audio dropped for want of room.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The input callback's half: one buffer folded to mono, queued, and its peak kept. A test
    /// stands for a microphone with it. REALTIME: atomics and the ring only.
    pub fn captured(&self, mono: impl Iterator<Item = f32>) {
        let mut peak = 0.0f32;
        for sample in mono {
            peak = peak.max(sample.abs());
            let _ = self.mic.push(sample);
        }
        // Positive floats order as their bits do.
        self.peak
            .fetch_max(peak.min(1.0).to_bits(), Ordering::AcqRel);
    }
}

/// The output callback's own state: whether it is playing or filling the cushion, and the bed.
pub struct Cushion {
    cushion: usize,
    ceiling: usize,
    playing: bool,
    bed: f32,
    noise: u32,
    /// Read at the top of each callback.
    muted: bool,
    listening: bool,
}

impl Cushion {
    /// The cushion for an output at `rate`.
    pub fn new(rate: u32) -> Self {
        let samples = |ms: u32| (rate as usize) * (ms as usize) / 1000;
        Self {
            cushion: samples(CUSHION_MS),
            ceiling: samples(CEILING_MS),
            playing: false,
            bed: BED_LEVEL * gain(),
            noise: 0x2545_f491,
            muted: false,
            listening: false,
        }
    }

    /// At the top of each output callback: M9, and the ceiling. REALTIME.
    pub fn begin(&mut self, rings: &Rings) {
        self.muted = rings.muted.load(Ordering::Acquire);
        self.listening = rings.listening.load(Ordering::Acquire);
        if self.muted {
            // The callback is the ring's consumer, so it may empty it.
            rings.play.clear();
            self.playing = false;
            return;
        }
        let waiting = rings.play.len();
        if waiting > self.ceiling {
            for _ in self.cushion..waiting {
                let _ = rings.play.pop();
            }
        }
        if !self.playing && rings.play.len() >= self.cushion {
            self.playing = true;
        }
    }

    /// The next sample for the output. REALTIME.
    pub fn next(&mut self, rings: &Rings) -> f32 {
        let heard = if self.muted {
            0.0
        } else if self.playing {
            match rings.play.pop() {
                Some(sample) => sample,
                None => {
                    // Dry: fill the cushion again before playing on.
                    self.playing = false;
                    self.bed()
                }
            }
        } else {
            self.bed()
        };
        (heard + rings.tone.pop().unwrap_or(0.0)).clamp(-PEAK, PEAK)
    }

    /// The bed while listening, silence otherwise.
    fn bed(&mut self) -> f32 {
        if !self.listening {
            return 0.0;
        }
        self.noise = self
            .noise
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let unit = (self.noise >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
        unit * self.bed
    }
}

/// This computer's own sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// An over ended, or the shack fell silent while one was held: two short falling notes.
    OverEnded,
    /// This computer ▸ Audio's test: a steady note.
    Test,
}

/// `sound` at `rate`, ready for [`Rings::sound`]: about -18 dBFS, each note eased in and out over
/// 5 ms so it never clicks.
pub fn sound(sound: Sound, rate: u32) -> Vec<f32> {
    let notes: &[(f32, u32)] = match sound {
        Sound::OverEnded => &[(880.0, 110), (0.0, 40), (660.0, 160)],
        Sound::Test => &[(1_000.0, 500)],
    };
    let rate_f = rate as f32;
    let ease = (rate / 200).max(1) as usize;
    let mut out = Vec::new();
    for &(hz, ms) in notes {
        let n = (rate as usize) * (ms as usize) / 1000;
        for i in 0..n {
            let edge = i.min(n - 1 - i).min(ease) as f32 / ease as f32;
            let level = if hz > 0.0 { 0.125 * edge } else { 0.0 };
            out.push(level * (2.0 * std::f32::consts::PI * hz * i as f32 / rate_f).sin());
        }
    }
    out
}

#[cfg(feature = "device")]
pub use device_headset::Headset;

#[cfg(feature = "device")]
mod device_headset {
    use super::{Cushion, Rings};
    use crate::device::{
        capture_config, dispatch_format, format_quality_rank, open_capture_with_fallback,
        resolve_configured, supported_device_format, DeviceSample, AUDIO_HOST_LOCK,
    };
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// How often the headset's thread looks at what is wanted.
    const LOOK: Duration = Duration::from_millis(20);
    /// How long a direction that would not open waits before it is tried again on the same device.
    const AGAIN: Duration = Duration::from_secs(2);

    struct Shared {
        rings: Arc<Rings>,
        /// The output and input chosen, "" for the system default.
        devices: Mutex<(String, String)>,
        /// Until when each direction is wanted, in ms from `epoch`.
        out_until: AtomicU64,
        in_until: AtomicU64,
        epoch: Instant,
        /// Why each direction is not open, when it would not open.
        trouble: Mutex<(Option<String>, Option<String>)>,
        /// Which directions are open now.
        open: Mutex<(bool, bool)>,
        stopping: AtomicBool,
    }

    impl Shared {
        fn ms(&self, at: Instant) -> u64 {
            u64::try_from(at.saturating_duration_since(self.epoch).as_millis()).unwrap_or(u64::MAX)
        }
    }

    /// This computer's headset: the rings, and the thread that opens and holds the devices while
    /// they are wanted. Cheap to share; the thread ends when the last holder lets it go.
    pub struct Headset {
        shared: Arc<Shared>,
    }

    impl Headset {
        /// The headset on `output` and `input` ("" for the system default), opening nothing until
        /// something wants a direction. `None` if its thread would not start.
        pub fn start(output: &str, input: &str) -> Option<Self> {
            let shared = Arc::new(Shared {
                rings: Arc::new(Rings::default()),
                devices: Mutex::new((output.to_string(), input.to_string())),
                out_until: AtomicU64::new(0),
                in_until: AtomicU64::new(0),
                epoch: Instant::now(),
                trouble: Mutex::new((None, None)),
                open: Mutex::new((false, false)),
                stopping: AtomicBool::new(false),
            });
            let theirs = shared.clone();
            std::thread::Builder::new()
                .name("nexus-headset".into())
                .spawn(move || run(&theirs))
                .ok()?;
            Some(Self { shared })
        }

        pub fn rings(&self) -> &Arc<Rings> {
            &self.shared.rings
        }

        /// The devices to use from now on; a direction open on another one opens again.
        pub fn devices(&self, output: &str, input: &str) {
            *self
                .shared
                .devices
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = (output.to_string(), input.to_string());
        }

        /// The output is wanted for `for_` from now: a road listening renews it, as does a sound.
        pub fn want_output(&self, for_: Duration) {
            let until = self.shared.ms(Instant::now() + for_);
            self.shared.out_until.fetch_max(until, Ordering::AcqRel);
        }

        /// The input is wanted for `for_` from now: a live press renews it, as does the meter.
        pub fn want_input(&self, for_: Duration) {
            let until = self.shared.ms(Instant::now() + for_);
            self.shared.in_until.fetch_max(until, Ordering::AcqRel);
        }

        /// Why the output and the input are not open, for the ones that would not open.
        pub fn trouble(&self) -> (Option<String>, Option<String>) {
            self.shared
                .trouble
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }

        /// Whether the output and the input are open now.
        pub fn open(&self) -> (bool, bool) {
            *self.shared.open.lock().unwrap_or_else(|e| e.into_inner())
        }
    }

    impl Drop for Headset {
        fn drop(&mut self) {
            self.shared.stopping.store(true, Ordering::Release);
        }
    }

    /// One direction as the thread holds it.
    #[derive(Default)]
    struct Held {
        stream: Option<Stream>,
        /// The device it is open on.
        device: String,
        /// The device that last would not open, and when.
        failed: Option<(String, Instant)>,
    }

    fn run(shared: &Arc<Shared>) {
        let (mut out, mut input) = (Held::default(), Held::default());
        while !shared.stopping.load(Ordering::Acquire) {
            let now = Instant::now();
            let ms = shared.ms(now);
            let (want_out, want_in) = shared
                .devices
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let out_wanted = ms < shared.out_until.load(Ordering::Acquire);
            let in_wanted = ms < shared.in_until.load(Ordering::Acquire);
            let out_trouble = keep(&mut out, out_wanted, &want_out, now, |device| {
                open_output(device, &shared.rings)
            });
            let in_trouble = keep(&mut input, in_wanted, &want_in, now, |device| {
                open_input(device, &shared.rings)
            });
            if out.stream.is_none() {
                shared.rings.out_rate.store(0, Ordering::Release);
            }
            if input.stream.is_none() {
                shared.rings.in_rate.store(0, Ordering::Release);
            }
            {
                let mut trouble = shared.trouble.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(said) = out_trouble {
                    trouble.0 = said;
                }
                if let Some(said) = in_trouble {
                    trouble.1 = said;
                }
            }
            *shared.open.lock().unwrap_or_else(|e| e.into_inner()) =
                (out.stream.is_some(), input.stream.is_some());
            std::thread::sleep(LOOK);
        }
        // The streams close on this thread, where they opened, and under the host lock.
        let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        drop(out);
        drop(input);
    }

    /// One direction kept as wanted: closed when not wanted, opened (again) on `device` when it is
    /// and is not open there. What changed in why it is not open, if anything.
    fn keep(
        held: &mut Held,
        wanted: bool,
        device: &str,
        now: Instant,
        open: impl FnOnce(&str) -> Result<(Stream, u32), String>,
    ) -> Option<Option<String>> {
        if !wanted {
            if held.stream.is_some() {
                let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                held.stream = None;
            }
            return None;
        }
        if held.stream.is_some() && held.device == device {
            return None;
        }
        let recently = held
            .failed
            .as_ref()
            .is_some_and(|(on, at)| on == device && now.saturating_duration_since(*at) < AGAIN);
        if recently {
            return None;
        }
        let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        held.stream = None;
        match open(device) {
            Ok((stream, _)) => {
                held.stream = Some(stream);
                held.device = device.to_string();
                held.failed = None;
                Some(None)
            }
            Err(why) => {
                held.failed = Some((device.to_string(), now));
                Some(Some(why))
            }
        }
    }

    /// The best buildable config at `rate`, if the device offers one.
    fn config_at(
        configs: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
        rate: u32,
    ) -> Option<cpal::SupportedStreamConfig> {
        configs
            .filter(|range| {
                supported_device_format(range.sample_format())
                    && range.min_sample_rate() <= rate
                    && rate <= range.max_sample_rate()
            })
            .max_by_key(|range| format_quality_rank(range.sample_format()))
            .and_then(|range| range.try_with_sample_rate(rate))
    }

    /// The output on `device` ("" for the default), at 48 kHz where it offers it: the stream and
    /// its rate. The caller holds the host lock.
    fn open_output(device: &str, rings: &Arc<Rings>) -> Result<(Stream, u32), String> {
        let host = cpal::default_host();
        let name = (!device.trim().is_empty()).then_some(device);
        let dev = resolve_configured(
            || host.output_devices().ok(),
            name,
            host.default_output_device(),
            "headset output",
        )?;
        let supported = dev
            .supported_output_configs()
            .ok()
            .and_then(|configs| config_at(configs, crate::receive_decode::PLAY_RATE_HZ))
            .or_else(|| dev.default_output_config().ok())
            .ok_or("this output offers no way to play")?;
        let rate = supported.sample_rate();
        let channels = (supported.channels() as usize).max(1);
        let config = supported.config();
        let stream = dispatch_format!(
            supported.sample_format(),
            build_output,
            "headset output",
            &dev,
            &config,
            channels,
            rate,
            rings,
        )?;
        stream.play().map_err(|e| e.to_string())?;
        rings.out_rate.store(rate, Ordering::Release);
        rings.opened.fetch_add(1, Ordering::AcqRel);
        Ok((stream, rate))
    }

    /// The output callback. REALTIME: the cushion and the rings only.
    fn build_output<T: DeviceSample + Send + 'static>(
        dev: &cpal::Device,
        config: &cpal::StreamConfig,
        channels: usize,
        rate: u32,
        rings: &Arc<Rings>,
    ) -> Result<Stream, String> {
        let rings = rings.clone();
        let mut cushion = Cushion::new(rate);
        dev.build_output_stream(
            *config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                cushion.begin(&rings);
                for frame in data.chunks_mut(channels) {
                    let sample = T::from_unit(cushion.next(&rings));
                    for slot in frame.iter_mut() {
                        *slot = sample;
                    }
                }
            },
            |e| eprintln!("tempo-audio: headset output error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
    }

    /// The input on `device` ("" for the default), at its own rate: the stream and its rate. The
    /// caller holds the host lock.
    fn open_input(device: &str, rings: &Arc<Rings>) -> Result<(Stream, u32), String> {
        let host = cpal::default_host();
        let name = (!device.trim().is_empty()).then_some(device);
        let dev = resolve_configured(
            || host.input_devices().ok(),
            name,
            host.default_input_device(),
            "headset microphone",
        )?;
        let supported = dev.default_input_config().map_err(|e| e.to_string())?;
        let rate = supported.sample_rate();
        let channels = (supported.channels() as usize).max(1);
        let format: SampleFormat = supported.sample_format();
        let stream = open_capture_with_fallback(capture_config(&supported), |config| {
            dispatch_format!(
                format,
                build_input,
                "headset microphone",
                &dev,
                config,
                channels,
                rings,
            )
        })?;
        stream.play().map_err(|e| e.to_string())?;
        rings.in_rate.store(rate, Ordering::Release);
        rings.opened.fetch_add(1, Ordering::AcqRel);
        Ok((stream, rate))
    }

    /// The input callback, folding each frame to mono by averaging. REALTIME: the ring only.
    fn build_input<T: DeviceSample + Send + 'static>(
        dev: &cpal::Device,
        config: cpal::StreamConfig,
        channels: usize,
        rings: &Arc<Rings>,
    ) -> Result<Stream, String> {
        let rings = rings.clone();
        dev.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                rings.captured(data.chunks(channels).map(|frame| {
                    frame.iter().map(|&s| s.to_unit()).sum::<f32>() / channels as f32
                }));
            },
            |e| eprintln!("tempo-audio: headset microphone error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn ms(n: u32) -> usize {
        (RATE as usize) * (n as usize) / 1000
    }

    /// `n` samples through the output's cushion, one callback.
    fn out(cushion: &mut Cushion, rings: &Rings, n: usize) -> Vec<f32> {
        cushion.begin(rings);
        (0..n).map(|_| cushion.next(rings)).collect()
    }

    /// ★ Playback waits for the cushion, plays it, and when it runs dry fills the cushion again
    /// before it plays on: the bed meanwhile, quiet, never a burst. CONTROL: once the cushion is
    /// there, what plays is the shack's audio, lifted by the gain.
    #[test]
    fn playback_starts_on_the_cushion_and_refills_it_after_running_dry() {
        let rings = Rings::default();
        rings.listen(true);
        let mut cushion = Cushion::new(RATE);
        rings.play(&vec![0.01; ms(40)]);
        let waiting = out(&mut cushion, &rings, ms(20));
        assert!(
            waiting.iter().all(|s| s.abs() <= BED_LEVEL * gain()),
            "played before the cushion was there"
        );
        assert!(waiting.iter().any(|s| *s != 0.0), "no bed while waiting");
        rings.play(&vec![0.01; ms(20)]);
        let playing = out(&mut cushion, &rings, ms(60));
        let lifted = 0.01 * gain();
        assert!(
            playing.iter().all(|s| (s - lifted).abs() < 1e-6),
            "the cushion did not play"
        );
        let bed = |samples: &[f32]| samples.iter().all(|s| s.abs() <= BED_LEVEL * gain());
        assert!(bed(&out(&mut cushion, &rings, ms(20))), "played on dry");
        // 20 ms more is not a cushion: it waits under the bed until there is one.
        rings.play(&vec![0.01; ms(20)]);
        assert!(
            bed(&out(&mut cushion, &rings, ms(20))),
            "played before the cushion"
        );
        rings.play(&vec![0.01; ms(40)]);
        let again = out(&mut cushion, &rings, ms(20));
        assert!(
            again.iter().all(|s| (s - lifted).abs() < 1e-6),
            "never played again"
        );
    }

    /// ★ Past the cushion and 100 ms more, the oldest audio goes, back to the cushion: the listener
    /// hears the band as it is now, never a speed-up. CONTROL: under the ceiling nothing goes.
    #[test]
    fn audio_past_the_ceiling_drops_back_to_the_cushion() {
        let rings = Rings::default();
        let mut cushion = Cushion::new(RATE);
        rings.play(&vec![0.01; ms(CEILING_MS)]);
        let _ = out(&mut cushion, &rings, 0);
        assert_eq!(
            rings.waiting(),
            ms(CEILING_MS),
            "under the ceiling, nothing goes"
        );
        rings.play(&vec![0.01; ms(40)]);
        let _ = out(&mut cushion, &rings, 0);
        assert_eq!(rings.waiting(), ms(CUSHION_MS));
    }

    /// ★ M9: muted, never ducked. While muted the output is silence (no bed, no audio), what was
    /// waiting is dropped and nothing new is queued, so nothing from before the over plays after
    /// it. The tone plays through the mute. CONTROL: unmuted, the audio sent then plays.
    #[test]
    fn muted_is_silence_and_drops_what_waited_but_the_tone_plays() {
        let rings = Rings::default();
        rings.listen(true);
        let mut cushion = Cushion::new(RATE);
        rings.play(&vec![0.02; ms(100)]);
        rings.mute(true);
        let muted = out(&mut cushion, &rings, ms(20));
        assert!(
            muted.iter().all(|s| *s == 0.0),
            "a muted output made a sound"
        );
        assert_eq!(rings.waiting(), 0, "what waited survived the mute");
        rings.play(&vec![0.02; ms(100)]);
        assert_eq!(rings.waiting(), 0, "queued while muted");
        rings.sound(&sound(Sound::OverEnded, RATE));
        let tone = out(&mut cushion, &rings, ms(400));
        assert!(
            tone.iter().any(|s| s.abs() > 0.05),
            "the tone did not play muted"
        );
        rings.mute(false);
        rings.play(&vec![0.02; ms(100)]);
        let heard = out(&mut cushion, &rings, ms(20));
        assert!(
            heard.iter().all(|s| (s - 0.02 * gain()).abs() < 1e-6),
            "{:?}",
            &heard[..4]
        );
    }

    /// ★ The tone goes to the output only: the microphone's ring holds what the input captured and
    /// nothing else, a sound included. CONTROL: what the input captures is what is taken, folded,
    /// and its peak is the meter's.
    #[test]
    fn the_tone_never_reaches_the_microphone() {
        let rings = Rings::default();
        rings.sound(&sound(Sound::OverEnded, RATE));
        rings.sound(&sound(Sound::Test, RATE));
        assert!(
            rings.take_mic().is_empty(),
            "a sound reached the microphone"
        );
        rings.captured([0.25f32, -0.5, 0.125].into_iter());
        assert_eq!(rings.take_mic(), [0.25, -0.5, 0.125]);
        assert_eq!(rings.peak(), 0.5);
        assert_eq!(rings.peak(), 0.0, "the peak is since the last read");
    }

    /// The sounds are short, quiet and click-free: under -17 dBFS, eased in and out.
    #[test]
    fn the_sounds_are_short_quiet_and_eased() {
        for which in [Sound::OverEnded, Sound::Test] {
            let samples = sound(which, RATE);
            assert!(samples.len() <= ms(600), "{which:?}");
            assert!(samples.iter().all(|s| s.abs() <= 0.125), "{which:?}");
            assert!(samples[0].abs() < 1e-3 && samples[samples.len() - 1].abs() < 0.01);
        }
    }
}
