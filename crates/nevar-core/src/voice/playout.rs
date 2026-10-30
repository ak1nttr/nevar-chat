use std::sync::{
    Arc, Mutex,
    atomic::Ordering,
    mpsc::SyncSender,
};

use tracing::warn;

use super::{
    Controls, Levels,
    jitter::{JitterBuffer, Next},
};
use crate::audio::{FRAME_SAMPLES, Frame, codec::Decoder, rms};

/// Pulls frames out of the jitter buffer and decodes them, at whatever pace the
/// speaker asks for. The speaker's clock is the master clock for playback.
pub struct Playout {
    jitter: Arc<Mutex<JitterBuffer>>,
    decoder: Decoder,
    controls: Arc<Controls>,
    levels: Arc<Levels>,
    /// Receives a copy of everything played, as the echo canceller's reference.
    played: Option<SyncSender<Frame>>,
    frame: Frame,
    pos: usize,
}

impl Playout {
    pub fn new(
        jitter: Arc<Mutex<JitterBuffer>>,
        controls: Arc<Controls>,
        levels: Arc<Levels>,
        played: Option<SyncSender<Frame>>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            jitter,
            decoder: Decoder::new()?,
            controls,
            levels,
            played,
            frame: [0.0; FRAME_SAMPLES],
            pos: FRAME_SAMPLES,
        })
    }

    /// Fills `out` with mono 48 kHz samples.
    pub fn fill(&mut self, out: &mut [f32]) {
        let mut written = 0;
        while written < out.len() {
            if self.pos == FRAME_SAMPLES {
                self.decode_next();
                self.pos = 0;
            }
            let n = (FRAME_SAMPLES - self.pos).min(out.len() - written);
            out[written..written + n].copy_from_slice(&self.frame[self.pos..self.pos + n]);
            self.pos += n;
            written += n;
        }
    }

    fn decode_next(&mut self) {
        let next = self.jitter.lock().unwrap_or_else(|e| e.into_inner()).pop();
        let result = match next {
            Next::Packet(p) => self.decoder.decode(&p, &mut self.frame),
            Next::Recover(p) => self.decoder.recover(&p, &mut self.frame),
            Next::Conceal => self.decoder.conceal(&mut self.frame),
            Next::Silence => {
                self.frame.fill(0.0);
                Ok(())
            }
        };
        if let Err(e) = result {
            warn!("decode failed: {e:#}");
            self.frame.fill(0.0);
        }

        // Still decode while deafened, so the codec and buffer stay in step.
        let volume = if self.controls.deafened() { 0.0 } else { self.controls.output_volume() };
        if volume != 1.0 {
            for s in &mut self.frame {
                *s = (*s * volume).clamp(-1.0, 1.0);
            }
        }
        self.levels.speaker.store(rms(&self.frame).to_bits(), Ordering::Relaxed);
        if let Some(played) = &self.played {
            // If the sender thread is behind, it just misses some reference audio.
            let _ = played.try_send(self.frame);
        }
    }
}
