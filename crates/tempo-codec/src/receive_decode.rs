//! The shack's receive audio, decoded on a computer driving it over its own network: one Opus
//! packet from the audio road in, 20 ms of mono at 48 kHz out, for the headset
//! ([`crate::headset`]). The shack encodes it with [`crate::receive_encode`] (CELT-only, 24 kHz,
//! 24 kbit/s); libopus decodes any stream at any of its rates, and 48 kHz is the rate most headsets
//! run at, so most need no resampling at all.
//!
//! **This module knows nothing about a transport or a device.** It turns packets into samples.
//!
//! A lost packet is not concealed here: the headset plays its quiet bed in a known gap, as the
//! hosted page does, so a hole in the band never comes out of the headset as a made-up burst.

/// The rate the decoder gives: what most headsets run at.
pub const PLAY_RATE_HZ: u32 = 48_000;
/// Samples in one 20 ms frame at [`PLAY_RATE_HZ`].
pub const PLAY_FRAME_SAMPLES: usize =
    (PLAY_RATE_HZ as usize) * (crate::receive_encode::FRAME_MS as usize) / 1000;
/// The longest Opus packet (120 ms) at [`PLAY_RATE_HZ`].
const MAX_SAMPLES: usize = (PLAY_RATE_HZ as usize) * 120 / 1000;

/// One road's receive decoder. One per road: a decoder carries state from packet to packet.
pub struct ReceiveDecoder {
    decoder: opus::Decoder,
    out: Vec<f32>,
}

impl ReceiveDecoder {
    pub fn new() -> Result<Self, opus::Error> {
        Ok(Self {
            decoder: opus::Decoder::new(PLAY_RATE_HZ, opus::Channels::Mono)?,
            out: vec![0.0; MAX_SAMPLES],
        })
    }

    /// One packet, as mono samples at [`PLAY_RATE_HZ`]. `None` for a packet libopus refuses
    /// (malformed, or empty, which is its "conceal this"): that moment is then a gap.
    pub fn decode(&mut self, packet: &[u8]) -> Option<Vec<f32>> {
        if packet.is_empty() {
            return None;
        }
        let n = self
            .decoder
            .decode_float(packet, &mut self.out, false)
            .ok()?;
        Some(self.out[..n].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receive_encode::encode_offline;

    /// ★ What the shack's own receive encoder makes decodes here, frame by frame, as 20 ms at the
    /// headset's rate, and a 700 Hz tone in the band comes out a 700 Hz tone. CONTROL: a packet
    /// libopus refuses, and an empty one, are a gap, never a made-up frame.
    #[test]
    fn the_shacks_receive_frames_decode_as_20_ms_at_48_khz() {
        let rate = 48_000;
        let band: Vec<f32> = (0..rate as usize)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 700.0 * i as f32 / rate as f32).sin())
            .collect();
        let frames = encode_offline(rate, &band).unwrap();
        assert!(frames.len() >= 40, "premise: {} frames", frames.len());
        let mut decoder = ReceiveDecoder::new().unwrap();
        let mut decoded = Vec::new();
        for frame in &frames {
            let samples = decoder.decode(&frame.packet).expect("a frame was refused");
            assert_eq!(samples.len(), PLAY_FRAME_SAMPLES);
            decoded.extend(samples);
        }
        let tail = &decoded[decoded.len() - 9_600..];
        let crossings = tail
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        let hz = crossings as f32 / 2.0 / (tail.len() as f32 / PLAY_RATE_HZ as f32);
        assert!(
            (680.0..720.0).contains(&hz),
            "a 700 Hz tone came out {hz:.0} Hz"
        );
        assert_eq!(decoder.decode(&[]), None);
        assert_eq!(decoder.decode(&[0xff; 3]), None);
    }
}
