//! RTTY tuning scope — the crossed-ellipse XY display of a classic terminal unit.
//!
//! Two narrow bandpass resonators sit on the mark and space tones the decoder is listening to
//! (the AFC-corrected pair). The mark resonator's output is X and the space resonator's is Y, so a
//! tone on mark draws a long, thin ellipse along X and a tone on space one along Y: a tuned RTTY
//! signal, alternating between the two, draws a "+".
//!
//! WHY RESONATORS AND NOT THE DECODER'S OWN FILTERS. A resonator's phase swings through ±90°
//! across its passband, so a tone that is off a resonator's centre comes out of the two filters
//! with a phase difference that depends on WHICH SIDE it is off. That phase difference is the
//! tilt of the ellipse. The decoder's matched filters are linear-phase: through them a mistuned
//! tone draws a straight line whose slope says how far off it is but not which way. With these,
//! the cross rotates one way when the signal sits high and the other way when it sits low.
//!
//! Each arm also leans a little even when tuned (the other resonator still passes a few percent of
//! the tone, at a phase lead or lag), and the two arms lean in opposite senses, so a tuned "+" is
//! symmetric. Mistuning breaks the symmetry: one arm leans further and the other straightens, and
//! the cross as a whole turns. [`TuningScope::tuning`] reads the same phase difference back as a
//! number of hertz, for the words beside the picture.
//!
//! Textbook DSP only: the two filters are the standard second-order constant-peak bandpass (the
//! audio-EQ cookbook form), and the estimate inverts their exact frequency response.

use std::collections::VecDeque;
use std::f64::consts::PI;

/// Resonator bandwidth (Hz, -3 dB). Wide enough that a tone on one resonator still excites the
/// other one (at 170 Hz shift, about a quarter of the amplitude), which is what puts the phase
/// difference, and so the tilt, into the picture; narrow enough that the two arms stay apart and
/// a neighbouring signal 100 Hz away barely shows.
pub const SCOPE_BW_HZ: f32 = 100.0;

/// Keep one XY point in this many input samples: 12 kHz becomes 3 kS/s. Every kept point still
/// lies on the ellipse (each is an exact sample of the two filter outputs), so the trace is drawn
/// as dots, which fill the ellipse's outline evenly.
pub const DECIMATE: usize = 4;

/// The XY ring's length: the newest 200 ms of points at 3 kS/s.
pub const RING: usize = 600;

/// A tone move smaller than this (Hz) keeps the filters as they are. The AFC walks in fractions
/// of a hertz; rebuilding on every step would clear the picture several times a second.
pub const RETUNE_HZ: f32 = 1.0;

/// Within this many hertz the signal reads as tuned.
pub const TUNED_HZ: f32 = 5.0;

/// The tuning statistics' memory (seconds): long enough to average over several characters,
/// short enough that a nudge shows within a second.
const STATS_TAU_S: f64 = 0.5;

/// The arm-picking envelope's time constant (seconds): a few cycles of the tone, much shorter
/// than a 45.45 Bd bit (22 ms).
const ENV_TAU_S: f64 = 0.002;

/// How long (seconds) one arm must have held before its points count toward the tuning estimate.
/// At a mark/space change the resonator that just lost its tone keeps ringing at the OLD frequency
/// (its amplitude falls by e every 3.2 ms at 100 Hz bandwidth), and that ring is far larger than
/// the steady cross-talk the estimate reads, so the first few milliseconds of every bit would pull
/// the read-out toward zero. 10 ms leaves it under a tenth of the cross-talk and still keeps half
/// of a 45.45 Bd bit.
const HOLD_S: f64 = 0.010;

/// And how long (seconds) it must go on holding AFTER a point for that point to count. The
/// envelopes notice a change of tone a few milliseconds late, so the last points credited to an arm
/// before the change already belong to the next bit: the new tone rising in one resonator while the
/// old one rings in the other. Judging each point this much later keeps them out.
const LOOKAHEAD_S: f64 = 0.004;

/// An arm counts as present when its own resonator carries at least this many times the other
/// one's power. A tone within about ±50 Hz of a 170 Hz pair clears it; band noise, which excites
/// both resonators alike, does not.
const DOMINANCE: f64 = 4.0;

/// An arm needs at least this share of the total energy to be read, so a stray stretch of noise
/// that happens to favour one resonator cannot stand in for an arm that is not there.
const MIN_ARM_SHARE: f64 = 0.1;

/// One second-order bandpass resonator, constant 0 dB peak gain (b1 = 0, b2 = -b0).
#[derive(Clone, Copy, Debug)]
struct Resonator {
    b0: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Resonator {
    fn new(sample_rate: f64, f0: f64, bw: f64) -> Self {
        let w0 = 2.0 * PI * f0 / sample_rate;
        let q = f0 / bw;
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn step(&mut self, x: f64) -> f64 {
        let y = self.b0 * (x - self.x2) - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    /// The phase (radians) of the exact response at `f` Hz:
    /// H(z) = b0 (1 - z⁻²) / (1 + a1 z⁻¹ + a2 z⁻²) at z = e^{jw}.
    fn phase(&self, sample_rate: f64, f: f64) -> f64 {
        let w = 2.0 * PI * f / sample_rate;
        let (num_re, num_im) = (self.b0 * (1.0 - (2.0 * w).cos()), self.b0 * (2.0 * w).sin());
        let den_re = 1.0 + self.a1 * w.cos() + self.a2 * (2.0 * w).cos();
        let den_im = -self.a1 * w.sin() - self.a2 * (2.0 * w).sin();
        num_im.atan2(num_re) - den_im.atan2(den_re)
    }
}

/// Exponentially weighted second moments of the points belonging to one arm.
#[derive(Clone, Copy, Debug, Default)]
struct Arm {
    sxx: f64,
    syy: f64,
    sxy: f64,
}

impl Arm {
    fn decay(&mut self, k: f64) {
        self.sxx *= k;
        self.syy *= k;
        self.sxy *= k;
    }

    fn energy(&self) -> f64 {
        self.sxx + self.syy
    }

    /// The measured phase difference between the two resonators for this arm's tone, in
    /// [0, π]: for a steady tone x = A cos(wt), y = B cos(wt + θ), Sxy/√(Sxx·Syy) = cos θ.
    fn theta(&self) -> f64 {
        let d = (self.sxx * self.syy).sqrt();
        if d <= 0.0 {
            return PI / 2.0;
        }
        (self.sxy / d).clamp(-1.0, 1.0).acos()
    }
}

/// What the scope reads off the signal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tuning {
    /// Neither arm stands out of the noise.
    NoSignal,
    /// How far the signal sits from the tones the decoder is listening to (Hz): positive means
    /// the signal is HIGHER in the audio passband than the decoder's tones.
    Offset(f32),
}

/// One poll's worth of the picture: the newest points, quantised for the wire.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScopeFrame {
    /// Counts every point the scope has ever kept, so a reader can tell a new frame from one it
    /// has drawn already.
    pub seq: u64,
    /// The largest |x| or |y| in this frame, in the input's own units: the drawing's auto-scale
    /// follows it.
    pub peak: f32,
    /// Interleaved x, y pairs, each scaled so `peak` is ±127.
    pub xy: Vec<i8>,
    /// The tuning read-out, `None` when there is no signal.
    pub offset_hz: Option<f32>,
}

/// The crossed-ellipse tuning scope: two resonators on the decoder's mark and space tones, a
/// bounded ring of decimated XY points, and the tuning estimate.
#[derive(Clone, Debug)]
pub struct TuningScope {
    sample_rate: f64,
    mark_hz: f32,
    space_hz: f32,
    mark: Resonator,
    space: Resonator,
    env_x: f64,
    env_y: f64,
    env_k: f64,
    stats_k: f64,
    hold: usize,
    /// The newest points, waiting [`LOOKAHEAD_S`] to be judged.
    pending: VecDeque<(f64, f64)>,
    lookahead: usize,
    arms: [Arm; 2],
    /// Which arm the envelopes favour now, and for how many samples it has.
    current: usize,
    run: usize,
    phase: usize,
    ring: VecDeque<(f32, f32)>,
    kept: u64,
}

impl TuningScope {
    /// A scope at `sample_rate` Hz whose resonators sit on `mark_hz` (X) and `space_hz` (Y).
    pub fn new(sample_rate: f32, mark_hz: f32, space_hz: f32) -> Self {
        let sr = sample_rate as f64;
        let bw = SCOPE_BW_HZ as f64;
        Self {
            sample_rate: sr,
            mark_hz,
            space_hz,
            mark: Resonator::new(sr, mark_hz as f64, bw),
            space: Resonator::new(sr, space_hz as f64, bw),
            env_x: 0.0,
            env_y: 0.0,
            env_k: 1.0 - (-1.0 / (ENV_TAU_S * sr)).exp(),
            stats_k: (-1.0 / (STATS_TAU_S * sr)).exp(),
            hold: (HOLD_S * sr) as usize,
            pending: VecDeque::new(),
            lookahead: (LOOKAHEAD_S * sr) as usize,
            arms: [Arm::default(); 2],
            current: 0,
            run: 0,
            phase: 0,
            ring: VecDeque::with_capacity(RING),
            kept: 0,
        }
    }

    /// The (mark, space) tones the resonators sit on.
    pub fn tones(&self) -> (f32, f32) {
        (self.mark_hz, self.space_hz)
    }

    /// Follow the decoder's tones. A move under [`RETUNE_HZ`] on both is ignored; a larger one
    /// rebuilds the resonators and clears the picture and the statistics, which described the old
    /// tones. Returns whether it rebuilt.
    pub fn retune(&mut self, mark_hz: f32, space_hz: f32) -> bool {
        if (mark_hz - self.mark_hz).abs() < RETUNE_HZ
            && (space_hz - self.space_hz).abs() < RETUNE_HZ
        {
            return false;
        }
        let kept = self.kept;
        *self = Self::new(self.sample_rate as f32, mark_hz, space_hz);
        self.kept = kept;
        true
    }

    /// Drop the picture and the statistics (our own transmission, or a fresh start), keeping the
    /// tones. The point count carries on so a reader sees the change.
    pub fn clear(&mut self) {
        let (kept, mark, space) = (self.kept, self.mark_hz, self.space_hz);
        *self = Self::new(self.sample_rate as f32, mark, space);
        self.kept = kept;
    }

    /// Run audio through both resonators.
    pub fn feed(&mut self, samples: &[f32]) {
        for &s in samples {
            let x = self.mark.step(s as f64);
            let y = self.space.step(s as f64);
            self.env_x += self.env_k * (x * x - self.env_x);
            self.env_y += self.env_k * (y * y - self.env_y);
            for arm in &mut self.arms {
                arm.decay(self.stats_k);
            }
            let now = usize::from(self.env_y > self.env_x);
            if now == self.current {
                self.run = self.run.saturating_add(1);
            } else {
                self.current = now;
                self.run = 0;
            }
            // The point from `lookahead` samples ago counts when this arm has held from `hold`
            // samples before it until now.
            self.pending.push_back((x, y));
            if self.pending.len() > self.lookahead {
                let (px, py) = self.pending.pop_front().unwrap_or_default();
                if self.run >= self.hold + self.lookahead {
                    let arm = &mut self.arms[now];
                    arm.sxx += px * px;
                    arm.syy += py * py;
                    arm.sxy += px * py;
                }
            }
            self.phase += 1;
            if self.phase >= DECIMATE {
                self.phase = 0;
                if self.ring.len() == RING {
                    self.ring.pop_front();
                }
                self.ring.push_back((x as f32, y as f32));
                self.kept += 1;
            }
        }
    }

    /// The newest `n` XY points (fewer while the ring fills), oldest first.
    pub fn points(&self, n: usize) -> impl Iterator<Item = (f32, f32)> + '_ {
        self.ring
            .iter()
            .skip(self.ring.len().saturating_sub(n))
            .copied()
    }

    /// How many points the ring holds (at most [`RING`]).
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// The newest `n` points as a [`ScopeFrame`].
    pub fn frame(&self, n: usize) -> ScopeFrame {
        let peak = self
            .points(n)
            .fold(0.0f32, |m, (x, y)| m.max(x.abs()).max(y.abs()));
        let scale = if peak > 0.0 { 127.0 / peak } else { 0.0 };
        let q = |v: f32| (v * scale).round().clamp(-127.0, 127.0) as i8;
        ScopeFrame {
            seq: self.kept,
            peak,
            xy: self.points(n).flat_map(|(x, y)| [q(x), q(y)]).collect(),
            offset_hz: match self.tuning() {
                Tuning::NoSignal => None,
                Tuning::Offset(hz) => Some(hz),
            },
        }
    }

    /// Where the signal sits relative to the decoder's tones, read from the phase difference the
    /// two resonators put on each arm's tone (the tilt the operator sees).
    pub fn tuning(&self) -> Tuning {
        let total: f64 = self.arms.iter().map(Arm::energy).sum();
        if total <= 0.0 {
            return Tuning::NoSignal;
        }
        let mut estimates = Vec::with_capacity(2);
        // Arm 0 is the mark arm (X dominant), arm 1 the space arm (Y dominant).
        for (i, arm) in self.arms.iter().enumerate() {
            let (own, other) = if i == 0 {
                (arm.sxx, arm.syy)
            } else {
                (arm.syy, arm.sxx)
            };
            if arm.energy() < MIN_ARM_SHARE * total || own < DOMINANCE * other {
                continue;
            }
            let tone = if i == 0 { self.mark_hz } else { self.space_hz } as f64;
            if let Some(hz) = self.invert(tone, arm.theta()) {
                estimates.push((hz, arm.energy()));
            }
        }
        if estimates.is_empty() {
            return Tuning::NoSignal;
        }
        let weight: f64 = estimates.iter().map(|(_, w)| w).sum();
        let hz = estimates.iter().map(|(hz, w)| hz * w).sum::<f64>() / weight;
        Tuning::Offset(hz as f32)
    }

    /// The model: the phase difference, folded into [0, π], between the two resonators for a
    /// tone at `f` Hz.
    fn model_theta(&self, f: f64) -> f64 {
        let d = self.space.phase(self.sample_rate, f) - self.mark.phase(self.sample_rate, f);
        let wrapped = (d + PI).rem_euclid(2.0 * PI) - PI;
        wrapped.abs()
    }

    /// The offset δ (Hz) at which a tone near `tone` would show phase difference `theta`, by
    /// bisection over the range where the model is monotonic (up to half the shift either way,
    /// less a margin). A measurement past either end reads as that end.
    fn invert(&self, tone: f64, theta: f64) -> Option<f64> {
        let half = ((self.space_hz - self.mark_hz).abs() as f64 / 2.0 - 5.0).max(5.0);
        let (mut lo, mut hi) = (-half, half);
        let (t_lo, t_hi) = (self.model_theta(tone + lo), self.model_theta(tone + hi));
        if !(t_lo.is_finite() && t_hi.is_finite()) || (t_hi - t_lo).abs() < 1e-9 {
            return None;
        }
        let rising = t_hi > t_lo;
        if (rising && theta <= t_lo) || (!rising && theta >= t_lo) {
            return Some(lo);
        }
        if (rising && theta >= t_hi) || (!rising && theta <= t_hi) {
            return Some(hi);
        }
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            let t = self.model_theta(tone + mid);
            if (t < theta) == rising {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(0.5 * (lo + hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 12_000.0;
    const MARK: f32 = 2125.0;
    const SPACE: f32 = 2295.0;

    fn tone(f: f32, secs: f32) -> Vec<f32> {
        let n = (SR * secs) as usize;
        (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * f as f64 * i as f64 / SR as f64).sin() as f32 * 0.5
            })
            .collect()
    }

    /// Phase-continuous FSK alternating mark and space every bit at 45.45 Bd (diddle-like).
    fn fsk(mark: f32, space: f32, secs: f32) -> Vec<f32> {
        let n = (SR * secs) as usize;
        let bit = SR as f64 / 45.45;
        let mut ph = 0.0f64;
        (0..n)
            .map(|i| {
                let f = if ((i as f64 / bit) as usize).is_multiple_of(2) {
                    mark
                } else {
                    space
                };
                ph += 2.0 * std::f64::consts::PI * f as f64 / SR as f64;
                ph.sin() as f32 * 0.5
            })
            .collect()
    }

    /// Deterministic white noise (an LCG), so the noise test never flakes.
    fn noise(secs: f32) -> Vec<f32> {
        let mut s: u32 = 0x1234_5678;
        (0..(SR * secs) as usize)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect()
    }

    /// The principal-axis angle (degrees, counter-clockwise from +X) of every point in the ring.
    fn axis_deg(scope: &TuningScope) -> f64 {
        let (mut sxx, mut syy, mut sxy) = (0.0f64, 0.0f64, 0.0f64);
        for (x, y) in scope.points(RING) {
            let (x, y) = (x as f64, y as f64);
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
        }
        0.5 * (2.0 * sxy).atan2(sxx - syy).to_degrees()
    }

    /// THE PICTURE'S ROTATION for a signal `delta` Hz off: the mark arm's lean from horizontal
    /// and the space arm's from vertical, both counter-clockwise, averaged. Positive = the cross
    /// turned counter-clockwise. Each arm is measured from its own pure tone, from the drawn
    /// points alone, so nothing here depends on the scope's own estimate.
    fn rotation_deg(make: impl Fn() -> TuningScope, delta: f32) -> f64 {
        let mut m = make();
        let (mark, space) = m.tones();
        m.feed(&tone(mark + delta, 0.3));
        let mark_arm = axis_deg(&m);
        let mut s = make();
        s.feed(&tone(space + delta, 0.3));
        let mut space_arm = axis_deg(&s) - 90.0;
        if space_arm < -90.0 {
            space_arm += 180.0;
        }
        (mark_arm + space_arm) / 2.0
    }

    /// "The tilt tells you which way": high and low turn the cross in OPPOSITE directions.
    fn discriminates(make: impl Fn() -> TuningScope) -> bool {
        let (hi, lo) = (rotation_deg(&make, 20.0), rotation_deg(&make, -20.0));
        hi < -1.0 && lo > 1.0
    }

    fn power(scope: &TuningScope) -> (f64, f64) {
        scope.points(RING).fold((0.0, 0.0), |(xx, yy), (x, y)| {
            (xx + (x * x) as f64, yy + (y * y) as f64)
        })
    }

    #[test]
    fn a_mark_tone_draws_along_x_and_a_space_tone_along_y() {
        let mut s = TuningScope::new(SR, MARK, SPACE);
        s.feed(&tone(MARK, 0.3));
        let (xx, yy) = power(&s);
        assert!(xx > 10.0 * yy, "mark: X power {xx} must dwarf Y power {yy}");

        let mut s = TuningScope::new(SR, MARK, SPACE);
        s.feed(&tone(SPACE, 0.3));
        let (xx, yy) = power(&s);
        assert!(
            yy > 10.0 * xx,
            "space: Y power {yy} must dwarf X power {xx}"
        );
    }

    #[test]
    fn the_tilt_shows_which_way_the_signal_is_mistuned() {
        let make = || TuningScope::new(SR, MARK, SPACE);
        let tuned = rotation_deg(make, 0.0);
        let high = rotation_deg(make, 20.0);
        let low = rotation_deg(make, -20.0);
        assert!(
            tuned.abs() < 1.0,
            "a tuned cross is square: rotation {tuned:.2}°"
        );
        assert!(
            high < -1.0,
            "a signal 20 Hz high turns the cross clockwise: {high:.2}°"
        );
        assert!(
            low > 1.0,
            "a signal 20 Hz low turns it counter-clockwise: {low:.2}°"
        );
        assert!(discriminates(make));
    }

    /// POSITIVE CONTROL for the test above: with both resonators on one frequency the outputs are
    /// identical, every point sits on the diagonal and nothing says which way the signal is off.
    /// The same discriminating predicate must FAIL here, or it could not fail anywhere.
    #[test]
    fn control_resonators_on_one_tone_cannot_tell_high_from_low() {
        let make = || TuningScope::new(SR, MARK, MARK + 0.001);
        assert!(!discriminates(make));
    }

    #[test]
    fn the_read_out_names_the_offset_by_value_and_sign() {
        for delta in [-40.0f32, -20.0, -10.0, 0.0, 10.0, 20.0, 40.0] {
            let mut s = TuningScope::new(SR, MARK, SPACE);
            s.feed(&fsk(MARK + delta, SPACE + delta, 2.0));
            match s.tuning() {
                Tuning::Offset(hz) => assert!(
                    (hz - delta).abs() < 3.0,
                    "a signal {delta} Hz off read as {hz:.1} Hz"
                ),
                Tuning::NoSignal => panic!("a clean signal {delta} Hz off read as no signal"),
            }
        }
    }

    #[test]
    fn reverse_sense_reads_the_same_direction() {
        // Mark above space: the scope's X is still the mark resonator.
        let mut s = TuningScope::new(SR, SPACE, MARK);
        s.feed(&fsk(SPACE + 20.0, MARK + 20.0, 2.0));
        match s.tuning() {
            Tuning::Offset(hz) => assert!((hz - 20.0).abs() < 3.0, "read {hz:.1}"),
            Tuning::NoSignal => panic!("no signal"),
        }
    }

    #[test]
    fn a_lone_carrier_on_mark_reads_from_its_one_arm() {
        let mut s = TuningScope::new(SR, MARK, SPACE);
        s.feed(&tone(MARK - 15.0, 2.0));
        match s.tuning() {
            Tuning::Offset(hz) => assert!((hz + 15.0).abs() < 3.0, "read {hz:.1}"),
            Tuning::NoSignal => panic!("no signal"),
        }
    }

    #[test]
    fn band_noise_reads_as_no_signal() {
        let mut s = TuningScope::new(SR, MARK, SPACE);
        s.feed(&noise(2.0));
        assert_eq!(s.tuning(), Tuning::NoSignal);
        let mut quiet = TuningScope::new(SR, MARK, SPACE);
        quiet.feed(&[0.0; 12_000]);
        assert_eq!(quiet.tuning(), Tuning::NoSignal);
    }

    #[test]
    fn the_ring_is_bounded_and_decimated() {
        let mut s = TuningScope::new(SR, MARK, SPACE);
        s.feed(&tone(MARK, 0.01)); // 120 samples
        assert_eq!(s.len(), 120 / DECIMATE);
        s.feed(&tone(MARK, 2.0));
        assert_eq!(s.len(), RING);
        assert_eq!(s.points(256).count(), 256);
        assert_eq!(s.frame(256).xy.len(), 512);
    }

    #[test]
    fn retune_follows_a_move_and_ignores_a_wobble() {
        let mut s = TuningScope::new(SR, MARK, SPACE);
        s.feed(&tone(MARK, 0.1));
        let seq = s.frame(10).seq;
        assert!(
            !s.retune(MARK + 0.4, SPACE + 0.4),
            "an AFC wobble keeps the picture"
        );
        assert_eq!(s.tones(), (MARK, SPACE));
        assert!(!s.is_empty());
        assert!(s.retune(MARK + 12.0, SPACE + 12.0));
        assert_eq!(s.tones(), (MARK + 12.0, SPACE + 12.0));
        assert!(s.is_empty(), "the old picture described the old tones");
        assert_eq!(s.frame(10).seq, seq, "the count carries on");
        // The moved resonators are what a signal 12 Hz up now sits on.
        s.feed(&fsk(MARK + 12.0, SPACE + 12.0, 2.0));
        match s.tuning() {
            Tuning::Offset(hz) => assert!(hz.abs() < 3.0, "read {hz:.1}"),
            Tuning::NoSignal => panic!("no signal"),
        }
    }

    #[test]
    fn clear_drops_the_picture_and_the_estimate() {
        let mut s = TuningScope::new(SR, MARK, SPACE);
        s.feed(&fsk(MARK, SPACE, 1.0));
        assert!(matches!(s.tuning(), Tuning::Offset(_)));
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.tuning(), Tuning::NoSignal);
    }

    #[test]
    fn a_frame_is_scaled_to_its_own_peak() {
        let mut s = TuningScope::new(SR, MARK, SPACE);
        s.feed(&tone(MARK, 0.5));
        let f = s.frame(256);
        assert!(
            f.peak > 0.3 && f.peak < 0.6,
            "a 0.5 tone on mark peaks near 0.5: {}",
            f.peak
        );
        assert_eq!(f.xy.iter().map(|v| v.unsigned_abs()).max(), Some(127));
        let empty = TuningScope::new(SR, MARK, SPACE).frame(256);
        assert_eq!(
            (empty.peak, empty.xy.len(), empty.offset_hz),
            (0.0, 0, None)
        );
    }
}
