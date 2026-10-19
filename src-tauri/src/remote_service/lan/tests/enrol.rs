//! A computer enrolling on this network by its own sign-in, with no code and no press: its proof is
//! checked at the shack from what the station keeps (internet down), for the key its TLS session
//! proved, and only then is it paired.
//!
//! The proofs are tempo-client's account fixture, minted once for public keys whose private halves
//! were never kept. So the admission itself is driven directly ([`lan_enrol::admit`], what the session
//! runs), and over TLS a fresh key shows that a valid proof for another key is refused.
use super::super::enrol as lan_enrol;
use super::*;
use crate::remote_service::tests::MemoryVault;
use crate::remote_service::vault::AccountVault;
use tempo_client::account::{
    mailbox, CachedDevice, Invite, KeySet, Owner, Role, Tenant, PROOF_MAX_AGE_S,
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
