//! The room's tunnel at the shack (`crate::remote_service::tunnel`): this road's channel through a
//! pipe whose two ends pass binary messages unread, held to the computers approved for it here. The
//! room is played by two WebSocket ends joined in memory, each read and written through the pipe's
//! own bridge; the computer by the client core's own TLS over its end. Its key is the fixtures'
//! computer key; the station proves the fixtures' station key.
use super::*;
use crate::remote_service::tunnel::{bridge, Approved};
use std::collections::BTreeMap;

/// The device id the service gave the computer when it registered: a hosted device's, never the
/// LAN's, which is made from the key.
const APPROVED: &str = "10000000-0000-4000-8000-00000000000d";

/// Who the tunnel takes: `computer`, approved under [`APPROVED`], or nobody.
fn approved_for(computer: Option<&Computer>) -> BTreeMap<[u8; 32], String> {
    computer
        .map(|c| (c.pin, APPROVED.to_string()))
        .into_iter()
        .collect()
}

/// `s`'s tunnel, held to `roster`, its internet roads on loopback.
fn tunnel_of(s: &Shack, roster: Roster, approved: &Arc<Approved>) -> Tunnel {
    let deps = TunnelDeps {
        authority: s.shared.authority.clone(),
        engine: s.shared.engine.clone(),
        feeds: s.shared.feeds.clone(),
        dispatcher: s.shared.dispatcher.clone(),
        commands: s.shared.commands.clone(),
    };
    let counted = approved.clone();
    Tunnel::new(
        &deps,
        s.shared.identity.clone(),
        roster,
        Arc::new(move || counted.revisions()),
        Ipv4Addr::LOCALHOST.into(),
        None,
    )
    .expect("the tunnel")
}

/// A pipe through an in-memory room, the station's end served by `tunnel` until `stop`: the
/// computer's end, as the byte stream its TLS runs over.
async fn piped(tunnel: &Tunnel, stop: watch::Receiver<bool>) -> tokio::io::DuplexStream {
    let (station_side, room_side) = tokio::io::duplex(256 * 1024);
    let room = tokio::spawn(async move { tokio_tungstenite::accept_async(room_side).await });
    let (station_end, _) = tokio_tungstenite::client_async("ws://room/pipe", station_side)
        .await
        .unwrap();
    let computer_end = room.await.unwrap().unwrap();
    let (station_bytes, _) = bridge(station_end);
    let (computer_bytes, _) = bridge(computer_end);
    let tunnel = tunnel.clone();
    tokio::spawn(async move { tunnel.serve(station_bytes, stop).await });
    computer_bytes
}

type Through =
    tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::io::DuplexStream>>;

/// TLS with `key`, pinning `station`, then the upgrade, over the computer's end of the pipe.
async fn through(
    pipe: tokio::io::DuplexStream,
    key: &str,
    station: &str,
) -> Result<Through, String> {
    let connector = tokio_rustls::TlsConnector::from(tls::client::config(key, station).unwrap());
    let name = rustls::pki_types::ServerName::try_from("nexus-station").unwrap();
    let tls = connector
        .connect(name, pipe)
        .await
        .map_err(|e| format!("tls: {e}"))?;
    let (socket, _) = tokio_tungstenite::client_async("ws://nexus-station/", tls)
        .await
        .map_err(|e| format!("upgrade: {e}"))?;
    Ok(socket)
}

/// The station's next message other than the status line, how the connection ended, or `silent`.
async fn heard(socket: &mut Through) -> Value {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] != "status" {
                    return value;
                }
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(other) => return json!({"closed": format!("{other:?}")}),
            Err(_) => return json!({"silent": true}),
        }
    }
}

async fn say(socket: &mut Through, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn hello_through(socket: &mut Through) -> Value {
    say(
        socket,
        json!({"type":"hello","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2}),
    )
    .await;
    heard(socket).await
}

/// ★ Through the room, a computer approved here is welcomed, stamped with the device id the
/// service gave it, by the key pinned when it was approved; the station proves its own key.
/// A key not approved here is refused in the handshake and never welcomed. CONTROL: approved, the
/// same key is welcomed over a fresh pipe.
#[tokio::test]
async fn through_the_room_only_a_computer_approved_here_is_welcomed() {
    let s = shack(home());
    let approved = Arc::new(Approved::default());
    let tunnel = tunnel_of(&s, approved.roster(), &approved);
    let (_stop, stop) = watch::channel(false);
    let refused = match through(
        piped(&tunnel, stop.clone()).await,
        &s.computer.key,
        &s.public_key,
    )
    .await
    {
        Ok(mut socket) => hello_through(&mut socket).await,
        Err(e) => json!({"refused": e}),
    };
    assert!(
        refused["type"] != "welcome",
        "a key not approved was welcomed: {refused}"
    );
    approved.set(approved_for(Some(&s.computer)));
    let mut socket = through(piped(&tunnel, stop).await, &s.computer.key, &s.public_key)
        .await
        .unwrap();
    let welcome = hello_through(&mut socket).await;
    assert_eq!(
        (
            &welcome["type"],
            &welcome["deviceId"],
            &welcome["stationId"]
        ),
        (&json!("welcome"), &json!(APPROVED), &json!(STATION)),
        "{welcome}"
    );
    assert!(
        welcome.get("stopRoad").is_none() && welcome.get("audioRoad").is_none(),
        "no road of the LAN's through the room: {welcome}"
    );
}

/// ★ A computer that holds another key for this station refuses the station in the handshake
/// BEFORE it sends its own key: TLS 1.3 proves the server first, and the shack is never shown the
/// computer's key, so its roster is never asked about one. CONTROL: with the right key pinned, the
/// roster is asked, and the computer is welcomed.
#[tokio::test]
async fn a_wrong_station_key_is_refused_before_this_computers_key_is_sent() {
    let s = shack(home());
    let approved = Arc::new(Approved::default());
    approved.set(approved_for(Some(&s.computer)));
    let asked = Arc::new(AtomicUsize::new(0));
    let roster = {
        let (inner, asked) = (approved.roster(), asked.clone());
        Roster {
            paired: Arc::new(move |pin: &[u8; 32]| {
                asked.fetch_add(1, Ordering::SeqCst);
                (inner.paired)(pin)
            }),
            subscribe: inner.subscribe.clone(),
        }
    };
    let tunnel = tunnel_of(&s, roster, &approved);
    let (_stop, stop) = watch::channel(false);
    let impostor = tls::Identity::new(&fixture_key(), STATION.into())
        .unwrap()
        .public_key()
        .to_string();
    let refused = through(
        piped(&tunnel, stop.clone()).await,
        &s.computer.key,
        &impostor,
    )
    .await;
    let Err(why) = refused else {
        panic!("a station proving another key was taken");
    };
    assert!(why.starts_with("tls: "), "{why}");
    assert!(
        why.contains("certificate"),
        "refused as the station's key: {why}"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(asked.load(Ordering::SeqCst), 0, "the shack was shown a key");
    let mut socket = through(piped(&tunnel, stop).await, &s.computer.key, &s.public_key)
        .await
        .unwrap();
    assert_eq!(hello_through(&mut socket).await["type"], "welcome");
    assert!(
        asked.load(Ordering::SeqCst) > 0,
        "control: the roster is asked"
    );
}

/// ★ A computer removed here (its approval revoked, its pin gone) is out at once, before anything
/// else it sends is answered, and its next pipe is refused in the handshake.
#[tokio::test]
async fn a_computer_removed_here_is_out_at_once_and_refused_after() {
    let s = shack(home());
    let approved = Arc::new(Approved::default());
    approved.set(approved_for(Some(&s.computer)));
    let tunnel = tunnel_of(&s, approved.roster(), &approved);
    let (_stop, stop) = watch::channel(false);
    let mut socket = through(
        piped(&tunnel, stop.clone()).await,
        &s.computer.key,
        &s.public_key,
    )
    .await
    .unwrap();
    assert_eq!(hello_through(&mut socket).await["type"], "welcome");
    let removed = Instant::now();
    approved.set(approved_for(None));
    let ended = heard(&mut socket).await;
    assert!(ended.get("closed").is_some(), "still connected: {ended}");
    assert!(
        removed.elapsed() < Duration::from_secs(2),
        "{:?}",
        removed.elapsed()
    );
    let again = match through(piped(&tunnel, stop).await, &s.computer.key, &s.public_key).await {
        Ok(mut socket) => hello_through(&mut socket).await,
        Err(e) => json!({"refused": e}),
    };
    assert!(
        again["type"] != "welcome",
        "a removed computer was welcomed: {again}"
    );
}

/// ★ Nothing pairs through the room, and no stream is signalled there: an approved computer that
/// asks to pair is closed without a word, and one that signals a stream is told none is available.
#[tokio::test]
async fn nothing_pairs_through_the_room_and_no_stream_is_signalled() {
    let s = shack(home());
    let approved = Arc::new(Approved::default());
    approved.set(approved_for(Some(&s.computer)));
    let tunnel = tunnel_of(&s, approved.roster(), &approved);
    let (_stop, stop) = watch::channel(false);
    let mut socket = through(
        piped(&tunnel, stop.clone()).await,
        &s.computer.key,
        &s.public_key,
    )
    .await
    .unwrap();
    say(
        &mut socket,
        json!({"type":"pair","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
            "name":"Shack laptop","nonce":"00".repeat(16)}),
    )
    .await;
    let paired = heard(&mut socket).await;
    assert!(
        paired.get("closed").is_some(),
        "a pairing was answered: {paired}"
    );
    let mut socket = through(piped(&tunnel, stop).await, &s.computer.key, &s.public_key)
        .await
        .unwrap();
    assert_eq!(hello_through(&mut socket).await["type"], "welcome");
    say(
        &mut socket,
        json!({"type":"streamSignal","leaseId":"20000000-0000-4000-8000-000000000001",
            "payload":{"kind":"close"}}),
    )
    .await;
    assert_eq!(
        heard(&mut socket).await,
        json!({"type":"streamState","streaming":false,"reason":"streamUnavailable"})
    );
}

/// One operation request and its answer, asked again while the station is busy for a moment.
async fn ask_through(socket: &mut Through, request: Value) -> Value {
    for _ in 0..50 {
        let mut request = request.clone();
        if request["type"] != "stopTransmit" {
            request["requestId"] = json!(id());
        }
        say(socket, json!({"type":"operationRequest","request":request})).await;
        let answer = heard(socket).await;
        if answer["error"] != "stationBusy" && answer["error"] != "remoteBusy" {
            return answer;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the station stayed busy")
}

/// ★ Through the room, a welcomed computer holds nothing of its own: the tunnel grants no station
/// control, so its state says so and its take of the lease is refused, by name. CONTROL: given
/// station control, the same connection takes the lease.
#[tokio::test]
async fn through_the_room_a_welcomed_computer_holds_no_control_of_its_own() {
    let s = shack(home());
    let approved = Arc::new(Approved::default());
    approved.set(approved_for(Some(&s.computer)));
    let tunnel = tunnel_of(&s, approved.roster(), &approved);
    let (_stop, stop) = watch::channel(false);
    let mut socket = through(piped(&tunnel, stop).await, &s.computer.key, &s.public_key)
        .await
        .unwrap();
    assert_eq!(hello_through(&mut socket).await["type"], "welcome");
    let state = ask_through(&mut socket, json!({"type":"state","requestId":id()})).await;
    assert_eq!(
        state["value"]["phase"], "localPermissionRequired",
        "{state}"
    );
    let boot = state["value"]["stationBootId"].clone();
    let acquire = json!({"type":"acquire","requestId":id(),"stationBootId":boot});
    let refused = ask_through(&mut socket, acquire.clone()).await;
    assert_eq!(refused["error"], "localPermissionRequired", "{refused}");
    s.shared.authority.permit_station(APPROVED, true).unwrap();
    let acquired = ask_through(&mut socket, acquire).await;
    assert_eq!(acquired["value"]["phase"], "controlling", "{acquired}");
}
