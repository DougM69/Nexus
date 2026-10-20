//! The internet road's requests, against a stand-in for the Remote service on loopback that checks
//! each one as the service does (`signedBy`, `nativeDevice`): the challenge it gave, the device it
//! named, and the signature over the five lines with the key it holds for that device.
use super::*;
use futures_util::{SinkExt, StreamExt};
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
use std::sync::Mutex;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;

const STATION: &str = "20000000-0000-4000-8000-000000000001";
const DEVICE: &str = "30000000-0000-4000-8000-000000000002";

/// What the stand-in holds and saw.
#[derive(Default)]
struct Seen {
    /// The device key it registered for [`DEVICE`], SPKI hex.
    device_key: Option<String>,
    /// The challenges it gave and has not yet taken.
    challenges: Vec<String>,
    /// Each request: method and path.
    requests: Vec<(String, String)>,
    /// A refusal it gives to the pipe, if any: status and code.
    refuse_pipe: Option<(u16, &'static str)>,
    /// What `client/session` lists.
    session: Option<Value>,
}

/// Whether `signature` is the key `spki`'s over the five lines for `challenge`, `method`, `path`
/// and `body`.
fn verifies(
    spki: &str,
    challenge: &str,
    method: &str,
    path: &str,
    body: &[u8],
    signature: &str,
) -> bool {
    let point = tempo_stream::protocol::hex_bytes(spki)
        .map(|der| der[der.len() - 65..].to_vec())
        .unwrap_or_default();
    let Some(signature) = tempo_stream::protocol::hex_bytes(signature) else {
        return false;
    };
    let lines = native::signed_lines(challenge, method, path, body);
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
        .verify(&lines, &signature)
        .is_ok()
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// The stand-in's checks for a signed request as the device: its id, a challenge it gave (taken),
/// and a good signature by the key it holds for that device.
fn signed(
    seen: &mut Seen,
    headers: &[(String, String)],
    method: &str,
    path: &str,
    body: &[u8],
) -> bool {
    let challenge = header(headers, "x-nexus-challenge")
        .unwrap_or_default()
        .to_string();
    let Some(at) = seen.challenges.iter().position(|c| *c == challenge) else {
        return false;
    };
    seen.challenges.remove(at);
    header(headers, "x-nexus-device-id") == Some(DEVICE)
        && seen.device_key.as_deref().is_some_and(|key| {
            verifies(
                key,
                &challenge,
                method,
                path,
                body,
                header(headers, "x-nexus-signature").unwrap_or_default(),
            )
        })
}

fn reply(status: u16, body: Value) -> Vec<u8> {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// One plain request: method, path, headers and body.
async fn read_request(
    stream: &mut TcpStream,
) -> Option<(String, String, Vec<(String, String)>, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&bytes[..end]).ok()?.to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length: usize = header(&headers, "content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    let mut body = bytes[end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some((method, path, headers, body))
}

/// The stand-in on loopback: its origin, and what it saw.
async fn service() -> (String, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Seen::default()));
    let held = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let seen = held.clone();
            tokio::spawn(async move {
                let mut peeked = [0u8; 256];
                let n = stream.peek(&mut peeked).await.unwrap_or(0);
                let line = String::from_utf8_lossy(&peeked[..n]).to_string();
                if line.starts_with("GET ") && line.contains("/client/pipe ") {
                    pipe(stream, seen).await;
                    return;
                }
                let Some((method, path, headers, body)) = read_request(&mut stream).await else {
                    return;
                };
                let answer = answer_to(&seen, &method, &path, &headers, &body);
                let _ = stream.write_all(&answer).await;
            });
        }
    });
    (origin, seen)
}

/// The stand-in's answer to one plain request.
fn answer_to(
    seen: &Mutex<Seen>,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Vec<u8> {
    let mut seen = seen.lock().unwrap();
    seen.requests.push((method.to_string(), path.to_string()));
    let bearer = header(headers, "authorization") == Some("Bearer token-1");
    let station = |verb: &str| format!("/api/remote/stations/{STATION}/client/{verb}");
    if path == "/api/remote/client/challenge" || path == station("challenge") {
        // The account's with a token, or the device's own when it reopens.
        let reopening = path != "/api/remote/client/challenge";
        let ok = if reopening {
            header(headers, "x-nexus-device-id") == Some(DEVICE) && !bearer
        } else {
            bearer
        };
        if !ok {
            return reply(401, json!({"error":"signInRequired"}));
        }
        let challenge = format!("{:064x}", seen.challenges.len() + seen.requests.len());
        seen.challenges.push(challenge.clone());
        return reply(200, json!({"challenge":challenge,"expiresAt":1}));
    }
    if path == "/api/remote/client/session" && bearer {
        return reply(200, seen.session.clone().unwrap_or(json!({})));
    }
    if path == station("device") && bearer {
        let sent: Value = serde_json::from_slice(body).unwrap_or_default();
        let key = sent["publicKey"].as_str().unwrap_or_default().to_string();
        let challenge = header(headers, "x-nexus-challenge")
            .unwrap_or_default()
            .to_string();
        let good = seen.challenges.contains(&challenge)
            && verifies(
                &key,
                &challenge,
                "POST",
                path,
                body,
                header(headers, "x-nexus-signature").unwrap_or_default(),
            )
            && sent["kind"] == "computer";
        if !good {
            return reply(401, json!({"error":"invalidSignature"}));
        }
        seen.device_key = Some(key);
        return reply(
            200,
            json!({"deviceId":DEVICE,"approved":false,"expiresAt":1}),
        );
    }
    if (path == station("turn") || path == station("renew") || path == station("forget")) && !bearer
    {
        if !signed(&mut seen, headers, "POST", path, body) {
            return reply(401, json!({"error":"invalidSignature"}));
        }
        if path == station("turn") {
            return reply(
                200,
                json!({"iceServers":[{"urls":["turns:turn.example.net:443?transport=tcp",
                "turn:127.0.0.1:3478?transport=udp"],"username":"u","credential":"p"}]}),
            );
        }
        return reply(200, json!({"ok":true}));
    }
    reply(404, json!({"error":"notFound"}))
}

/// The pipe: the upgrade's headers checked as a signed GET with no body, then each binary message
/// sent back as it came.
async fn pipe(stream: TcpStream, seen: Arc<Mutex<Seen>>) {
    let checked = seen.clone();
    let callback = move |request: &Request, response: Response| {
        let headers: Vec<(String, String)> = request
            .headers()
            .iter()
            .map(|(n, v)| {
                (
                    n.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let path = request.uri().path().to_string();
        let mut seen = checked.lock().unwrap();
        seen.requests.push(("GET".into(), path.clone()));
        let refusal = |status: u16, code: &str| {
            let mut refused = tokio_tungstenite::tungstenite::http::Response::new(Some(
                json!({"error":code}).to_string(),
            ));
            *refused.status_mut() = status.try_into().unwrap();
            refused
        };
        if let Some((status, code)) = seen.refuse_pipe {
            return Err(refusal(status, code));
        }
        if !signed(&mut seen, &headers, "GET", &path, b"") {
            return Err(refusal(401, "invalidSignature"));
        }
        Ok(response)
    };
    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
        return;
    };
    while let Some(Ok(message)) = socket.next().await {
        if let Message::Binary(bytes) = message {
            let _ = socket.send(Message::Binary(bytes)).await;
        }
    }
}

/// The road this computer keeps once registered with the stand-in.
/// The operator's pick when the account has more than one station: `pick`, or none; and every list
/// offered.
fn picking(pick: Option<&str>) -> (Choose, Arc<Mutex<Vec<Vec<Choice>>>>) {
    let offered: Arc<Mutex<Vec<Vec<Choice>>>> = Arc::default();
    let (kept, pick) = (offered.clone(), pick.map(str::to_string));
    let choose: Choose = Arc::new(move |stations| {
        kept.lock().unwrap().push(stations);
        let pick = pick.clone();
        Box::pin(async move { pick })
    });
    (choose, offered)
}

async fn registered(origin: &str, seen: &Arc<Mutex<Seen>>) -> InternetRoad {
    let (station_key, _) = ComputerKey::generate().unwrap();
    seen.lock().unwrap().session =
        Some(json!({"entitlement":{"enabled":true,"expiresAt":u64::MAX},
        "stations":[{"id":STATION,"name":"Shack","stationKey":station_key.public_key()}]}));
    let service = Service::new(origin).unwrap();
    let (choose, offered) = picking(None);
    let (road, approved) = register_with(&service, "token-1", "Den PC", choose)
        .await
        .unwrap();
    assert!(offered.lock().unwrap().is_empty(), "asked with one station");
    assert!(!approved);
    assert_eq!(road.station_key, station_key.public_key());
    road
}

/// ★ Registering: the account's one station is read with the token, and this computer registers a
/// new key of its own as a device, signed by that key over a challenge the account was given; the
/// road kept is the service's station id, the device id it gave and the station key it listed.
/// CONTROL: an entitlement that does not take in the internet road registers nothing
/// (`internetNotIncluded`).
#[tokio::test]
async fn adding_the_road_registers_a_new_key_for_the_accounts_station() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    assert_eq!(
        (road.station_id.as_str(), road.device_id.as_str()),
        (STATION, DEVICE)
    );
    let key = ComputerKey::restore(&road.pkcs8).unwrap();
    assert_eq!(
        seen.lock().unwrap().device_key.as_deref(),
        Some(key.public_key())
    );

    let service = Service::new(&origin).unwrap();
    let before = seen.lock().unwrap().requests.len();
    seen.lock().unwrap().session = Some(json!({"entitlement":{"enabled":true,"expiresAt":1},
        "stations":[{"id":STATION,"name":"Shack","stationKey":key.public_key()}]}));
    assert_eq!(
        register_with(&service, "token-1", "Den PC", picking(None).0)
            .await
            .err(),
        Some("internetNotIncluded")
    );
    let asked: Vec<String> = seen.lock().unwrap().requests[before..]
        .iter()
        .map(|(_, p)| p.clone())
        .collect();
    assert_eq!(
        asked,
        vec!["/api/remote/client/session".to_string()],
        "nothing registered"
    );
}

/// ★ The road kept says this computer's key for it as the station shows that key when it asks to
/// approve this computer: the shack's own fingerprint (`remote_service::fingerprint`) of the key the
/// service registered for the device, which is what the service lists to the station. And whether the
/// device still waits for that approval; the road is kept for the station paired here. CONTROL: a road
/// the service approved at once waits for nothing.
#[tokio::test]
async fn the_road_kept_says_this_computers_key_as_the_station_shows_it() {
    const PAIRED: &str = "60000000-0000-4000-8000-000000000001";
    let (origin, seen) = service().await;
    for approved in [false, true] {
        let road = registered(&origin, &seen).await;
        let listed = seen.lock().unwrap().device_key.clone().unwrap();
        let stations = Arc::new(Stations::new(Arc::new(
            crate::lan_client::tests::StationStore::default(),
        )));
        stations
            .keep(&crate::lan_client::tests::record(
                PAIRED,
                &["192.168.1.20:42075"],
            ))
            .unwrap();
        let pkcs8 = road.pkcs8.clone();
        let (waiting, key) = keep_road(Arc::clone(&stations), PAIRED.into(), road, approved)
            .await
            .unwrap();
        assert_eq!(
            Some(key),
            crate::remote_service::fingerprint(&listed),
            "the key the station shows"
        );
        assert_eq!(waiting, !approved, "waits only while not approved there");
        assert_eq!(
            stations.internet(PAIRED).unwrap().map(|kept| kept.pkcs8),
            Some(pkcs8),
            "kept for the station paired here"
        );
    }
}

/// ★ An account with two stations: the operator is offered each one's id and name, and the station
/// picked is the one registered with, and its listed key the one kept. CONTROL: no pick (cancelled,
/// or the window gone), or an id the service never listed, registers nothing (`stationNotChosen`).
#[tokio::test]
async fn an_account_with_two_stations_asks_which_and_registers_with_the_one_picked() {
    const CLUB: &str = "20000000-0000-4000-8000-0000000000c1";
    let (origin, seen) = service().await;
    let (club_key, _) = ComputerKey::generate().unwrap();
    let (shack_key, _) = ComputerKey::generate().unwrap();
    seen.lock().unwrap().session = Some(json!({"entitlement":{"access":"internet"},"stations":[
        {"id":CLUB,"name":"Club station","stationKey":club_key.public_key()},
        {"id":STATION,"name":"Shack","stationKey":shack_key.public_key()}]}));
    let service = Service::new(&origin).unwrap();
    let device = format!("/api/remote/stations/{STATION}/client/device");
    let registers = |seen: &Arc<Mutex<Seen>>| {
        seen.lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(_, path)| path.ends_with("/client/device"))
            .count()
    };
    for pick in [None, Some("30000000-0000-4000-8000-0000000000ff")] {
        let (choose, offered) = picking(pick);
        assert_eq!(
            register_with(&service, "token-1", "Den PC", choose)
                .await
                .err(),
            Some("stationNotChosen"),
            "{pick:?}"
        );
        assert_eq!(offered.lock().unwrap().len(), 1);
        assert_eq!(registers(&seen), 0, "{pick:?}: registered");
    }
    let (choose, offered) = picking(Some(STATION));
    let (road, _) = register_with(&service, "token-1", "Den PC", choose)
        .await
        .unwrap();
    assert_eq!(
        offered.lock().unwrap().as_slice(),
        [vec![
            Choice {
                id: CLUB.into(),
                name: "Club station".into()
            },
            Choice {
                id: STATION.into(),
                name: "Shack".into()
            },
        ]]
    );
    assert_eq!(road.station_id, STATION);
    assert_eq!(road.station_key, shack_key.public_key());
    let asked = seen.lock().unwrap().requests.clone();
    assert_eq!(asked.last(), Some(&("POST".to_string(), device)));
}

/// ★ Reopening needs no sign-in: TURN credentials and a renewal are asked as the device, each
/// signed by the road's key over a challenge given for that device, and the UDP TURN server comes
/// back (the TLS one left out). CONTROL: a road holding another key is refused (`accessDenied`).
#[tokio::test]
async fn reopening_is_signed_by_the_roads_key_with_no_sign_in() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    let service = Service::new(&origin).unwrap();
    let servers = service.turn(&road).await.unwrap();
    let named: Vec<(String, u16)> = servers.iter().map(|s| (s.host.clone(), s.port)).collect();
    assert_eq!(named, vec![("127.0.0.1".to_string(), 3478)]);
    service.renew(&road).await.unwrap();
    let relay = relay(servers).await.expect("the relay");
    assert_eq!(relay.server, "127.0.0.1:3478".parse().unwrap());

    let (_, other) = ComputerKey::generate().unwrap();
    let impostor = InternetRoad {
        pkcs8: other,
        ..road
    };
    assert_eq!(service.renew(&impostor).await.err(), Some("accessDenied"));
}

/// ★ Forget asks the service, as the device and with no sign-in, to end this device's approval:
/// signed by the road's key over a challenge given for that device. CONTROL: a road holding another
/// key is refused (`accessDenied`).
#[tokio::test]
async fn forgetting_is_signed_by_the_roads_key_with_no_sign_in() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    let service = Service::new(&origin).unwrap();
    let before = seen.lock().unwrap().requests.len();
    service.forget(&road).await.unwrap();
    let asked = seen.lock().unwrap().requests[before..].to_vec();
    assert_eq!(
        asked.last(),
        Some(&(
            "POST".to_string(),
            format!("/api/remote/stations/{STATION}/client/forget")
        ))
    );
    let (_, other) = ComputerKey::generate().unwrap();
    let impostor = InternetRoad {
        pkcs8: other,
        ..road
    };
    assert_eq!(service.forget(&impostor).await.err(), Some("accessDenied"));
}

/// ★ The pipe is a WebSocket signed as the device (a GET with no body), and carries bytes both
/// ways. CONTROL: a pipe the service refuses for the entitlement says `internetNotIncluded`, and a
/// road holding another key is refused.
#[tokio::test]
async fn the_pipe_opens_signed_and_carries_bytes() {
    let (origin, seen) = service().await;
    let road = registered(&origin, &seen).await;
    let service = Service::new(&origin).unwrap();
    let mut socket = service.pipe(&road).await.expect("the pipe");
    socket
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    match tokio::time::timeout(Duration::from_secs(5), socket.next()).await {
        Ok(Some(Ok(Message::Binary(bytes)))) => assert_eq!(bytes.as_ref(), &[1, 2, 3]),
        other => panic!("nothing came back: {other:?}"),
    }
    let (_, other) = ComputerKey::generate().unwrap();
    let impostor = InternetRoad {
        pkcs8: other,
        ..clone_road(&road)
    };
    assert_eq!(service.pipe(&impostor).await.err(), Some("accessDenied"));
    seen.lock().unwrap().refuse_pipe = Some((403, "trialRequired"));
    assert_eq!(service.pipe(&road).await.err(), Some("internetNotIncluded"));
}

fn clone_road(road: &InternetRoad) -> InternetRoad {
    InternetRoad {
        station_id: road.station_id.clone(),
        device_id: road.device_id.clone(),
        station_key: road.station_key.clone(),
        pkcs8: road.pkcs8.clone(),
    }
}

/// ★ The sign-in's loopback listener takes only the redirect it is waiting for: a favicon and a
/// redirect with another state are answered and the wait goes on; the code with this state is
/// taken. CONTROL: the issuer's refusal with this state ends the wait (`signInRefused`).
#[tokio::test]
async fn the_loopback_listener_takes_only_this_sign_ins_redirect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let ask = |target: &'static str| async move {
        let mut stream = TcpStream::connect(at).await.unwrap();
        stream
            .write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut page = String::new();
        let _ = stream.read_to_string(&mut page).await;
        page
    };
    let waiting = tokio::spawn(async move {
        let code = redirected(&listener, "state-1").await;
        (code, listener)
    });
    assert!(ask("/favicon.ico").await.contains("not the sign-in"));
    assert!(ask("/callback?code=stolen&state=other")
        .await
        .contains("not the sign-in"));
    assert!(ask("/callback?code=the-code&state=state-1")
        .await
        .contains("signed in"));
    let (code, listener) = waiting.await.unwrap();
    assert_eq!(code, Ok("the-code".to_string()));

    let refused = tokio::spawn(async move { redirected(&listener, "state-2").await });
    let _ = ask("/callback?error=access_denied&state=state-2").await;
    assert_eq!(refused.await.unwrap(), Err("signInRefused"));
}

/// The service's refusals as this computer names them: only those it knows, the rest by status.
#[test]
fn only_known_refusals_are_named() {
    let said = |code: &str| json!({"error":code});
    assert_eq!(
        named(403, Some(&said("trialRequired"))),
        "internetNotIncluded"
    );
    assert_eq!(
        named(401, Some(&said("signInRequired"))),
        "notApprovedThere"
    );
    assert_eq!(
        named(403, Some(&said("deviceNotApproved"))),
        "waitingApproval"
    );
    assert_eq!(named(403, Some(&said("<script>"))), "accessDenied");
    assert_eq!(named(429, None), "tryLater");
    assert_eq!(named(500, None), "serviceUnavailable");
    assert!(lasting("internetNotIncluded") && !lasting("serviceUnavailable"));
}
