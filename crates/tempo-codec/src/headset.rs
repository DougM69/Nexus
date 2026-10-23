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
//! ## What is heard: the hosted page's rules, at the road's cushion
//!
//! - **A fixed cushion** ([`CUSHION_MS`] on the LAN, [`INTERNET_CUSHION_MS`] on the internet, set by
//!   the road the link opened on, [`Rings::set_cushion_ms`]): playback starts once that much is
//!   waiting, and again the same way whenever it runs dry. Past the cushion and 100 ms more
//!   ([`CEILING_MS`] on the LAN) the oldest audio goes, back down to the cushion: a drop, never a
//!   speed-up or a slow-down.
//! - **A quiet bed in a known gap** ([`BED_LEVEL`]): while listening with nothing to play, a noise
//!   some 8 dB under a quiet band, as the page plays, so a gap never sounds like a dead link and
//!   never bursts.
//! - **The page's level** ([`GAIN_DB`]): the station sets its receive level for its decoders, some
//!   40 dB under a normal listen, so it is lifted by the page's default volume and held under
//!   [`PEAK`]. The operator's own volume moves the lift, as the page's slider does, from
//!   [`VOLUME_DB_MIN`] to [`VOLUME_DB_MAX`] ([`Rings::set_volume_db`]); the bed moves with it.
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
//! The devices behind it, opened only while wanted, are tempo-audio's (`headset::Headset`, with its
//! `device` feature): this is what any headset plays and records through, a test's stand-in too.
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::ring::SpscRing;

/// The cushion on the LAN: playback starts once this much is waiting.
pub const CUSHION_MS: u32 = 60;
/// The cushion on the internet, where jitter is larger: twice the LAN's.
pub const INTERNET_CUSHION_MS: u32 = 120;
/// How far past the cushion audio may wait before the oldest goes, back down to the cushion.
pub const HEADROOM_MS: u32 = 100;
/// Past this much waiting on the LAN, the oldest audio goes, back down to the cushion.
pub const CEILING_MS: u32 = CUSHION_MS + HEADROOM_MS;
/// The bed, in the station's own scale before the gain: the hosted page's, about -68 dBFS RMS.
pub const BED_LEVEL: f32 = 0.0006;
/// The station's level lifted by the hosted page's default volume.
pub const GAIN_DB: f32 = 24.0;
/// The quietest and loudest volume the operator may choose: the hosted page's slider's range.
pub const VOLUME_DB_MIN: f32 = 0.0;
pub const VOLUME_DB_MAX: f32 = 42.0;
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
    /// The open output's and input's rates, 0 while closed. Public, with `opened`, for the headset
    /// on this computer's devices (tempo-audio's `headset::Headset`), which sets them as each opens.
    pub out_rate: AtomicU32,
    pub in_rate: AtomicU32,
    /// The microphone's peak since the meter last read it, as `f32` bits.
    peak: AtomicU32,
    /// Moves each time a direction opens, so a reader at the old rate starts again.
    pub opened: AtomicU64,
    /// The operator's volume as the factor the shack's audio is lifted by, as `f32` bits.
    lift: AtomicU32,
    /// Samples of the shack's audio the output had no room for.
    dropped: AtomicU64,
    /// The cushion for the road the link is on, in ms: [`CUSHION_MS`] until one is set.
    cushion_ms: AtomicU32,
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
            lift: AtomicU32::new(gain().to_bits()),
            dropped: AtomicU64::new(0),
            cushion_ms: AtomicU32::new(CUSHION_MS),
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

    /// The operator's volume, in dB over the station's own level: held inside [`VOLUME_DB_MIN`]
    /// and [`VOLUME_DB_MAX`], and heard from the next audio played.
    pub fn set_volume_db(&self, db: f32) {
        let db = if db.is_finite() { db } else { GAIN_DB };
        let db = db.clamp(VOLUME_DB_MIN, VOLUME_DB_MAX);
        self.lift
            .store(10f32.powf(db / 20.0).to_bits(), Ordering::Release);
    }

    /// The cushion for the road the link opened on: [`CUSHION_MS`] on the LAN,
    /// [`INTERNET_CUSHION_MS`] on the internet. The output takes it at its next callback.
    pub fn set_cushion_ms(&self, ms: u32) {
        self.cushion_ms.store(ms, Ordering::Release);
    }

    /// The cushion set for the road the link is on, in ms.
    pub fn cushion_ms(&self) -> u32 {
        self.cushion_ms.load(Ordering::Acquire)
    }

    /// The factor the shack's audio is lifted by: the operator's volume, [`gain`] until one is set.
    pub fn lift(&self) -> f32 {
        f32::from_bits(self.lift.load(Ordering::Acquire))
    }

    /// The shack's audio, at the output's rate: lifted by the operator's volume ([`Rings::lift`]),
    /// held under [`PEAK`] and queued. Nothing is queued while muted, and what will not fit is
    /// dropped and counted.
    pub fn play(&self, samples: &[f32]) {
        let _writer = lock(&self.play_writer);
        if self.muted.load(Ordering::Acquire) {
            return;
        }
        let lift = self.lift();
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

/// `ms` of audio at `rate`, in samples.
fn samples(rate: u32, ms: u32) -> usize {
    (rate as usize) * (ms as usize) / 1000
}

/// The output callback's own state: whether it is playing or filling the cushion, and the bed.
pub struct Cushion {
    rate: u32,
    /// The cushion in ms these two were made for.
    made_for: u32,
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
        Self {
            rate,
            made_for: CUSHION_MS,
            cushion: samples(rate, CUSHION_MS),
            ceiling: samples(rate, CEILING_MS),
            playing: false,
            bed: BED_LEVEL * gain(),
            noise: 0x2545_f491,
            muted: false,
            listening: false,
        }
    }

    /// At the top of each output callback: the mute, the bed at the operator's volume, and the
    /// ceiling. REALTIME.
    pub fn begin(&mut self, rings: &Rings) {
        let ms = rings.cushion_ms.load(Ordering::Acquire);
        if ms != self.made_for {
            self.made_for = ms;
            self.cushion = samples(self.rate, ms);
            self.ceiling = samples(self.rate, ms + HEADROOM_MS);
        }
        self.muted = rings.muted.load(Ordering::Acquire);
        self.listening = rings.listening.load(Ordering::Acquire);
        self.bed = BED_LEVEL * rings.lift();
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

    /// ★ The operator's volume lifts what plays, from the page's 0 dB to its 42 dB and never past
    /// them, and the bed moves with it; held under the peak. CONTROL: until it is set, the lift is
    /// the page's default 24 dB.
    #[test]
    fn the_volume_lifts_what_plays_inside_the_pages_range() {
        let rings = Rings::default();
        assert!((rings.lift() - gain()).abs() < 1e-6, "the default");
        let played = |db: f32| {
            rings.set_volume_db(db);
            rings.play(&[0.001]);
            let mut cushion = Cushion::new(RATE);
            cushion.begin(&rings);
            (rings.play.pop().unwrap(), cushion.bed)
        };
        let (quiet, quiet_bed) = played(0.0);
        assert!(
            (quiet - 0.001).abs() < 1e-7,
            "0 dB plays the station's own level: {quiet}"
        );
        let (loud, loud_bed) = played(42.0);
        let lifted = 0.001 * 10f32.powf(42.0 / 20.0);
        assert!((loud - lifted).abs() < 1e-5, "42 dB: {loud} vs {lifted}");
        assert!(
            (loud_bed / quiet_bed - lifted / 0.001).abs() < 1e-2,
            "the bed moved with it"
        );
        assert_eq!(
            played(-6.0).0,
            played(0.0).0,
            "below the range is its floor"
        );
        assert_eq!(played(60.0).0, played(42.0).0, "above the range is its top");
        rings.set_volume_db(42.0);
        rings.play(&[0.5]);
        assert_eq!(rings.play.pop(), Some(PEAK), "held under the peak");
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

    /// ★ On the internet the cushion is 120 ms: playback waits for it, and past it and 100 ms more
    /// the oldest goes, back to it. CONTROL: on the LAN's 60 ms, what was waiting then plays.
    #[test]
    fn on_the_internet_playback_waits_for_the_longer_cushion() {
        let rings = Rings::default();
        rings.listen(true);
        rings.set_cushion_ms(INTERNET_CUSHION_MS);
        let mut cushion = Cushion::new(RATE);
        rings.play(&vec![0.01; ms(100)]);
        let bed = |samples: &[f32]| samples.iter().all(|s| s.abs() <= BED_LEVEL * gain());
        assert!(
            bed(&out(&mut cushion, &rings, ms(10))),
            "played before 120 ms waited"
        );
        rings.play(&vec![0.01; ms(30)]);
        rings.play(&vec![0.01; ms(200)]);
        let _ = out(&mut cushion, &rings, 0);
        assert_eq!(rings.waiting(), ms(INTERNET_CUSHION_MS), "the ceiling");
        let lifted = 0.01 * gain();
        let playing = out(&mut cushion, &rings, ms(20));
        assert!(playing.iter().all(|s| (s - lifted).abs() < 1e-6));
        let lan = Rings::default();
        lan.listen(true);
        let mut cushion = Cushion::new(RATE);
        lan.play(&vec![0.01; ms(100)]);
        let playing = out(&mut cushion, &lan, ms(10));
        assert!(playing.iter().all(|s| (s - lifted).abs() < 1e-6), "LAN");
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
