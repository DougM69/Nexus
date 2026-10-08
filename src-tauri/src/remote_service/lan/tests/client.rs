//! Two Nexus over loopback, end to end: client mode (`crate::client_mode`) on this computer,
//! through Tauri's own IPC on its mock runtime, driving the shack's real channel and command lane
//! behind a loopback port. It connects, takes the lease and the feeds, and every window's calls go
//! where their class says; Leave hands the lease back at once.
use super::commands::{running, App};
use super::computer::{behind_a_port, paired_record};
use super::*;
use crate::client_mode::tests::{ask, eventually, scratch, screens};
use crate::client_mode::{Client, Find, Phase, ThisComputer};
use crate::lan_client::link::Link;
use crate::remote_service::vault::PairedStation;
use tauri::Manager;

/// ★ Client mode drives the shack, end to end: it connects to the station's real lane, takes the
/// lease, and the screens' snapshot poll is answered from the feed the shack pushes; a station
/// command runs at the shack, with this computer's own two settings shown in place of the shack's;
/// a start never leaves this computer, a Stop runs at the shack, and a command of this computer's
/// runs here. Leave hands the lease back, so control is free at once. CONTROL: the shack ran
/// exactly what was sent to it.
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
        Some(at),
    ));
    eventually("control taken", || client.status().phase == Phase::Live);
    assert_eq!(client.status().station, at.to_string());
    eventually("the shack's snapshot here", || {
        ask(&screens.main, "get_snapshot", json!({})).is_ok_and(|snapshot| snapshot["n"].is_u64())
    });
    assert_eq!(
        ask(&screens.main, "get_settings", json!({})),
        Ok(json!({ "settings": "as read", "betaUpdates": false, "launchAtLogin": false }))
    );
    assert_eq!(
        ask(&screens.main, "call_cq", json!({})),
        Err(json!("nexusClient:transmitNotYet"))
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
        let (other, _) = Link::connect(&record, Some(at), 7)
            .await
            .expect("the link opens again");
        let mut state = json!({});
        for _ in 0..100 {
            let answer = other
                .operation(json!({"type":"state","requestId":id()}))
                .await
                .expect("an answer");
            if answer["error"] != "stationBusy" {
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
        .filter(|cmd| cmd != "get_meters" && cmd != "get_spectrum_row")
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
