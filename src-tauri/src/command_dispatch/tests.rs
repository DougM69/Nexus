//! The dispatcher's rules, each shown refusing what it refuses and admitting what it admits, and
//! the guards that hold its tables to the commands, the screen and the stores they describe.

use super::*;
use crate::command_classes::registered;
use serde_json::json;

// --- Scaffolding ---------------------------------------------------------------------------------

/// Stores whose revisions and `ui-state.json` a test sets.
#[derive(Default)]
struct Fake {
    settings: Mutex<Option<Revision>>,
    ui: Mutex<BTreeMap<String, String>>,
}

impl Stores for Fake {
    fn revision(&self, store: Store) -> Option<Revision> {
        match store {
            Store::Settings => lock(&self.settings).clone(),
            Store::UiState => Revision::of(&split_ui_state(self.ui_state()).0),
        }
    }

    fn ui_state(&self) -> BTreeMap<String, String> {
        lock(&self.ui).clone()
    }
}

fn rev(text: &str) -> Revision {
    Revision::new(text.to_string())
}

/// A shack whose settings stand at revision `r0`.
fn shack() -> Dispatcher<Fake> {
    let shack = Dispatcher::new(Fake::default());
    settings_now(&shack, Some("r0"));
    shack
}

fn settings_now(shack: &Dispatcher<Fake>, revision: Option<&str>) {
    *lock(&shack.inner.stores.settings) = revision.map(rev);
}

fn ui_now(shack: &Dispatcher<Fake>, pairs: &[(&str, &str)]) {
    *lock(&shack.inner.stores.ui) = map(pairs);
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn caller(run: u64, holds_lease: bool) -> Caller {
    Caller {
        key: CallerKey {
            device: "the operator's laptop".to_string(),
            run,
        },
        holds_lease,
    }
}

fn controller() -> Caller {
    caller(1, true)
}

/// A paired computer that does not hold the lease.
fn other_device() -> Caller {
    caller(1, false)
}

fn call(id: u64, cmd: &str, args: Value) -> Call {
    Call {
        id,
        cmd: cmd.to_string(),
        args,
        base: None,
    }
}

fn save(id: u64, cmd: &str, args: Value, base: &str) -> Call {
    Call {
        base: Some(rev(base)),
        ..call(id, cmd, args)
    }
}

/// The admission in a word, for assertions that read plainly.
fn kind<S: Stores>(admission: &Admission<S>) -> String {
    match admission {
        Admission::Stop { .. } => "stop".to_string(),
        Admission::Run { .. } => "run".to_string(),
        Admission::Answered(outcome) => format!("answered {outcome:?}"),
        Admission::Refused(refusal) => format!("refused {}", refusal.code()),
    }
}

fn refused(refusal: Refusal) -> String {
    format!("refused {}", refusal.code())
}

/// The commands of one class, from the handler list.
fn of_class(class: Class) -> Vec<&'static str> {
    registered()
        .into_iter()
        .filter(|name| class_of(name) == Some(class))
        .collect()
}

/// Arguments that read as the on half of `cmd`, when it is a transmit switch; `{}` otherwise.
fn on_args(cmd: &str) -> Value {
    match OFF_HALVES.iter().find(|(name, ..)| *name == cmd) {
        Some((_, arg, Off::False)) => json!({ *arg: true }),
        Some((_, arg, Off::OneOf(_))) => json!({ *arg: "qso-run" }),
        None => json!({}),
    }
}

/// Every argument that reads as the off half of `cmd`.
fn off_args(cmd: &str) -> Vec<Value> {
    match OFF_HALVES.iter().find(|(name, ..)| *name == cmd) {
        Some((_, arg, Off::False)) => vec![json!({ *arg: false })],
        Some((_, arg, Off::OneOf(values))) => values.iter().map(|v| json!({ *arg: v })).collect(),
        None => panic!("{cmd} has no off half"),
    }
}

/// Fills every slot one computer has: [`IN_FLIGHT`] calls held running, from ids 1000 up.
fn fill_the_slots(shack: &Dispatcher<Fake>, who: &Caller) -> Vec<Admission<Fake>> {
    let held: Vec<_> = (0..IN_FLIGHT as u64)
        .map(|n| {
            let cmd = if n % 2 == 0 {
                "get_snapshot"
            } else {
                "get_band_plan"
            };
            shack.admit(who, call(1000 + n, cmd, json!({})))
        })
        .collect();
    assert!(held.iter().all(|a| kind(a) == "run"), "the slots filled");
    held
}

// --- Deny by default -----------------------------------------------------------------------------

#[test]
fn an_unclassed_command_is_refused_at_both_ends() {
    let shack = shack();
    for (id, name) in ["throwaway_unclassed_probe", "plugin:dialog|open", ""]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            route(name, &json!({})),
            Route::Refused(Refusal::Unclassed),
            "{name:?} here"
        );
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, json!({})));
        assert_eq!(
            kind(&admission),
            refused(Refusal::Unclassed),
            "{name:?} at the shack"
        );
    }
}

#[test]
fn a_command_that_runs_here_runs_here_and_never_at_the_shack() {
    let shack = shack();
    let here = of_class(Class::RunsHere);
    assert!(here.len() >= 30, "the table's own count: {}", here.len());
    for (id, name) in here.into_iter().enumerate() {
        assert_eq!(route(name, &json!({})), Route::Here, "{name}");
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, json!({})));
        assert_eq!(kind(&admission), refused(Refusal::RunsHere), "{name}");
    }
}

#[test]
fn a_shack_only_command_is_refused_at_both_ends() {
    // The operator's rulings of 2026-10-07 by name: a settings restore and a factory reset, a
    // whole-log purge and the Connections log stay at the shack. The clock's repair waits on an
    // administrator prompt on the shack's own screen.
    for name in [
        "import_settings_bundle",
        "reset_settings",
        "purge_log",
        "get_connection_log",
        "repair_clock",
    ] {
        assert_eq!(class_of(name), Some(Class::RefusedHere), "{name}");
    }
    let shack = shack();
    for (id, name) in of_class(Class::RefusedHere).into_iter().enumerate() {
        assert_eq!(
            route(name, &json!({})),
            Route::Refused(Refusal::ShackOnly),
            "{name} here"
        );
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, json!({})));
        assert_eq!(
            kind(&admission),
            refused(Refusal::ShackOnly),
            "{name} at the shack"
        );
    }
}

#[test]
fn nothing_from_afar_starts_a_transmission_yet() {
    let shack = shack();
    let starts = of_class(Class::StartsTx);
    assert!(
        starts.len() >= 30,
        "the table's own count: {}",
        starts.len()
    );
    for (id, name) in starts.into_iter().enumerate() {
        // The other computer sends it; the shack refuses it.
        assert_eq!(route(name, &on_args(name)), Route::Shack, "{name}");
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, on_args(name)));
        assert_eq!(kind(&admission), refused(Refusal::TransmitNotYet), "{name}");
    }
}

#[test]
fn only_the_computer_holding_the_lease_runs_a_station_command() {
    let shack = shack();
    let refused_here = shack.admit(&other_device(), call(1, "get_snapshot", json!({})));
    assert_eq!(kind(&refused_here), refused(Refusal::NotController));
    let admitted = shack.admit(&controller(), call(2, "get_snapshot", json!({})));
    assert_eq!(kind(&admitted), "run");
}

#[test]
fn arguments_that_are_not_an_object_are_refused() {
    let shack = shack();
    for (id, args) in [json!(null), json!([1, 2]), json!("get_snapshot")]
        .into_iter()
        .enumerate()
    {
        let admission = shack.admit(&controller(), call(id as u64 + 1, "get_snapshot", args));
        assert_eq!(kind(&admission), refused(Refusal::BadArguments));
    }
}

// --- Stop's rules --------------------------------------------------------------------------------

/// Each `args` for `cmd` is admitted as a Stop under every condition that refuses any other call:
/// from a device without the lease, with every slot full, past the rate cap, as a repeated id, and
/// with the shack's stores unreadable.
fn admitted_as_a_stop_whatever_else(cmd: &str, args: &[Value]) {
    let shack = shack();
    settings_now(&shack, None);
    let held = fill_the_slots(&shack, &controller());
    let now = Instant::now();
    for n in 0..=PER_SECOND as u64 {
        // Past the cap: these are refused, and the stops below still are not.
        let _ = shack.admit_at(
            &controller(),
            call(2000 + n, "get_snapshot", json!({})),
            now,
        );
    }
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(3000, "get_snapshot", json!({})), now)),
        refused(Refusal::RateLimited),
        "the rate cap is reached"
    );
    for args in args {
        for who in [controller(), other_device()] {
            for id in [9000, 9000, 1, 0] {
                match shack.admit_at(&who, call(id, cmd, args.clone()), now) {
                    Admission::Stop { args: run } => {
                        assert_eq!(&run, args, "{cmd} runs with its own arguments")
                    }
                    other => panic!("{cmd} {args} id {id}: {}", kind(&other)),
                }
            }
        }
    }
    drop(held);
}

#[test]
fn every_stop_is_admitted_whatever_else() {
    let stops = of_class(Class::StopsTx);
    assert_eq!(stops.len(), 8, "the table's own count: {stops:?}");
    for name in stops {
        assert_eq!(route(name, &json!({})), Route::Shack, "{name}");
        // A stop takes no arguments the dispatcher reads, so even malformed ones do not refuse it.
        admitted_as_a_stop_whatever_else(name, &[json!({}), json!(null)]);
    }
}

#[test]
fn the_off_half_of_every_transmit_switch_is_never_refused() {
    // The operator's ruling of 2026-10-07: the dispatcher runs the off half under Stop's rules,
    // reading the command's own on/off argument.
    for (name, ..) in OFF_HALVES {
        admitted_as_a_stop_whatever_else(name, &off_args(name));
    }
}

#[test]
fn the_on_half_of_a_transmit_switch_is_its_class() {
    let shack = shack();
    for (id, (name, ..)) in OFF_HALVES.iter().enumerate() {
        let admission = shack.admit(&controller(), call(id as u64 + 1, name, on_args(name)));
        let expected = match class_of(name) {
            Some(Class::StartsTx) => refused(Refusal::TransmitNotYet),
            // Turning the RTTY sequencer on keys nothing, so it runs at the shack.
            Some(Class::RunsAtShack) => "run".to_string(),
            other => panic!("{name} is {other:?}"),
        };
        assert_eq!(kind(&admission), expected, "{name}");
    }
}

#[test]
fn set_mode_is_a_stop_only_into_a_passive_mode() {
    let shack = shack();
    for mode in ["chat", "qso-monitor", "fieldday-sp"] {
        let admission = shack.admit(
            &other_device(),
            call(1, "set_mode", json!({ "mode": mode })),
        );
        assert_eq!(kind(&admission), "stop", "{mode}");
    }
    // A run mode calls CQ, and a mode this table does not know is never read as off.
    for (id, mode) in ["qso-run", "fieldday-run", "bogus", "QSO-MONITOR"]
        .into_iter()
        .enumerate()
    {
        let admission = shack.admit(
            &controller(),
            call(id as u64 + 10, "set_mode", json!({ "mode": mode })),
        );
        assert_eq!(kind(&admission), refused(Refusal::TransmitNotYet), "{mode}");
    }
}

#[test]
fn an_argument_that_does_not_read_as_off_is_the_on_half() {
    // Fail-safe for a start: only the argument's real "off" is a stop, and the guard below holds
    // each argument's name to the command's signature, so the screen's real off half reads.
    let shack = shack();
    for (id, args) in [
        json!({}),
        json!({ "on": "false" }),
        json!({ "on": 0 }),
        json!({ "on": null }),
        json!({ "enabled": false }),
    ]
    .into_iter()
    .enumerate()
    {
        let admission = shack.admit(&controller(), call(id as u64 + 1, "set_tune", args.clone()));
        assert_eq!(kind(&admission), refused(Refusal::TransmitNotYet), "{args}");
    }
}

#[test]
fn a_stop_takes_no_slot_and_leaves_no_outcome() {
    let shack = shack();
    for id in 1..=(2 * IN_FLIGHT as u64) {
        assert_eq!(
            kind(&shack.admit(&controller(), call(id, "halt_tx", json!({})))),
            "stop"
        );
    }
    let next = shack.admit(&controller(), call(100, "get_snapshot", json!({})));
    assert_eq!(kind(&next), "run", "no slot was taken");
    assert_eq!(
        shack.outcomes(&controller().key, &[1, 2]),
        vec![Outcome::Unknown, Outcome::Unknown],
        "a stop is answered when it runs, and never from the journal"
    );
}

// --- The rulings by name -------------------------------------------------------------------------

#[test]
fn a_whole_settings_save_from_afar_saves_every_field() {
    // The operator's ruling of 2026-10-07: "Every field saves", the stream window's behaviour, so
    // the connector fields, the stream switch and the two listener switches pass untouched.
    let shack = shack();
    let settings = json!({ "settings": {
        "remoteStream": true,
        "connectWeb": true,
        "fdScoreboard": true,
        "cloudlogUrl": "https://log.example/index.php",
        "tqslPath": "/opt/tqsl/bin/tqsl",
        "lotwUsername": "KD9TAW",
    }});
    match shack.admit(
        &controller(),
        save(1, "set_settings", settings.clone(), "r0"),
    ) {
        Admission::Run { args, .. } => assert_eq!(args, settings),
        other => panic!("{}", kind(&other)),
    }
}

#[test]
fn the_rigs_vox_switch_runs_at_the_shack_vox_on_included() {
    // The operator's ruling of 2026-10-07: "Allow VOX too".
    assert_eq!(class_of("set_rig_func"), Some(Class::RunsAtShack));
    let shack = shack();
    for (id, (func, on)) in [("VOX", true), ("VOX", false), ("NB", true)]
        .into_iter()
        .enumerate()
    {
        let args = json!({ "func": func, "on": on });
        assert_eq!(route("set_rig_func", &args), Route::Shack);
        let admission = shack.admit(&controller(), call(id as u64 + 1, "set_rig_func", args));
        assert_eq!(kind(&admission), "run", "{func} {on}");
    }
}

#[test]
fn a_winlink_password_typed_here_is_refused_at_both_ends() {
    // The operator's ruling of 2026-10-07: "Shack's stored password". An empty one is the
    // command's own "use the keychain's", so the session runs at the shack.
    let shack = shack();
    let stored = json!({ "callsign": "KD9TAW", "password": "" });
    assert_eq!(route("winlink_connect", &stored), Route::Shack);
    assert_eq!(
        kind(&shack.admit(&controller(), call(1, "winlink_connect", stored))),
        "run"
    );
    for (id, typed) in [
        json!({ "callsign": "KD9TAW", "password": "not-a-real-one" }),
        json!({ "callsign": "KD9TAW", "password": " " }),
        json!({ "callsign": "KD9TAW", "password": null }),
        json!({ "callsign": "KD9TAW" }),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            route("winlink_connect", &typed),
            Route::Refused(Refusal::TypedPassword),
            "never sent: {}",
            typed["password"]
        );
        let admission = shack.admit(&controller(), call(id as u64 + 2, "winlink_connect", typed));
        assert_eq!(kind(&admission), refused(Refusal::TypedPassword));
    }
}

// --- At most once --------------------------------------------------------------------------------

#[test]
fn a_repeated_id_is_answered_and_never_run_twice() {
    let shack = shack();
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(5, "log_current_qso", json!({})))
    else {
        panic!("admitted")
    };
    assert_eq!(
        kind(&shack.admit(&controller(), call(5, "log_current_qso", json!({})))),
        "answered Running",
        "a repeat while it runs"
    );
    let _ = ticket.finish(Ok(json!({ "logged": true })));
    assert_eq!(
        kind(&shack.admit(&controller(), call(5, "log_current_qso", json!({})))),
        "answered Done",
        "a repeat once it ran"
    );
    assert_eq!(
        kind(&shack.admit(&controller(), call(3, "log_current_qso", json!({})))),
        refused(Refusal::Replayed),
        "an id below the highest, never seen, is never run"
    );
}

#[test]
fn what_became_of_each_call_after_a_reconnect() {
    let shack = shack();
    let me = controller();
    let run = |id, cmd: &str| match shack.admit(&me, call(id, cmd, json!({}))) {
        Admission::Run { ticket, .. } => ticket,
        other => panic!("{id}: {}", kind(&other)),
    };
    let _ = run(1, "log_current_qso").finish(Ok(json!(null)));
    let _ = run(2, "upload_lotw_report").finish(Err("LoTW answered 503".to_string()));
    let running = run(3, "import_adif");
    let _ = shack.admit(&other_device(), call(4, "log_current_qso", json!({})));
    drop(run(5, "edit_qso"));
    assert_eq!(
        shack.outcomes(&me.key, &[1, 2, 3, 4, 5, 6]),
        vec![
            Outcome::Done,
            Outcome::Failed("LoTW answered 503".to_string()),
            Outcome::Running,
            Outcome::Refused(Refusal::NotController),
            Outcome::Lost,
            Outcome::Unknown,
        ]
    );
    drop(running);
    assert_eq!(
        shack.outcomes(&caller(2, true).key, &[1]),
        vec![Outcome::Unknown],
        "another run of Nexus has its own journal"
    );
}

#[test]
fn outcomes_are_kept_for_ten_minutes_and_an_old_id_is_never_run_again() {
    let shack = shack();
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(1, "log_current_qso", json!({})))
    else {
        panic!("admitted")
    };
    let _ = ticket.finish(Ok(json!(null)));
    let start = Instant::now();
    let key = controller().key;
    assert_eq!(
        shack.outcomes_at(&key, &[1], start + RESULT_AGE - Duration::from_secs(1)),
        vec![Outcome::Done]
    );
    let later = start + RESULT_AGE + Duration::from_secs(1);
    assert_eq!(shack.outcomes_at(&key, &[1], later), vec![Outcome::Unknown]);
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(1, "log_current_qso", json!({})), later)),
        refused(Refusal::Replayed)
    );
}

#[test]
fn the_journal_keeps_the_newest_outcomes_and_never_reruns_an_older_id() {
    let shack = shack();
    let now = Instant::now();
    for id in 1..=(JOURNAL as u64 + 1) {
        // Spread across seconds so the rate cap stays out of it.
        let at = now + Duration::from_secs(id / u64::from(PER_SECOND) + 1);
        match shack.admit_at(&controller(), call(id, "get_snapshot", json!({})), at) {
            Admission::Run { ticket, .. } => drop(ticket.finish(Ok(json!(null)))),
            other => panic!("{id}: {}", kind(&other)),
        }
    }
    let key = controller().key;
    let last = JOURNAL as u64 + 1;
    assert_eq!(
        shack.outcomes(&key, &[1, 2, last]),
        vec![Outcome::Unknown, Outcome::Done, Outcome::Done]
    );
    assert_eq!(
        kind(&shack.admit(&controller(), call(1, "get_snapshot", json!({})))),
        refused(Refusal::Replayed)
    );
}

#[test]
fn a_new_run_of_nexus_starts_its_ids_again() {
    let shack = shack();
    drop(shack.admit(&caller(1, true), call(7, "get_snapshot", json!({}))));
    assert_eq!(
        kind(&shack.admit(&caller(2, true), call(1, "get_snapshot", json!({})))),
        "run"
    );
}

#[test]
fn a_long_error_is_kept_cut_on_a_character_boundary() {
    let long = "é".repeat(ERROR_BYTES);
    let kept = bounded(&long);
    assert!((ERROR_BYTES - 1..=ERROR_BYTES).contains(&kept.len()));
    assert!(long.starts_with(&kept));
    assert_eq!(bounded("short"), "short");
}

// --- Caps ----------------------------------------------------------------------------------------

#[test]
fn the_in_flight_caps_refuse_past_eight_calls_and_four_of_one_command() {
    let shack = shack();
    let me = controller();
    let same: Vec<_> = (1..=PER_COMMAND as u64)
        .map(|id| shack.admit(&me, call(id, "get_snapshot", json!({}))))
        .collect();
    assert!(same.iter().all(|a| kind(a) == "run"));
    assert_eq!(
        kind(&shack.admit(&me, call(10, "get_snapshot", json!({})))),
        refused(Refusal::Busy),
        "a fifth of one command"
    );
    let others: Vec<_> = (11..)
        .take(IN_FLIGHT - PER_COMMAND)
        .map(|id| shack.admit(&me, call(id, "get_band_plan", json!({}))))
        .collect();
    assert!(others.iter().all(|a| kind(a) == "run"));
    assert_eq!(
        kind(&shack.admit(&me, call(20, "get_settings", json!({})))),
        refused(Refusal::Busy),
        "a ninth call"
    );
    assert_eq!(
        kind(&shack.admit(&caller(9, true), call(1, "get_settings", json!({})))),
        "run",
        "the caps are each run's own"
    );
    drop(others);
    assert_eq!(
        kind(&shack.admit(&me, call(21, "get_settings", json!({})))),
        "run",
        "a finished call frees its slot"
    );
    drop(same);
}

#[test]
fn the_rate_cap_refuses_past_its_count_in_a_second() {
    let shack = shack();
    let now = Instant::now();
    for id in 1..=u64::from(PER_SECOND) {
        match shack.admit_at(&controller(), call(id, "get_snapshot", json!({})), now) {
            Admission::Run { ticket, .. } => drop(ticket.finish(Ok(json!(null)))),
            other => panic!("{id}: {}", kind(&other)),
        }
    }
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(1000, "get_snapshot", json!({})), now)),
        refused(Refusal::RateLimited)
    );
    let next = now + Duration::from_secs(1);
    assert_eq!(
        kind(&shack.admit_at(&controller(), call(1001, "get_snapshot", json!({})), next)),
        "run"
    );
}

// --- A stale whole save --------------------------------------------------------------------------

#[test]
fn a_whole_save_from_an_older_copy_is_refused() {
    let shack = shack();
    let saves: Vec<&str> = STORES
        .iter()
        .filter(|(_, store, access)| *store == Store::Settings && *access == Access::WholeSave)
        .map(|(name, ..)| *name)
        .collect();
    assert_eq!(saves.len(), 7, "{saves:?}");
    let mut id = 0;
    let mut next = || {
        id += 1;
        id
    };
    for name in saves {
        settings_now(&shack, Some("r1"));
        for base in ["r0", "", "r10"] {
            let admission = shack.admit(&controller(), save(next(), name, json!({}), base));
            assert_eq!(
                kind(&admission),
                refused(Refusal::ChangedAtShack),
                "{name} {base:?}"
            );
        }
        let no_base = shack.admit(&controller(), call(next(), name, json!({})));
        assert_eq!(
            kind(&no_base),
            refused(Refusal::ChangedAtShack),
            "{name} names no copy"
        );
        settings_now(&shack, None);
        let unreadable = shack.admit(&controller(), save(next(), name, json!({}), "r1"));
        assert_eq!(
            kind(&unreadable),
            refused(Refusal::ChangedAtShack),
            "{name} unreadable"
        );
        settings_now(&shack, Some("r1"));
        let current = shack.admit(&controller(), save(next(), name, json!({}), "r1"));
        assert_eq!(kind(&current), "run", "{name} from the current copy");
    }
}

#[test]
fn a_read_answers_with_the_revision_from_before_it_ran() {
    let shack = shack();
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(1, "get_settings", json!({})))
    else {
        panic!("admitted")
    };
    // A save at the shack lands while the read runs: the copy may or may not have it, so the
    // older revision is the one that can only refuse, never let a stale save through.
    settings_now(&shack, Some("r1"));
    let reply = ticket.finish(Ok(json!({ "mycall": "KD9TAW" })));
    assert_eq!(reply.result, Ok(json!({ "mycall": "KD9TAW" })));
    assert_eq!(reply.revision.as_ref().map(Revision::as_str), Some("r0"));
    assert_eq!(
        kind(&shack.admit(&controller(), save(2, "set_settings", json!({}), "r0"))),
        refused(Refusal::ChangedAtShack)
    );
}

#[test]
fn an_accepted_save_answers_with_the_revision_after_it() {
    let shack = shack();
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), save(1, "set_settings", json!({}), "r0"))
    else {
        panic!("admitted")
    };
    settings_now(&shack, Some("r1"));
    let reply = ticket.finish(Ok(json!({ "snapshot": true })));
    assert_eq!(reply.revision.as_ref().map(Revision::as_str), Some("r1"));
    // The panel saves the station first and the radio second, from one copy.
    let second = shack.admit(
        &controller(),
        save(2, "update_radio_profile", json!({}), "r1"),
    );
    assert_eq!(kind(&second), "run");
    // A failed save answers with no revision: the copy is what it was.
    let failed = match second {
        Admission::Run { ticket, .. } => ticket.finish(Err("serial port busy".to_string())),
        other => panic!("{}", kind(&other)),
    };
    assert_eq!(failed.revision, None);
}

// --- The one mixed store -------------------------------------------------------------------------

#[test]
fn a_load_from_afar_carries_only_the_station_keys() {
    let shack = shack();
    let whole = map(&[
        ("nexus.memory.bank.v2", "[1]"),
        ("nexus.watchlist", "[\"K1ABC\"]"),
        ("nexus-ui-scale-cap", "1.5"),
        ("nexus.panels.phone.main", "{}"),
    ]);
    ui_now(&shack, &[]);
    assert_eq!(route("ui_state_load", &json!({})), Route::Split);
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(1, "ui_state_load", json!({})))
    else {
        panic!("admitted")
    };
    let reply = ticket.finish(Ok(serde_json::to_value(&whole).unwrap()));
    assert_eq!(
        reply.result,
        Ok(json!({ "nexus.memory.bank.v2": "[1]", "nexus.watchlist": "[\"K1ABC\"]" }))
    );
    assert_eq!(
        reply.revision,
        Revision::of(&BTreeMap::<String, String>::new()),
        "the revision of the station's part, as it stood before the read"
    );
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(2, "ui_state_load", json!({})))
    else {
        panic!("admitted")
    };
    assert_eq!(
        ticket.finish(Ok(json!(["not", "a", "map"]))).result,
        Ok(json!({})),
        "a malformed answer sends nothing"
    );
}

#[test]
fn a_save_from_afar_writes_the_station_keys_and_keeps_the_shacks_own() {
    let shack = shack();
    ui_now(
        &shack,
        &[
            ("nexus-ui-scale-cap", "1.5"),
            ("nexus.navOrder", "[\"ft8\"]"),
            ("nexus.watchlist", "old"),
            ("nexus.sats.chasing", "[25544]"),
        ],
    );
    let base = shack.inner.stores.revision(Store::UiState).unwrap();
    let sent = json!({ "state": {
        "nexus.watchlist": "new",
        "nexus-ui-scale-cap": "3.0",
        "nexus.features.v1": "{}",
    }});
    assert_eq!(route("ui_state_save", &sent), Route::Split);
    match shack.admit(
        &controller(),
        save(1, "ui_state_save", sent.clone(), base.as_str()),
    ) {
        Admission::Run { args, .. } => assert_eq!(
            args,
            // The other computer's scale and modes stay there; the chase set it removed is gone.
            json!({ "state": {
                "nexus-ui-scale-cap": "1.5",
                "nexus.navOrder": "[\"ft8\"]",
                "nexus.watchlist": "new",
            }})
        ),
        other => panic!("{}", kind(&other)),
    }
    // A save made before the shack's station keys changed is refused like any stale save; one
    // that changed only the shack's own keys is not.
    ui_now(
        &shack,
        &[
            ("nexus-ui-scale-cap", "2.0"),
            ("nexus.watchlist", "old"),
            ("nexus.sats.chasing", "[25544]"),
        ],
    );
    assert_eq!(
        kind(&shack.admit(
            &controller(),
            save(2, "ui_state_save", sent.clone(), base.as_str())
        )),
        "run",
        "only the shack's own keys moved"
    );
    ui_now(&shack, &[("nexus.watchlist", "changed at the shack")]);
    assert_eq!(
        kind(&shack.admit(&controller(), save(3, "ui_state_save", sent, base.as_str()))),
        refused(Refusal::ChangedAtShack)
    );
    let base = shack.inner.stores.revision(Store::UiState).unwrap();
    assert_eq!(
        kind(&shack.admit(
            &controller(),
            save(4, "ui_state_save", json!({ "state": [1] }), base.as_str())
        )),
        refused(Refusal::BadArguments)
    );
}

#[test]
fn the_other_computer_splits_and_joins_by_the_same_keys() {
    let whole = map(&[
        ("nexus.memory.bank.v2", "mine"),
        ("nexus.profiles", "mine"),
        ("nexus-ui-scale-cap", "1.25"),
        ("nexus.panels.cw.main", "{}"),
    ]);
    let (station, own) = split_ui_state(whole);
    assert_eq!(
        station,
        map(&[("nexus.memory.bank.v2", "mine"), ("nexus.profiles", "mine")])
    );
    assert_eq!(
        own,
        map(&[
            ("nexus-ui-scale-cap", "1.25"),
            ("nexus.panels.cw.main", "{}")
        ])
    );
    let from_shack = map(&[
        ("nexus.memory.bank.v2", "shack's"),
        ("nexus-ui-scale-cap", "9"),
    ]);
    assert_eq!(
        join_ui_state(from_shack, own),
        map(&[
            ("nexus.memory.bank.v2", "shack's"),
            ("nexus-ui-scale-cap", "1.25"),
            ("nexus.panels.cw.main", "{}"),
        ]),
        "the shack's station keys, this computer's own keys"
    );
}

// --- Events --------------------------------------------------------------------------------------

#[test]
fn pounce_is_the_one_event_that_crosses_and_both_screens_use_its_name() {
    assert!(forwards("pounce"));
    for other in [
        "",
        "pounce2",
        "Pounce",
        "tauri://close-requested",
        "remote-log-notice",
    ] {
        assert!(!forwards(other), "{other:?}");
    }
    assert!(
        include_str!("../lib.rs").contains(r#".emit("pounce", "#),
        "the shack emits it by this name"
    );
    assert!(
        include_str!("../../../ui/src/usePounce.ts").contains("listen<PounceAlert>('pounce'"),
        "the screen listens for it by this name"
    );
}

#[test]
fn every_refusal_has_its_own_code() {
    let all = [
        Refusal::Unclassed,
        Refusal::RunsHere,
        Refusal::ShackOnly,
        Refusal::TransmitNotYet,
        Refusal::TypedPassword,
        Refusal::NotController,
        Refusal::Busy,
        Refusal::RateLimited,
        Refusal::Replayed,
        Refusal::ChangedAtShack,
        Refusal::BadArguments,
    ];
    let mut codes: Vec<&str> = all.iter().map(|r| r.code()).collect();
    assert!(codes
        .iter()
        .all(|c| c.chars().all(|ch| ch.is_ascii_alphabetic())));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), all.len());
}

// --- The tables, held to what they describe ------------------------------------------------------

/// The parameters the screen gives a command, as `(json key, Rust type)`, read from its
/// `#[tauri::command]` signature in `lib.rs`. Managed state, the app handle and the window are
/// Tauri's to supply and are left out. A key is the parameter's name in camelCase, as Tauri reads
/// it, or as written under `rename_all = "snake_case"`.
fn parameters(cmd: &str) -> Vec<(String, String)> {
    let lib = include_str!("../lib.rs");
    let needle = format!("fn {cmd}(");
    let (start, attribute) = lib
        .match_indices(&needle)
        .filter_map(|(at, _)| {
            let attribute = lib[..at].rfind("#[tauri::command")?;
            (!lib[attribute..at].contains("fn ")).then_some((at, attribute))
        })
        .next()
        .unwrap_or_else(|| panic!("no #[tauri::command] fn {cmd}( in lib.rs"));
    let snake = lib[attribute..start].contains("rename_all = \"snake_case\"");
    let list = &lib[start + needle.len()..];
    let (mut parens, mut angles, mut from) = (0usize, 0usize, 0usize);
    let mut pieces = Vec::new();
    for (at, c) in list.char_indices() {
        match c {
            '(' => parens += 1,
            ')' if parens == 0 => {
                pieces.push(&list[from..at]);
                break;
            }
            ')' => parens -= 1,
            '<' => angles += 1,
            '>' => angles = angles.saturating_sub(1),
            ',' if parens == 0 && angles == 0 => {
                pieces.push(&list[from..at]);
                from = at + 1;
            }
            _ => {}
        }
    }
    pieces
        .into_iter()
        .filter_map(|piece| {
            let (name, ty) = piece.split_once(':')?;
            let (name, ty) = (
                name.trim(),
                ty.split_whitespace().collect::<Vec<_>>().join(" "),
            );
            let tauris = ["State<", "AppHandle", "Window", "WebviewWindow"];
            if tauris
                .iter()
                .any(|t| ty.trim_start_matches("tauri::").starts_with(t))
            {
                return None;
            }
            let key = if snake {
                name.to_string()
            } else {
                name.split('_')
                    .enumerate()
                    .map(|(n, part)| {
                        let mut chars = part.chars();
                        match (n, chars.next()) {
                            (0, _) | (_, None) => part.to_string(),
                            (_, Some(first)) => first.to_uppercase().chain(chars).collect(),
                        }
                    })
                    .collect()
            };
            Some((key, ty))
        })
        .collect()
}

#[test]
fn the_parameter_reader_reads_what_tauri_reads() {
    assert_eq!(
        parameters("js8_answer_reply"),
        vec![
            ("yes".to_string(), "bool".to_string()),
            ("display".to_string(), "String".to_string()),
            ("firesAtMs".to_string(), "u64".to_string()),
        ]
    );
    assert_eq!(
        parameters("winlink_connect"),
        vec![
            ("callsign".to_string(), "String".to_string()),
            ("password".to_string(), "String".to_string()),
        ]
    );
}

#[test]
fn each_off_half_reads_an_argument_its_command_takes() {
    let registered = registered();
    for (name, arg, off) in OFF_HALVES {
        assert!(registered.contains(name), "{name} is not registered");
        let wanted = match off {
            Off::False => "bool",
            Off::OneOf(_) => "String",
        };
        assert!(
            parameters(name).contains(&(arg.to_string(), wanted.to_string())),
            "{name} takes no `{arg}: {wanted}`: {:?}",
            parameters(name)
        );
    }
}

/// The transmit commands with an on/off-looking argument that have no off half, and why.
const NO_OFF_HALF: &[(&str, &str)] = &[
    (
        "js8_answer_reply",
        "Its No declines a reply that only a Yes would queue: nothing is on the air to stop.",
    ),
    (
        "sstv_send",
        "Whether the call goes in the picture is part of what is sent, not a switch.",
    ),
];

#[test]
fn every_transmit_command_with_a_switch_argument_has_its_off_half() {
    for name in of_class(Class::StartsTx) {
        let switch = parameters(name)
            .iter()
            .any(|(_, ty)| ty == "bool" || ty == "Option<bool>");
        let listed = OFF_HALVES.iter().any(|(row, ..)| *row == name)
            || NO_OFF_HALF.iter().any(|(row, _)| *row == name);
        assert!(
            !switch || listed,
            "{name} starts a transmission and takes a bool: give its off half a row in \
             OFF_HALVES, or say in NO_OFF_HALF why it has none"
        );
    }
    for (name, why) in NO_OFF_HALF {
        assert_eq!(class_of(name), Some(Class::StartsTx), "{name}: {why}");
    }
}

#[test]
fn every_store_command_is_a_registered_station_command() {
    let registered = registered();
    for (name, ..) in STORES {
        assert!(registered.contains(name), "{name}");
        assert_eq!(class_of(name), Some(Class::RunsAtShack), "{name}");
    }
    for window in STORES.windows(2) {
        assert!(window[0].0 < window[1].0, "{} out of order", window[1].0);
    }
}

/// The durable keys that belong to the computer the operator sits at: the UI scale's cap, the
/// rail's order, which modes the rail shows, and the main window's one-time layout switch. The main
/// window's pane layouts (`nexus.panels.<view>.main`) are durable by prefix and stay there too.
const OWN_KEYS: &[&str] = &[
    "nexus-ui-scale-cap",
    "nexus.navOrder",
    "nexus.features.v1",
    "nexus.connect.switch.main",
];

#[test]
fn every_durable_key_is_in_exactly_one_half_of_the_split() {
    let store = include_str!("../../../ui/src/features/durableStore.ts");
    let list = store
        .split_once("export const DURABLE_KEYS: readonly string[] = [")
        .and_then(|(_, rest)| rest.split_once("\n]"))
        .map(|(list, _)| list)
        .expect("durableStore.ts has its DURABLE_KEYS list");
    let durable: Vec<&str> = list
        .lines()
        .map(|line| line.split("//").next().unwrap_or("").trim())
        .filter_map(|line| line.strip_prefix('\'')?.split_once('\''))
        .map(|(key, _)| key)
        .collect();
    assert!(durable.len() >= 14, "read the list: {durable:?}");
    for key in &durable {
        let station = STATION_KEYS.contains(key);
        let own = OWN_KEYS.contains(key);
        assert!(
            station != own,
            "{key}: say in STATION_KEYS or OWN_KEYS which computer it belongs to"
        );
    }
    for key in STATION_KEYS.iter().chain(OWN_KEYS) {
        assert!(durable.contains(key), "{key} is not a durable key");
    }
    assert!(store.contains("key.startsWith('nexus.panels.') && key.endsWith('.main')"));
    assert!(!STATION_KEYS
        .iter()
        .any(|key| key.starts_with("nexus.panels.")));
}

// --- The shack's own stores ----------------------------------------------------------------------

#[test]
fn the_shacks_settings_revision_moves_with_a_change_and_only_with_one() {
    let engine: crate::SharedEngine = Arc::new(Mutex::new(tempo_app::engine::Engine::new(
        "KD9TAW", "EN52", 0,
    )));
    let stores = ShackStores {
        engine: Arc::clone(&engine),
    };
    let first = stores
        .revision(Store::Settings)
        .expect("the settings serialize");
    assert_eq!(first.as_str().len(), 64, "a SHA-256, in hex");
    assert_eq!(
        stores.revision(Store::Settings),
        Some(first.clone()),
        "nothing changed"
    );
    let mut changed = crate::engine_lock(&engine).settings().clone();
    changed.mycall = "W9XYZ".to_string();
    crate::engine_lock(&engine).apply_settings(changed);
    assert_ne!(stores.revision(Store::Settings), Some(first));
}

#[test]
fn a_stale_save_is_refused_against_the_shacks_real_settings() {
    let engine: crate::SharedEngine = Arc::new(Mutex::new(tempo_app::engine::Engine::new(
        "KD9TAW", "EN52", 0,
    )));
    let shack = Dispatcher::new(ShackStores {
        engine: Arc::clone(&engine),
    });
    let Admission::Run { ticket, .. } =
        shack.admit(&controller(), call(1, "get_settings", json!({})))
    else {
        panic!("admitted")
    };
    let copy = ticket
        .finish(Ok(json!({})))
        .revision
        .expect("a read answers with its revision");
    let fresh = shack.admit(
        &controller(),
        save(2, "set_settings", json!({}), copy.as_str()),
    );
    assert_eq!(kind(&fresh), "run", "made from the current copy");
    drop(fresh);
    // The shack's own screen saves a change.
    let mut changed = crate::engine_lock(&engine).settings().clone();
    changed.mycall = "W9XYZ".to_string();
    crate::engine_lock(&engine).apply_settings(changed);
    assert_eq!(
        kind(&shack.admit(
            &controller(),
            save(3, "set_settings", json!({}), copy.as_str())
        )),
        refused(Refusal::ChangedAtShack)
    );
}
