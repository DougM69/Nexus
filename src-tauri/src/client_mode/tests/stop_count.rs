//! Every call carries how many Stops this computer had pressed when it was made, a Stop its own
//! number (the one its second road carries too), and every hold names the count its press began
//! under: what the shack judges a start or a press by (`remote_service::lan::stop_road::Ready`).
use super::*;

/// ★ What each call and each road datagram carries, by value. A CQ pressed first carries 0. The off
/// half of Enable TX is the first Stop: 1, on the link alone, as an off half goes. Stop TX is the
/// second: 2 on the link, and numbered 2 on its road. A CQ pressed after both carries 2, and so does
/// a station command. A press begun then names 2 at every hold, those after a third Stop (the off
/// half again, which leaves the press held) included. CONTROL: the shack was sent exactly those
/// calls, in order, and the road no Stop but Stop TX's.
#[test]
fn every_call_and_hold_carries_the_stops_pressed_before_it() {
    use crate::remote_service::lan::stop_road::Up;
    let (shack, end, _client, s, dir) = present_for(Duration::from_secs(10));
    for (cmd, args) in [
        ("call_cq", json!({})),
        ("set_tx_enabled", json!({"enabled": false})),
        ("halt_tx", json!({})),
        ("call_cq", json!({})),
        ("clear_hunt_target", json!({})),
    ] {
        assert!(ask(&s.main, cmd, args).is_ok(), "{cmd}");
    }
    for _ in 0..3 {
        assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
        std::thread::sleep(Duration::from_millis(100));
    }
    eventually("three holds", || end.presses().len() >= 3);
    assert!(ask(&s.main, "set_tx_enabled", json!({"enabled": false})).is_ok());
    for _ in 0..2 {
        assert!(ask(&s.main, "set_ptt", json!({"on": true})).is_ok());
        std::thread::sleep(Duration::from_millis(100));
    }
    eventually("five holds", || end.presses().len() >= 5);
    let counted: Vec<(String, u64)> = lock(&shack.counted).clone();
    let mut stops: Vec<u64> = end
        .ups()
        .into_iter()
        .filter_map(|up| match up {
            Up::Stop { stop } => Some(stop),
            _ => None,
        })
        .collect();
    stops.dedup();
    let holds = end.presses();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        counted,
        [
            ("call_cq", 0),
            ("set_tx_enabled", 1),
            ("halt_tx", 2),
            ("call_cq", 2),
            ("clear_hunt_target", 2),
            ("set_tx_enabled", 3),
        ]
        .map(|(cmd, stops)| (cmd.to_string(), stops))
        .to_vec(),
        "what each call carried"
    );
    assert_eq!(stops, [2], "the road's Stop, by its number");
    assert_eq!(
        holds[..5].to_vec(),
        vec![Up::Hold { press: 1, stops: 2 }; 5],
        "each hold of the press"
    );
}
