//! A contest contact reaches a program listening on the WSJT-X UDP API COMPLETE, as WSJT-X sends
//! one: `MainWindow::acceptQSO` (`widgets/mainwindow.cpp` 10652-10655) sends a `QSOLogged` with the
//! grid, both reports, the power, the frequency on the air (dial + Tx audio), the start and end
//! times, the operator and both exchanges as they went over the air, and then a `LoggedADIF` with
//! the record — for every contest, not Field Day alone.
//!
//! The loop is the real `RadioLoop::step`, crossing a slot boundary with a WSJT-X sink pointed at a
//! socket of the test's own; every datagram that arrives is read back.

use super::*;

/// Every `QSOLogged` and `LoggedADIF` datagram `listener` receives, in arrival order.
fn logged_datagrams(listener: &std::net::UdpSocket) -> Vec<WsjtxInbound> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    while let Ok((n, _)) = listener.recv_from(&mut buf) {
        match tempo_net::wsjtx::parse_inbound(&buf[..n]) {
            Some(m @ WsjtxInbound::QsoLogged { .. })
            | Some(m @ WsjtxInbound::LoggedAdif { .. }) => out.push(m),
            _ => {}
        }
    }
    out
}

/// Run `contest` with an operator, a power and a TX audio offset set, log one contact through
/// `log`, cross a slot boundary, and return what the WSJT-X sink sent.
fn sent_for(contest: &str, log: impl FnOnce(&mut Engine)) -> Vec<WsjtxInbound> {
    let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    listener
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
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
        s.fd_active = true;
        s.fd_event = contest.to_string();
        s.fd_class = "2B".to_string();
        s.fd_section = "EMA".to_string();
        s.fd_operator = "n1op".to_string();
        s.station_power_w = Some(100.0);
        s.dial_mhz = 50.313;
        s.band = "6m".to_string();
        s.tx_offset_hz = 1500.0;
        eng.apply_settings(s);
        eng.set_tx_offset(1500.0);
        assert!(eng.snapshot().field_day.is_some(), "{contest} running");
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
    // The first boundary seeds the forwarder's cursor past every row a restore loaded, so the
    // contact is logged after it, as one is in a running session.
    let mut log = Some(log);
    for now in [0.0, 16_000.0] {
        if now > 0.0 {
            (log.take().unwrap())(&mut engine.lock().unwrap());
        }
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
    logged_datagrams(&listener)
}

/// ⭐ An ARRL Field Day contact: the frequency on the air, the operator at the key, the power, the
/// received exchange, and a `LoggedADIF` after the `QSOLogged`.
#[test]
fn a_field_day_contact_goes_out_complete_with_its_adif() {
    let sent = sent_for("arrlfd", |e| {
        assert!(e
            .fd_log_manual_submode("W1AW", "1D", "CT", "DIG", "FT8")
            .unwrap());
    });
    let [WsjtxInbound::QsoLogged { qso, .. }, WsjtxInbound::LoggedAdif { adif, .. }] =
        sent.as_slice()
    else {
        panic!("a QSOLogged then a LoggedADIF, got {sent:?}");
    };
    assert_eq!(qso.dx_call, "W1AW");
    assert_eq!(
        qso.tx_freq, 50_314_500,
        "dial + Tx audio, as WSJT-X sends it"
    );
    assert_eq!(
        qso.op_call, "N1OP",
        "the operator at the key, not the station's call"
    );
    assert_eq!(qso.my_call, "K1ABC");
    assert_eq!(qso.tx_power, "100");
    assert_eq!(qso.exchange_sent, "2B EMA");
    assert_eq!(qso.exchange_recvd, "1D CT");
    assert!(qso.time_on > 0 && qso.time_on <= qso.time_off, "{qso:?}");
    assert!(adif.contains("<CALL:4>W1AW"), "{adif}");
    assert!(adif.contains("<OPERATOR:4>N1OP"), "{adif}");
    assert!(adif.contains("<FREQ:9>50.314500"), "{adif}");
    assert!(adif.contains("<SRX_STRING:5>1D CT"), "{adif}");
}

/// ⭐ Outside Field Day the received exchange is what was received, not Field Day's two blank
/// slots, and a grid the exchange carried is the contact's grid.
#[test]
fn a_vhf_contest_contact_carries_its_grid_and_exchange() {
    let sent = sent_for("arrlvhf_jun", |e| {
        let fields = vec![("GRID".to_string(), "FN31".to_string())];
        assert!(e
            .contest_log_manual("W1AW", &fields, "DIG", Some("FT8"))
            .unwrap());
    });
    let [WsjtxInbound::QsoLogged { qso, .. }, WsjtxInbound::LoggedAdif { adif, .. }] =
        sent.as_slice()
    else {
        panic!("a QSOLogged then a LoggedADIF, got {sent:?}");
    };
    assert_eq!(qso.dx_grid, "FN31");
    assert_eq!(qso.exchange_recvd, "FN31");
    assert_eq!(qso.tx_freq, 50_314_500);
    assert!(adif.contains("<GRIDSQUARE:4>FN31"), "{adif}");
}
