//! WSJT-X's FT8 and FT4 transmit encoder in Rust: the 77-bit packer, CRC-14, LDPC(174,91), the
//! tone maps and the GFSK waveform, a pure function of its input that takes no lock.
//!
//! WHY IT EXISTS. The vendored encoder (`ft8_encode` / `ft4_encode` / `*_gen_wave`) is Fortran
//! with process-global state shared with the decoder, so every call waits on `MODEM_LOCK`, which a
//! decode holds for its whole length. An over built at its key (one that changed after the early
//! pass: a double-click, a new TX frequency or message, the sequencer moving on) therefore went out
//! late by however long the decode overran, and lost its end at the next boundary. WSJT-X's decoder
//! is another process and never delays a key. This builds the same over without the lock.
//!
//! THE SAME OVER, BY CONSTRUCTION AND BY TEST. Each routine is the vendored one ported statement
//! by statement (`packjt77.f90`, `genft8.f90`, `genft4.f90`, `encode174_91.f90`, `crc14.cpp`,
//! `gen_ft8wave.f90`, `gen_ft4wave.f90`); `crates/modes/tests/wsjt77_golden.rs` runs both encoders
//! over every message type and a generated corpus, and any tone that differs fails it. The tones
//! are what goes on the air; the waveform matches to float rounding (see `wave`).
//!
//! WHAT IT DOES NOT DO. `pack77` also writes the callsign tables the decoder reads to print a hashed
//! call (`save_hash_call`, and `unpack77`'s recent-call and hash memos). This port never touches
//! them: the caller hands the message to the vendored encoder after the fact
//! (`ft8::encode_later` / `ft4::encode_later`), which writes exactly what it always did, before the
//! next modem call can read them. And where the Fortran's result is not a function of the text
//! (`pack::Vendored`), these functions answer `None` and the caller uses the vendored encoder.

pub mod pack;
pub mod wave;

pub use wave::{ft4_wave, ft8_wave};

/// The vendored LDPC(174,91) generator, `ldpc_174_91_c_generator.f90`, read at compile time: 83
/// rows of 91 bits, column `c` (1-based) at bit `91 - c`.
const GENERATOR: [u128; 83] = parse_generator(include_bytes!(
    "../../../../libtempo/vendor/wsjtx/lib/ft8/ldpc_174_91_c_generator.f90"
));

/// Each quoted string of the Fortran `data g/…/` is one row: 23 hex digits, of which the first 91
/// bits are the columns (`encode174_91.f90` reads 4 bits of each digit, 3 of the last).
const fn parse_generator(src: &[u8]) -> [u128; 83] {
    let mut rows = [0u128; 83];
    let mut row = 0;
    let mut i = 0;
    while i < src.len() {
        if src[i] != b'"' {
            i += 1;
            continue;
        }
        let mut v: u128 = 0;
        let mut j = 0;
        while j < 23 {
            let c = src[i + 1 + j];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => panic!("a generator row holds a character that is not a hex digit"),
            };
            v = (v << 4) | d as u128;
            j += 1;
        }
        assert!(src[i + 24] == b'"', "a generator row is not 23 hex digits");
        rows[row] = v >> 1;
        row += 1;
        i += 25;
    }
    assert!(row == 83, "the generator does not have 83 rows");
    rows
}

/// `crc14` (`boost::augmented_crc<14, 0x2757>`) over the 12 bytes `encode174_91` hands it: the
/// 77 message bits, then 19 zero bits.
fn crc14(msg: &[u8; 77]) -> u16 {
    let mut rem: u16 = 0;
    for bit in msg
        .iter()
        .map(|&b| u16::from(b))
        .chain(std::iter::repeat_n(0, 19))
    {
        let top = (rem >> 13) & 1;
        rem = ((rem << 1) | bit) & 0x3FFF;
        if top == 1 {
            rem ^= 0x2757;
        }
    }
    rem
}

/// `encode174_91`: the 91 bits (message and CRC-14), then the 83 parity checks.
fn codeword(msg: &[u8; 77]) -> [u8; 174] {
    let crc = crc14(msg);
    let mut cw = [0u8; 174];
    cw[..77].copy_from_slice(msg);
    for k in 0..14 {
        cw[77 + k] = ((crc >> (13 - k)) & 1) as u8;
    }
    let m = cw[..91]
        .iter()
        .fold(0u128, |v, &b| (v << 1) | u128::from(b));
    for (i, row) in GENERATOR.iter().enumerate() {
        cw[91 + i] = ((row & m).count_ones() & 1) as u8;
    }
    cw
}

/// `genft8` from the 77 bits: three 7x7 Costas arrays and 58 Gray-coded 3-bit symbols.
fn ft8_map(msg: &[u8; 77]) -> Vec<i32> {
    const COSTAS: [i32; 7] = [3, 1, 4, 0, 6, 5, 2];
    const GRAY: [i32; 8] = [0, 1, 3, 2, 5, 6, 4, 7];
    let cw = codeword(msg);
    let mut itone = vec![0i32; 79];
    itone[..7].copy_from_slice(&COSTAS);
    itone[36..43].copy_from_slice(&COSTAS);
    itone[72..].copy_from_slice(&COSTAS);
    for j in 0..58 {
        let k = if j < 29 { 7 + j } else { 14 + j };
        let b = &cw[3 * j..3 * j + 3];
        itone[k] = GRAY[usize::from(b[0] * 4 + b[1] * 2 + b[2])];
    }
    itone
}

/// `genft4` from the 77 bits: scrambled by `rvec`, four 4x4 sync arrays and 87 2-bit symbols.
fn ft4_map(msg: &[u8; 77]) -> Vec<i32> {
    const RVEC: [u8; 77] = [
        0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1,
        0, 0, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1,
        0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1,
    ];
    const SYNC: [[i32; 4]; 4] = [[0, 1, 3, 2], [1, 0, 2, 3], [2, 3, 1, 0], [3, 2, 0, 1]];
    let mut scrambled = *msg;
    for (b, r) in scrambled.iter_mut().zip(RVEC) {
        *b ^= r;
    }
    let cw = codeword(&scrambled);
    let data: Vec<i32> = (0..87)
        .map(|i| match 2 * cw[2 * i] + cw[2 * i + 1] {
            2 => 3,
            3 => 2,
            is => i32::from(is),
        })
        .collect();
    let mut itone = Vec::with_capacity(103);
    for (block, sync) in SYNC.iter().enumerate() {
        itone.extend_from_slice(sync);
        if block < 3 {
            itone.extend_from_slice(&data[29 * block..29 * block + 29]);
        }
    }
    itone
}

/// The 37 characters `ft8_encode` / `ft4_encode` copy from a C string: at most 37 bytes, up to
/// the first NUL, blank padded. `None` for a byte outside printable ASCII, which no message Nexus
/// sends holds and whose handling (gfortran's `ichar`, its tab and blank rules) is not ported.
fn c_message(msg: &str) -> Option<[u8; 37]> {
    let mut m = [b' '; 37];
    for (slot, &b) in m
        .iter_mut()
        .zip(msg.as_bytes().iter().take_while(|&&b| b != 0))
    {
        *slot = b;
    }
    m.iter().all(|b| (0x20..0x7F).contains(b)).then_some(m)
}

/// The 79 tones the vendored `ft8_encode` makes of `msg`, all zero where `genft8` calls it a bad
/// message (that entry point refuses nothing: it returns those zeros as 79 tones). `None` where the
/// vendored result is not a function of the text: encode it with `ft8::encode`.
pub fn ft8_tones(msg: &str) -> Option<Vec<i32>> {
    let msg37 = c_message(msg)?;
    match pack::pack(&msg37) {
        Err(pack::Vendored) => None,
        Ok(None) => Some(vec![0; 79]),
        Ok(Some(bits)) => Some(ft8_map(&bits)),
    }
}

/// The 103 tones the vendored `ft4_encode` makes of `msg`, empty where it refuses a bad message.
/// `None` where the vendored result is not a function of the text: encode it with `ft4::encode`.
pub fn ft4_tones(msg: &str) -> Option<Vec<i32>> {
    let mut message = c_message(msg)?;
    // genft4 strips leading blanks with `message=message(i+1:)` on its loop index, so a second
    // blank in a row takes the character after it too. Kept as it is.
    for i in 1..=37 {
        if message[0] != b' ' {
            break;
        }
        let mut next = [b' '; 37];
        next[..37 - i].copy_from_slice(&message[i..]);
        message = next;
    }
    match pack::pack(&message) {
        Err(pack::Vendored) => None,
        Ok(None) => Some(Vec::new()),
        Ok(Some(bits)) => Some(ft4_map(&bits)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The generator's first and last rows, as the Fortran spells them.
    #[test]
    fn the_generator_is_read_from_the_vendored_table() {
        assert_eq!(GENERATOR[0], 0x8329ce11bf31eaf509f27fc >> 1);
        assert_eq!(GENERATOR[82], 0x608cc857594bfbb55d69600 >> 1);
    }

    /// genft4's leading-blank loop, quirk and all: two blanks lose the character after them.
    #[test]
    fn ft4_strips_leading_blanks_as_genft4_does() {
        assert_eq!(ft4_tones("  ACQ K1ABC FN42"), ft4_tones("CQ K1ABC FN42"));
        assert_eq!(ft4_tones(" CQ K1ABC FN42"), ft4_tones("CQ K1ABC FN42"));
        assert_eq!(ft4_tones("   CQ K1ABC FN42"), ft4_tones("CQ K1ABC FN42"));
    }
}
