//! Logging-only authority, durable outcomes and exact pending-contact identity.
use super::*;
use tempo_app::engine::remote_transmit::FtExchangeContext;

fn run(f: &Fixture, request: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, 4),
        SESSION,
        DEVICE,
        request,
        &f.engine,
        f.now(),
    )
}

fn acquire(f: &Fixture) -> Value {
    f.authority.permit(DEVICE, true).unwrap();
    let state = control_state_version(f, f.now(), 4);
    run(
        f,
        &Request::Acquire {
            request_id: id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        },
    )
    .unwrap()
}

fn hold(f: &Fixture) -> String {
    let state = acquire(f);
    assert_eq!(state["phase"], "controlling");
    let record: tempo_app::dto::LoggedQso = serde_json::from_value(json!({"call":"W1AW","grid":"FN31",
        "country":null,"state":"CT","band":"20m","freqMhz":14.25,"mode":"SSB","rstSent":"59",
        "rstRcvd":"57","name":"Joe","qth":"Newington","comment":"Remote test","notes":"Keep this note",
        "whenUnix":crate::remote_service::now_ms()/1000,"confirmed":false,"awardConfirmed":false}))
    .unwrap();
    let mut e = f.engine.lock().unwrap();
    e.set_pending_qso_path(f.dir.join("pending.json"));
    e.load_pending_qso(record.into());
    e.persist_pending_qso();
    e.pending_qso_log_key().unwrap()
}

fn confirm(key: &str) -> Value {
    json!({"action":"qso.confirm","expectedKey":key,"edits":{"call":"W1AW","grid":"FN32","rstSent":"59","rstRcvd":"58"}})
}

fn command(f: &Fixture, action: Value) -> Request {
    control_request(&control_state_version(f, f.now(), 4), action)
}

#[test]
fn pending_confirm_requires_logging_not_radio_or_transmit_permission() {
    let f = Fixture::new();
    let key = hold(&f);
    let state = control_state_version(&f, f.now(), 4);
    assert_eq!(state["controls"]["capabilities"], json!(["qsoLogging"]));
    assert!(state["transmitEpoch"].is_null());
    assert!(!f.engine.lock().unwrap().tx_enabled());
    let request = control_request(&state, confirm(&key));
    for version in [1, 2, 3] {
        assert_eq!(
            f.authority.handle_version(
                (f.connection, version),
                SESSION,
                DEVICE,
                &request,
                &f.engine,
                f.now()
            ),
            Err("stationUnsupported")
        );
    }
    let before = f.engine.lock().unwrap().settings().clone();
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(result["evidence"], "fileSynced");
    // A previous browser cannot parse v4 persistence evidence as its older
    // station-control result. Refuse the query explicitly, without replay.
    for version in [2, 3] {
        assert_eq!(
            f.authority.handle_version(
                (f.connection, version),
                SESSION,
                DEVICE,
                &Request::Result {
                    request_id: id(),
                    operation_id: request.id().into()
                },
                &f.engine,
                f.now()
            ),
            Err("stationUnsupported")
        );
    }
    let bytes = std::fs::read(f.dir.join("contacts.adi")).unwrap();
    assert!(!f.dir.join("pending.json").exists());
    assert_eq!(run(&f, &request).unwrap(), result);
    assert_eq!(
        run(
            &f,
            &Request::Result {
                request_id: id(),
                operation_id: request.id().into()
            }
        )
        .unwrap(),
        result
    );
    assert_eq!(std::fs::read(f.dir.join("contacts.adi")).unwrap(), bytes);
    let mut e = f.engine.lock().unwrap();
    assert!(e.pending_qso_log_key().is_none());
    assert_eq!(e.stored_log().len(), 1);
    let record = &e.stored_log()[0];
    assert_eq!(record.grid.as_deref(), Some("FN32"));
    assert_eq!(record.rst_rcvd.as_deref(), Some("58"));
    assert_eq!(record.name.as_deref(), Some("Joe"));
    assert_eq!(record.notes.as_deref(), Some("Keep this note"));
    assert_eq!(e.take_pending_uploads().len(), 1);
    assert_eq!(
        serde_json::to_value(e.settings()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert!(!e.tx_enabled());
}

#[test]
fn radio_permission_cannot_confirm_or_discard_a_contact() {
    for action in ["confirm", "discard"] {
        let f = Fixture::new();
        let key = hold(&f);
        f.authority.permit(DEVICE, false).unwrap();
        let state = acquire_controls_version(&f, f.now(), 4);
        assert!(!state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("qsoLogging")));
        let action = if action == "confirm" {
            confirm(&key)
        } else {
            json!({"action":"qso.discard","expectedKey":key})
        };
        assert_eq!(
            run(&f, &control_request(&state, action)),
            Err("localPermissionRequired")
        );
        assert_eq!(
            f.engine.lock().unwrap().pending_qso_log_key().as_deref(),
            Some(key.as_str())
        );
        assert!(f.engine.lock().unwrap().stored_log().is_empty());
        assert!(f.dir.join("pending.json").exists());
    }
}

#[test]
fn replacing_a_hold_with_identical_fields_refuses_both_old_gestures() {
    for action in ["confirm", "discard"] {
        let f = Fixture::new();
        let key = hold(&f);
        let action = if action == "confirm" {
            confirm(&key)
        } else {
            json!({"action":"qso.discard","expectedKey":key})
        };
        let request = command(&f, action);
        let new_key = {
            let mut e = f.engine.lock().unwrap();
            let record = e.pending_log_identity().unwrap().record().clone();
            let key = e.pending_qso_log_key().unwrap_or_default();
            assert!(e.discard_pending_log(&key));
            e.load_pending_qso(record);
            e.persist_pending_qso();
            e.pending_qso_log_key().unwrap()
        };
        assert_ne!(new_key, key);
        let result = run(&f, &request).unwrap();
        assert_eq!(result["outcome"], "rejected");
        assert_eq!(result["reason"], "contextChanged");
        assert_eq!(
            f.engine.lock().unwrap().pending_qso_log_key(),
            Some(new_key)
        );
        assert!(f.engine.lock().unwrap().stored_log().is_empty());
        assert!(f.dir.join("pending.json").exists());
    }
}

#[test]
fn a_failed_append_retains_pending_record_and_journal_without_replaying() {
    let f = Fixture::new();
    let key = hold(&f);
    let journal = std::fs::read(f.dir.join("pending.json")).unwrap();
    let blocked = f.dir.join("not-a-log-file");
    std::fs::create_dir(&blocked).unwrap();
    f.engine.lock().unwrap().set_log_path(blocked);
    let request = command(&f, confirm(&key));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "unknown");
    assert_eq!(result["reason"], "persistenceFailed");
    assert_eq!(run(&f, &request).unwrap(), result);
    assert_eq!(std::fs::read(f.dir.join("pending.json")).unwrap(), journal);
    assert_eq!(f.engine.lock().unwrap().pending_qso_log_key(), Some(key));
}

#[test]
fn confirmation_sync_releases_engine_and_never_clears_a_newer_hold() {
    let mut f = Fixture::new();
    let key = hold(&f);
    let engine = f.engine.clone();
    f.authority.before_sync = Some(Box::new(move || {
        let mut e = engine
            .try_lock()
            .expect("disk wait must release the engine");
        let record = e.pending_log_identity().unwrap().record().clone();
        let key = e.pending_qso_log_key().unwrap_or_default();
        assert!(e.discard_pending_log(&key));
        e.load_pending_qso(record);
        e.persist_pending_qso();
    }));
    let request = command(&f, confirm(&key));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "applied");
    assert_eq!(result["evidence"], "fileSynced");
    assert_ne!(f.engine.lock().unwrap().pending_qso_log_key().unwrap(), key);
    assert!(f.dir.join("pending.json").exists());
    assert_eq!(f.engine.lock().unwrap().stored_log().len(), 1);
}

/// The confirm-before-log popup, answered from a browser, against the STORE-owned log: the
/// contact is in the store before the hold is cleared (the proof the clear waits on is the
/// store's commit), and the answer is the one every page reads. The engine is free throughout
/// the wait — the same `before_sync` check as the 1.13 path's test above.
#[test]
fn a_remote_confirm_is_in_the_store_before_the_hold_is_cleared() {
    let mut f = Fixture::with_store();
    let key = hold(&f);
    let engine = f.engine.clone();
    f.authority.before_sync = Some(Box::new(move || {
        assert!(
            engine.try_lock().is_ok(),
            "the durable wait must release the engine"
        );
    }));
    let request = command(&f, confirm(&key));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["outcome"], "applied", "{result}");
    assert_eq!(result["evidence"], "fileSynced");
    assert!(
        f.stored()
            .iter()
            .any(|r| r.call == "W1AW" && r.grid.as_deref() == Some("FN32")),
        "the confirmed contact is in the store"
    );
    assert!(
        f.engine.lock().unwrap().pending_qso_log_key().is_none(),
        "and the hold is gone"
    );
}

#[test]
fn discard_is_durable_and_duplicate_receipts_do_not_discard_a_later_contact() {
    let f = Fixture::new();
    let key = hold(&f);
    let request = command(&f, json!({"action":"qso.discard","expectedKey":key}));
    let result = run(&f, &request).unwrap();
    assert_eq!(result["evidence"], "pendingDiscarded");
    assert!(!f.dir.join("pending.json").exists());
    let newer = hold(&f);
    assert_eq!(run(&f, &request).unwrap(), result);
    assert_eq!(f.engine.lock().unwrap().pending_qso_log_key(), Some(newer));
    assert!(f.dir.join("pending.json").exists());
    assert!(f.engine.lock().unwrap().stored_log().is_empty());
}

#[test]
fn current_log_checks_native_eligibility_and_contact_incarnation() {
    for replace in [false, true] {
        let f = Fixture::new();
        acquire(&f);
        let action = {
            let mut e = f.engine.lock().unwrap();
            e.call_station("W1AW");
            e.set_tx_enabled(false);
            let action = json!({"action":"qso.logCurrent","expectedKey":e.current_qso_log_key().unwrap(),"expectedTier":e.tier(),
                "expectedQso":FtExchangeContext::from(&e.snapshot().qso.unwrap())});
            if replace {
                e.call_station("W1AW");
                e.set_tx_enabled(false);
            }
            action
        };
        let result = run(&f, &command(&f, action)).unwrap();
        assert_eq!(result["outcome"], "rejected");
        assert_eq!(
            result["reason"],
            if replace {
                "contextChanged"
            } else {
                "noEligibleContact"
            }
        );
        assert!(f.engine.lock().unwrap().stored_log().is_empty());
        assert!(!f.engine.lock().unwrap().tx_enabled());
    }
}

// ---- The shack's own log commands find the row the view showed by its content ------------

const SEED: &str = "<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<FREQ:6>14.074<QSO_DATE:8>20260909<TIME_ON:6>010000<NAME:4>Joe <EOR>\n\
<CALL:5>K1ABC<BAND:3>40m<MODE:2>CW<FREQ:5>7.030<QSO_DATE:8>20260909<TIME_ON:6>020000<EOR>\n";

fn seed(f: &Fixture) {
    f.engine.lock().unwrap().import_adif(SEED);
    assert_eq!(f.engine.lock().unwrap().stored_log().len(), 2);
    // In the file, as 1.13's import had written it before it returned: on the 1.13 path the
    // file is written on its own lane since SPEC-2 v3 C19 (D1-A).
    let _ = written(f);
}

fn adif(f: &Fixture) -> Vec<u8> {
    std::fs::read(f.dir.join("contacts.adi")).unwrap()
}

/// ★ THE SHACK'S COMMANDS, SPLIT THE SAME WAY: the desktop's log commands find the row the view
/// showed in the store with the engine lock released (`find_seen`), and change it only while it
/// is still the version found (`change_found`: planned with the lock released, checked and made
/// under it). A row no contact holds any more is refused in the words the view always showed,
/// whether it changed before the search or between the search and the change; a store whose
/// writer is still behind is refused as unread, nothing changed. The control: once the write
/// lands, the row is found.
#[test]
fn the_shacks_commands_find_their_row_off_the_lock_and_check_it_under_it() {
    use crate::remote_service::log_tests::parse_one;
    use tempo_core::logbook::sqlite::WriteHold;
    let f = Fixture::with_store();
    seed(&f);
    let seen = |call: &str| {
        let e = f.engine.lock().unwrap();
        let row = e.stored_log().into_iter().find(|r| r.call == call);
        crate::logged_qso(&row.unwrap())
    };
    let card = |id| vec![tempo_core::logbook::LogOp::MarkQslCard { id, received: true }];
    let comment = |text: &str| {
        let mut e = f.engine.lock().unwrap();
        let mut r = e
            .stored_log()
            .iter()
            .find(|r| r.call == "K1ABC")
            .unwrap()
            .as_ref()
            .clone();
        r.comment = Some(text.into());
        assert!(e.update_qso(r.id.unwrap(), r));
    };

    let held = seen("K1ABC");
    let found = crate::find_seen(&f.engine, &held).expect("the row the view shows");
    assert_eq!(
        Some(found.id.clone()),
        held.id,
        "the contact the view shows"
    );
    let (changed, _) = crate::change_found(&f.engine, &found, "test", card);
    let changed = changed.expect("still that contact").expect("in the log");
    assert_eq!(changed.id.map(|id| id.to_string()), held.id);

    comment("changed before the search");
    // The change is in the store before the search: on a loaded machine its write can outlast
    // the search's own wait, which is then refused as unread.
    let _ = written(&f);
    assert_eq!(
        crate::find_seen(&f.engine, &held).unwrap_err(),
        crate::LOG_ROW_GONE
    );

    let held = seen("K1ABC");
    let found = crate::find_seen(&f.engine, &held).expect("the row as it is now");
    comment("changed between the search and the change");
    assert_eq!(
        crate::change_found(&f.engine, &found, "test", card)
            .0
            .unwrap_err(),
        crate::LOG_ROW_GONE
    );

    let held = seen("W1AW");
    let hold = WriteHold::take(&f.dir.join("contacts.sqlite3")).unwrap();
    f.engine.lock().unwrap().log_qso(parse_one(
        "<CALL:5>N3CCC<BAND:3>20m<MODE:3>SSB<QSO_DATE:8>20260911<TIME_ON:6>010000<EOR>",
    ));
    let unread = crate::find_seen(&f.engine, &held);
    drop(hold);
    assert_eq!(unread.unwrap_err(), crate::LOG_UNREAD);
    // "Once written" is the control's premise, so wait for the write: letting the hold go only
    // lets the writer start.
    let _ = written(&f);
    assert!(
        crate::find_seen(&f.engine, &held).is_ok(),
        "control: once written, found"
    );
    written(&f);
}

/// What the log's file holds once everything submitted is written — the store's writer and its
/// `log.adi` mirror flushed, or on the 1.13 path the lane that writes `log.adi` (SPEC-2 v3 C19,
/// D1-A) — so a comparison of the file is not a race with either, and the fixture's folder can
/// go.
fn written(f: &Fixture) -> Vec<u8> {
    f.engine
        .lock()
        .unwrap()
        .flush_log_store(std::time::Duration::from_secs(60))
        .expect("written");
    adif(f)
}
