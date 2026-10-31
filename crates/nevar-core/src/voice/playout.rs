use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
    mpsc::SyncSender,
};

use anyhow::Result;
use iroh::EndpointId;
use tracing::warn;

use super::{
    Controls, Levels,
    jitter::{JitterBuffer, Next},
};
use crate::audio::{FRAME_SAMPLES, Frame, codec::Decoder, rms};

/// One person's voice on its way to the speakers.
struct Source {
    id: EndpointId,
    jitter: Arc<Mutex<JitterBuffer>>,
    decoder: Decoder,
    /// Their level as last played, stored as `f32` bits.
    level: Arc<AtomicU32>,
    /// How loud we want them, set per person.
    volume: f32,
}

/// The voices being played. The call adds and removes them; the speaker callback mixes them.
#[derive(Clone, Default)]
pub struct Sources(Arc<Mutex<Vec<Source>>>);

impl Sources {
    /// Starts playing whatever arrives in `jitter`. Returns that voice's live level (`f32` bits).
    pub fn add(&self, id: EndpointId, jitter: Arc<Mutex<JitterBuffer>>) -> Result<Arc<AtomicU32>> {
        let level = Arc::new(AtomicU32::new(0));
        let source = Source { id, jitter, decoder: Decoder::new()?, level: level.clone(), volume: 1.0 };
        let mut sources = self.0.lock().unwrap_or_else(|e| e.into_inner());
        sources.retain(|s| s.id != id);
        sources.push(source);
        Ok(level)
    }

    pub fn remove(&self, id: &EndpointId) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).retain(|s| s.id != *id);
    }

    /// Scales one voice: 1.0 as sent, 0 silent, up to 2.0.
    pub fn set_volume(&self, id: &EndpointId, volume: f32) {
        let volume = volume.clamp(0.0, 2.0);
        for s in self.0.lock().unwrap_or_else(|e| e.into_inner()).iter_mut().filter(|s| s.id == *id) {
            s.volume = volume;
        }
    }
}

/// Pulls a frame from every voice's jitter buffer, decodes and mixes them, at whatever
/// pace the speaker asks for. The speaker's clock is the master clock for playback.
pub struct Playout {
    sources: Sources,
    controls: Arc<Controls>,
    levels: Arc<Levels>,
    /// Receives a copy of everything played, as the echo canceller's reference.
    played: Option<SyncSender<Frame>>,
    frame: Frame,
    pos: usize,
}

impl Playout {
    pub fn new(
        sources: Sources,
        controls: Arc<Controls>,
        levels: Arc<Levels>,
        played: Option<SyncSender<Frame>>,
    ) -> Self {
        Self { sources, controls, levels, played, frame: [0.0; FRAME_SAMPLES], pos: FRAME_SAMPLES }
    }

    /// Fills `out` with mono 48 kHz samples.
    pub fn fill(&mut self, out: &mut [f32]) {
        let mut written = 0;
        while written < out.len() {
            if self.pos == FRAME_SAMPLES {
                self.mix_next();
                self.pos = 0;
            }
            let n = (FRAME_SAMPLES - self.pos).min(out.len() - written);
            out[written..written + n].copy_from_slice(&self.frame[self.pos..self.pos + n]);
            self.pos += n;
            written += n;
        }
    }

    fn mix_next(&mut self) {
        // Still decode while deafened, so codecs and buffers stay in step.
        let volume = if self.controls.deafened() { 0.0 } else { self.controls.output_volume() };
        self.frame.fill(0.0);
        let mut voice: Frame = [0.0; FRAME_SAMPLES];

        let mut sources = self.sources.0.lock().unwrap_or_else(|e| e.into_inner());
        for source in sources.iter_mut() {
            let next = source.jitter.lock().unwrap_or_else(|e| e.into_inner()).pop();
            let result = match next {
                Next::Packet(p) => source.decoder.decode(&p, &mut voice),
                Next::Recover(p) => source.decoder.recover(&p, &mut voice),
                Next::Conceal => source.decoder.conceal(&mut voice),
                Next::Silence => {
                    voice.fill(0.0);
                    Ok(())
                }
            };
            if let Err(e) = result {
                warn!("decode failed: {e:#}");
                voice.fill(0.0);
            }
            if source.volume != 1.0 {
                for s in &mut voice {
                    *s *= source.volume;
                }
            }
            source.level.store((rms(&voice) * volume).to_bits(), Ordering::Relaxed);
            for (mixed, s) in self.frame.iter_mut().zip(voice.iter()) {
                *mixed += s;
            }
        }
        drop(sources);

        for s in &mut self.frame {
            *s = (*s * volume).clamp(-1.0, 1.0);
        }
        self.levels.speaker.store(rms(&self.frame).to_bits(), Ordering::Relaxed);
        if let Some(played) = &self.played {
            // If the sender thread is behind, it just misses some reference audio.
            let _ = played.try_send(self.frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{codec::Encoder, codec::MAX_PACKET};
    use bytes::Bytes;

    fn tone(hz: f32, index: usize) -> Frame {
        let mut frame = [0.0; FRAME_SAMPLES];
        for (i, s) in frame.iter_mut().enumerate() {
            let t = (index * FRAME_SAMPLES + i) as f32 / 48_000.0;
            *s = 0.2 * (2.0 * std::f32::consts::PI * hz * t).sin();
        }
        frame
    }

    /// A jitter buffer holding `frames` of an encoded tone.
    fn voice(hz: f32, frames: usize) -> Arc<Mutex<JitterBuffer>> {
        let mut encoder = Encoder::new().unwrap();
        let mut jb = JitterBuffer::default();
        let mut buf = [0u8; MAX_PACKET];
        for i in 0..frames {
            let len = encoder.encode(&tone(hz, i), &mut buf).unwrap();
            jb.push(i as u32, Bytes::copy_from_slice(&buf[..len]), false);
        }
        Arc::new(Mutex::new(jb))
    }

    fn id(n: u8) -> EndpointId {
        iroh::SecretKey::from_bytes(&[n; 32]).public()
    }

    #[test]
    fn mixes_every_voice_and_reports_each_level() {
        let sources = Sources::default();
        let a = sources.add(id(1), voice(300.0, 40)).unwrap();
        let b = sources.add(id(2), voice(700.0, 40)).unwrap();
        let levels = Arc::new(Levels::default());
        let mut playout = Playout::new(sources.clone(), Arc::new(Controls::default()), levels.clone(), None);

        // A few frames only: the jitter buffer trims a big backlog, so the voices run dry soon after.
        let mut out = vec![0.0; FRAME_SAMPLES * 3];
        playout.fill(&mut out);

        let a = f32::from_bits(a.load(Ordering::Relaxed));
        let b = f32::from_bits(b.load(Ordering::Relaxed));
        assert!(a > 0.05 && b > 0.05, "each voice should be heard: {a} {b}");
        // Two uncorrelated tones add up louder than either one.
        assert!(levels.speaker() > a.max(b), "mix {} vs {a} {b}", levels.speaker());

        // A voice turned down to zero goes quiet; the other carries on.
        sources.set_volume(&id(1), 0.0);
        playout.fill(&mut out[..FRAME_SAMPLES]);
        assert_eq!(f32::from_bits(sources.0.lock().unwrap()[0].level.load(Ordering::Relaxed)), 0.0);
        assert!(f32::from_bits(sources.0.lock().unwrap()[1].level.load(Ordering::Relaxed)) > 0.05);

        // A removed voice stops contributing.
        sources.remove(&id(2));
        assert_eq!(sources.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn plays_silence_with_nobody_in_the_call() {
        let mut playout =
            Playout::new(Sources::default(), Arc::new(Controls::default()), Arc::new(Levels::default()), None);
        let mut out = vec![1.0; 500];
        playout.fill(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
    }
}
