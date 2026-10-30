//! The official libopus, fixed to 48 kHz mono, 20 ms frames.

use anyhow::{Result, bail};
use opus::{Application, Bitrate, Channels, Signal};

use super::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

/// Large enough for any single 20 ms frame at our bitrate.
pub const MAX_PACKET: usize = 1275;

const BITRATE: i32 = 32_000;
/// Tells the encoder how much loss to expect, which sizes the error-correction data.
const EXPECTED_LOSS_PERCENT: i32 = 5;

pub struct Encoder(opus::Encoder);

impl Encoder {
    pub fn new() -> Result<Self> {
        let mut enc = opus::Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)?;
        enc.set_bitrate(Bitrate::Bits(BITRATE))?;
        enc.set_signal(Signal::Voice)?;
        enc.set_inband_fec(true)?;
        enc.set_packet_loss_perc(EXPECTED_LOSS_PERCENT)?;
        Ok(Self(enc))
    }

    /// Encodes one frame into `out`, returning the packet length.
    pub fn encode(&mut self, pcm: &Frame, out: &mut [u8]) -> Result<usize> {
        Ok(self.0.encode_float(pcm, out)?)
    }
}

pub struct Decoder(opus::Decoder);

impl Decoder {
    pub fn new() -> Result<Self> {
        Ok(Self(opus::Decoder::new(SAMPLE_RATE, Channels::Mono)?))
    }

    /// Decodes one frame into `out`.
    pub fn decode(&mut self, packet: &[u8], out: &mut Frame) -> Result<()> {
        self.decode_raw(packet, false, out)
    }

    /// Rebuilds a lost frame from the error-correction data in the packet that followed it.
    pub fn recover(&mut self, next_packet: &[u8], out: &mut Frame) -> Result<()> {
        self.decode_raw(next_packet, true, out)
    }

    /// Synthesizes a plausible frame when we have nothing (packet loss concealment).
    /// Repeated calls fade out to silence.
    pub fn conceal(&mut self, out: &mut Frame) -> Result<()> {
        self.decode_raw(&[], false, out)
    }

    fn decode_raw(&mut self, packet: &[u8], fec: bool, out: &mut Frame) -> Result<()> {
        let n = self.0.decode_float(packet, out, fec)?;
        if n > FRAME_SAMPLES {
            bail!("decoded {n} samples into a {FRAME_SAMPLES}-sample frame");
        }
        // A packet shorter than our frame size leaves the rest silent.
        out[n..].fill(0.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::tests::{rms, tone};

    #[test]
    fn roundtrip_keeps_signal() {
        let mut enc = Encoder::new().unwrap();
        let mut dec = Decoder::new().unwrap();
        let mut packet = [0u8; MAX_PACKET];
        let mut out = [0f32; FRAME_SAMPLES];
        // The codec needs a few frames to settle.
        for i in 0..10 {
            let len = enc.encode(&tone(i), &mut packet).unwrap();
            assert!(len < 200, "packet unexpectedly large: {len}");
            dec.decode(&packet[..len], &mut out).unwrap();
        }
        let ratio = rms(&out) / rms(&tone(9));
        assert!((0.7..1.3).contains(&ratio), "level changed too much: {ratio}");
    }

    #[test]
    fn lost_frame_is_recovered_from_the_next_packet() {
        let mut enc = Encoder::new().unwrap();
        let mut dec = Decoder::new().unwrap();
        let mut packets = Vec::new();
        let mut buf = [0u8; MAX_PACKET];
        for i in 0..12 {
            let len = enc.encode(&tone(i), &mut buf).unwrap();
            packets.push(buf[..len].to_vec());
        }
        let mut out = [0f32; FRAME_SAMPLES];
        for p in &packets[..10] {
            dec.decode(p, &mut out).unwrap();
        }
        // Frame 10 is "lost"; rebuild it from frame 11.
        dec.recover(&packets[11], &mut out).unwrap();
        let ratio = rms(&out) / rms(&tone(10));
        assert!((0.5..1.3).contains(&ratio), "recovered level off: {ratio}");
    }

    #[test]
    fn concealment_continues_then_fades() {
        let mut enc = Encoder::new().unwrap();
        let mut dec = Decoder::new().unwrap();
        let mut packet = [0u8; MAX_PACKET];
        let mut out = [0f32; FRAME_SAMPLES];
        for i in 0..10 {
            let len = enc.encode(&tone(i), &mut packet).unwrap();
            dec.decode(&packet[..len], &mut out).unwrap();
        }
        let before = rms(&out);
        dec.conceal(&mut out).unwrap();
        let first = rms(&out);
        assert!(first > 0.5 * before, "first concealed frame should continue the sound: {first}");
        assert!(first < 1.2 * before, "concealment must never get louder: {first}");
        let mut last = first;
        for _ in 0..5 {
            dec.conceal(&mut out).unwrap();
            assert!(rms(&out) <= last + 0.01, "concealment should keep fading");
            last = rms(&out);
        }
        assert!(last < 0.5 * first, "concealment should fade out: {last}");
    }
}
