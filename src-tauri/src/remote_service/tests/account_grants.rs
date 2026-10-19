//! The station's account, wired into the shack: signing in there binds the station with no code, and
//! a device the service approved on its own proof is granted only once the station has checked that
//! proof itself, then only as its role says.
//!
//! The proofs are tempo-client's account fixture (tokens minted once by throwaway keys that never
//! left a scratch process), read at the fixture's own time. The service is the fake cloud on
//! loopback; the issuer is never asked, because the station already holds its keys, fetched at
//! that same time.
use super::*;
use crate::remote_service::vault::{AccountKeys, AccountVault, People};
use tempo_client::account::{mailbox, CachedDevice, Invite, KeySet, Owner, Role, Tenant};

const CASES: &str =
    include_str!("../../../../crates/tempo-client/tests/fixtures/account-proof-cases.json");
const COMPUTER_ID: &str = "10000000-0000-4000-8000-00000000000d";

fn cases() -> serde_json::Value {
    serde_json::from_str(CASES).unwrap()
}
/// The fixture's own time: its proofs were signed 100 s before it.
fn fixture_now() -> u64 {
    1_800_000_000
}
fn key(name: &str) -> String {
    cases()["keys"][name].as_str().unwrap().to_string()
}
fn token(name: &str) -> String {
    cases()["tokens"][name].as_str().unwrap().to_string()
}
fn sha256(hex: &str) -> String {
    let spki = tempo_stream::protocol::hex_bytes(hex).unwrap();
    tempo_client::hex(ring::digest::digest(&ring::digest::SHA256, &spki).as_ref())
}

/// The fixture's tenant, its issuer never asked (the station holds its keys).
fn test_service() -> Arc<account::Service> {
    let tenant: &'static Tenant = Box::leak(Box::new(Tenant {
        origin: "http://127.0.0.1:9",
        issuer: "https://issuer.test.invalid/",
        native_client_id: "test-native-client",
        spa_client_id: "test-spa-client",
    }));
    Arc::new(account::Service::with("http://127.0.0.1:9", tenant, "http://127.0.0.1:9/").unwrap())
}

/// What the stand-in binder was given, call by call: name, station key, LAN key, credential.
type Asked = Arc<Mutex<Vec<(String, String, String, String)>>>;

/// The account on `vault`, binding through `bind`, naming the fixture's LAN key, at the fixture's
/// time.
fn accounts(vault: &MemoryVault, bind: account::Binder) -> Accounts {
    Accounts {
        vault: Arc::new(vault.clone()),
        service: Ok(test_service()),
        bind,
        lan_key: Some(Arc::new(|| Some(key("stationLan")))),
        now_s: fixture_now,
    }
}
fn launch_with(cloud: &FakeCloud, vault: &MemoryVault, accounts: Accounts) -> Service {
    Service::with_accounts(
        cloud.origin.clone(),
        Box::new(vault.clone()),
        engine(),
        crate::remote_monitor::Publisher::default(),
        accounts,
    )
}
/// A binder that answers each sign-in in turn from `answers`, recording what it was given; one
/// past the last never answers, as a sign-in left open in the browser.
fn binder(answers: Vec<Result<account::Bound, &'static str>>) -> (account::Binder, Asked) {
    let asked: Asked = Arc::default();
    let held = asked.clone();
    let answers = Arc::new(Mutex::new(answers.into_iter()));
    let bind: account::Binder = Arc::new(move |binds: account::Binds| {
        held.lock().unwrap().push((
            binds.name,
            binds.station_key,
            binds.lan_key,
            binds.credential,
        ));
        let answer = answers.lock().unwrap().next();
        Box::pin(async move {
            match answer {
                Some(answer) => answer,
                None => std::future::pending().await,
            }
        })
    });
    (bind, asked)
}
fn bound() -> account::Bound {
    account::Bound {
        station_id: STATION.into(),
        account_id: ACCOUNT.into(),
    }
}

// ---- Signing in at the shack ---------------------------------------------------------------------

/// ★ Sign in to Nexus Remote: the station's keys and credential are made and staged before the
/// browser opens, a sign-in tried again names the very same ones (so the service finds the station
/// it already made for them), and once bound the credential and the signing key are kept under the
/// binding the service gave, and Remote turns on as approving a pairing turns it on.
#[tokio::test(flavor = "multi_thread")]
async fn signing_in_binds_the_station_with_the_keys_it_staged_and_turns_remote_on() {
    let cloud = fake_cloud().await;
    let vault = MemoryVault::default();
    let (bind, asked) = binder(vec![Err("signInFailed"), Ok(bound())]);
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));

    service
        .action(Action::SignIn {
            name: " Shack ".into(),
        })
        .await
        .unwrap();
    let failed = eventually(&service, "the first sign-in ended", |s| {
        !s.account.signing_in && s.error.is_some()
    })
    .await;
    assert_eq!(failed.error, Some("signInFailed"));
    assert_eq!(
        failed.station_id, None,
        "nothing is bound on a failed sign-in"
    );
    assert!(vault.binding().unwrap().is_none());

    service
        .action(Action::SignIn {
            name: "Shack".into(),
        })
        .await
        .unwrap();
    let status = eventually(&service, "bound and on", |s| {
        s.station_id.as_deref() == Some(STATION) && on(s)
    })
    .await;
    assert!(!status.account.signing_in);

    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    let (name, station_key, lan_key, credential) = asked[0].clone();
    assert_eq!(name, "Shack", "the name, trimmed");
    assert_eq!(lan_key, key("stationLan"), "the LAN's own key");
    assert!(
        tempo_stream::protocol::device_key(&station_key),
        "a P-256 SPKI"
    );
    assert!(credential.len() == 64 && credential.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(
        asked[1], asked[0],
        "the second sign-in names the same keys and credential"
    );
    // Nothing the screen is shown holds a secret: no credential, no signing key.
    let kept_key = vault.station_key().unwrap().unwrap().pkcs8;
    let shown = serde_json::to_string(&service.status().unwrap()).unwrap();
    assert!(!shown.contains(&credential) && !shown.contains(&kept_key));

    let binding = Binding {
        origin: cloud.origin.clone(),
        station_id: STATION.into(),
        account_id: ACCOUNT.into(),
    };
    assert!(vault.binding().unwrap() == Some(binding.clone()));
    assert_eq!(vault.credential(&binding).unwrap(), credential);
    let kept = vault.station_key().unwrap().unwrap();
    assert!(
        kept.binding == binding,
        "the signing key is kept under the binding"
    );
    assert_eq!(
        station_key::Signer::restore(&kept.pkcs8)
            .unwrap()
            .public_key(),
        station_key,
        "the key kept is the one the sign-in named"
    );
    assert_eq!(
        status.station_key,
        fingerprint(&station_key),
        "and the station signs with it"
    );
}

/// Signing in is refused while the station is bound, and while this build has no sign-in for its
/// service: then no key is made and nothing is staged.
#[tokio::test(flavor = "multi_thread")]
async fn signing_in_is_refused_when_bound_or_not_ready() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let (bind, asked) = binder(vec![Ok(bound())]);
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let sign_in = || Action::SignIn {
        name: "Shack".into(),
    };
    assert!(matches!(
        service.action(sign_in()).await,
        Err("invalidRequest")
    ));
    assert!(asked.lock().unwrap().is_empty());

    let vault = MemoryVault::default();
    let (bind, asked) = binder(vec![Ok(bound())]);
    let service = launch_with(
        &cloud,
        &vault,
        Accounts {
            service: Err("accountNotReady"),
            ..accounts(&vault, bind)
        },
    );
    assert!(matches!(
        service.action(sign_in()).await,
        Err("accountNotReady")
    ));
    assert!(asked.lock().unwrap().is_empty());
    assert!(
        vault.values.lock().unwrap().is_empty(),
        "nothing staged: {:?}",
        vault.values.lock().unwrap().keys().collect::<Vec<_>>()
    );
}

/// Cancel while the browser is still open ends the sign-in: the station stays unbound, and a
/// pairing code can be asked for again.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_ends_a_sign_in_left_open() {
    let cloud = fake_cloud().await;
    let vault = MemoryVault::default();
    let (bind, _) = binder(Vec::new());
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));
    let signing = service
        .action(Action::SignIn {
            name: "Shack".into(),
        })
        .await
        .unwrap();
    assert!(signing.account.signing_in);
    let begin = || Action::Begin {
        name: "Shack".into(),
    };
    assert!(matches!(
        service.action(begin()).await,
        Err("invalidRequest")
    ));
    let cancelled = service.action(Action::Cancel {}).await.unwrap();
    assert!(!cancelled.account.signing_in);
    assert_eq!(cancelled.phase, "unpaired");
    service.action(begin()).await.unwrap();
}

// ---- A device approved on its own proof ----------------------------------------------------------

/// The station bound to the fixture's owner, holding the issuer's keys as fetched at the fixture's
/// time, with `invites` invited.
fn owned_vault(origin: &str, invites: Vec<Invite>) -> MemoryVault {
    let vault = paired_vault(origin);
    vault
        .save_owner(&Owner {
            station_id: STATION.into(),
            issuer: "https://issuer.test.invalid/".into(),
            subject: cases()["owner"].as_str().unwrap().into(),
            proof_kid: "test-p".into(),
            proof_issued_at: cases()["issuedAt"].as_u64().unwrap(),
        })
        .unwrap();
    vault
        .save_account_keys(&AccountKeys {
            issuer: "https://issuer.test.invalid/".into(),
            keys: KeySet::fetched(&cases()["jwks"], fixture_now()).unwrap(),
        })
        .unwrap();
    vault
        .save_account_entry(
            "account-people",
            &People {
                station_id: STATION.into(),
                invites,
            },
        )
        .unwrap();
    vault
}
fn browser_row(proof: &str) -> serde_json::Value {
    json!({"id": BROWSER, "name": "Owner's browser", "approved": 1, "expiresAt": APPROVED_UNTIL,
        "generation": 2, "renewsUntil": null, "publicKey": key("browser"), "kind": "browser",
        "platform": null, "untilRemoved": 1, "role": "operate", "addedBy": "account",
        "proof": proof, "lanKey": null})
}
fn computer_row(proof: &str, role: &str) -> serde_json::Value {
    json!({"id": COMPUTER_ID, "name": "DESKTOP-7Q", "approved": 1, "expiresAt": APPROVED_UNTIL,
        "generation": 3, "renewsUntil": null, "publicKey": key("road"), "kind": "computer",
        "platform": "windows", "untilRemoved": 1, "role": role, "addedBy": "account",
        "proof": proof, "lanKey": key("computerLan")})
}
/// Remote on, the service listing `rows`, and the station's list read.
async fn listed(service: &Service, cloud: &FakeCloud, rows: Vec<serde_json::Value>) -> Status {
    *cloud.devices.lock().unwrap() = json!({ "devices": rows }).to_string();
    eventually(service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    service.action(Action::Refresh {}).await.unwrap()
}
fn launch_owned(cloud: &FakeCloud, vault: &MemoryVault) -> Service {
    let (bind, _) = binder(Vec::new());
    launch_with(cloud, vault, accounts(vault, bind))
}

/// ★ A browser signed in as the station's owner: the service approved it, and the station, having
/// checked its proof itself, grants it what an Approve press grants (station control and logging,
/// never FT8/FT4 transmit) and pins its key, with no press; the notice names it, and the LAN keeps
/// it under its key's pin.
#[tokio::test(flavor = "multi_thread")]
async fn a_browser_signed_in_as_the_owner_is_granted_and_pinned_with_no_press() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(status.account.unconfirmed, BTreeMap::new());
    assert_eq!(status.station_permissions, vec![BROWSER.to_string()]);
    // The proof was listed to the station, and the screen is never shown it.
    let shown = serde_json::to_string(&status).unwrap();
    assert!(!shown.contains(&token("browser")[..40]), "{shown}");
    assert!(
        shown.contains(r#""addedBy":"account""#),
        "CONTROL: the row itself is shown"
    );
    assert_eq!(status.logging_permissions, vec![BROWSER.to_string()]);
    assert_eq!(status.transmit_permissions, Vec::<String>::new());
    assert_eq!(status.pinned_devices, vec![BROWSER.to_string()]);
    // What a restart restores is the same: never transmit.
    let mut remembered = None;
    for _ in 0..100 {
        remembered = vault.state().unwrap().map(|state| state.grants);
        if remembered.as_ref().is_some_and(|g| !g.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let remembered: Vec<_> = remembered
        .unwrap()
        .iter()
        .map(|g| (g.device_id.clone(), g.logging, g.control, g.transmit))
        .collect();
    assert_eq!(remembered, vec![(BROWSER.to_string(), true, true, false)]);
    let added: Vec<_> = status
        .account
        .added
        .iter()
        .map(|a| (a.device_id.as_str(), a.role, a.guest, a.key.clone()))
        .collect();
    assert_eq!(
        added,
        vec![(BROWSER, Role::Operate, false, Some(sha256(&key("browser"))))]
    );
    let cache = vault.device_cache().unwrap().unwrap();
    assert_eq!(cache.station_id, STATION);
    assert_eq!(
        cache.devices,
        vec![CachedDevice {
            pin: sha256(&key("browser")),
            role: Role::Operate,
            guest: false,
            issued_at: cases()["issuedAt"].as_u64().unwrap(),
            confirmed_at: fixture_now(),
        }]
    );
    // The notice goes when the operator has seen it; the grants stay.
    let dismissed = service
        .action(Action::DismissAdded {
            device_id: BROWSER.into(),
        })
        .await
        .unwrap();
    assert!(dismissed.account.added.is_empty());
    assert_eq!(dismissed.station_permissions, vec![BROWSER.to_string()]);
}

/// ★ A proof the station cannot confirm grants nothing, whatever the service says: no permission,
/// no pin, nothing kept for the LAN, and the device is shown with why.
#[tokio::test(flavor = "multi_thread")]
async fn a_proof_the_station_cannot_confirm_grants_nothing() {
    let mut other_keys = computer_row(&token("computer"), "operate");
    other_keys["lanKey"] = key("stationLan").into();
    for (row, id, why) in [
        // A browser's keys, signed in to the wrong application.
        (
            browser_row(&token("browserNativeAud")),
            BROWSER,
            "tokenAudience",
        ),
        // Signed with a key the issuer never had.
        (
            computer_row(&token("forged"), "operate"),
            COMPUTER_ID,
            "tokenSignature",
        ),
        // The owner's sign-in, for another LAN key than the one listed.
        (other_keys, COMPUTER_ID, "tokenNonce"),
    ] {
        let proof = why;
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let service = launch_owned(&cloud, &vault);
        let status = listed(&service, &cloud, vec![row]).await;
        assert_eq!(
            status.account.unconfirmed,
            BTreeMap::from([(id.to_string(), why)]),
            "{proof}"
        );
        assert!(status.station_permissions.is_empty(), "{proof}");
        assert!(status.logging_permissions.is_empty(), "{proof}");
        assert!(status.pinned_devices.is_empty(), "{proof}");
        assert!(status.account.added.is_empty(), "{proof}");
        assert_eq!(vault.device_cache().unwrap(), None, "{proof}");
    }
}

/// A computer signed in by someone the station invited to operate: pinned (its own pin, which the
/// room's tunnel holds station control by), granted nothing on the relay's lanes, and kept for the
/// LAN under its LAN key as a guest's. The service's role is not the station's to take: the
/// invitation decides. Invited to listen, a computer is refused, since its pin alone would hold
/// station control; and someone not invited gets nothing.
#[tokio::test(flavor = "multi_thread")]
async fn an_invited_guests_computer_is_pinned_by_the_invitations_role() {
    let invited = |role| {
        vec![Invite {
            mailbox: mailbox("K9.Guest@example.test"),
            role,
        }]
    };
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, invited(Role::Operate));
    let service = launch_owned(&cloud, &vault);
    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("guest"), "listen")],
    )
    .await;
    assert_eq!(status.account.unconfirmed, BTreeMap::new());
    assert_eq!(status.pinned_devices, vec![COMPUTER_ID.to_string()]);
    assert!(status.logging_permissions.is_empty());
    let added: Vec<_> = status
        .account
        .added
        .iter()
        .map(|a| (a.device_id.as_str(), a.role, a.guest))
        .collect();
    assert_eq!(added, vec![(COMPUTER_ID, Role::Operate, true)]);
    let cache = vault.device_cache().unwrap().unwrap();
    assert_eq!(
        cache
            .devices
            .iter()
            .map(|d| (d.pin.clone(), d.role, d.guest))
            .collect::<Vec<_>>(),
        vec![(sha256(&key("computerLan")), Role::Operate, true)]
    );

    for (invites, proof, why) in [
        (invited(Role::Listen), "guest", "listenNative"),
        (Vec::new(), "stranger", "proofNotYours"),
    ] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, invites);
        let service = launch_owned(&cloud, &vault);
        let status = listed(
            &service,
            &cloud,
            vec![computer_row(&token(proof), "operate")],
        )
        .await;
        assert_eq!(
            status.account.unconfirmed,
            BTreeMap::from([(COMPUTER_ID.to_string(), why)]),
            "{proof}"
        );
        assert!(status.pinned_devices.is_empty(), "{proof}");
        assert!(status.station_permissions.is_empty(), "{proof}");
        assert_eq!(vault.device_cache().unwrap(), None, "{proof}");
    }
}

/// A station whose owner is not known here (paired by code, or bound as another station) vouches
/// for nobody: the owner's own browser waits for a press, as it does today.
#[tokio::test(flavor = "multi_thread")]
async fn a_station_with_no_owner_of_its_own_vouches_for_nobody() {
    for other in [false, true] {
        let cloud = fake_cloud().await;
        let vault = if other {
            let vault = owned_vault(&cloud.origin, Vec::new());
            let mut owner = vault.owner().unwrap().unwrap();
            owner.station_id = "20000000-0000-4000-8000-0000000000ff".into();
            vault.save_owner(&owner).unwrap();
            vault
        } else {
            paired_vault(&cloud.origin)
        };
        let service = launch_owned(&cloud, &vault);
        let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
        assert_eq!(
            status.account.unconfirmed,
            BTreeMap::from([(BROWSER.to_string(), "stationNotBound")]),
            "another station's owner: {other}"
        );
        assert!(status.station_permissions.is_empty());
        assert!(status.pinned_devices.is_empty());
    }
}

/// Once granted, a device is the operator's: station control taken away at the shack stays away
/// through the next list, which does not grant it again.
#[tokio::test(flavor = "multi_thread")]
async fn the_operators_own_decision_outlasts_the_next_list() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(status.station_permissions, vec![BROWSER.to_string()]);
    service
        .action(Action::StationPermission {
            device_id: BROWSER.into(),
            allow: false,
        })
        .await
        .unwrap();
    let again = service.action(Action::Refresh {}).await.unwrap();
    assert!(
        again.station_permissions.is_empty(),
        "{again:?}",
        again = again.station_permissions
    );
    assert_eq!(again.pinned_devices, vec![BROWSER.to_string()]);
}

// ---- The LAN's side ------------------------------------------------------------------------------

/// The LAN takes enrolments while the station is bound to an owner of its own, with the issuer's keys
/// kept: from the launch on, and no longer once the station is removed.
#[tokio::test(flavor = "multi_thread")]
async fn the_lan_takes_enrolments_only_while_the_station_has_its_owner() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let open = || service.control.lock().unwrap().enrolment.open();
    assert!(!open(), "no owner kept: closed");

    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let open = || service.control.lock().unwrap().enrolment.open();
    assert!(open(), "bound to its owner: open");
    service.action(Action::Forget {}).await.unwrap();
    assert!(!open(), "removed: closed");

    let vault = owned_vault(&cloud.origin, Vec::new());
    let mut owner = vault.owner().unwrap().unwrap();
    owner.station_id = "20000000-0000-4000-8000-0000000000ff".into();
    vault.save_owner(&owner).unwrap();
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let open = || service.control.lock().unwrap().enrolment.open();
    assert!(!open(), "another station's owner: closed");
}

/// ★ A computer kept for the LAN on its proof stays while the service lists it as approved, and
/// leaves the LAN's book and its keeping at the first list that no longer does. A computer paired
/// by code, which the station keeps on no proof, is left alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_the_service_stops_listing_leaves_the_lan() {
    use crate::remote_service::lan::tests::{LanStore, Scratch};
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let enrolled: [u8; 32] = tempo_stream::protocol::hex_bytes(&sha256(&key("computerLan")))
        .unwrap()
        .try_into()
        .unwrap();
    let paired: [u8; 32] = [7; 32];
    let (_, station) = station_key::Signer::generate().unwrap();
    let store = LanStore::holding(
        &station,
        "60000000-0000-4000-8000-0000000000aa",
        &[(&paired, "Den PC"), (&enrolled, "DESKTOP-7Q")],
    );
    let mut cache = tempo_client::account::DeviceCache::new(STATION);
    cache
        .keep(CachedDevice {
            pin: sha256(&key("computerLan")),
            role: Role::Operate,
            guest: false,
            issued_at: cases()["issuedAt"].as_u64().unwrap(),
            confirmed_at: 0,
        })
        .unwrap();
    vault.save_device_cache(&cache).unwrap();
    let scratch = Scratch::new();
    let engine = engine();
    let service =
        super::lan_grants::with_lan(launch_owned(&cloud, &vault), &engine, &store, &scratch);
    let names = |status: &Status| -> Vec<String> {
        let mut names: Vec<String> = status
            .lan
            .as_ref()
            .map(|lan| lan.devices.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
        names.sort();
        names
    };
    eventually(&service, "the book read", |s| names(s).len() == 2).await;

    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    assert_eq!(names(&status), vec!["DESKTOP-7Q", "Den PC"]);
    let kept = vault.device_cache().unwrap().unwrap().devices;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].confirmed_at, fixture_now(), "confirmed by the list");

    // Removed on the web: listed, no longer approved.
    let mut removed = computer_row(&token("computer"), "operate");
    removed["approved"] = 0.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [removed] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(names(&status), vec!["Den PC"]);
    assert_eq!(vault.device_cache().unwrap().unwrap().devices, Vec::new());
}

// ---- Removals made offline -----------------------------------------------------------------------

/// ★ A device removed at the shack while the service cannot be told: out at the station at once, as
/// ever, and kept waiting (in the store too) instead of failing. While it waits, its proof grants
/// it nothing, though the service still lists it as approved; the service is told before the next
/// list once it answers, and it is no longer waiting.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_made_offline_is_sent_before_the_next_list_and_grants_nothing_meanwhile() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(status.pinned_devices, vec![BROWSER.to_string()], "premise");

    cloud.refusing_removals.store(true, Ordering::SeqCst);
    let removed = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            transmit: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert_eq!(removed.account.removals, vec![BROWSER.to_string()]);
    assert!(removed.station_permissions.is_empty());
    assert!(removed.pinned_devices.is_empty());
    assert_eq!(
        vault.revokes().unwrap(),
        Some(vault::Revokes {
            station_id: STATION.into(),
            devices: vec![BROWSER.into()]
        })
    );

    // The service still lists it as approved, with its proof: nothing is granted.
    let waiting = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(waiting.account.removals, vec![BROWSER.to_string()]);
    assert!(waiting.station_permissions.is_empty());
    assert!(waiting.pinned_devices.is_empty());

    // A restart reads them back, and still grants nothing.
    drop(service);
    let service = launch_owned(&cloud, &vault);
    let restarted = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(restarted.account.removals, vec![BROWSER.to_string()]);
    assert!(restarted.pinned_devices.is_empty());

    cloud.refusing_removals.store(false, Ordering::SeqCst);
    let heard = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(heard.account.removals, Vec::<String>::new());
    assert_eq!(heard.devices.len(), 0, "the service took it off its list");
    assert_eq!(
        vault.revokes().unwrap().unwrap().devices,
        Vec::<String>::new()
    );
}

/// A removal the service has meanwhile taken off its list (removed on the web too) is no longer
/// waiting, though the service still will not answer the removal itself.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_the_service_no_longer_lists_is_not_waiting() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    cloud.refusing_removals.store(true, Ordering::SeqCst);
    let removed = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            transmit: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert_eq!(
        removed.account.removals,
        vec![BROWSER.to_string()],
        "premise"
    );
    *cloud.devices.lock().unwrap() = json!({ "devices": [] }).to_string();
    let heard = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(heard.account.removals, Vec::<String>::new());
}

/// Removals that cannot be read may hide a device removed here: then no proof grants anything.
#[tokio::test(flavor = "multi_thread")]
async fn unreadable_removals_grant_nothing_on_a_proof() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    vault
        .values
        .lock()
        .unwrap()
        .insert("account-revokes".into(), "{not json".into());
    let service = launch_owned(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(
        status.account.unconfirmed,
        BTreeMap::from([(BROWSER.to_string(), "credentialStoreUnavailable")])
    );
    assert!(status.pinned_devices.is_empty());
}

/// A removal the service refuses by name (not for being out of reach) is that refusal, said at the
/// shack, and nothing waits to be sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_the_service_refuses_by_name_does_not_wait() {
    let origin = refusing_cloud("403 Forbidden", r#"{"error":"stationRevoked"}"#).await;
    let vault = paired_vault(&origin);
    let service = Service::with_accounts(
        origin,
        Box::new(vault.clone()),
        engine(),
        crate::remote_monitor::Publisher::default(),
        accounts(&vault, binder(Vec::new()).0),
    );
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let refused = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            transmit: false,
            key: None,
            kind: None,
        })
        .await;
    assert_eq!(refused.err(), Some("stationRevoked"));
    assert_eq!(
        service.status().unwrap().account.removals,
        Vec::<String>::new()
    );
    assert_eq!(vault.revokes().unwrap(), None);
}

// ---- People --------------------------------------------------------------------------------------

/// ★ Inviting someone tells the service (the address as typed, and the role), then keeps them here
/// by their address as the service matches it; their computer is then approved on their own proof.
/// Taking the invitation back takes them out here before the service is told.
#[tokio::test(flavor = "multi_thread")]
async fn an_invitation_is_kept_here_and_its_guest_is_approved_on_their_proof() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    // Invited again with another role: one invitation, the later role.
    for role in [Role::Listen, Role::Operate] {
        service
            .action(Action::Invite {
                email: " K9.Guest+radio@Example.test ".into(),
                role,
            })
            .await
            .unwrap();
    }
    let invited = service.status().unwrap();
    let kept = vec![Invite {
        mailbox: "k9.guest@example.test".into(),
        role: Role::Operate,
    }];
    assert_eq!(invited.account.people, kept);
    let enrolment = service.control.lock().unwrap().enrolment.clone();
    assert_eq!(
        enrolment.invites(),
        Some(kept.clone()),
        "the LAN takes them too"
    );
    assert_eq!(
        vault.people().unwrap(),
        Some(People {
            station_id: STATION.into(),
            invites: kept
        })
    );
    assert_eq!(
        cloud.invites.lock().unwrap().last().cloned(),
        Some((
            "invite".to_string(),
            json!({"email": "K9.Guest+radio@Example.test", "role": "operate"})
        ))
    );
    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("guest"), "operate")],
    )
    .await;
    assert_eq!(status.pinned_devices, vec![COMPUTER_ID.to_string()]);

    let taken_back = service
        .action(Action::Uninvite {
            email: "k9.guest@example.test".into(),
        })
        .await
        .unwrap();
    assert_eq!(taken_back.account.people, Vec::new());
    assert_eq!(vault.people().unwrap().unwrap().invites, Vec::new());
    assert_eq!(
        cloud.invites.lock().unwrap().last().cloned(),
        Some((
            "uninvite".to_string(),
            json!({"email": "k9.guest@example.test"})
        ))
    );
}

/// An invitation the service did not take is not kept; an address out of shape is sent nowhere; and
/// an invitation taken back while the service cannot be told is out here all the same.
#[tokio::test(flavor = "multi_thread")]
async fn invitations_fail_closed() {
    let cloud = fake_cloud().await;
    let invite = Invite {
        mailbox: "k9.guest@example.test".into(),
        role: Role::Operate,
    };
    let vault = owned_vault(&cloud.origin, vec![invite]);
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    for email in [
        "no-at-sign",
        "@example.test",
        "k9@",
        "two words@example.test",
    ] {
        assert!(
            matches!(
                service
                    .action(Action::Invite {
                        email: email.into(),
                        role: Role::Listen,
                    })
                    .await,
                Err("invalidRequest")
            ),
            "{email}"
        );
    }
    assert!(cloud.invites.lock().unwrap().is_empty());

    cloud.refusing.store(true, Ordering::SeqCst);
    assert!(matches!(
        service
            .action(Action::Invite {
                email: "w1aw@example.test".into(),
                role: Role::Listen,
            })
            .await,
        Err("serviceUnavailable")
    ));
    assert_eq!(vault.people().unwrap().unwrap().invites.len(), 1);
    assert!(matches!(
        service
            .action(Action::Uninvite {
                email: "K9.Guest@example.test".into(),
            })
            .await,
        Err("serviceUnavailable")
    ));
    assert_eq!(vault.people().unwrap().unwrap().invites, Vec::new());
}
