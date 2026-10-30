pub mod codec;
pub mod device;
pub mod processing;

/// Opus runs natively at 48 kHz; everything inside nevar uses this rate, mono.
pub const SAMPLE_RATE: u32 = 48_000;
/// 20 ms at 48 kHz.
pub const FRAME_SAMPLES: usize = 960;

pub type Frame = [f32; FRAME_SAMPLES];

/// Root-mean-square level of a block of samples (0.0 = silence, ~0.7 = full-scale sine).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub use super::rms;

    /// Frame `index` of a continuous 440 Hz tone at half volume.
    pub fn tone(index: usize) -> Frame {
        let mut frame = [0.0; FRAME_SAMPLES];
        for (i, s) in frame.iter_mut().enumerate() {
            let t = (index * FRAME_SAMPLES + i) as f32 / SAMPLE_RATE as f32;
            *s = 0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
        }
        frame
    }
}
