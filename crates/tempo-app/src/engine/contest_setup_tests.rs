//! The plain modes never wait on the contest picked in Settings: FT8 QSO mode's Call CQ and S&P,
//! and Chat, switch whatever state that contest's setup is in, with Contest mode on or off. Only
//! the contest's own modes build its session, so a contest whose exchange is not filled in refuses
//! only those, with its own reason.

use super::*;
use crate::dto::Tier;

/// A station on FT8 with `contest` picked and its exchange left as a fresh install has it (the
/// Texas QSO Party needs the station's county or state), Contest mode `on` or off.
fn picked_and_unfilled(contest: &str, on: bool) -> Engine {
    let mut e = Engine::new("K1ABC", "FN42", 0);
    let mut s = e.settings().clone();
    s.fd_event = contest.into();
    s.fd_active = on;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    e
}

/// ⭐ A plain CQ run (`start_cq`: the Tx6 row outside a contest, and the Remote page's), S&P and
/// Chat switch, with Contest mode off and on; the contest's own S&P is still refused for the
/// exchange it lacks (the control).
#[test]
fn plain_modes_never_wait_on_the_contests_setup() {
    for on in [false, true] {
        for spec in ["qso-run", "qso-monitor", "chat"] {
            let mut e = picked_and_unfilled("txqp", on);
            assert_eq!(e.set_mode(spec), Ok(()), "Contest mode {on}: {spec}");
        }
        let mut e = picked_and_unfilled("txqp", on);
        assert_eq!(
            e.start_cq(None),
            Ok(()),
            "Contest mode {on}: a plain CQ run"
        );
        assert!(
            matches!(&e.mode, Mode::Qso { running: true, .. }),
            "Contest mode {on}: the run started"
        );
        let r = e.set_mode("fieldday-sp");
        assert!(
            r.is_err(),
            "Contest mode {on}: the contest's own S&P still needs its exchange, {r:?}"
        );
    }
}
