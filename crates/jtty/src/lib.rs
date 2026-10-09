//! Safe Rust wrapper over `libtempo`'s JTTY modem.
//!
//! JTTY is WSJT-X's untimed keyboard mode: 4-GFSK at 31.25 baud, about 125 Hz
//! wide, sent as 1.888 s frames of 59 symbols (13 sync, 46 code). Each frame
//! carries one 34-bit atom (a call action, a structured exchange, or five
//! characters of text) under a CRC-12 and a K=10 tail-biting convolutional
//! code; a message is 1 to 16 frames. There is no slot clock: the receiver
//! searches the audio continuously, a quarter frame at a time, and reports each
//! message as it grows.
//!
//! ⚠️ **Not offered in any release.** The modem comes from a WSJT-X pre-release
//! (3.3.0-beta1; see NOTICE), and this crate is the FFI only. Nothing in the app
//! calls it, and nothing here keys a transmitter: [`gen_wave`] returns audio
//! samples and stops there.
//!
//! Wraps Nexus's `jtty_cabi.f90` ([`encode`], [`encode_atoms`], [`gen_wave`],
//! [`pack`] and [`unpack`], and the update records) and upstream's own receive API,
//! `jtty_receive_context.f90`'s `jtty_rx_*`, which a [`Receiver`] holds one
//! handle of.
//!
//! # Thread safety
//! Not thread-safe; every entry serializes behind
//! [`tempo_fast_sys::MODEM_LOCK`]. Each receive handle keeps its own reception
//! state, which upstream swaps in around every call, but the decoder's DSP
//! scratch and caches are process-wide (`modem-state-manifest.toml`, GROUP O),
//! so receivers take turns, one call at a time.

use std::os::raw::{c_char, c_int};

use tempo_fast_sys::{modem_lock, JttyUpdateT, JTTY_ERR_ARGS, JTTY_ERR_ENCODE};

pub use tempo_fast_sys::{
    JttySourceAtomC as AtomDescriptor, JTTY_FRAME_SYMBOLS as FRAME_SYMBOLS,
    JTTY_MAX_FRAMES as MAX_FRAMES, JTTY_MAX_TONES as MAX_TONES, JTTY_NSPS as NSPS,
    JTTY_TEXT_LEN as TEXT_LEN,
};

/// WSJT-X-family audio sample rate (Hz).
pub const SAMPLE_RATE: f32 = 12_000.0;

/// JTTY baud rate and tone spacing (h=1 GFSK): 12000/384 = 31.25.
pub const BAUD: f32 = SAMPLE_RATE / NSPS as f32;

/// Samples in one frame at 12 kHz: 59 symbols, 1.888 s.
pub const FRAME_SAMPLES: usize = FRAME_SYMBOLS * NSPS;

/// The exchange profile the encoder packs structured exchanges for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// No contest exchange expected.
    Unknown = 0,
    /// ARRL Field Day: class and section.
    FieldDay = 1,
    /// ARRL RTTY Roundup: report and serial number, or state/province.
    RttyRoundup = 2,
}

/// A message ready for [`gen_wave`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    /// Channel tones, 0..=3, a whole number of frames.
    pub tones: Vec<i32>,
    /// The text as the encoder normalised it (upper case, single spaces,
    /// characters outside the JTTY alphabet as `#`): what the far end decodes.
    pub sent: String,
}

impl Encoded {
    /// Frames on the air: 1..=16.
    pub fn frames(&self) -> usize {
        self.tones.len() / FRAME_SYMBOLS
    }
}

/// Why [`encode`] produced nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// Nothing to send once the text is normalised.
    Empty,
    /// Longer than [`TEXT_LEN`] bytes.
    TooLong,
    /// The modem cannot encode it (more frames than [`MAX_FRAMES`], or an
    /// exchange the profile rejects). `sent` is the normalised text it was given.
    NotEncodable { sent: String },
}

/// Encode message text into channel tones, normalising it the way WSJT-X does.
///
/// The text goes to the modem as bytes; a byte outside the JTTY alphabet is
/// sent as `#`.
pub fn encode(text: &str, profile: Profile) -> Result<Encoded, EncodeError> {
    let mut tones: Vec<c_int> = vec![0; MAX_TONES];
    let mut sent = [0u8; TEXT_LEN + 1];
    let len = c_int::try_from(text.len()).unwrap_or(c_int::MAX);
    let n = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::jtty_encode_text(
                text.as_ptr().cast::<c_char>(),
                len,
                profile as c_int,
                tones.as_mut_ptr(),
                MAX_TONES as c_int,
                sent.as_mut_ptr().cast::<c_char>(),
                sent.len() as c_int,
            )
        }
    };
    let sent = nul_terminated(&sent);
    match n {
        n if n > 0 => {
            tones.truncate(n as usize);
            Ok(Encoded { tones, sent })
        }
        0 => Err(EncodeError::Empty),
        // The only argument this function can get wrong is the length: both
        // buffers are sized for the largest message.
        JTTY_ERR_ARGS => Err(EncodeError::TooLong),
        JTTY_ERR_ENCODE => Err(EncodeError::NotEncodable { sent }),
        _ => Err(EncodeError::NotEncodable { sent }),
    }
}

/// A message as its frames' source words: what [`encode`] sends, before the
/// CRC and the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    /// One 34-bit word per frame. Source bit 1 is the most significant of the
    /// 34, bit 33 is reserved (zero) and bit 34, the least significant, is
    /// end-of-message: the form upstream's spec publishes its vectors in.
    pub frames: Vec<u64>,
    /// The text as the encoder normalised it, as [`Encoded::sent`].
    pub sent: String,
}

/// Pack message text into the source words of the frames [`encode`] puts on
/// the air. `last` sets end-of-message on the final frame, as a whole message
/// does; text sent while it is still being typed leaves it clear.
pub fn pack(text: &str, profile: Profile, last: bool) -> Result<Packed, EncodeError> {
    let mut frames = [0i64; MAX_FRAMES];
    let mut sent = [0u8; TEXT_LEN + 1];
    let len = c_int::try_from(text.len()).unwrap_or(c_int::MAX);
    let n = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::jtty_pack_text(
                text.as_ptr().cast::<c_char>(),
                len,
                profile as c_int,
                c_int::from(last),
                frames.as_mut_ptr(),
                MAX_FRAMES as c_int,
                sent.as_mut_ptr().cast::<c_char>(),
                sent.len() as c_int,
            )
        }
    };
    let sent = nul_terminated(&sent);
    match n {
        n if n > 0 => Ok(Packed {
            frames: frames.iter().take(n as usize).map(|&w| w as u64).collect(),
            sent,
        }),
        0 => Err(EncodeError::Empty),
        // As in `encode`: the length is the only argument this can get wrong.
        JTTY_ERR_ARGS => Err(EncodeError::TooLong),
        JTTY_ERR_ENCODE => Err(EncodeError::NotEncodable { sent }),
        _ => Err(EncodeError::NotEncodable { sent }),
    }
}

/// A message read back out of source words by [`unpack`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpacked {
    /// The text as the receiver shows it.
    pub text: String,
    /// The last word carries end-of-message.
    pub eom: bool,
}

/// Why [`unpack`] did not read a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnpackError {
    /// Not 1..=16 words, or a word wider than 34 bits.
    BadFrames,
    /// One or more words the receiver would discard (a reserved bit set, an
    /// unassigned form, a field out of range); `text` is what the rest say.
    InvalidWord { text: String },
}

/// Render source words (as [`Packed::frames`]) the way the receiver shows the
/// message, checking each word as the decoder checks a frame after its CRC.
pub fn unpack(frames: &[u64]) -> Result<Unpacked, UnpackError> {
    if frames.is_empty() || frames.len() > MAX_FRAMES || frames.iter().any(|&w| w >> 34 != 0) {
        return Err(UnpackError::BadFrames);
    }
    let words: Vec<i64> = frames.iter().map(|&w| w as i64).collect();
    let mut text = [0u8; TEXT_LEN + 1];
    let mut eom: c_int = 0;
    let status = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::jtty_unpack_frames(
                words.as_ptr(),
                words.len() as c_int,
                text.as_mut_ptr().cast::<c_char>(),
                text.len() as c_int,
                &mut eom,
            )
        }
    };
    let text = nul_terminated(&text);
    match status {
        0 => Ok(Unpacked {
            text,
            eom: eom == 1,
        }),
        1 => Err(UnpackError::InvalidWord { text }),
        _ => Err(UnpackError::BadFrames),
    }
}

/// Build one structured-atom descriptor. Returns `None` if `text` is longer
/// than the 8 bytes upstream's descriptor holds; the encoder validates the rest.
pub fn atom(kind: i8, subtype: i8, role: i8, value: i32, text: &str) -> Option<AtomDescriptor> {
    if text.len() > 8 {
        return None;
    }
    let mut a = AtomDescriptor {
        kind,
        subtype,
        role,
        reserved: 0,
        value,
        text: [0; 9],
    };
    a.text[..text.len()].copy_from_slice(text.as_bytes());
    Some(a)
}

/// Why [`encode_atoms`] produced nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomError {
    /// A descriptor upstream rejects, or not 1..=16 of them.
    InvalidDescriptor,
    /// A class/section atom naming no ARRL/RAC section.
    UnknownSection,
}

/// Encode structured atoms, one frame each: upstream's own C entry
/// (`genjtty_atoms_c`), which WSJT-X's macros use.
pub fn encode_atoms(atoms: &[AtomDescriptor]) -> Result<Vec<i32>, AtomError> {
    let mut tones: Vec<c_int> = vec![0; MAX_TONES];
    let mut status: c_int = 1;
    let natoms = c_int::try_from(atoms.len()).unwrap_or(c_int::MAX);
    let n = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::jtty_encode_atoms(
                atoms.as_ptr(),
                natoms,
                tones.as_mut_ptr(),
                MAX_TONES as c_int,
                &mut status,
            )
        }
    };
    if n > 0 {
        tones.truncate(n as usize);
        return Ok(tones);
    }
    Err(match status {
        2 => AtomError::UnknownSection,
        _ => AtomError::InvalidDescriptor,
    })
}

/// Channel tones → real audio at 12 kHz, amplitude 1: `tones.len() * NSPS`
/// samples, ramped over the first and last eighth of a symbol and with no
/// silence either side. `f0` is the audio frequency of tone 0; the signal
/// spans about `f0 .. f0 + 4 * BAUD`.
///
/// Returns `None` unless `tones` is whole frames, at most [`MAX_TONES`], of
/// tones 0..=3.
pub fn gen_wave(tones: &[i32], f0: f32) -> Option<Vec<f32>> {
    if tones.len() > MAX_TONES {
        return None;
    }
    let mut wave = vec![0f32; tones.len() * NSPS];
    let n = {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::jtty_gen_wave(
                tones.as_ptr(),
                tones.len() as c_int,
                f0,
                wave.as_mut_ptr(),
                wave.len() as c_int,
            )
        }
    };
    (n > 0 && n as usize == wave.len()).then_some(wave)
}

/// Where a [`Receiver`] looks, with upstream's meaning: `center_hz` ±
/// `tolerance_hz` is always searched (WSJT-X's RX frequency and FTol), and two
/// fixed 300 Hz channels centred on 1350 Hz and 1650 Hz are searched where they
/// fall inside `low_hz..=high_hz` (WSJT-X's waterfall range).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchBand {
    pub low_hz: i32,
    pub high_hz: i32,
    pub center_hz: f32,
    pub tolerance_hz: f32,
}

/// How a message ended, as [`Update::terminal`] reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    /// More frames may follow.
    Growing,
    /// The frame carrying the end-of-message flag arrived.
    Complete,
    /// No further frame arrived in time.
    Expired,
    /// The reception was ended while the message was open.
    ReceptionEnded,
    /// A value upstream does not produce today.
    Unknown(i32),
}

impl Terminal {
    fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Terminal::Growing,
            1 => Terminal::Complete,
            2 => Terminal::Expired,
            3 => Terminal::ReceptionEnded,
            other => Terminal::Unknown(other),
        }
    }
}

/// One step in a message's life: its text so far and how it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    /// Stable across one message's updates; distinct across messages and
    /// receivers.
    pub message_id: i64,
    /// The text so far.
    pub text: String,
    /// Audio frequency, Hz.
    pub freq_hz: f32,
    /// The message's first frame, seconds on the caller's sample timeline.
    pub start_s: f64,
    /// Its latest frame, same timeline.
    pub latest_s: f64,
    pub terminal: Terminal,
    /// SNR of the first frame, dB.
    pub snr_db: i32,
}

/// Why [`Receiver::process`] did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    /// No reception is running: [`Receiver::begin`] was not called, or the
    /// reception was ended.
    NotRunning,
    /// The audio does not reach back to [`Receiver::next_required_sample`].
    NeedEarlierAudio,
}

/// How [`Receiver::end`] closes the messages still open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Expired = 2,
    ReceptionEnded = 3,
}

/// One of upstream's eight receive contexts. Dropping it frees the context.
#[derive(Debug)]
pub struct Receiver {
    handle: c_int,
}

impl Receiver {
    /// A free receive context, or `None` when all eight are taken.
    pub fn new() -> Option<Self> {
        let handle = {
            let _guard = modem_lock();
            unsafe { tempo_fast_sys::jtty_rx_create() }
        };
        (handle > 0).then_some(Self { handle })
    }

    /// Start a reception at [`NSPS`], ending any running one. Sample positions
    /// are on the caller's timeline; the first search falls on the
    /// quarter-frame grid from `grid_origin`, at or after `first_search`.
    pub fn begin(&mut self, session_id: i64, grid_origin: i64, first_search: i64) {
        let _guard = modem_lock();
        unsafe {
            tempo_fast_sys::jtty_rx_begin(
                self.handle,
                session_id,
                grid_origin,
                first_search,
                NSPS as c_int,
            )
        }
    }

    /// Search `pcm` (12 kHz, its first element is sample `first_sample`) for
    /// up to `max_steps` quarter frames, never at or past `stop_sample`.
    /// Returns the steps run; 0 means more audio is needed. Retro searches reach
    /// back up to three steps, so keep audio from
    /// [`next_required_sample`](Self::next_required_sample) on.
    pub fn process(
        &mut self,
        pcm: &[i16],
        first_sample: i64,
        stop_sample: i64,
        max_steps: u32,
        band: SearchBand,
    ) -> Result<u32, ProcessError> {
        let count = pcm.len().min(c_int::MAX as usize) as c_int;
        let steps = max_steps.min(c_int::MAX as u32) as c_int;
        let n = {
            let _guard = modem_lock();
            unsafe {
                tempo_fast_sys::jtty_rx_process(
                    self.handle,
                    pcm.as_ptr(),
                    count,
                    first_sample,
                    stop_sample,
                    steps,
                    band.low_hz,
                    band.high_hz,
                    band.center_hz,
                    band.tolerance_hz,
                )
            }
        };
        match n {
            n if n >= 0 => Ok(n as u32),
            -2 => Err(ProcessError::NeedEarlierAudio),
            _ => Err(ProcessError::NotRunning),
        }
    }

    /// The earliest sample the next [`process`](Self::process) may read.
    pub fn next_required_sample(&self) -> i64 {
        let _guard = modem_lock();
        unsafe { tempo_fast_sys::jtty_rx_next_required_sample(self.handle) }
    }

    /// Where the next forward search starts.
    pub fn next_search_sample(&self) -> i64 {
        let _guard = modem_lock();
        unsafe { tempo_fast_sys::jtty_rx_next_search_sample(self.handle) }
    }

    /// End the reception; open messages are reported with `reason`.
    pub fn end(&mut self, reason: EndReason) {
        let _guard = modem_lock();
        unsafe { tempo_fast_sys::jtty_rx_end(self.handle, reason as c_int) }
    }

    /// Every update queued since the last call, oldest first.
    pub fn take_updates(&mut self) -> Vec<Update> {
        // WSJT-X's own decoder takes 30 at a time (JttyDecoder.hpp); so does
        // jtty_take_updates per inner call, so a full batch means look again.
        const BATCH: usize = 30;
        let mut out = Vec::new();
        loop {
            let mut raw = [JttyUpdateT::default(); BATCH];
            let n = {
                let _guard = modem_lock();
                unsafe {
                    tempo_fast_sys::jtty_take_updates(self.handle, raw.as_mut_ptr(), BATCH as c_int)
                }
            };
            let n = n.clamp(0, BATCH as c_int) as usize;
            out.extend(raw[..n].iter().map(|u| Update {
                message_id: u.message_id,
                text: nul_terminated(&u.text),
                freq_hz: u.freq,
                start_s: u.start_s,
                latest_s: u.latest_s,
                terminal: Terminal::from_raw(u.terminal),
                snr_db: u.snr,
            }));
            if n < BATCH {
                return out;
            }
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let _guard = modem_lock();
        unsafe { tempo_fast_sys::jtty_rx_destroy(self.handle) }
    }
}

/// NUL-terminated (or NUL-filled) byte field → trimmed String.
fn nul_terminated(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAND: SearchBand = SearchBand {
        low_hz: 200,
        high_hz: 2800,
        center_hz: 1500.0,
        tolerance_hz: 50.0,
    };

    /// `tones` as audio at `f0`, two seconds into a buffer with at least three
    /// seconds of silence after it (room for the decoder's last searches and
    /// retro sweeps), padded to `len` samples if that is longer.
    fn on_air_len(tones: &[i32], f0: f32, len: usize) -> Vec<i16> {
        let wave = gen_wave(tones, f0).expect("the tones must make a waveform");
        let lead = 2 * SAMPLE_RATE as usize;
        let need = lead + wave.len() + 3 * SAMPLE_RATE as usize;
        let mut pcm = vec![0i16; need.max(len)];
        for (i, s) in wave.iter().enumerate() {
            pcm[lead + i] = (s * 20_000.0) as i16;
        }
        pcm
    }

    fn on_air(tones: &[i32], f0: f32) -> Vec<i16> {
        on_air_len(tones, f0, 0)
    }

    /// One reception over the whole buffer, ended, and everything it reported.
    fn receive(pcm: &[i16]) -> Vec<Update> {
        let mut rx = Receiver::new().expect("a free receive context");
        rx.begin(1, 0, 0);
        let steps = rx
            .process(pcm, 0, pcm.len() as i64, 10_000, BAND)
            .expect("the whole buffer is given");
        assert!(steps > 0, "the buffer is long enough to search");
        rx.end(EndReason::ReceptionEnded);
        rx.take_updates()
    }

    /// Encode, synthesise, decode, all in-process: the text that comes back must
    /// be the text that went in, as one completed message, at the frequency it
    /// was sent on. A two-frame message, so the decoder's frame assembly is in
    /// the loop too.
    #[test]
    fn a_message_round_trips_through_the_modem() {
        let enc = encode("CQ KD9TAW EN52", Profile::Unknown).expect("the message encodes");
        assert_eq!(enc.sent, "CQ KD9TAW EN52");
        assert!(enc.frames() >= 2, "{} frames", enc.frames());

        let updates = receive(&on_air(&enc.tones, 1500.0));
        let complete: Vec<_> = updates
            .iter()
            .filter(|u| u.terminal == Terminal::Complete)
            .collect();
        assert_eq!(complete.len(), 1, "one completed message among {updates:?}");
        assert_eq!(complete[0].text, enc.sent, "{updates:?}");
        assert!(
            (complete[0].freq_hz - 1500.0).abs() < 5.0,
            "decoded at {} Hz",
            complete[0].freq_hz
        );
        assert!(
            (complete[0].start_s - 2.0).abs() < 0.1,
            "started at {} s",
            complete[0].start_s
        );
    }

    /// A frame whose 46 code symbols are all wrong must not decode: the sync is
    /// intact, so the decoder finds it and tries, and the CRC must refuse what
    /// comes out. The same message uncorrupted, placed the same way, decodes:
    /// that is what makes the silence the corruption's doing.
    #[test]
    fn a_frame_with_corrupted_code_symbols_does_not_decode() {
        let enc = encode("CQ KD9TAW CQ", Profile::Unknown).expect("the message encodes");
        assert_eq!(enc.frames(), 1);

        let clean = receive(&on_air(&enc.tones, 1500.0));
        assert!(
            clean
                .iter()
                .any(|u| u.terminal == Terminal::Complete && u.text == enc.sent),
            "control: the uncorrupted frame decodes, got {clean:?}"
        );

        let mut corrupted = enc.tones.clone();
        for (i, t) in corrupted[13..FRAME_SYMBOLS].iter_mut().enumerate() {
            *t = (*t + 1 + (i % 3) as i32) % 4;
        }
        assert!(
            corrupted[13..]
                .iter()
                .zip(&enc.tones[13..])
                .all(|(a, b)| a != b),
            "every code symbol changed"
        );
        let updates = receive(&on_air(&corrupted, 1500.0));
        assert!(updates.is_empty(), "a corrupted frame decoded: {updates:?}");
    }

    /// Two receivers fed two different signals, their calls interleaved, each
    /// report only their own message: upstream keeps reception state per
    /// handle and swaps it in around each call.
    #[test]
    fn two_receivers_keep_their_messages_apart() {
        let a = encode("CQ KD9TAW CQ", Profile::Unknown).unwrap();
        let b = encode("WB9XYZ 599 0123", Profile::Unknown).unwrap();
        let len = on_air(&b.tones, 1500.0)
            .len()
            .max(on_air(&a.tones, 1500.0).len());
        let (pa, pb) = (
            on_air_len(&a.tones, 1500.0, len),
            on_air_len(&b.tones, 1500.0, len),
        );
        let n = len;
        let mut ra = Receiver::new().expect("a free receive context");
        let mut rb = Receiver::new().expect("a second free receive context");
        ra.begin(1, 0, 0);
        rb.begin(2, 0, 0);
        let block = 3456;
        let mut end = 0;
        while end < n {
            end = (end + block).min(n);
            for (rx, pcm) in [(&mut ra, &pa), (&mut rb, &pb)] {
                while rx
                    .process(&pcm[..end], 0, end as i64, 1, BAND)
                    .expect("audio from 0")
                    > 0
                {}
            }
        }
        ra.end(EndReason::ReceptionEnded);
        rb.end(EndReason::ReceptionEnded);
        let (ua, ub) = (ra.take_updates(), rb.take_updates());
        let texts = |u: &[Update]| {
            u.iter()
                .filter(|x| x.terminal == Terminal::Complete)
                .map(|x| x.text.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(&ua), vec![a.sent.clone()], "receiver A: {ua:?}");
        assert_eq!(texts(&ub), vec![b.sent.clone()], "receiver B: {ub:?}");
        assert!(ua.iter().all(|x| x.text != b.sent) && ub.iter().all(|x| x.text != a.sent));
    }

    /// The waveform carries the tones it was given: in each of the 13 sync
    /// symbols, the strongest of the four tone frequencies is the sync tone.
    #[test]
    fn the_waveform_carries_the_sync_tones() {
        let enc = encode("CQ KD9TAW CQ", Profile::Unknown).unwrap();
        let f0 = 1500.0;
        let wave = gen_wave(&enc.tones, f0).unwrap();
        assert_eq!(wave.len(), FRAME_SAMPLES);
        assert!(wave.iter().all(|s| s.abs() <= 1.0));
        let sync = [0, 2, 2, 3, 0, 0, 3, 2, 1, 3, 1, 2, 0];
        assert_eq!(&enc.tones[..13], &sync[..]);
        for (k, &want) in sync.iter().enumerate() {
            // A whole symbol: over it the four tones are orthogonal (h = 1).
            let s = &wave[k * NSPS..(k + 1) * NSPS];
            let power = |tone: usize| {
                let w = 2.0 * std::f32::consts::PI * (f0 + tone as f32 * BAUD) / SAMPLE_RATE;
                let (re, im) = s.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, x)| {
                    (re + x * (w * i as f32).cos(), im - x * (w * i as f32).sin())
                });
                re * re + im * im
            };
            let best = (0..4)
                .max_by(|&x, &y| power(x).total_cmp(&power(y)))
                .unwrap();
            assert_eq!(best, want as usize, "sync symbol {k}");
        }
    }

    /// The normalised text comes back with the tones, and is what gets encoded.
    #[test]
    fn encode_returns_the_normalised_text() {
        let messy = encode("cq  kd9taw   cq ", Profile::Unknown).unwrap();
        let clean = encode("CQ KD9TAW CQ", Profile::Unknown).unwrap();
        assert_eq!(messy.sent, "CQ KD9TAW CQ");
        assert_eq!(messy, clean);
    }

    /// Upstream's spec publishes ten 34-bit words ("Representative 34-bit
    /// vectors" in its `jtty_source_encoding.txt`). Eight are reachable from
    /// text through the packing `encode` sends; each must pack to exactly its
    /// word, and read back to the same text with end-of-message where the spec
    /// puts it. Clearing end-of-message subtracts one, as the spec says.
    #[test]
    fn the_spec_vectors_pack_bit_exact() {
        use Profile::*;
        let vectors = [
            ("CQ K1ABC CQ", Unknown, true, 0x026F78D41),
            ("599 123", RttyRoundup, false, 0x20007B008),
            ("599 123", RttyRoundup, true, 0x20007B009),
            ("599 CA", RttyRoundup, true, 0x2001BA019),
            ("1D EMA", FieldDay, true, 0x082C58029),
            ("AGN?", Unknown, true, 0x000000049),
            ("FN42", Unknown, true, 0x04A198049),
            ("HELLO", Unknown, true, 0x11395558D),
        ];
        for (text, profile, last, word) in vectors {
            let p = pack(text, profile, last).expect("the text packs");
            assert_eq!(p.frames, vec![word], "{text:?} {profile:?} last={last}");
            assert_eq!(p.sent, text);
            assert_eq!(
                unpack(&p.frames),
                Ok(Unpacked {
                    text: text.to_string(),
                    eom: last
                }),
                "{word:#011X}"
            );
        }
        // The profile chooses the atom, not just the frame count: Unknown packs
        // "599 123" as a generic number, RTTY Roundup as a serial.
        let generic = pack("599 123", Unknown, true).unwrap();
        assert_eq!(generic.frames.len(), 1);
        assert_ne!(generic.frames[0], 0x20007B009);
        assert_eq!(unpack(&generic.frames).unwrap().text, "599 123");
    }

    /// The other two published words carry atoms no text packs to (upstream
    /// infers neither a zone-and-location pair nor a serial-and-time exchange
    /// from text, and its C descriptor reaches neither), so Nexus can only
    /// receive them. They must read back as the spec renders them.
    #[test]
    fn the_spec_vectors_nexus_cannot_send_read_back() {
        for (word, text) in [(0x00B790D29, "599 05 NWT"), (0x204E42D39, "599 156 1749")] {
            assert_eq!(
                unpack(&[word]),
                Ok(Unpacked {
                    text: text.to_string(),
                    eom: true
                })
            );
        }
    }

    /// The spec's frame-count examples (its "Automatic text packing"), with
    /// the text each profile normalises to. `encode`, the path that goes on the
    /// air, must agree with `pack` on both.
    #[test]
    fn frame_counts_match_the_spec_examples() {
        use Profile::*;
        let examples = [
            ("CQ K1ABC CQ", Unknown, 1, "CQ K1ABC CQ"),
            ("599 123", Unknown, 1, "599 123"),
            ("599 MA", Unknown, 1, "599 MA"),
            ("599 FN42", Unknown, 1, "599 FN42"),
            ("1D EMA", Unknown, 1, "1D EMA"),
            ("599 001", Unknown, 2, "599 001"),
            ("599 001", FieldDay, 2, "599 001"),
            ("599 001", RttyRoundup, 1, "599 001"),
            ("K1ABC 599 001", RttyRoundup, 2, "K1ABC 599 001"),
            ("599 05", Unknown, 2, "599 05"),
            ("599 05", FieldDay, 2, "599 05"),
            ("599 05", RttyRoundup, 1, "599 005"),
            ("599 0123", Unknown, 2, "599 0123"),
            ("599 0123", RttyRoundup, 1, "599 123"),
            ("599 BRUCE", Unknown, 2, "599 BRUCE"),
        ];
        for (text, profile, frames, sent) in examples {
            let p = pack(text, profile, true).expect("the text packs");
            assert_eq!(
                (p.frames.len(), p.sent.as_str()),
                (frames, sent),
                "pack {text:?} {profile:?}"
            );
            let e = encode(text, profile).expect("the text encodes");
            assert_eq!(
                (e.frames(), e.sent.as_str()),
                (frames, sent),
                "encode {text:?} {profile:?}"
            );
        }
    }

    /// `unpack` renders words the way the receiver shows a message: for each
    /// of these, the receiver's own decode of the sent audio, `unpack` of the
    /// packed words and the sent text all agree.
    #[test]
    fn unpack_reads_what_the_receiver_shows() {
        for (text, profile) in [
            ("CQ KD9TAW EN52", Profile::Unknown),
            ("TU K1ABC CQ", Profile::Unknown),
            ("K1ABC 599 0123", Profile::RttyRoundup),
            ("2A EMA HELLO 73", Profile::FieldDay),
        ] {
            let p = pack(text, profile, true).unwrap();
            let e = encode(text, profile).unwrap();
            assert_eq!(p.frames.len(), e.frames());
            let shown = unpack(&p.frames).expect("valid words");
            assert_eq!(shown.text, e.sent);
            assert!(shown.eom);
            let updates = receive(&on_air(&e.tones, 1500.0));
            let complete: Vec<_> = updates
                .iter()
                .filter(|u| u.terminal == Terminal::Complete)
                .map(|u| u.text.as_str())
                .collect();
            assert_eq!(complete, vec![shown.text.as_str()], "{updates:?}");
        }
    }

    /// A word the receiver would discard is reported, and left out of the text.
    #[test]
    fn unpack_refuses_what_the_receiver_discards() {
        let cq = 0x026F78D41;
        let tu = pack("TU", Profile::Unknown, true).unwrap().frames[0];
        assert_eq!(unpack(&[cq]).map(|u| u.text), Ok("CQ K1ABC CQ".to_string()));
        // Bit 33 is reserved and must be zero.
        assert_eq!(
            unpack(&[cq | 0b10]),
            Err(UnpackError::InvalidWord {
                text: String::new()
            })
        );
        // The all-zero grammar word is the invalid sentinel, end-of-message or not.
        assert!(matches!(unpack(&[0]), Err(UnpackError::InvalidWord { .. })));
        assert!(matches!(unpack(&[1]), Err(UnpackError::InvalidWord { .. })));
        // A good word before a bad one still renders; end-of-message needs
        // every word valid.
        let more = pack("CQ K1ABC CQ", Profile::Unknown, false).unwrap().frames[0];
        assert_eq!(
            unpack(&[more, tu | 0b10]),
            Err(UnpackError::InvalidWord {
                text: "CQ K1ABC CQ".to_string()
            })
        );
        assert_eq!(
            unpack(&[more, tu]),
            Ok(Unpacked {
                text: "CQ K1ABC CQ TU".to_string(),
                eom: true
            })
        );
        // Text another sender packed with a leading space is shown without it,
        // as the receiver shows it: TEXT5 " CQ  " (alphabet indices 36 12 26 36
        // 36, form 11, end-of-message).
        let text5 = [36u64, 12, 26, 36, 36].iter().fold(0, |w, &c| w * 64 + c) << 4 | 0b1101;
        assert_eq!(
            unpack(&[text5]),
            Ok(Unpacked {
                text: "CQ".to_string(),
                eom: true
            })
        );
        assert_eq!(unpack(&[]), Err(UnpackError::BadFrames));
        assert_eq!(unpack(&[cq; 17]), Err(UnpackError::BadFrames));
        assert_eq!(unpack(&[1 << 34]), Err(UnpackError::BadFrames));
    }

    #[test]
    fn empty_and_overlong_text_are_refused() {
        assert_eq!(encode("", Profile::Unknown), Err(EncodeError::Empty));
        assert_eq!(encode("   ", Profile::Unknown), Err(EncodeError::Empty));
        let longest = "A".repeat(TEXT_LEN);
        assert!(encode(&longest, Profile::Unknown).is_ok());
        let too_long = "A".repeat(TEXT_LEN + 1);
        assert_eq!(
            encode(&too_long, Profile::Unknown),
            Err(EncodeError::TooLong)
        );
    }

    #[test]
    fn atoms_encode_one_frame_each_and_bad_ones_are_refused() {
        let cq = atom(0, 0, 0, 0, "KD9TAW").expect("a call atom");
        assert_eq!(encode_atoms(&[cq]).map(|t| t.len()), Ok(FRAME_SYMBOLS));
        assert_eq!(
            encode_atoms(&[cq, cq]).map(|t| t.len()),
            Ok(2 * FRAME_SYMBOLS)
        );
        let fd = atom(3, 1, 0, 1, "EMA").expect("a class/section atom");
        assert_eq!(encode_atoms(&[fd]).map(|t| t.len()), Ok(FRAME_SYMBOLS));
        let nowhere = atom(3, 1, 0, 1, "XQZ").unwrap();
        assert_eq!(encode_atoms(&[nowhere]), Err(AtomError::UnknownSection));
        assert_eq!(encode_atoms(&[]), Err(AtomError::InvalidDescriptor));
        assert_eq!(encode_atoms(&[cq; 17]), Err(AtomError::InvalidDescriptor));
        assert!(atom(0, 0, 0, 0, "KD9TAWXX").is_some(), "8 bytes fit");
        assert!(atom(0, 0, 0, 0, "KD9TAWXXX").is_none(), "9 bytes do not");
    }

    #[test]
    fn gen_wave_refuses_partial_frames_and_bad_tones() {
        let enc = encode("CQ KD9TAW CQ", Profile::Unknown).unwrap();
        assert!(gen_wave(&enc.tones, 1500.0).is_some());
        assert_eq!(gen_wave(&enc.tones[..58], 1500.0), None, "a partial frame");
        assert_eq!(gen_wave(&[], 1500.0), None, "no frames");
        // Past one frame but not a whole number of them: only the whole-frame
        // check refuses these (58 above is caught by the one-frame minimum).
        let three = encode("CQ KD9TAW EN52", Profile::Unknown).unwrap();
        assert_eq!(three.frames(), 3);
        for n in [
            FRAME_SYMBOLS + 1,
            2 * FRAME_SYMBOLS - 1,
            2 * FRAME_SYMBOLS + 1,
        ] {
            assert_eq!(gen_wave(&three.tones[..n], 1500.0), None, "{n} tones");
        }
        assert!(gen_wave(&three.tones[..2 * FRAME_SYMBOLS], 1500.0).is_some());
        let mut bad = enc.tones.clone();
        bad[20] = 4;
        assert_eq!(gen_wave(&bad, 1500.0), None, "a tone above 3");
        bad[20] = -1;
        assert_eq!(gen_wave(&bad, 1500.0), None, "a negative tone");
        assert_eq!(
            gen_wave(&[0; MAX_TONES + FRAME_SYMBOLS], 1500.0),
            None,
            "17 frames"
        );
    }

    /// The C ABI's own refusals, called directly: each must refuse and write
    /// nothing. The safe API above cannot reach most of them, which is why they
    /// are pinned here.
    #[test]
    fn the_c_abi_refuses_bad_arguments_and_writes_nothing() {
        let _guard = modem_lock();
        let msg = b"CQ KD9TAW EN52";
        let mut tones: [c_int; MAX_TONES] = [-7; MAX_TONES];
        let mut sent = [b'?' as c_char; TEXT_LEN + 1];
        let call = |len: c_int, profile: c_int, tones: &mut [c_int], sent: &mut [c_char]| unsafe {
            tempo_fast_sys::jtty_encode_text(
                msg.as_ptr().cast(),
                len,
                profile,
                tones.as_mut_ptr(),
                tones.len() as c_int,
                sent.as_mut_ptr(),
                sent.len() as c_int,
            )
        };
        assert_eq!(call(-1, 0, &mut tones, &mut sent), JTTY_ERR_ARGS);
        assert_eq!(
            call(TEXT_LEN as c_int + 1, 0, &mut tones, &mut sent),
            JTTY_ERR_ARGS
        );
        assert_eq!(
            call(msg.len() as c_int, 0, &mut tones, &mut sent[..TEXT_LEN]),
            JTTY_ERR_ARGS
        );
        assert_eq!(
            call(
                msg.len() as c_int,
                0,
                &mut tones[..FRAME_SYMBOLS],
                &mut sent
            ),
            JTTY_ERR_ARGS
        );
        assert!(tones.iter().all(|&t| t == -7), "a refused call wrote tones");
        assert!(
            sent.iter().all(|&c| c == b'?' as c_char),
            "a refused call wrote text"
        );
        // A profile the modem does not know: the text is there, so it is
        // unencodable, not empty.
        assert_eq!(
            call(msg.len() as c_int, 7, &mut tones, &mut sent),
            JTTY_ERR_ENCODE
        );
        assert_eq!(call(0, 0, &mut tones, &mut sent), 0);
        let n = call(msg.len() as c_int, 0, &mut tones, &mut sent);
        assert!(
            n >= 2 * FRAME_SYMBOLS as c_int && n % FRAME_SYMBOLS as c_int == 0,
            "{n}"
        );

        let cq = atom(0, 0, 0, 0, "KD9TAW").unwrap();
        let mut status = -1;
        let mut few: [c_int; FRAME_SYMBOLS - 1] = [-7; FRAME_SYMBOLS - 1];
        let r = unsafe {
            tempo_fast_sys::jtty_encode_atoms(
                &cq,
                1,
                few.as_mut_ptr(),
                few.len() as c_int,
                &mut status,
            )
        };
        assert_eq!(r, JTTY_ERR_ARGS);
        assert!(few.iter().all(|&t| t == -7), "a refused call wrote tones");

        let one = &tones[..FRAME_SYMBOLS];
        let mut wave = [9.0f32; FRAME_SAMPLES];
        let wav = |t: &[c_int], nsym: c_int, wave: &mut [f32]| unsafe {
            tempo_fast_sys::jtty_gen_wave(
                t.as_ptr(),
                nsym,
                1500.0,
                wave.as_mut_ptr(),
                wave.len() as c_int,
            )
        };
        assert_eq!(
            wav(one, FRAME_SYMBOLS as c_int - 1, &mut wave),
            JTTY_ERR_ARGS
        );
        assert_eq!(
            wav(one, FRAME_SYMBOLS as c_int, &mut wave[..FRAME_SAMPLES - 1]),
            JTTY_ERR_ARGS
        );
        assert!(wave.iter().all(|&s| s == 9.0), "a refused call wrote audio");
        // One frame and a symbol, into a buffer big enough for it: only the
        // whole-frame check is left to refuse.
        let mut roomy = vec![9.0f32; 2 * FRAME_SAMPLES];
        assert_eq!(
            wav(
                &tones[..FRAME_SYMBOLS + 1],
                FRAME_SYMBOLS as c_int + 1,
                &mut roomy
            ),
            JTTY_ERR_ARGS
        );
        assert!(
            roomy.iter().all(|&s| s == 9.0),
            "a refused call wrote audio"
        );
        assert_eq!(
            wav(one, FRAME_SYMBOLS as c_int, &mut wave),
            FRAME_SAMPLES as c_int
        );

        let mut out = [JttyUpdateT::default(); 1];
        assert_eq!(
            unsafe { tempo_fast_sys::jtty_take_updates(1, out.as_mut_ptr(), 0) },
            0
        );
    }

    /// The same for the source-word entries, which the safe API checks first.
    #[test]
    fn the_word_entries_refuse_bad_arguments_and_write_nothing() {
        let _guard = modem_lock();
        let msg = b"CQ KD9TAW EN52";
        let mut frames = [-7i64; MAX_FRAMES];
        let mut sent = [b'?' as c_char; TEXT_LEN + 1];
        let pack_c = |len: c_int, last: c_int, frames: &mut [i64], sent: &mut [c_char]| unsafe {
            tempo_fast_sys::jtty_pack_text(
                msg.as_ptr().cast(),
                len,
                0,
                last,
                frames.as_mut_ptr(),
                frames.len() as c_int,
                sent.as_mut_ptr(),
                sent.len() as c_int,
            )
        };
        let len = msg.len() as c_int;
        assert_eq!(pack_c(-1, 1, &mut frames, &mut sent), JTTY_ERR_ARGS);
        assert_eq!(
            pack_c(TEXT_LEN as c_int + 1, 1, &mut frames, &mut sent),
            JTTY_ERR_ARGS
        );
        assert_eq!(
            pack_c(len, 1, &mut frames, &mut sent[..TEXT_LEN]),
            JTTY_ERR_ARGS
        );
        assert_eq!(pack_c(len, 2, &mut frames, &mut sent), JTTY_ERR_ARGS);
        assert_eq!(pack_c(len, -1, &mut frames, &mut sent), JTTY_ERR_ARGS);
        // Three frames do not fit in two.
        assert_eq!(pack_c(len, 1, &mut frames[..2], &mut sent), JTTY_ERR_ARGS);
        assert!(
            frames.iter().all(|&w| w == -7),
            "a refused call wrote words"
        );
        assert!(
            sent.iter().all(|&c| c == b'?' as c_char),
            "a refused call wrote text"
        );
        assert_eq!(pack_c(len, 1, &mut frames[..3], &mut sent), 3);
        assert_eq!(pack_c(len, 0, &mut frames, &mut sent), 3);
        assert_eq!(frames[2] & 1, 0, "is_final 0 leaves end-of-message clear");

        let mut text = [b'?' as c_char; TEXT_LEN + 1];
        let mut eom: c_int = -1;
        let unpack_c = |words: &[i64], text: &mut [c_char], eom: &mut c_int| unsafe {
            tempo_fast_sys::jtty_unpack_frames(
                words.as_ptr(),
                words.len() as c_int,
                text.as_mut_ptr(),
                text.len() as c_int,
                eom,
            )
        };
        let cq = 0x026F78D41i64;
        assert_eq!(unpack_c(&[], &mut text, &mut eom), JTTY_ERR_ARGS);
        assert_eq!(unpack_c(&[cq; 17], &mut text, &mut eom), JTTY_ERR_ARGS);
        assert_eq!(unpack_c(&[cq, 1 << 34], &mut text, &mut eom), JTTY_ERR_ARGS);
        assert_eq!(unpack_c(&[cq, -1], &mut text, &mut eom), JTTY_ERR_ARGS);
        assert_eq!(
            unpack_c(&[cq], &mut text[..TEXT_LEN], &mut eom),
            JTTY_ERR_ARGS
        );
        assert!(
            text.iter().all(|&c| c == b'?' as c_char),
            "a refused call wrote text"
        );
        assert_eq!(eom, -1, "a refused call wrote end-of-message");
        assert_eq!(unpack_c(&[cq], &mut text, &mut eom), 0);
        assert_eq!(
            (nul_terminated(&text.map(|c| c as u8)), eom),
            ("CQ K1ABC CQ".to_string(), 1)
        );
    }
}
