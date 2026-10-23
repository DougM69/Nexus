//! The microphone of a computer driving a shack over its own network, encoded: 20 ms of mono voice
//! at 48 kHz in, one Opus packet out, one packet a datagram on the audio road. The shack decodes it
//! with [`crate::mic_decode::MicDecoder`], exactly as it decodes a browser's microphone, and what
//! may reach the transmitter, and when, stays `tempo_app::mic`'s to decide.
//!
//! **This module knows nothing about keying or about a transport.** It turns samples into packets.
//!
//! ## The codec configuration, and why each line is what it is
//!
//! * **[`opus::Application::Voip`], 48 kHz in.** Voice mode, at the rate a browser sends today, so
//!   the shack's decoder meets the same kind of stream it already takes from the page.
//! * **20 ms frames**, one a datagram: the page's frame length, and the receive side's.
//! * **32 kbit/s, constant.** Every packet is [`MIC_PACKET_BYTES`] long, so a frame can never
//!   outgrow its datagram and the road's rate is known before a word is said.
//! * **DTX off.** Discontinuous transmission stops sending during a pause, and a pause the shack
//!   never hears reads as a gap: 200 ms of it ends the over (M2). Off by default; stated anyway.
//! * **In-band FEC off, and no loss percentage given.** The shack conceals nothing: a lost frame is
//!   silence on the air, never made up. Off by default; stated anyway.
//!
//! Neural extensions (deep PLC, DRED, OSCE) stay off, as for the receive encoder: see the crate's
//! Cargo.toml.

/// The rate the encoder takes: the page's own.
pub const MIC_ENCODE_RATE_HZ: u32 = 48_000;
/// One frame, in milliseconds.
pub const MIC_FRAME_MS: u64 = 20;
/// Samples in one frame at [`MIC_ENCODE_RATE_HZ`].
pub const MIC_FRAME_SAMPLES: usize = (MIC_ENCODE_RATE_HZ as usize) * (MIC_FRAME_MS as usize) / 1000;
/// The constant bit rate.
pub const MIC_BITRATE_BPS: i32 = 32_000;
/// Every packet's length at that rate: 32 kbit/s for 20 ms.
pub const MIC_PACKET_BYTES: usize = (MIC_BITRATE_BPS as usize) * (MIC_FRAME_MS as usize) / 8000;

/// One computer's microphone encoder. One per press: an encoder carries state from frame to frame.
pub struct MicEncoder {
    encoder: opus::Encoder,
    out: Vec<u8>,
}

impl MicEncoder {
    pub fn new() -> Result<Self, opus::Error> {
        let mut encoder = opus::Encoder::new(
            MIC_ENCODE_RATE_HZ,
            opus::Channels::Mono,
            opus::Application::Voip,
        )?;
        encoder.set_bitrate(opus::Bitrate::Bits(MIC_BITRATE_BPS))?;
        encoder.set_vbr(false)?;
        encoder.set_expert_frame_duration(opus::FrameSize::Ms20)?;
        encoder.set_dtx(false)?;
        encoder.set_inband_fec(false)?;
        encoder.set_packet_loss_perc(0)?;
        Ok(Self {
            encoder,
            // Room past the constant size, which libopus pads every packet out to.
            out: vec![0; 4 * MIC_PACKET_BYTES],
        })
    }

    /// One frame, [`MIC_FRAME_SAMPLES`] mono samples at [`MIC_ENCODE_RATE_HZ`], as one packet.
    /// `None` for a frame of any other length, or one libopus refuses: that moment is then silence
    /// at the shack, as a lost one is.
    pub fn encode(&mut self, frame: &[f32]) -> Option<Vec<u8>> {
        if frame.len() != MIC_FRAME_SAMPLES {
            return None;
        }
        let n = self.encoder.encode_float(frame, &mut self.out).ok()?;
        Some(self.out[..n].to_vec())
    }

    #[cfg(test)]
    fn encoder(&mut self) -> &mut opus::Encoder {
        &mut self.encoder
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mic_decode::{MicDecoder, FRAME_SAMPLES, MIC_RATE_HZ};

    /// Frame `k` of a `hz` tone at `level`, at 48 kHz.
    fn tone(hz: f32, level: f32, k: usize) -> Vec<f32> {
        (0..MIC_FRAME_SAMPLES)
            .map(|i| {
                let t = (k * MIC_FRAME_SAMPLES + i) as f32 / MIC_ENCODE_RATE_HZ as f32;
                level * (2.0 * std::f32::consts::PI * hz * t).sin()
            })
            .collect()
    }

    /// ★ Every packet is the constant size, whatever the frame holds: speech-loud, quiet, silent,
    /// or noise. 80 bytes, so a frame with its press and number fits a datagram of 128. CONTROL:
    /// the same encoder in variable rate makes packets of other sizes from the same frames.
    #[test]
    fn every_packet_is_the_constant_size() {
        assert_eq!(MIC_PACKET_BYTES, 80);
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        let frames: Vec<Vec<f32>> = (0..50)
            .map(|k| match k % 4 {
                0 => tone(440.0, 0.8, k),
                1 => tone(1_200.0, 0.01, k),
                2 => vec![0.0; MIC_FRAME_SAMPLES],
                _ => (0..MIC_FRAME_SAMPLES).map(|_| noise()).collect(),
            })
            .collect();
        let mut encoder = MicEncoder::new().unwrap();
        for (k, frame) in frames.iter().enumerate() {
            let packet = encoder.encode(frame).expect("a frame was refused");
            assert_eq!(packet.len(), MIC_PACKET_BYTES, "frame {k}");
        }
        let mut variable = MicEncoder::new().unwrap();
        variable.encoder().set_vbr(true).unwrap();
        let sizes: std::collections::BTreeSet<usize> = frames
            .iter()
            .map(|frame| variable.encode(frame).unwrap().len())
            .collect();
        assert!(sizes.len() > 1, "control: variable rate varied nothing");
    }

    /// ★ The encoder is set as the road needs it: voice mode, 32 kbit/s constant, no silence
    /// suppression, no FEC. Read back from libopus, not from these lines.
    #[test]
    fn the_codec_is_voice_constant_rate_with_no_dtx_and_no_fec() {
        let mut encoder = MicEncoder::new().unwrap();
        let e = encoder.encoder();
        assert_eq!(e.get_application().unwrap(), opus::Application::Voip);
        assert_eq!(
            e.get_bitrate().unwrap(),
            opus::Bitrate::Bits(MIC_BITRATE_BPS)
        );
        assert!(!e.get_vbr().unwrap());
        assert!(!e.get_dtx().unwrap());
        assert!(!e.get_inband_fec().unwrap());
        assert_eq!(e.get_packet_loss_perc().unwrap(), 0);
    }

    /// ★ The shack's own decoder takes what this encodes: each packet is 20 ms at the transmit
    /// route's 12 kHz, and a 1 kHz tone comes out a 1 kHz tone. CONTROL: a frame of the wrong
    /// length is refused, not padded.
    #[test]
    fn the_shacks_decoder_takes_each_frame_as_20_ms_of_voice() {
        let mut encoder = MicEncoder::new().unwrap();
        let mut decoder = MicDecoder::new().unwrap();
        let mut decoded = Vec::new();
        for k in 0..25 {
            let packet = encoder.encode(&tone(1_000.0, 0.5, k)).unwrap();
            let samples = decoder.decode(&packet).expect("the shack refused a frame");
            assert_eq!(samples.len(), FRAME_SAMPLES);
            decoded.extend(samples);
        }
        let tail = &decoded[decoded.len() - 2_400..];
        let crossings = tail
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        let hz = crossings as f32 / 2.0 / (tail.len() as f32 / MIC_RATE_HZ as f32);
        assert!(
            (950.0..1_050.0).contains(&hz),
            "a 1 kHz tone came out {hz:.0} Hz"
        );
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        assert!(rms > 0.2, "the tone came out at RMS {rms:.3}");
        assert_eq!(encoder.encode(&[0.0; MIC_FRAME_SAMPLES - 1]), None);
    }
}
