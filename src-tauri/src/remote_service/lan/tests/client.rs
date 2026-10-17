//! Two Nexus over loopback, end to end: client mode (`crate::client_mode`) on this computer,
//! through Tauri's own IPC on its mock runtime, driving the shack's real channel and command lane
//! behind a loopback port. It connects, takes the lease and the feeds, and every window's calls go
//! where their class says; Leave hands the lease back at once.
use super::commands::{running, App};
use super::computer::{behind_a_port, behind_a_port_on, paired_record};
use super::*;
use crate::client_mode::tests::{ask, eventually, scratch, screens};
use crate::client_mode::{Client, Find, Phase, ThisComputer};
use crate::lan_client::link::Link;
use crate::remote_service::vault::PairedStation;
use tauri::Manager;

/// ★ Client mode drives the shack, end to end: it connects to the station's real lane, takes the
/// lease, and the screens' snapshot poll is answered from the feed the shack pushes; a station
/// command runs at the shack, with this computer's own two settings shown in place of the shack's;
/// a start, with no Stop road beside this link, never leaves this computer, a Stop runs at the
/// shack, and a command of this computer's runs here. Leave hands the lease back, so control is
/// free at once. CONTROL: the shack ran exactly what was sent to it.
#[test]
fn client_mode_drives_the_shack_end_to_end() {
    let app = App::new();
    let (s, at, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        (s, at, stopper)
    });
    let record = paired_record(&s, at);
    let dir = scratch("end-to-end");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(STATION.to_string(), this));
    let screens = screens(&client);
    // The vault's record, as the store hands it out each time it is asked (a record carries this
    // computer's key, so it is never cloned).
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        screens.main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(at),
    ));
    eventually("control taken", || client.status().phase == Phase::Live);
    assert_eq!(client.status().station, at.to_string());
    eventually("the shack's snapshot here", || {
        ask(&screens.main, "get_snapshot", json!({})).is_ok_and(|snapshot| snapshot["n"].is_u64())
    });
    let mut settings = ask(&screens.main, "get_settings", json!({})).unwrap();
    let revision = settings
        .as_object_mut()
        .and_then(|fields| fields.remove("nexusRevision"));
    assert_eq!(
        settings,
        json!({ "settings": "as read", "betaUpdates": false, "launchAtLogin": false })
    );
    assert!(
        revision
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|r| r.len() == 64),
        "the shack's revision of the read, to the screen: {revision:?}"
    );
    assert_eq!(
        ask(&screens.main, "call_cq", json!({})),
        Err(json!("nexusClient:noStopRoad"))
    );
    assert!(ask(&screens.main, "halt_tx", json!({})).is_ok());
    assert_eq!(
        ask(&screens.popout, "app_version", json!({})),
        Ok(json!("ran here"))
    );
    assert_eq!(sent_here(&app), ["get_settings", "halt_tx"]);
    assert_eq!(screens.ran_here(), ["app_version"]);

    driving.abort();
    tauri::async_runtime::block_on(async {
        client.let_go().await;
        let (other, _) = Link::connect(&record, Some(at), 7, None)
            .await
            .expect("the link opens again");
        let mut state = json!({});
        for _ in 0..100 {
            let answer = other
                .operation(json!({"type":"state","requestId":id()}))
                .await
                .expect("an answer");
            if answer["error"] != "stationBusy" && answer["error"] != "remoteBusy" {
                state = answer;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(state["value"]["phase"], "available", "{state}");
    });
    assert_eq!(sent_here(&app), ["get_settings", "halt_tx"]);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What the shack ran for this computer's calls: everything it ran but the feeds' own commands,
/// which its lane runs itself while a watched feed is pushed (`App::ran` already leaves out the
/// snapshot's).
fn sent_here(app: &App) -> Vec<String> {
    app.ran()
        .into_iter()
        // The feeds' own reads at the shack, which run on their own pace whatever the screens do.
        .filter(|cmd| cmd != "get_meters" && cmd != "get_spectrum_row" && cmd != "get_rtty_scope")
        .collect()
}

/// ★ A station that proves the key pinned for its pairing but then names itself another station
/// is not the one this computer was asked to drive: refused, said why, and nothing runs there.
/// CONTROL (above): the station that names itself the one asked for is driven.
#[test]
fn a_station_that_names_itself_another_is_refused() {
    let app = App::new();
    let (s, at, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        (s, at, stopper)
    });
    let record = paired_record(&s, at);
    let dir = scratch("another");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let asked_for = "0aaaaaaa-0000-4000-8000-0000000000aa";
    let client = Arc::new(Client::new(asked_for.to_string(), this));
    let screens = screens(&client);
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        screens.main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(at),
    ));
    eventually("refused", || {
        client.status().reason.as_deref() == Some("notThisStation")
    });
    assert_eq!(client.status().phase, Phase::Refused);
    assert_eq!(
        ask(&screens.main, "get_band_plan", json!({})),
        Err(json!("nexusClient:notLive"))
    );
    driving.abort();
    assert!(sent_here(&app).is_empty(), "{:?}", app.ran());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ★ A station not where it was is found by name. With its one remembered address dead (a new
/// lease from the router, say) and no address typed, client mode looks for the station by name,
/// past another station's advert at a port of this one, and drives it where its own advert says
/// it is. CONTROL: with only the other station's advert found, it is never tried, nothing is
/// driven, and the station's own answer is told (nothing answered at the address it remembered).
/// Skipped, saying so, on a box with no private address of its own.
#[test]
fn client_mode_finds_a_station_by_name_when_its_remembered_address_is_dead() {
    use crate::lan_client::tests::where_nothing_listens;
    use ring::digest::{digest, SHA256};
    use tempo_stream::lan::dnssd::Found;
    let Some(private) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own");
        return;
    };
    let app = App::new();
    let (s, live, also, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let live = behind_a_port_on(&s, private, stop.clone()).await;
        let also = behind_a_port_on(&s, private, stop).await;
        (s, live, also, stopper)
    });
    let dead = where_nothing_listens(private);
    let record = paired_record(&s, live);
    let spki = tempo_stream::protocol::hex_bytes(&s.public_key).unwrap();
    let tag: String = digest(&SHA256, &spki)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()[..16]
        .to_string();
    let advert = |address: SocketAddrV4, key: &str| Found {
        name: "Nexus 3F2A 9B1C".into(),
        address,
        protocol: 1,
        key: key.into(),
    };
    let other = advert(also, "00c0ffee00c0ffee");
    for (adverts, found) in [
        (vec![other.clone(), advert(live, &tag)], true),
        (vec![other], false),
    ] {
        let dir = scratch("by-name");
        let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
        let client = Arc::new(Client::new(STATION.to_string(), this));
        let screens = screens(&client);
        let (device, key, pkcs8) = (
            record.device_id.clone(),
            record.station_key.clone(),
            record.pkcs8.clone(),
        );
        let find: Find = Arc::new(move |_| {
            Ok(Some(PairedStation {
                station_id: STATION.into(),
                device_id: device.clone(),
                station_key: key.clone(),
                pkcs8: pkcs8.clone(),
                addresses: vec![dead.to_string()],
            }))
        });
        let looks = Arc::new(AtomicUsize::new(0));
        let look: crate::lan_client::road::Look = {
            let looks = looks.clone();
            Arc::new(move |_| {
                looks.fetch_add(1, Ordering::SeqCst);
                Ok(adverts.clone())
            })
        };
        let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
            screens.main.app_handle().clone(),
            find,
            look,
            None,
        ));
        if found {
            eventually("control taken where it was found", || {
                client.status().phase == Phase::Live
            });
            assert_eq!(client.status().station, live.to_string());
        } else {
            eventually("a look made", || looks.load(Ordering::SeqCst) >= 1);
            eventually("the station's own answer told", || {
                client.status().reason.is_some()
            });
            let status = client.status();
            assert_ne!(status.phase, Phase::Live, "{status:?}");
            assert!(
                crate::lan_client::road::UNREACHED.contains(&status.reason.as_deref().unwrap()),
                "{status:?}"
            );
        }
        driving.abort();
        tauri::async_runtime::block_on(client.let_go());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// ⛔ A lease heartbeat the shack answers as busy keeps the lease. Client mode drives the shack
/// with the lease taken; then the shack's authority is busy for 2.5 s (its lock held, as another
/// request holds it a moment), so each heartbeat that comes meanwhile is answered `remoteBusy`.
/// The lease is held at every look, every 50 ms, through it and after: dropping it there stopped
/// the Stop road's heartbeat and the receive audio for a turn, then took the lease again. CONTROL:
/// the lease was held before the busy time, and looked at through it and 1.5 s past it; with the
/// busy answer read as a refusal, as before, the looks find it dropped.
#[test]
fn a_lease_heartbeat_answered_busy_keeps_the_lease() {
    let app = App::new();
    let (s, at, _listening) = tauri::async_runtime::block_on(async {
        let s = running(&app);
        s.shared.authority.permit_station(&s.device, true).unwrap();
        let (stopper, stop) = watch::channel(false);
        let at = behind_a_port(&s, stop).await;
        (s, at, stopper)
    });
    let record = paired_record(&s, at);
    let dir = scratch("busy-heartbeat");
    let this = ThisComputer::read(dir.join("settings.json"), dir.join("ui-state.json"));
    let client = Arc::new(Client::new(STATION.to_string(), this));
    let screens = screens(&client);
    let (device, key, pkcs8) = (
        record.device_id.clone(),
        record.station_key.clone(),
        record.pkcs8.clone(),
    );
    let find: Find = Arc::new(move |_| {
        Ok(Some(PairedStation {
            station_id: STATION.into(),
            device_id: device.clone(),
            station_key: key.clone(),
            pkcs8: pkcs8.clone(),
            addresses: vec![at.to_string()],
        }))
    });
    let driving = tauri::async_runtime::spawn(Arc::clone(&client).drive(
        screens.main.app_handle().clone(),
        find,
        Arc::new(|_| Ok(Vec::new())),
        Some(at),
    ));
    eventually("control taken", || client.lease_held().is_some());
    let before = client.lease_held();
    let busy = s.shared.authority.busy_for_a_test();
    let mut looks = Vec::new();
    let until = std::time::Instant::now() + Duration::from_millis(2500);
    while std::time::Instant::now() < until {
        looks.push(client.lease_held());
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(busy);
    for _ in 0..30 {
        looks.push(client.lease_held());
        std::thread::sleep(Duration::from_millis(50));
    }
    driving.abort();
    tauri::async_runtime::block_on(client.let_go());
    std::fs::remove_dir_all(&dir).unwrap();
    let dropped = looks.iter().filter(|held| **held != before).count();
    assert_eq!(
        (before.is_some(), looks.len() >= 70, dropped),
        (true, true, 0),
        "(held before, looked through it, looks when the lease was not the one held before)"
    );
}
