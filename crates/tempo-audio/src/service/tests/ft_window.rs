//! FT8 and FT4 overs on WSJT-X's window, with their tones on the slot clock, on the real loop.
//!
//! WSJT-X (upstream master, checked line by line) keys from the clock and ends an over on it: the
//! over is on the air while the slot position is under `txDuration` (FT8 13.64 s, FT4 6.04 s;
//! `helper_functions.cpp:6-7`, `widgets/mainwindow.cpp:5012-5015, 5055`), the GUI tick past it stops
//! the audio (`:5180`, `:5530`, `stopTx` `:5743-5753`), and PTT drops 200 ms later (`ptt0Timer` →
//! `stopTx2`, `:5753-5760`). Its Modulator lays the tones on the clock: silence up to their nominal
//! start, the symbols a late start has passed skipped (`Modulator/Modulator.cpp:56-57, 85-101`).
//! Nexus ended an over at key + wave + 250 ms and played its tones from the key, so keying latency
//! moved the whole over late and cut its end at the boundary.
//!
//! The loop is the real `RadioLoop::step`, on a rigctld that keys and a mock sound card. Every
//! scene runs at a synthetic time 30 s or more ahead of the real clock: the key re-reads the clock
//! and keeps the later of the two (`slot::slot_tx_phase`), so each key lands at exactly the phase
//! the scene ticks it at.

use super::*;
use crate::rig::remote_tests::{retuning_peer, writes, Peer};

/// The loop on a rigctld that keys, for an engine with a CQ to send at its next own boundary.
struct Over {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    peer: Peer,
    t: f64,
    /// The wall clock when the rigctld first received the key (`T 1`).
    key_at: Arc<Mutex<Option<f64>>>,
}

impl Over {
    /// `tier`, armed (TX on, as Call CQ arms it) with nothing sent yet; the CQ is queued by
    /// [`Self::cq`]. The loop's clock starts on an even (this station's) boundary of `tier`, at
    /// least 30 s ahead of the real clock.
    fn new(tier: Tier) -> Over {
        let key_at = Arc::new(Mutex::new(None));
        let noted = Arc::clone(&key_at);
        let peer = retuning_peer(14_074_000, "PKTUSB", move |line, state| {
            (line == "T 1").then(|| {
                noted.lock().unwrap().get_or_insert(now_unix_ms());
                state.keyed = true;
                "RPRT 0\n".to_string()
            })
        });
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        let period_ms = {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.set_tier(tier);
            e.set_frequency(14.074, "20m", "USB");
            e.active_slot_secs() * 1000.0
        };
        let pair = 2.0 * period_ms;
        let t = ((now_unix_ms() + 30_000.0) / pair).ceil() * pair;
        Over {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&peer.address),
            peer,
            t,
            key_at,
        }
    }

    /// Queue a CQ for the next own boundary (Call CQ's arm, its snappy first over drained).
    fn cq(&self) {
        let mut e = self.engine.lock().unwrap();
        e.broadcast("CQ KD9TAW EN52");
        let _ = e.take_immediate_tx();
    }

    /// Queue a CQ that keys at once if it can (Call CQ inside this station's own slot).
    fn cq_now(&self) {
        self.engine.lock().unwrap().broadcast("CQ KD9TAW EN52");
    }

    /// Run the loop, a 20 ms tick at a time, to `until` ms (inclusive).
    fn run_to(&mut self, until: f64) {
        step_to(
            &self.engine,
            &mut self.state,
            &mut self.backend,
            &mut self.rig,
            &mut self.t,
            until,
        );
    }

    /// How many times `line` went on the wire.
    fn sent(&self, line: &str) -> usize {
        writes(&self.peer).iter().filter(|l| *l == line).count()
    }
}

/// FT8's slot-positioned wave, in samples: 0.5 s of lead-in and 79 symbols of 1920 (12.64 s).
const FT8_WAVE: usize = 6_000 + 79 * 1_920;

/// `got` is exactly `want`, or why not in a few words: an over is 150 000 samples, too many to print.
fn same_audio(got: &[f32], want: &[f32]) -> Result<(), String> {
    if got.len() != want.len() {
        return Err(format!("{} samples, expected {}", got.len(), want.len()));
    }
    match got.iter().zip(want).position(|(a, b)| a != b) {
        Some(i) => Err(format!("differs from sample {i}")),
        None => Ok(()),
    }
}

/// ⭐ AN ON-TIME FT8 OVER: TONES FROM 0.5 s, AUDIO STOPPED AT 13.64 s, PTT OFF AT 13.84 s, AS WSJT-X.
/// The over used to drop PTT at key + 13.14 s + 250 ms = 13.39 s, 0.45 s short of WSJT-X, with
/// 250 ms of room for the output's latency against WSJT-X's 500 ms.
#[test]
fn an_on_time_ft8_over_stops_its_audio_and_drops_ptt_at_wsjtxs_times() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.cq();
    s.run_to(t0);
    assert_eq!(s.sent("T 1"), 1, "premise: the boundary keyed");
    assert_eq!(
        s.backend.played.len(),
        FT8_WAVE,
        "an on-time over plays its whole wave: the lead-in puts the tones at 0.5 s"
    );
    assert_eq!(s.state.tx_until_ms, Some(t0 + 13_840.0), "PTT off");
    let flushes = s.backend.flush_calls;
    s.run_to(t0 + 13_620.0);
    assert_eq!(s.backend.flush_calls, flushes, "the audio stopped early");
    s.run_to(t0 + 13_640.0);
    assert_eq!(
        s.backend.flush_calls,
        flushes + 1,
        "the audio did not stop at 13.64 s"
    );
    assert_eq!(s.sent("T 0"), 0, "PTT dropped with the audio");
    s.run_to(t0 + 13_820.0);
    assert_eq!(s.sent("T 0"), 0, "PTT dropped before 13.84 s");
    s.run_to(t0 + 13_840.0);
    assert_eq!(s.sent("T 0"), 1, "PTT did not drop at 13.84 s");
    assert_eq!(s.state.tx_until_ms, None);
}

/// ⭐ FT4 THE SAME WAY: AUDIO STOPPED AT 6.04 s, PTT OFF AT 6.24 s (WSJT-X's `txDuration` 6.04 s and
/// its 200 ms). The over used to hold PTT to key + 6.548 s + 250 ms = 6.80 s. The 0.5 s lead-in is
/// Nexus's own (WSJT-X's Modulator starts FT4 at 0.3 s; `crates/modes/src/mode.rs` says why) and is
/// unchanged: the tones still start at 0.5 s, and end at 5.54 s.
#[test]
fn an_on_time_ft4_over_stops_its_audio_and_drops_ptt_at_wsjtxs_times() {
    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.cq();
    s.run_to(t0);
    assert_eq!(s.sent("T 1"), 1, "premise: the boundary keyed");
    let played = &s.backend.played;
    assert!(
        played[..6_000].iter().all(|&x| x == 0.0) && played[6_000..6_100].iter().any(|&x| x != 0.0),
        "FT4's tones do not start 0.5 s into the slot, as before"
    );
    assert_eq!(s.state.tx_until_ms, Some(t0 + 6_240.0), "PTT off");
    let flushes = s.backend.flush_calls;
    s.run_to(t0 + 6_020.0);
    assert_eq!(s.backend.flush_calls, flushes, "the audio stopped early");
    s.run_to(t0 + 6_040.0);
    assert_eq!(
        s.backend.flush_calls,
        flushes + 1,
        "the audio did not stop at 6.04 s"
    );
    s.run_to(t0 + 6_220.0);
    assert_eq!(s.sent("T 0"), 0, "PTT dropped before 6.24 s");
    s.run_to(t0 + 6_240.0);
    assert_eq!(s.sent("T 0"), 1, "PTT did not drop at 6.24 s");
}

/// ⭐ A LATE KEY LOSES ITS FIRST SYMBOLS, NOT ITS LAST: THE TONES STAY ON THE SLOT CLOCK AND THE OVER
/// ENDS WHERE AN ON-TIME ONE DOES. A boundary keyed 2 s late (a slow decode, a CAT read in the
/// boundary tick) used to play the whole over from the key: every tone 2 s late (DT +2) and the end
/// cut at the boundary. Now it plays from where the slot is, as WSJT-X's Modulator does.
#[test]
fn a_late_ft8_key_skips_its_first_symbols_and_ends_on_time() {
    let mut on_time = Over::new(Tier::Ft8);
    let t0 = on_time.t;
    on_time.cq();
    on_time.run_to(t0);
    assert_eq!(
        on_time.backend.played.len(),
        FT8_WAVE,
        "premise: the control"
    );

    let mut late = Over::new(Tier::Ft8);
    let t0 = late.t;
    late.cq();
    late.t = t0 + 2_000.0; // the loop's first tick in the slot, 2 s in: the boundary keys late
    late.run_to(t0 + 2_000.0);
    assert_eq!(late.sent("T 1"), 1, "premise: the late boundary keyed");
    let skipped = 24_000; // 2.000 s at 12 kHz: the 0.5 s lead-in and the first 9.375 symbols
    if let Err(why) = same_audio(&late.backend.played, &on_time.backend.played[skipped..]) {
        panic!("the late over is not the on-time over's audio from 2.0 s on: {why}");
    }
    assert_eq!(
        late.state.tx_until_ms,
        Some(t0 + 13_840.0),
        "a late key moved the end of the over"
    );
    late.run_to(t0 + 13_640.0);
    late.run_to(t0 + 13_840.0);
    assert_eq!(late.sent("T 0"), 1, "PTT did not drop at 13.84 s");
}

/// ⭐ A CALL CQ INSIDE THIS STATION'S OWN SLOT (THE LATE START) IS ON THE SLOT CLOCK TOO. It used to
/// trim the wave so its END met the boundary: a CQ at 3 s played from 1.14 s of the wave (tones
/// 1.86 s late) and held PTT to 15.25 s, 250 ms into the other station's slot. Now it plays from 3 s
/// of the wave, ends with the window, and still keys at once.
#[test]
fn a_late_start_ft8_cq_plays_on_the_slot_clock_and_ends_with_the_window() {
    let mut on_time = Over::new(Tier::Ft8);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.run_to(t0 + 2_980.0);
    assert_eq!(s.sent("T 1"), 0, "premise: nothing to send at the boundary");
    s.cq_now();
    s.run_to(t0 + 3_000.0);
    assert_eq!(s.sent("T 1"), 1, "the late start did not key at once");
    if let Err(why) = same_audio(&s.backend.played, &on_time.backend.played[36_000..]) {
        panic!("the late start is not the on-time over's audio from 3.0 s on: {why}");
    }
    assert_eq!(s.state.tx_until_ms, Some(t0 + 13_840.0), "PTT off");
}

// ── How late a late key may be: WSJT-X's 75 % of the period ────────────────────────────────────
// WSJT-X asserts PTT only while the slot is under 75 % of its period (`widgets/mainwindow.cpp:5114`,
// `:5126`, `fTR < 0.75`, "Allow late starts"): FT8 to 11.25 s, FT4 to 5.625 s, the symbols that remain
// sent on the clock. Later, the over waits for the next own slot. Nexus stopped a late start at its
// own budget (6.0 s of FT8's leading audio, 3.0 s of FT4's) and keyed a late boundary as long as any
// tone remained.

/// ⭐ A CALL CQ INSIDE THIS STATION'S OWN SLOT KEYS AT ONCE UNTIL 75 % OF IT, AS WSJT-X'S DOES: 6.5 s
/// and 11.2 s into an FT8 slot, the symbols that remain on the slot clock and the over ended with
/// the window. Both waited for the next own slot under the old budget.
#[test]
fn a_late_start_ft8_cq_keys_at_once_until_75_percent_of_the_slot() {
    let mut on_time = Over::new(Tier::Ft8);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut waited = Vec::new();
    for at_ms in [6_500.0, 11_200.0] {
        let mut s = Over::new(Tier::Ft8);
        let t0 = s.t;
        s.run_to(t0 + at_ms - 20.0);
        s.cq_now();
        s.run_to(t0 + at_ms);
        if s.sent("T 1") != 1 {
            waited.push(at_ms);
            continue;
        }
        let skipped = (at_ms * 12.0) as usize;
        if let Err(why) = same_audio(&s.backend.played, &on_time.backend.played[skipped..]) {
            panic!("the CQ {at_ms} ms in is not the on-time over's audio from there on: {why}");
        }
        assert_eq!(
            s.state.tx_until_ms,
            Some(t0 + 13_840.0),
            "{at_ms} ms in: PTT off"
        );
    }
    assert!(
        waited.is_empty(),
        "a CQ this many ms into its own slot did not key at once: {waited:?}"
    );
}

/// AND FROM 75 % IT WAITS FOR THIS STATION'S NEXT SLOT, AS WSJT-X'S DOES: a CQ 11.3 s into an FT8
/// slot keys nothing in it, and goes out whole, on time, at the next own boundary.
#[test]
fn a_late_start_past_75_percent_waits_for_the_next_own_slot() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.run_to(t0 + 11_280.0);
    s.cq_now();
    s.run_to(t0 + 29_980.0);
    assert_eq!(s.sent("T 1"), 0, "a CQ 11.3 s in keyed this slot");
    s.run_to(t0 + 30_000.0);
    assert_eq!(
        s.sent("T 1"),
        1,
        "and it keys at the next own boundary, on time"
    );
    assert_eq!(s.backend.played.len(), FT8_WAVE);
}

/// FT4 THE SAME WAY: 75 % of its 7.5 s slot is 5.625 s. A CQ 5.5 s in keys at once with the 40 ms of
/// tones still ahead of the clock (FT4's 0.5 s lead-in puts its last tone at 5.54 s) and ends with
/// the window; 5.7 s in, it waits for the next own slot. The old budget stopped FT4 at 3.0 s.
#[test]
fn an_ft4_late_start_keys_until_75_percent_and_later_waits() {
    let mut on_time = Over::new(Tier::Ft4);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 5_480.0);
    s.cq_now();
    s.run_to(t0 + 5_500.0);
    assert_eq!(s.sent("T 1"), 1, "a CQ 5.5 s in did not key at once");
    if let Err(why) = same_audio(&s.backend.played, &on_time.backend.played[66_000..]) {
        panic!("the CQ 5.5 s in is not the on-time over's audio from there on: {why}");
    }
    assert_eq!(s.state.tx_until_ms, Some(t0 + 6_240.0), "PTT off");

    let mut s = Over::new(Tier::Ft4);
    let t0 = s.t;
    s.run_to(t0 + 5_680.0);
    s.cq_now();
    s.run_to(t0 + 14_980.0);
    assert_eq!(s.sent("T 1"), 0, "a CQ 5.7 s in keyed this slot");
    s.run_to(t0 + 15_000.0);
    assert_eq!(s.sent("T 1"), 1, "and it keys at the next own boundary");
    assert_eq!(
        s.backend.played.len(),
        on_time.backend.played.len(),
        "and whole"
    );
}

/// ⭐ A BOUNDARY WHOSE KEY COMES PAST 75 % OF THE SLOT IS NOT KEYED, AND ITS OVER WAITS: the loop's
/// first tick in the slot is 11.3 s in (a decode that drained that late, a stalled computer), where
/// WSJT-X never asserts PTT. Nothing keys, and the CQ, never planned, goes out whole at the next own
/// boundary. It used to key and send the symbols still ahead (to 13.14 s). The control, 11.2 s in,
/// keys and sends what remains on the slot clock, ending with the window.
#[test]
fn a_boundary_keyed_past_75_percent_waits_for_the_next_own_slot() {
    let mut on_time = Over::new(Tier::Ft8);
    on_time.cq();
    let t0 = on_time.t;
    on_time.run_to(t0);

    let mut late = Over::new(Tier::Ft8);
    let t0 = late.t;
    late.cq();
    late.t = t0 + 11_300.0; // the loop's first tick in the slot, 11.3 s in
    late.run_to(t0 + 29_980.0);
    assert_eq!(late.sent("T 1"), 0, "a boundary keyed 11.3 s in");
    late.run_to(t0 + 30_000.0);
    assert_eq!(
        late.sent("T 1"),
        1,
        "the CQ did not go out at the next own boundary"
    );
    // The same over, whole: the CQ's first frame, which a plan at 11.3 s would have spent.
    if let Err(why) = same_audio(&late.backend.played, &on_time.backend.played) {
        panic!("the next own boundary did not send the over that waited, whole: {why}");
    }

    let mut control = Over::new(Tier::Ft8);
    let t0 = control.t;
    control.cq();
    control.t = t0 + 11_200.0;
    control.run_to(t0 + 11_200.0);
    assert_eq!(control.sent("T 1"), 1, "control: a boundary 11.2 s in");
    if let Err(why) = same_audio(&control.backend.played, &on_time.backend.played[134_400..]) {
        panic!("control: not the on-time over's audio from 11.2 s on: {why}");
    }
    assert_eq!(
        control.state.tx_until_ms,
        Some(t0 + 13_840.0),
        "control: PTT off"
    );
}

/// CONTROL: FT2 KEEPS ITS OWN LATE START. WSJT-X's 75 % is FT8's and FT4's; FT2 still keys a late start
/// only while its whole over (2.52 s) fits before the next boundary: 1.2 s into its 3.75 s slot it
/// keys at once, 1.3 s in (75 % would reach 2.81 s) it waits for the next own slot.
#[test]
fn an_ft2_late_start_keeps_its_whole_over_rule() {
    let mut s = Over::new(Tier::Ft2);
    let t0 = s.t;
    s.run_to(t0 + 1_180.0);
    s.cq_now();
    s.run_to(t0 + 1_200.0);
    assert_eq!(s.sent("T 1"), 1, "a CQ 1.2 s in did not key at once");

    let mut s = Over::new(Tier::Ft2);
    let t0 = s.t;
    s.run_to(t0 + 1_280.0);
    s.cq_now();
    s.run_to(t0 + 7_480.0);
    assert_eq!(s.sent("T 1"), 0, "a CQ 1.3 s in keyed this slot");
    s.run_to(t0 + 7_500.0);
    assert_eq!(s.sent("T 1"), 1, "and it keys at the next own boundary");
}

/// ⭐ STOP TX MID-OVER STILL CUTS AT ONCE, AND NOTHING OF THE CUT OVER FIRES AFTER IT. The window's
/// audio stop belongs to the over it was set for: once Stop TX has cut it, 13.64 s flushes nothing
/// and 13.84 s unkeys nothing.
#[test]
fn stop_tx_mid_over_cuts_at_once_and_the_window_does_not_fire_after() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.cq();
    s.run_to(t0 + 4_980.0);
    assert_eq!(s.sent("T 1"), 1, "premise: keyed");
    let flushes = s.backend.flush_calls;
    s.engine.lock().unwrap().halt_tx();
    s.run_to(t0 + 5_000.0);
    assert_eq!(s.sent("T 0"), 1, "Stop TX did not unkey on the next tick");
    assert_eq!(s.backend.flush_calls, flushes + 1, "Stop TX did not flush");
    s.run_to(t0 + 14_000.0);
    assert_eq!(
        s.backend.flush_calls,
        flushes + 1,
        "the cut over's window flushed later"
    );
    assert_eq!(s.sent("T 0"), 1, "the cut over's window unkeyed later");
}

/// TX OFF MID-OVER LETS THE OVER FINISH, AS IT ALWAYS DID (operator, 2026-07-31), NOW ON WSJT-X'S
/// WINDOW: the audio stops at 13.64 s and PTT drops at 13.84 s, and the next slot keys nothing.
#[test]
fn tx_off_mid_over_lets_it_finish_on_the_window() {
    let mut s = Over::new(Tier::Ft8);
    let t0 = s.t;
    s.cq();
    s.run_to(t0 + 4_980.0);
    s.engine.lock().unwrap().set_tx_enabled_by_operator(false);
    let flushes = s.backend.flush_calls;
    s.run_to(t0 + 13_620.0);
    assert_eq!(
        s.sent("T 0"),
        0,
        "PTT dropped before 13.64 s (TX Off, or a shorter hold)"
    );
    assert_eq!(
        s.backend.flush_calls, flushes,
        "the audio was cut before 13.64 s"
    );
    s.run_to(t0 + 13_640.0);
    assert_eq!(
        s.backend.flush_calls,
        flushes + 1,
        "the audio did not stop at 13.64 s"
    );
    s.run_to(t0 + 13_840.0);
    assert_eq!(s.sent("T 0"), 1, "PTT did not drop at 13.84 s");
    s.run_to(t0 + 30_100.0);
    assert_eq!(s.sent("T 1"), 1, "TX Off keyed the next over");
}

// ── A busy decoder does not hold up the key ────────────────────────────────────────────────────
// An early decode still running at the boundary holds the modem, and building an over needs the
// modem too, so the key waited for the decode and the over went out late. WSJT-X's decoder is
// another process and never delays a key. The early pass now builds the next over before it
// dispatches its decode, and a boundary whose plan asks for exactly that over keys it as built.

/// Steer the loop's clock (and the engine's, which the loop follows) so the REAL clock reads
/// `phase_ms` into the odd slot before one of this station's even boundaries, and return that
/// boundary. The busy path commits its over against a fresh read of the real clock
/// (`commit_tx`'s slot check), so these scenes run on it rather than ahead of it.
fn steer_into_the_odd_slot(over: &mut Over, phase_ms: f64) -> f64 {
    let pair = 2.0 * 15_000.0;
    let real = now_unix_ms();
    let offset = (real.rem_euclid(pair) - (15_000.0 + phase_ms)).round() as i64;
    over.engine
        .lock()
        .unwrap()
        .set_clock_offset_ms(Some(offset));
    over.state.clock_offset_ms = offset;
    ((real - offset as f64) / pair).ceil() * pair
}

/// Move the steered clock to `before_ms` short of `boundary`, within the same slot.
fn steer_to_before(over: &mut Over, boundary: f64, before_ms: f64) {
    let offset = (now_unix_ms() - (boundary - before_ms)).round() as i64;
    over.engine
        .lock()
        .unwrap()
        .set_clock_offset_ms(Some(offset));
    over.state.clock_offset_ms = offset;
}

impl Over {
    /// One loop tick on the real clock; returns when the tick ended.
    fn tick_now(&mut self) -> std::time::Instant {
        self.tick_read_early(0.0)
    }

    /// One loop tick on the real clock, its clock read `early_ms` before the tick runs on (a tick
    /// that spent that long on CAT before it reached what is measured); returns when it ended.
    fn tick_read_early(&mut self, early_ms: f64) -> std::time::Instant {
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                now_unix_ms() - early_ms,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        std::time::Instant::now()
    }
}

/// A decode in flight: it holds the modem until told to let go, or `hold` passes. Returns the
/// way to tell it, and the thread, which returns when it let go.
fn a_decode_holding_the_modem(
    hold: Duration,
) -> (
    std::sync::mpsc::Sender<()>,
    std::thread::JoinHandle<std::time::Instant>,
) {
    let (holding, held) = std::sync::mpsc::channel();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let decode = std::thread::spawn(move || {
        let modem = loop {
            match tempo_core::modes::Ft8A7ResetGuard::try_acquire() {
                Some(modem) => break modem,
                None => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        holding.send(()).unwrap();
        let _ = wait.recv_timeout(hold);
        drop(modem);
        std::time::Instant::now()
    });
    held.recv().unwrap();
    (go, decode)
}

/// ⭐ A DECODE STILL RUNNING AT THE BOUNDARY DOES NOT DELAY THE KEY OF THE OVER BUILT BEFORE IT. The
/// early pass at 11.8 s builds the CQ the boundary is about to send before it dispatches its
/// decode; that decode is still holding the modem when the slot starts; the boundary keys the CQ
/// on time, with every tone in place (0.5 s), and the decode lets go only afterwards.
///
/// The control, in the same scene, is the over that was NOT built ahead (the TX frequency changed
/// after the early pass): it still waits for the decode, as every over used to, and now goes out
/// on the slot clock, what the wait cost skipped and its end unchanged.
///
/// ⚠️ THE MODEM LOCK IS THE WHOLE PROCESS'S, so every other test's decode waits while this scene
/// holds it (one of them gives its decode a second, `in_flight_guard_serializes_dispatch`). It is
/// held for the boundary tick only: let go as soon as that tick ends, or after 500 ms at most for
/// the control, whose tick waits for it.
#[test]
fn a_busy_decoder_does_not_delay_the_key_of_the_over_built_before_it() {
    for built_ahead in [true, false] {
        let what = if built_ahead {
            "built ahead"
        } else {
            "control: not built ahead"
        };
        let mut s = Over::new(Tier::Ft8);
        s.cq();
        let boundary = steer_into_the_odd_slot(&mut s, 11_700.0);
        let slot = (boundary / 15_000.0) as u64;
        let give_up = std::time::Instant::now() + Duration::from_secs(10);
        let tick_until = |s: &mut Over, done: &dyn Fn(&Over) -> bool, why: &str| loop {
            let ended = s.tick_now();
            if done(s) {
                break ended;
            }
            assert!(std::time::Instant::now() < give_up, "{what}: {why}");
            std::thread::sleep(Duration::from_millis(5));
        };
        s.tick_now(); // the odd slot's boundary, with nothing captured yet: no decode, no key
        for _ in 0..400 {
            s.backend.queue_capture(vec![0.0; 240]);
        }
        tick_until(&mut s, &|s| s.state.decode_in_flight, "no early pass");
        // The early pass dispatched its decode in that tick, and built the CQ before it.
        let hint = s
            .engine
            .lock()
            .unwrap()
            .next_over_hint(slot)
            .expect("the CQ");
        assert!(
            s.engine.lock().unwrap().prebuilt_wave(&hint).is_some(),
            "{what}: the early pass did not build the next over before its decode"
        );
        // Its decode of silence comes back at once; the one below stands in for a slow one.
        tick_until(
            &mut s,
            &|s| !s.state.decode_in_flight,
            "the early decode never came back",
        );
        if !built_ahead {
            s.engine.lock().unwrap().set_tx_offset(1_600.0);
        }
        // A decode still running at the boundary, holding the modem.
        let (go, decode) = a_decode_holding_the_modem(Duration::from_millis(500));
        s.state.decode_in_flight = true;
        steer_to_before(&mut s, boundary, 30.0);
        // The rig's own reads (health, dial, S-meter, its PTT) are held off past the boundary: the
        // clock moved forward just now, which makes them all due, and one landing in the boundary
        // tick delays the key by its own CAT round trips (hundreds of ms against this rigctld).
        // The tones on the clock absorb that too, but this scene measures the decode's hold on the
        // modem and nothing else.
        let later = boundary + 60_000.0;
        s.state.last_rig_poll = later;
        s.state.last_freq_poll = later;
        s.state.last_smeter_poll = later;
        s.state.last_ptt_poll = later;
        let keyed_tick_ended =
            tick_until(&mut s, &|s| s.state.last_slot == Some(slot), "no boundary");
        let tick_end_wall = now_unix_ms();
        let _ = go.send(());
        let decode_let_go = decode.join().unwrap();
        assert_eq!(s.sent("T 1"), 1, "{what}: the boundary did not key");
        // On the slot clock whatever the key cost: what was skipped is where the slot was between
        // the key reaching the radio and the tick ending (the carrier came up in between).
        let offset = s.state.clock_offset_ms as f64;
        let key_wall = s.key_at.lock().unwrap().expect("the key's time");
        let lo = ((key_wall - offset - boundary) * 12.0).floor() as usize;
        let hi = ((tick_end_wall - offset - boundary) * 12.0).ceil() as usize;
        assert_eq!(
            s.state.tx_until_ms,
            Some(boundary + 13_840.0),
            "{what}: the over's end moved"
        );
        let skipped = FT8_WAVE - s.backend.played.len();
        assert!(
            (lo..=hi).contains(&skipped),
            "{what}: {skipped} samples skipped, expected the key's place in the slot, {lo}..={hi}"
        );
        if built_ahead {
            assert!(
                keyed_tick_ended < decode_let_go,
                "{what}: the key waited for the decode to let go of the modem"
            );
        } else {
            assert!(
                keyed_tick_ended >= decode_let_go,
                "{what}: premise: an over not built ahead waits for the decode"
            );
            assert!(skipped < FT8_WAVE, "{what}: the over was dropped");
        }
    }
}

/// THE LATE START DECIDES ON THE CLOCK AT ITS DECISION, NOT ON ITS TICK'S, as WSJT-X decides on a
/// clock it has just read. A tick that read its clock 2 s before it reached the CQ (CAT earlier in
/// the tick) finds the slot 11.5 s in: it keys nothing, though its own clock said 9.5 s. At 10.5 s it
/// keys. On the real clock, steered into this station's own slot.
#[test]
fn a_late_start_decides_on_the_clock_at_the_decision_not_on_its_ticks() {
    for (at_ms, keys) in [(11_500.0, false), (10_500.0, true)] {
        let mut s = Over::new(Tier::Ft8);
        let even = steer_into_the_odd_slot(&mut s, 0.0);
        steer_to_before(&mut s, even - 15_000.0, 15_000.0 - at_ms);
        // The rig's own reads held off, as in the busy scene: the steering made them all due.
        let later = even + 60_000.0;
        s.state.last_rig_poll = later;
        s.state.last_freq_poll = later;
        s.state.last_smeter_poll = later;
        s.state.last_ptt_poll = later;
        s.tick_now(); // this slot's boundary, with nothing to send
        assert_eq!(s.sent("T 1"), 0, "premise: nothing to send at the boundary");
        s.cq_now();
        s.tick_read_early(2_000.0);
        assert_eq!(
            s.sent("T 1"),
            usize::from(keys),
            "a CQ {at_ms} ms in, its tick's clock 2 s behind"
        );
    }
}
