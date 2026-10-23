//! What the audio road carries, as both ends read and write it: [`Up`] from the computer driving
//! the shack (`listen`, and its microphone's frames while its press is live), [`Down`] back (the
//! station's receive audio), one 20 ms frame a datagram ([`crate::datagram`]) of at most
//! [`AUDIO_MAX`] bytes, and the format's number in the build's contract ([`contract_text`]). What
//! the shack sends and takes is its road's (src-tauri's `remote_service::lan::audio_road`); this
//! computer's end is [`crate::audio_road`].
use crate::datagram::{self, AUDIO_MAX, HEADER};

/// The road's message format, in the build's contract.
const FORMAT: u8 = 1;
/// The AEAD's tag after the body.
const SEAL: usize = 16;
/// The longest body a datagram on this road carries.
const BODY: usize = AUDIO_MAX - HEADER - SEAL;

const LISTEN: u8 = 1;
const MIC: u8 = 2;
const RX: u8 = 16;

/// What a computer sends on its audio road.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Up {
    /// Send the station's audio here: the computer's lease.
    Listen { lease: String },
    /// One frame of the computer's microphone: its press, its number in that press, and the packet.
    Mic {
        press: u64,
        frame: u32,
        packet: Vec<u8>,
    },
}

/// What the shack sends back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Down {
    /// One frame of receive audio: the capture's epoch, its number in that epoch, and the packet.
    Rx {
        epoch: u64,
        seq: u32,
        packet: Vec<u8>,
    },
}

impl Up {
    pub fn kind(&self) -> u8 {
        match self {
            Up::Listen { .. } => LISTEN,
            Up::Mic { .. } => MIC,
        }
    }

    pub fn body(&self) -> Vec<u8> {
        match self {
            Up::Listen { lease } => lease.as_bytes().to_vec(),
            Up::Mic {
                press,
                frame,
                packet,
            } => {
                let mut body = press.to_be_bytes().to_vec();
                body.extend_from_slice(&frame.to_be_bytes());
                body.extend_from_slice(packet);
                body
            }
        }
    }

    /// A body that verified, read exactly: a lease the station could have issued, or a frame with a
    /// packet in it.
    pub fn read(kind: u8, body: &[u8]) -> Option<Self> {
        Some(match kind {
            LISTEN if body.len() == 36 => {
                let lease = std::str::from_utf8(body).ok()?;
                if !tempo_stream::protocol::identifier(lease) {
                    return None;
                }
                Up::Listen {
                    lease: lease.to_string(),
                }
            }
            MIC if (13..=BODY).contains(&body.len()) => Up::Mic {
                press: u64::from_be_bytes(body[..8].try_into().ok()?),
                frame: u32::from_be_bytes(body[8..12].try_into().ok()?),
                packet: body[12..].to_vec(),
            },
            _ => return None,
        })
    }
}

impl Down {
    pub fn kind(&self) -> u8 {
        match self {
            Down::Rx { .. } => RX,
        }
    }

    pub fn body(&self) -> Vec<u8> {
        match self {
            Down::Rx { epoch, seq, packet } => {
                let mut body = epoch.to_be_bytes().to_vec();
                body.extend_from_slice(&seq.to_be_bytes());
                body.extend_from_slice(packet);
                body
            }
        }
    }

    /// A body that verified, read exactly.
    pub fn read(kind: u8, body: &[u8]) -> Option<Self> {
        match kind {
            RX if (13..=BODY).contains(&body.len()) => Some(Down::Rx {
                epoch: u64::from_be_bytes(body[..8].try_into().ok()?),
                seq: u32::from_be_bytes(body[8..12].try_into().ok()?),
                packet: body[12..].to_vec(),
            }),
            _ => None,
        }
    }
}

/// The road's format, for the build's contract ([`crate::wire::contract`]).
pub fn contract_text() -> String {
    format!(
        "audio road {} {} format {FORMAT} max {AUDIO_MAX} up listen={LISTEN}(lease) \
         mic={MIC}(press,frame,opus) down rx={RX}(epoch,seq,opus)\n",
        datagram::VERSION,
        String::from_utf8_lossy(datagram::AUDIO_LABEL),
    )
}
