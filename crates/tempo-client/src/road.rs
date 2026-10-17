//! The road to a paired station: this computer's end of the shack's LAN channel
//! (`remote_service::lan::channel`, whose wire this speaks), for the window's page.
//!
//! 1. **TLS 1.3 with the station's key pinned** (`tls::client::config`). Anything else the address
//!    presents is refused in the handshake, before this computer's own key is sent: TLS 1.3 proves
//!    the server's key first. That is "this station's key has changed" (`keyChanged`). A station
//!    that refuses this computer's key (removed there, or its identity reset) answers the handshake
//!    with an alert (`notPaired`).
//! 2. **The hello**, with the versions this build speaks. The station welcomes this computer under
//!    the device id it gave it at pairing and a session id of its own, or says which side to
//!    update (`updateStation`, `updateComputer`: "Refuse, say which", as ruled on 2026-10-04).
//! 3. **The lane.** The page's operation requests go on as the station's lane takes them: the five
//!    this road carries (state, acquire, heartbeat, release, Stop), anything else answered here as
//!    the station would (`stationUnsupported`), so a page can never close the road with a request
//!    the station cannot parse. Its stream signals go on with every offer signed here with this
//!    computer's key for this station and session (A5): the page never holds the key. What the
//!    station sends comes back: operation answers, stream state, the status line, and stream
//!    signals, the answer only once it is checked against the station's pinned key for that offer
//!    (S3-M1) and its candidates only behind it.
//!
//! Addresses are tried in order, the one the operator typed first, then the remembered ones, the
//! last that worked first; the first to welcome this computer is the road.
use std::collections::VecDeque;
use std::net::SocketAddrV4;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use ring::digest::{digest, SHA256};
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
use rustls::ClientConfig;
use serde::Deserialize;
use serde_json::json;
use tempo_stream::protocol::{self, BrowserSignal, RoomToPage, StationSignal, Validate};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::ComputerKey;
use crate::tls;
use crate::wire::VERSIONS;
use crate::PairedStation;

/// A TCP connection to an address, before anything else: a station on the same network answers
/// at once.
const CONNECT: Duration = Duration::from_secs(3);
/// TLS, the upgrade and the station's first word: the station's own handshake deadline.
pub const HANDSHAKE: Duration = Duration::from_secs(5);
/// How often this computer pings, so the station never takes it for gone (it waits 15 s).
pub const PING_EVERY: Duration = Duration::from_secs(5);
/// The largest message this computer sends: the station's own bound.
const TO_STATION_BYTES: usize = 8192;
/// The largest message the station may send: an operation answer is a few KiB.
const FROM_STATION_BYTES: usize = 64 * 1024;
/// The station's candidates kept while its answer is checked: the page's own bound.
const HELD_CANDIDATES: usize = 64;
/// The operation requests this road carries, as the station's lane does.
const OPERATIONS: [&str; 5] = ["state", "acquire", "heartbeat", "release", "stopTransmit"];
/// The operation version every request on this road is answered at.
const OPERATION_VERSION: u8 = VERSIONS.2;
/// The TLS exporter label of a pairing's proofs: 32 bytes, no context, from each side of this one
/// session (src-tauri's `remote_service::lan::pairing`, the ceremony's both halves).
pub const EXPORTER: &[u8] = b"EXPORTER-nexus-lan-pairing";

/// What a road's TLS runs over: a TCP connection to the station's listener on this network, or this
/// computer's end of a pipe through the station's room on the internet ([`crate::tunnel`]). TLS,
/// the hello and everything after are the same on both.
pub enum Carrier {
    Tcp(TcpStream),
    Pipe(tokio::io::DuplexStream),
}

impl Carrier {
    /// The TCP connection, when it is one.
    pub fn tcp(&self) -> Option<&TcpStream> {
        match self {
            Self::Tcp(tcp) => Some(tcp),
            Self::Pipe(_) => None,
        }
    }
}

impl AsyncRead for Carrier {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(tcp) => Pin::new(tcp).poll_read(cx, buf),
            Self::Pipe(pipe) => Pin::new(pipe).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Carrier {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Tcp(tcp) => Pin::new(tcp).poll_write(cx, buf),
            Self::Pipe(pipe) => Pin::new(pipe).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(tcp) => Pin::new(tcp).poll_flush(cx),
            Self::Pipe(pipe) => Pin::new(pipe).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(tcp) => Pin::new(tcp).poll_shutdown(cx),
            Self::Pipe(pipe) => Pin::new(pipe).poll_shutdown(cx),
        }
    }
}

#[cfg(not(feature = "bench-road-switch"))]
pub type Socket = WebSocketStream<tokio_rustls::client::TlsStream<Carrier>>;
/// In a test build every road to a station is under the road switch it was opened under, if it was
/// opened under one (`crate::bench_road_switch`).
#[cfg(feature = "bench-road-switch")]
pub type Socket =
    WebSocketStream<tokio_rustls::client::TlsStream<crate::bench_road_switch::Gated<Carrier>>>;

/// A TLS session with a station, upgraded: the socket, the key the station presented (SPKI) and
/// this session's exporter, which a pairing's proofs are bound to.
pub struct Opened {
    pub socket: Socket,
    pub station: Vec<u8>,
    pub exporter: [u8; 32],
}

/// Why a connection did not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failed {
    /// Nothing answered, or it went away before the handshake was done.
    Unreachable,
    /// No TCP connection was made: how it ended (`TimedOut` when nothing answered in time), for
    /// `tempo_stream::lan::unreached` to name.
    NotConnected(std::io::ErrorKind),
    /// The address presented a key this computer did not pin.
    KeyChanged,
    /// The station refused this computer's key in the handshake.
    Refused,
}

/// The rustls error inside an I/O error from the TLS stream, if there is one.
fn tls_error(error: &std::io::Error) -> Option<&rustls::Error> {
    error.get_ref()?.downcast_ref::<rustls::Error>()
}

/// What a failed read or write says about the handshake: refused by the station, or anything else.
fn refused_or_gone(error: &std::io::Error) -> Failed {
    match tls_error(error) {
        // What a station's verifier answers a key it does not take (`tls::PinnedClients`).
        Some(rustls::Error::AlertReceived(rustls::AlertDescription::AccessDenied)) => {
            Failed::Refused
        }
        Some(rustls::Error::InvalidCertificate(_)) => Failed::KeyChanged,
        _ => Failed::Unreachable,
    }
}

fn socket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(FROM_STATION_BYTES))
        .max_frame_size(Some(FROM_STATION_BYTES))
}

/// TCP, TLS with `config`, and the upgrade, at `at`.
pub async fn open(at: SocketAddrV4, config: Arc<ClientConfig>) -> Result<Opened, Failed> {
    // A test build's road switch with the command link blocked: no connection leaves this
    // computer, and it is given up after the connect's own wait, as an unanswered one is.
    #[cfg(feature = "bench-road-switch")]
    let switch = crate::bench_road_switch::Switch::opening();
    #[cfg(feature = "bench-road-switch")]
    if switch
        .as_ref()
        .is_some_and(|switch| switch.blocks(crate::bench_road_switch::Road::Link))
    {
        tokio::time::sleep(CONNECT).await;
        return Err(Failed::NotConnected(std::io::ErrorKind::TimedOut));
    }
    let tcp = match tokio::time::timeout(CONNECT, TcpStream::connect(at)).await {
        Ok(Ok(tcp)) => tcp,
        Ok(Err(error)) => return Err(Failed::NotConnected(error.kind())),
        Err(_) => return Err(Failed::NotConnected(std::io::ErrorKind::TimedOut)),
    };
    // Every frame goes as it is written, the station's way too: a Stop is a small frame, and
    // Nagle can hold one for the acknowledgement of the frame before (about 40 ms, measured).
    let _ = tcp.set_nodelay(true);
    let carrier = Carrier::Tcp(tcp);
    #[cfg(feature = "bench-road-switch")]
    let carrier = crate::bench_road_switch::Gated::new(carrier, switch);
    secure(carrier, config).await
}

/// TLS with `config` and the upgrade over this computer's end of a pipe through the station's room
/// ([`crate::tunnel`]). In a test build it is under the road switch it was opened under, as a TCP
/// connection is, so a blocked command link blocks it too.
pub async fn open_pipe(
    pipe: tokio::io::DuplexStream,
    config: Arc<ClientConfig>,
) -> Result<Opened, Failed> {
    #[cfg(feature = "bench-road-switch")]
    let switch = crate::bench_road_switch::Switch::opening();
    #[cfg(feature = "bench-road-switch")]
    if switch
        .as_ref()
        .is_some_and(|switch| switch.blocks(crate::bench_road_switch::Road::Link))
    {
        tokio::time::sleep(CONNECT).await;
        return Err(Failed::NotConnected(std::io::ErrorKind::TimedOut));
    }
    let carrier = Carrier::Pipe(pipe);
    #[cfg(feature = "bench-road-switch")]
    let carrier = crate::bench_road_switch::Gated::new(carrier, switch);
    secure(carrier, config).await
}

#[cfg(not(feature = "bench-road-switch"))]
type Carried = Carrier;
#[cfg(feature = "bench-road-switch")]
type Carried = crate::bench_road_switch::Gated<Carrier>;

/// TLS with `config`, the station's key checked before this computer's own is sent, and the
/// upgrade, over `carrier`.
async fn secure(carrier: Carried, config: Arc<ClientConfig>) -> Result<Opened, Failed> {
    let name = rustls::pki_types::ServerName::try_from("nexus-station")
        .map_err(|_| Failed::Unreachable)?;
    let tls = tokio_rustls::TlsConnector::from(config)
        .connect(name, carrier)
        .await
        .map_err(|e| refused_or_gone(&e))?;
    let station = tls
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|keys| keys.first())
        .ok_or(Failed::Unreachable)?
        .as_ref()
        .to_vec();
    let exporter = tls
        .get_ref()
        .1
        .export_keying_material([0; 32], EXPORTER, None)
        .map_err(|_| Failed::Unreachable)?;
    // In TLS 1.3 the station judges this computer's key after this computer's side of the
    // handshake is done, so its refusal is the first thing read here.
    let (socket, _) = tokio_tungstenite::client_async_with_config(
        "ws://nexus-station/",
        tls,
        Some(socket_config()),
    )
    .await
    .map_err(|e| match e {
        tokio_tungstenite::tungstenite::Error::Io(io) => refused_or_gone(&io),
        _ => Failed::Unreachable,
    })?;
    Ok(Opened {
        socket,
        station,
        exporter,
    })
}

/// Send one text message.
pub async fn say(socket: &mut Socket, text: String) -> Result<(), ()> {
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// What the station says first on a road.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum First {
    Welcome {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        #[serde(rename = "stationId")]
        station_id: String,
        /// The station's build (`remote_service::lan::wire`): what the command link must match.
        app: String,
        contract: String,
        /// Stop's second road for this connection, when the station opened one.
        #[serde(rename = "stopRoad", default)]
        stop_road: Option<RoadOffer>,
        /// The audio road for this connection, when the station opened one.
        #[serde(rename = "audioRoad", default)]
        audio_road: Option<RoadOffer>,
    },
    Refused {
        reason: String,
    },
}

/// Stop's second road or the audio road as a welcome names it (`remote_service::lan::stop_road`,
/// `remote_service::lan::audio_road`): the station's UDP port, on the address that welcomed this
/// computer, and the road's tag.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoadOffer {
    pub port: u16,
    /// Eight bytes, as sixteen lowercase hexadecimal characters.
    pub tag: String,
}

impl RoadOffer {
    /// The tag's eight bytes, or `None` for a tag or a port no station gives.
    pub fn tag(&self) -> Option<[u8; 8]> {
        let shaped = self.tag.len() == 16
            && self
                .tag
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        if !shaped || self.port == 0 {
            return None;
        }
        protocol::hex_bytes(&self.tag)?.try_into().ok()
    }
}

/// The reasons that say nothing answered (`tempo_stream::lan::unreached`'s, and `unreachable` for a
/// connection that went away before the handshake was done), as against a station that answered
/// and said no.
pub const UNREACHED: [&str; 4] = ["unreachable", "otherNetwork", "refused", "noAnswer"];

/// How much a reason says, for the one to tell when no address welcomed this computer. Nothing
/// answering says least, and of its words a refusal says most: the station's computer answered.
pub fn weight(reason: &str) -> u8 {
    match reason {
        "updateStation" | "updateComputer" | "otherBuild" => 7,
        "notPaired" => 6,
        "keyChanged" => 5,
        "notStation" => 4,
        "refused" => 3,
        "otherNetwork" => 2,
        "noAnswer" => 1,
        _ => 0,
    }
}

/// The ids a road was opened under: the station's, the device id it gave this computer, and the
/// session it stamped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ids {
    pub station: String,
    pub device: String,
    pub session: String,
}

/// What a station welcomed this computer with: the session it stamped, its build, and Stop's
/// second road and the audio road, if it opened them.
pub struct Welcomed {
    pub session: String,
    pub app: String,
    pub contract: String,
    pub stop_road: Option<RoadOffer>,
    pub audio_road: Option<RoadOffer>,
}

/// The hello at `at`: the socket and what the station welcomed it with, or why not.
async fn hello(
    at: SocketAddrV4,
    config: Arc<ClientConfig>,
    record: &PairedStation,
) -> Result<(Socket, Welcomed), &'static str> {
    let opened = open(at, config).await.map_err(|failed| match failed {
        // Nothing answered at the address: why, in the words the card uses for it.
        Failed::NotConnected(how) => {
            tempo_stream::lan::unreached(&how.into(), tempo_stream::lan::here(*at.ip()))
        }
        failed => said(failed),
    })?;
    hello_on(opened, record).await
}

/// Why a TLS session did not open, in the card's words.
fn said(failed: Failed) -> &'static str {
    match failed {
        Failed::Unreachable | Failed::NotConnected(_) => "unreachable",
        Failed::KeyChanged => "keyChanged",
        Failed::Refused => "notPaired",
    }
}

/// The hello on a TLS session open to the station `record` names: the socket and what the station
/// welcomed it with, or why not.
async fn hello_on(
    Opened { mut socket, .. }: Opened,
    record: &PairedStation,
) -> Result<(Socket, Welcomed), &'static str> {
    let (protocol, stream, operation) = VERSIONS;
    let hello = json!({"type":"hello","protocol":protocol,"stream":stream,"operation":operation});
    say(&mut socket, hello.to_string())
        .await
        .map_err(|_| "unreachable")?;
    let first = loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => break text,
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            // Through TLS and then closed without a word: the station no longer has this key
            // paired (removed while this computer connected).
            _ => return Err("notPaired"),
        }
    };
    match serde_json::from_str::<First>(&first) {
        Ok(First::Welcome {
            session_id,
            device_id,
            station_id,
            app,
            contract,
            stop_road,
            audio_road,
        }) if device_id == record.device_id
            && station_id == record.station_id
            && protocol::identifier(&session_id) =>
        {
            let welcomed = Welcomed {
                session: session_id,
                app,
                contract,
                stop_road,
                audio_road,
            };
            Ok((socket, welcomed))
        }
        Ok(First::Refused { reason }) if reason == "updateStation" => Err("updateStation"),
        Ok(First::Refused { reason }) if reason == "updateComputer" => Err("updateComputer"),
        _ => Err("notStation"),
    }
}

/// A look by name (`tempo_stream::lan::dnssd::find`, or a test's own): the stations that answer
/// within a wait. It blocks for that long, so it is only called on a thread that may wait.
pub type Look = Arc<
    dyn Fn(
            Duration,
        )
            -> Result<Vec<tempo_stream::lan::dnssd::Found>, tempo_stream::lan::dnssd::Unavailable>
        + Send
        + Sync,
>;

/// Whether a road that did not open is worth a look by name: nothing answered at the addresses
/// tried, or another key did (another station at an address that was this one's).
pub fn look_further(why: &str) -> bool {
    UNREACHED.contains(&why) || why == "keyChanged"
}

/// Where a look by name finds the station `record` names, at addresses not in `tried`: each advert
/// whose key tag starts the fingerprint of the key pinned for it, so another station's is never
/// tried (it could only fail the handshake, and say the key had changed).
pub fn found_for(
    record: &PairedStation,
    tried: &[SocketAddrV4],
    found: Vec<tempo_stream::lan::dnssd::Found>,
) -> Vec<SocketAddrV4> {
    let Some(pinned) = super::StationView::of(record) else {
        return Vec::new();
    };
    let mut at = Vec::new();
    for shack in found {
        if pinned.key.starts_with(&shack.key)
            && !tried.contains(&shack.address)
            && !at.contains(&shack.address)
        {
            at.push(shack.address);
        }
    }
    at
}

/// What is told when no address welcomed this computer: `why`, the addresses tried first; `later`,
/// those a look by name found, if it found any. Another key at a tried address is told unless what
/// was found by name told more; an advert that answered with another key is a forged or foreign
/// one, since an advert is a hint and never an identity, so it is never told as this station's key
/// having changed.
pub fn told(why: &'static str, later: &'static str, found: bool) -> &'static str {
    if !found || (why == "keyChanged" && weight(later) <= weight(why)) {
        why
    } else if later == "keyChanged" {
        "notThisStation"
    } else {
        later
    }
}

/// The addresses the road to the station `record` names is tried at, in turn: `typed` first when
/// the operator gave an address, then each remembered one, the last that worked first.
pub fn order(record: &PairedStation, typed: Option<SocketAddrV4>) -> Vec<SocketAddrV4> {
    let mut order: Vec<SocketAddrV4> = typed.into_iter().collect();
    for remembered in record
        .addresses
        .iter()
        .filter_map(|a| tempo_stream::lan::typed(a))
    {
        if !order.contains(&remembered) {
            order.push(remembered);
        }
    }
    order
}

/// Open the road to the station `record` names: at `typed` first when the operator gave an
/// address, then at each remembered one in turn. The road and the address that welcomed this
/// computer, or the most telling reason none did.
pub async fn connect(
    record: &PairedStation,
    typed: Option<SocketAddrV4>,
) -> Result<(Road, SocketAddrV4), &'static str> {
    connect_at(record, &order(record, typed)).await
}

/// Open the road to the station `record` names at each of `order` in turn: the road and the
/// address that welcomed this computer, or the most telling reason none did (`unreachable`, with
/// nowhere to try).
pub async fn connect_at(
    record: &PairedStation,
    order: &[SocketAddrV4],
) -> Result<(Road, SocketAddrV4), &'static str> {
    let key = ComputerKey::restore(&record.pkcs8).ok_or("storeUnavailable")?;
    let reached = reach(record, order).await?;
    let road = Road::new(reached.socket, key, record.station_key.clone(), reached.ids);
    Ok((road, reached.at))
}

/// A station welcomed this computer: the socket, the ids it was welcomed under, the station's
/// build, Stop's second road and the audio road if it opened them, and the address that answered.
pub struct Reached {
    pub socket: Socket,
    pub ids: Ids,
    pub app: String,
    pub contract: String,
    pub stop_road: Option<RoadOffer>,
    pub audio_road: Option<RoadOffer>,
    pub at: SocketAddrV4,
}

/// The hello to the station `record` names at each of `order` in turn, the window's road and the
/// command link alike: the first that welcomes this computer, or the most telling reason none did
/// (`unreachable`, with nowhere to try).
pub async fn reach(
    record: &PairedStation,
    order: &[SocketAddrV4],
) -> Result<Reached, &'static str> {
    let config =
        tls::client::config(&record.pkcs8, &record.station_key).ok_or("storeUnavailable")?;
    let mut told = "unreachable";
    for &at in order {
        let reached = tokio::time::timeout(HANDSHAKE, hello(at, config.clone(), record))
            .await
            .unwrap_or(Err("unreachable"));
        match reached {
            Ok((socket, welcomed)) => {
                let ids = Ids {
                    station: record.station_id.clone(),
                    device: record.device_id.clone(),
                    session: welcomed.session,
                };
                return Ok(Reached {
                    socket,
                    ids,
                    app: welcomed.app,
                    contract: welcomed.contract,
                    stop_road: welcomed.stop_road,
                    audio_road: welcomed.audio_road,
                    at,
                });
            }
            Err(reason) if weight(reason) > weight(told) => told = reason,
            Err(_) => {}
        }
    }
    Err(told)
}

/// The hello to the station `record` names through this computer's end of a pipe through its room
/// ([`crate::tunnel`]): TLS 1.3 with this computer's key for it and its station key pinned, then the
/// same hello as on this network. What it welcomed this computer with, or why not; a road of this
/// network's it names is never taken here.
pub async fn reach_through(
    pipe: tokio::io::DuplexStream,
    record: &PairedStation,
) -> Result<(Socket, Ids, Welcomed), &'static str> {
    let config =
        tls::client::config(&record.pkcs8, &record.station_key).ok_or("storeUnavailable")?;
    let reached = async {
        let opened = open_pipe(pipe, config).await.map_err(said)?;
        hello_on(opened, record).await
    };
    let (socket, welcomed) = tokio::time::timeout(HANDSHAKE, reached)
        .await
        .unwrap_or(Err("unreachable"))?;
    let ids = Ids {
        station: record.station_id.clone(),
        device: record.device_id.clone(),
        session: welcomed.session.clone(),
    };
    Ok((socket, ids, welcomed))
}

/// Is `answer` the station's own (S3-M1): signed with `station_key` (SPKI, lowercase hex, the key
/// this computer pinned) over both DTLS fingerprints and the three ids, for exactly `offer`?
/// `stationNotSigned` with no signature line, `stationKeyMismatch` for any other failure: the
/// page's own words for the same two refusals (`station-key.ts`).
pub fn check_answer(
    station_key: &str,
    answer: &str,
    offer: &str,
    ids: &Ids,
) -> Result<(), &'static str> {
    let mismatch = "stationKeyMismatch";
    let signature = answer
        .lines()
        .find_map(|line| line.strip_prefix(protocol::ANSWER_SIGNATURE_ATTRIBUTE))
        .ok_or("stationNotSigned")?;
    if signature.len() != protocol::SIGNATURE_HEX_CHARS {
        return Err(mismatch);
    }
    let hashed = |sdp: &str| -> Option<[u8; 32]> {
        digest(&SHA256, &protocol::offer_fingerprint(sdp)?)
            .as_ref()
            .try_into()
            .ok()
    };
    let (answered, offered) = (
        hashed(answer).ok_or(mismatch)?,
        hashed(offer).ok_or(mismatch)?,
    );
    let bound =
        protocol::answer_binding(&answered, &offered, &ids.station, &ids.device, &ids.session);
    let spki = protocol::hex_bytes(station_key).ok_or(mismatch)?;
    let point = spki.get(spki.len().saturating_sub(65)..).ok_or(mismatch)?;
    let signature = protocol::hex_bytes(signature).ok_or(mismatch)?;
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
        .verify(&bound, &signature)
        .map_err(|_| mismatch)
}

/// What the page is told from the road.
#[derive(Debug, PartialEq)]
pub enum ToPage {
    /// One of the station's own messages, as it sent it.
    Station(String),
    /// The station's answer did not hold, and was not passed on: the stream is ended at the
    /// station too.
    AnswerRefused(&'static str),
}

/// Where the answer to the last offer stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// No offer made, or its answer not here yet.
    Awaited,
    /// Checked, and it held: its candidates go to the page.
    Held,
    /// Checked, and refused: nothing more of that negotiation goes to the page.
    Refused,
}

/// The road, open: this computer welcomed by the station, under [`Ids`].
pub struct Road {
    sink: SplitSink<Socket, Message>,
    stream: SplitStream<Socket>,
    key: ComputerKey,
    /// The station's key, SPKI as lowercase hex, as this computer pinned it.
    station_key: String,
    pub ids: Ids,
    /// The last offer signed here, and the lease it went under.
    offered: Option<(String, String)>,
    answer: Answer,
    /// The station's candidates for the last offer, while its answer is checked.
    held: Vec<String>,
    /// What the page is to be told next, in order.
    outbox: VecDeque<ToPage>,
    /// When to ping next.
    ping: tokio::time::Interval,
}

impl Road {
    fn new(socket: Socket, key: ComputerKey, station_key: String, ids: Ids) -> Self {
        let (sink, stream) = socket.split();
        let mut ping = tokio::time::interval(PING_EVERY);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self {
            sink,
            stream,
            key,
            station_key,
            ids,
            offered: None,
            answer: Answer::Awaited,
            held: Vec::new(),
            outbox: VecDeque::new(),
            ping,
        }
    }

    async fn say(&mut self, text: String) -> Result<(), ()> {
        if text.len() > TO_STATION_BYTES {
            return Err(());
        }
        self.sink
            .send(Message::Text(text.into()))
            .await
            .map_err(|_| ())
    }

    /// One of the page's operation requests, on to the station. `Ok(Some(answer))` for a request
    /// answered here instead, as the station would answer it: one this road does not carry, or
    /// one asked at another operation version. `Err`: the road has gone.
    pub async fn operation(
        &mut self,
        version: Option<u8>,
        request: &serde_json::Value,
    ) -> Result<Option<String>, ()> {
        let (Some(kind), Some(request_id)) = (
            request["type"].as_str(),
            request["requestId"]
                .as_str()
                .filter(|id| protocol::identifier(id)),
        ) else {
            return Ok(None);
        };
        if !OPERATIONS.contains(&kind) || version.is_some_and(|v| v != OPERATION_VERSION) {
            let answer = json!({"type":"operationResponse","requestId":request_id,
                "error":"stationUnsupported"});
            return Ok(Some(answer.to_string()));
        }
        let text = json!({"type":"operationRequest","request":request}).to_string();
        self.say(text).await.map(|_| None)
    }

    /// One of the page's stream signals, on to the station: an offer signed here first, with this
    /// computer's key for this station and this session (A5).
    pub async fn signal(&mut self, lease: String, payload: BrowserSignal) -> Result<(), ()> {
        if !protocol::identifier(&lease) {
            return Err(());
        }
        let payload = match payload {
            BrowserSignal::Offer { sdp, .. } => {
                let signature = self.key.sign_offer(
                    &sdp,
                    &self.ids.station,
                    &self.ids.device,
                    &self.ids.session,
                );
                self.offered = Some((sdp.clone(), lease.clone()));
                self.answer = Answer::Awaited;
                self.held.clear();
                BrowserSignal::Offer {
                    sdp,
                    public_key: signature
                        .as_ref()
                        .map(|_| self.key.public_key().to_string()),
                    signature,
                }
            }
            other => other,
        };
        if !payload.valid() {
            return Err(());
        }
        let text = json!({"type":"streamSignal","leaseId":lease,"payload":payload}).to_string();
        self.say(text).await
    }

    /// The next thing to tell the page, or `None` once the station has gone (or said something no
    /// station says, which ends the road as surely). While it waits, it pings the station every
    /// [`PING_EVERY`], so the station never takes this computer for gone.
    pub async fn next(&mut self) -> Option<ToPage> {
        loop {
            if let Some(told) = self.outbox.pop_front() {
                return Some(told);
            }
            let text = tokio::select! {
                message = self.stream.next() => match message {
                    Some(Ok(Message::Text(text))) => text.to_string(),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => return None,
                },
                _ = self.ping.tick() => {
                    if self.sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                        return None;
                    }
                    continue;
                }
            };
            self.heard(text).await?;
        }
    }

    /// One message from the station, sorted into the outbox. `None`: not a station's message.
    async fn heard(&mut self, text: String) -> Option<()> {
        #[derive(Deserialize)]
        struct Typed {
            #[serde(rename = "type")]
            kind: String,
        }
        let kind = serde_json::from_str::<Typed>(&text).ok()?.kind;
        match kind.as_str() {
            "operationResponse" | "status" | "streamState" => {
                self.outbox.push_back(ToPage::Station(text));
            }
            "streamSignal" => {
                let RoomToPage::StreamSignal { payload } = serde_json::from_str(&text).ok()? else {
                    return None;
                };
                if !payload.valid() {
                    return None;
                }
                match payload {
                    StationSignal::Answer { sdp } => self.answered(&sdp, text).await,
                    StationSignal::Candidate { .. } => match self.answer {
                        Answer::Held => self.outbox.push_back(ToPage::Station(text)),
                        Answer::Awaited if self.held.len() < HELD_CANDIDATES => {
                            self.held.push(text)
                        }
                        Answer::Awaited | Answer::Refused => {}
                    },
                }
            }
            _ => return None,
        }
        Some(())
    }

    /// The station's answer to the last offer: on to the page if it holds, with the candidates
    /// that came before it was checked; otherwise refused, and the stream ended at the station.
    async fn answered(&mut self, sdp: &str, text: String) {
        let Some((offer, lease)) = self.offered.clone() else {
            return;
        };
        if self.answer != Answer::Awaited {
            return;
        }
        match check_answer(&self.station_key, sdp, &offer, &self.ids) {
            Ok(()) => {
                self.answer = Answer::Held;
                self.outbox.push_back(ToPage::Station(text));
                for candidate in std::mem::take(&mut self.held) {
                    self.outbox.push_back(ToPage::Station(candidate));
                }
            }
            Err(reason) => {
                // The page is told first: whatever happens to the close, it never takes this
                // negotiation further.
                self.answer = Answer::Refused;
                self.held.clear();
                self.outbox.push_back(ToPage::AnswerRefused(reason));
                let close = json!({"type":"streamSignal","leaseId":lease,"payload":BrowserSignal::Close {}});
                let _ = self.say(close.to_string()).await;
            }
        }
    }

    /// Close the road. The station ends this computer's stream and lease with it.
    pub async fn close(mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(1), self.sink.close()).await;
    }
}
