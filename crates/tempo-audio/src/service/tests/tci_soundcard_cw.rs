//! The Soundcard CW keyer on a TCI radio, end to end on the real radio loop with no radio. An F-key
//! macro goes to the SDR program as Nexus's own audio (`trx:0,true,tci`) with the program in DIGU:
//! a tone at the operator's pitch, keyed element by element as the keyer times them, the
//! transmitter dropped after the last element. With Audio over TCI off the key is a plain one and
//! the tone is played on the sound card. Esc ends the over on the loop's next tick with nothing of
//! it left queued. The radio's own keyer (the CAT keyer) keys nothing on TCI, and the CW screen
//! says the radio did not take it. The licence privileges and the TX latch still keep every word
//! off the air. And a macro whose route fails part way (the program stops pulling, or the
//! connection drops) ends there: never a silent over, and nothing of it on the sound card.
//!
//! The SDR program is the TCI simulator (`tempo-tcisim`) on loopback; the loop is the real
//! `RadioLoop::step` on the real clock, driving the real `Rig` into the TCI client's rigctld shim,
//! as `tci.rs`'s scenes do. What left Nexus is read from the simulator's log by value: the `trx`
//! commands in order, and the transmit audio it pulled, measured as a receiver hears it (the
//! tone's frequency, the length of every element and gap).

use super::*;
use crate::tci::{registry, Options as TciOptions, TciDaemon};
use std::cell::Cell;
use tempo_net::tci::Trx;
use tempo_tcisim::frame::{kind, SampleType, HEADER_LEN};
use tempo_tcisim::{
    Config as SimConfig, Dialect as SimDialect, Event as SimEvent, Fault, Keyer, Logged, Mic,
    Simulator, Source as SimSource,
};

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(10);

/// The operator's CW pitch: not the 600 Hz default, so the tone heard is the setting's.
const PITCH: f32 = 700.0;

/// The operator's CW speed: a 48 ms dit.
const WPM: u32 = 25;

/// The dial, in 20 m's CW segment and above 10 MHz, where the soundcard keyer's data mode is the
/// upper one: the tone goes out at the dial plus the pitch.
const DIAL_HZ: u64 = 14_030_000;

/// The macro the F-key sends: two words, so the over carries a word space and a second word.
const MACRO: &str = "CQ TEST";

/// [`MACRO`]'s two words as the keyer must key them, in dits: `=` the tone up, `.` down.
const MACRO_HEARD: [&str; 2] = ["===.=.===.=...===.===.=.===", "===...=...=.=.=...==="];

/// What the CW screen says when the licence keeps a send off the air.
const OUTSIDE_PRIVILEGES: &str = "CW not sent: this frequency is outside your license privileges, \
     so it was dropped, not held for later. Send it again from inside them.";

/// What the CW screen says when TX was off for a send's words.
const TX_WAS_OFF: &str = "CW stopped: transmit was turned off, so what was still to go was \
     dropped, not held for later. Send it again when you are ready.";

fn trx0() -> Trx {
    Trx::new(0).expect("trx 0")
}

fn dit_ms() -> f64 {
    1200.0 / f64::from(WPM)
}

/// The scene's sound card, as `tci.rs`'s: it keeps what is played on it, and hands an over to an
/// installed transmit tee instead; a Stop's flush goes to the tee.
#[derive(Clone, Default)]
struct Card {
    played: Arc<Mutex<Vec<f32>>>,
    tee: Arc<Mutex<Option<crate::backend::TxTeeHandle>>>,
}

impl AudioBackend for Card {
    fn capture(&mut self) -> Vec<f32> {
        vec![0.0; 240]
    }
    fn play(&mut self, samples: &[f32]) {
        match self.tee.lock().unwrap().as_ref() {
            Some(tee) => tee.feed(samples),
            None => self.played.lock().unwrap().extend_from_slice(samples),
        }
    }
    fn set_tx_tee(&mut self, tee: Option<crate::backend::TxTeeHandle>) {
        *self.tee.lock().unwrap() = tee;
    }
    fn flush_output(&mut self) -> usize {
        self.tee.lock().unwrap().as_ref().map_or(0, |t| t.flush())
    }
}

/// The radio loop on a radio of the TCI connection, its client on the simulated SDR program.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    card: Card,
    sim: Simulator,
    options: TciOptions,
    /// Whether a rebuild of the loop's CAT link may open a fresh client on the same program, as
    /// `open_cat` does; otherwise a rebuild fails the test (the client under test is gone).
    reopen_client: bool,
}

/// The production waits, but a session that keeps a stop the server never confirms open for the
/// reading rather than escalating after two seconds (`tci.rs`'s `patient`).
fn patient() -> TciOptions {
    TciOptions {
        adjust: |c| c.unkey_deadline_ms = 30_000,
        ..TciOptions::default()
    }
}

impl Scene {
    /// The scene, the engine set up by `setup` once the radio is on the TCI connection, Audio over
    /// TCI on as Settings has it by default.
    fn new(setup: impl FnOnce(&mut Engine)) -> Scene {
        Scene::on([], TciOptions::default(), setup)
    }

    /// [`Scene::new`] on a program with `faults`, its client's waits `options`.
    fn on(
        faults: impl IntoIterator<Item = Fault>,
        options: TciOptions,
        setup: impl FnOnce(&mut Engine),
    ) -> Scene {
        let sim = Simulator::start(SimConfig::dialect(SimDialect::expert_sdr3()).with(faults))
            .expect("the simulator starts");
        registry::forget((sim.addr(), trx0()));
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            let mut s = e.settings().clone();
            s.rig_model = 7;
            s.rig_conn = "tci".into();
            s.tci_host = "127.0.0.1".into();
            s.tci_port = sim.addr().port();
            s.ptt_method = "cat".into();
            e.apply_settings(s);
            setup(&mut e);
        }
        // The transport the loop compares its settings against: the same settings, so it never
        // rebuilds the link under the test.
        let applied = Transport::from_settings(engine.lock().unwrap().settings());
        let (daemon, rig) = Scene::client(&applied, &options);
        let cfg = RadioConfig {
            rig_model: 7,
            ..RadioConfig::default()
        };
        let state = RadioLoop::new(applied, Some(CatDaemon::Tci(daemon)), &cfg);
        Scene {
            engine,
            state,
            rig,
            card: Card::default(),
            sim,
            options,
            reopen_client: false,
        }
    }

    /// A client for `t` on an OS-chosen rigctld port, and the loop's `Rig` on it, as `open_cat`
    /// makes them.
    fn client(t: &Transport, options: &TciOptions) -> (TciDaemon, Rig) {
        let mut target = t.tci_target().expect("a TCI radio");
        target.rigctld_port = 0;
        let daemon =
            TciDaemon::start_receiving(&target, options.clone()).expect("the client starts");
        let mut rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
        rig.set_slow_transport(true);
        (daemon, rig)
    }

    /// One tick of the loop on the real clock, as `run_radio` runs it.
    fn step(&mut self) {
        let card = self.card.clone();
        let mut reopen_audio = move |_t: &Transport| Ok::<_, String>(card.clone());
        let (reopen_client, options) = (self.reopen_client, self.options.clone());
        let rebuilt = Cell::new(false);
        let mut reopen_rig = |t: &Transport, _coexist: bool| -> RigOpen {
            rebuilt.set(true);
            if !reopen_client {
                return (Rig::vox(), None, CatProbe::status(None, ""));
            }
            let (daemon, mut rig) = Scene::client(t, &options);
            let probe = finish_cat_open(&mut rig, t);
            (rig, Some(CatDaemon::Tci(daemon)), probe)
        };
        let mut station = StationSinks::new();
        self.state
            .step(
                &self.engine,
                &mut self.card,
                &mut self.rig,
                &no_sinks(),
                now_unix_ms(),
                &mut reopen_audio,
                &mut reopen_rig,
                &mut station,
            )
            .unwrap();
        assert!(
            reopen_client || !rebuilt.get(),
            "the loop rebuilt its CAT link: the client under test is gone"
        );
    }

    /// Run the loop a step then 20 ms, as `run_radio` does, for `ms` of real time.
    fn run(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Run the loop until `done`, or fail saying `what`.
    fn run_until(&mut self, what: &str, done: impl Fn(&Scene) -> bool) {
        let deadline = Instant::now() + WAIT;
        while !done(self) {
            assert!(Instant::now() < deadline, "{what}");
            self.step();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap()
    }

    fn daemon(&self) -> &TciDaemon {
        self.state
            .rigctld_proc
            .as_ref()
            .and_then(CatDaemon::tci)
            .expect("the TCI client")
    }

    /// The client's transmit route.
    fn route(&self) -> Arc<crate::tci::audio::TciTx> {
        self.daemon().tx().expect("the transmit route").clone()
    }

    /// Wait until the SDR program has taken Nexus's audio stream and its receive audio flows, and
    /// the loop has put its transmit tee in: a key of Nexus's audio would be admitted.
    fn routed(&mut self) {
        assert!(
            self.daemon()
                .session()
                .wait_until(WAIT, |snap| snap.rx_audio_fresh
                    && snap.model.trx(trx0()).audio.is_true()),
            "the SDR program never took Nexus's audio stream"
        );
        self.run_until("the loop never put the transmit tee in", |s| {
            s.state.dax_tee_id.is_some()
        });
    }

    /// The `trx` writes the scene's client sent, in order (its reads left out).
    fn trx_writes(&self) -> Vec<String> {
        self.sim
            .commands_from(0)
            .into_iter()
            .filter(|c| c.starts_with("trx:0,"))
            .collect()
    }

    /// What keyed the simulated radio, in order.
    fn keys(&self) -> Vec<(Keyer, SimSource)> {
        self.sim
            .log()
            .iter()
            .filter_map(|l| match l.event {
                SimEvent::Keyed { by, source, .. } => Some((by, source)),
                _ => None,
            })
            .collect()
    }

    /// Whether the scene's client asked the SDR program to transmit in any way: a key on any
    /// source, a tune, or anything of the program's own CW keyer.
    fn any_transmit_asked(&self) -> Vec<String> {
        self.sim
            .commands_from(0)
            .into_iter()
            .filter(|c| {
                c.starts_with("trx:0,true")
                    || c.starts_with("tune:0,true")
                    || c.starts_with("cw_")
                    || c.starts_with("keyer:")
            })
            .collect()
    }

    /// When the SDR program read `text` from the scene's client first, if it did.
    fn first_command_at(&self, text: &str) -> Option<Duration> {
        self.sim.log().iter().find_map(|l| match &l.event {
            SimEvent::Command {
                conn: 0, text: t, ..
            } if t == text => Some(l.at),
            _ => None,
        })
    }
}

/// The CW section with the Soundcard keyer at [`PITCH`] and [`WPM`], on [`DIAL_HZ`].
fn soundcard_cw(e: &mut Engine) {
    e.set_cw_keyer("soundcard", PITCH);
    e.set_operating_mode("cw", false);
    e.set_frequency(DIAL_HZ as f64 / 1e6, "20m", "CW");
    e.set_cw_wpm(WPM);
}

/// The transmit-audio frames the SDR program took from the scene's client, in order: when each
/// arrived, how many samples of channel 0 it carried, and at what rate. Its samples are the
/// matching run of `Simulator::tx_audio(0)`.
fn tx_frames(log: &[Logged]) -> Vec<(Duration, usize, u32)> {
    log.iter()
        .filter_map(|l| match &l.event {
            SimEvent::FrameIn {
                conn: 0,
                header: Some(h),
                bytes,
                ..
            } if h.kind == kind::TX_AUDIO => {
                let width = SampleType::from_code(h.format)?.bytes();
                let values = bytes.checked_sub(HEADER_LEN)? / width;
                Some((l.at, values / h.channels.max(1) as usize, h.sample_rate))
            }
            _ => None,
        })
        .collect()
}

/// The one rate every transmit frame was sent at.
fn tx_rate(frames: &[(Duration, usize, u32)]) -> u32 {
    let rate = frames.first().expect("no transmit audio").2;
    assert!(
        frames.iter().all(|f| f.2 == rate),
        "transmit frames at more than one rate"
    );
    rate
}

/// When the frame carrying sample `index` of the transmit audio arrived.
fn arrival_of(frames: &[(Duration, usize, u32)], index: usize) -> Duration {
    let mut end = 0;
    for (at, n, _) in frames {
        end += n;
        if index < end {
            return *at;
        }
    }
    panic!("sample {index} is past the {end} the frames carried")
}

/// How many samples of transmit audio arrived before `at`.
fn samples_before(frames: &[(Duration, usize, u32)], at: Duration) -> usize {
    frames.iter().take_while(|f| f.0 < at).map(|f| f.1).sum()
}

/// The tone's marks in `audio`: the first and one past the last sample of each run where it is up,
/// a sample over a quarter of the peak up and a run ending at a quiet stretch of 2 ms (several
/// cycles of the tone).
fn marks(audio: &[f32], rate: u32) -> Vec<(usize, usize)> {
    let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak == 0.0 {
        return Vec::new();
    }
    let quiet = rate as usize / 500;
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, s) in audio.iter().enumerate() {
        if s.abs() < peak / 4.0 {
            continue;
        }
        match out.last_mut() {
            Some((_, end)) if i - *end < quiet => *end = i + 1,
            _ => out.push((i, i + 1)),
        }
    }
    out
}

/// The keying a receiver hears in `audio`, word by word, in dits: `=` the tone up, `.` down, each
/// mark and gap rounded to whole dits. A gap of five dits or more ends a word. Fails if any mark or
/// gap inside a word is more than 10 ms from its whole number of dits: the keyer's own timing, as a
/// receiver measures it through the 5 ms shaped edges.
fn heard(audio: &[f32], rate: u32) -> Vec<String> {
    let ms = |n: usize| n as f64 * 1000.0 / f64::from(rate);
    let dit = dit_ms();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut last_end = None;
    for (start, end) in marks(audio, rate) {
        if let Some(prev) = last_end {
            let gap = ms(start - prev);
            let dits = (gap / dit).round();
            if dits >= 5.0 {
                words.push(std::mem::take(&mut word));
            } else {
                assert!(
                    (gap - dits * dit).abs() <= 10.0,
                    "a {gap:.1} ms gap is no whole number of {dit} ms dits"
                );
                word.push_str(&".".repeat(dits as usize));
            }
        }
        let mark = ms(end - start);
        let dits = (mark / dit).round();
        assert!(
            (mark - dits * dit).abs() <= 10.0,
            "a {mark:.1} ms element is no whole number of {dit} ms dits"
        );
        word.push_str(&"=".repeat(dits as usize));
        last_end = Some(end);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// The gaps between words in `audio`, in ms.
fn word_spaces(audio: &[f32], rate: u32) -> Vec<f64> {
    marks(audio, rate)
        .windows(2)
        .map(|w| (w[1].0 - w[0].1) as f64 * 1000.0 / f64::from(rate))
        .filter(|gap| *gap >= 5.0 * dit_ms())
        .collect()
}

/// The tone's frequency in `audio`, Hz: whole cycles between its first and last rising zero
/// crossing in each mark (each crossing placed between its two samples), their shaped edges (6 ms
/// each end) left out.
fn tone_hz(audio: &[f32], rate: u32) -> f64 {
    let edge = rate as usize * 6 / 1000;
    let (mut cycles, mut span) = (0usize, 0.0f64);
    for (start, end) in marks(audio, rate) {
        if end - start <= 2 * edge {
            continue;
        }
        let rising: Vec<f64> = (start + edge..end - edge - 1)
            .filter(|&i| audio[i] < 0.0 && audio[i + 1] >= 0.0)
            .map(|i| i as f64 + f64::from(-audio[i] / (audio[i + 1] - audio[i])))
            .collect();
        if let [first, .., last] = rising[..] {
            cycles += rising.len() - 1;
            span += last - first;
        }
    }
    assert!(cycles > 0, "no tone long enough to measure");
    cycles as f64 * f64::from(rate) / span
}

/// ⭐ A SOUNDCARD CW MACRO GOES TO THE SDR PROGRAM AS NEXUS'S OWN AUDIO, IN DIGU, AND THE TRANSMITTER
/// DROPS AFTER ITS LAST ELEMENT. With Audio over TCI on, the F-key's two words key the program once,
/// `trx:0,true,tci`, and unkey it once, `trx:0,false`, in that order and nothing else on `trx`. The
/// program is in DIGU on the dial, and the audio it pulls is the macro as a receiver hears it: a
/// 700 Hz tone (so the signal is on the dial plus the pitch), every element and gap of both words
/// its whole number of dits at 25 WPM, a word space between them. The unkey comes after the last
/// element has been pulled, within the word space and the PTT tail after it. None of it is played
/// on the sound card.
#[test]
fn a_soundcard_cw_macro_goes_to_the_sdr_program_as_nexus_audio_in_digu_and_unkeys_after_it() {
    let mut s = Scene::new(soundcard_cw);
    s.routed();
    s.engine().send_cw(MACRO);
    s.run_until("the macro never unkeyed", |s| {
        s.trx_writes().iter().any(|c| c == "trx:0,false")
    });
    s.run(300);
    assert_eq!(
        s.trx_writes(),
        ["trx:0,true,tci", "trx:0,false"],
        "the keys on the wire"
    );
    assert_eq!(
        s.keys(),
        [(Keyer::Client(0), SimSource::Tci)],
        "keyed on Nexus's audio"
    );
    let radio = s.sim.radio();
    assert_eq!(radio.trx[0].modulation, "DIGU", "the program's mode");
    assert_eq!(radio.trx[0].vfo[0], DIAL_HZ, "the program's dial");
    let log = s.sim.log();
    let frames = tx_frames(&log);
    let rate = tx_rate(&frames);
    let audio = s.sim.tx_audio(0);
    assert_eq!(heard(&audio, rate), MACRO_HEARD, "the macro as heard");
    let tone = tone_hz(&audio, rate);
    assert!(
        (tone - f64::from(PITCH)).abs() < 3.0,
        "the tone: {tone:.1} Hz, so the signal is at {:.0} Hz",
        DIAL_HZ as f64 + tone
    );
    // The word space is the loop's pacing (the keyer's seven dits, on the wall clock, a 20 ms step
    // at a time), so it is bounded only as a receiver needs it: one gap long enough to be heard as
    // a word space, never a character's gap (`word_spaces`), and not a second long.
    let spaces = word_spaces(&audio, rate);
    assert!(
        spaces.len() == 1 && spaces[0] < 1_000.0,
        "the word space: {spaces:?} ms"
    );
    let last = marks(&audio, rate).last().expect("the tone").1 - 1;
    let last_at = arrival_of(&frames, last);
    let unkey_at = s.first_command_at("trx:0,false").expect("the unkey");
    assert!(
        unkey_at > last_at,
        "the unkey ({unkey_at:?}) before the last element was pulled ({last_at:?})"
    );
    let bound =
        Duration::from_secs_f64((7.0 * dit_ms() + crate::slot::TX_TAIL_MS + 400.0) / 1000.0);
    assert!(
        unkey_at - last_at <= bound,
        "the unkey {:?} after the last element, more than its word space and tail allow",
        unkey_at - last_at
    );
    assert!(
        s.card.played.lock().unwrap().is_empty(),
        "played on the sound card as well"
    );
}

/// ⭐ WITH AUDIO OVER TCI OFF, A SOUNDCARD CW MACRO KEYS PLAINLY AND PLAYS ON THE SOUND CARD (the
/// operator routes the card to the program's input): one `trx:0,true` naming no source, the
/// program's own input keyed, still in DIGU, one unkey. The sound card is handed exactly the
/// keyer's two words at the modem rate, which a receiver hears as the macro on a 700 Hz tone, and
/// nothing of it reaches the program over TCI, whose audio stream is never asked for.
#[test]
fn with_audio_over_tci_off_a_soundcard_cw_macro_keys_plainly_and_plays_on_the_sound_card() {
    let mut s = Scene::new(|e| {
        let mut settings = e.settings().clone();
        settings.tci_audio = false;
        e.apply_settings(settings);
        soundcard_cw(e);
    });
    assert!(
        s.daemon().session().wait_until(WAIT, |snap| snap.phase
            == tempo_net::tci::session::Phase::Ready),
        "premise: the SDR program is ready"
    );
    s.run(300);
    s.engine().send_cw(MACRO);
    s.run_until("the macro never unkeyed", |s| {
        s.trx_writes().iter().any(|c| c == "trx:0,false")
    });
    s.run(300);
    assert_eq!(
        s.trx_writes(),
        ["trx:0,true", "trx:0,false"],
        "the keys on the wire"
    );
    assert_eq!(
        s.keys(),
        [(Keyer::Client(0), SimSource::Mic(Mic::Selected))],
        "the program's own input"
    );
    assert_eq!(
        s.sim.radio().trx[0].modulation,
        "DIGU",
        "the program's mode"
    );
    let played = s.card.played.lock().unwrap().clone();
    let rate = tempo_fast::SAMPLE_RATE as u32;
    let words: Vec<Vec<f32>> = MACRO
        .split_whitespace()
        .map(|w| tempo_core::cw::morse_samples(w, WPM, PITCH, rate))
        .collect();
    assert!(
        played == words.concat(),
        "the sound card was not handed the macro"
    );
    // The card here keeps each word as it is handed over, so the two abut in what it kept; on a
    // real card the loop's wait between them is the word space. Each word, as a receiver hears it:
    let (first, second) = played.split_at(words[0].len());
    for (word, want) in [first, second].into_iter().zip(MACRO_HEARD) {
        assert_eq!(heard(word, rate), [want], "the word as heard");
        let tone = tone_hz(word, rate);
        assert!(
            (tone - f64::from(PITCH)).abs() < 3.0,
            "the tone: {tone:.1} Hz"
        );
    }
    assert!(
        !s.sim
            .commands_from(0)
            .iter()
            .any(|c| c.starts_with("audio_start")),
        "the audio stream asked for"
    );
    assert!(s.sim.tx_audio(0).is_empty(), "audio sent over TCI");
}

/// ⛔ THE CAT KEYER ON A TCI RADIO KEYS NOTHING, AND THE CW SCREEN SAYS THE RADIO DID NOT TAKE IT.
/// The TCI client does not drive the SDR program's own CW keyer, so its rigctld answers the radio's
/// keyer (`b`) as a backend that has none. The program is put in CW; the F-key's words reach no key,
/// no tune, nothing of the program's keyer and no audio, on the wire or on the sound card; and the
/// CW screen says the radio did not accept CAT CW keying and names the keyers that do.
#[test]
fn the_cat_keyer_on_a_tci_radio_keys_nothing_and_the_cw_screen_says_why() {
    let mut s = Scene::new(|e| {
        soundcard_cw(e);
        e.set_cw_keyer("cat", PITCH);
    });
    s.routed();
    s.engine().send_cw(MACRO);
    // Both words tried: the second a word's length and a word space after the first.
    s.run(3_000);
    assert_eq!(s.sim.radio().trx[0].modulation, "CW", "the program's mode");
    assert_eq!(
        s.any_transmit_asked(),
        Vec::<String>::new(),
        "the CAT keyer asked the SDR program to transmit"
    );
    assert_eq!(s.keys(), [], "the radio keyed");
    assert!(s.sim.tx_audio(0).is_empty(), "audio sent over TCI");
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
    let line = s.engine().cw_keyer_error().unwrap_or_default();
    assert!(
        line.starts_with("Your rig didn't accept CAT CW keying (Hamlib send_morse) — ")
            && line.ends_with(
                ". Use the WinKeyer keyer, or the Soundcard keyer (which needs Nexus's audio \
                 routed to the rig)."
            ),
        "the CW screen: {line:?}"
    );
}

/// ⛔ ESC MID-CHARACTER ENDS A SOUNDCARD CW OVER ON TCI ON THE LOOP'S NEXT TICK, WITH NOTHING OF IT
/// LEFT QUEUED. Esc is the CW screen's `stop_cw` then `halt_tx`. Pressed part way through the first
/// element of the first character, one tick of the loop empties the transmit route, drops the
/// over's hold, and sends the unkey, which reaches the program with no further tick. What the
/// program pulled after it is silence, and the rest of the macro never keys, not even at the
/// operator's next TX On.
#[test]
fn esc_mid_character_ends_a_soundcard_cw_over_on_tci_within_a_tick_with_nothing_queued() {
    let mut s = Scene::new(soundcard_cw);
    s.routed();
    s.engine().send_cw(MACRO);
    s.run_until("none of the macro's audio reached the SDR program", |s| {
        s.sim.tx_audio(0).iter().any(|v| *v != 0.0)
    });
    s.run(100);
    let audio = s.sim.tx_audio(0);
    let rate = tx_rate(&tx_frames(&s.sim.log()));
    assert_eq!(
        marks(&audio, rate).len(),
        1,
        "premise: still in the first element of the first character"
    );
    assert!(s.route().queued() > 0, "premise: the word still going out");
    {
        let mut e = s.engine();
        e.stop_cw();
        e.halt_tx();
    }
    s.step();
    assert_eq!(s.route().queued(), 0, "the rest of the word left queued");
    assert_eq!(s.state.tx_until_ms, None, "the over's hold left up");
    assert!(
        s.sim
            .wait_for(Duration::from_secs(1), |log| log.iter().any(|l| matches!(
                &l.event,
                SimEvent::Command { conn: 0, text, .. } if text == "trx:0,false"
            ))),
        "the unkey did not leave on the tick after Esc: {:?}",
        s.trx_writes()
    );
    let stop = s.first_command_at("trx:0,false").expect("the unkey");
    s.run(1_500);
    let frames = tx_frames(&s.sim.log());
    let before = samples_before(&frames, stop);
    let audio = s.sim.tx_audio(0);
    assert!(
        audio[before..].iter().all(|v| *v == 0.0),
        "the over's audio after the unkey"
    );
    // The rest of the macro was dropped, not held: the operator's next TX On keys none of it.
    s.engine().set_tx_enabled_by_operator(true);
    s.run(1_000);
    assert_eq!(
        s.trx_writes()
            .iter()
            .filter(|c| c.starts_with("trx:0,true"))
            .count(),
        1,
        "keyed again after Esc: {:?}",
        s.trx_writes()
    );
}

/// ⛔ THE LICENCE PRIVILEGES STILL KEEP SOUNDCARD CW OFF THE SDR PROGRAM. A General on 14.020 MHz
/// (an Extra's CW): the F-key's macro reaches no key and no audio, on the wire or the sound card;
/// it is dropped, not held, so the Extra privileges that would have covered it key nothing later;
/// and the CW screen says why. CONTROL: an Extra on the same dial keys it.
#[test]
fn the_licence_privileges_keep_soundcard_cw_off_the_sdr_program() {
    let outside = |e: &mut Engine| {
        soundcard_cw(e);
        e.set_license_class("general");
        e.set_frequency(14.020, "20m", "CW");
    };
    let mut s = Scene::new(outside);
    s.routed();
    s.engine().send_cw(MACRO);
    s.run(1_000);
    assert_eq!(
        s.any_transmit_asked(),
        Vec::<String>::new(),
        "keyed outside the licence"
    );
    assert!(s.sim.tx_audio(0).is_empty(), "audio sent over TCI");
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
    assert_eq!(
        s.engine().cw_keyer_error().as_deref(),
        Some(OUTSIDE_PRIVILEGES)
    );
    s.engine().set_license_class("extra");
    s.run(1_000);
    assert_eq!(
        s.any_transmit_asked(),
        Vec::<String>::new(),
        "the refused macro was held and keyed later"
    );
    // CONTROL: the same dial inside the privileges.
    let mut s = Scene::new(|e| {
        outside(e);
        e.set_license_class("extra");
    });
    s.routed();
    s.engine().send_cw(MACRO);
    s.run_until("control: the macro never keyed", |s| !s.keys().is_empty());
    assert_eq!(s.trx_writes()[0], "trx:0,true,tci", "control");
}

/// ⛔ THE TX LATCH STILL KEEPS SOUNDCARD CW OFF THE SDR PROGRAM. The F-key arms TX and queues its
/// macro, and before the loop's next tick the watchdog trips, which lowers the latch and leaves the
/// queue as it was: no word reaches a key or the program, the macro is dropped rather than held for
/// the operator's next TX On, and the CW screen says why. CONTROL: the same macro with the latch up
/// keys.
#[test]
fn the_tx_latch_keeps_soundcard_cw_off_the_sdr_program() {
    let mut s = Scene::new(soundcard_cw);
    s.routed();
    {
        let mut e = s.engine();
        e.send_cw(MACRO);
        e.trip_tx_watchdog_for_test();
    }
    s.run(1_000);
    assert_eq!(
        s.any_transmit_asked(),
        Vec::<String>::new(),
        "keyed with TX off"
    );
    assert!(s.sim.tx_audio(0).is_empty(), "audio sent over TCI");
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
    assert_eq!(s.engine().cw_keyer_error().as_deref(), Some(TX_WAS_OFF));
    s.engine().set_tx_enabled_by_operator(true);
    s.run(1_000);
    assert_eq!(
        s.any_transmit_asked(),
        Vec::<String>::new(),
        "the refused macro was held and keyed at TX On"
    );
    // CONTROL: the latch left up.
    let mut s = Scene::new(soundcard_cw);
    s.routed();
    s.engine().send_cw(MACRO);
    s.run_until("control: the macro never keyed", |s| !s.keys().is_empty());
    assert_eq!(s.trx_writes()[0], "trx:0,true,tci", "control");
}

/// ⭐ NEVER A SILENT CW OVER ON TCI: the SDR program stops pulling Nexus's audio part way through
/// the first word of a soundcard CW macro, and stays keyed. The client ends the over, and the loop
/// halts transmit with the reason on the CAT line: TX goes off, the over's hold goes, nothing of it
/// is left queued, and none of it goes to the sound card. The rest of the macro is dropped, not
/// held: the operator's next TX On keys none of it.
#[test]
fn a_soundcard_cw_over_the_sdr_program_stops_pulling_is_ended_and_says_why() {
    let mut s = Scene::on(
        [Fault::ChronoStall {
            after: Duration::from_millis(400),
        }],
        patient(),
        soundcard_cw,
    );
    s.routed();
    s.engine().send_cw(MACRO);
    s.run_until("the halt never reached the engine", |s| {
        s.engine().snapshot().radio.cat_detail
            == "TCI: the SDR program stopped taking Nexus's audio part way through the over, so \
                Nexus ended it."
    });
    s.run_until("the over's hold never went", |s| {
        s.state.tx_until_ms.is_none()
    });
    assert!(!s.engine().tx_enabled(), "TX is still on");
    assert!(
        s.trx_writes().iter().any(|c| c == "trx:0,false"),
        "no stop: {:?}",
        s.trx_writes()
    );
    assert_eq!(s.route().queued(), 0, "the rest of the word left queued");
    s.engine().set_tx_enabled_by_operator(true);
    s.run(1_500);
    assert_eq!(
        s.trx_writes()
            .iter()
            .filter(|c| c.starts_with("trx:0,true"))
            .count(),
        1,
        "keyed again: {:?}",
        s.trx_writes()
    );
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
}

/// ⭐ A TCI CONNECTION LOST PART WAY THROUGH A SOUNDCARD CW MACRO ENDS IT. The program's connection
/// drops in the first word. The loop takes the transmit tee out and builds a new session, whose
/// first command is the unkey; the rest of the macro is dropped with the old session, so none of it
/// keys on the new one, and none of it goes to the sound card in between.
#[test]
fn a_tci_connection_lost_part_way_through_a_soundcard_cw_macro_ends_it() {
    let mut s = Scene::on(
        [Fault::DisconnectMidOver {
            after: Duration::from_millis(400),
            radio_stays_keyed: false,
        }],
        TciOptions::default(),
        soundcard_cw,
    );
    let key = s.daemon().key();
    s.reopen_client = true;
    s.routed();
    s.engine().send_cw(MACRO);
    s.run_until("the macro never keyed", |s| !s.keys().is_empty());
    s.run_until("no next session", |s| !s.sim.commands_from(1).is_empty());
    s.run_until("the over's hold never went", |s| {
        s.state.tx_until_ms.is_none()
    });
    assert_eq!(
        s.sim.commands_from(1)[0],
        "trx:0,false",
        "the next session's first command"
    );
    s.run(2_000);
    assert!(
        !s.sim
            .commands_from(1)
            .iter()
            .any(|c| c.starts_with("trx:0,true")),
        "the macro came back on the next session: {:?}",
        s.sim.commands_from(1)
    );
    assert!(s.card.played.lock().unwrap().is_empty(), "played");
    registry::forget(key);
}
