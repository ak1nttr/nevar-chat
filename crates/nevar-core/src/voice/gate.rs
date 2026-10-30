//! Voice activation: decides from RNNoise's speech probability whether we're talking.

/// Speech probability needed to start transmitting. Steady noise peaks around 0.45.
const OPEN_AT: f32 = 0.75;
/// Once open, anything above this still counts as talking (word gaps, soft endings).
const STAY_OPEN_ABOVE: f32 = 0.4;
/// Quiet frames tolerated before closing (300 ms), so trailing words aren't cut off.
const HANGOVER_FRAMES: u32 = 15;

#[derive(Debug, Default)]
pub struct VoiceGate {
    open: bool,
    quiet_frames: u32,
}

impl VoiceGate {
    /// Feeds one frame's speech probability; returns whether the gate is open.
    pub fn update(&mut self, speech: f32) -> bool {
        if speech >= OPEN_AT {
            self.open = true;
            self.quiet_frames = 0;
        } else if self.open {
            if speech >= STAY_OPEN_ABOVE {
                self.quiet_frames = 0;
            } else {
                self.quiet_frames += 1;
                if self.quiet_frames > HANGOVER_FRAMES {
                    self.open = false;
                }
            }
        }
        self.open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_on_speech_and_holds_through_short_pauses() {
        let mut gate = VoiceGate::default();
        assert!(!gate.update(0.5), "noise-level probability must not open the gate");
        assert!(gate.update(0.9));
        for _ in 0..HANGOVER_FRAMES {
            assert!(gate.update(0.1), "should hold open during the hangover");
        }
        assert!(gate.update(0.6), "medium probability keeps it open");
        for _ in 0..HANGOVER_FRAMES {
            gate.update(0.0);
        }
        assert!(!gate.update(0.0), "closes after the hangover runs out");
    }
}
