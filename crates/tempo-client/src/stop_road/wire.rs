//! What Stop's second road carries, as both ends read and write it: [`Up`] from the computer
//! driving the shack, [`Down`] back, each one datagram ([`crate::datagram`]) of at most 128 bytes,
//! and the format's number in the build's contract ([`contract_text`]). And the state echo's clock
//! ([`StateClock`]): when the shack took each snapshot it numbered on one connection, as the command
//! link's writer records it ([`crate::outbox`]) and the shack's road judges a heartbeat by. What the
//! shack takes, refuses and answers is its road's (src-tauri's `remote_service::lan::stop_road`);
//! this computer's end is [`crate::stop_road`].
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tempo_app::mic::{MicEnded, MicStatus};
use tempo_app::remote_control::ptt_hold::HoldEnd;

use crate::datagram;

/// The format of the road's messages: 2 since the microphone over's state carries its holes, 3
/// since a hold names the Stops pressed before its press began.
const FORMAT: u8 = 3;
/// How long a road's keys outlive its connection, for a Stop sent as the link drops or after it.
pub const AFTERLIFE: Duration = Duration::from_secs(60);
/// How old the state a heartbeat names may be and still renew presence: the stream's two seconds
/// for a picture, measured the same way, by the station's own clock.
pub const FRESH: Duration = Duration::from_millis(tempo_stream::protocol::FRESH_FRAME_MS);
/// Snapshots remembered per connection: about nineteen seconds of them, against two.
const REMEMBERED: usize = 64;

// --- What it carries -----------------------------------------------------------------------------

const STOP: u8 = 1;
const HOLD: u8 = 2;
const RELEASE: u8 = 3;
const HEARTBEAT: u8 = 4;
const STOPPED: u8 = 16;
const PRESS: u8 = 17;
const MIC: u8 = 18;
const PRESENCE: u8 = 19;

/// What a computer sends on its road.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Up {
    /// A Stop press, repeated until it is acknowledged: the same number each time, its computer's
    /// count of Stops pressed this run, itself included (the count the command link carries).
    Stop { stop: u64 },
    /// The press `press` is still held: the screen re-asserts it every 100 ms. `stops` is how many
    /// Stops its computer had pressed when the press began, the same at every hold.
    Hold { press: u64, stops: u64 },
    /// The press `press` was let go.
    Release { press: u64 },
    /// The computer's lease, and the number of the last snapshot its screen fetched.
    Heartbeat { lease: String, state: u64 },
}

/// What the shack sends back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Down {
    /// The Stop `stop` was taken.
    Stopped { stop: u64 },
    /// What became of a press: keyed, or ended and why.
    Press {
        press: u64,
        keyed: bool,
        end: Option<HoldEnd>,
    },
    /// The microphone over, as the engine last published it.
    Mic(MicStatus),
    /// Presence was renewed: this long until it lapses unless renewed again.
    Presence { left_ms: u32 },
}

impl Up {
    pub fn kind(&self) -> u8 {
        match self {
            Up::Stop { .. } => STOP,
            Up::Hold { .. } => HOLD,
            Up::Release { .. } => RELEASE,
            Up::Heartbeat { .. } => HEARTBEAT,
        }
    }

    pub fn body(&self) -> Vec<u8> {
        match self {
            Up::Stop { stop: n } | Up::Release { press: n } => n.to_be_bytes().to_vec(),
            Up::Hold { press, stops } => {
                let mut body = press.to_be_bytes().to_vec();
                body.extend_from_slice(&stops.to_be_bytes());
                body
            }
            Up::Heartbeat { lease, state } => {
                let mut body = lease.as_bytes().to_vec();
                body.extend_from_slice(&state.to_be_bytes());
                body
            }
        }
    }

    /// A body that verified, read exactly: each kind has one length, and a lease is one the
    /// station could have issued.
    pub fn read(kind: u8, body: &[u8]) -> Option<Self> {
        let number = || body.try_into().ok().map(u64::from_be_bytes);
        Some(match kind {
            STOP => Up::Stop { stop: number()? },
            HOLD if body.len() == 16 => Up::Hold {
                press: u64::from_be_bytes(body[..8].try_into().ok()?),
                stops: u64::from_be_bytes(body[8..].try_into().ok()?),
            },
            RELEASE => Up::Release { press: number()? },
            HEARTBEAT if body.len() == 36 + 8 => {
                let lease = std::str::from_utf8(&body[..36]).ok()?;
                if !tempo_stream::protocol::identifier(lease) {
                    return None;
                }
                Up::Heartbeat {
                    lease: lease.to_string(),
                    state: u64::from_be_bytes(body[36..].try_into().ok()?),
                }
            }
            _ => return None,
        })
    }
}

fn hold_end(end: Option<HoldEnd>) -> u8 {
    match end {
        None => 0,
        Some(HoldEnd::Refused) => 1,
        Some(HoldEnd::Lapsed) => 2,
        Some(HoldEnd::Released) => 3,
        Some(HoldEnd::Stopped) => 4,
    }
}

fn hold_end_of(code: u8) -> Option<Option<HoldEnd>> {
    Some(match code {
        0 => None,
        1 => Some(HoldEnd::Refused),
        2 => Some(HoldEnd::Lapsed),
        3 => Some(HoldEnd::Released),
        4 => Some(HoldEnd::Stopped),
        _ => return None,
    })
}

fn mic_ended(ended: Option<MicEnded>) -> u8 {
    match ended {
        None => 0,
        Some(MicEnded::Released) => 1,
        Some(MicEnded::Stopped) => 2,
        Some(MicEnded::AudioGap) => 3,
        Some(MicEnded::Presence) => 4,
        Some(MicEnded::Ceiling) => 5,
        Some(MicEnded::Watchdog) => 6,
        Some(MicEnded::RouteChanged) => 7,
    }
}

fn mic_ended_of(code: u8) -> Option<Option<MicEnded>> {
    Some(match code {
        0 => None,
        1 => Some(MicEnded::Released),
        2 => Some(MicEnded::Stopped),
        3 => Some(MicEnded::AudioGap),
        4 => Some(MicEnded::Presence),
        5 => Some(MicEnded::Ceiling),
        6 => Some(MicEnded::Watchdog),
        7 => Some(MicEnded::RouteChanged),
        _ => return None,
    })
}

fn flag(byte: u8) -> Option<bool> {
    match byte {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

impl Down {
    pub fn kind(&self) -> u8 {
        match self {
            Down::Stopped { .. } => STOPPED,
            Down::Press { .. } => PRESS,
            Down::Mic(_) => MIC,
            Down::Presence { .. } => PRESENCE,
        }
    }

    pub fn body(&self) -> Vec<u8> {
        match self {
            Down::Stopped { stop } => stop.to_be_bytes().to_vec(),
            Down::Press { press, keyed, end } => {
                let mut body = press.to_be_bytes().to_vec();
                body.extend_from_slice(&[u8::from(*keyed), hold_end(*end)]);
                body
            }
            Down::Mic(status) => {
                let mut body = vec![
                    u8::from(status.armed),
                    u8::from(status.keyed),
                    u8::from(status.no_power_out),
                    mic_ended(status.ended),
                ];
                body.extend_from_slice(&status.holes.to_be_bytes());
                body.extend_from_slice(&status.hole_ms.to_be_bytes());
                body
            }
            Down::Presence { left_ms } => left_ms.to_be_bytes().to_vec(),
        }
    }

    /// A body that verified, read exactly.
    pub fn read(kind: u8, body: &[u8]) -> Option<Self> {
        Some(match (kind, body.len()) {
            (STOPPED, 8) => Down::Stopped {
                stop: u64::from_be_bytes(body.try_into().ok()?),
            },
            (PRESS, 10) => Down::Press {
                press: u64::from_be_bytes(body[..8].try_into().ok()?),
                keyed: flag(body[8])?,
                end: hold_end_of(body[9])?,
            },
            (MIC, 12) => Down::Mic(MicStatus {
                armed: flag(body[0])?,
                keyed: flag(body[1])?,
                no_power_out: flag(body[2])?,
                ended: mic_ended_of(body[3])?,
                holes: u32::from_be_bytes(body[4..8].try_into().ok()?),
                hole_ms: u32::from_be_bytes(body[8..12].try_into().ok()?),
            }),
            (PRESENCE, 4) => Down::Presence {
                left_ms: u32::from_be_bytes(body.try_into().ok()?),
            },
            _ => return None,
        })
    }
}

/// The road's format, for the build's contract ([`crate::wire::contract`]): two builds that read
/// these datagrams differently are not the same build.
pub fn contract_text() -> String {
    format!(
        "stop road {} {} format {FORMAT} up stop={STOP} hold={HOLD}(press,stops) \
         release={RELEASE} heartbeat={HEARTBEAT} down stopped={STOPPED} press={PRESS} \
         mic={MIC}(armed,keyed,noPowerOut,ended,holes,holeMs) presence={PRESENCE}\n",
        datagram::VERSION,
        String::from_utf8_lossy(datagram::STOP_LABEL),
    )
}

// --- The state echo's clock ----------------------------------------------------------------------

/// When the shack took each snapshot it numbered on one connection: the state echo's clock, as the
/// stream's video clock is the picture's (`tempo_stream::frame_clock`). The command link records
/// each number as it gives it ([`crate::outbox`]), with the moment the shack began reading that
/// state, so a state read slowly reads as older, never newer.
///
/// ⛔ AN ECHO MUST NAME A SNAPSHOT THE SHACK SENT. A number it never gave, or one it has forgotten,
/// is not fresh whatever its value, so a computer cannot keep presence alive by naming a number it
/// guessed, and a garbled one reads as stale.
#[derive(Default)]
pub struct StateClock {
    taken: Mutex<VecDeque<(u64, Instant)>>,
}

impl StateClock {
    /// The snapshot numbered `number` was taken at `taken`.
    pub fn sent(&self, number: u64, taken: Instant) {
        let mut sent = self
            .taken
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if sent.len() >= REMEMBERED {
            sent.pop_front();
        }
        sent.push_back((number, taken));
    }

    /// Is the snapshot numbered `number` one this connection sent, taken no more than [`FRESH`]
    /// before `now`? Asked by the road's thread, which only tries the lock: a moment's contention
    /// reads as not fresh, and the next heartbeat asks again.
    pub fn fresh(&self, number: u64, now: Instant) -> bool {
        self.freshness(number, now).unwrap_or(false)
    }

    /// [`StateClock::fresh`], or `None` while the lock is held this moment: the road's thread never
    /// waits, and a press it cannot judge now is asked about again at its next hold, never refused.
    pub fn freshness(&self, number: u64, now: Instant) -> Option<bool> {
        match self.taken.try_lock() {
            Ok(sent) => Some(fresh_in(&sent, number, now)),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                Some(fresh_in(&poisoned.into_inner(), number, now))
            }
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    /// [`StateClock::fresh`], waiting for the lock: for the command lane, which may wait.
    pub fn fresh_waiting(&self, number: u64, now: Instant) -> bool {
        let sent = self
            .taken
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        fresh_in(&sent, number, now)
    }
}

fn fresh_in(sent: &VecDeque<(u64, Instant)>, number: u64, now: Instant) -> bool {
    sent.iter()
        .rev()
        .find(|(sent, _)| *sent == number)
        .is_some_and(|(_, taken)| now.saturating_duration_since(*taken) <= FRESH)
}
