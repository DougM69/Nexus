//! The internet road, this computer's half of the Remote service (`remote/src/index.ts`, THE NATIVE
//! CLIENT): adding the road to a station paired here, and opening it again each time client mode
//! needs it.
//!
//! ## Adding the road ([`add`])
//!
//! Once, from Stations on this network, for a station already paired here. The operator signs in
//! through the system browser to the service's Native application ([`sign_in`]: PKCE and a redirect
//! to this computer's loopback address, RFC 8252), whose issuer and client id the service names
//! (`GET /api/remote/config`), so neither is built in here. With the token, which is kept in memory
//! for this one exchange and then dropped, this computer reads the account's stations (asking the
//! operator which one, when there are more than one), makes a new key for the road (N3: a key of
//! its own, never the LAN road's), and registers it as a device of that station, signed by that key. The station then shows it for approval like a browser. What is
//! kept is the road's record (`vault::InternetRoad`), in the OS credential store beside the
//! station's: the service's station id, the device id it gave, the station key it listed (pinned
//! from then on) and the road's key. No account credential is ever kept.
//!
//! The station asks the operator to approve this computer only if the key it shows matches the one
//! Nexus shows on this computer, so the window shows this computer's key for each road too
//! ([`shown_key`]): the public half's fingerprint, as the station shows it, never the private key.
//!
//! ## Opening it ([`Service::pipe`], [`Service::turn`], [`Service::renew`])
//!
//! With no sign-in: every request names the device, and is signed by its key over a challenge the
//! service gives for that device alone (reopening). The pipe through the station's room is a signed
//! WebSocket the command link's TLS runs inside; the TURN credentials are for the two datagram
//! roads (audio and Stop) beside it; the pipe lives a minute unless renewed, so it is renewed every
//! [`RENEW_EVERY`] while the link lives.
//!
//! ## Refusals
//!
//! The service names what it refused; only the names below are taken from it ([`named`]), as the
//! shack's own transport does, and anything else is said by its status alone.
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tempo_client::native::{self, Pkce, SignIn, TurnServer};
use tempo_client::ComputerKey;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::Stations;
use crate::remote_service::vault::InternetRoad;

/// The loopback port the sign-in's redirect comes back to: the Native application's allowed
/// callback is exactly `http://127.0.0.1:42078/callback`.
pub(crate) const SIGN_IN_PORT: u16 = 42078;
/// How long the browser has to come back with the code.
const SIGN_IN_WAIT: Duration = Duration::from_secs(5 * 60);
/// How often an open pipe is renewed. The room gives it a minute at a time.
pub(crate) const RENEW_EVERY: Duration = Duration::from_secs(20);
/// How long the pipe's socket may take to open: the room waits 10 s for the station's end.
const PIPE_OPEN: Duration = Duration::from_secs(12);
/// The most of a service answer read.
const ANSWER_LIMIT: usize = 16384;
/// The largest message either end of the pipe may send (the room's bound).
use tempo_client::tunnel::PIPE_MESSAGE_BYTES;

/// The pipe's socket, as it is opened.
pub(crate) type PipeSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Adding the internet road to a paired station, from the Stations on this network window: the
/// station's id here, and what the page is told ([`Added`]), or why not.
pub(crate) type AddInternet = Arc<
    dyn Fn(String, Choose) -> Pin<Box<dyn Future<Output = Result<Added, &'static str>> + Send>>
        + Send
        + Sync,
>;

/// What the page is told once the road is added (`internetAdded`): whether the device still waits for
/// approval at the station, and this computer's key for the road as the station shows it.
pub(crate) type Added = (bool, String);

/// An account with more than one station asks the operator which is the one being added: each
/// station's id and name are offered, and the one picked comes back, or none (cancelled, or the
/// window gone).
pub(crate) type Choose =
    Arc<dyn Fn(Vec<Choice>) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

/// One of the account's stations, as the operator is offered it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Choice {
    pub id: String,
    pub name: String,
}

/// How long the operator has to pick the station, as for the sign-in.
pub(crate) const CHOOSE_WAIT: Duration = SIGN_IN_WAIT;

/// The longest station name offered, in characters: the service's bound for a name (`label`).
const CHOICE_NAME_CHARS: usize = 48;
/// The most stations offered at once, as the page reads the list.
const MOST_CHOICES: usize = 16;

/// Forgetting a station's internet road at the service, as Forget on this computer does: the
/// device's approval ended there, or why the service was not told.
pub(crate) type ForgetInternet = Arc<
    dyn Fn(InternetRoad) -> Pin<Box<dyn Future<Output = Result<(), &'static str>> + Send>>
        + Send
        + Sync,
>;

/// The Remote service, as this computer's internet road speaks to it.
#[derive(Clone)]
pub(crate) struct Service {
    http: reqwest::Client,
    origin: String,
}

impl Service {
    /// The service at `origin`: the one Nexus is built for, or in a test an `http://127.0.0.1:port`
    /// stand-in.
    pub(crate) fn new(origin: &str) -> Result<Self, &'static str> {
        let allowed = origin == crate::remote_service::REMOTE_ORIGIN;
        #[cfg(test)]
        let allowed = allowed
            || origin
                .strip_prefix("http://127.0.0.1:")
                .is_some_and(|port| port.parse::<u16>().is_ok());
        if !allowed {
            return Err("originDenied");
        }
        let http = reqwest::Client::builder()
            .https_only(!cfg!(test))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "serviceUnavailable")?;
        Ok(Self {
            http,
            origin: origin.to_string(),
        })
    }

    /// The path a request's signature covers: the URL's own.
    fn path(verb: &str) -> String {
        format!("/api/remote/{verb}")
    }

    /// The service's sign-in: its issuer, the Native application, and its API. `internetNotReady`
    /// while it names no Native application.
    async fn config(&self) -> Result<SignIn, &'static str> {
        let response = self
            .http
            .get(format!("{}/api/remote/config", self.origin))
            .send()
            .await
            .map_err(|_| "serviceUnavailable")?;
        let config = answer(response).await?;
        SignIn::from_config(&config).ok_or("internetNotReady")
    }

    /// A POST of `body` to `verb`, with `headers`: its answer, or the refusal it names.
    async fn post(
        &self,
        verb: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> Result<Value, &'static str> {
        let mut request = self
            .http
            .post(format!("{}{}", self.origin, Self::path(verb)))
            .header("content-type", "application/json")
            .body(body);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        answer(request.send().await.map_err(|_| "serviceUnavailable")?).await
    }

    /// A challenge for the next signed request: the signed-in account's with `token`, or with none
    /// this device's own, reopening the station `road` names.
    async fn challenge(
        &self,
        token: Option<&str>,
        road: Option<&InternetRoad>,
    ) -> Result<String, &'static str> {
        let given = match (token, road) {
            (Some(token), _) => {
                let bearer = format!("Bearer {token}");
                self.post(
                    "client/challenge",
                    &[("authorization", &bearer)],
                    b"{}".to_vec(),
                )
                .await?
            }
            (None, Some(road)) => {
                self.post(
                    &format!("stations/{}/client/challenge", road.station_id),
                    &[("x-nexus-device-id", &road.device_id)],
                    b"{}".to_vec(),
                )
                .await?
            }
            (None, None) => return Err("signInRequired"),
        };
        given["challenge"]
            .as_str()
            .filter(|c| c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_string)
            .ok_or("invalidResponse")
    }

    /// The station's verb `verb` (`turn`, `renew`, `forget`), as the device `road` names, signed by
    /// its key over a challenge given for it, with no sign-in.
    async fn reopened(&self, road: &InternetRoad, verb: &str) -> Result<Value, &'static str> {
        let key = ComputerKey::restore(&road.pkcs8).ok_or("storeUnavailable")?;
        let challenge = self.challenge(None, Some(road)).await?;
        let verb = format!("stations/{}/client/{verb}", road.station_id);
        let body = b"{}".to_vec();
        let signature = key
            .sign_request(&challenge, "POST", &Self::path(&verb), &body)
            .ok_or("serviceUnavailable")?;
        self.post(
            &verb,
            &[
                ("x-nexus-device-id", &road.device_id),
                ("x-nexus-challenge", &challenge),
                ("x-nexus-signature", &signature),
            ],
            body,
        )
        .await
    }

    /// TURN servers for this device's two datagram roads, UDP only.
    pub(crate) async fn turn(&self, road: &InternetRoad) -> Result<Vec<TurnServer>, &'static str> {
        Ok(native::turn_servers(&self.reopened(road, "turn").await?))
    }

    /// The open pipe's minute renewed.
    pub(crate) async fn renew(&self, road: &InternetRoad) -> Result<(), &'static str> {
        self.reopened(road, "renew").await.map(|_| ())
    }

    /// This device's approval at the station ended, as the device itself asks (Forget on this
    /// computer): signed by its key, with no sign-in.
    pub(crate) async fn forget(&self, road: &InternetRoad) -> Result<(), &'static str> {
        self.reopened(road, "forget").await.map(|_| ())
    }

    /// The pipe to the station through its room: a WebSocket, signed by this device's key over a
    /// challenge given for it, which the command link's TLS then runs inside.
    pub(crate) async fn pipe(&self, road: &InternetRoad) -> Result<PipeSocket, &'static str> {
        let key = ComputerKey::restore(&road.pkcs8).ok_or("storeUnavailable")?;
        let challenge = self.challenge(None, Some(road)).await?;
        let path = Self::path(&format!("stations/{}/client/pipe", road.station_id));
        let signature = key
            .sign_request(&challenge, "GET", &path, b"")
            .ok_or("serviceUnavailable")?;
        let origin = self
            .origin
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        let mut request = format!("{origin}{path}")
            .into_client_request()
            .map_err(|_| "invalidRequest")?;
        let headers = request.headers_mut();
        for (name, value) in [
            ("x-nexus-device-id", road.device_id.as_str()),
            ("x-nexus-challenge", &challenge),
            ("x-nexus-signature", &signature),
        ] {
            headers.insert(name, value.parse().map_err(|_| "invalidRequest")?);
        }
        // `true`: no Nagle. A Stop's frame, and its answer, go as they are written.
        let connect = tokio_tungstenite::connect_async_with_config(request, Some(config()), true);
        match tokio::time::timeout(PIPE_OPEN, connect).await {
            Ok(Ok((socket, _))) => Ok(socket),
            Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) => {
                let status = response.status().as_u16();
                let said = response
                    .body()
                    .as_deref()
                    .and_then(|body| serde_json::from_slice::<Value>(body).ok());
                Err(named(status, said.as_ref()))
            }
            _ => Err("serviceUnavailable"),
        }
    }

    /// The account's stations, each with its station key, and its entitlement, for the signed-in
    /// account `token` is for.
    async fn session(&self, token: &str) -> Result<Value, &'static str> {
        let bearer = format!("Bearer {token}");
        self.post(
            "client/session",
            &[("authorization", &bearer)],
            b"{}".to_vec(),
        )
        .await
    }

    /// This computer registered as a device of the account's station `station` (as `session`
    /// listed it), named `name`, with a new key of its own: the road to keep, and whether it is
    /// approved already (a key registered before and approved since).
    async fn register(
        &self,
        token: &str,
        station: &Value,
        name: &str,
    ) -> Result<(InternetRoad, bool), &'static str> {
        let station_id = station["id"]
            .as_str()
            .filter(|id| tempo_stream::protocol::identifier(id))
            .ok_or("invalidResponse")?;
        // The station key the service lists, pinned from now on: without one the station has not
        // yet said it to the service, and the road could prove nothing.
        let station_key = station["stationKey"]
            .as_str()
            .filter(|key| tempo_stream::protocol::device_key(key))
            .ok_or("updateStation")?;
        let (key, pkcs8) = ComputerKey::generate().ok_or("serviceUnavailable")?;
        let verb = format!("stations/{station_id}/client/device");
        let body = serde_json::to_vec(&json!({"name":name,"kind":"computer","platform":platform(),
            "publicKey":key.public_key()}))
        .map_err(|_| "serviceUnavailable")?;
        let challenge = self.challenge(Some(token), None).await?;
        let signature = key
            .sign_request(&challenge, "POST", &Self::path(&verb), &body)
            .ok_or("serviceUnavailable")?;
        let bearer = format!("Bearer {token}");
        let registered = self
            .post(
                &verb,
                &[
                    ("authorization", &bearer),
                    ("x-nexus-challenge", &challenge),
                    ("x-nexus-signature", &signature),
                ],
                body,
            )
            .await?;
        let device_id = registered["deviceId"]
            .as_str()
            .filter(|id| tempo_stream::protocol::identifier(id))
            .ok_or("invalidResponse")?;
        let road = InternetRoad {
            station_id: station_id.to_string(),
            device_id: device_id.to_string(),
            station_key: station_key.to_string(),
            pkcs8,
        };
        Ok((road, registered["approved"].as_bool() == Some(true)))
    }
}

/// The pipe's socket: binary messages up to the room's bound, written as they come.
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(PIPE_MESSAGE_BYTES))
        .max_frame_size(Some(PIPE_MESSAGE_BYTES))
        .write_buffer_size(0)
}

/// This computer's platform, as the station shows it at approval.
fn platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// A service answer: its JSON, or the refusal it names.
async fn answer(mut response: reqwest::Response) -> Result<Value, &'static str> {
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "serviceUnavailable")? {
        if bytes.len() + chunk.len() > ANSWER_LIMIT {
            return Err("invalidResponse");
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = serde_json::from_slice::<Value>(&bytes).ok();
    if !(200..300).contains(&status) {
        return Err(named(status, body.as_ref()));
    }
    body.ok_or("invalidResponse")
}

/// What the service refused, by the name it gave when it is one of these (an ALLOW-LIST: a code
/// from the service never becomes text this side shows), otherwise by its status alone.
pub(crate) fn named(status: u16, body: Option<&Value>) -> &'static str {
    match body.and_then(|b| b["error"].as_str()) {
        // N4: the internet road is the paid service's; the trial or the plan does not take it in.
        Some("trialRequired" | "trialEnded" | "trialDisabled") => "internetNotIncluded",
        // Reopening is refused alike for a device removed, waiting, unknown or never approved.
        Some("signInRequired") => "notApprovedThere",
        Some("deviceNotApproved") => "waitingApproval",
        Some("deviceLimit") => "deviceLimit",
        Some("stationUnavailable") => "stationUnavailable",
        Some("serviceNotConfigured") => "internetNotReady",
        Some("relayNotConfigured" | "relayUnavailable") => "relayUnavailable",
        Some("pipeClosed") => "pipeClosed",
        _ => match status {
            401 | 403 => "accessDenied",
            404 => "stationUnavailable",
            429 => "tryLater",
            _ => "serviceUnavailable",
        },
    }
}

/// Whether a refusal will not change by asking again: the operator must act first.
pub(crate) fn lasting(reason: &str) -> bool {
    matches!(
        reason,
        "internetNotIncluded"
            | "notApprovedThere"
            | "waitingApproval"
            | "stationUnavailable"
            | "internetNotReady"
            | "accessDenied"
    )
}

/// The first sign-in, through the system browser: the service's sign-in read, this computer's
/// loopback port opened for the redirect, the browser sent to the issuer by `open`, and the code it
/// brings back exchanged for a token. The token, kept in memory by the caller for this one
/// exchange. `signInPortBusy` when the port is taken, `signInTimedOut` after [`SIGN_IN_WAIT`].
pub(crate) async fn sign_in(
    service: &Service,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Result<String, &'static str> {
    let sign = service.config().await?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, SIGN_IN_PORT))
        .await
        .map_err(|_| "signInPortBusy")?;
    let redirect = format!("http://127.0.0.1:{SIGN_IN_PORT}{}", native::CALLBACK_PATH);
    let pkce = Pkce::new().ok_or("signInFailed")?;
    open(&native::authorize_url(&sign, &redirect, &pkce)).map_err(|_| "signInFailed")?;
    let code = tokio::time::timeout(SIGN_IN_WAIT, redirected(&listener, &pkce.state))
        .await
        .map_err(|_| "signInTimedOut")??;
    drop(listener);
    let form = native::token_form(&sign, &redirect, &pkce, &code);
    let response = service
        .http
        .post(format!("{}oauth/token", sign.issuer))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|_| "signInFailed")?;
    let given = answer(response).await.map_err(|_| "signInFailed")?;
    native::token(&given)
        .map(|(token, _)| token)
        .ok_or("signInFailed")
}

/// The browser's redirect to this computer's loopback port: the code it brought for this sign-in's
/// `state`, or the issuer's refusal. A request for anything else (a favicon, a redirect this
/// sign-in did not start) is answered and the wait goes on.
pub(crate) async fn redirected(
    listener: &TcpListener,
    state: &str,
) -> Result<String, &'static str> {
    loop {
        let Ok((mut stream, peer)) = listener.accept().await else {
            continue;
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let target = read_target(&mut stream).await;
        let result = match target.as_deref() {
            Some(target) => native::callback(target, state),
            None => Err("signInFailed"),
        };
        let page = match result {
            Ok(_) => "Nexus is signed in. You can close this tab and go back to Nexus.",
            Err("signInRefused") => "The sign-in was cancelled. You can close this tab.",
            Err(_) => "This is not the sign-in Nexus is waiting for.",
        };
        let body =
            format!("<!doctype html><meta charset=\"utf-8\"><title>Nexus</title><p>{page}</p>");
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\n\
             cache-control: no-store\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(reply.as_bytes()).await;
        let _ = stream.shutdown().await;
        match result {
            Ok(code) => return Ok(code),
            Err("signInRefused") => return Err("signInRefused"),
            Err(_) => continue,
        }
    }
}

/// A GET's request target from the first line of a request read within a second, or `None`.
async fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    let read = async {
        while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 8192 {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            head.extend_from_slice(&chunk[..n]);
        }
        Some(())
    };
    tokio::time::timeout(Duration::from_secs(1), read)
        .await
        .ok()??;
    let line = std::str::from_utf8(&head).ok()?.lines().next()?.to_string();
    let mut parts = line.split(' ');
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) => Some(target.to_string()),
        _ => None,
    }
}

/// Adds the internet road to the station paired here as `id`: signed in through `open`'s browser,
/// the account's station chosen, this computer registered as its device, and the road kept. Whether
/// the device still waits for approval at the station, and this computer's key for the road as the
/// station shows it ([`keep_road`]), or why not.
///
/// The station is the account's only one, or, when it has more, the one the operator picks from its
/// list through `choose` (`stationNotChosen` when none is). The token is held, in memory only, while
/// the operator picks. `internetNotIncluded` when the account's entitlement does not take in the internet road (N4):
/// nothing is registered then.
pub(crate) async fn add(
    service: &Service,
    stations: Arc<Stations>,
    id: String,
    name: String,
    open: impl FnOnce(&str) -> Result<(), String>,
    choose: Choose,
) -> Result<Added, &'static str> {
    let kept = stations.clone();
    let lan_id = id.clone();
    tokio::task::spawn_blocking(move || kept.get(&lan_id))
        .await
        .unwrap_or(Err("storeUnavailable"))?
        .ok_or("unknownStation")?;
    let token = sign_in(service, open).await?;
    let (road, approved) = register_with(service, &token, &name, choose).await?;
    drop(token);
    keep_road(stations, id, road, approved).await
}

/// The road [`register_with`] gave, kept for the station paired here as `id`: whether the device
/// still waits for approval at the station (not `approved` there already), and this computer's key
/// for the road as the station shows it ([`shown_key`]).
pub(crate) async fn keep_road(
    stations: Arc<Stations>,
    id: String,
    road: InternetRoad,
    approved: bool,
) -> Result<Added, &'static str> {
    let key = shown_key(&road).ok_or("notStation")?;
    tokio::task::spawn_blocking(move || stations.keep_internet(&id, &road))
        .await
        .unwrap_or(Err("storeUnavailable"))?;
    Ok((!approved, key))
}

/// This computer's key for `road` as the station shows it when it asks to approve this computer,
/// and under Remote access ▸ Your computers: the station's own fingerprint
/// (`remote_service::fingerprint`) of the key this computer registered, its public half. The
/// private half is never shown, sent or logged. `None` for a record with no key in it.
pub(crate) fn shown_key(road: &InternetRoad) -> Option<String> {
    crate::remote_service::fingerprint(ComputerKey::restore(&road.pkcs8)?.public_key())
}

/// With the signed-in `token`: the account's station (its only one, or the one the operator picks
/// through `choose`), and this computer registered as its device, named `name`. See [`add`].
pub(crate) async fn register_with(
    service: &Service,
    token: &str,
    name: &str,
    choose: Choose,
) -> Result<(InternetRoad, bool), &'static str> {
    let session = service.session(token).await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if !native::internet_included(&session["entitlement"], now) {
        return Err("internetNotIncluded");
    }
    let listed = session["stations"].as_array().cloned().unwrap_or_default();
    let station = match listed.as_slice() {
        [] => return Err("stationUnavailable"),
        [only] => only.clone(),
        _ => {
            // Only a station the service listed can be picked: the answer is looked up in the
            // list, never taken as an id of its own.
            let offered: Vec<Choice> = listed
                .iter()
                .filter_map(|station| {
                    let id = station["id"]
                        .as_str()
                        .filter(|id| tempo_stream::protocol::identifier(id))?;
                    let name = station["name"].as_str().unwrap_or("");
                    Some(Choice {
                        id: id.to_string(),
                        name: name.chars().take(CHOICE_NAME_CHARS).collect(),
                    })
                })
                .take(MOST_CHOICES)
                .collect();
            let picked = choose(offered).await.ok_or("stationNotChosen")?;
            listed
                .iter()
                .find(|station| {
                    station["id"].as_str() == Some(picked.as_str())
                        && tempo_stream::protocol::identifier(&picked)
                })
                .cloned()
                .ok_or("stationNotChosen")?
        }
    };
    service.register(token, &station, name).await
}

/// The internet road as client mode opens it (`client_mode::Afar`): each road's record read from
/// this computer's store, the Remote service asked for the pipe, the relay and each renewal.
pub(crate) struct Roads {
    stations: Arc<Stations>,
    service: Service,
}

impl Roads {
    pub(crate) fn new(stations: Arc<Stations>) -> Result<Self, &'static str> {
        Ok(Self {
            stations,
            service: Service::new(crate::remote_service::REMOTE_ORIGIN)?,
        })
    }

    /// The road kept for the station paired here as `id`, read off the async threads.
    async fn road(stations: Arc<Stations>, id: String) -> Result<InternetRoad, &'static str> {
        tokio::task::spawn_blocking(move || stations.internet(&id))
            .await
            .unwrap_or(Err("storeUnavailable"))?
            .ok_or("unknownStation")
    }
}

impl crate::client_mode::Afar for Roads {
    fn has_road(&self, id: &str) -> bool {
        matches!(self.stations.internet(id), Ok(Some(_)))
    }

    fn open(
        &self,
        id: String,
        headset: Option<Arc<dyn tempo_client::audio_road::Headset>>,
    ) -> Pin<Box<dyn Future<Output = Result<tempo_client::link::Link, &'static str>> + Send>> {
        let (stations, service) = (Arc::clone(&self.stations), self.service.clone());
        Box::pin(async move {
            let road = Self::road(stations, id).await?;
            open(&service, &road, headset).await
        })
    }

    fn renew(&self, id: String) -> Pin<Box<dyn Future<Output = Result<(), &'static str>> + Send>> {
        let (stations, service) = (Arc::clone(&self.stations), self.service.clone());
        Box::pin(async move {
            let road = Self::road(stations, id).await?;
            service.renew(&road).await
        })
    }
}

/// How long the STUN server's address may take to look up before the roads go without it.
const STUN_LOOKUP: Duration = Duration::from_secs(2);

/// The command link to the station `road` names, through its room: the TURN credentials asked
/// (the roads go direct only, said in the log, when the service has none to give), the STUN
/// server's address looked up, the signed pipe opened, and the link's TLS, hello and `open` run
/// inside it with the two internet roads beside it, each on a socket of its own.
pub(crate) async fn open(
    service: &Service,
    road: &InternetRoad,
    headset: Option<Arc<dyn tempo_client::audio_road::Headset>>,
) -> Result<tempo_client::link::Link, &'static str> {
    let relay = match service.turn(road).await {
        Ok(servers) => relay(servers).await,
        Err(why) => {
            tempo_core::applog::warn(
                "client",
                &format!("no relay for the internet roads ({why}); they try direct paths only"),
            );
            None
        }
    };
    let lookup = tokio::net::lookup_host(tempo_stream::stun::SERVER);
    let stun = match tokio::time::timeout(STUN_LOOKUP, lookup).await {
        Ok(Ok(mut found)) => found.find(SocketAddr::is_ipv4),
        _ => None,
    };
    let socket = service.pipe(road).await?;
    let (pipe, _pumps) = tempo_client::tunnel::bridge(socket);
    let record = tempo_client::PairedStation {
        station_id: road.station_id.clone(),
        device_id: road.device_id.clone(),
        station_key: road.station_key.clone(),
        pkcs8: road.pkcs8.clone(),
        addresses: Vec::new(),
    };
    let internet = tempo_client::link::Internet {
        bind: Ipv4Addr::UNSPECIFIED.into(),
        direct: true,
        stun,
        relay,
    };
    tempo_client::link::Link::connect_tunnel(
        pipe,
        &record,
        tempo_client::link::run_id(),
        headset,
        &internet,
    )
    .await
}

/// The TURN servers' addresses, IPv4, each looked up within a second, with their credentials, as
/// the internet roads' relay takes them: the first that resolves.
pub(crate) async fn relay(servers: Vec<TurnServer>) -> Option<tempo_client::ice::Relay> {
    for server in servers {
        let lookup = tokio::net::lookup_host((server.host.as_str(), server.port));
        let Ok(Ok(mut found)) = tokio::time::timeout(Duration::from_secs(1), lookup).await else {
            continue;
        };
        if let Some(at) = found.find(SocketAddr::is_ipv4) {
            return Some(tempo_client::ice::Relay {
                server: at,
                username: server.username,
                password: server.password,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests;
