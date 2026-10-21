//! Nexus's spots on a TCI radio's own panadapter, in the radio loop: the loop offers the engine's
//! spots to the active radio's client every tick, so they reach the SDR program only while the
//! radio's switch is on; and a click on one of them in the SDR program comes back through the
//! client to the loop, which hands it to `Engine::tci_spot_clicked`: the dial moves, and nothing
//! else does, and nothing moves while anything is on the air.
//!
//! The SDR program is the TCI simulator (`tempo-tcisim`) on loopback; the loop is the real
//! `RadioLoop::step`, driving the real `Rig` into the client's rigctld shim, as `run_radio` does.
//! The client's registry is process-wide and the simulators listen on ephemeral ports another
//! test's simulator may be given, so the scene forgets its server's entry first.

use super::*;
use crate::tci::{registry, Options as TciOptions, TciDaemon};
use tempo_app::engine::tci_spots::PanadapterSpot;
use tempo_net::tci::Trx;
use tempo_tcisim::{Config as SimConfig, Dialect as SimDialect, Event as SimEvent, Simulator};

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

/// The radio loop on a TCI radio, its client on the simulated SDR program.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    card: Card,
    sim: Simulator,
}

impl Scene {
    /// The scene, the engine set up by `setup` after the radio is put on the TCI connection with
    /// its spots switch `spots`.
    fn new(spots: bool, setup: impl FnOnce(&mut Engine)) -> Scene {
        let sim = Simulator::start(SimConfig::dialect(SimDialect::expert_sdr3()))
            .expect("the simulator starts");
        registry::forget((sim.addr(), Trx::new(0).expect("trx 0")));
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            let mut s = e.settings().clone();
            s.rig_model = 7;
            s.rig_conn = "tci".into();
            s.tci_host = "127.0.0.1".into();
            s.tci_port = sim.addr().port();
            s.tci_audio = false;
            s.tci_spots = spots;
            s.ptt_method = "cat".into();
            e.apply_settings(s);
            setup(&mut e);
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
        let cfg = RadioConfig {
            rig_model: 7,
            ..RadioConfig::default()
        };
        let state = RadioLoop::new(applied, Some(CatDaemon::Tci(daemon)), &cfg);
        Scene {
            engine,
            state,
            rig,
            card: Card,
            sim,
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

    fn engine(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap()
    }

    /// The calls on the SDR program's panadapter.
    fn on_panadapter(&self) -> Vec<String> {
        self.sim.radio().spots.into_iter().map(|s| s.call).collect()
    }

    /// The radio's spots switch, as Settings saves it.
    fn set_spots(&self, on: bool) {
        let mut e = self.engine();
        let mut s = e.settings().clone();
        s.tci_spots = on;
        e.apply_settings(s);
    }

    /// Whether anything keyed the simulated radio.
    fn keyed(&self) -> bool {
        self.sim
            .log()
            .iter()
            .any(|l| matches!(l.event, SimEvent::Keyed { .. }))
    }
}

/// Seconds since the Unix epoch.
fn now_unix() -> i64 {
    i64::try_from(now_unix_ms() as u64 / 1_000).expect("a time")
}

/// The station's needed spots: K1ABC on 20 m CW and VP2V/K1ABC on 40 m CW, heard now.
fn needed() -> Vec<PanadapterSpot> {
    let spot = |call: &str, hz: i64| PanadapterSpot {
        call: call.into(),
        hz,
        mode: "CW".into(),
        argb: 0xFFF2_3EC0,
        text: "New one".into(),
        heard_unix: now_unix(),
    };
    vec![spot("K1ABC", 14_025_000), spot("VP2V/K1ABC", 7_030_000)]
}

/// ⭐ THE STATION'S SPOTS REACH THE SDR PROGRAM'S PANADAPTER ONLY WHILE THE RADIO'S SWITCH IS ON:
/// off, the loop offers nothing and nothing is sent; on, `spot_clear;` and the spots; off again,
/// they come off.
#[test]
fn the_stations_spots_reach_the_panadapter_only_while_the_switch_is_on() {
    let mut s = Scene::new(false, |e| e.set_tci_spots(needed()));
    s.run(600);
    assert!(
        !s.sim.commands_from(0).iter().any(|c| c.starts_with("spot")),
        "off: nothing sent"
    );
    s.set_spots(true);
    s.run_until("the spots never reached the panadapter", |s| {
        s.on_panadapter() == ["K1ABC", "VP2V/K1ABC"]
    });
    s.set_spots(false);
    s.run_until("the switch off never took them off", |s| {
        s.on_panadapter().is_empty()
    });
    assert!(!s.keyed());
}

type Setup = fn(&mut Engine);

/// The states a click can find the station in, none of them on the air.
fn states() -> Vec<(&'static str, Setup)> {
    vec![
        ("launch, TX off", |_| {}),
        ("Phone, armed", |e| {
            e.set_operating_mode("phone", true);
            e.set_frequency(14.250, "20m", "USB");
        }),
        ("after Stop TX", |e| {
            e.set_operating_mode("phone", true);
            e.set_frequency(14.250, "20m", "USB");
            e.halt_tx();
        }),
        ("CW, armed", |e| {
            e.set_operating_mode("cw", true);
            e.set_frequency(14.030, "20m", "CW");
        }),
    ]
}

/// What a click must not touch, as the engine shows it.
#[derive(Debug, Clone, PartialEq)]
struct Untouched {
    tx_enabled: bool,
    tripped: bool,
    qso: Option<String>,
    section: tempo_app::settings::OperatingMode,
    work_tick: u64,
}

fn untouched(e: &Engine) -> Untouched {
    let snap = e.snapshot();
    Untouched {
        tx_enabled: e.tx_enabled(),
        tripped: snap.radio.tx_watchdog,
        qso: e.qso_dxcall().map(str::to_string),
        section: e.settings().operating_mode,
        work_tick: snap.work_tick,
    }
}

/// ⛔ A CLICK ON ONE OF OUR SPOTS IN THE SDR PROGRAM TUNES AND NEVER TRANSMITS, THROUGH THE WHOLE
/// LOOP: from each state, a click on the 20 m spot and one on the 40 m spot each move the dial to
/// the spot, and nothing else: TX is never armed (a band change may lower it), no QSO starts, the
/// section stays, the window is not asked to open a cockpit, the watchdog is untouched, and nothing
/// keys the radio. After Stop TX the hold stands: a section entry after the click arms nothing.
/// CONTROL: every click moved the dial to its spot; and the hold's probe, with no Stop TX, arms.
#[test]
fn a_click_on_our_spot_in_the_sdr_panadapter_tunes_the_loop_and_never_transmits() {
    let mut moved = Vec::new();
    let mut tuned = 0;
    for (state, setup) in states() {
        for (call, hz) in [("K1ABC", 14_025_000), ("VP2V/K1ABC", 7_030_000)] {
            let mut s = Scene::new(true, |e| {
                setup(e);
                e.set_tci_spots(needed());
            });
            s.run_until("our spots never reached the panadapter", |s| {
                s.on_panadapter().len() == 2
            });
            let before = untouched(&s.engine());
            s.sim.click_spot(call, hz as u64);
            let what = format!("{state}: {call} at {hz} Hz");
            let deadline = Instant::now() + WAIT;
            while (s.engine().settings().dial_mhz - hz as f64 / 1e6).abs() > 1e-9
                && Instant::now() < deadline
            {
                s.run(20);
            }
            s.run(300);
            let mut after = untouched(&s.engine());
            if (s.engine().settings().dial_mhz - hz as f64 / 1e6).abs() < 1e-9 {
                tuned += 1;
            }
            // A band change may lower TX; it may never raise it.
            if before.tx_enabled && !after.tx_enabled {
                after.tx_enabled = true;
            }
            if after != before {
                moved.push(format!("{what}: {before:?} -> {after:?}"));
            }
            if s.keyed() {
                moved.push(format!("{what}: the radio keyed"));
            }
            if state == "after Stop TX" {
                let mut e = s.engine();
                e.set_operating_mode("cw", true);
                if e.tx_enabled() {
                    moved.push(format!("{what}: the Stop's hold was cleared"));
                }
            }
        }
    }
    assert!(
        moved.is_empty(),
        "a click moved them:\n{}",
        moved.join("\n")
    );
    assert_eq!(tuned, states().len() * 2, "control: every click tuned");
    // CONTROL for the hold's probe: with nothing holding TX (launch, TX off), the same entry arms.
    let mut probe = Engine::new("W9XYZ", "EN37", 0);
    probe.set_license_class("extra");
    assert!(!probe.tx_enabled(), "premise: TX off at launch");
    probe.set_operating_mode("cw", true);
    assert!(
        probe.tx_enabled(),
        "control: with nothing holding TX, the section entry arms"
    );
}

/// ⛔ A CLICK DURING A RADIO SWITCH THE HANDOFF HAS NOT SEEN IS NOT TAKEN: while the engine names
/// another radio than the one the loop's link was handed, the link is the radio being left, so the
/// loop hands its clicks to nobody, and a click it could not take is dropped once it is stale
/// rather than tuning the dial late. CONTROL: on its radio again, a new click tunes.
#[test]
fn a_click_during_a_switch_the_handoff_has_not_seen_is_not_taken() {
    let mut s = Scene::new(true, |e| {
        e.set_operating_mode("phone", true);
        e.set_frequency(14.250, "20m", "USB");
        e.set_tci_spots(needed());
    });
    s.run_until("our spots never reached the panadapter", |s| {
        s.on_panadapter().len() == 2
    });
    let active = s.engine().settings().active_radio;
    s.state.handed_over = Some(active + 1);
    s.step();
    assert!(
        s.state.switch_unhanded,
        "scene: the loop reads a switch it was not handed"
    );
    s.sim.click_spot("K1ABC", 14_025_000);
    s.run(1_000 + crate::tci::spots::CLICK_KEPT_FOR.as_millis() as u64);
    assert!(
        (s.engine().settings().dial_mhz - 14.250).abs() < 1e-9,
        "the click tuned during the switch: {}",
        s.engine().settings().dial_mhz
    );
    s.state.handed_over = Some(active);
    s.run(500);
    assert!(
        (s.engine().settings().dial_mhz - 14.250).abs() < 1e-9,
        "the stale click tuned once the loop was back: {}",
        s.engine().settings().dial_mhz
    );
    s.sim.click_spot("K1ABC", 14_025_000);
    s.run_until("control: on its radio again, the click never tuned", |s| {
        (s.engine().settings().dial_mhz - 14.025).abs() < 1e-9
    });
}

/// ⛔ NOT WHILE ANYTHING IS ON THE AIR, END TO END: with the operator's PTT keying the SDR program
/// through the loop, a click on our spot leaves the dial where it is. CONTROL: once PTT is released,
/// the same click tunes.
#[test]
fn a_click_while_on_the_air_moves_nothing_and_after_it_tunes() {
    let mut s = Scene::new(true, |e| {
        e.set_operating_mode("phone", true);
        e.set_frequency(14.250, "20m", "USB");
        e.set_tci_spots(needed());
    });
    s.run_until("our spots never reached the panadapter", |s| {
        s.on_panadapter().len() == 2
    });
    s.engine().set_ptt(true);
    s.run_until("the operator's PTT never keyed the SDR program", |s| {
        s.keyed()
    });
    assert!(s.engine().on_air(), "premise: the station is on the air");
    s.sim.click_spot("K1ABC", 14_025_000);
    s.run(1_000);
    assert!(
        (s.engine().settings().dial_mhz - 14.250).abs() < 1e-9,
        "on the air, the dial moved: {}",
        s.engine().settings().dial_mhz
    );
    s.engine().set_ptt(false);
    s.run_until("the unkey never reached the SDR program", |s| {
        s.sim.radio().trx[0].key.is_none()
    });
    s.run_until("the station never came off the air", |s| {
        !s.engine().on_air()
    });
    s.sim.click_spot("K1ABC", 14_025_000);
    s.run_until("control: off the air, the click never tuned", |s| {
        (s.engine().settings().dial_mhz - 14.025).abs() < 1e-9
    });
}
