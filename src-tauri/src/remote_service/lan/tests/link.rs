//! The computer's end of protocol 2 (`crate::lan_client::link`) against the shack's real channel
//! behind a loopback port: the link opens only on the same build; runs the shack's commands and
//! keeps the computer's own on the computer; sends a Stop ahead of a big call; answers a call gone
//! when it ends, and the next link asks after it; keeps each feed's newest value; and hears the
//! station's events.
use super::super::wire;
use super::commands::{running, App, LetGo};
use super::computer::{behind_a_port, paired_record};
use super::*;
use crate::command_dispatch::{Outcome, Refusal};
use crate::lan_client::link::{Answer, Link};

/// Another build's contract.
const ANOTHER: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// One of the lease's operation requests, asked again while the station is busy for a moment.
async fn operate(link: &Link, request: Value) -> Value {
    for _ in 0..100 {
        let mut request = request.clone();
        request["requestId"] = json!(id());
        let answer = link.operation(request).await.expect("the link carries it");
        if answer["error"] != "stationBusy" && answer["error"] != "controllerBusy" {
            return answer;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the station stayed busy")
}

/// The station's lease, taken through the link. (Each link below is given its loopback address as
/// typed: remembered addresses are tried only when they are private ones.)
async fn in_control(link: &Link) {
    let state = operate(link, json!({"type":"state"})).await;
    let boot = state["value"]["stationBootId"].clone();
    let acquired = operate(link, json!({"type":"acquire","stationBootId":boot})).await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
}

/// A shack running on `app`, listening behind a loopback port, with its computer allowed control:
/// the shack, the port, and what keeps it listening.
async fn shack_on(app: &App) -> (Shack, SocketAddrV4, watch::Sender<bool>) {
    let s = running(app);
    s.shared.authority.permit_station(&s.device, true).unwrap();
    let (stopper, stop) = watch::channel(false);
    let at = behind_a_port(&s, stop).await;
    (s, at, stopper)
}

/// What kind of answer it is, for a message that must not print a 12 MB value.
fn kind(answer: &Answer) -> &'static str {
    match answer {
        Answer::Value { .. } => "value",
        Answer::Error(_) => "error",
        Answer::Refused(_) => "refused",
        Answer::Before(_) => "before",
        Answer::TooLarge => "tooLarge",
        Answer::Gone => "gone",
    }
}

/// Waits up to five seconds for `done`.
async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// ★ The link, end to end on this build: it opens, takes the lease through the operation requests,
/// runs the shack's commands with their answers and revisions, carries a call bigger than a frame
/// both ways, and runs a Stop; a command that runs on this computer or is refused here, a typed
/// password and a call too big to send are answered here and never reach the shack. CONTROL: the
/// shack ran exactly what was sent.
#[tokio::test]
async fn the_link_runs_the_shacks_commands_and_keeps_this_computers_here() {
    let app = App::new();
    let (s, at, _listening) = shack_on(&app).await;
    let (link, reached) = Link::connect(&paired_record(&s, at), Some(at), 41, None)
        .await
        .expect("opened");
    assert_eq!(reached, at);
    assert_eq!(
        (link.ids.station.as_str(), link.ids.device.as_str()),
        (STATION, s.device.as_str())
    );
    in_control(&link).await;
    let (_, read) = link.call("get_settings", json!({}), None, 0).await;
    let Answer::Value { value, revision } = read else {
        panic!("{read:?}")
    };
    assert_eq!(value, json!({"settings":"as read"}));
    assert_eq!(revision.map(|revision| revision.len()), Some(64));
    for (cmd, args, refusal) in [
        ("check_for_update", json!({}), Refusal::RunsHere),
        ("repair_clock", json!({}), Refusal::ShackOnly),
        ("no_such_command", json!({}), Refusal::Unclassed),
        (
            "winlink_connect",
            json!({"password":"typed"}),
            Refusal::TypedPassword,
        ),
    ] {
        assert_eq!(
            link.call(cmd, args, None, 0).await.1,
            Answer::Refused(refusal),
            "{cmd}"
        );
    }
    let big = "x".repeat(100_000);
    assert_eq!(
        link.call("add_radio", json!({"name": big}), None, 0)
            .await
            .1,
        Answer::Value {
            value: json!({"ran":"add_radio","args":{"name":big}}),
            revision: None
        }
    );
    let too_big = "x".repeat(wire::CALL_BYTES);
    assert_eq!(
        link.call("add_radio", json!({"name": too_big}), None, 0)
            .await
            .1,
        Answer::TooLarge
    );
    assert!(matches!(
        link.call("halt_tx", json!({}), None, 0).await.1,
        Answer::Value { .. }
    ));
    assert_eq!(app.ran(), ["get_settings", "add_radio", "halt_tx"]);
}

/// ★ The same build, at this end: a station of another build is refused before this computer
/// opens anything, saying which side to update (a newer station: this computer; an older one: the
/// station; another build of the same version: `otherBuild`), and nothing runs there. CONTROL:
/// this build opens.
#[tokio::test]
async fn the_link_refuses_a_station_of_another_build() {
    let app = App::new();
    for (build, said) in [
        (("999.0.0", wire::contract()), "updateComputer"),
        (("0.0.1", wire::contract()), "updateStation"),
        ((wire::APP, ANOTHER), "otherBuild"),
    ] {
        let mut s = running(&app);
        s.shared.build = build;
        let (_listening, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        let refused = Link::connect(&paired_record(&s, at), Some(at), 41, None)
            .await
            .err();
        assert_eq!(refused, Some(said), "{build:?}");
    }
    assert!(app.ran().is_empty(), "{:?}", app.ran());
    let (s, at, _listening) = shack_on(&app).await;
    assert!(Link::connect(&paired_record(&s, at), Some(at), 41, None)
        .await
        .is_ok());
}

/// ★ Stop ahead, at this end: a Stop asked for while a 12 MB call is still going out in pieces goes
/// before the rest of them, so the shack runs it first. CONTROL: the big call then runs, whole.
#[tokio::test]
async fn the_links_stop_goes_out_ahead_of_a_big_call() {
    let app = App::new();
    let (s, at, _listening) = shack_on(&app).await;
    let (link, _) = Link::connect(&paired_record(&s, at), Some(at), 41, None)
        .await
        .unwrap();
    in_control(&link).await;
    let link = Arc::new(link);
    let big = "x".repeat(12 << 20);
    let sending = {
        let (link, big) = (link.clone(), big.clone());
        tokio::spawn(async move {
            link.call("add_radio", json!({"name": big}), None, 0)
                .await
                .1
        })
    };
    tokio::task::yield_now().await;
    assert!(matches!(
        link.call("halt_tx", json!({}), None, 0).await.1,
        Answer::Value { .. }
    ));
    let sent = sending.await.unwrap();
    assert_eq!(kind(&sent), "value");
    assert_eq!(app.ran(), ["halt_tx", "add_radio"]);
}

/// ★ A link that ends with a call running answers it gone, and the next link of the same run asks
/// after it: running while it runs, done once it has. A link of another run knows nothing of it.
/// CONTROL: the call ran, once.
#[tokio::test]
async fn a_call_cut_off_is_asked_after_on_the_next_link() {
    let app = App::holding();
    let _let_go = LetGo(app.clone());
    let (s, at, _listening) = shack_on(&app).await;
    let record = paired_record(&s, at);
    let (link, _) = Link::connect(&record, Some(at), 77, None).await.unwrap();
    in_control(&link).await;
    let link = Arc::new(link);
    let calling = {
        let link = link.clone();
        tokio::spawn(async move { link.call("ask_log", json!({}), None, 0).await })
    };
    eventually("the call runs", || app.ran() == ["ask_log"]).await;
    link.close();
    tokio::time::timeout(Duration::from_secs(1), link.closed())
        .await
        .expect("closed at once");
    let (id, gone) = calling.await.unwrap();
    assert_eq!(gone, Answer::Gone);
    let (next, _) = Link::connect(&record, Some(at), 77, None).await.unwrap();
    in_control(&next).await;
    assert_eq!(next.outcomes(&[id]).await, Some(vec![Outcome::Running]));
    app.release();
    let deadline = Instant::now() + Duration::from_secs(5);
    while next.outcomes(&[id]).await != Some(vec![Outcome::Done]) {
        assert!(Instant::now() < deadline, "never done");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    next.close();
    let (other, _) = Link::connect(&record, Some(at), 78, None).await.unwrap();
    assert_eq!(other.outcomes(&[id]).await, Some(vec![Outcome::Unknown]));
    assert_eq!(app.ran(), ["ask_log"]);
}

/// ★ Feeds at this end: the link keeps each watched feed's newest value with its number, and the
/// station's answer to the watch names the feeds it has and the names it has none for. CONTROL:
/// the numbers keep rising while it is watched, and the link stays up.
#[tokio::test]
async fn the_link_keeps_each_feeds_newest_value() {
    let app = App::new();
    let (s, at, _listening) = shack_on(&app).await;
    let (link, _) = Link::connect(&paired_record(&s, at), Some(at), 41, None)
        .await
        .unwrap();
    in_control(&link).await;
    assert!(link.watch(&["snapshot/1", "noSuchFeed/1"]));
    eventually("three snapshots", || {
        link.latest("snapshot/1")
            .is_some_and(|(number, _)| number >= 3)
    })
    .await;
    let (number, value) = link.latest("snapshot/1").unwrap();
    assert!(value["n"].as_u64().is_some(), "{value}");
    eventually("a newer one", || {
        link.latest("snapshot/1")
            .is_some_and(|(newer, _)| newer > number)
    })
    .await;
    assert_eq!(
        link.watching(),
        Some(json!({"type":"watching","feeds":["snapshot/1"],"unknown":["noSuchFeed/1"]}))
    );
    assert!(!link.ended());
}

/// ★ Events at this end: a pounce raised at the station arrives, with its payload as raised.
#[tokio::test]
async fn the_link_hears_the_stations_events() {
    let app = App::new();
    let (s, at, _listening) = shack_on(&app).await;
    let (link, _) = Link::connect(&paired_record(&s, at), Some(at), 41, None)
        .await
        .unwrap();
    in_control(&link).await;
    let mut events = link.events();
    s.shared
        .commands
        .raised("alert", json!({"call":"VP8PJ"}).to_string());
    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("an event in time")
        .expect("an event");
    assert_eq!(
        (event.name.as_str(), event.data),
        ("alert", json!({"call":"VP8PJ"}))
    );
}

/// ★ No Nagle at the computer's end: its socket sends each frame as it is written, so a Stop never
/// waits for the acknowledgement of the frame before it (about 40 ms, measured).
#[tokio::test]
async fn the_computers_socket_sends_each_frame_at_once() {
    let app = App::new();
    let (s, at, _listening) = shack_on(&app).await;
    let reached = crate::lan_client::road::reach(&paired_record(&s, at), &[at])
        .await
        .expect("welcomed");
    assert_eq!(
        reached.socket.get_ref().get_ref().0.nodelay().ok(),
        Some(true)
    );
}

/// ★ This run of Nexus has one id, never 0, the same however often it is asked: the journal at the
/// shack keys this run's calls on it.
#[test]
fn this_run_has_one_id() {
    let run = crate::lan_client::link::run_id();
    assert_ne!(run, 0);
    assert_eq!(crate::lan_client::link::run_id(), run);
}

/// ★ A call carries its count of Stops from this computer's link to the shack's reader, by value: a
/// start's count, a Stop's own number, the largest number there is, and a whole save's revision
/// beside it. A call that names no count reads as 0, older than any Stop. CONTROL: a message the
/// shack would close the connection over reads as no call at all.
#[test]
fn a_calls_count_of_stops_crosses_the_link_as_written() {
    use super::super::channel::read_call;
    use crate::lan_client::link::call_message;
    for (stops, base) in [(0, None), (3, None), (u64::MAX, Some("7-1"))] {
        let text = call_message(7, "call_cq", json!({}), base, stops).to_string();
        assert_eq!(read_call(&text), Some((7, stops)), "{text}");
    }
    let unnumbered = json!({"type":"call","id":8,"cmd":"halt_tx","args":{}}).to_string();
    assert_eq!(read_call(&unnumbered), Some((8, 0)));
    let garbled = json!({"type":"call","id":9,"cmd":"halt_tx","args":{},"stops":"two"}).to_string();
    assert_eq!(read_call(&garbled), None);
}
