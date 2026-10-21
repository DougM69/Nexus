//! The WSJT-X UDP Decode message's `Time` is the START of the period the decode was heard in,
//! whenever the datagram goes out — a late full-pass decode included.
//!
//! WSJT-X 3.0.2 stamps every pass of a period with the period's start (`dec_data.params.nutc`,
//! `widgets/mainwindow.cpp:5397-5404`, truncated to the T/R period) and sends the decode line's
//! `hhmmss` as the Decode's `Time` (`postDecode`, :13409-13410; `Network/NetworkMessage.hpp`:
//! Decode, `Time QTime`), for a Replay too. JTAlert, GridTracker and loggers group decodes by it,
//! so the arrival time put a decode of the :15 period that landed at :31 under the :30 period.
//! ALL.TXT already stamps the period start; the datagram now says the same.

use super::decode_in_flight::ft8_frame;
use super::*;

/// 2023-11-14 22:13:15 UTC: the start of an FT8 period.
const PERIOD_START_S: u64 = 1_699_999_995;
/// The engine's index for that period's decodes: the audio's slot + 1, for both passes.
const SLOT: u64 = PERIOD_START_S / 15 + 1;
/// 22:13:15 as a Decode's `Time`: milliseconds since midnight UTC.
const PERIOD_START_MS_OF_DAY: u32 = (22 * 3600 + 13 * 60 + 15) * 1000;

/// W1AW's CQ heard in the 22:13:15 period, on an FT8 station.
fn heard_in_the_period() -> Engine {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_tier(Tier::Ft8);
    assert!(
        e.ingest(&ft8_frame("CQ W1AW FN31"), SLOT) >= 1,
        "premise: W1AW's CQ is decoded"
    );
    e
}

/// `(New, Time, Message)` of one Decode datagram, read in `NetworkMessage.hpp`'s order.
fn decode_on_the_wire(datagram: &[u8]) -> (bool, u32, String) {
    let mut r = tempo_net::qds::QdsReader::new(datagram);
    assert_eq!(r.read_u32(), Some(tempo_net::wsjtx::MAGIC));
    r.read_u32().unwrap(); // schema
    assert_eq!(r.read_u32(), Some(tempo_net::wsjtx::msg_type::DECODE));
    r.read_utf8().unwrap(); // Id
    let new = r.read_bool().unwrap();
    let time = r.read_u32().unwrap(); // QTime: ms since midnight
    r.read_i32().unwrap(); // snr
    r.read_f64().unwrap(); // Delta time
    r.read_u32().unwrap(); // Delta frequency
    r.read_utf8().unwrap(); // Mode
    let message = r.read_utf8().unwrap().unwrap_or_default();
    (new, time, message)
}

/// A UDP consumer, and Nexus's WSJT-X server sending to it.
fn consumer() -> (std::net::UdpSocket, WsjtxServer) {
    let listener = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    listener
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let server = WsjtxServer::new(
        "127.0.0.1:0".parse().unwrap(),
        listener.local_addr().unwrap(),
    )
    .unwrap();
    (listener, server)
}

/// The next datagram the consumer receives.
fn received(listener: &std::net::UdpSocket) -> Vec<u8> {
    let mut buf = [0u8; 4096];
    let (n, _) = listener.recv_from(&mut buf).expect("a datagram");
    buf[..n].to_vec()
}

/// A decode of the 22:13:15 period goes out stamped 22:13:15 whether it is sent at :27 (the early
/// pass), at :31 or at :34 (a full pass that ended in the next period), and New, as WSJT-X sends
/// a decode as it completes (`postDecode (true, …)`, mainwindow.cpp 3416, 5676, 7383).
#[test]
fn a_decode_goes_out_stamped_with_its_period_start_whenever_it_is_sent() {
    let e = heard_in_the_period();
    let (listener, server) = consumer();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
        cfg_dial_hz: 14_074_000,
    };
    let (mut spots, mut cache) = (Vec::new(), SpotCache::default());
    for sent_at in [27.0, 31.0, 34.0] {
        let now = (PERIOD_START_S as f64 - 15.0 + sent_at) * 1000.0;
        emit_rx_decodes(&sinks, &e, &mut spots, &mut cache, now, 14_074_000);
        let (new, time, message) = decode_on_the_wire(&received(&listener));
        assert_eq!(message, "CQ W1AW FN31");
        assert_eq!(
            (new, time),
            (true, PERIOD_START_MS_OF_DAY),
            "sent at :{sent_at}: the Decode's New and Time"
        );
    }
}

/// Control: ALL.TXT stamps that decode's line with the same second the datagram carries.
#[test]
fn all_txt_stamps_the_same_period_start() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    let mut s = e.settings().clone();
    s.write_all_txt = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    assert!(e.ingest(&ft8_frame("CQ W1AW FN31"), SLOT) >= 1);
    let lines = e.take_all_txt_pending();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("231114_221315") && lines[0].ends_with("CQ W1AW FN31"),
        "{}",
        lines[0]
    );
}

/// A logger's Replay gets the period's decodes back with the period's start, not the moment it
/// asked, and not New: WSJT-X replays each old decode "with the 'New' field set to false"
/// (`Network/NetworkMessage.hpp` 221-226; `replayDecodes` → `postDecode (false, …)`,
/// mainwindow.cpp 13374-13396).
#[test]
fn a_replay_sends_the_period_start() {
    let engine = Arc::new(Mutex::new(heard_in_the_period()));
    let (listener, server) = consumer();
    let sinks = Sinks {
        wsjtx: Some(&server),
        psk: None,
        cfg_dial_hz: 14_074_000,
    };
    let mut replay = tempo_net::qds::QdsWriter::new();
    replay
        .put_u32(tempo_net::wsjtx::MAGIC)
        .put_u32(tempo_net::wsjtx::SCHEMA)
        .put_u32(tempo_net::wsjtx::msg_type::REPLAY)
        .put_utf8(Some("JTAlert"));
    listener
        .send_to(&replay.into_bytes(), server.local_addr().unwrap())
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let mut backend = MockBackend::new();
    let mut rig = Rig::vox();
    let mut state = loop_state_for(&engine);
    let (mut ra, mut reopen) = (mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    // Asked at 22:13:41, in the period after the one heard.
    let now = (PERIOD_START_S as f64 + 26.0) * 1000.0;
    state
        .step(
            &engine,
            &mut backend,
            &mut rig,
            &sinks,
            now,
            &mut ra,
            &mut reopen,
            &mut station,
        )
        .unwrap();
    // The loop also sends a Status; read until the replayed Decode.
    let decode = std::iter::repeat_with(|| received(&listener))
        .take(16)
        .find(|d| d.get(8..12) == Some(&tempo_net::wsjtx::msg_type::DECODE.to_be_bytes()[..]))
        .expect("the replayed Decode");
    let (new, time, message) = decode_on_the_wire(&decode);
    assert_eq!(message, "CQ W1AW FN31");
    assert_eq!(
        (new, time),
        (false, PERIOD_START_MS_OF_DAY),
        "the replayed Decode's New and Time"
    );
}
