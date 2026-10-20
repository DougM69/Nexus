//! A computer enrolling on this network by its own sign-in, with no code and no press: its proof is
//! checked at the shack from what the station keeps (internet down), for the key its TLS session
//! proved, and only then is it paired.
//!
//! The proofs are tempo-client's account fixture, minted once for public keys whose private halves
//! were never kept. So the admission itself is driven directly ([`lan_enrol::admit`], what the session
//! runs), and over TLS a fresh key shows that a valid proof for another key is refused.
//!
//! A computer paired again with a code at the shack is held to no proof's age from then on: there a
//! fresh key stands for an enrolled computer, kept in the device cache as an enrolment keeps it.
use super::super::enrol as lan_enrol;
use super::*;
use crate::remote_service::tests::MemoryVault;
use crate::remote_service::vault::{AccountKeys, AccountVault, People, Revokes, Uninvites};
use std::sync::atomic::AtomicU64;
use tempo_client::account::{
    mailbox, CachedDevice, DeviceCache, Invite, KeySet, Owner, Role, Tenant, PROOF_MAX_AGE_S,
};

const CASES: &str =
    include_str!("../../../../../crates/tempo-client/tests/fixtures/account-proof-cases.json");
/// The service's id for the station, which its owner was proven under.
const SERVICE_STATION: &str = "20000000-0000-4000-8000-000000000001";

fn cases() -> Value {
    serde_json::from_str(CASES).unwrap()
}
fn fixture_now() -> u64 {
    cases()["now"].as_u64().unwrap()
}
fn issued_at() -> u64 {
    cases()["issuedAt"].as_u64().unwrap()
}
fn key(name: &str) -> String {
    cases()["keys"][name].as_str().unwrap().to_string()
}
fn token(name: &str) -> String {
    cases()["tokens"][name].as_str().unwrap().to_string()
}
fn spki(name: &str) -> Vec<u8> {
    tempo_stream::protocol::hex_bytes(&key(name)).unwrap()
}

/// What a station bound to the fixture's owner keeps, with `invites`.
fn ready(vault: &MemoryVault, invites: Vec<Invite>) -> lan_enrol::Ready {
    lan_enrol::Ready {
        tenant: Box::leak(Box::new(Tenant {
            origin: "http://127.0.0.1:9",
            issuer: "https://issuer.test.invalid/",
            native_client_id: "test-native-client",
            spa_client_id: "test-spa-client",
        })),
        owner: Owner {
            station_id: SERVICE_STATION.into(),
            issuer: "https://issuer.test.invalid/".into(),
            subject: cases()["owner"].as_str().unwrap().into(),
            proof_kid: "test-p".into(),
            proof_issued_at: issued_at(),
        },
        keys: KeySet::fetched(&cases()["jwks"], fixture_now()).unwrap(),
        invites,
        vault: Arc::new(vault.clone()),
        now_s: fixture_now,
    }
}
fn armed(vault: &MemoryVault, invites: Vec<Invite>) -> Arc<lan_enrol::Enrolment> {
    let enrolment = Arc::new(lan_enrol::Enrolment::default());
    enrolment.arm(Some(ready(vault, invites)));
    enrolment
}
/// A station with its LAN key and nobody paired: its book.
fn empty_book() -> Arc<Book> {
    book_on(&LanStore::holding(&fixture_key(), STATION, &[]))
}
/// The fixture's computer asking, its TLS session having proved `tls_key`.
fn computer(proof: &str, tls_key: &str) -> lan_enrol::Asked {
    lan_enrol::Asked::new("DESKTOP-7Q", "computer", &key("road"), proof, spki(tls_key)).unwrap()
}

/// ★ The owner's computer, its proof for exactly the key it proved: kept for the LAN (by that key's
/// pin, a proof's signing time, not yet heard from the service), then paired. The same proof
/// with another key proved: nothing.
#[test]
fn the_owners_computer_enrols_by_the_key_its_session_proved() {
    let vault = MemoryVault::default();
    let enrolment = armed(&vault, Vec::new());
    let book = empty_book();
    let pin = tls::pin(&spki("computerLan"));

    let other = computer(&token("computer"), "stationLan");
    assert_eq!(
        lan_enrol::admit(&book, &enrolment, STATION, &other, fixture_now()),
        Err("tokenNonce")
    );
    assert_eq!(book.ids(), Vec::<String>::new());
    assert_eq!(vault.device_cache().unwrap(), None);

    // Only a computer or a phone enrols on the LAN.
    assert!(lan_enrol::Asked::new(
        "Browser",
        "browser",
        &key("browser"),
        &token("browser"),
        spki("browser")
    )
    .is_none());
    let asked = computer(&token("computer"), "computerLan");
    let device = lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()).unwrap();
    assert_eq!(device, book::device_id(&pin));
    assert_eq!(book.paired(&pin), Some(device));
    let cache = vault.device_cache().unwrap().unwrap();
    assert_eq!(cache.station_id, SERVICE_STATION);
    assert_eq!(
        cache.devices,
        vec![CachedDevice {
            pin: tempo_client::hex(&pin),
            role: Role::Operate,
            guest: false,
            issued_at: issued_at(),
            confirmed_at: 0,
        }]
    );
}

/// The offline age, one rule from the proof's signing (operator rulings): taken up to the last
/// second of thirty days, refused from then on.
#[test]
fn a_proof_enrols_for_thirty_days_from_its_signing() {
    for (age, enrols) in [(PROOF_MAX_AGE_S - 1, true), (PROOF_MAX_AGE_S, false)] {
        let vault = MemoryVault::default();
        let book = empty_book();
        let asked = computer(&token("computer"), "computerLan");
        let result = lan_enrol::admit(
            &book,
            &armed(&vault, Vec::new()),
            STATION,
            &asked,
            issued_at() + age,
        );
        assert_eq!(result.is_ok(), enrols, "{age}: {result:?}");
        if !enrols {
            assert_eq!(result, Err("tokenIssuedAt"));
            assert_eq!(book.ids(), Vec::<String>::new());
        }
    }
}

/// Someone invited to operate enrols as a guest; invited only to listen, or not invited, they do
/// not; nor does anyone while the station has nothing to check against.
#[test]
fn only_an_operating_invitation_or_the_owner_enrols() {
    let invited = |role| {
        vec![Invite {
            mailbox: mailbox("K9.Guest@example.test"),
            role,
        }]
    };
    let vault = MemoryVault::default();
    let book = empty_book();
    let guest = computer(&token("guest"), "computerLan");
    lan_enrol::admit(
        &book,
        &armed(&vault, invited(Role::Operate)),
        STATION,
        &guest,
        fixture_now(),
    )
    .unwrap();
    let kept = vault.device_cache().unwrap().unwrap().devices;
    assert_eq!((kept[0].role, kept[0].guest), (Role::Operate, true));

    let unarmed = Arc::new(lan_enrol::Enrolment::default());
    for (enrolment, proof, why) in [
        (
            armed(&vault, invited(Role::Listen)),
            "guest",
            "listenNative",
        ),
        (armed(&vault, Vec::new()), "stranger", "proofNotYours"),
        (unarmed, "computer", "enrolClosed"),
    ] {
        let vault = MemoryVault::default();
        let book = empty_book();
        let asked = computer(&token(proof), "computerLan");
        assert_eq!(
            lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()),
            Err(why),
            "{proof}"
        );
        assert_eq!(book.ids(), Vec::<String>::new(), "{proof}");
        assert_eq!(vault.device_cache().unwrap(), None, "{proof}");
    }
}

/// The station's listener as `listener` starts it, taking unknown keys while `enrolment` is open
/// (or its pairing window is).
fn enrolling(s: &mut Shack, enrolment: Arc<lan_enrol::Enrolment>) {
    let desk = s.shared.desk.clone().unwrap();
    let identity = tls::Identity::new(&s.station_key, STATION.into()).unwrap();
    let (paired, pairing) = desk.book.verifier();
    let unknown = enrolment.or_pairing(pairing);
    s.shared.tls = tls::server(&identity, paired, unknown).unwrap();
    s.shared.desk =
        Some(super::pairing::Desk::new(desk.book.clone(), &identity, enrolment).unwrap());
}
fn enrol_message(proof: &str) -> Value {
    json!({"type":"enrol","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
        "name":"DESKTOP-7Q","kind":"computer","publicKey":key("road"),"proof":proof})
}

/// ★ Over the LAN: with enrolment open, an unknown key gets as far as an enrol-only connection and
/// no further. A valid proof made for another key is refused by name (the session checks the key
/// TLS proved, not one a message names), counts against the address, and pairs nothing; a hello
/// from that key is not welcomed. With enrolment closed (and no window), TLS refuses the key.
#[tokio::test(flavor = "multi_thread")]
async fn over_the_lan_an_unknown_key_reaches_only_the_enrolment() {
    let (_tx, stop) = watch::channel(false);
    let vault = MemoryVault::default();
    let mut s = shack(home());
    let stranger = Computer::new();
    let (task, refused) = dial(&s, &stranger.key, "192.168.1.31:50000", stop.clone()).await;
    assert!(refused.is_err(), "closed: refused in the handshake");
    task.await.unwrap();

    enrolling(&mut s, armed(&vault, Vec::new()));
    let (task, socket) = dial(&s, &stranger.key, "192.168.1.31:50000", stop.clone()).await;
    let mut socket = socket.unwrap();
    let answer = hello(&mut socket, VERSIONS).await;
    assert!(answer.get("closed").is_some(), "no welcome: {answer}");
    task.await.unwrap();

    for _ in 0..FAILURES_BEFORE_IGNORED {
        let (task, socket) = dial(&s, &stranger.key, "192.168.1.32:50000", stop.clone()).await;
        let mut socket = socket.unwrap();
        send(&mut socket, enrol_message(&token("computer"))).await;
        assert_eq!(
            next(&mut socket).await,
            json!({"type":"refused","reason":"tokenNonce"})
        );
        task.await.unwrap();
    }
    assert_eq!(
        s.shared.desk.as_ref().unwrap().book.ids(),
        vec![s.device.clone()]
    );
    assert_eq!(vault.device_cache().unwrap(), None);
    assert_eq!(
        s.shared
            .gate
            .admit("192.168.1.32".parse().unwrap(), Instant::now())
            .err(),
        Some(super::gate::Refused::Ignored),
        "each refused proof counted"
    );
}

/// The pairing window still works beside it, and an enrolment while the station has nothing to
/// check against is turned away by name, not counted.
#[tokio::test(flavor = "multi_thread")]
async fn pairing_stands_beside_enrolment() {
    let (_tx, stop) = watch::channel(false);
    let mut s = shack(home());
    enrolling(&mut s, Arc::new(lan_enrol::Enrolment::default()));
    let desk = s.shared.desk.clone().unwrap();
    desk.book.open(Instant::now(), 0).unwrap();
    let stranger = Computer::new();
    for _ in 0..FAILURES_BEFORE_IGNORED {
        let (task, socket) = dial(&s, &stranger.key, "192.168.1.33:50000", stop.clone()).await;
        let mut socket = socket.unwrap();
        send(&mut socket, enrol_message(&token("computer"))).await;
        assert_eq!(
            next(&mut socket).await,
            json!({"type":"refused","reason":"enrolClosed"})
        );
        task.await.unwrap();
    }
    assert!(s
        .shared
        .gate
        .admit("192.168.1.33".parse().unwrap(), Instant::now())
        .is_ok());
    // And the window's own ceremony starts as it did: the station's proof comes first.
    let (_task, socket) = dial(&s, &stranger.key, "192.168.1.34:50000", stop.clone()).await;
    let mut socket = socket.unwrap();
    send(
        &mut socket,
        json!({"type":"pair","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
            "name":"Laptop","nonce":"00".repeat(32)}),
    )
    .await;
    assert_eq!(next(&mut socket).await["type"], "pairProof");
}

/// ★ The computer's own enrolment (`crate::lan_client::account::enrol`) against the station's real
/// channel: its message is the one the station reads (parsed whole and exactly), and the station's
/// refusal of a proof made for another key comes back named as the window says it
/// (`proofRefused`), with nothing paired. CONTROL: with nothing to check a sign-in against, the
/// station refuses the unknown key in TLS (`stationNotSignedIn`); and a computer pinning another
/// station's key goes no further than TLS (`keyChanged`).
#[tokio::test(flavor = "multi_thread")]
async fn the_computers_enrolment_is_the_one_the_station_reads() {
    use crate::lan_client::account::enrol as computer_enrols;
    let (_tx, stop) = watch::channel(false);
    let vault = MemoryVault::default();
    let mut s = shack(home());
    let stranger = Computer::new();
    let asked = |at, station: String| {
        let pkcs8 = stranger.key.clone();
        async move {
            computer_enrols(
                at,
                &pkcs8,
                &station,
                "Den PC",
                &key("road"),
                &token("computer"),
            )
            .await
        }
    };
    let closed = super::computer::behind_a_port(&s, stop.clone()).await;
    assert_eq!(
        asked(closed, s.public_key.clone()).await,
        Err("stationNotSignedIn")
    );
    enrolling(&mut s, armed(&vault, Vec::new()));
    let open = super::computer::behind_a_port(&s, stop.clone()).await;
    assert_eq!(asked(open, s.public_key.clone()).await, Err("proofRefused"));
    assert_eq!(
        s.shared.desk.as_ref().unwrap().book.ids(),
        vec![s.device.clone()],
        "nothing paired"
    );
    assert_eq!(vault.device_cache().unwrap(), None);
    assert_eq!(
        asked(open, super::computer::another_key()).await,
        Err("keyChanged")
    );
}

// ---- A proof's age on the LAN --------------------------------------------------------------------

/// A device kept for the LAN by `pin`, its proof signed at `issued_at`.
fn kept(pin: &[u8; 32], guest: bool, issued_at: u64) -> CachedDevice {
    CachedDevice {
        pin: tempo_client::hex(pin),
        role: Role::Operate,
        guest,
        issued_at,
        confirmed_at: 0,
    }
}
/// A store holding the LAN's device cache with `devices`.
fn caching(devices: Vec<CachedDevice>) -> MemoryVault {
    let vault = MemoryVault::default();
    let mut cache = DeviceCache::new(SERVICE_STATION);
    for device in devices {
        cache.keep(device).unwrap();
    }
    vault.save_device_cache(&cache).unwrap();
    vault
}

/// ★ A computer enrolled on its own sign-in is admitted for thirty days from its proof's signing, the
/// owner's and a guest's alike, and not a second longer; a computer paired by code is not held to
/// it, and the book's own refusal stands. Until the cache has been read nobody is admitted, as the
/// book admits nobody until it has read its own.
#[test]
fn an_enrolled_computer_is_admitted_for_thirty_days_from_its_proof() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let signed = 1_000_000;
    let (owners, guests, by_code) = ([1; 32], [2; 32], [3; 32]);
    let vault = caching(vec![
        kept(&owners, false, signed),
        kept(&guests, true, signed),
    ]);
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(Arc::new(vault), clock));
    let paired = enrolment.bounded(Arc::new(|pin: &[u8; 32]| Some(book::device_id(pin))));
    let admitted = |pin: &[u8; 32]| paired(pin) == Some(book::device_id(pin));
    CLOCK.store(signed, Ordering::SeqCst);
    assert!(!admitted(&owners) && !admitted(&by_code), "not read yet");

    enrolment.load();
    for at in [signed, signed + PROOF_MAX_AGE_S - 1] {
        CLOCK.store(at, Ordering::SeqCst);
        assert!(
            admitted(&owners) && admitted(&guests) && admitted(&by_code),
            "{at}"
        );
        assert_eq!(enrolment.expired(), Vec::<[u8; 32]>::new());
    }
    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    assert!(!admitted(&owners), "the owner's, thirty days on");
    assert!(!admitted(&guests), "a guest's, thirty days on");
    assert!(admitted(&by_code), "paired by code: the book alone decides");
    assert_eq!(enrolment.expired(), vec![owners, guests]);
    // Signed in the future (past the clock's leeway): not yet.
    CLOCK.store(signed - 61, Ordering::SeqCst);
    assert!(!admitted(&owners));

    CLOCK.store(signed, Ordering::SeqCst);
    let refused = enrolment.bounded(Arc::new(|_: &[u8; 32]| None));
    assert_eq!(refused(&owners), None, "never more than the book admits");
    // With no account at all (a test's LAN), nothing is enrolled and nothing held to an age.
    assert!(lan_enrol::Enrolment::default().admits(&owners));
}

/// Enrolling again once the old proof has aged out replaces it: admitted from the new proof's
/// signing, at once, with no re-read of the store.
#[test]
fn enrolling_again_admits_from_the_new_proof() {
    let pin = tls::pin(&spki("computerLan"));
    let vault = caching(vec![kept(&pin, false, issued_at() - PROOF_MAX_AGE_S)]);
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        fixture_now,
    ));
    enrolment.arm(Some(ready(&vault, Vec::new())));
    enrolment.load();
    assert!(!enrolment.admits(&pin), "premise: aged out");
    let book = empty_book();
    let asked = computer(&token("computer"), "computerLan");
    lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()).unwrap();
    assert!(enrolment.admits(&pin));
    assert_eq!(
        vault.device_cache().unwrap().unwrap().devices,
        vec![kept(&pin, false, issued_at())]
    );
}

/// The LAN on the station `s` serves, its book `book` and its enrolment `enrolment`.
fn lan_enrolled(
    s: &Shack,
    scratch: &Scratch,
    network: Network,
    book: Arc<Book>,
    enrolment: Arc<lan_enrol::Enrolment>,
) -> Lan {
    Lan::start(
        scratch.path(),
        Deps {
            authority: s.shared.authority.clone(),
            engine: s.shared.engine.clone(),
            feeds: s.shared.feeds.clone(),
            book,
            enrolment,
            resolve: Arc::new(move |_| only(Ok(network))),
            advertise: advertise(),
            firewall: firewall_says(),
            dispatcher: s.shared.dispatcher.clone(),
            commands: s.shared.commands.clone(),
        },
    )
}

/// ★ At launch, before anything listens, an enrolled computer past its proof's age leaves the book
/// and the LAN's device cache (it signs in again to come back). One within its age stays, and so
/// does a computer paired by code.
#[tokio::test]
async fn at_launch_a_computer_past_its_proofs_age_leaves_the_lan() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let signed = 1_000_000;
    let (aged, fresh, by_code) = ([1; 32], [2; 32], [3; 32]);
    let vault = caching(vec![
        kept(&aged, false, signed),
        kept(&fresh, true, signed + 1),
    ]);
    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    let s = shack(home());
    let store = LanStore::holding(
        &s.station_key,
        STATION,
        &[(&aged, "Aged"), (&fresh, "Fresh"), (&by_code, "Den PC")],
    );
    let scratch = Scratch::new();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        clock,
    ));
    let lan = lan_enrolled(&s, &scratch, home(), book_on(&store), enrolment);
    let names = |status: &LanStatus| {
        let mut names: Vec<String> = status.devices.iter().map(|d| d.name.clone()).collect();
        names.sort();
        names
    };
    let status = eventually(&lan, "the aged one stayed", |st| names(st).len() == 2).await;
    assert_eq!(names(&status), vec!["Den PC", "Fresh"]);
    assert_eq!(
        vault.device_cache().unwrap().unwrap().devices,
        vec![kept(&fresh, true, signed + 1)]
    );
}

/// ★ Over the LAN's own listener: an enrolled computer is welcomed within its proof's age; once that
/// passes, its session ends at its next tick and its next connection is refused. CONTROL: a computer
/// paired by code on the same listener is welcomed after.
#[tokio::test(flavor = "multi_thread")]
async fn the_lans_listener_holds_an_enrolled_computer_to_its_proofs_age() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = std::net::TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let network = Network::new(address, 32).unwrap();
    let signed = 1_000_000;
    let (station_key, enrolled, by_code) = (fixture_key(), Computer::new(), Computer::new());
    let store = LanStore::holding(
        &station_key,
        STATION,
        &[(&enrolled.pin, "Enrolled"), (&by_code.pin, "Den PC")],
    );
    let s = shack_with(network, station_key, enrolled, store);
    let vault = caching(vec![kept(&s.computer.pin, false, signed)]);
    CLOCK.store(signed + PROOF_MAX_AGE_S - 1, Ordering::SeqCst);
    let scratch = Scratch::new();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(Arc::new(vault), clock));
    let lan = lan_enrolled(&s, &scratch, network, book_on(&s.store), enrolment);
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    eventually(&lan, "never listened", |st| {
        st.listening.as_deref() == Some(at.to_string().as_str())
    })
    .await;
    let dial_at = |key: &str| {
        let (key, station) = (key.to_string(), s.public_key.clone());
        async move {
            let stream = tokio::net::TcpStream::connect(at).await.unwrap();
            open(stream, &key, &station).await
        }
    };
    let mut socket = dial_at(&s.computer.key).await.unwrap();
    assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");

    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    let ended = loop {
        let message = next(&mut socket).await;
        assert!(
            message["silent"].is_null(),
            "the session outlived the proof's age"
        );
        if message["closed"].is_string() {
            break message;
        }
    };
    assert!(ended["closed"].is_string());
    let control = hello(&mut dial_at(&by_code.key).await.unwrap(), VERSIONS).await;
    assert_eq!(control["type"], "welcome", "{control}");
    let again = match dial_at(&s.computer.key).await {
        Ok(mut socket) => hello(&mut socket, VERSIONS).await,
        Err(refused) => json!({ "refused": refused }),
    };
    assert_ne!(again["type"], "welcome", "{again}");
}

// ---- Paired again with a code at the shack -------------------------------------------------------

/// ★ A computer enrolled on its own sign-in and then paired again with a code at the shack, within
/// its proof's thirty days, is held to that age no longer: on its proof's thirtieth day the
/// listener still welcomes it, a list (`Lan::expire`) leaves it in the book, and the station
/// started again that day reads it as admitted, as every computer paired by code is, until it is
/// removed at the shack. CONTROL: a computer enrolled at the same time and never paired by code
/// leaves that day, as before. Skipped, saying so, on a box with no private address of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_paired_again_with_a_code_outlasts_its_proofs_age() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = std::net::TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let network = Network::new(address, 32).unwrap();
    let signed = 1_000_000;
    let (station_key, again, other) = (fixture_key(), Computer::new(), Computer::new());
    let store = LanStore::holding(
        &station_key,
        STATION,
        &[(&again.pin, "DESKTOP-7Q"), (&other.pin, "Other PC")],
    );
    let s = shack_with(network, station_key, again.clone(), store);
    let vault = caching(vec![
        kept(&again.pin, false, signed),
        kept(&other.pin, false, signed),
    ]);
    CLOCK.store(signed + 86_400, Ordering::SeqCst);
    let scratch = Scratch::new();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        clock,
    ));
    let lan = lan_enrolled(&s, &scratch, network, book_on(&s.store), enrolment.clone());
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    eventually(&lan, "never listened", |st| {
        st.listening.as_deref() == Some(at.to_string().as_str())
    })
    .await;

    // Paired again with the code the shack shows, by the key it enrolled with.
    lan.pair().unwrap();
    let shown = lan.status().pairing.unwrap().code;
    let code: [u8; 8] = tempo_stream::protocol::hex_bytes(&shown)
        .unwrap()
        .try_into()
        .unwrap();
    let stream = tokio::net::TcpStream::connect(at).await.unwrap();
    let mut pairing = super::ceremony::open_pairing(stream, &again.key)
        .await
        .unwrap();
    assert_eq!(
        super::ceremony::prove_code(&mut pairing, &again, &code, "DESKTOP-7Q").await,
        json!({"type":"paired","deviceId":again.device,"stationId":STATION})
    );

    // Its proof's thirtieth day, and the station's list.
    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    lan.expire();
    let names: Vec<String> = lan.status().devices.into_iter().map(|d| d.name).collect();
    assert_eq!(
        names,
        vec!["DESKTOP-7Q"],
        "after a list on its proof's thirtieth day"
    );
    let dial_at = |key: &str| {
        let (key, station, lan) = (key.to_string(), s.public_key.clone(), &lan);
        async move {
            // Nothing else open from here first: a refusal is then the handshake's, not the gate's.
            let deadline = Instant::now() + Duration::from_secs(5);
            while lan.open_from(address.into()) > 0 {
                assert!(
                    Instant::now() < deadline,
                    "a connection from here stayed open"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let stream = tokio::net::TcpStream::connect(at).await.unwrap();
            match open(stream, &key, &station).await {
                Ok(mut socket) => hello(&mut socket, VERSIONS).await,
                Err(refused) => json!({ "refused": refused }),
            }
        }
    };
    let welcome = dial_at(&again.key).await;
    assert_eq!(
        welcome["type"], "welcome",
        "paired again by code: {welcome}"
    );
    assert_eq!(welcome["deviceId"], again.device);
    let control = dial_at(&other.key).await;
    assert_ne!(
        control["type"], "welcome",
        "never paired by code: {control}"
    );
    assert_eq!(vault.device_cache().unwrap().unwrap().devices, Vec::new());
    // The station started again that day reads the same from the store.
    let restarted = lan_enrol::Enrolment::kept_in(Arc::new(vault.clone()), clock);
    restarted.load();
    assert!(restarted.admits(&again.pin), "at launch");
    assert_eq!(restarted.expired(), Vec::<[u8; 32]>::new());

    // Removed at the shack: out at once.
    lan.revoke(&again.device).unwrap();
    let removed = dial_at(&again.key).await;
    assert_ne!(
        removed["type"], "welcome",
        "removed at the shack: {removed}"
    );
}

/// The station's account entries in `inner`, its device cache refused as a credential store that
/// will not answer refuses it: the read while `reads` is set, the write while `saves` is.
#[derive(Clone, Default)]
struct Refusing {
    inner: MemoryVault,
    reads: Arc<AtomicBool>,
    saves: Arc<AtomicBool>,
}
impl AccountVault for Refusing {
    fn owner(&self) -> Result<Option<Owner>, &'static str> {
        self.inner.owner()
    }
    fn save_owner(&self, owner: &Owner) -> Result<(), &'static str> {
        self.inner.save_owner(owner)
    }
    fn account_keys(&self) -> Result<Option<AccountKeys>, &'static str> {
        self.inner.account_keys()
    }
    fn save_account_keys(&self, keys: &AccountKeys) -> Result<(), &'static str> {
        self.inner.save_account_keys(keys)
    }
    fn device_cache(&self) -> Result<Option<DeviceCache>, &'static str> {
        if self.reads.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        self.inner.device_cache()
    }
    fn save_device_cache(&self, cache: &DeviceCache) -> Result<(), &'static str> {
        if self.saves.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        self.inner.save_device_cache(cache)
    }
    fn people(&self) -> Result<Option<People>, &'static str> {
        self.inner.people()
    }
    fn save_people(&self, people: &People) -> Result<(), &'static str> {
        self.inner.save_people(people)
    }
    fn revokes(&self) -> Result<Option<Revokes>, &'static str> {
        self.inner.revokes()
    }
    fn save_revokes(&self, revokes: &Revokes) -> Result<(), &'static str> {
        self.inner.save_revokes(revokes)
    }
    fn uninvites(&self) -> Result<Option<Uninvites>, &'static str> {
        self.inner.uninvites()
    }
    fn save_uninvites(&self, uninvites: &Uninvites) -> Result<(), &'static str> {
        self.inner.save_uninvites(uninvites)
    }
}

/// `computer` paired again at the shack `s`, a window opened for it with the code then shown: the
/// shack's last word.
async fn paired_again(
    s: &Shack,
    computer: &Computer,
    peer: &str,
    stop: watch::Receiver<bool>,
) -> Value {
    let book = s.shared.desk.as_ref().unwrap().book.clone();
    book.open(Instant::now(), 0).unwrap();
    let code = super::ceremony::shown_code(&book);
    let (_task, pairing) = super::ceremony::dial_pairing(s, &computer.key, peer, stop).await;
    super::ceremony::prove_code(&mut pairing.unwrap(), computer, &code, "Shack laptop").await
}

/// ★ Fails closed: a code pairing whose computer the LAN's device cache will not let go of, because
/// the store will not keep the change or will not be read, is refused as `unavailable`, and the
/// computer stays held to its proof's age, out on its thirtieth day as before. So does one whose
/// pairing the book will not keep, with the cache untouched: it changes only once the book keeps the
/// pairing. CONTROL: once the stores answer, the same computer is paired and held to no age.
#[tokio::test(flavor = "multi_thread")]
async fn a_code_pairing_the_cache_will_not_take_leaves_the_computer_held() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let signed = 1_000_000;
    let (_tx, stop) = watch::channel(false);
    let mut s = shack(home());
    let computer = s.computer.clone();
    let vault = Refusing::default();
    let mut cache = DeviceCache::new(SERVICE_STATION);
    cache.keep(kept(&computer.pin, false, signed)).unwrap();
    vault.inner.save_device_cache(&cache).unwrap();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        clock,
    ));
    CLOCK.store(signed + 86_400, Ordering::SeqCst);
    enrolment.load();
    enrolling(&mut s, enrolment.clone());
    let held = |when: &str| {
        CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
        assert!(
            !enrolment.admits(&computer.pin),
            "{when}: admitted on day 30"
        );
        assert_eq!(enrolment.expired(), vec![computer.pin], "{when}");
        CLOCK.store(signed + 86_400, Ordering::SeqCst);
    };
    let refused = json!({"type":"refused","reason":"unavailable"});

    vault.saves.store(true, Ordering::SeqCst);
    let answer = paired_again(&s, &computer, "192.168.1.40:50000", stop.clone()).await;
    assert_eq!(answer, refused, "the cache would not keep the change");
    vault.saves.store(false, Ordering::SeqCst);
    held("the cache would not keep the change");

    vault.reads.store(true, Ordering::SeqCst);
    let answer = paired_again(&s, &computer, "192.168.1.41:50000", stop.clone()).await;
    assert_eq!(answer, refused, "the cache would not be read");
    vault.reads.store(false, Ordering::SeqCst);
    held("the cache would not be read");

    s.store.locked.store(true, Ordering::SeqCst);
    let answer = paired_again(&s, &computer, "192.168.1.42:50000", stop.clone()).await;
    assert_eq!(answer, refused, "the book would not keep the pairing");
    s.store.locked.store(false, Ordering::SeqCst);
    held("the book would not keep the pairing");
    assert_eq!(
        vault.device_cache().unwrap().unwrap().devices,
        vec![kept(&computer.pin, false, signed)],
        "the cache untouched"
    );

    let answer = paired_again(&s, &computer, "192.168.1.43:50000", stop).await;
    assert_eq!(
        answer,
        json!({"type":"paired","deviceId":computer.device,"stationId":STATION})
    );
    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    assert!(enrolment.admits(&computer.pin), "paired by code: no age");
    assert_eq!(enrolment.expired(), Vec::<[u8; 32]>::new());
}
