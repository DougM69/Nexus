//! A TCI radio's panadapter in the radio loop: the loop renews the active radio's client's word
//! every tick from the radio's switch (`tci_pan`), so the SDR program's IQ is drawn on the loop's
//! spectrum feed while the switch is on and stopped once it goes off, and a switch the handoff has
//! not seen renews nothing, so the client being left stops drawing.
//!
//! The SDR program is the TCI simulator (`tempo-tcisim`) on loopback; the loop is the real
//! `RadioLoop::step`, driving the real `Rig` into the client's rigctld shim, as `run_radio` does.
//! The client's registry is process-wide and the simulators listen on ephemeral ports another
//! test's simulator may be given, so the scene forgets its server's entry first.

use super::*;
use crate::tci::{registry, Options as TciOptions, TciDaemon};
use tempo_net::tci::Trx;
use tempo_tcisim::{Config as SimConfig, Dialect as SimDialect, Simulator};

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(10);

/// The scene's sound card: silence in, nothing kept out.
#[derive(Clone, Default)]
struct Card;

impl AudioBackend for Card {
    fn capture(&mut self) -> Vec<f32> {
        vec![0.0; 240]
    }
    fn play(&mut self, _samples: &[f32]) {}
}

/// The radio loop on a TCI radio, its client on the simulated SDR program, its spectrum feed the
/// one the scope reads.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    card: Card,
    sim: Simulator,
    feed: tempo_app::engine::SpectrumFeed,
}

impl Scene {
    fn new() -> Scene {
        let sim = Simulator::start(SimConfig::dialect(SimDialect::expert_sdr3()))
            .expect("the simulator starts");
        registry::forget((sim.addr(), Trx::new(0).expect("trx 0")));
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut e = engine.lock().unwrap();
            let mut s = e.settings().clone();
            s.rig_model = 7;
            s.rig_conn = "tci".into();
            s.tci_host = "127.0.0.1".into();
            s.tci_port = sim.addr().port();
            s.ptt_method = "cat".into();
            e.apply_settings(s);
        }
        // The transport the loop compares its settings against: the same settings, so it never
        // rebuilds the link.
        let applied = Transport::from_settings(engine.lock().unwrap().settings());
        let mut target = applied.tci_target().expect("a TCI radio");
        target.rigctld_port = 0;
        let daemon =
            TciDaemon::start_receiving(&target, TciOptions::default()).expect("the client starts");
        let mut rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
        rig.set_slow_transport(true);
        let feed = tempo_app::engine::SpectrumFeed::default();
        let cfg = RadioConfig {
            rig_model: 7,
            spectrum_feed: feed.clone(),
            ..RadioConfig::default()
        };
        let state = RadioLoop::new(applied, Some(CatDaemon::Tci(daemon)), &cfg);
        Scene {
            engine,
            state,
            rig,
            card: Card,
            sim,
            feed,
        }
    }

    fn step(&mut self) {
        let card = self.card.clone();
        let mut reopen_audio = move |_t: &Transport| Ok::<_, String>(card.clone());
        let mut reopen_rig = |_t: &Transport, _coexist: bool| -> RigOpen {
            panic!("the loop rebuilt its transport: the client under test is gone")
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

    fn daemon(&self) -> &TciDaemon {
        self.state
            .rigctld_proc
            .as_ref()
            .and_then(CatDaemon::tci)
            .expect("the TCI client")
    }

    /// The radio's panadapter switch, as Settings saves it.
    fn set_pan(&self, on: bool) {
        let mut e = self.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.tci_pan = on;
        e.apply_settings(s);
    }

    fn sent(&self, text: &str) -> usize {
        self.sim
            .commands_from(0)
            .iter()
            .filter(|c| *c == text)
            .count()
    }

    /// The source of the frame the RF scope pane would draw; empty when the RF slot holds none.
    fn rf_source(&self) -> String {
        self.feed
            .rf_frame_after(0)
            .map(|f| f.source)
            .unwrap_or_default()
    }
}

/// ⭐ THE LOOP DRAWS A TCI RADIO'S PANADAPTER ONLY WHILE ITS SWITCH IS ON: off as it comes, nothing
/// is asked of the SDR program; switched on, `iq_start` goes out and the program's IQ reaches the
/// loop's spectrum feed as TCI rows; switched off, `iq_stop` goes out and the RF slot empties.
#[test]
fn the_loop_draws_a_tci_radios_panadapter_only_while_its_switch_is_on() {
    let mut s = Scene::new();
    s.run(500);
    assert_eq!(s.sent("iq_start:0"), 0, "off as it comes: nothing asked");
    assert!(s.rf_source().is_empty());
    s.set_pan(true);
    s.run_until("the panadapter never drew", |s| {
        s.rf_source() == crate::tci::iq::SOURCE
    });
    assert_eq!(s.sent("iq_start:0"), 1);
    s.set_pan(false);
    s.run_until("the IQ never stopped", |s| s.sent("iq_stop:0") == 1);
    s.run_until("the RF slot never emptied", |s| s.rf_source().is_empty());
    let rows = s.daemon().pan().status().rows;
    s.run(300);
    assert_eq!(
        s.daemon().pan().status().rows,
        rows,
        "nothing drawn once off"
    );
}

/// ⭐ A SWITCH THE HANDOFF HAS NOT SEEN RENEWS NOTHING: while the engine names another radio than
/// the one the loop's link was handed (W0), the link is still the radio being left, so its client's
/// word is not renewed and it stops drawing; once the loop is on its radio again, it draws again.
#[test]
fn a_switch_the_handoff_has_not_seen_renews_no_panadapter() {
    let mut s = Scene::new();
    s.set_pan(true);
    s.run_until("the panadapter never drew", |s| {
        s.daemon().pan().status().rows >= 3
    });
    let active = s.engine.lock().unwrap().settings().active_radio;
    s.state.handed_over = Some(active + 1);
    s.step();
    assert!(
        s.state.switch_unhanded,
        "scene: the loop reads a switch it was not handed"
    );
    s.run_until("the panadapter kept drawing", |s| {
        !s.daemon().pan().status().drawing
    });
    let rows = s.daemon().pan().status().rows;
    s.run(300);
    assert_eq!(
        s.daemon().pan().status().rows,
        rows,
        "no row while unrenewed"
    );
    s.state.handed_over = Some(active);
    s.run_until("the panadapter never drew again", |s| {
        s.daemon().pan().status().rows > rows
    });
}
