//! Nexus's spots on the SDR program's panadapter: what the spots' plan sends for an offer (pure,
//! on a made-up clock), and what reaches the simulated SDR program (`tempo-tcisim`) over a real
//! WebSocket, read back from the simulator's own log and its panadapter. The registry is
//! process-wide and the simulators listen on ephemeral ports another test's simulator may be given,
//! so each test forgets its server's entry before its daemon.

use std::time::Duration;

use tempo_net::tci::session::NoStreams;
use tempo_net::tci::Trx;
use tempo_tcisim::{Config as SimConfig, Dialect as SimDialect, Event as SimEvent, Simulator};

use super::*;
use crate::tci::{registry, Options, Target};

/// The longest any one expected thing may take here.
const WAIT: Duration = Duration::from_secs(10);

/// The Needed board's colour for a new one.
const MAGENTA: u32 = 0xFFF2_3EC0;

fn spot(call: &str, hz: i64, heard_unix: i64) -> PanadapterSpot {
    PanadapterSpot {
        call: call.into(),
        hz,
        mode: "CW".into(),
        argb: MAGENTA,
        text: "New one".into(),
        heard_unix,
    }
}

/// The `n`th of a run of callsigns with the prefix letter `p`: `<p>1AAA`, `<p>1AAB`, …
fn nth(p: char, n: i64) -> String {
    let letter = |k: i64| char::from(b'A' + u8::try_from(k % 26).expect("below 26"));
    format!("{p}1{}{}{}", letter(n / 676), letter(n / 26), letter(n))
}

fn offer(generation: u64, spots: Vec<PanadapterSpot>) -> PanadapterSpots {
    PanadapterSpots {
        generation,
        spots: spots.into(),
    }
}

/// The wire text of each op, as the codec renders it in the specification's forms.
fn texts(ops: &[Op]) -> Vec<String> {
    texts_in(ops, Forms::Spec)
}

/// The wire text of each op in `forms`.
fn texts_in(ops: &[Op], forms: Forms) -> Vec<String> {
    ops.iter()
        .map(|op| op.command(forms).render().as_str().to_string())
        .collect()
}

// ── The plan, on a made-up clock ─────────────────────────────────────────────────────────────

/// A state with `offer` made and renewed at `at`, ours `shown`, and `spot_clear;` already sent
/// when `cleared`.
fn state(offer: Option<PanadapterSpots>, at: Instant, shown: &[Up], cleared: bool) -> State {
    State {
        offer,
        renewed: Some(at),
        shown: shown
            .iter()
            .map(|u| {
                (
                    u.call.as_str().to_string(),
                    Shown {
                        up: u.clone(),
                        since: at,
                    },
                )
            })
            .collect(),
        cleared,
        ..State::default()
    }
}

fn up(call: &str, hz: i64) -> Up {
    Up::of(&spot(call, hz, 0)).expect("a spot that can go out")
}

/// ⭐ THE FIRST LIVE OFFER CLEARS FIRST: `spot_clear;` before any spot, whatever an earlier session
/// left; nothing at all while the switch is off. CONTROL: an offer that has cleared puts up only
/// what is not up yet.
#[test]
fn the_first_offer_clears_first_and_no_offer_sends_nothing() {
    let t = Instant::now();
    let now = 1_000_000;
    let two = offer(
        1,
        vec![spot("K1ABC", 14_025_000, now), spot("W1AW", 7_030_000, now)],
    );
    assert!(plan(&state(None, t, &[], false), t, now).is_empty(), "off");
    assert_eq!(
        texts(&plan(&state(Some(two.clone()), t, &[], false), t, now)),
        [
            "spot_clear;",
            "spot:K1ABC,CW,14025000,4294065856,New one;",
            "spot:W1AW,CW,7030000,4294065856,New one;"
        ]
    );
    assert_eq!(
        texts(&plan(
            &state(Some(two), t, &[up("K1ABC", 14_025_000)], true),
            t,
            now
        )),
        ["spot:W1AW,CW,7030000,4294065856,New one;"],
        "control: K1ABC is up already"
    );
}

/// What goes, goes first: a spot no longer offered is deleted before anything is put up, so the
/// panadapter never holds more than the cap; one that moved is put up again; one unchanged is left.
#[test]
fn what_goes_is_deleted_first_and_what_moved_is_put_up_again() {
    let t = Instant::now();
    let now = 1_000_000;
    let shown = [
        up("K1ABC", 14_025_000),
        up("W1AW", 7_030_000),
        up("N0CALL", 3_530_000),
    ];
    let o = offer(
        2,
        vec![
            spot("G4ABC", 21_025_000, now),
            spot("W1AW", 7_031_000, now),
            spot("N0CALL", 3_530_000, now),
        ],
    );
    assert_eq!(
        texts(&plan(&state(Some(o), t, &shown, true), t, now)),
        [
            "spot_delete:K1ABC;",
            "spot:G4ABC,CW,21025000,4294065856,New one;",
            "spot:W1AW,CW,7031000,4294065856,New one;"
        ]
    );
}

/// ⭐ AGING: a spot comes off once its station has not been heard for [`MAX_AGE_SECS`], though the
/// offer still lists it; one heard since stays. CONTROL: a second before, it is still wanted.
#[test]
fn a_spot_comes_off_once_its_station_has_not_been_heard_for_fifteen_minutes() {
    let t = Instant::now();
    let heard = 1_000_000;
    let o = offer(
        1,
        vec![
            spot("K1ABC", 14_025_000, heard),
            spot("W1AW", 7_030_000, heard + 600),
        ],
    );
    let s = state(
        Some(o),
        t,
        &[up("K1ABC", 14_025_000), up("W1AW", 7_030_000)],
        true,
    );
    assert!(
        plan(&s, t, heard + MAX_AGE_SECS).is_empty(),
        "control: fifteen minutes on the dot, still up"
    );
    assert_eq!(
        texts(&plan(&s, t, heard + MAX_AGE_SECS + 1)),
        ["spot_delete:K1ABC;"]
    );
}

/// ⭐ A LAPSED OR WITHDRAWN OFFER CLEARS OURS: the switch off, or an offer the radio loop stopped
/// renewing for the [`LEASE`], is one `spot_clear;`, and then nothing. CONTROL: within the lease the
/// offer stands.
#[test]
fn an_offer_withdrawn_or_lapsed_clears_ours() {
    let t = Instant::now();
    let now = 1_000_000;
    let o = offer(1, vec![spot("K1ABC", 14_025_000, now)]);
    let shown = [up("K1ABC", 14_025_000)];
    assert_eq!(
        texts(&plan(&state(None, t, &shown, true), t, now)),
        ["spot_clear;"]
    );
    let s = state(Some(o), t, &shown, true);
    assert!(
        plan(&s, t + LEASE - Duration::from_millis(1), now).is_empty(),
        "control: within the lease"
    );
    assert_eq!(texts(&plan(&s, t + LEASE, now)), ["spot_clear;"]);
    let mut gone = State::default();
    gone.sent(&Op::Clear, t + LEASE);
    assert!(plan(&gone, t + LEASE, now).is_empty(), "and then nothing");
}

/// ⭐ AETHERSDR'S FORMS: no `spot_clear;` ever (it clears every spot it shows, from every source):
/// the first offer only puts ours up, a withdrawn one deletes each of ours by call and frequency;
/// colours go as `#AARRGGBB`; a spot that moved is deleted at its old frequency before it goes up
/// again (AetherSDR keeps every `spot:` as a new spot), and one up for [`AETHERSDR_REFRESH`] is put
/// up again the same way, before AetherSDR forgets it. CONTROL: the specification's forms clear,
/// replace a moved spot by putting it up again, and refresh nothing.
#[test]
fn on_aethersdr_nothing_is_cleared_and_every_delete_names_its_frequency() {
    let t = Instant::now();
    let now = 1_000_000;
    let aether = |o: Option<PanadapterSpots>, shown: &[Up], cleared: bool| State {
        forms: Forms::AetherSdr,
        ..state(o, t, shown, cleared)
    };
    let o = offer(1, vec![spot("K1ABC", 14_025_000, now)]);
    assert_eq!(
        texts_in(
            &plan(&aether(Some(o.clone()), &[], false), t, now),
            Forms::AetherSdr
        ),
        ["spot:K1ABC,CW,14025000,#FFF23EC0,New one;"],
        "the first offer: no clear"
    );
    let up_14025 = [up("K1ABC", 14_025_000), up("W1AW", 7_030_000)];
    assert_eq!(
        texts_in(
            &plan(&aether(None, &up_14025, false), t, now),
            Forms::AetherSdr
        ),
        ["spot_delete:K1ABC,14025000;", "spot_delete:W1AW,7030000;"],
        "withdrawn: each of ours, by call and frequency"
    );
    let moved = offer(2, vec![spot("K1ABC", 14_026_000, now)]);
    assert_eq!(
        texts_in(
            &plan(&aether(Some(moved.clone()), &up_14025[..1], false), t, now),
            Forms::AetherSdr
        ),
        [
            "spot_delete:K1ABC,14025000;",
            "spot:K1ABC,CW,14026000,#FFF23EC0,New one;"
        ],
        "moved: deleted where it was, then put up"
    );
    let mut s = aether(Some(o.clone()), &up_14025[..1], false);
    assert!(plan(&s, t, now).is_empty(), "unchanged: nothing");
    s.renewed = Some(t + AETHERSDR_REFRESH);
    assert_eq!(
        texts_in(&plan(&s, t + AETHERSDR_REFRESH, now), Forms::AetherSdr),
        [
            "spot_delete:K1ABC,14025000;",
            "spot:K1ABC,CW,14025000,#FFF23EC0,New one;"
        ],
        "put up again before AetherSDR forgets it"
    );
    // CONTROL: the specification's forms.
    assert_eq!(
        texts(&plan(&state(Some(moved), t, &up_14025[..1], true), t, now)),
        ["spot:K1ABC,CW,14026000,4294065856,New one;"],
        "control: moved, put up again in its place"
    );
    let mut s = state(Some(o), t, &up_14025[..1], true);
    s.renewed = Some(t + AETHERSDR_REFRESH);
    assert!(
        plan(&s, t + AETHERSDR_REFRESH, now).is_empty(),
        "control: no refresh"
    );
    assert_eq!(
        texts(&plan(&state(None, t, &up_14025, true), t, now)),
        ["spot_clear;"],
        "control: withdrawn, one clear"
    );
}

/// The server's forms come from what it calls itself: AetherSDR by name, in any case; anything
/// else, or nothing, the specification's.
#[test]
fn the_forms_follow_the_servers_device_line() {
    let got: Vec<Forms> = [
        Some("AetherSDR"),
        Some(" aethersdr "),
        Some("SunSDR2DX"),
        None,
    ]
    .into_iter()
    .map(Forms::of)
    .collect();
    assert_eq!(
        got,
        [Forms::AetherSdr, Forms::AetherSdr, Forms::Spec, Forms::Spec]
    );
}

/// ⭐ AT MOST [`TCI_SPOTS_MAX`], THE BEST FIRST, ONE PER CALL: of 150 offered, the first 100 calls
/// go; a call offered twice goes once, as its first (best) offer; a call that is not a callsign or a
/// frequency no radio has is not sent at all.
#[test]
fn at_most_the_cap_go_the_best_first_one_per_call() {
    let now = 1_000_000;
    let mut spots: Vec<PanadapterSpot> = (0..150)
        .map(|n| spot(&nth('K', n), 14_000_000 + n * 1_000, now))
        .collect();
    spots.insert(1, spot(&nth('K', 0), 7_000_000, now));
    spots.insert(2, spot("HELLO WORLD", 7_001_000, now));
    spots.insert(3, spot("W1AW", 0, now));
    let up = wanted(&spots, now);
    assert_eq!(up.len(), TCI_SPOTS_MAX);
    assert_eq!(up[0].call.as_str(), "K1AAA");
    assert_eq!(up[0].hz.get(), 14_000_000, "the first offer of K1AAA");
    assert_eq!(up[1].call.as_str(), "K1AAB");
    assert_eq!(up[TCI_SPOTS_MAX - 1].call.as_str(), nth('K', 99));
    assert!(!up
        .iter()
        .any(|u| u.call.as_str() == "W1AW" || u.call.as_str().contains(' ')));
}

/// ⛔ NOTHING FROM A SPOT BECOMES A COMMAND OF ITS OWN: a call with a separator in it is no callsign
/// and is never sent; the mode keeps letters and digits; the text keeps printable characters, the
/// codec's escapes (`^`, `~`, `*`) dropped and its `:`, `,`, `;` escaped. Each rendering, read by
/// the simulator's own parser (not Nexus's codec), is exactly one `spot` command. CONTROL: the same
/// text rendered raw is read as more than one.
#[test]
fn no_call_mode_or_text_from_a_spot_becomes_a_command() {
    let hostile = PanadapterSpot {
        call: "W1AW".into(),
        hz: 14_025_000,
        mode: "CW;trx:0,true".into(),
        argb: MAGENTA,
        text: "x;trx:0,true;\u{7}\r\n^~*spot_clear;é".into(),
        heard_unix: 0,
    };
    let rendered = Up::of(&hostile)
        .expect("W1AW can be a spot")
        .command(Forms::Spec)
        .render()
        .as_str()
        .to_string();
    assert_eq!(
        rendered,
        "spot:W1AW,CWTRX0TR,14025000,4294065856,x*trx^0~true*spot_clear*;"
    );
    let read = tempo_tcisim::wire::parse(&rendered);
    assert_eq!(
        read.commands
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["spot"]
    );
    assert_eq!(read.commands[0].args.len(), 5);
    for call in [
        "W1AW;trx:0,true",
        "K1ABC,1",
        "W1AW:x",
        "N0CALL\u{7}",
        "VP2V/K1A\u{1b}BC",
    ] {
        assert_eq!(
            Up::of(&PanadapterSpot {
                call: call.into(),
                ..hostile.clone()
            }),
            None,
            "{call:?}"
        );
    }
    let raw = "spot:W1AW,CW,14025000,4294065856,x;trx:0,true;";
    assert!(
        tempo_tcisim::wire::parse(raw).commands.len() > 1,
        "control: unescaped, the text is a second command"
    );
}

// ── On the simulated SDR program ─────────────────────────────────────────────────────────────

fn sim(dialect: SimDialect) -> Simulator {
    Simulator::start(SimConfig::dialect(dialect)).expect("the simulator starts")
}

fn daemon(sim: &Simulator) -> TciDaemon {
    registry::forget((sim.addr(), Trx::new(0).expect("trx 0")));
    let target = Target {
        host: "127.0.0.1".into(),
        port: sim.addr().port(),
        trx: 0,
        dialect: tempo_app::settings::TciDialect::Auto,
        audio: false,
        rigctld_port: 0,
    };
    TciDaemon::start_with(&target, Box::new(NoStreams), Options::default())
        .expect("the daemon starts")
}

/// Seconds since the Unix epoch: a spot heard now.
fn now_unix() -> i64 {
    unix_now()
}

/// The spot commands the simulator read from our connection, in order (no `;`).
fn spot_commands(sim: &Simulator) -> Vec<String> {
    sim.commands_from(0)
        .into_iter()
        .filter(|c| c.starts_with("spot"))
        .collect()
}

/// The calls on the simulated panadapter, in order.
fn on_panadapter(sim: &Simulator) -> Vec<String> {
    sim.radio().spots.into_iter().map(|s| s.call).collect()
}

/// Wait until `done` holds for the simulator, or fail saying `what`.
fn until(sim: &Simulator, what: &str, done: impl Fn(&Simulator) -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done(sim) {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Offer `o` as the radio loop does, every 20 ms, for `ms`.
fn renew(d: &TciDaemon, o: &Option<PanadapterSpots>, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    loop {
        d.offer_spots(o.clone());
        if Instant::now() >= end {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// ⭐ SPOTS GO UP ONLY WHILE THE SWITCH IS ON, AFTER A `spot_clear;`, AND COME OFF AT CLOSE: with
/// the switch off nothing reaches the panadapter; on, `spot_clear;` then each spot; off again, one
/// `spot_clear;`; on again, the clear first again; and the client's close sends `spot_clear;`
/// before its close frame. The spots, coloured, are on the panadapter as offered.
#[test]
fn spots_go_up_only_while_the_switch_is_on_after_a_clear_and_come_off_at_close() {
    let s = sim(SimDialect::expert_sdr3());
    let d = daemon(&s);
    let now = now_unix();
    renew(&d, &None, 300);
    assert!(spot_commands(&s).is_empty(), "the switch off: nothing");
    let o = Some(offer(
        1,
        vec![spot("K1ABC", 14_025_000, now), spot("W1AW", 7_030_000, now)],
    ));
    renew(&d, &o, 0);
    until(&s, "the spots never reached the panadapter", |s| {
        on_panadapter(s) == ["K1ABC", "W1AW"]
    });
    assert_eq!(
        spot_commands(&s),
        [
            "spot_clear",
            "spot:K1ABC,CW,14025000,4294065856,New one",
            "spot:W1AW,CW,7030000,4294065856,New one"
        ]
    );
    assert!(s
        .radio()
        .spots
        .iter()
        .all(|sp| sp.argb == MAGENTA && sp.by == 0));
    renew(&d, &None, 0);
    until(&s, "the switch off never cleared them", |s| {
        on_panadapter(s).is_empty()
    });
    renew(&d, &o, 0);
    until(&s, "the spots never came back", |s| {
        on_panadapter(s).len() == 2
    });
    assert_eq!(
        spot_commands(&s)[3..],
        [
            "spot_clear",
            "spot_clear",
            "spot:K1ABC,CW,14025000,4294065856,New one",
            "spot:W1AW,CW,7030000,4294065856,New one"
        ],
        "off: one clear; on again: the clear first again"
    );
    drop(d);
    until(&s, "the close never reached the simulator", |s| {
        s.log().iter().any(|l| {
            matches!(
                l.event,
                SimEvent::Closed {
                    conn: 0,
                    by: tempo_tcisim::Closer::Client
                }
            )
        })
    });
    let log = s.log();
    let cleared = log.iter().rposition(
        |l| matches!(&l.event, SimEvent::Command { conn: 0, text, .. } if text == "spot_clear"),
    );
    let closed = log.iter().position(|l| {
        matches!(
            l.event,
            SimEvent::Closed {
                conn: 0,
                by: tempo_tcisim::Closer::Client
            }
        )
    });
    assert!(
        cleared.is_some_and(|c| closed.is_some_and(|x| c < x)) && on_panadapter(&s).is_empty(),
        "the close's spot_clear before its close frame: {:?}",
        spot_commands(&s)
    );
}

/// ⭐ AT MOST [`TCI_SPOTS_MAX`] AT ONCE ON THE PANADAPTER: 150 offered put up 100; 100 others
/// offered in their place never make the panadapter hold more than 100 at any moment (replayed
/// from the commands it read, in order), and the old are gone.
#[test]
fn never_more_than_the_cap_on_the_panadapter_at_once() {
    let s = sim(SimDialect::expert_sdr3());
    let d = daemon(&s);
    let now = now_unix();
    let first: Vec<PanadapterSpot> = (0..150)
        .map(|n| spot(&nth('K', n), 14_000_000 + n * 1_000, now))
        .collect();
    renew(&d, &Some(offer(1, first)), 0);
    until(&s, "100 never went up", |s| {
        s.radio().spots.len() == TCI_SPOTS_MAX
    });
    let second: Vec<PanadapterSpot> = (0..150)
        .map(|n| spot(&nth('W', n), 7_000_000 + n * 1_000, now))
        .collect();
    renew(&d, &Some(offer(2, second)), 0);
    until(&s, "the second 100 never went up", |s| {
        let calls = on_panadapter(s);
        calls.len() == TCI_SPOTS_MAX && calls.iter().all(|c| c.starts_with('W'))
    });
    let mut up = std::collections::HashSet::new();
    let mut most = 0;
    for c in spot_commands(&s) {
        if c == "spot_clear" {
            up.clear();
        } else if let Some(call) = c.strip_prefix("spot_delete:") {
            up.remove(call);
        } else if let Some(rest) = c.strip_prefix("spot:") {
            up.insert(rest.split(',').next().unwrap_or_default().to_string());
        }
        most = most.max(up.len());
    }
    assert_eq!(most, TCI_SPOTS_MAX);
    drop(d);
}

/// ⛔ A SPOT FROM OUTSIDE NEVER PUTS A SECOND COMMAND ON THE WIRE: of a hostile offer (a call with
/// `;` and `,` in it, a mode and text with commands, separators and control characters), the
/// simulator reads one `spot_clear` and one `spot` per sendable spot from our connection, and no
/// other command; nothing keys; the text reaches the panadapter escaped.
#[test]
fn a_hostile_spot_never_puts_a_second_command_on_the_wire() {
    let s = sim(SimDialect::expert_sdr3());
    let d = daemon(&s);
    let now = now_unix();
    // The session's own setup at `ready;` ends with its meters; everything after is ours.
    until(&s, "the session's setup never arrived", |s| {
        s.commands_from(0)
            .iter()
            .any(|c| c.starts_with("tx_sensors_enable"))
    });
    let before = s.commands_from(0).len();
    let hostile = vec![
        PanadapterSpot {
            call: "K1ABC;trx:0,true".into(),
            ..spot("K1ABC", 14_025_000, now)
        },
        PanadapterSpot {
            mode: "CW;tune:0,true".into(),
            text: "x;trx:0,true;\u{7}\n^spot_clear;".into(),
            ..spot("W1AW", 14_026_000, now)
        },
        PanadapterSpot {
            text: "a,b:c".into(),
            ..spot("VP2V/K1ABC", 7_030_000, now)
        },
    ];
    let hostile = Some(offer(1, hostile));
    renew(&d, &hostile, 0);
    until(&s, "the two sendable spots never went up", |s| {
        s.radio().spots.len() == 2
    });
    // A little longer, for anything that would follow them.
    renew(&d, &hostile, 300);
    let read: Vec<String> = s.commands_from(0)[before..].to_vec();
    assert_eq!(
        read,
        [
            "spot_clear",
            "spot:W1AW,CWTUNE0T,14026000,4294065856,x*trx^0~true*spot_clear*",
            "spot:VP2V/K1ABC,CW,7030000,4294065856,a~b^c"
        ]
    );
    assert!(
        !s.log()
            .iter()
            .any(|l| matches!(l.event, SimEvent::Keyed { .. })),
        "nothing keyed"
    );
    drop(d);
}

/// ⭐ A CLICK IS KEPT FOR THE RADIO LOOP ONCE, AND ONLY ON ONE OF OURS: our spot clicked on our
/// panorama's A receiver, in both spellings, is kept once; a click on a spot that is not ours, on
/// the B receiver, or on another transceiver's panorama is not; nor is any click with the switch
/// off. A click keys nothing.
#[test]
fn a_click_is_kept_once_and_only_on_one_of_ours() {
    let s = sim(SimDialect::expert_sdr3());
    let d = daemon(&s);
    let now = now_unix();
    let o = Some(offer(1, vec![spot("K1ABC", 14_025_000, now)]));
    renew(&d, &o, 0);
    until(&s, "our spot never went up", |s| {
        on_panadapter(s) == ["K1ABC"]
    });
    let kept = |d: &TciDaemon| {
        // Long enough for a legacy click's wait, and for the watcher to read it.
        renew(d, &o, 3 * tempo_net::tci::session::CLICK_PAIR_MS);
        std::iter::from_fn(|| d.take_spot_click()).collect::<Vec<_>>()
    };
    s.click_spot("K1ABC", 14_025_000);
    assert_eq!(kept(&d), [("K1ABC".to_string(), 14_025_000)], "ours, once");
    s.click_spot("W1AW", 7_030_000);
    s.click_spot_on(0, 1, "K1ABC", 14_025_000);
    s.click_spot_on(1, 0, "K1ABC", 14_025_000);
    s.click_spot("K1ABC", 18_100_000);
    assert_eq!(
        kept(&d),
        [],
        "not ours, B, another transceiver's, another frequency"
    );
    renew(&d, &None, 0);
    until(&s, "the switch off never cleared ours", |s| {
        on_panadapter(s).is_empty()
    });
    s.click_spot("K1ABC", 14_025_000);
    std::thread::sleep(Duration::from_millis(
        3 * tempo_net::tci::session::CLICK_PAIR_MS,
    ));
    assert_eq!(
        std::iter::from_fn(|| d.take_spot_click()).collect::<Vec<_>>(),
        [],
        "the switch off"
    );
    assert!(
        !s.log()
            .iter()
            .any(|l| matches!(l.event, SimEvent::Keyed { .. })),
        "no click keyed anything"
    );
    drop(d);
}

/// ⭐ AN OFFER THE RADIO LOOP STOPS RENEWING LAPSES: a client a radio switch left to the monitor
/// takes its spots off within the [`LEASE`] (and a [`LOOK`]), and takes no click. CONTROL: renewed,
/// they stay up past the lease.
#[test]
fn an_offer_the_radio_loop_stops_renewing_lapses_and_its_spots_come_off() {
    let s = sim(SimDialect::expert_sdr3());
    let d = daemon(&s);
    let now = now_unix();
    let o = Some(offer(1, vec![spot("K1ABC", 14_025_000, now)]));
    renew(&d, &o, 0);
    until(&s, "our spot never went up", |s| {
        on_panadapter(s) == ["K1ABC"]
    });
    renew(&d, &o, (LEASE + LOOK).as_millis() as u64);
    assert_eq!(on_panadapter(&s), ["K1ABC"], "control: renewed, it stays");
    let left = Instant::now();
    until(&s, "the lapsed offer's spots stayed up", |s| {
        on_panadapter(s).is_empty()
    });
    assert!(
        left.elapsed() >= LEASE - Duration::from_millis(100),
        "off only once the lease was over: {:?}",
        left.elapsed()
    );
    s.click_spot("K1ABC", 14_025_000);
    std::thread::sleep(Duration::from_millis(
        3 * tempo_net::tci::session::CLICK_PAIR_MS,
    ));
    assert_eq!(d.take_spot_click(), None, "no click taken");
    drop(d);
}

/// piHPSDR's TCI has no spot commands: its panadapter shows none, and nothing else goes wrong.
#[test]
fn a_server_with_no_spot_commands_shows_none_and_carries_on() {
    let s = sim(SimDialect::pihpsdr());
    let d = daemon(&s);
    let now = now_unix();
    renew(
        &d,
        &Some(offer(1, vec![spot("K1ABC", 14_025_000, now)])),
        500,
    );
    assert!(s.radio().spots.is_empty());
    assert!(d.is_alive(), "the session goes on");
    drop(d);
}

/// ⭐ ON AETHERSDR, OUR SPOTS ARE DRAWN IN THEIR COLOURS, NEVER TWICE, AND OTHER SPOTS ARE LEFT
/// ALONE: a spot AetherSDR shows from another source stays through ours going up, moving, the
/// switch going off and the client's close; ours are drawn in the need's colour; a moved spot
/// leaves no copy; nothing sends `spot_clear;`. CONTROL: on a server of the specification's
/// forms, `spot_clear;` takes the other spot too.
#[test]
fn on_aethersdr_ours_are_drawn_never_twice_and_other_spots_stay() {
    let others = |s: &Simulator| -> Vec<String> {
        s.radio()
            .spots
            .into_iter()
            .filter(|sp| sp.by == usize::MAX)
            .map(|sp| sp.call)
            .collect()
    };
    let s = sim(SimDialect::aether_sdr());
    s.operator("spot:DL1ABC,CW,14030000,#FF00FF00,cluster;");
    let d = daemon(&s);
    let now = now_unix();
    renew(&d, &Some(offer(1, vec![spot("K1ABC", 14_025_000, now)])), 0);
    until(&s, "our spot never went up", |s| {
        on_panadapter(s) == ["DL1ABC", "K1ABC"]
    });
    assert_eq!(
        s.radio()
            .spots
            .iter()
            .find(|sp| sp.call == "K1ABC")
            .and_then(|sp| sp.drawn),
        Some(MAGENTA),
        "drawn in the need's colour"
    );
    renew(&d, &Some(offer(2, vec![spot("K1ABC", 14_026_000, now)])), 0);
    until(&s, "the moved spot never moved", |s| {
        s.radio()
            .spots
            .iter()
            .any(|sp| sp.call == "K1ABC" && sp.hz == 14_026_000)
    });
    assert_eq!(
        on_panadapter(&s),
        ["DL1ABC", "K1ABC"],
        "no copy left behind"
    );
    renew(&d, &None, 0);
    until(&s, "the switch off never took ours off", |s| {
        on_panadapter(s) == ["DL1ABC"]
    });
    renew(&d, &Some(offer(3, vec![spot("W1AW", 7_030_000, now)])), 0);
    until(&s, "ours never came back", |s| {
        on_panadapter(s) == ["DL1ABC", "W1AW"]
    });
    drop(d);
    until(&s, "the close never took ours off", |s| {
        on_panadapter(s) == ["DL1ABC"]
    });
    assert!(
        !spot_commands(&s).iter().any(|c| c == "spot_clear"),
        "no spot_clear: {:?}",
        spot_commands(&s)
    );
    assert_eq!(others(&s), ["DL1ABC"]);
    // CONTROL: the specification's forms clear, the other spot with ours.
    let s = sim(SimDialect::expert_sdr3());
    s.operator("spot:DL1ABC,CW,14030000,4278255360,cluster;");
    let d = daemon(&s);
    renew(&d, &Some(offer(1, vec![spot("K1ABC", 14_025_000, now)])), 0);
    until(&s, "control: our spot never went up", |s| {
        on_panadapter(s).contains(&"K1ABC".to_string())
    });
    drop(d);
    until(&s, "control: the close never cleared", |s| {
        on_panadapter(s).is_empty()
    });
}
