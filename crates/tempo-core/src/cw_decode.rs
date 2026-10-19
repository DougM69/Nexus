//! Single-signal CW decoder — reads Morse from the receive audio at the operator's CW
//! pitch. Pipeline: Goertzel envelope at the pitch → adaptive threshold → mark/space
//! segments → adaptive unit (WPM) → dit/dah + gap classification → Morse → text.
//!
//! Pure + deterministic. Tuned + tested against [`crate::cw::morse_samples`] (clean,
//! machine-timed CW); weak/hand-sent signals are the expected on-air tuning frontier.

use crate::cw::morse_code;
use crate::spectrum::tone_power;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Reverse of [`morse_code`]: a Morse string ("-.-.") → its character. Built once from
/// the forward table (so it stays in sync with whatever glyphs the table supports).
fn morse_to_char(code: &str) -> Option<char> {
    static REV: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    let map = REV.get_or_init(|| {
        let mut m = HashMap::new();
        for u in 0x20u8..0x7f {
            let c = (u as char).to_ascii_uppercase();
            if let Some(code) = morse_code(c) {
                m.entry(code).or_insert(c);
            }
        }
        m
    });
    map.get(code).copied()
}

/// A CW decode result: the recovered text and the estimated sending speed (WPM).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CwDecode {
    pub text: String,
    pub wpm: u32,
}

/// Decode CW from `samples` (mono f32) heard at `pitch_hz`, sampled at `sr` Hz.
/// Returns empty text when there's no clear keyed signal in the buffer.
pub fn decode_cw(samples: &[f32], sr: f32, pitch_hz: f32) -> CwDecode {
    if sr <= 0.0 || samples.len() < (sr * 0.05) as usize {
        return CwDecode::default(); // < ~50 ms — nothing to decode
    }
    // 1. Envelope: non-overlapping ~4 ms hops of Goertzel power at the pitch.
    let hop = (sr * 0.004).max(1.0) as usize;
    let env: Vec<f32> = samples
        .chunks(hop)
        .filter(|c| c.len() == hop)
        .map(|c| tone_power(c, sr, pitch_hz))
        .collect();
    if env.len() < 8 {
        return CwDecode::default();
    }
    // 2. Threshold: midpoint between the noise floor (low percentile) and the signal
    //    (high percentile). Require a clear on/off ratio, else there's no keying.
    let mut sorted = env.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let lo = sorted[sorted.len() / 10].max(1e-9);
    let hi = sorted[sorted.len() * 9 / 10];
    if hi < lo * 3.0 {
        return CwDecode::default(); // no clear keyed signal (steady noise/carrier/silence)
    }
    let thresh = (lo + hi) * 0.5;
    // 3. Segments: runs of mark (key-down) / space (key-up), in hops.
    let mut segs: Vec<(bool, usize)> = Vec::new();
    for &p in &env {
        let mark = p >= thresh;
        match segs.last_mut() {
            Some((m, n)) if *m == mark => *n += 1,
            _ => segs.push((mark, 1)),
        }
    }
    // Trim leading silence so a long pre-signal gap can't emit spurious spaces.
    let start = segs.iter().position(|(m, _)| *m).unwrap_or(segs.len());
    let segs = &segs[start..];
    if segs.is_empty() {
        return CwDecode::default();
    }
    // 4. Adaptive unit (1 dit, in hops): a low percentile of all element durations —
    //    dits and intra-character gaps are the shortest, most-common elements (1 unit).
    let mut durs: Vec<usize> = segs.iter().map(|(_, n)| *n).collect();
    durs.sort_unstable();
    let rough = (durs[durs.len() / 5].max(1)) as f32;
    // Refine the dit length by averaging the whole 1-unit cluster (elements < 2× rough):
    // threshold-crossing trims a hop off each dit but adds it to the adjacent intra-char
    // gap, so the dit-and-gap mean cancels that bias → an accurate WPM.
    let ones: Vec<usize> = durs
        .iter()
        .copied()
        .filter(|&d| (d as f32) < 2.0 * rough)
        .collect();
    let unit = if ones.is_empty() {
        rough
    } else {
        ones.iter().sum::<usize>() as f32 / ones.len() as f32
    };
    // 5. Decode: marks → dit/dah (boundary 2 units); spaces → intra (<2u) / inter (2–5u,
    //    emit the character) / word (≥5u, also a space).
    let mut text = String::new();
    let mut sym = String::new();
    let flush = |sym: &mut String, text: &mut String| {
        if !sym.is_empty() {
            if let Some(c) = morse_to_char(sym) {
                text.push(c);
            }
            sym.clear();
        }
    };
    for (mark, n) in segs {
        let u = *n as f32 / unit;
        if *mark {
            sym.push(if u < 2.0 { '.' } else { '-' });
        } else if u >= 2.0 {
            flush(&mut sym, &mut text);
            if u >= 5.0 {
                text.push(' ');
            }
        }
    }
    flush(&mut sym, &mut text);
    // WPM from the dit unit: dit_secs = unit_hops × hop / sr; PARIS wpm = 1.2 / dit_secs.
    let dit_secs = unit * hop as f32 / sr;
    let wpm = if dit_secs > 0.0 {
        (1.2 / dit_secs).round().clamp(0.0, 99.0) as u32
    } else {
        0
    };
    CwDecode {
        text: text.trim().to_string(),
        wpm,
    }
}

/// A CW signal found by the wideband skimmer: its audio pitch (Hz), decoded text, and WPM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkimHit {
    pub pitch_hz: u32,
    pub text: String,
    pub wpm: u32,
}

/// Wideband CW skimmer: decode `samples` at every audio pitch from `lo_hz` to `hi_hz` in
/// `step_hz` steps and return the channels that yielded readable text. A single signal
/// smears across a few neighbouring channels (the Goertzel has finite bandwidth), so runs
/// of adjacent channels are merged, keeping the longest (best-centred) decode. This reuses
/// the single-signal [`decode_cw`] per channel — Tier 2 of CW decode.
pub fn skim_cw(samples: &[f32], sr: f32, lo_hz: u32, hi_hz: u32, step_hz: u32) -> Vec<SkimHit> {
    let step = step_hz.max(1) as usize;
    let channels: Vec<u32> = (lo_hz..=hi_hz).step_by(step).collect();
    if channels.len() < 3 {
        return Vec::new();
    }
    // 1. Total Goertzel power per channel over the whole buffer — the band's spectrum.
    let powers: Vec<f32> = channels
        .iter()
        .map(|&f| tone_power(samples, sr, f as f32))
        .collect();
    // 2. Noise floor = median power; a real carrier peak stands well above it. Decoding
    //    every channel (not just peaks) is what let one signal's Goertzel sidelobes post
    //    garbage on far-off channels — so decode ONLY at local-maximum peaks above the floor.
    let mut sorted = powers.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = sorted[sorted.len() / 2].max(1e-9);
    let mut hits: Vec<SkimHit> = Vec::new();
    for i in 0..channels.len() {
        let p = powers[i];
        let peak = p > floor * 4.0
            && powers.get(i.wrapping_sub(1)).is_none_or(|&l| p >= l)
            && powers.get(i + 1).is_none_or(|&r| p >= r);
        if !peak {
            continue;
        }
        let d = decode_cw(samples, sr, channels[i] as f32);
        let glyphs = d.text.chars().filter(|c| !c.is_whitespace()).count();
        if glyphs >= 3 && (8..=50).contains(&d.wpm) && !hits.iter().any(|h| h.text == d.text) {
            hits.push(SkimHit {
                pitch_hz: channels[i],
                text: d.text,
                wpm: d.wpm,
            });
        }
    }
    hits
}

const CW_TRANSCRIPT_CAP: usize = 4000; // keep the tail; drop older text
                                       // Every noise gate is operator-scaled by the sensitivity slider — the Schmitt keying
                                       // rails (hi_frac/lo_frac), the sub-dit spike fraction (spike_frac), the per-mark SNR +
                                       // presence multipliers (snr_mult/present_mult), and the Morse-timing plausibility
                                       // squelch (timing_plausible). 0.5 = the historical fixed behavior.

/// Streaming, stateful CW decoder — the persistent-transcript sibling of [`decode_cw`].
///
/// Feed it receive audio incrementally (as it arrives) and it ACCUMULATES decoded text
/// that doesn't vanish when old audio scrolls off: the batch [`decode_cw`] re-reads a
/// sliding window every call, so its output churns and the last characters disappear
/// within seconds. This one emits each character once, into a growing transcript.
///
/// Pipeline per ~4 ms envelope hop, hardened toward what fldigi does:
/// Goertzel power at the pitch → **fast-attack peak / slow noise-floor AGC** (flags a
/// signal on its first hop; adapts to fading) →
/// **Schmitt-trigger** key state (two thresholds → no chatter near the noise floor) →
/// mark/space runs with **sub-dit spike rejection** → **adaptive dit unit** tracked from
/// a rolling mark history (follows the sender's speed) → dit/dah + gap → Morse → char.
/// Offsets (Hz) either side of the signal pitch at which to probe band noise for the true-SNR
/// gates. Several spread probes + taking the QUIETEST (min) mean `off_noise` only inflates when
/// EVERY probe lands on another signal — so a single adjacent QSO (or even a couple on a packed
/// contest band) can't gate out the wanted signal, and no single "unlucky" offset dominates. The
/// low-side probe is skipped when it would fall at/below ~DC (a low operator pitch).
const OFF_NOISE_HZ: [f32; 4] = [250.0, 400.0, 600.0, 800.0];

/// The dit (30 WPM) the spike floor scales from before the speed has locked, instead of the
/// 20 WPM seed: even the strictest floor (0.55 of it, 22 ms) passes a 50 WPM dit (24 ms).
const FASTEST_DIT_SECS: f32 = 1.2 / 30.0;

pub struct CwStreamDecoder {
    sr: f32,
    pitch: f32,
    hop: usize,
    carry: Vec<f32>, // audio not yet a full hop
    noise: f32,      // slow-tracking ON-PITCH floor follower (drives the Schmitt keying rails)
    off_noise: f32,  // OFF-PITCH band-noise follower (drives the true-SNR presence/mark gates)
    peak: f32,       // fast-attack, slow-decay signal-peak follower (AGC)
    mark_peak: f32,  // peak power within the current key-down (per-mark SNR gate)
    lo_thresh: f32,  // Schmitt lower rail (leave mark below this)
    hi_thresh: f32,  // Schmitt upper rail (enter mark at/above this)
    present: bool,   // a keyed signal is present (else don't decode noise)
    key_down: bool,
    run: usize,         // current key-run length (hops), mark or space per `key_down`
    space_run: usize,   // accumulated gap hops (carries THROUGH sub-dit spikes)
    dit_avg: f32,       // adaptive dit length (hops)
    dah_avg: f32,       // adaptive dah length (hops); the pair classify each mark
    dah_seen: bool,     // dah_avg has learned from a real mark (else it is still the seed)
    last_run: f32,      // length of the previous real mark (hops); 0 before the first
    misfit: u8,         // consecutive dit/dah pairs the trackers split wrongly
    locked: bool,       // a dit/dah pair has confirmed the trackers (the seed is not a speed)
    saw_mark: bool,     // any real mark seen yet (gates WPM before sync)
    sym: String,        // in-progress Morse symbol (".-")
    sym_runs: Vec<f32>, // the mark lengths behind `sym`, so a re-sync can re-read it
    flushed_char: bool, // this gap already flushed its pending character
    flushed_word: bool, // this gap already emitted a word space
    transcript: String,
    sensitivity: f32, // 0..1 operator control; scales the presence + per-mark SNR gates
}

impl CwStreamDecoder {
    /// A decoder for audio sampled at `sr` Hz, listening at `pitch_hz`.
    pub fn new(sr: f32, pitch_hz: f32) -> Self {
        let hop = (sr * 0.004).max(1.0) as usize;
        // Seed the dit unit at ~20 wpm so the first character classifies sensibly before
        // the speed tracker has any marks to learn from; it converges within a few elements.
        let default_dit = ((1.2 / 20.0) * sr / hop.max(1) as f32).max(1.0);
        Self {
            sr,
            pitch: pitch_hz,
            hop,
            carry: Vec::new(),
            noise: 1e-9,
            off_noise: 1e-9,
            peak: 0.0,
            mark_peak: 0.0,
            lo_thresh: 0.0,
            hi_thresh: f32::INFINITY,
            present: false,
            key_down: false,
            run: 0,
            space_run: 0,
            dit_avg: default_dit,
            dah_avg: 3.0 * default_dit,
            dah_seen: false,
            last_run: 0.0,
            misfit: 0,
            locked: false,
            saw_mark: false,
            sym: String::new(),
            sym_runs: Vec::new(),
            flushed_char: false,
            flushed_word: false,
            transcript: String::new(),
            sensitivity: 0.5, // 0.5 == the historical fixed gates (SNR ×4, presence ×3)
        }
    }

    /// Operator decode sensitivity in [0, 1]. Higher = catch weaker/off-pitch marks (like the
    /// wideband skimmer, at the cost of more noise); lower = stricter. 0.5 keeps the original
    /// gates. Scales the presence gate (×`present_mult`) and the per-mark SNR gate.
    pub fn set_sensitivity(&mut self, s: f32) {
        self.sensitivity = s.clamp(0.0, 1.0);
    }

    /// Per-mark SNR multiplier: 0→×6 (strict) · 0.5→×4 (default) · 1→×2 (loose).
    fn snr_mult(&self) -> f32 {
        6.0 - 4.0 * self.sensitivity
    }

    /// Presence-gate multiplier: 0→×4 · 0.5→×3 (default) · 1→×2.
    fn present_mult(&self) -> f32 {
        4.0 - 2.0 * self.sensitivity
    }

    /// Schmitt upper-rail fraction of the AGC span: 0→0.85 (strict) · 0.5→0.60
    /// (the historical fixed rail) · 1→0.35 (loose). THIS is the gate that
    /// actually bites on a noisy band: the noise floor follower rides the
    /// envelope MINIMUM, so against real band noise the peak/floor ratio dwarfs
    /// the snr/present multipliers and they saturate at every slider position —
    /// the operator-visible effect must come from how much of the peak−floor
    /// span a mark must climb before it keys.
    fn hi_frac(&self) -> f32 {
        0.85 - 0.5 * self.sensitivity
    }

    /// Schmitt lower rail rides 0.20 below the upper (hysteresis width unchanged).
    fn lo_frac(&self) -> f32 {
        self.hi_frac() - 0.20
    }

    /// Sub-dit spike-rejection fraction: 0→0.55 (strict) · 0.5→0.40 (the historical
    /// CW_SPIKE_FRAC) · 1→0.25 (loose). Noise storms are made of barely-dit marks;
    /// the strict end simply refuses them.
    fn spike_frac(&self) -> f32 {
        0.55 - 0.3 * self.sensitivity
    }

    /// Morse-timing plausibility half-width: real keying holds dah ≈ 3×dit. A noise
    /// storm drags one tracker toward spike length while the other lags → the ratio
    /// leaves the physical band, and characters emitted in that state are muted.
    /// Band = [3/k, 3k]: 0→k 1.6 (strict) · 0.5→k 3.0 (permissive ≈ historical) ·
    /// 1→k 5.8 (anything goes).
    fn timing_plausible(&self) -> bool {
        let k = 1.6 + 2.8 * self.sensitivity;
        let ratio = self.dah_avg / self.dit_avg.max(1e-6);
        (3.0 / k..=3.0 * k).contains(&ratio)
    }

    /// Retune to a new marker pitch. No-op if unchanged; otherwise resets all state +
    /// transcript (the old text belonged to a different signal) but keeps the operator's
    /// sensitivity setting.
    pub fn retune(&mut self, pitch_hz: f32) {
        if (pitch_hz - self.pitch).abs() > 1.0 {
            let s = self.sensitivity;
            *self = Self::new(self.sr, pitch_hz);
            self.sensitivity = s;
        }
    }

    /// The accumulated decoded text.
    pub fn transcript(&self) -> &str {
        &self.transcript
    }

    /// Estimated sending speed (WPM) from the current dit unit; 0 before any marks.
    ///
    /// Once both trackers have learned, the unit is half the dah-dit difference: a dah is
    /// two units longer than a dit however the sender weights the marks, and the envelope
    /// detector's constant lengthening or shortening of every mark cancels (measured dits
    /// at 40 WPM ran a hop short, reading ~10% fast).
    pub fn wpm(&self) -> u32 {
        if !self.saw_mark {
            return 0;
        }
        let unit = if self.dah_seen && self.dah_avg > 2.0 * self.dit_avg {
            (self.dah_avg - self.dit_avg) * 0.5
        } else {
            self.dit_avg
        };
        let dit_secs = unit * self.hop as f32 / self.sr;
        if dit_secs > 0.0 {
            (1.2 / dit_secs).round().clamp(0.0, 99.0) as u32
        } else {
            0
        }
    }

    /// Clear the accumulated text (keeps the learned speed/threshold so decoding of an
    /// in-progress signal continues cleanly).
    pub fn clear(&mut self) {
        self.transcript.clear();
        self.sym.clear();
        self.sym_runs.clear();
        self.flushed_char = false;
        self.flushed_word = false;
    }

    /// Feed newly-arrived audio; decoded characters are appended to the transcript.
    pub fn push(&mut self, samples: &[f32]) {
        if self.sr <= 0.0 || self.hop == 0 {
            return;
        }
        self.carry.extend_from_slice(samples);
        let mut i = 0;
        while i + self.hop <= self.carry.len() {
            let hop_slice = &self.carry[i..i + self.hop];
            let p = tone_power(hop_slice, self.sr, self.pitch);
            // TRUE band-noise reference: the tone power just OFF the signal, taking the quieter of
            // the two sides so an adjacent signal on one side doesn't inflate it. This — not the
            // on-pitch floor — is what the SNR gates must measure against: between a real signal's
            // own elements the on-pitch floor sits near silence, so a peak/on-pitch-floor ratio is
            // enormous and the presence/mark-SNR gates (and the operator's sensitivity slider that
            // scales them) saturate. Peak vs OFF-pitch noise is a real SNR the slider can move.
            let mut off = f32::INFINITY;
            for &d in &OFF_NOISE_HZ {
                off = off.min(tone_power(hop_slice, self.sr, self.pitch + d));
                if self.pitch - d >= 250.0 {
                    // skip a low-side probe below the RX audio passband (rumble/hum region on a
                    // low pitch) — it reads near-nothing and would drag the min to a false zero.
                    off = off.min(tone_power(hop_slice, self.sr, self.pitch - d));
                }
            }
            // The min over a WIDE spread only inflates if the whole neighbourhood is occupied
            // (genuinely high band noise); a single strong neighbour can't. Fall back to the
            // on-pitch floor if every probe was skipped (unreachable: the high side always runs).
            let off = if off.is_finite() { off } else { self.noise };
            i += self.hop;
            // Threshold from the CURRENT followers (before updating them) so a signal onset
            // is judged against the PRIOR noise floor and detected on its first hop — no
            // warmup dead-zone (a percentile window can't flag a signal until it fills a
            // fifth of the window, which truncated the first character into a dit).
            let span = (self.peak - self.noise).max(0.0);
            // Presence gates on the TRUE (off-pitch) SNR; the Schmitt rails still ride the on-pitch
            // floor + span (they define keying WITHIN the signal's own envelope).
            self.present =
                self.peak >= self.noise.max(self.off_noise) * self.present_mult() && span > 1e-9;
            self.hi_thresh = self.noise + self.hi_frac() * span; // Schmitt upper rail
            self.lo_thresh = self.noise + self.lo_frac() * span; // Schmitt lower rail (hysteresis)
            self.step(p);
            // Update the AGC: the peak attacks instantly and decays slowly (holds through
            // inter-character gaps); the noise floor drops instantly to a quieter hop and
            // creeps up slowly (so a long key-down barely lifts it).
            self.peak = if p > self.peak {
                p
            } else {
                self.peak * 0.999 + p * 0.001
            };
            self.noise = if p < self.noise {
                p
            } else {
                self.noise * 0.9995 + p * 0.0005
            };
            // Off-pitch band noise tracks with a plain EMA — it holds through a signal's marks
            // (leakage into the off bins is only a fraction of the on-pitch peak) and settles to
            // the real floor in the gaps.
            self.off_noise = self.off_noise * 0.98 + off * 0.02;
        }
        self.carry.drain(0..i);
    }

    fn step(&mut self, p: f32) {
        if self.key_down {
            self.run += 1;
            self.mark_peak = self.mark_peak.max(p);
            if p < self.lo_thresh {
                self.on_mark_end();
                self.key_down = false;
                self.run = 0;
            }
        } else if self.present && p >= self.hi_thresh {
            // A real mark starts. Any pending character was already flushed by the gap
            // ticks below, so just begin the mark.
            self.key_down = true;
            self.run = 1;
            self.mark_peak = p;
        } else {
            self.space_run += 1;
            self.on_gap_tick();
        }
    }

    fn on_mark_end(&mut self) {
        // Until a dit/dah pair has confirmed the speed, the dit tracker is only the 20 WPM
        // seed (or a dah misread as a dit), and a floor scaled from it rejects a fast
        // sender's real dits (0.4 of a 20 WPM dit is a whole 50 WPM dit), so the trackers
        // never see a pair to lock on. Before the lock the floor scales from a 30 WPM dit
        // instead.
        let unit = if self.locked {
            self.dit_avg
        } else {
            FASTEST_DIT_SECS * self.sr / self.hop as f32
        };
        let spike_min = (self.spike_frac() * unit).max(1.0);
        // Reject a mark that is too SHORT (impulse) OR too WEAK (its peak barely clears the
        // noise floor — whichever of the on-pitch and off-pitch estimates is larger). The weak
        // case kills the "E E E…" storm the decoder otherwise emits from noise between signals:
        // a real keyed element sits well above BOTH floors. Taking the max keeps the on-pitch
        // storm gate alive when the off-pitch probes read near-zero (narrow filter / quiet band).
        if (self.run as f32) < spike_min
            || self.mark_peak < self.noise.max(self.off_noise) * self.snr_mult()
        {
            // Not a keyed element — roll its hops into the surrounding gap so it doesn't
            // reset the character timing.
            self.space_run += self.run;
            return;
        }
        self.saw_mark = true;
        let run = self.run as f32;
        // Re-sync on dit/dah pairs the trackers split wrongly. Two adjacent marks 2-4x apart
        // in length are a dit and a dah whatever the trackers say (the pairing rule fldigi's
        // CW speed tracker also relies on), when the space between them is element-sized as
        // keying makes it; noise chattering through one weak blip splits it into fragments
        // a hop or two apart, which would otherwise pass for pairs. From the 20 WPM seed a
        // fast sender's dah falls below the dit/dah midpoint (every element read as a dit:
        // 40 WPM showed as ~15 and copied as E/I), and a slow sender's dit above it. Two such
        // pairs in a row, so one noise fragment cannot re-time a good lock, take the pair as
        // the new dit and dah and re-read the character in progress.
        let (short, long) = (self.last_run.min(run), self.last_run.max(run));
        let gap = self.space_run as f32;
        if short > 0.0
            && (2.0 * short..=4.0 * short).contains(&long)
            && (0.5 * short..=4.0 * short).contains(&gap)
        {
            let mid = (self.dit_avg + self.dah_avg) * 0.5;
            if short < mid && long >= mid {
                self.misfit = 0;
                self.locked = true;
            } else {
                self.misfit += 1;
                if self.misfit >= 2 {
                    self.misfit = 0;
                    self.dit_avg = short;
                    self.dah_avg = long;
                    self.dah_seen = true;
                    self.locked = true;
                    let mid = (short + long) * 0.5;
                    self.sym = self
                        .sym_runs
                        .iter()
                        .map(|&r| if r < mid { '.' } else { '-' })
                        .collect();
                }
            }
        }
        self.last_run = run;
        // Classify by the midpoint between the tracked dit and dah lengths, then nudge the
        // matching average toward this mark -- a standard adaptive-Morse speed tracker. It
        // follows the sender's speed and, unlike a min-cluster estimate, never lets one
        // early mark redefine the unit as itself (which read every leading dah as a dit).
        if run < (self.dit_avg + self.dah_avg) * 0.5 {
            self.sym.push('.');
            self.dit_avg += 0.25 * (run - self.dit_avg);
        } else {
            self.sym.push('-');
            self.dah_avg += 0.25 * (run - self.dah_avg);
            self.dah_seen = true;
        }
        self.sym_runs.push(run);
        // A real element begins a fresh gap.
        self.space_run = 0;
        self.flushed_char = false;
        self.flushed_word = false;
    }

    fn on_gap_tick(&mut self) {
        let g = self.space_run as f32;
        // Inter-character gap (≈3 dit): the symbol is complete → decode + emit it —
        // unless the tracked timing is outside plausible Morse (the storm squelch;
        // scaled by the sensitivity slider). The trackers keep adapting either way,
        // so a real sender re-opens the gate within a few elements.
        if !self.flushed_char && !self.sym.is_empty() && g >= 2.0 * self.dit_avg {
            if self.timing_plausible() {
                if let Some(c) = morse_to_char(&self.sym) {
                    self.push_char(c);
                }
            }
            self.sym.clear();
            self.sym_runs.clear();
            self.flushed_char = true;
        }
        // Word gap (≈7 dit): one space between words.
        if !self.flushed_word && self.flushed_char && g >= 5.0 * self.dit_avg {
            self.push_char(' ');
            self.flushed_word = true;
        }
        // A silence well past a word gap usually means the next signal is another station. Let
        // the spike floor drop back to the cold-start one so a faster sender's dits are not
        // rejected against the last station's speed; the next dit/dah pair locks again.
        if g >= 10.0 * self.dit_avg {
            self.locked = false;
        }
    }

    fn push_char(&mut self, c: char) {
        self.transcript.push(c);
        if self.transcript.len() > CW_TRANSCRIPT_CAP {
            let drop = self.transcript.len() - CW_TRANSCRIPT_CAP;
            self.transcript.drain(0..drop);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cw::morse_samples;

    const SR: f32 = 48_000.0;
    const PITCH: f32 = 600.0;

    fn decode(text: &str, wpm: u32) -> CwDecode {
        // Pad with leading + trailing silence (a real capture is never perfectly trimmed).
        let mut audio = vec![0.0f32; (SR * 0.1) as usize];
        audio.extend(morse_samples(text, wpm, PITCH, SR as u32));
        audio.extend(vec![0.0f32; (SR * 0.1) as usize]);
        decode_cw(&audio, SR, PITCH)
    }

    #[test]
    fn decodes_a_clean_callsign_and_estimates_wpm() {
        let d = decode("CQ TEST DE W1ABC", 20);
        assert_eq!(d.text, "CQ TEST DE W1ABC");
        assert!((d.wpm as i32 - 20).abs() <= 2, "≈20 wpm, got {}", d.wpm);
    }

    #[test]
    fn decodes_across_speeds() {
        assert_eq!(decode("PARIS", 15).text, "PARIS");
        assert_eq!(decode("599 TU", 25).text, "599 TU");
        assert_eq!(decode("K", 30).text, "K");
    }

    #[test]
    fn empty_on_silence_and_steady_tone() {
        assert_eq!(
            decode_cw(&vec![0.0f32; 48_000], SR, PITCH),
            CwDecode::default()
        );
        // A steady (un-keyed) carrier — no on/off ratio → nothing to decode.
        let steady: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * std::f32::consts::PI * PITCH * i as f32 / SR).sin())
            .collect();
        assert_eq!(decode_cw(&steady, SR, PITCH).text, "");
    }

    #[test]
    fn skimmer_finds_a_signal_at_its_pitch() {
        let mut audio = vec![0.0f32; (SR * 0.1) as usize];
        audio.extend(morse_samples("CQ TEST", 22, 600.0, SR as u32));
        let hits = skim_cw(&audio, SR, 300, 1200, 50);
        assert!(!hits.is_empty(), "found the signal");
        assert!(
            hits.iter().any(|h| h.text == "CQ TEST"),
            "decoded the text: {hits:?}"
        );
        // Decoding channels cluster near the 600 Hz tone — no spurious far-off hits.
        assert!(
            hits.iter().all(|h| (h.pitch_hz as i32 - 600).abs() <= 300),
            "clustered near 600 Hz: {hits:?}"
        );
    }

    #[test]
    fn skimmer_empty_on_silence() {
        assert!(skim_cw(&vec![0.0f32; 48_000], SR, 300, 1200, 50).is_empty());
    }

    #[test]
    fn morse_to_char_reverses_the_table() {
        assert_eq!(morse_to_char("."), Some('E'));
        assert_eq!(morse_to_char("-.-."), Some('C'));
        assert_eq!(morse_to_char("....."), Some('5'));
        assert_eq!(morse_to_char(".-.-"), None); // not a glyph
    }

    /// Morse audio with lead + `trail_secs` trailing silence (the trailing gap must be
    /// ≥ ~3 dit for the streaming decoder to flush the final character).
    fn morse_audio(text: &str, wpm: u32, trail_secs: f32) -> Vec<f32> {
        let mut a = vec![0.0f32; (SR * 0.1) as usize];
        a.extend(morse_samples(text, wpm, PITCH, SR as u32));
        a.extend(vec![0.0f32; (SR * trail_secs) as usize]);
        a
    }

    #[test]
    fn stream_decodes_a_callsign_and_estimates_wpm() {
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.push(&morse_audio("CQ TEST DE W1ABC", 20, 0.6));
        assert_eq!(d.transcript().trim(), "CQ TEST DE W1ABC");
        assert!((d.wpm() as i32 - 20).abs() <= 3, "≈20 wpm, got {}", d.wpm());
    }

    #[test]
    fn stream_accumulates_across_small_chunks() {
        // Fed in tiny slices (as real audio arrives) the transcript must match a single
        // push — proving the incremental state machine carries across chunk boundaries.
        let audio = morse_audio("PARIS DE K1ABC", 25, 0.6);
        let mut d = CwStreamDecoder::new(SR, PITCH);
        for chunk in audio.chunks(1000) {
            d.push(chunk);
        }
        assert_eq!(d.transcript().trim(), "PARIS DE K1ABC");
    }

    #[test]
    fn stream_keeps_earlier_text_when_more_arrives() {
        // The whole point of the rewrite: earlier text does NOT vanish when new audio
        // scrolls the window — the transcript grows.
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.push(&morse_audio("K", 20, 0.6));
        assert_eq!(d.transcript().trim(), "K");
        d.push(&morse_audio("W1ABC", 20, 0.6));
        assert!(
            d.transcript().contains('K'),
            "kept the earlier K: {:?}",
            d.transcript()
        );
        assert!(
            d.transcript().contains("W1ABC"),
            "added the new call: {:?}",
            d.transcript()
        );
    }

    #[test]
    fn stream_stays_silent_on_noise_and_unkeyed_carrier() {
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.push(&vec![0.0f32; SR as usize]); // 1 s silence
        assert_eq!(d.transcript(), "");
        // A steady, un-keyed carrier has no on/off ratio → nothing to decode.
        let mut d2 = CwStreamDecoder::new(SR, PITCH);
        let steady: Vec<f32> = (0..SR as usize)
            .map(|i| (2.0 * std::f32::consts::PI * PITCH * i as f32 / SR).sin())
            .collect();
        d2.push(&steady);
        assert_eq!(d2.transcript(), "");
    }

    /// Static-crash storm: the tone randomly keyed with sub-dit-to-dit-scale
    /// bursts — the classic "ton of false characters" band condition.
    fn crash_storm(secs: f32) -> Vec<f32> {
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / 16_777_216.0
        };
        let n = (SR * secs) as usize;
        let mut audio = vec![0.0f32; n];
        let mut i = 0usize;
        while i < n {
            let on = (SR * (0.004 + 0.05 * rnd())) as usize; // 4–54 ms burst
            let off = (SR * (0.01 + 0.08 * rnd())) as usize; // 10–90 ms gap
            let amp = 0.3 + 0.7 * rnd();
            for j in 0..on.min(n - i) {
                audio[i + j] =
                    amp * (2.0 * std::f32::consts::PI * PITCH * (i + j) as f32 / SR).sin();
            }
            i += on + off;
        }
        audio
    }

    fn storm_glyphs(sens: f32) -> usize {
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.set_sensitivity(sens);
        d.push(&crash_storm(12.0));
        d.transcript()
            .chars()
            .filter(|c| !c.is_whitespace())
            .count()
    }

    #[test]
    fn sensitivity_slider_visibly_gates_a_noise_storm() {
        // The operator-reported bug: sliding the CW sensitivity did NOTHING to a
        // false-character storm (all gates saturated against the min-following
        // noise floor). The slider now drives the Schmitt rails, the spike gate,
        // and the timing-plausibility squelch — each end must be VISIBLY
        // different on the same storm.
        let strict = storm_glyphs(0.0);
        let default = storm_glyphs(0.5);
        let loose = storm_glyphs(1.0);
        assert!(
            strict * 2 < default,
            "strict must at least halve the storm: strict={strict} default={default}"
        );
        assert!(
            default < loose,
            "loose must admit more than default: default={default} loose={loose}"
        );
    }

    #[test]
    fn strict_sensitivity_still_copies_clean_cw() {
        // Turning the slider all the way down must never break solid copy.
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.set_sensitivity(0.0);
        d.push(&morse_audio("CQ DX DE W1ABC", 22, 0.6));
        assert_eq!(d.transcript().trim(), "CQ DX DE W1ABC");
    }

    /// Deterministic broadband white noise (energy at every frequency, incl. the off-pitch probes)
    /// — unlike `crash_storm`, which is on-pitch bursts the off-pitch SNR gate can't see.
    fn white_noise(n: usize, amp: f32) -> Vec<f32> {
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            ((seed >> 33) as f32 / (1u32 << 31) as f32) - 1.0 // ~[-1, 1)
        };
        (0..n).map(|_| amp * rnd()).collect()
    }

    #[test]
    fn off_pitch_snr_lets_the_slider_trade_copy_in_broadband_noise() {
        // A WEAK tone buried in BROADBAND noise — the real-world case the off-pitch reference is
        // for (the on-pitch floor sat near silence, so the SNR gates saturated and the slider did
        // nothing here). With a true band-noise reference the looser end admits at least as much
        // copy as the stricter end. `loose >= strict` is structurally guaranteed by the more-
        // permissive gates, so it's a stable regression guard that this path actually engages.
        let copy = |sens: f32| -> String {
            let mut d = CwStreamDecoder::new(SR, PITCH);
            d.set_sensitivity(sens);
            let mut sig = morse_samples("PARIS PARIS PARIS PARIS", 18, PITCH, SR as u32);
            for s in sig.iter_mut() {
                *s *= 0.30; // weak
            }
            let noise = white_noise(sig.len(), 0.15);
            let mixed: Vec<f32> = sig
                .iter()
                .zip(&noise)
                .map(|(s, n)| (s + n).clamp(-1.0, 1.0))
                .collect();
            d.push(&mixed);
            d.transcript().to_string()
        };
        let strict = copy(0.0);
        let loose = copy(1.0);
        let n = |t: &str| t.chars().filter(|c| !c.is_whitespace()).count();
        // The loose end must produce REAL copy through the broadband noise — the off-pitch
        // reference must not have inflated and killed the signal. (We assert copy exists and
        // is monotonic, not a specific strict<loose margin: whether the slider actually TRADES
        // more copy depends on where the noise puts marks vs. the gate, which is signal-specific.)
        assert!(loose.contains("PARIS"), "loose lost real copy: {loose:?}");
        assert!(n(&loose) > 0, "loose decoded nothing");
        assert!(
            n(&loose) >= n(&strict),
            "loose must admit at least as much weak copy as strict: strict={strict:?} loose={loose:?}"
        );
    }

    #[test]
    fn stream_suppresses_near_noise_marks() {
        // Weak, regularly-keyed on-pitch blips buried in BROADBAND noise so each blip clears the
        // true (off-pitch) floor by only ~3× in power — below the ~4× default SNR gate. Each blip
        // is a 1-dit "on" with a 3-dit gap, so ANY blip the decoder wrongly accepts FLUSHES as an
        // 'E'; a broken gate would decode a "EEEE…" storm. With the off-pitch SNR gate the blips
        // are rejected, so the transcript stays essentially empty. (The off_pitch_snr test is the
        // complement: it proves genuine copy DOES survive when the signal clears the floor.)
        let dit = (SR * 0.06) as usize; // ~20 wpm element
        let mut sig: Vec<f32> = Vec::new();
        for _ in 0..40 {
            for k in 0..dit {
                let n = sig.len() + k;
                sig.push(0.10 * (2.0 * std::f32::consts::PI * PITCH * n as f32 / SR).sin());
            }
            sig.extend(std::iter::repeat_n(0.0, 3 * dit)); // flushing gap
        }
        let noise = white_noise(sig.len(), 0.14);
        let mixed: Vec<f32> = sig
            .iter()
            .zip(&noise)
            .map(|(s, n)| (s + n).clamp(-1.0, 1.0))
            .collect();
        let mut d = CwStreamDecoder::new(SR, PITCH); // default sensitivity
        d.push(&mixed);
        let junk = d
            .transcript()
            .chars()
            .filter(|c| !c.is_whitespace())
            .count();
        assert!(
            junk <= 8,
            "near-noise produced a storm ({junk} chars): {:?}",
            d.transcript()
        );
    }

    #[test]
    fn stream_clear_and_retune_reset_text() {
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.push(&morse_audio("K", 20, 0.6));
        assert!(!d.transcript().is_empty());
        d.clear();
        assert_eq!(d.transcript(), "");
        d.push(&morse_audio("E", 20, 0.6)); // "E" = "."
        assert_eq!(d.transcript().trim(), "E");
        d.retune(PITCH + 200.0); // new pitch → full reset
        assert_eq!(d.transcript(), "");
    }

    // ---- Cold-start speed lock: synthetic senders from 15 to 50 WPM ----

    /// Contest-style copy every speed test keys. Mixed dit/dah characters lead, as real CQs do.
    const SPEED_TEXT: &str = "CQ TEST K1ABC K1ABC TEST";

    /// Tiny deterministic xorshift in [-1, 1) for jitter and noise.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            ((self.0 >> 33) as f32 / (1u32 << 31) as f32) - 1.0
        }
    }

    /// Keyed CW at `wpm` where every element and gap wanders by up to ±`jitter` of its length
    /// (a hand or bug sender) and every mark is stretched by `weight` dits at the expense of the
    /// gap after it (a weighted keyer). Returns the audio and, per character, the sample index
    /// where its inter-character gap has just elapsed (so a decoder fed that far has seen it).
    fn keyed(text: &str, wpm: u32, jitter: f32, weight: f32, seed: u64) -> (Vec<f32>, Vec<usize>) {
        let dit = 1.2 / wpm as f32 * SR;
        let ramp = ((0.005 * SR) as usize).min(dit as usize / 2 + 1);
        let step = 2.0 * std::f32::consts::PI * PITCH / SR;
        let mut rng = Rng(seed | 1);
        let len = |units: f32, rng: &mut Rng| {
            ((units * dit) * (1.0 + jitter * rng.next())).max(1.0) as usize
        };
        let mut out = vec![0.0f32; (SR * 0.1) as usize];
        let mut ends = Vec::new();
        let mut phase = 0.0f32;
        for (wi, word) in text.split_whitespace().enumerate() {
            if wi > 0 {
                let n = len(4.0, &mut rng); // a word gap is 7 dits; 3 already followed the char
                out.extend(std::iter::repeat_n(0.0, n));
            }
            for ch in word.chars() {
                let code = crate::cw::morse_code(ch).expect("keyable");
                for (ei, el) in code.chars().enumerate() {
                    if ei > 0 {
                        let n = len(1.0 - weight, &mut rng);
                        out.extend(std::iter::repeat_n(0.0, n));
                    }
                    let units = if el == '-' { 3.0 } else { 1.0 };
                    let n = len(units + weight, &mut rng);
                    for i in 0..n {
                        let env = if i < ramp {
                            0.5 - 0.5 * (std::f32::consts::PI * i as f32 / ramp as f32).cos()
                        } else if i + ramp >= n {
                            0.5 - 0.5
                                * (std::f32::consts::PI * (n - 1 - i) as f32 / ramp as f32).cos()
                        } else {
                            1.0
                        };
                        out.push(env * phase.sin());
                        phase += step;
                    }
                }
                let n = len(3.0 - weight, &mut rng);
                out.extend(std::iter::repeat_n(0.0, n));
                ends.push(out.len());
            }
        }
        out.extend(vec![0.0f32; (SR * 0.6) as usize]);
        (out, ends)
    }

    /// Add broadband white noise of peak `amp` (the signal peaks at 1.0).
    fn with_noise(mut a: Vec<f32>, amp: f32, seed: u64) -> Vec<f32> {
        let mut rng = Rng(seed | 1);
        for s in a.iter_mut() {
            *s = (*s + amp * rng.next()).clamp(-1.0, 1.0);
        }
        a
    }

    /// Edit distance between two strings, by character.
    fn edits(a: &str, b: &str) -> usize {
        let b: Vec<char> = b.chars().collect();
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.chars().enumerate() {
            let mut prev = row[0];
            row[0] = i + 1;
            for j in 0..b.len() {
                let cur = row[j + 1];
                row[j + 1] = (prev + usize::from(ca != b[j]))
                    .min(row[j] + 1)
                    .min(cur + 1);
                prev = cur;
            }
        }
        row[b.len()]
    }

    /// Character accuracy of a copy against `SPEED_TEXT`: 1 − edits / length (whitespace
    /// collapsed to single spaces), floored at 0.
    fn accuracy(copy: &str) -> f32 {
        let copy = copy.split_whitespace().collect::<Vec<_>>().join(" ");
        let e = edits(&copy, SPEED_TEXT) as f32;
        (1.0 - e / SPEED_TEXT.len() as f32).max(0.0)
    }

    /// The four conditions every speed is keyed under: (name, jitter, noise peak).
    const CONDITIONS: [(&str, f32, f32); 4] = [
        ("clean", 0.0, 0.0),
        ("noise", 0.0, 0.5),
        ("jitter", 0.10, 0.0),
        ("noise+jitter", 0.10, 0.5),
    ];

    /// One cold-start run: the WPM estimate after `n_chars` characters, and the whole copy's accuracy.
    fn cold_run(
        wpm: u32,
        jitter: f32,
        noise: f32,
        weight: f32,
        n_chars: usize,
        seed: u64,
    ) -> (u32, f32, String) {
        let (audio, ends) = keyed(SPEED_TEXT, wpm, jitter, weight, seed);
        let audio = if noise > 0.0 {
            with_noise(audio, noise, seed ^ 0xA5A5)
        } else {
            audio
        };
        let mut d = CwStreamDecoder::new(SR, PITCH);
        let cut = ends[n_chars - 1];
        d.push(&audio[..cut]);
        let est = d.wpm();
        d.push(&audio[cut..]);
        (
            est,
            accuracy(d.transcript()),
            d.transcript().trim().to_string(),
        )
    }

    /// Characters a cold decoder hears before its speed must be within ±10%.
    const LOCK_CHARS: usize = 6;

    #[test]
    fn cold_start_locks_onto_15_to_50_wpm_within_six_characters() {
        // From a cold start the 20 WPM seed read every dah of a 34+ WPM sender as a dit:
        // 40 WPM measured ~15 and copied as E/I. Three senders per speed and condition.
        let mut off = Vec::new();
        for wpm in (15u32..=50).step_by(5) {
            for (name, jitter, noise) in CONDITIONS {
                for seed in 1..=3u64 {
                    let (est, _, _) = cold_run(wpm, jitter, noise, 0.0, LOCK_CHARS, seed * 7919);
                    if (est as f32 - wpm as f32).abs() > 0.10 * wpm as f32 {
                        off.push(format!("{wpm} {name} seed {seed}: {est}"));
                    }
                }
            }
        }
        assert!(
            off.is_empty(),
            "estimate off by more than 10% after {LOCK_CHARS} characters: {off:?}"
        );
    }

    #[test]
    fn cold_start_copies_fast_cw() {
        let mut bad = Vec::new();
        for wpm in [35u32, 40, 45, 50] {
            for (name, jitter, noise) in CONDITIONS {
                for seed in 1..=3u64 {
                    let (_, acc, copy) = cold_run(wpm, jitter, noise, 0.0, 1, seed * 7919);
                    if acc < 0.9 {
                        bad.push(format!("{wpm} {name} seed {seed}: {acc:.2} {copy:?}"));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "fast copy below 90%: {bad:?}");
    }

    /// Worst character accuracy over the three senders, measured on the decoder before the
    /// cold-start re-sync (the same harness), per (WPM, condition). The copy must not get worse.
    const BASELINE_ACCURACY: [(u32, &str, f32); 30] = [
        (15, "clean", 1.00),
        (15, "noise", 0.96),
        (15, "jitter", 1.00),
        (15, "noise+jitter", 0.96),
        (20, "clean", 1.00),
        (20, "noise", 0.96),
        (20, "jitter", 1.00),
        (20, "noise+jitter", 0.96),
        (25, "clean", 1.00),
        (25, "noise", 0.96),
        (25, "jitter", 1.00),
        (25, "noise+jitter", 0.96),
        (30, "clean", 0.17),
        (30, "noise", 0.96),
        (30, "jitter", 0.12),
        (30, "noise+jitter", 0.75),
        // Slow, weighted (marks +0.3 dit) and hand-sent (±20% timing) senders.
        (8, "clean", 0.92),
        (10, "clean", 0.92),
        (12, "clean", 1.00),
        (8, "weighted", 0.96),
        (12, "weighted", 1.00),
        (20, "weighted", 1.00),
        (30, "weighted", 0.88),
        (8, "hand", 0.75),
        (10, "hand", 0.92),
        (12, "hand", 1.00),
        (15, "hand", 1.00),
        (20, "hand", 1.00),
        (25, "hand", 0.88),
        (30, "hand", 0.12),
    ];

    #[test]
    fn cold_start_copy_is_no_worse_than_before() {
        let mut worse = Vec::new();
        for (wpm, name, before) in BASELINE_ACCURACY {
            let (jitter, noise, weight) = match name {
                "weighted" => (0.05, 0.0, 0.3),
                "hand" => (0.20, 0.0, 0.0),
                _ => CONDITIONS
                    .iter()
                    .find(|c| c.0 == name)
                    .map(|c| (c.1, c.2, 0.0))
                    .unwrap(),
            };
            let now = (1..=3u64)
                .map(|seed| cold_run(wpm, jitter, noise, weight, 1, seed * 7919).1)
                .fold(1.0, f32::min);
            if now + 0.005 < before {
                worse.push(format!("{wpm} {name}: {before:.2} -> {now:.2}"));
            }
        }
        assert!(worse.is_empty(), "copy got worse: {worse:?}");
    }

    #[test]
    fn a_weighted_sender_reads_at_its_speed() {
        // Weighting stretches every mark at the expense of the space after it. The speed is
        // half the dah-dit difference, which weighting does not change; the dit alone read a
        // +0.3 weighted 20 WPM sender as 17.
        for wpm in [10u32, 20, 30, 40, 50] {
            let (est, _, _) = cold_run(wpm, 0.0, 0.0, 0.3, LOCK_CHARS, 7919);
            assert!(
                (est as f32 - wpm as f32).abs() <= 0.10 * wpm as f32,
                "weighted {wpm} WPM read as {est}"
            );
        }
    }

    #[test]
    fn one_noise_fragment_does_not_retime_a_locked_speed() {
        // Locked on 20 WPM, then a long dit and a fragment a third its length, an element
        // apart: one pair the trackers split wrongly. A single such pair must not re-time
        // the decoder (two in a row do, which is how a fast sender re-syncs).
        let (mut audio, _) = keyed("CQ TEST", 20, 0.0, 0.0, 1);
        audio.truncate(audio.len() - (SR * 0.5) as usize);
        let dit = (SR * 0.06) as usize;
        let tone =
            |n: usize| (0..n).map(|k| (2.0 * std::f32::consts::PI * PITCH * k as f32 / SR).sin());
        audio.extend(tone(dit * 18 / 10));
        audio.extend(std::iter::repeat_n(0.0, dit));
        audio.extend(tone(dit * 6 / 10));
        audio.extend(std::iter::repeat_n(0.0, dit * 3));
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.push(&audio);
        assert!(
            (18..=22).contains(&d.wpm()),
            "one fragment re-timed 20 WPM to {}",
            d.wpm()
        );
    }

    #[test]
    fn a_fast_station_after_a_slow_one_and_a_pause_is_copied() {
        // The decoder is rarely cold: it was locked on the last station heard. Locked at 15 or
        // 20 WPM, the spike floor rejected every dit of a 50 WPM station heard after a pause
        // (copy "5E EIIE E", 21 WPM).
        for slow in [15u32, 20] {
            let (first, _) = keyed(SPEED_TEXT, slow, 0.0, 0.0, 3);
            let mut d = CwStreamDecoder::new(SR, PITCH);
            d.push(&first);
            d.push(&vec![0.0f32; (SR * 2.0) as usize]);
            d.clear();
            let (audio, ends) = keyed(SPEED_TEXT, 50, 0.10, 0.0, 5);
            let audio = with_noise(audio, 0.5, 77);
            d.push(&audio[..ends[LOCK_CHARS - 1]]);
            let est = d.wpm();
            d.push(&audio[ends[LOCK_CHARS - 1]..]);
            assert!(
                (45..=55).contains(&est),
                "50 WPM after {slow} WPM read as {est}"
            );
            assert!(
                accuracy(d.transcript()) >= 0.9,
                "50 WPM after {slow} WPM copied {:?}",
                d.transcript()
            );
        }
    }

    /// Keyed `text` with the lead-in and tail silence cut, so pieces join with element-sized gaps.
    fn keyed_tight(text: &str, wpm: u32) -> Vec<f32> {
        let (a, _) = keyed(text, wpm, 0.0, 0.0, 1);
        a[(SR * 0.1) as usize..a.len() - (SR * 0.6) as usize].to_vec()
    }

    fn tone(n: usize) -> impl Iterator<Item = f32> {
        (0..n).map(|k| (2.0 * std::f32::consts::PI * PITCH * k as f32 / SR).sin())
    }

    #[test]
    fn a_cold_decoder_copies_the_first_word_of_a_fast_sender() {
        // The re-sync re-reads the character it lands in, so the CQ is not lost to the seed.
        for wpm in [35u32, 40, 45, 50] {
            let (audio, _) = keyed(SPEED_TEXT, wpm, 0.0, 0.0, 7919);
            let mut d = CwStreamDecoder::new(SR, PITCH);
            d.push(&audio);
            assert!(
                d.transcript().starts_with("CQ TEST"),
                "{wpm} WPM copied {:?}",
                d.transcript()
            );
        }
    }

    #[test]
    fn short_blips_inside_a_locked_senders_copy_stay_rejected() {
        // Once a sender's dit/dah pairs confirm 20 WPM, the spike floor scales from its dit
        // again: a 16 ms blip (a 75 WPM dit) in the word gaps of its copy is noise, not an E.
        let dit = (SR * 0.06) as usize;
        let mut audio = vec![0.0f32; (SR * 0.1) as usize];
        audio.extend(keyed_tight("CQ", 20));
        for word in ["TEST", "TEST"] {
            audio.extend(tone((SR * 0.016) as usize));
            audio.extend(std::iter::repeat_n(0.0, dit * 4));
            audio.extend(keyed_tight(word, 20));
        }
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.push(&audio);
        assert_eq!(d.transcript().trim(), "CQ TEST TEST");
    }

    #[test]
    fn two_noise_fragments_apart_do_not_retime_a_locked_speed() {
        // A correctly split pair between them clears the count: only two misread pairs in a
        // row re-time the decoder, not two in a whole QSO.
        let dit = (SR * 0.06) as usize;
        let fragment = |a: &mut Vec<f32>| {
            a.extend(std::iter::repeat_n(0.0, dit * 3));
            a.extend(tone(dit * 18 / 10));
            a.extend(std::iter::repeat_n(0.0, dit));
            a.extend(tone(dit * 6 / 10));
            a.extend(std::iter::repeat_n(0.0, dit * 3));
        };
        let mut audio = vec![0.0f32; (SR * 0.1) as usize];
        audio.extend(keyed_tight("CQ TEST", 20));
        fragment(&mut audio);
        audio.extend(keyed_tight("TEST", 20));
        fragment(&mut audio);
        let mut d = CwStreamDecoder::new(SR, PITCH);
        d.push(&audio);
        assert!(
            (18..=22).contains(&d.wpm()),
            "two fragments re-timed 20 WPM to {}",
            d.wpm()
        );
    }

    #[test]
    #[ignore = "measurement table; run with --ignored --nocapture"]
    fn print_cold_start_table() {
        let mut rows: Vec<(&str, f32, f32, f32)> =
            CONDITIONS.iter().map(|&(n, j, z)| (n, j, z, 0.0)).collect();
        rows.push(("weighted", 0.05, 0.0, 0.3));
        rows.push(("hand", 0.20, 0.0, 0.0));
        rows.push(("hand+noise", 0.20, 0.5, 0.0));
        for wpm in [8u32, 10, 12, 15, 20, 25, 30, 35, 40, 45, 50] {
            for &(name, j, nz, w) in &rows {
                let mut line = format!("{wpm:>3} {name:<13}");
                let mut amin = 1.0f32;
                for n in [4usize, 6, 8] {
                    let ests: Vec<u32> = (1..=3u64)
                        .map(|s| cold_run(wpm, j, nz, w, n, s * 7919).0)
                        .collect();
                    line += &format!(" N{n}={ests:?}");
                }
                for s in 1..=3u64 {
                    amin = amin.min(cold_run(wpm, j, nz, w, 4, s * 7919).1);
                }
                println!("{line} acc_min={amin:.2}");
            }
        }
    }
}
