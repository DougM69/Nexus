//! The Operate strip's Call CQ, a JS8 message, the JS8 repeating CQ and automatic replies, and a
//! CW macro, pressed on a computer driving the station, on the roadside the road tests use: the shack's real channel, command lane, Stop road
//! and engine over loopback, and the app's own handlers reached as the main window reaches them
//! (`command_dispatch::tauri_runner`). The shack runs each with only the arguments the screen sent
//! and the stand-down the lane wrote, and each is ended by the link's Stop alone, by the road's
//! Stop alone, and by presence lapsing with the lease still held.
use super::*;
use crate::command_dispatch::{tauri_runner, Runner};

/// A JS8 message long enough to need several frames, so one is on the air with more still to go.
const LONG: &str = "A MESSAGE FROM AFAR LONG ENOUGH TO NEED SEVERAL FRAMES AT NORMAL SPEED";

/// The overs: the Operate strip's Call CQ (a CQ run), a JS8 message once its first frame is
/// keyed, the JS8 repeating CQ switched on, the automatic replies switched on with a reply asking,
/// a yes to a reply, and a CW macro on the keyer.
const OVERS: [&str; 6] = [
    "cq-run",
    "js8",
    "js8-cq-repeat",
    "js8-autoreply",
    "js8-reply",
    "cw-macro",
];

/// The ways an over ends: the link's Stop alone (the road's datagrams never sent), the road's Stop
/// alone (the link no longer read), and presence lapsing while the link keeps the lease.
const ENDS: [&str; 3] = ["link", "road", "presence"];

/// The calls the screen makes for each over, after the shack's own set-up: what it sends, never a
/// time.
fn presses(over: &str) -> Vec<(&'static str, Value)> {
    match over {
        "cq-run" => vec![("start_running", json!({}))],
        "js8" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("js8_send", json!({"to": null, "text": LONG})),
        ],
        "js8-cq-repeat" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("js8_cq_repeat", json!({"on": true})),
        ],
        "js8-autoreply" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("js8_arm", json!({"which": "autoreply", "on": true})),
        ],
        // The reply is named as the shack shows it, once it asks (filled in by the test).
        "js8-reply" => vec![
            ("set_tx_enabled", json!({"enabled": true})),
            ("js8_answer_reply", json!({"yes": true})),
        ],
        "cw-macro" => vec![("send_cw", json!({"text": "CQ {MYCALL} {MYCALL} TEST"}))],
        _ => unreachable!(),
    }
}

/// Each call the shack's handlers got, with its arguments, in order.
type Got = Arc<Mutex<Vec<(String, Value)>>>;

/// The shack's handlers for these commands, the app's own, on the roadside's engine, reached as
/// the main window reaches them. Every call the lane hands over is recorded with its arguments as
/// the handler got them; the snapshot feed's polls are answered here, as the other road tests do.
fn the_apps_handlers(r: &mut Roadside) -> (crate::tests::ShackApp, Got) {
    let (app, main) = crate::tests::shack_app_for_starts_from_afar(r.s.shared.engine.clone());
    let runner: Runner = tauri_runner(app.handle().clone());
    let got = Arc::new(Mutex::new(Vec::new()));
    let record = got.clone();
    r.s.shared.commands = super::super::super::commands::Commands::new(Arc::new(
        move |cmd: String, args: Value, answer: crate::command_dispatch::Answer| {
            if cmd == "get_snapshot" {
                return answer(Ok(json!({})));
            }
            record.lock().unwrap().push((cmd.clone(), args.clone()));
            runner(cmd, args, answer)
        },
    ));
    ((app, main), got)
}

/// The station as the shack's own screen leaves it before the computer presses anything: FT8, or
/// the JS8 tier; TX off.
fn set_up_at_the_shack(e: &mut tempo_app::engine::Engine, over: &str) {
    if over.starts_with("js8") {
        let mut settings = e.settings().clone();
        settings.js8_hb_networking = true;
        // Off at the shack, so turning them on from afar is seen.
        settings.js8_autoreply = over != "js8-autoreply";
        e.apply_settings(settings);
        e.js8_enter();
    } else {
        e.set_tier(tempo_app::dto::Tier::Ft8);
    }
    e.set_tx_enabled(false);
    e.take_slot_tx_abort();
}

/// Whether the over is on the air as the shack keys it: the run armed and calling; the message's
/// first frame keyed by the JS8 sender at its period with more still queued; the repeating CQ
/// armed to key at its period; the automatic replies armed, with a reply asking; the
/// reply queued; the macro holding the transmitter on the keyer.
fn on_the_air(e: &mut tempo_app::engine::Engine, over: &str) -> bool {
    match over {
        "cq-run" => e.tx_enabled(),
        "js8" => {
            let now = tempo_core::timing::now_unix_ms() as u64 / 1000 / 15;
            let keyed = (now..now + 3).any(|slot| !e.poll_tx(slot).is_empty());
            keyed && !e.js8_state().queue.is_empty()
        }
        "js8-cq-repeat" => e.js8_state().armed.cq,
        "js8-autoreply" => {
            let js8 = e.js8_state();
            js8.armed.autoreply && js8.pending_reply.is_some()
        }
        "js8-reply" => !e.js8_state().queue.is_empty(),
        "cw-macro" => e.tx_owner().is_some(),
        _ => unreachable!(),
    }
}

/// What every end leaves, as the shack's own Stop TX leaves it: the transmitter free, TX off, an
/// over in flight cut, the keyer aborted, and for JS8 the queue, the heartbeat and repeating CQ
/// schedules and any reply asking dropped and nothing armed, so nothing keys once TX is armed
/// again.
fn assert_over(e: &mut tempo_app::engine::Engine, over: &str, end: &str) {
    let what = format!("{over}, ended by the {end}");
    assert_stopped(e, if over == "cw-macro" { "cw" } else { "ft" });
    if over.starts_with("js8") {
        let js8 = e.js8_state();
        assert!(js8.queue.is_empty(), "{what}: the queue dropped");
        assert!(!js8.hb_on && !js8.cq_on, "{what}: the schedules ended");
        assert!(js8.pending_reply.is_none(), "{what}: no reply asking");
        let armed = js8.armed;
        assert!(
            !(armed.hb || armed.cq || armed.autoreply || armed.relay || armed.hb_ack),
            "{what}: nothing armed: {armed:?}"
        );
        e.set_tx_enabled(true);
        let now = tempo_core::timing::now_unix_ms() as u64 / 1000 / 15;
        for slot in now..now + 5 {
            assert!(e.poll_tx(slot).is_empty(), "{what}: keyed again at {slot}");
        }
        e.set_tx_enabled(false);
    }
}

/// ⛔ An over started from afar ends by each road alone and by presence. For the Operate strip's
/// Call CQ, a JS8 message, the JS8 repeating CQ and automatic replies switched on, a yes to a JS8
/// reply, and a CW macro: the computer presses it, the shack's own handler runs it
/// and the station keys it; then the link's Stop alone, the road's Stop alone, or presence lapsing
/// (the heartbeats stop, the lease still kept) ends it, as the shack's own Stop TX would. The shack
/// got only what the screen sent, and `afar` with the stand-down alone: no time crossed. CONTROL:
/// each over is on the air before it ends, and each press was answered with no refusal.
#[test]
fn an_over_started_from_afar_ends_by_either_road_alone_and_by_presence() {
    let _alone = alone();
    on_a_runtime(2, async {
        for over in OVERS {
            for end in ENDS {
                let what = format!("{over}, ended by the {end}");
                let app = App::new();
                let mut r = roadside(&app);
                let (_handlers, got) = the_apps_handlers(&mut r);
                set_up_at_the_shack(&mut r.s.shared.engine.lock().unwrap(), over);
                let mut seat = seated(&r).await;
                seat.renewed().await;
                let mut presses = presses(over);
                for (n, (cmd, args)) in presses.iter_mut().enumerate() {
                    if *cmd == "js8_answer_reply" {
                        // A query heard at the shack; its reply asks, and the yes names it.
                        let mut e = r.s.shared.engine.lock().unwrap();
                        e.js8_api_hear_snr_query("W1AW");
                        let reply = e.js8_state().pending_reply.expect("premise: a reply asks");
                        args["display"] = json!(reply.display);
                        args["firesAtMs"] = json!(reply.fires_at_ms);
                    }
                    let said = seat.call(n as u64 + 1, cmd, args.clone()).await;
                    assert!(
                        said.get("refused").is_none() && said.get("error").is_none(),
                        "{what}: {cmd} {said}"
                    );
                }
                if over == "js8-autoreply" {
                    // A query heard at the shack once the automatic replies are on.
                    r.s.shared
                        .engine
                        .lock()
                        .unwrap()
                        .js8_api_hear_snr_query("W1AW");
                }
                assert!(
                    on_the_air(&mut r.s.shared.engine.lock().unwrap(), over),
                    "{what}: premise, on the air"
                );
                // No time crossed: each press reached the handler with what the screen sent and
                // the stand-down the lane wrote, nothing else.
                let got = got.lock().unwrap().clone();
                for (cmd, args) in &presses {
                    let (_, reached) = got
                        .iter()
                        .find(|(name, _)| name == cmd)
                        .unwrap_or_else(|| panic!("{what}: {cmd} never reached the shack"));
                    let mut sent = args.as_object().unwrap().clone();
                    let afar = reached["afar"].clone();
                    sent.insert("afar".into(), afar.clone());
                    assert_eq!(reached, &Value::Object(sent), "{what}: {cmd}");
                    let keys: Vec<&String> = afar.as_object().unwrap().keys().collect();
                    assert_eq!(keys, ["standDown"], "{what}: {cmd} {afar}");
                }
                match end {
                    "link" => {
                        // What the cockpit's Stop TX sends: CW's stops the keyer, then halts.
                        let kind = if over == "cw-macro" { "cw" } else { "ft" };
                        for (n, stop) in cockpit_stop(kind).into_iter().enumerate() {
                            let said = seat.call(90 + n as u64, stop, json!({})).await;
                            assert!(said.get("error").is_none(), "{what}: {said}");
                        }
                    }
                    "road" => {
                        seat.valve.shut();
                        assert!(
                            seat.road
                                .road_stop(1, Duration::from_secs(2))
                                .await
                                .is_some(),
                            "{what}: the Stop acknowledged"
                        );
                        assert!(came_to_a_stop(&r.s).await, "{what}");
                    }
                    _ => {
                        let renewed = Instant::now();
                        while renewed.elapsed() < PRESENCE + Duration::from_millis(300) {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            seat.keep_the_lease().await;
                        }
                        // The radio loop's tick, which carries out a lapse.
                        r.s.shared
                            .engine
                            .lock()
                            .unwrap()
                            .poll_remote_transmit(Instant::now());
                    }
                }
                assert_over(&mut r.s.shared.engine.lock().unwrap(), over, end);
            }
        }
    });
}
