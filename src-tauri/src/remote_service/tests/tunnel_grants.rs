//! One rule for every device (the operator's decision of 2026-10-10): a device approved once at the
//! shack operates whenever Remote is on, and "End remote control" drops everyone and holds until
//! Remote is turned on again, with no device approved again after it.
//!
//! - A computer approved here for the room's tunnel holds station control as a computer paired on
//!   the LAN does: while hosted Remote is on, under the shack's own name for its key
//!   (`lan::tunnel_device_id`), never the service's id; removed here, it loses it at once.
//! - End remote control turns hosted Remote off (every browser and every tunnel session out) and
//!   ends Remote over this network; turning Remote on again gives each approved device back what
//!   it had, browsers included.
use super::lan_grants::{lan_acquire, lan_store, with_lan};
use super::*;
use crate::remote_service::lan::tests::Scratch;

/// A computer listed waiting, then approved here as the operator was shown it: the name the room's
/// tunnel takes it as.
async fn approved_computer(service: &Service, cloud: &FakeCloud, key: &DeviceKey) -> String {
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    approve_computer(service, COMPUTER, key).await.unwrap();
    let approved = service.control.lock().unwrap().approved.clone();
    let named = approved
        .paired(&key.pin())
        .expect("premise: the tunnel takes the computer approved here");
    assert_eq!(
        named,
        lan::tunnel_device_id(&key.pin()),
        "premise: its name"
    );
    named
}

/// ★ A computer approved here for the room's tunnel holds station control while hosted Remote is
/// on, as a LAN computer does, under its own name and never the service's id: it can take the
/// lease. Turn off takes it away, and Turn on gives it back with no approval again. It holds
/// nothing on the relay's lanes (the page's FT8/FT4 transmit stays a browser's tick).
#[tokio::test(flavor = "multi_thread")]
async fn a_tunnel_computer_holds_station_control_whenever_remote_is_on() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let named = approved_computer(&service, &cloud, &key).await;
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "an approved computer through the room holds no station control"
    );
    let status = service.status().unwrap();
    assert!(status.station_permissions.contains(&named), "shown held");
    assert!(
        !status.station_permissions.contains(&COMPUTER.to_string()),
        "held under the service's id"
    );
    assert!(
        !status.transmit_permissions.contains(&named)
            && !status.logging_permissions.contains(&named),
        "a computer holds nothing on the relay's lanes"
    );

    service.action(Action::Disable {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "held while Remote is off"
    );
    settle().await;
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "came back by itself while Remote is off"
    );

    service.action(Action::Enable {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "Turn on did not give it back"
    );
}

/// ★ Removed here, a tunnel computer loses station control at once, before the next request; a
/// computer still waiting, one the service lists with a key other than the one pinned, and one
/// never approved here are never held.
#[tokio::test(flavor = "multi_thread")]
async fn a_tunnel_computer_removed_or_never_approved_here_holds_nothing() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let named = lan::tunnel_device_id(&key.pin());
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "a computer waiting for approval"
    );
    // Listed approved by the service, but never approved here: no pin, so nothing.
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "a computer the service approved and the shack never did"
    );

    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(0, &key)] }).to_string();
    let named = approved_computer(&service, &cloud, &key).await;
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "premise"
    );
    // The service lists another key for it: not the pin, so out at once.
    let other = DeviceKey::new();
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &other)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "a listing with another key left it control"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &lan::tunnel_device_id(&other.pin())),
        "localPermissionRequired",
        "the key never pinned"
    );
    // Back, then removed here: out at once.
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "control: back"
    );
    service
        .action(Action::Device {
            device_id: COMPUTER.into(),
            approve: false,
            transmit: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert!(
        !service
            .status()
            .unwrap()
            .station_permissions
            .contains(&named),
        "removed, still shown held"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "removed here, it kept station control"
    );
}

/// ★ A tunnel computer holding the lease loses it the moment it is removed here: its control is
/// revoked, not only refused to its next acquire.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_tunnel_computer_ends_the_lease_it_holds() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let named = approved_computer(&service, &cloud, &key).await;
    let authority = service.control.lock().unwrap().operations.clone();
    let connection = operations::LanConnection::new(authority.clone());
    let session = lan_grants::fresh_id();
    let ask = |request: operations::Request| {
        authority.handle_version(
            (connection.id, 4),
            &session,
            &named,
            &request,
            &engine,
            Instant::now(),
        )
    };
    let boot = ask(operations::Request::State {
        request_id: lan_grants::fresh_id(),
    })
    .unwrap()["stationBootId"]
        .as_str()
        .unwrap()
        .to_string();
    let acquired = ask(operations::Request::Acquire {
        request_id: lan_grants::fresh_id(),
        station_boot_id: boot,
    })
    .unwrap();
    assert_eq!(acquired["phase"], "controlling", "premise");
    assert!(authority.lan_controls(connection.id, &named, Instant::now()));
    service
        .action(Action::Device {
            device_id: COMPUTER.into(),
            approve: false,
            transmit: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert!(
        !authority.lan_controls(connection.id, &named, Instant::now()),
        "removed here, it still held the lease"
    );
}

/// ★ End remote control drops everyone, browser, tunnel computer and LAN computer alike: hosted
/// Remote goes off (and stays off across a restart), Remote over this network ends, and nobody
/// holds anything. Turning Remote on again gives every approved device back what it had with no
/// approval again: the browser its control, logging and transmit tick, the tunnel computer its
/// control; the LAN computer comes back with its own switch, as before. The TX-enable latch is off
/// throughout.
#[tokio::test(flavor = "multi_thread")]
async fn end_remote_control_drops_everyone_and_holds_until_remote_is_turned_on_again() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let engine = engine();
    let store = lan_store();
    let scratch = Scratch::new();
    let service = with_lan(launch(&cloud, &vault, &engine), &engine, &store, &scratch);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service
        .action(Action::LanOn {
            address: None,
            port: None,
        })
        .await
        .unwrap();
    let status = eventually(&service, "the LAN book was read", |s| {
        s.lan.as_ref().is_some_and(|lan| !lan.devices.is_empty())
    })
    .await;
    let lan_computer = status.lan.unwrap().devices[0].id.clone();
    service.action(Action::Enable {}).await.unwrap();
    pending_browser(&cloud);
    service.action(Action::Refresh {}).await.unwrap();
    let granted = approve(&service, true).await;
    assert_eq!(
        granted.transmit_permissions,
        [BROWSER],
        "premise: the browser"
    );
    // The computer beside the browser in the service's list.
    let key = DeviceKey::new();
    let browser_listed: serde_json::Value =
        serde_json::from_str::<serde_json::Value>(&cloud.devices.lock().unwrap().clone()).unwrap()
            ["devices"][0]
            .clone();
    *cloud.devices.lock().unwrap() =
        json!({ "devices": [browser_listed.clone(), computer_listed(0, &key)] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    approve_computer(&service, COMPUTER, &key).await.unwrap();
    let named = lan::tunnel_device_id(&key.pin());
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "premise: the computer"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &lan_computer),
        "controlling",
        "premise: the LAN computer"
    );
    let browser_held = |s: &Status| {
        s.station_permissions.contains(&BROWSER.to_string())
            && s.logging_permissions.contains(&BROWSER.to_string())
            && s.transmit_permissions.contains(&BROWSER.to_string())
    };
    assert!(
        browser_held(&service.status().unwrap()),
        "premise: the browser's grants"
    );

    let ended = service.action(Action::TakeOverLogging {}).await.unwrap();
    assert!(!on(&ended), "End remote control left hosted Remote on");
    assert!(
        ended.station_permissions.is_empty()
            && ended.logging_permissions.is_empty()
            && ended.transmit_permissions.is_empty(),
        "End remote control left a grant: {}",
        serde_json::to_string(&ended).unwrap()
    );
    let lan = ended.lan.clone().unwrap();
    assert!(
        !lan.on && lan.reason == Some("endedAtShack"),
        "LAN not ended"
    );
    for (who, device) in [
        ("browser", BROWSER),
        ("computer", &named),
        ("LAN computer", &lan_computer),
    ] {
        assert_eq!(
            lan_acquire(&service, &engine, device),
            "localPermissionRequired",
            "the {who} kept station control"
        );
    }
    assert!(transmitter_idle(&engine));
    // It holds: across a wait and a restart, nobody is back and Remote stays off.
    settle().await;
    drop(service);
    let service = with_lan(launch(&cloud, &vault, &engine), &engine, &store, &scratch);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    settle().await;
    let held = service.status().unwrap();
    assert!(
        !on(&held),
        "Remote came back on by itself after End remote control"
    );
    assert!(
        held.station_permissions.is_empty(),
        "a grant came back by itself"
    );
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "localPermissionRequired",
        "the computer came back by itself"
    );

    // Turned on again: everyone approved is back, with no approval again.
    service.action(Action::Enable {}).await.unwrap();
    let back = eventually(&service, "the browser's grants came back", browser_held).await;
    assert!(on(&back), "Remote is on");
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &named),
        "controlling",
        "the computer needed approving again"
    );
    service
        .action(Action::LanOn {
            address: None,
            port: None,
        })
        .await
        .unwrap();
    assert_eq!(
        lan_acquire(&service, &engine, &lan_computer),
        "controlling",
        "the LAN computer"
    );
    assert!(transmitter_idle(&engine), "the latch is still off");
}

/// The service tells a Nexus that asks for kinds which approvals last until removed, and this
/// station shows so; a service from before says nothing of it, and the device is read as before.
#[tokio::test(flavor = "multi_thread")]
async fn an_approval_until_removed_is_read_and_shown_and_a_service_without_it_still_lists() {
    let cloud = fake_cloud().await;
    let engine = engine();
    let service = on_at(&cloud, &engine).await;
    let key = DeviceKey::new();
    let mut listed = computer_listed(1, &key);
    listed["untilRemoved"] = 1.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [listed] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    let shown = serde_json::to_value(&status.devices).unwrap();
    assert_eq!(shown[0]["untilRemoved"], 1, "{shown}");
    *cloud.devices.lock().unwrap() = json!({ "devices": [computer_listed(1, &key)] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    let shown = serde_json::to_value(&status.devices).unwrap();
    assert_eq!(shown[0]["untilRemoved"], 0, "{shown}");
}
