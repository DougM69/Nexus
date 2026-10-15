//! JS8Call's API for programs on the station's own computer is the operator's to turn on at the
//! station, and only there. Its six settings never reach a Remote browser (the hosted page and
//! the LAN page read the same document through these lanes), and a browser change that names one
//! is refused before anything is consumed. Nothing a browser can send reaches the API's sockets:
//! they listen on 127.0.0.1 alone (`src-tauri/src/js8_api.rs`).
use super::*;

const KEYS: [&str; 6] = [
    "js8ApiTcp",
    "js8ApiTcpPort",
    "js8ApiUdp",
    "js8ApiUdpPort",
    "js8ApiTcpAccept",
    "js8ApiUdpAccept",
];

fn store(f: &Fixture) {
    f.engine
        .lock()
        .unwrap()
        .configure_remote_settings_store(f.dir.join("settings.json"));
}

fn settings(f: &Fixture) -> Value {
    serde_json::to_value(f.engine.lock().unwrap().settings()).unwrap()
}

fn revision(f: &Fixture) -> String {
    super::super::super::query::settings_revision(f.engine.lock().unwrap().settings()).unwrap()
}

fn run(f: &Fixture, request: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, 4),
        SESSION,
        DEVICE,
        request,
        &f.engine,
        Instant::now(),
    )
}

fn lease(f: &Fixture) {
    f.authority.permit(DEVICE, true).unwrap();
    f.authority.permit_station(DEVICE, true).unwrap();
    let state = control_state_version(f, Instant::now(), 4);
    run(
        f,
        &Request::Acquire {
            request_id: id(),
            station_boot_id: state["stationBootId"].as_str().unwrap().into(),
        },
    )
    .unwrap();
}

fn change(f: &Fixture, values: Value) -> Request {
    let s = control_state_version(f, Instant::now(), 4);
    serde_json::from_value(json!({"type":"logChange","requestId":id(),"stationBootId":s["stationBootId"],
        "leaseId":s["leaseId"],"expectedRevision":s["revision"],"commandWindowId":s["commandWindowId"],
        "clientSequence":s["nextSequence"],"change":{"kind":"settings","revision":revision(f),"values":values}}))
    .unwrap()
}

/// Turned on at the station, on ports of the operator's own, the API's settings are still not
/// in the document a browser reads, nor named anywhere in it.
#[test]
fn the_api_settings_never_reach_a_browser() {
    let f = Fixture::new();
    {
        let mut e = f.engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.js8_api_tcp = true;
        s.js8_api_tcp_port = 2443;
        s.js8_api_udp = true;
        s.js8_api_udp_port = 2243;
        s.js8_api_tcp_accept = true;
        s.js8_api_udp_accept = true;
        e.apply_settings(s);
    }
    let native = settings(&f);
    let document = super::super::super::query::configuration_probe(&f.engine).unwrap();
    let view = document["settings"]["settings"].as_object().unwrap();
    for key in KEYS {
        // The control: each name is the station's own wire name, so its absence below is a
        // decision and not a typo.
        assert!(native.get(key).is_some(), "{key} is not a setting");
        assert!(!view.contains_key(key), "{key} reached a browser");
        assert!(
            !document.to_string().contains(key),
            "{key} is named to a browser"
        );
    }
    assert!(
        view.contains_key("js8Info"),
        "the document still carries the JS8 settings a browser may see"
    );
}

/// A browser change naming any of them, alone or beside an allowed one, is refused and spends
/// nothing; the same change without it is saved.
#[test]
fn a_browser_cannot_turn_the_api_on_or_move_it() {
    let f = Fixture::new();
    store(&f);
    lease(&f);
    let before = settings(&f);
    let sequence = control_state_version(&f, Instant::now(), 4)["nextSequence"].clone();
    for values in [
        json!({"js8ApiTcp":true}),
        json!({"js8ApiTcpPort":2443}),
        json!({"js8ApiUdp":true}),
        json!({"js8ApiUdpPort":2243}),
        json!({"js8ApiTcpAccept":true}),
        json!({"js8ApiUdpAccept":true}),
        json!({"contestCheck":"73","js8ApiTcp":true}),
        json!({"contestCheck":"73","js8ApiUdpAccept":true}),
    ] {
        assert_eq!(
            run(&f, &change(&f, values.clone())),
            Err("invalidRecord"),
            "{values}"
        );
    }
    assert_eq!(
        control_state_version(&f, Instant::now(), 4)["nextSequence"],
        sequence,
        "a denial spends no command"
    );
    assert_eq!(settings(&f), before);
    assert!(!f.dir.join("settings.json").exists(), "nothing was saved");
    assert_eq!(
        run(&f, &change(&f, json!({"contestCheck":"73"}))).unwrap()["outcome"],
        "applied"
    );
}
