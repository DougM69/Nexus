//! The WSJT-X UDP Status names a running contest's own activity, as WSJT-X reports the one it is
//! set to: `statusUpdate` sends `m_specOp` (`widgets/mainwindow.cpp` 13955-13985), whose numbers
//! `NetworkMessage.hpp` lists — 0 NONE, 1 NA VHF, 3 FIELD DAY, 7 HOUND. ARRL Field Day is FIELD
//! DAY, the three ARRL VHF contests are NA VHF, and every other contest is NONE, Winter Field Day
//! included (WSJT-X's Field Day exchange takes the classes A to F only). A Hound setting left on
//! never names Hound over a contest that is running.
//!
//! The loop is the real `RadioLoop::step`, crossing a slot boundary with a WSJT-X sink pointed at
//! a socket of the test's own; the number is read out of the Status datagram that arrives. CQ WW
//! and CQ WPX are not here: they do not start without a country file, which this binary does not
//! install (tempo-core's table test names their activity).

use super::*;

/// The contests that start without a country file: the picker's list less CQ WW and CQ WPX, and
/// the blank contest a fresh install carries.
const CONTESTS: [&str; 14] = [
    "",
    "arrlfd",
    "wfd",
    "arrlss_cw",
    "arrlss_ssb",
    "arrlvhf_jan",
    "arrlvhf_jun",
    "arrlvhf_sep",
    "cqp",
    "ilqp",
    "nyqp",
    "ohqp",
    "tnqp",
    "txqp",
];

/// WSJT-X's number for the activity a station in `contest` sets (`NetworkMessage.hpp`): this
/// test's own table, not the code's.
fn wsjtx_number(contest: &str) -> u8 {
    match contest {
        "" | "arrlfd" => 3,
        "arrlvhf_jan" | "arrlvhf_jun" | "arrlvhf_sep" => 1,
        _ => 0,
    }
}

/// The special operation number in the first Status datagram `listener` receives.
fn status_special_op(listener: &std::net::UdpSocket) -> u8 {
    let mut buf = [0u8; 4096];
    loop {
        let (n, _) = listener.recv_from(&mut buf).expect("a Status datagram");
        let mut r = tempo_net::qds::QdsReader::new(&buf[..n]);
        let (_magic, _schema, kind) = (r.read_u32(), r.read_u32(), r.read_u32());
        if kind != Some(tempo_net::wsjtx::msg_type::STATUS) {
            continue;
        }
        r.read_utf8(); // id
        r.read_u64(); // dial
        for _ in 0..4 {
            r.read_utf8(); // mode, DX call, report, TX mode
        }
        for _ in 0..3 {
            r.read_bool(); // TX enabled, transmitting, decoding
        }
        r.read_u32(); // RX offset
        r.read_u32(); // TX offset
        for _ in 0..3 {
            r.read_utf8(); // DE call, DE grid, DX grid
        }
        r.read_bool(); // TX watchdog
        r.read_utf8(); // submode
        r.read_bool(); // fast mode
        return r.read_u8().expect("the special operation number");
    }
}

/// The special operation number a station sends with `contest` running (`None`: the contest
/// switch off) and a Hound setting left as `hound`, once the loop has crossed a slot boundary.
fn status_for(contest: Option<&str>, hound: bool) -> u8 {
    let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    listener
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let server = WsjtxServer::new(
        "127.0.0.1:0".parse().unwrap(),
        listener.local_addr().unwrap(),
    )
    .unwrap();
    let engine = Arc::new(Mutex::new(Engine::new("K1ABC", "FN42", 0)));
    {
        let mut eng = engine.lock().unwrap();
        let mut s = eng.settings().clone();
        s.fd_active = contest.is_some();
        s.fd_event = contest.unwrap_or_default().to_string();
        // Winter Field Day's categories are H, I, O and M; ARRL Field Day's are A to F.
        s.fd_class = if contest == Some("wfd") { "1H" } else { "2B" }.to_string();
        s.fd_section = "EMA".to_string();
        s.contest_qth_state = "MA".to_string();
        s.contest_cq_zone = 5;
        s.contest_itu_zone = 8;
        s.contest_check = "74".to_string();
        s.contest_category_operator = "SINGLE-OP".to_string();
        s.contest_category_power = "LOW".to_string();
        s.contest_category_assisted = "NON-ASSISTED".to_string();
        s.special_op = if hound {
            tempo_app::settings::SpecialOp::Hound
        } else {
            tempo_app::settings::SpecialOp::None
        };
        eng.apply_settings(s);
        assert_eq!(
            eng.snapshot().field_day.is_some(),
            contest.is_some(),
            "{contest:?}: the contest is running exactly when it is switched on"
        );
    }
    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
    };
    let (mut ra, mut rr) = (mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    for now in [0.0, 16_000.0] {
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                now,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
    }
    status_special_op(&listener)
}

/// ⭐ Each running contest's Status carries WSJT-X's own number for it, with or without a Hound
/// setting left on; with no contest running, Hound is 7 and nothing is 0, as before.
#[test]
fn the_status_names_the_running_contests_wsjtx_activity() {
    for contest in CONTESTS {
        assert_eq!(
            status_for(Some(contest), false),
            wsjtx_number(contest),
            "{contest:?}"
        );
        assert_eq!(
            status_for(Some(contest), true),
            wsjtx_number(contest),
            "{contest:?}, a Hound setting left on"
        );
    }
    assert_eq!(status_for(None, true), 7, "Hound, no contest running");
    assert_eq!(status_for(None, false), 0, "no contest, no activity");
}
