//! Spot me: ONE DX cluster spot of the operator's own call on the station's dial, per press, from
//! either of two buttons: the contest screen's and the Spots board's (in every cockpit that shows
//! it). Both are this one path.
//!
//! - **Only a press posts.** [`contest_self_spot`] and [`cluster_self_spot`] are the commands the
//!   two buttons call; nothing in the app calls either on a timer or on an event, and nothing here
//!   retries or repeats. A spot is public.
//! - **Own call and own dial.** Both are read from the station; the caller sends only the comment.
//! - **ONE cooldown of [`COOLDOWN`]** from the last spot that went, whichever button sent it: a
//!   second press inside it (a double-click, an impatient retry, the other button) is refused and
//!   says how long is left. A refused press does not start one.
//! - **Clear refusals**, as tokens the screen turns into sentences: no call set, no cluster
//!   connected, off the band plan, Contest mode off (the contest screen's button only), a contest
//!   whose rules allow no spotting, or one that allows spotting only over amateur RF
//!   ([`crate::internet_spot_allowed`]).
//! - **The Spots board's button outside a contest** posts on one press. With Contest mode on it is
//!   held to the running contest's rules exactly as the contest screen's is.
//! - **Self-spotting is asked twice where the rules may forbid it.** A contest whose rules data does
//!   not say self-spotting is allowed (`AssistancePolicy::self_spot_allowed`: not allowed, or not
//!   read) refuses the press with [`SELF_UNCONFIRMED`]; the screen warns, and only its second
//!   press, sent as `confirmed`, posts.
//! - **The line is the cluster's own DX command**, `DX <freq kHz> <call> <comment>` (DXSpider's
//!   `dx [by <call>] <freq> <call> <remarks>`; AR-Cluster and CC Cluster take the same), built by
//!   `tempo_net::cluster::format_dx_spot` and queued for the connected node by [`crate::queue_spot`].
//!
//! ⛔ A test build has no path to the cluster: the real door is compiled only outside tests.
use serde::Serialize;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::State;

use tempo_app::engine::Engine;
use tempo_core::fd_rules::SelfSpot;

use crate::SharedEngine;

/// The least time between two spots, from either button.
pub const COOLDOWN: Duration = Duration::from_secs(60);
/// The longest comment sent; the rest is dropped (a cluster line has little room for remarks).
pub const MAX_COMMENT: usize = 30;

pub const NO_CALL: &str = "spotNoCall";
pub const NO_CLUSTER: &str = "spotNoCluster";
pub const OFF_BAND: &str = "spotOffBand";
pub const NO_CONTEST: &str = "spotNoContest";
pub const NOT_ALLOWED: &str = "spotNotAllowed";
/// A contest whose rules are not known to allow SELF-spotting: the screen warns, and only a
/// second press (sent as `confirmed`) posts.
pub const SELF_UNCONFIRMED: &str = "spotSelfUnconfirmed";
pub const FAILED: &str = "spotFailed";
/// `spotCooldown:<seconds left>`.
pub const COOLING: &str = "spotCooldown";

/// What went, for the screen to say.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Posted {
    pub freq_khz: f64,
    pub call: String,
    pub comment: String,
    pub cooldown_secs: u64,
}

/// When the last spot went. Held for a whole press, so two presses cannot both slip past it.
pub struct Gate(Mutex<Option<Instant>>);

impl Gate {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }
}

/// The one cooldown both buttons share ([`spot`] is its only user).
static GATE: Gate = Gate::new();

/// Which Spot me was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    /// The contest screen's, which belongs to a contest.
    Contest,
    /// The Spots board's, in any cockpit, in a contest or not.
    Spots,
}

/// The comment as sent: one line, printable, at most [`MAX_COMMENT`] characters.
fn clean_comment(raw: &str) -> String {
    let one: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    one.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_COMMENT)
        .collect::<String>()
        .trim()
        .to_string()
}

/// One press, through `post` (the cluster door).
pub fn send_with(
    gate: &Gate,
    now: Instant,
    call: &str,
    dial_hz: u64,
    comment: &str,
    post: impl FnOnce(f64, &str, &str) -> Result<(), String>,
) -> Result<Posted, String> {
    let call = call.trim().to_ascii_uppercase();
    if call.is_empty() || !crate::is_real_call(&call) {
        return Err(NO_CALL.into());
    }
    let freq_mhz = dial_hz as f64 / 1e6;
    if tempo_app::bandplan::band_for_dial(freq_mhz).is_none() {
        return Err(OFF_BAND.into());
    }
    let mut last = gate.0.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(at) = *last {
        let since = now.saturating_duration_since(at);
        if since < COOLDOWN {
            let left = (COOLDOWN - since).as_secs_f64().ceil() as u64;
            return Err(format!("{COOLING}:{}", left.max(1)));
        }
    }
    let comment = clean_comment(comment);
    match post(freq_mhz, &call, &comment) {
        Ok(()) => {
            *last = Some(now);
            Ok(Posted {
                freq_khz: dial_hz as f64 / 1000.0,
                call,
                comment,
                cooldown_secs: COOLDOWN.as_secs(),
            })
        }
        // `queue_spot` names the cluster only when no node is connected.
        Err(e) if e.contains("cluster") => Err(NO_CLUSTER.into()),
        Err(e) => {
            eprintln!("contest spot: the DX cluster spot failed: {e}");
            Err(FAILED.into())
        }
    }
}

#[cfg(not(test))]
fn post_cluster(freq_mhz: f64, call: &str, comment: &str) -> Result<(), String> {
    crate::queue_spot(freq_mhz, call.into(), comment.into())
}

#[cfg(test)]
fn post_cluster(_: f64, _: &str, _: &str) -> Result<(), String> {
    Err("test build: nothing is posted".into())
}

/// May this press spot, by the station's contest and its rules? The station's call and dial when
/// it may. `confirmed` is the operator's second press after the self-spot warning.
fn station_spot(eng: &Engine, button: Button, confirmed: bool) -> Result<(String, u64), String> {
    let s = eng.settings();
    if !s.fd_active {
        // Outside a contest the Spots board's button spots on one press; the contest screen's
        // belongs to a contest.
        return match button {
            Button::Spots => Ok((s.mycall.clone(), s.dial_hz())),
            Button::Contest => Err(NO_CONTEST.into()),
        };
    }
    crate::internet_spot_allowed(eng)?;
    let event = s.fd_event.trim();
    let event = if event.is_empty() { "arrlfd" } else { event };
    let rules =
        tempo_core::fd_rules::ruleset_by_id(event, tempo_core::fd_rules::CURRENT_RULES_YEAR);
    if rules.is_some_and(|rs| !rs.assistance.spotting_allowed) {
        return Err(NOT_ALLOWED.into());
    }
    let self_spot = rules.map_or(SelfSpot::Unknown, |rs| rs.assistance.self_spot_allowed);
    if self_spot != SelfSpot::Allowed && !confirmed {
        return Err(SELF_UNCONFIRMED.into());
    }
    Ok((s.mycall.clone(), s.dial_hz()))
}

/// Either button's press: the station's checks (the Engine lock is released before anything is
/// posted), then the ONE cooldown and the cluster.
fn spot(
    engine: &Mutex<Engine>,
    button: Button,
    comment: &str,
    confirmed: Option<bool>,
) -> Result<Posted, String> {
    let (call, dial_hz) = station_spot(
        &crate::engine_lock(engine),
        button,
        confirmed.unwrap_or(false),
    )?;
    send_with(&GATE, Instant::now(), &call, dial_hz, comment, post_cluster)
}

/// The contest screen's Spot button: the station's call and dial, the operator's comment.
#[tauri::command(async)]
pub fn contest_self_spot(
    state: State<'_, SharedEngine>,
    comment: String,
    confirmed: Option<bool>,
) -> Result<Posted, String> {
    spot(&state, Button::Contest, &comment, confirmed)
}

/// The Spots board's Spot button, in any cockpit: the same press and the same cooldown as the
/// contest screen's. Outside a contest it posts on one press; with Contest mode on, the running
/// contest's rules hold it exactly as they hold the contest screen's.
#[tauri::command(async)]
pub fn cluster_self_spot(
    state: State<'_, SharedEngine>,
    comment: String,
    confirmed: Option<bool>,
) -> Result<Posted, String> {
    spot(&state, Button::Spots, &comment, confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn press(
        gate: &Gate,
        now: Instant,
        call: &str,
        comment: &str,
        sent: &RefCell<Vec<String>>,
    ) -> Result<Posted, String> {
        send_with(gate, now, call, 14_025_500, comment, |f, c, m| {
            sent.borrow_mut()
                .push(tempo_net::cluster::format_dx_spot(f * 1000.0, c, m));
            Ok(())
        })
    }

    #[test]
    fn one_press_queues_one_dx_line_with_the_call_dial_and_comment() {
        let gate = Gate::new();
        let sent = RefCell::new(Vec::new());
        let posted = press(&gate, Instant::now(), " kd9taw ", "ILQP CQ", &sent).unwrap();
        assert_eq!(
            *sent.borrow(),
            vec!["DX 14025.5 KD9TAW ILQP CQ\r\n".to_string()]
        );
        assert_eq!(
            posted,
            Posted {
                freq_khz: 14025.5,
                call: "KD9TAW".into(),
                comment: "ILQP CQ".into(),
                cooldown_secs: 60
            }
        );
    }

    #[test]
    fn a_second_press_inside_the_cooldown_posts_nothing_and_says_how_long() {
        let gate = Gate::new();
        let sent = RefCell::new(Vec::new());
        let t0 = Instant::now();
        press(&gate, t0, "KD9TAW", "CQ", &sent).unwrap();
        assert_eq!(
            press(
                &gate,
                t0 + Duration::from_millis(200),
                "KD9TAW",
                "CQ",
                &sent
            )
            .unwrap_err(),
            "spotCooldown:60"
        );
        assert_eq!(
            press(&gate, t0 + Duration::from_secs(45), "KD9TAW", "CQ", &sent).unwrap_err(),
            "spotCooldown:15"
        );
        assert_eq!(sent.borrow().len(), 1);
        press(&gate, t0 + COOLDOWN, "KD9TAW", "CQ", &sent).unwrap();
        assert_eq!(sent.borrow().len(), 2);
    }

    #[test]
    fn no_call_or_an_off_band_dial_posts_nothing_and_starts_no_cooldown() {
        let gate = Gate::new();
        let sent = RefCell::new(Vec::new());
        let t0 = Instant::now();
        assert_eq!(press(&gate, t0, "", "CQ", &sent).unwrap_err(), NO_CALL);
        assert_eq!(press(&gate, t0, "  ", "CQ", &sent).unwrap_err(), NO_CALL);
        let off = send_with(&gate, t0, "KD9TAW", 99_000_000_000, "", |_, _, _| {
            panic!("posted off the band plan")
        });
        assert_eq!(off.unwrap_err(), OFF_BAND);
        assert!(sent.borrow().is_empty());
        press(&gate, t0, "KD9TAW", "CQ", &sent).unwrap();
        assert_eq!(sent.borrow().len(), 1);
    }

    #[test]
    fn no_cluster_connected_is_said_and_starts_no_cooldown() {
        let gate = Gate::new();
        let t0 = Instant::now();
        let r = send_with(&gate, t0, "KD9TAW", 14_025_000, "CQ", |_, _, _| {
            Err("no DX cluster connected — set a cluster host in Settings".into())
        });
        assert_eq!(r.unwrap_err(), NO_CLUSTER);
        let r = send_with(&gate, t0, "KD9TAW", 14_025_000, "CQ", |_, _, _| {
            Err("queue".into())
        });
        assert_eq!(r.unwrap_err(), FAILED);
        let sent = RefCell::new(Vec::new());
        press(&gate, t0, "KD9TAW", "CQ", &sent).unwrap();
        assert_eq!(sent.borrow().len(), 1);
    }

    #[test]
    fn the_comment_is_one_short_line() {
        let gate = Gate::new();
        let sent = RefCell::new(Vec::new());
        let long = "IL  QSO\r\nDX 1 X party CQ CQ CQ CQ CQ CQ CQ CQ CQ";
        let p = press(&gate, Instant::now(), "KD9TAW", long, &sent).unwrap();
        assert_eq!(p.comment, "IL QSO DX 1 X party CQ CQ CQ C");
        assert_eq!(p.comment.chars().count(), MAX_COMMENT);
        assert!(!sent.borrow()[0].trim_end().contains('\n'));
    }

    #[test]
    fn a_test_build_reaches_no_cluster() {
        let gate = Gate::new();
        let r = send_with(
            &gate,
            Instant::now(),
            "KD9TAW",
            14_025_000,
            "CQ",
            post_cluster,
        );
        assert_eq!(r.unwrap_err(), FAILED);
    }

    fn contest_engine(event: &str) -> Engine {
        let mut e = Engine::new("KD9TAW", "EN61", 0);
        let mut s = e.settings().clone();
        s.fd_active = true;
        s.fd_event = event.into();
        e.apply_settings(s);
        e
    }

    #[test]
    fn a_contest_not_known_to_allow_self_spotting_posts_only_on_the_second_press() {
        // Rules data: CQ WW forbids self-spotting, the ARRL VHF contests and the Illinois QSO
        // Party allow it, ARRL Sweepstakes' text says nothing either way.
        for event in ["cqww_cw", "arrlss_cw"] {
            let e = contest_engine(event);
            assert_eq!(
                station_spot(&e, Button::Contest, false).unwrap_err(),
                SELF_UNCONFIRMED,
                "{event}"
            );
            assert_eq!(
                station_spot(&e, Button::Contest, true).unwrap().0,
                "KD9TAW",
                "{event}"
            );
        }
        for event in ["arrlvhf_jan", "ilqp"] {
            let e = contest_engine(event);
            assert_eq!(
                station_spot(&e, Button::Contest, false).unwrap().0,
                "KD9TAW",
                "{event}"
            );
        }
        let off = Engine::new("KD9TAW", "EN61", 0);
        assert_eq!(
            station_spot(&off, Button::Contest, true).unwrap_err(),
            NO_CONTEST
        );
    }

    /// The Spots board's button with Contest mode off: one press spots the station's own call on
    /// its own dial. The contest screen's still belongs to a contest.
    #[test]
    fn outside_a_contest_the_spots_button_posts_on_one_press() {
        let off = Engine::new("KD9TAW", "EN61", 0);
        assert_eq!(
            station_spot(&off, Button::Spots, false),
            Ok(("KD9TAW".to_string(), 14_074_000))
        );
        assert_eq!(
            station_spot(&off, Button::Contest, false).unwrap_err(),
            NO_CONTEST
        );
        // With no callsign set, the whole press is refused in words before anything is held.
        let no_call = Mutex::new(Engine::new("", "EN61", 0));
        assert_eq!(
            spot(&no_call, Button::Spots, "CQ CW", None).unwrap_err(),
            NO_CALL
        );
    }

    /// With Contest mode on, the Spots board's button is held to the running contest's rules
    /// exactly as the contest screen's is: one press where self-spotting is allowed, the warning
    /// and a second press where it is forbidden or not known, and no cluster spot at all while a
    /// contest that allows spotting only over RF runs. Every contest in the rules data, both
    /// presses, answered alike.
    #[test]
    fn with_contest_mode_on_the_spots_button_follows_the_contests_rule_exactly() {
        let year = tempo_core::fd_rules::CURRENT_RULES_YEAR;
        let wfd = tempo_core::fd_rules::ruleset(tempo_core::fieldday::FdEvent::WinterFd, year)
            .event_window(2027);
        let wfd_running = || {
            let mut e = contest_engine("wfd");
            e.set_spot_clock(Some(wfd.start_unix + 7200));
            e
        };
        for event in ["cqww_cw", "arrlss_cw"] {
            let e = contest_engine(event);
            assert_eq!(
                station_spot(&e, Button::Spots, false).unwrap_err(),
                SELF_UNCONFIRMED,
                "{event}"
            );
            assert_eq!(
                station_spot(&e, Button::Spots, true).unwrap().0,
                "KD9TAW",
                "{event}"
            );
        }
        assert_eq!(
            station_spot(&contest_engine("arrlvhf_jan"), Button::Spots, false)
                .unwrap()
                .0,
            "KD9TAW"
        );
        assert_eq!(
            station_spot(&wfd_running(), Button::Spots, true).unwrap_err(),
            crate::SPOT_RF_ONLY
        );

        let seed: serde_json::Value = serde_json::from_str(include_str!(
            "../../crates/tempo-core/src/fd_rules.seed.json"
        ))
        .unwrap();
        let events: Vec<&str> = seed["rulesets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["event"].as_str().unwrap())
            .collect();
        assert!(events.len() >= 18, "{events:?}");
        let mut engines: Vec<(String, Engine)> = events
            .iter()
            .map(|ev| (ev.to_string(), contest_engine(ev)))
            .collect();
        engines.push(("wfd while it runs".into(), wfd_running()));
        for (event, e) in &engines {
            for confirmed in [false, true] {
                assert_eq!(
                    station_spot(e, Button::Spots, confirmed),
                    station_spot(e, Button::Contest, confirmed),
                    "{event}, confirmed {confirmed}"
                );
            }
        }
    }

    /// ONE cooldown, not two: a spot that went holds BOTH buttons for the rest of the minute. The
    /// only test that touches the commands' own gate; a test build's cluster door refuses every
    /// spot, so a spot that went is stood in for by marking the gate as a spot that went does.
    #[test]
    fn a_spot_from_either_button_holds_the_other_for_the_cooldown() {
        let contest = Mutex::new(contest_engine("arrlvhf_jan"));
        let off = Mutex::new(Engine::new("KD9TAW", "EN61", 0));
        let clear = || *GATE.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let went = || {
            send_with(
                &GATE,
                Instant::now(),
                "KD9TAW",
                14_074_000,
                "CQ",
                |_, _, _| Ok(()),
            )
            .unwrap()
        };
        let cooling = |r: Result<Posted, String>| r.unwrap_err().starts_with("spotCooldown:");

        // Control: with no spot gone, both presses get past the cooldown to the cluster door.
        clear();
        assert_eq!(
            spot(&off, Button::Spots, "CQ CW", None).unwrap_err(),
            FAILED
        );
        assert_eq!(
            spot(&contest, Button::Contest, "VHF CQ", None).unwrap_err(),
            FAILED
        );

        went();
        assert!(cooling(spot(&off, Button::Spots, "CQ CW", None)));
        assert!(cooling(spot(&contest, Button::Spots, "CQ CW", None)));
        assert!(cooling(spot(&contest, Button::Contest, "VHF CQ", None)));
        clear();
    }

    /// A spot is public, so it goes only on the operator's press: the two buttons' commands are
    /// the only callers of the one `spot`, which is the only user of the cooldown and the sender,
    /// and nothing else in the shell reaches this module's sender.
    #[test]
    fn only_the_spot_buttons_commands_post() {
        let me = include_str!("contest_spot.rs");
        let body = &me[..me.find("#[cfg(test)]\nmod tests").unwrap()];
        let fn_body = |name: &str| {
            let at = body.find(name).unwrap_or_else(|| panic!("{name}"));
            &body[at..at + body[at..].find("\n}\n").unwrap()]
        };
        assert_eq!(body.matches("&GATE").count(), 1);
        assert!(fn_body("fn spot(").contains("send_with(&GATE"));
        // `spot(` as a name of its own: its definition and the two commands' calls, no more.
        let calls = body
            .match_indices("spot(")
            .filter(|(at, _)| !body[..*at].ends_with(|c: char| c == '_' || c.is_alphanumeric()))
            .count();
        assert_eq!(calls, 3);
        assert_eq!(body.matches("spot(&state").count(), 2);
        assert!(fn_body("pub fn contest_self_spot(").contains("spot(&state, Button::Contest"));
        assert!(fn_body("pub fn cluster_self_spot(").contains("spot(&state, Button::Spots"));
        assert_eq!(body.matches("static ").count(), 1, "one cooldown, not two");
        let lib = include_str!("lib.rs");
        assert!(!lib.contains("contest_spot::send_with"));
        assert!(!lib.contains("contest_spot::spot"));
        assert_eq!(lib.matches("contest_spot::contest_self_spot").count(), 1);
        assert_eq!(lib.matches("contest_spot::cluster_self_spot").count(), 1);
    }
}
