//! JS8NET — JS8Call's spotting network, `spot.js8call.com:50000`: the UDP JSON datagrams JS8Call
//! 3.0.3's `SpotClient` sends (`JS8_Network/SpotClient.cpp` and `JS8_Main/Message.cpp`, read as
//! the protocol; nothing here is its code).
//!
//! Each datagram is ONE compact JSON object, `{"params":{…},"type":"…","value":""}`, its keys in
//! the order Qt writes a `QJsonObject` (sorted, so `_ID` comes last). `params._ID` is a STRING:
//! milliseconds since 2017-07-06 00:00 UTC, JS8Call's message epoch, taken when the record was
//! made. Three kinds go up:
//!
//! * `RX.LOCAL` — this station: `CALLSIGN`, `GRID`, `INFO`, `VERSION`.
//! * `RX.SPOT` — a station heard: `BY {CALLSIGN, GRID}` (this station), `CALLSIGN`, `GRID`,
//!   `FREQ` (dial + offset), `DIAL`, `OFFSET`, `SNR`, `SPEED` (JS8Call's submode number).
//! * `RX.DIRECTED` — a command heard addressed to a group (`@JS8NET`, `@APRSIS`, `@HB`,
//!   `@ALLCALL`, …): `BY`, `CMD`, `FROM`, `TO`, `PATH`, `TEXT`, `GRID`, `EXTRA`, `FREQ`, `DIAL`,
//!   `OFFSET`, `SNR`, `SPEED`.
//!
//! The text fields carry what was heard over the air from strangers. `serde_json` escapes every
//! one of them, so nothing heard can break out of its string; the station layer has already capped
//! their lengths. JS8Call drains its queue once a minute (`SEND_INTERVAL`); so does the caller.
//!
//! Outbound only: [`SpotClient::send`] binds an ephemeral UDP port for the batch and drops it
//! after, reading nothing. Tests send to a loopback listener; none touches the real network.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

use serde_json::{Map, Value};

/// JS8Call's spot server (`UI_Constructor.cpp:69`).
pub const DEFAULT_TARGET: &str = "spot.js8call.com:50000";

/// JS8Call's message-id epoch, 2017-07-06 00:00 UTC in Unix milliseconds (`Message.cpp`).
pub const ID_EPOCH_MS: i64 = 1_499_299_200_000;

/// This station, as every `RX.SPOT` and `RX.DIRECTED` names it (`SpotClient::impl::by`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct By {
    pub call: String,
    pub grid: String,
}

/// One JS8NET record, before it is stamped and serialised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record {
    /// `RX.LOCAL`: this station, sent at the start and whenever it changes.
    Local {
        call: String,
        grid: String,
        info: String,
        version: String,
    },
    /// `RX.SPOT`: a station heard.
    Spot {
        by: By,
        call: String,
        grid: String,
        dial: i64,
        offset: i64,
        snr: i32,
        speed: i32,
    },
    /// `RX.DIRECTED`: a command heard addressed to a group.
    Directed {
        by: By,
        cmd: String,
        from: String,
        to: String,
        path: String,
        text: String,
        grid: String,
        extra: String,
        dial: i64,
        offset: i64,
        snr: i32,
        speed: i32,
    },
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn by_value(by: &By) -> Value {
    let mut m = Map::new();
    m.insert("CALLSIGN".into(), s(&by.call));
    m.insert("GRID".into(), s(&by.grid));
    Value::Object(m)
}

impl Record {
    /// The datagram for this record, its `_ID` taken from `at_ms` (Unix milliseconds).
    pub fn to_json(&self, at_ms: i64) -> String {
        let mut p = Map::new();
        let kind = match self {
            Record::Local {
                call,
                grid,
                info,
                version,
            } => {
                p.insert("CALLSIGN".into(), s(call));
                p.insert("GRID".into(), s(grid));
                p.insert("INFO".into(), s(info));
                p.insert("VERSION".into(), s(version));
                "RX.LOCAL"
            }
            Record::Spot {
                by,
                call,
                grid,
                dial,
                offset,
                snr,
                speed,
            } => {
                p.insert("BY".into(), by_value(by));
                p.insert("CALLSIGN".into(), s(call));
                p.insert("DIAL".into(), Value::from(*dial));
                p.insert("FREQ".into(), Value::from(dial + offset));
                p.insert("GRID".into(), s(grid));
                p.insert("OFFSET".into(), Value::from(*offset));
                p.insert("SNR".into(), Value::from(*snr));
                p.insert("SPEED".into(), Value::from(*speed));
                "RX.SPOT"
            }
            Record::Directed {
                by,
                cmd,
                from,
                to,
                path,
                text,
                grid,
                extra,
                dial,
                offset,
                snr,
                speed,
            } => {
                p.insert("BY".into(), by_value(by));
                p.insert("CMD".into(), s(cmd));
                p.insert("DIAL".into(), Value::from(*dial));
                p.insert("EXTRA".into(), s(extra));
                p.insert("FREQ".into(), Value::from(dial + offset));
                p.insert("FROM".into(), s(from));
                p.insert("GRID".into(), s(grid));
                p.insert("OFFSET".into(), Value::from(*offset));
                p.insert("PATH".into(), s(path));
                p.insert("SNR".into(), Value::from(*snr));
                p.insert("SPEED".into(), Value::from(*speed));
                p.insert("TEXT".into(), s(text));
                p.insert("TO".into(), s(to));
                "RX.DIRECTED"
            }
        };
        p.insert("_ID".into(), s(&(at_ms - ID_EPOCH_MS).to_string()));
        let mut m = Map::new();
        m.insert("params".into(), Value::Object(p));
        m.insert("type".into(), s(kind));
        m.insert("value".into(), s(""));
        Value::Object(m).to_string()
    }
}

/// Sends batches of JS8NET datagrams. The server's address is looked up when a batch first needs
/// it and kept (JS8Call takes the first address its lookup returns); a lookup that fails drops that
/// batch and is tried again with the next.
pub struct SpotClient {
    target: String,
    addr: Option<SocketAddr>,
}

impl SpotClient {
    /// A client for [`DEFAULT_TARGET`].
    pub fn new() -> Self {
        Self::with_target(DEFAULT_TARGET)
    }

    /// A client for another `host:port` (tests use a loopback listener).
    pub fn with_target(target: &str) -> Self {
        SpotClient {
            target: target.to_string(),
            addr: None,
        }
    }

    /// Send each datagram in `batch`, in order. Returns how many went. An empty batch looks
    /// nothing up and sends nothing.
    pub fn send(&mut self, batch: &[String]) -> std::io::Result<usize> {
        if batch.is_empty() {
            return Ok(0);
        }
        let addr = match self.addr {
            Some(a) => a,
            None => {
                let a = self.target.to_socket_addrs()?.next().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, "no address")
                })?;
                self.addr = Some(a);
                a
            }
        };
        let bind: SocketAddr = if addr.is_ipv6() {
            "[::]:0".parse().expect("literal")
        } else {
            "0.0.0.0:0".parse().expect("literal")
        };
        let socket = UdpSocket::bind(bind)?;
        let mut sent = 0;
        for d in batch {
            socket.send_to(d.as_bytes(), addr)?;
            sent += 1;
        }
        Ok(sent)
    }
}

impl Default for SpotClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by() -> By {
        By {
            call: "KD9TAW".into(),
            grid: "EN52WA".into(),
        }
    }

    /// 2026-10-15 12:00:00 UTC, so `_ID` is a value a reader can check by hand:
    /// 1_792_065_600_000 − 1_499_299_200_000.
    const AT: i64 = 1_792_065_600_000;

    #[test]
    fn a_heard_station_goes_up_as_js8call_writes_rx_spot() {
        let r = Record::Spot {
            by: by(),
            call: "KN4CRD".into(),
            grid: "EM73".into(),
            dial: 14_078_000,
            offset: 1_500,
            snr: -10,
            speed: 0,
        };
        assert_eq!(
            r.to_json(AT),
            r#"{"params":{"BY":{"CALLSIGN":"KD9TAW","GRID":"EN52WA"},"CALLSIGN":"KN4CRD","DIAL":14078000,"FREQ":14079500,"GRID":"EM73","OFFSET":1500,"SNR":-10,"SPEED":0,"_ID":"292766400000"},"type":"RX.SPOT","value":""}"#
        );
    }

    #[test]
    fn a_group_command_goes_up_as_js8call_writes_rx_directed() {
        let r = Record::Directed {
            by: by(),
            cmd: "MSG".into(),
            from: "KN4CRD".into(),
            to: "@JS8NET".into(),
            path: String::new(),
            text: "HELLO \"WORLD\"".into(),
            grid: String::new(),
            extra: String::new(),
            dial: 7_078_000,
            offset: 950,
            snr: 3,
            speed: 4,
        };
        assert_eq!(
            r.to_json(AT),
            r#"{"params":{"BY":{"CALLSIGN":"KD9TAW","GRID":"EN52WA"},"CMD":"MSG","DIAL":7078000,"EXTRA":"","FREQ":7078950,"FROM":"KN4CRD","GRID":"","OFFSET":950,"PATH":"","SNR":3,"SPEED":4,"TEXT":"HELLO \"WORLD\"","TO":"@JS8NET","_ID":"292766400000"},"type":"RX.DIRECTED","value":""}"#,
            "a quote heard over the air stays inside its string"
        );
    }

    #[test]
    fn this_station_goes_up_as_rx_local() {
        let r = Record::Local {
            call: "KD9TAW".into(),
            grid: "EN52WA".into(),
            info: "QRP".into(),
            version: "Nexus 1.19.0".into(),
        };
        assert_eq!(
            r.to_json(AT),
            r#"{"params":{"CALLSIGN":"KD9TAW","GRID":"EN52WA","INFO":"QRP","VERSION":"Nexus 1.19.0","_ID":"292766400000"},"type":"RX.LOCAL","value":""}"#
        );
    }

    #[test]
    fn a_batch_reaches_a_loopback_listener_one_datagram_per_record() {
        let listener = UdpSocket::bind("127.0.0.1:0").unwrap();
        listener
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut c = SpotClient::with_target(&listener.local_addr().unwrap().to_string());
        let batch = vec![
            Record::Local {
                call: "KD9TAW".into(),
                grid: "EN52".into(),
                info: String::new(),
                version: "Nexus".into(),
            }
            .to_json(AT),
            "second".to_string(),
        ];
        assert_eq!(c.send(&batch).unwrap(), 2);
        let mut buf = [0u8; 2048];
        for want in &batch {
            let (n, _) = listener.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..n], want.as_bytes());
        }
        assert_eq!(c.send(&[]).unwrap(), 0, "an empty batch sends nothing");
    }
}
