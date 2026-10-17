//! Client mode over the internet road, end to end: this computer's client (`crate::client_mode`)
//! on Tauri's mock runtime, its station's addresses on this network answering nothing, and the
//! station reached instead through the shack's real tunnel behind an in-memory room, by a stand-in
//! for the Remote service ([`Afar`]) that opens the pipe, counts each renewal and can refuse one.
//! The shack is the real one, its commands running; its internet roads bind loopback, and this
//! computer's go through a TURN stand-in, or through nothing at all where this network is said
//! to block UDP.
use super::commands::{running, App};
use super::computer::{behind_a_port, paired_record};
use super::tunnel::{approved_for, piped, through_record, tunnel_of, APPROVED};
use super::*;
use crate::client_mode::tests::{ask, eventually, scratch, screens};
use crate::client_mode::{Afar, Client, Find, Phase, ThisComputer, Timing};
use crate::lan_client::audio_road::{Headset, CUSHION_MS, INTERNET_CUSHION_MS};
use crate::lan_client::link::{Internet, Link};
use crate::lan_client::tests::where_nothing_listens;
use crate::remote_service::tunnel::Approved;
use crate::remote_service::vault::PairedStation;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use tauri::Manager;
use tempo_client::ice::Relay;
use tempo_client::turn::stand_in::StandIn;

const USER: &str = "road";
const PASSWORD: &str = "throwaway-password";

/// The Remote service as client mode meets it: a pipe to the shack's tunnel each time it is asked
/// to open, its roads through the TURN stand-in at `relay` (or, with none, gathering nothing: a
/// network that lets no UDP through), and each renewal counted and answered as `renewal` says.
struct Service {
    tunnel: Tunnel,
    stop: watch::Receiver<bool>,
    station_key: String,
    pkcs8: String,
    relay: Option<std::net::SocketAddr>,
    road_kept: AtomicBool,
    opens: AtomicUsize,
    renewals: AtomicUsize,
    renewal: Mutex<Result<(), &'static str>>,
}

impl Afar for Service {
    fn has_road(&self, _: &str) -> bool {
        self.road_kept.load(Ordering::SeqCst)
    }

    fn open(
        &self,
        _: String,
        headset: Option<Arc<dyn Headset>>,
    ) -> Pin<Box<dyn Future<Output = Result<Link, &'static str>> + Send>> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let (tunnel, stop, relay) = (self.tunnel.clone(), self.stop.clone(), self.relay);
        let record = PairedStation {
            addresses: Vec::new(),
            station_id: STATION.into(),
            device_id: APPROVED.into(),
            station_key: self.station_key.clone(),
            pkcs8: self.pkcs8.clone(),
        };
        Box::pin(async move {
            let internet = Internet {
                bind: Ipv4Addr::LOCALHOST.into(),
                direct: false,
                stun: None,
                relay: relay.map(|server| Relay {
                    server,
                    username: USER.into(),
                    password: PASSWORD.into(),
                }),
            };
            let pipe = piped(&tunnel, stop).await;
            Link::connect_tunnel(pipe, &record, 41, headset, &internet).await
        })
    }

    fn renew(&self, _: String) -> Pin<Box<dyn Future<Output = Result<(), &'static str>> + Send>> {
        self.renewals.fetch_add(1, Ordering::SeqCst);
        let said = *self.renewal.lock().unwrap();
        Box::pin(async move { said })
    }
}

/// The shack, running, with its tunnel taking this computer; client mode on this computer with its
/// station's address on this network at `lan` (shared, so a test can move it), the stand-in
/// service, the clocks a test can wait for, and a stand-in headset.
struct Afield {
    app: App,
    s: Shack,
    client: Arc<Client>,
    screens: crate::client_mode::tests::Screens,
    service: Arc<Service>,
    /// The station's address on this network, as the operator typed it: nothing listens there
    /// until a test puts the station's listener there ([`Afield::on_this_network`]).
    lan: SocketAddrV4,
    headset: Arc<tempo_client::audio_road::StandIn>,
    driving: tauri::async_runtime::JoinHandle<()>,
    dir: std::path::PathBuf,
    _turn: Option<StandIn>,
    _listening: watch::Sender<bool>,
}

impl Afield {
    fn new(label: &str, udp: bool, timing: Timing) -> Self {
        Self::kept(label, udp, timing, true)
    }

    /// The same, with an internet road kept for the station or none.
    fn kept(label: &str, udp: bool, timing: Timing, kept: bool) -> Self {
        let app = App::new();
        let s = running(&app);
        // The tunnel grants no control of its own: the shack gives it, as to any computer.
        s.shared.authority.permit_station(APPROVED, true).unwrap();
        let approved = Arc::new(Approved::default());
        approved.set(approved_for(Some(&s.computer)));
        let tunnel = tunnel_of(&s, approved.roster(), &approved);
        let (listening, stop) = watch::channel(false);
        let turn = udp.then(|| StandIn::start(USER, PASSWORD));
        let record = through_record(&s, &s.public_key);
        let service = Arc::new(Service {
            tunnel,
            stop,
            station_key: record.station_key.clone(),
            pkcs8: record.pkcs8.clone(),
            relay: turn.as_ref().map(|turn| turn.address),
            road_kept: AtomicBool::new(kept),
            opens: AtomicUsize::new(0),
            renewals: AtomicUsize::new(0),
            renewal: Mutex::new(Ok(())),
        });
        let dir = scratch(label);
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        let headset = tempo_client::audio_road::StandIn::new();
        client.record_from(Arc::clone(&headset) as Arc<dyn Headset>);
        client.reach_afar(Arc::clone(&service) as Arc<dyn Afar>);
        client.time_with(timing);
        let screens = screens(&client);
        let lan = where_nothing_listens(Ipv4Addr::LOCALHOST);
        let find: Find = {
            // This network's own record: the device id the station's LAN listener gave.
            let (key, pkcs8, device) = (
                record.station_key.clone(),
                record.pkcs8.clone(),
                s.device.clone(),
            );
            Arc::new(move |_| {
                Ok(Some(PairedStation {
                    station_id: STATION.into(),
                    device_id: device.clone(),
                    station_key: key.clone(),
                    pkcs8: pkcs8.clone(),
                    addresses: vec![lan.to_string()],
                }))
            })
        };
        let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
            screens.main.app_handle().clone(),
            find,
            Arc::new(|_| Ok(Vec::new())),
            Some(lan),
        ));
        Self {
            app,
            s,
            client,
            screens,
            service,
            lan,
            headset,
            driving,
            dir,
            _turn: turn,
            _listening: listening,
        }
    }

    fn live_over_the_internet(&self) {
        eventually("control taken over the internet", || {
            let status = self.client.status();
            status.phase == Phase::Live && status.internet
        });
    }

    /// The station's listener on this network, at the address the operator typed.
    fn on_this_network(&self, stop: watch::Receiver<bool>) {
        let shared = self.s.shared.clone();
        let at = self.lan;
        tauri::async_runtime::block_on(async move {
            let listener = tokio::net::TcpListener::bind(at).await.unwrap();
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(channel::connection(
                        stream,
                        PEER.parse().unwrap(),
                        shared.clone(),
                        stop.clone(),
                    ));
                }
            });
        });
    }

    fn end(self) {
        self.driving.abort();
        tauri::async_runtime::block_on(self.client.let_go());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The clocks a test can wait for: the datagram roads' grace, a try of this network, "Still
/// there?" and its end, and the pipe's renewal, each far shorter than the app's.
fn quick() -> Timing {
    Timing {
        udp_grace: Duration::from_millis(1500),
        lan_probe: Duration::from_millis(300),
        idle_prompt: Duration::from_secs(60),
        idle_end: Duration::from_secs(61),
        renew: Duration::from_millis(200),
    }
}

/// ★ With nothing answering at the station's address on this network, client mode drives it over
/// the internet road: control taken through the room, the shack's snapshot answering the screens'
/// poll, a station command run at the shack, the receive cushion at the internet's 120 ms, the
/// pipe renewed while the link lives, and the banner told it is the internet. CONTROL: with no
/// internet road kept for the station, nothing is opened over the internet and the station is said
/// to be out of reach, as before.
#[test]
fn off_this_network_client_mode_drives_the_station_over_the_internet() {
    let _alone = alone();
    let t = Afield::new("afar", true, quick());
    t.live_over_the_internet();
    eventually("the shack's snapshot here", || {
        ask(&t.screens.main, "get_snapshot", json!({})).is_ok_and(|s| s["n"].is_u64())
    });
    assert!(ask(&t.screens.main, "get_settings", json!({})).is_ok());
    assert!(t.app.ran().contains(&"get_settings".to_string()));
    assert_eq!(t.headset.rings().cushion_ms(), INTERNET_CUSHION_MS);
    eventually("the pipe renewed", || {
        t.service.renewals.load(Ordering::SeqCst) >= 2
    });
    assert!(!t.client.status().udp_blocked, "{:?}", t.client.status());
    assert_eq!(t.service.opens.load(Ordering::SeqCst), 1);
    t.end();

    let t = Afield::kept("afar-none", true, quick(), false);
    eventually("said to be out of reach", || {
        let reason = t.client.status().reason;
        reason.is_some_and(|reason| crate::lan_client::road::UNREACHED.contains(&reason.as_str()))
    });
    std::thread::sleep(Duration::from_millis(1000));
    let status = t.client.status();
    assert_eq!(
        t.service.opens.load(Ordering::SeqCst),
        0,
        "opened over the internet with no road kept"
    );
    assert!(
        !status.internet && status.phase != Phase::Live,
        "{status:?}"
    );
    t.end();
}

/// ⛔ Over the internet on a network that lets no UDP through, the datagram roads never find a
/// path: the banner says so, a start and a press are refused here as `udpBlocked` with nothing
/// sent, and the shack never keys; Stop still goes over the link and runs at the shack. CONTROL:
/// the same link with a relay is not said to be blocked (the test above).
#[test]
fn with_udp_blocked_over_the_internet_nothing_starts_and_stop_still_runs() {
    let _alone = alone();
    let t = Afield::new("afar-blocked", false, quick());
    t.live_over_the_internet();
    eventually("said to be blocked", || t.client.status().udp_blocked);
    assert_eq!(
        ask(&t.screens.main, "call_cq", json!({})),
        Err(json!("nexusClient:udpBlocked"))
    );
    assert_eq!(
        ask(&t.screens.main, "set_ptt", json!({"on": true})),
        Err(json!("nexusClient:udpBlocked"))
    );
    assert!(ask(&t.screens.main, "halt_tx", json!({})).is_ok());
    let ran = t.app.ran();
    assert!(!ran.contains(&"call_cq".to_string()), "{ran:?}");
    assert!(ran.contains(&"halt_tx".to_string()), "{ran:?}");
    assert!(!t.s.shared.engine.lock().unwrap().mic_armed());
    t.end();
}

/// ★ "Still there?" on the internet: with nothing from the operator the banner asks, then the
/// link ends, control is handed back at once (another computer finds it free), and nothing
/// connects again until the operator answers; then it does. CONTROL: input reported before the
/// prompt keeps the link live past the end.
#[test]
fn nobody_still_there_ends_the_internet_link_until_the_operator_answers() {
    let _alone = alone();
    let timing = Timing {
        idle_prompt: Duration::from_millis(2500),
        idle_end: Duration::from_millis(4000),
        ..quick()
    };
    let t = Afield::new("afar-idle", true, timing);
    t.live_over_the_internet();
    // CONTROL: the operator at work keeps it live past the end.
    for _ in 0..12 {
        t.client.still_here();
        std::thread::sleep(Duration::from_millis(500));
    }
    let status = t.client.status();
    assert!(
        status.phase == Phase::Live && !status.still_there,
        "{status:?}"
    );
    eventually("asked", || t.client.status().still_there);
    eventually("ended", || {
        let status = t.client.status();
        status.phase == Phase::Refused && status.reason.as_deref() == Some("idle")
    });
    let opened = t.service.opens.load(Ordering::SeqCst);
    // Control was handed back: another computer on this network, given control, finds it free.
    t.s.shared
        .authority
        .permit_station(&t.s.device, true)
        .unwrap();
    let (_stopper, stop) = watch::channel(false);
    let at = tauri::async_runtime::block_on(behind_a_port(&t.s, stop));
    let free = tauri::async_runtime::block_on(async {
        let (other, _) = Link::connect(&paired_record(&t.s, at), Some(at), 7, None)
            .await
            .expect("another link");
        let mut phase = Value::Null;
        for _ in 0..100 {
            let answer = other
                .operation(json!({"type":"state","requestId":id()}))
                .await
                .expect("an answer");
            if answer["error"] != "stationBusy" && answer["error"] != "remoteBusy" {
                phase = answer["value"]["phase"].clone();
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        other.close();
        phase
    });
    assert_eq!(free, "available");
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        t.service.opens.load(Ordering::SeqCst),
        opened,
        "connected again with nobody there"
    );
    t.client.still_here();
    eventually("connected again", || {
        t.service.opens.load(Ordering::SeqCst) > opened
    });
    t.live_over_the_internet();
    t.end();
}

/// ★ LAN first: while on the internet, the station answering on this network again ends the
/// internet link for this network's road, with the LAN cushion back. CONTROL: while it answers
/// nothing here, the link stays on the internet across several tries.
#[test]
fn the_station_on_this_network_again_takes_this_networks_road() {
    let _alone = alone();
    let t = Afield::new("afar-lan", true, quick());
    t.live_over_the_internet();
    std::thread::sleep(Duration::from_millis(1500));
    let status = t.client.status();
    assert!(status.internet && status.phase == Phase::Live, "{status:?}");
    assert_eq!(t.service.opens.load(Ordering::SeqCst), 1);
    let (_stopper, stop) = watch::channel(false);
    t.s.shared
        .authority
        .permit_station(&t.s.device, true)
        .unwrap();
    t.on_this_network(stop);
    let at = t.lan;
    let mut last = t.client.status();
    for _ in 0..100 {
        last = t.client.status();
        if last.phase == Phase::Live && !last.internet && last.station == at.to_string() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        last.phase == Phase::Live && !last.internet && last.station == at.to_string(),
        "never on this network at {at}: {last:?}, opened over the internet {} times",
        t.service.opens.load(Ordering::SeqCst)
    );
    assert_eq!(t.headset.rings().cushion_ms(), CUSHION_MS);
    t.end();
}

/// ★ A renewal the service refuses for good (the device no longer approved there) ends the link,
/// and the next open's refusal is said. CONTROL: renewals answered keep it live (the first test).
#[test]
fn a_renewal_refused_for_good_ends_the_internet_link() {
    let _alone = alone();
    let t = Afield::new("afar-renew", true, quick());
    t.live_over_the_internet();
    *t.service.renewal.lock().unwrap() = Err("notApprovedThere");
    eventually("the link ended", || t.client.status().phase != Phase::Live);
    t.end();
}
