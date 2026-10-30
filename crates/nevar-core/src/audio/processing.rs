//! Cleans up the mic signal before it is encoded:
//! high-pass filter + echo cancellation + automatic gain (sonora, a Rust port of
//! WebRTC's audio processing), then RNNoise noise suppression (nnnoiseless), which
//! also tells us how likely it is that someone is speaking.

use std::sync::mpsc::Receiver;

use nnnoiseless::DenoiseState;
use sonora::{
    AudioProcessing, Config, StreamConfig,
    config::{AdaptiveDigital, EchoCanceller, GainController2, HighPassFilter},
};

use super::{Frame, SAMPLE_RATE};

/// Both libraries work on 10 ms blocks.
const BLOCK: usize = 480;
/// RNNoise expects samples in 16-bit range.
const I16_SCALE: f32 = 32768.0;
/// Rough mic-to-speaker round trip through Windows audio buffers; AEC3 refines it itself.
const STREAM_DELAY_MS: i32 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessingSettings {
    pub echo_cancellation: bool,
    pub noise_suppression: bool,
    pub auto_gain: bool,
}

impl Default for ProcessingSettings {
    fn default() -> Self {
        Self { echo_cancellation: true, noise_suppression: true, auto_gain: true }
    }
}

pub struct Processor {
    apm: AudioProcessing,
    denoise: Box<DenoiseState<'static>>,
    settings: ProcessingSettings,
    /// What the speakers played, used as the echo reference.
    played: Receiver<Frame>,
    scratch: [f32; BLOCK],
}

impl Processor {
    pub fn new(settings: ProcessingSettings, played: Receiver<Frame>) -> Self {
        let stream = StreamConfig::new(SAMPLE_RATE, 1);
        Self {
            apm: AudioProcessing::builder()
                .config(apm_config(settings))
                .capture_config(stream)
                .render_config(stream)
                .build(),
            denoise: DenoiseState::new(),
            settings,
            played,
            scratch: [0.0; BLOCK],
        }
    }

    /// Applies new settings; cheap to call every frame when nothing changed.
    pub fn update(&mut self, settings: ProcessingSettings) {
        if settings != self.settings {
            self.apm.apply_config(apm_config(settings));
            self.settings = settings;
        }
    }

    /// Processes one mic frame in place. Returns the probability (0..1) that it contains speech.
    pub fn process(&mut self, frame: &mut Frame) -> f32 {
        // Feed everything the speakers played since last time, so AEC knows what echo to expect.
        while let Ok(played) = self.played.try_recv() {
            for block in played.chunks_exact(BLOCK) {
                let _ = self.apm.process_render_f32(&[block], &mut [&mut self.scratch]);
            }
        }

        let mut speech = 0.0f32;
        for block in frame.chunks_exact_mut(BLOCK) {
            let _ = self.apm.set_stream_delay_ms(STREAM_DELAY_MS);
            self.scratch.copy_from_slice(block);
            if self.apm.process_capture_f32(&[&self.scratch], &mut [block]).is_err() {
                block.copy_from_slice(&self.scratch);
            }

            // RNNoise always runs: its speech probability drives voice activation.
            for (s, &x) in self.scratch.iter_mut().zip(block.iter()) {
                *s = x * I16_SCALE;
            }
            let mut cleaned = [0.0f32; BLOCK];
            speech = speech.max(self.denoise.process_frame(&mut cleaned, &self.scratch));
            if self.settings.noise_suppression {
                for (out, &x) in block.iter_mut().zip(cleaned.iter()) {
                    *out = (x / I16_SCALE).clamp(-1.0, 1.0);
                }
            }
        }
        speech
    }
}

fn apm_config(settings: ProcessingSettings) -> Config {
    Config {
        high_pass_filter: Some(HighPassFilter::default()),
        echo_canceller: settings.echo_cancellation.then(EchoCanceller::default),
        // RNNoise handles noise; WebRTC's own suppressor would just double up.
        noise_suppression: None,
        gain_controller2: settings.auto_gain.then(|| GainController2 {
            adaptive_digital: Some(AdaptiveDigital::default()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::sync_channel;

    use super::*;
    use crate::audio::{FRAME_SAMPLES, rms};

    /// Deterministic white noise in [-amp, amp].
    struct Noise(u64, f32);
    impl Noise {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            ((self.0 >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * self.1
        }
        fn frame(&mut self) -> Frame {
            std::array::from_fn(|_| self.next())
        }
    }

    fn db(ratio: f32) -> f32 {
        20.0 * ratio.max(1e-9).log10()
    }

    #[test]
    fn echo_of_the_speakers_is_removed() {
        let (played_tx, played_rx) = sync_channel(64);
        let settings = ProcessingSettings { echo_cancellation: true, noise_suppression: false, auto_gain: false };
        let mut p = Processor::new(settings, played_rx);

        // The speakers play a broadband signal; the mic hears it 30 ms later, quieter.
        let delay = 30 * FRAME_SAMPLES / 20;
        let mut far = Noise(7, 0.3);
        let mut history = vec![0.0f32; delay];
        let (mut input_energy, mut output_energy) = (0.0, 0.0);
        for i in 0..400 {
            let played = far.frame();
            history.extend_from_slice(&played);
            let mut mic: Frame = std::array::from_fn(|j| history[j] * 0.5);
            history.drain(..FRAME_SAMPLES);
            played_tx.send(played).unwrap();

            let before = rms(&mic);
            p.process(&mut mic);
            // Judge only the last 2 s, after the canceller has adapted.
            if i >= 300 {
                input_energy += before;
                output_energy += rms(&mic);
            }
        }
        let reduction = -db(output_energy / input_energy);
        println!("echo reduced by {reduction:.1} dB");
        assert!(reduction > 20.0, "echo only reduced by {reduction:.1} dB");
    }

    #[test]
    fn steady_noise_is_suppressed_and_not_mistaken_for_speech() {
        let (_tx, rx) = sync_channel(1);
        let settings = ProcessingSettings { echo_cancellation: false, noise_suppression: true, auto_gain: false };
        let mut p = Processor::new(settings, rx);
        let mut noise = Noise(42, 0.05);
        let (mut input, mut output, mut max_speech) = (0.0, 0.0, 0.0f32);
        for i in 0..200 {
            let mut frame = noise.frame();
            let before = rms(&frame);
            let speech = p.process(&mut frame);
            if i >= 50 {
                input += before;
                output += rms(&frame);
                max_speech = max_speech.max(speech);
            }
        }
        let reduction = -db(output / input);
        println!("noise reduced by {reduction:.1} dB, max speech probability {max_speech:.2}");
        assert!(reduction > 10.0, "noise only reduced by {reduction:.1} dB");
        assert!(max_speech < 0.5, "noise looked like speech: {max_speech}");
    }

    #[test]
    fn disabled_processing_leaves_signal_alone() {
        let (_tx, rx) = sync_channel(1);
        let settings = ProcessingSettings { echo_cancellation: false, noise_suppression: false, auto_gain: false };
        let mut p = Processor::new(settings, rx);
        let mut ratio = 0.0;
        for i in 0..50 {
            // A 1 kHz tone: well inside the speech band the pipeline keeps (it works at 32 kHz).
            let mut frame: Frame = std::array::from_fn(|j| {
                let t = (i * FRAME_SAMPLES + j) as f32 / SAMPLE_RATE as f32;
                0.2 * (2.0 * std::f32::consts::PI * 1000.0 * t).sin()
            });
            let before = rms(&frame);
            p.process(&mut frame);
            ratio = rms(&frame) / before;
        }
        // Only the high-pass filter remains, which leaves 1 kHz alone.
        assert!((0.8..1.1).contains(&ratio), "level changed: {ratio}");
    }
}
