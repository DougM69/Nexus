//! The datagram layer of the native client's roads beside the command link: Stop's second road
//! (src-tauri's `remote_service::lan::stop_road` at the shack, [`crate::stop_road`] on the other
//! computer) and the audio road (`remote_service::lan::audio_road`, [`crate::audio_road`]), both
//! ends reading and writing it with this module. A datagram is small, sent once, and either
//! verifies under the road's keys or is dropped before anything reads its body.
//!
//! ## The keys
//!
//! A road's keys come from the command link's own TLS 1.3 session, through its exporter (RFC 8446
//! §7.5), with the road's own label ([`STOP_LABEL`], [`AUDIO_LABEL`]) and the session id the shack
//! stamped in its welcome as the context, so no two roads share a key. One export ([`EXPORTED`]
//! bytes) gives each direction a key and a salt:
//! "up", from the computer to the shack, and "down", back. Both ends derive the same bytes from the
//! session they share, and nobody else can: they exist only while the link that made them is open,
//! and the shack keeps the Stop road's for a minute past its close, for a late Stop. They are never
//! written to a file, logged, shown or sent, and nothing here can print them (no `Debug`).
//!
//! ## A datagram
//!
//! | Bytes | What |
//! |---|---|
//! | 0 | The version, [`VERSION`] |
//! | 1 | The kind of message (each road names its own) |
//! | 2..10 | The road's tag, from the welcome: which computer's road it is |
//! | 10..18 | The sequence, a `u64` big-endian, from 1 and never repeated under one key |
//! | 18.. | The body, AES-128-GCM, and its 16-byte tag |
//!
//! The clear 18 bytes are the AEAD's associated data, so none of them can be changed in transit.
//! The nonce is the direction's salt with the sequence in its last eight bytes (SRTP's and TLS 1.3's
//! construction), so no nonce repeats under one key while no sequence does. At most [`MAX`] bytes
//! on Stop's road; the audio road carries one Opus frame a datagram, whose receive frames are
//! variable in size, and has its own maximum ([`AUDIO_MAX`]).
//!
//! ## What is dropped, before the body is read
//!
//! Too short or too long, another version, a tag of no road here, sequence 0, a sequence the replay
//! window has seen or has passed, and a body whose tag does not verify. Only a datagram that
//! verified hands its body on ([`Opener::open`]), and only then does the window take its sequence.
//!
//! ## Replays
//!
//! A 64-wide window per road and direction, as RFC 4303 §3.4.3 keeps one: a sequence above the
//! highest seen moves the window; one inside it is taken once; one older than the window is
//! dropped. A sender never needs one taken late: everything a road carries is sent again.
use std::sync::atomic::{AtomicU64, Ordering};

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_128_GCM, NONCE_LEN};

/// The datagram format this build speaks.
pub const VERSION: u8 = 1;
/// The clear header: the version, the kind, the tag and the sequence.
pub const HEADER: usize = 18;
/// The AEAD's tag, after the body.
const SEAL: usize = 16;
/// The largest datagram either way on Stop's road, so none is ever split or waits behind another.
pub const MAX: usize = 128;
/// The largest datagram either way on the audio road: one 20 ms receive frame, whose Opus packet is
/// variable in size (the receive encoder's ceiling is 400 bytes), with its header, numbers and
/// seal. Far under any network's packet size, so it is never split either.
pub const AUDIO_MAX: usize = 512;
/// The exporter's label for Stop's second road: ours, beginning `EXPORTER` as RFC 5705 §4 asks, so
/// it cannot collide with a label of TLS's own.
pub const STOP_LABEL: &[u8] = b"EXPORTER-nexus-stop-road";
/// The exporter's label for the audio road: its keys are never Stop's.
pub const AUDIO_LABEL: &[u8] = b"EXPORTER-nexus-audio-road";
/// The labels of a connection's internet roads (`crate::ice`): their own, so that even a
/// connection with a road of each kind could never seal two roads' datagrams under one key.
pub const INTERNET_STOP_LABEL: &[u8] = b"EXPORTER-nexus-internet-stop-road";
pub const INTERNET_AUDIO_LABEL: &[u8] = b"EXPORTER-nexus-internet-audio-road";
/// One export: a key and a salt for each direction.
pub const EXPORTED: usize = 2 * (KEY + NONCE_LEN);
const KEY: usize = 16;
/// The replay window's width.
const WINDOW: u64 = 64;

/// A road's tag: eight bytes the shack picks for each connection's road and names in its welcome.
pub type Tag = [u8; 8];

/// Which end of a road this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Shack,
    Computer,
}

/// One direction's sending half: its key, its salt, the road's tag and the next sequence. Shared
/// by every thread that sends on the road, so the sequence is an atomic and never handed out twice.
pub struct Sealer {
    key: LessSafeKey,
    salt: [u8; NONCE_LEN],
    tag: Tag,
    next: AtomicU64,
    /// The road's largest datagram.
    max: usize,
}

/// One direction's receiving half: its key, its salt, the road's tag and the replay window.
pub struct Opener {
    key: LessSafeKey,
    salt: [u8; NONCE_LEN],
    tag: Tag,
    window: Window,
    /// The road's largest datagram.
    max: usize,
}

/// Both directions of one road, as one end holds them.
pub struct Keys {
    pub send: Sealer,
    pub receive: Opener,
}

impl Keys {
    /// The road `tag`'s keys at `end`, from the exporter's `exported` bytes: up's key and salt, then
    /// down's. The shack sends down and receives up; the computer the other way. Stop's road: at
    /// most [`MAX`] bytes a datagram.
    pub fn new(exported: &[u8; EXPORTED], tag: Tag, end: End) -> Option<Self> {
        Self::sized(exported, tag, end, MAX)
    }

    /// The same for a road whose datagrams may be up to `max` bytes (the audio road's [`AUDIO_MAX`]).
    pub fn sized(exported: &[u8; EXPORTED], tag: Tag, end: End, max: usize) -> Option<Self> {
        let (up, down) = exported.split_at(KEY + NONCE_LEN);
        let (send, receive) = match end {
            End::Shack => (down, up),
            End::Computer => (up, down),
        };
        Some(Self {
            send: Sealer {
                key: key(send)?,
                salt: salt(send)?,
                tag,
                next: AtomicU64::new(1),
                max,
            },
            receive: Opener {
                key: key(receive)?,
                salt: salt(receive)?,
                tag,
                window: Window::default(),
                max,
            },
        })
    }
}

fn key(half: &[u8]) -> Option<LessSafeKey> {
    UnboundKey::new(&AES_128_GCM, half.get(..KEY)?)
        .ok()
        .map(LessSafeKey::new)
}

fn salt(half: &[u8]) -> Option<[u8; NONCE_LEN]> {
    half.get(KEY..)?.try_into().ok()
}

/// The nonce for `sequence` under `salt`.
fn nonce(salt: &[u8; NONCE_LEN], sequence: u64) -> Nonce {
    let mut nonce = *salt;
    for (byte, seq) in nonce[NONCE_LEN - 8..]
        .iter_mut()
        .zip(sequence.to_be_bytes())
    {
        *byte ^= seq;
    }
    Nonce::assume_unique_for_key(nonce)
}

/// The clear header's parts: `(version, kind, tag, sequence)`, or `None` for a datagram that is too
/// short to be one, or longer than its road's `max`.
fn header(datagram: &[u8], max: usize) -> Option<(u8, u8, Tag, u64)> {
    if datagram.len() < HEADER + SEAL || datagram.len() > max {
        return None;
    }
    let tag: Tag = datagram[2..10].try_into().ok()?;
    let sequence = u64::from_be_bytes(datagram[10..HEADER].try_into().ok()?);
    Some((datagram[0], datagram[1], tag, sequence))
}

/// The tag a datagram names, read off its clear header alone, for the shack to find the road whose
/// keys may open it. `None` for anything that is not shaped as a datagram of this version on a road
/// whose datagrams are at most `max` bytes.
pub fn tag_of(datagram: &[u8], max: usize) -> Option<Tag> {
    header(datagram, max)
        .filter(|(version, ..)| *version == VERSION)
        .map(|(_, _, tag, _)| tag)
}

impl Sealer {
    /// A datagram of `kind` carrying `body`, sealed under the next sequence. `None` once the
    /// sequence is spent (it never wraps into one already used) or for a body that would not fit.
    pub fn seal(&self, kind: u8, body: &[u8]) -> Option<Vec<u8>> {
        if HEADER + body.len() + SEAL > self.max {
            return None;
        }
        let sequence = self
            .next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
            .ok()?;
        let mut datagram = Vec::with_capacity(HEADER + body.len() + SEAL);
        datagram.push(VERSION);
        datagram.push(kind);
        datagram.extend_from_slice(&self.tag);
        datagram.extend_from_slice(&sequence.to_be_bytes());
        let mut sealed = body.to_vec();
        self.key
            .seal_in_place_append_tag(
                nonce(&self.salt, sequence),
                Aad::from(&datagram[..HEADER]),
                &mut sealed,
            )
            .ok()?;
        datagram.extend_from_slice(&sealed);
        Some(datagram)
    }
}

impl Opener {
    /// `datagram`'s kind and body, once it has verified under this road's key and its sequence is
    /// one the window has not seen; `None`, with nothing read past the clear header, otherwise.
    pub fn open(&mut self, datagram: &[u8]) -> Option<(u8, Vec<u8>)> {
        let (version, kind, tag, sequence) = header(datagram, self.max)?;
        if version != VERSION || tag != self.tag || !self.window.fresh(sequence) {
            return None;
        }
        let mut sealed = datagram[HEADER..].to_vec();
        let body = self
            .key
            .open_in_place(
                nonce(&self.salt, sequence),
                Aad::from(&datagram[..HEADER]),
                &mut sealed,
            )
            .ok()?
            .to_vec();
        self.window.take(sequence);
        Some((kind, body))
    }
}

/// A 64-wide replay window: the highest sequence taken, and which of the 63 below it were.
#[derive(Default)]
struct Window {
    highest: u64,
    /// Bit `n`: `highest - n` was taken.
    seen: u64,
}

impl Window {
    /// May `sequence` be taken: never 0, above the highest, or inside the window and not taken.
    fn fresh(&self, sequence: u64) -> bool {
        if sequence == 0 {
            return false;
        }
        if sequence > self.highest {
            return true;
        }
        let behind = self.highest - sequence;
        behind < WINDOW && self.seen & (1 << behind) == 0
    }

    /// Take `sequence`, which [`Window::fresh`] allowed.
    fn take(&mut self, sequence: u64) {
        if sequence > self.highest {
            let ahead = sequence - self.highest;
            self.seen = if ahead >= WINDOW {
                0
            } else {
                self.seen << ahead
            };
            self.seen |= 1;
            self.highest = sequence;
        } else {
            self.seen |= 1 << (self.highest - sequence);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Key material from nowhere: what an exporter would give, for a test.
    fn exported(seed: u8) -> [u8; EXPORTED] {
        std::array::from_fn(|n| seed.wrapping_mul(31).wrapping_add(n as u8))
    }

    const TAG: Tag = [7; 8];

    fn ends(seed: u8) -> (Keys, Keys) {
        (
            Keys::new(&exported(seed), TAG, End::Computer).unwrap(),
            Keys::new(&exported(seed), TAG, End::Shack).unwrap(),
        )
    }

    /// ★ A datagram sealed at one end opens at the other, both ways, with its kind and body, under
    /// the same export. CONTROL: the clear header says what it is, and the body is not in the clear.
    #[test]
    fn a_datagram_crosses_both_ways_under_the_same_export() {
        let (mut computer, mut shack) = ends(1);
        let up = computer.send.seal(4, b"heartbeat body").unwrap();
        assert_eq!(&up[..2], &[VERSION, 4]);
        assert_eq!(&up[2..10], &TAG);
        assert_eq!(u64::from_be_bytes(up[10..18].try_into().unwrap()), 1);
        assert!(
            !up.windows(9).any(|w| w == b"heartbeat"),
            "the body is sealed"
        );
        assert_eq!(
            shack.receive.open(&up),
            Some((4, b"heartbeat body".to_vec()))
        );
        let down = shack.send.seal(16, &9u64.to_be_bytes()).unwrap();
        assert_eq!(
            computer.receive.open(&down),
            Some((16, 9u64.to_be_bytes().to_vec()))
        );
    }

    /// ★ Nothing that does not verify is opened: another session's keys, a datagram sent back to
    /// the end that sealed it (the other direction's key), a changed byte anywhere (header, body or
    /// tag), another road's tag, another version, sequence 0, and a datagram too short or too long.
    /// CONTROL: the unchanged datagram opens, after every refusal.
    #[test]
    fn a_datagram_that_does_not_verify_is_dropped() {
        let (mut computer, mut shack) = ends(2);
        let (_, mut stranger) = ends(3);
        let up = computer.send.seal(1, &5u64.to_be_bytes()).unwrap();
        assert_eq!(stranger.receive.open(&up), None, "another session's keys");
        assert_eq!(computer.receive.open(&up), None, "its own datagram back");
        for at in 0..up.len() {
            let mut changed = up.clone();
            changed[at] ^= 0x01;
            assert_eq!(shack.receive.open(&changed), None, "byte {at} changed");
        }
        let mut other_road = Keys::new(&exported(2), [8; 8], End::Shack).unwrap();
        assert_eq!(other_road.receive.open(&up), None, "another road's tag");
        assert_eq!(
            shack.receive.open(&up[..HEADER + SEAL - 1]),
            None,
            "too short"
        );
        let mut long = up.clone();
        long.resize(MAX + 1, 0);
        assert_eq!(shack.receive.open(&long), None, "too long");
        assert_eq!(
            computer.send.seal(1, &[0; MAX - HEADER - SEAL + 1]),
            None,
            "a body that would not fit is never sealed"
        );
        assert_eq!(
            shack.receive.open(&up),
            Some((1, 5u64.to_be_bytes().to_vec()))
        );
    }

    /// ★ Replays: a datagram is taken once; one sent earlier and delayed is taken late while it is
    /// inside the 64-wide window; one older than the window, or seen already, is dropped.
    /// CONTROL: the window moves with the highest sequence taken.
    #[test]
    fn each_sequence_is_taken_once_inside_the_window() {
        let (computer, mut shack) = ends(4);
        let sealed: Vec<Vec<u8>> = (0..100)
            .map(|n| computer.send.seal(1, &[n as u8; 8]).unwrap())
            .collect();
        assert!(shack.receive.open(&sealed[0]).is_some());
        assert_eq!(shack.receive.open(&sealed[0]), None, "a replay");
        assert!(shack.receive.open(&sealed[70]).is_some(), "sequence 71");
        assert_eq!(
            shack.receive.open(&sealed[6]),
            None,
            "sequence 7 is 64 behind 71"
        );
        assert!(
            shack.receive.open(&sealed[7]).is_some(),
            "sequence 8, 63 behind, never seen"
        );
        assert_eq!(shack.receive.open(&sealed[7]), None, "and only once");
        assert!(shack.receive.open(&sealed[99]).is_some(), "sequence 100");
        assert_eq!(
            shack.receive.open(&sealed[70]),
            None,
            "seen before the window moved"
        );
        assert!(
            shack.receive.open(&sealed[69]).is_some(),
            "sequence 70, never seen"
        );
    }

    /// ★ Each road has its own largest datagram: one past Stop's 128 bytes is sealed and opened under
    /// the audio road's keys, and refused on Stop's road before its body is read, whatever its tag
    /// says. CONTROL: past the audio road's own maximum it is refused there too, and the two roads'
    /// labels differ, so they never share a key.
    #[test]
    fn each_road_has_its_own_largest_datagram() {
        let computer = Keys::sized(&exported(5), TAG, End::Computer, AUDIO_MAX).unwrap();
        let mut audio = Keys::sized(&exported(5), TAG, End::Shack, AUDIO_MAX).unwrap();
        let mut stop = Keys::new(&exported(5), TAG, End::Shack).unwrap();
        let body = vec![0x42; 300];
        let big = computer
            .send
            .seal(2, &body)
            .expect("an audio-sized body is sealed");
        assert!(big.len() > MAX);
        assert_eq!(tag_of(&big, MAX), None, "too long for Stop's road");
        assert_eq!(tag_of(&big, AUDIO_MAX), Some(TAG));
        assert_eq!(
            stop.receive.open(&big),
            None,
            "Stop's road takes nothing that long"
        );
        assert_eq!(audio.receive.open(&big), Some((2, body)));
        assert_eq!(
            computer
                .send
                .seal(2, &vec![0; AUDIO_MAX - HEADER - SEAL + 1]),
            None
        );
        assert_ne!(STOP_LABEL, AUDIO_LABEL);
    }

    /// The window's own edges, against a plain record of what was taken.
    #[test]
    fn the_window_refuses_exactly_what_it_has_seen_or_passed() {
        let mut window = Window::default();
        let mut taken = std::collections::BTreeSet::new();
        // Each repeat is one the window has seen; each low number one it has passed.
        for sequence in [
            1u64, 3, 2, 3, 70, 7, 6, 70, 7, 69, 200, 137, 136, 199, 137, 201, 200, 1000, 937, 936,
            1000,
        ] {
            let highest = taken.iter().next_back().copied().unwrap_or(0);
            let expected = !taken.contains(&sequence) && sequence + WINDOW > highest;
            assert_eq!(window.fresh(sequence), expected, "{sequence}");
            if expected {
                window.take(sequence);
                taken.insert(sequence);
            }
        }
        assert!(!window.fresh(0));
    }

    /// The nonce is the salt with the sequence in its last eight bytes: two sequences never share
    /// one under a key. CONTROL: the salt's first four bytes are as they were.
    #[test]
    fn each_sequence_has_its_own_nonce() {
        let salt = [0xa5; NONCE_LEN];
        let one = *nonce(&salt, 1).as_ref();
        let two = *nonce(&salt, 2).as_ref();
        assert_ne!(one, two);
        assert_eq!(one[..4], salt[..4]);
        assert_eq!(one[NONCE_LEN - 1], 0xa5 ^ 1);
    }
}
