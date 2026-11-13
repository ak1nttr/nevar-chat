//! Live call settings, shared between the UI and the audio threads.
//! Everything is atomic so audio threads never wait on the UI.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering::Relaxed};

/// Shared music plays a bit under the voices unless turned up.
pub const DEFAULT_MUSIC_VOLUME: f32 = 0.6;

/// When the mic is transmitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum TransmitMode {
    /// Send while speech is detected.
    #[default]
    VoiceActivity = 0,
    /// Send only while the push-to-talk key is held.
    PushToTalk = 1,
    /// Always send.
    Open = 2,
}

#[derive(Debug)]
pub struct Controls {
    muted: AtomicBool,
    deafened: AtomicBool,
    push_to_talk_held: AtomicBool,
    mode: AtomicU8,
    noise_suppression: AtomicBool,
    echo_cancellation: AtomicBool,
    auto_gain: AtomicBool,
    /// Playback volume multiplier, as `f32` bits.
    output_volume: AtomicU32,
    /// Simulated bad network on our outgoing voice (test bot only): percent of packets
    /// dropped, and up to how many ms each packet is held back.
    sim_loss_percent: AtomicU32,
    sim_jitter_ms: AtomicU32,
    /// How loud music others share plays for us, as `f32` bits.
    music_volume: AtomicU32,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            muted: AtomicBool::new(false),
            deafened: AtomicBool::new(false),
            push_to_talk_held: AtomicBool::new(false),
            mode: AtomicU8::new(TransmitMode::default() as u8),
            noise_suppression: AtomicBool::new(true),
            echo_cancellation: AtomicBool::new(true),
            auto_gain: AtomicBool::new(true),
            output_volume: AtomicU32::new(1.0f32.to_bits()),
            sim_loss_percent: AtomicU32::new(0),
            sim_jitter_ms: AtomicU32::new(0),
            music_volume: AtomicU32::new(DEFAULT_MUSIC_VOLUME.to_bits()),
        }
    }
}

impl Controls {
    pub fn muted(&self) -> bool {
        self.muted.load(Relaxed)
    }
    pub fn set_muted(&self, on: bool) {
        self.muted.store(on, Relaxed);
    }

    /// Deafened means hearing nothing, and (like everywhere else) also muted.
    pub fn deafened(&self) -> bool {
        self.deafened.load(Relaxed)
    }
    pub fn set_deafened(&self, on: bool) {
        self.deafened.store(on, Relaxed);
    }

    pub fn push_to_talk_held(&self) -> bool {
        self.push_to_talk_held.load(Relaxed)
    }
    pub fn set_push_to_talk_held(&self, held: bool) {
        self.push_to_talk_held.store(held, Relaxed);
    }

    pub fn mode(&self) -> TransmitMode {
        match self.mode.load(Relaxed) {
            1 => TransmitMode::PushToTalk,
            2 => TransmitMode::Open,
            _ => TransmitMode::VoiceActivity,
        }
    }
    pub fn set_mode(&self, mode: TransmitMode) {
        self.mode.store(mode as u8, Relaxed);
    }

    pub fn noise_suppression(&self) -> bool {
        self.noise_suppression.load(Relaxed)
    }
    pub fn set_noise_suppression(&self, on: bool) {
        self.noise_suppression.store(on, Relaxed);
    }

    pub fn echo_cancellation(&self) -> bool {
        self.echo_cancellation.load(Relaxed)
    }
    pub fn set_echo_cancellation(&self, on: bool) {
        self.echo_cancellation.store(on, Relaxed);
    }

    pub fn auto_gain(&self) -> bool {
        self.auto_gain.load(Relaxed)
    }
    pub fn set_auto_gain(&self, on: bool) {
        self.auto_gain.store(on, Relaxed);
    }

    /// 1.0 is unchanged; up to 2.0 boosts quiet friends.
    pub fn output_volume(&self) -> f32 {
        f32::from_bits(self.output_volume.load(Relaxed))
    }
    pub fn set_output_volume(&self, volume: f32) {
        self.output_volume.store(volume.clamp(0.0, 2.0).to_bits(), Relaxed);
    }

    /// Music sits under the voices by default: 0 silent, 1 as shared.
    pub fn music_volume(&self) -> f32 {
        f32::from_bits(self.music_volume.load(Relaxed))
    }
    pub fn set_music_volume(&self, volume: f32) {
        self.music_volume.store(volume.clamp(0.0, 1.0).to_bits(), Relaxed);
    }

    /// Simulated network trouble for outgoing voice: (percent lost, max extra delay in ms).
    pub fn simulated_network(&self) -> (u32, u32) {
        (self.sim_loss_percent.load(Relaxed), self.sim_jitter_ms.load(Relaxed))
    }
    pub fn set_simulated_network(&self, loss_percent: u32, jitter_ms: u32) {
        self.sim_loss_percent.store(loss_percent.min(100), Relaxed);
        self.sim_jitter_ms.store(jitter_ms.min(2000), Relaxed);
    }
}
