use std::sync::{
    Arc, Mutex, MutexGuard,
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
use crate::audio::{
    FRAME_SAMPLES, Frame, STEREO_SAMPLES, StereoFrame,
    codec::{Decoder, MusicDecoder},
    downmix, rms,
};

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

/// Music someone shares, on its way to the speakers.
struct MusicSource {
    id: EndpointId,
    jitter: Arc<Mutex<JitterBuffer>>,
    decoder: MusicDecoder,
    /// Its level as last played, `f32` bits. Above zero while they're sharing.
    level: Arc<AtomicU32>,
}

#[derive(Default)]
struct Mix {
    voices: Vec<Source>,
    music: Vec<MusicSource>,
}

/// The voices (and shared music) being played. The call adds and removes them; the
/// speaker callback mixes them.
#[derive(Clone, Default)]
pub struct Sources(Arc<Mutex<Mix>>);

impl Sources {
    fn lock(&self) -> MutexGuard<'_, Mix> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts playing whatever arrives in `jitter`. Returns that voice's live level (`f32` bits).
    pub fn add(&self, id: EndpointId, jitter: Arc<Mutex<JitterBuffer>>) -> Result<Arc<AtomicU32>> {
        let level = Arc::new(AtomicU32::new(0));
        let source = Source { id, jitter, decoder: Decoder::new()?, level: level.clone(), volume: 1.0 };
        let mut mix = self.lock();
        mix.voices.retain(|s| s.id != id);
        mix.voices.push(source);
        Ok(level)
    }

    /// Plays the music `id` shares, from `jitter`. Returns its live level (`f32` bits).
    pub fn add_music(&self, id: EndpointId, jitter: Arc<Mutex<JitterBuffer>>) -> Result<Arc<AtomicU32>> {
        let level = Arc::new(AtomicU32::new(0));
        let source = MusicSource { id, jitter, decoder: MusicDecoder::new()?, level: level.clone() };
        let mut mix = self.lock();
        mix.music.retain(|s| s.id != id);
        mix.music.push(source);
        Ok(level)
    }

    /// Stops playing `id`'s voice and music.
    pub fn remove(&self, id: &EndpointId) {
        let mut mix = self.lock();
        mix.voices.retain(|s| s.id != *id);
        mix.music.retain(|s| s.id != *id);
    }

    /// Scales one voice: 1.0 as sent, 0 silent, up to 2.0.
    pub fn set_volume(&self, id: &EndpointId, volume: f32) {
        let volume = volume.clamp(0.0, 2.0);
        for s in self.lock().voices.iter_mut().filter(|s| s.id == *id) {
            s.volume = volume;
        }
    }
}

/// Pulls a frame from every voice's (and music's) jitter buffer, decodes and mixes them,
/// at whatever pace the speaker asks for. The speaker's clock is the master clock for playback.
pub struct Playout {
    sources: Sources,
    controls: Arc<Controls>,
    levels: Arc<Levels>,
    /// Receives a mono copy of everything played, as the echo canceller's reference.
    played: Option<SyncSender<Frame>>,
    frame: StereoFrame,
    pos: usize,
}

impl Playout {
    pub fn new(
        sources: Sources,
        controls: Arc<Controls>,
        levels: Arc<Levels>,
        played: Option<SyncSender<Frame>>,
    ) -> Self {
        Self { sources, controls, levels, played, frame: [0.0; STEREO_SAMPLES], pos: STEREO_SAMPLES }
    }

    /// Fills `out` with stereo 48 kHz samples, interleaved left/right.
    pub fn fill(&mut self, out: &mut [f32]) {
        let mut written = 0;
        while written < out.len() {
            if self.pos == STEREO_SAMPLES {
                self.mix_next();
                self.pos = 0;
            }
            let n = (STEREO_SAMPLES - self.pos).min(out.len() - written);
            out[written..written + n].copy_from_slice(&self.frame[self.pos..self.pos + n]);
            self.pos += n;
            written += n;
        }
    }

    fn mix_next(&mut self) {
        // Still decode while deafened, so codecs and buffers stay in step.
        let deafened = self.controls.deafened();
        let volume = if deafened { 0.0 } else { self.controls.output_volume() };
        let music_volume = if deafened { 0.0 } else { self.controls.music_volume() };
        let mut voices: Frame = [0.0; FRAME_SAMPLES];
        let mut voice: Frame = [0.0; FRAME_SAMPLES];
        self.frame.fill(0.0);

        let mut mix = self.sources.lock();
        for source in mix.voices.iter_mut() {
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
            for (mixed, s) in voices.iter_mut().zip(voice.iter()) {
                *mixed += s * volume;
            }
        }

        let mut music: StereoFrame = [0.0; STEREO_SAMPLES];
        for source in mix.music.iter_mut() {
            let next = source.jitter.lock().unwrap_or_else(|e| e.into_inner()).pop();
            let result = match next {
                Next::Packet(p) => source.decoder.decode(&p, &mut music),
                // Music packets carry no error-correction data: conceal instead.
                Next::Recover(_) | Next::Conceal => source.decoder.conceal(&mut music),
                Next::Silence => {
                    music.fill(0.0);
                    Ok(())
                }
            };
            if let Err(e) = result {
                warn!("music decode failed: {e:#}");
                music.fill(0.0);
            }
            source.level.store(rms(&music).to_bits(), Ordering::Relaxed);
            for (mixed, s) in self.frame.iter_mut().zip(music.iter()) {
                *mixed += s * music_volume;
            }
        }
        drop(mix);

        // Voices sit in the middle; music keeps its stereo.
        for (lr, v) in self.frame.chunks_exact_mut(2).zip(voices.iter()) {
            lr[0] = (lr[0] + v).clamp(-1.0, 1.0);
            lr[1] = (lr[1] + v).clamp(-1.0, 1.0);
        }
        let mono = downmix(&self.frame);
        self.levels.speaker.store(rms(&mono).to_bits(), Ordering::Relaxed);
        if let Some(played) = &self.played {
            // If the sender thread is behind, it just misses some reference audio.
            let _ = played.try_send(mono);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::codec::{Encoder, MAX_PACKET, MusicEncoder};
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

    /// A jitter buffer holding `frames` of encoded music: a tone on the left only.
    fn music_left(hz: f32, frames: usize) -> Arc<Mutex<JitterBuffer>> {
        let mut encoder = MusicEncoder::new().unwrap();
        let mut jb = JitterBuffer::for_music();
        let mut buf = [0u8; MAX_PACKET];
        for i in 0..frames {
            let mut stereo = [0.0; STEREO_SAMPLES];
            for (lr, s) in stereo.chunks_exact_mut(2).zip(tone(hz, i)) {
                lr[0] = s;
            }
            let len = encoder.encode(&stereo, &mut buf).unwrap();
            jb.push(i as u32, Bytes::copy_from_slice(&buf[..len]), false);
        }
        Arc::new(Mutex::new(jb))
    }

    fn id(n: u8) -> EndpointId {
        iroh::SecretKey::from_bytes(&[n; 32]).public()
    }

    fn channel(out: &[f32], which: usize) -> Vec<f32> {
        out.iter().skip(which).step_by(2).copied().collect()
    }

    #[test]
    fn mixes_every_voice_and_reports_each_level() {
        let sources = Sources::default();
        let a = sources.add(id(1), voice(300.0, 40)).unwrap();
        let b = sources.add(id(2), voice(700.0, 40)).unwrap();
        let levels = Arc::new(Levels::default());
        let mut playout = Playout::new(sources.clone(), Arc::new(Controls::default()), levels.clone(), None);

        // A few frames only: the jitter buffer trims a big backlog, so the voices run dry soon after.
        let mut out = vec![0.0; STEREO_SAMPLES * 3];
        playout.fill(&mut out);

        let a = f32::from_bits(a.load(Ordering::Relaxed));
        let b = f32::from_bits(b.load(Ordering::Relaxed));
        assert!(a > 0.05 && b > 0.05, "each voice should be heard: {a} {b}");
        // Two uncorrelated tones add up louder than either one.
        assert!(levels.speaker() > a.max(b), "mix {} vs {a} {b}", levels.speaker());
        // Voices play in the middle: the same on both sides.
        assert_eq!(channel(&out, 0), channel(&out, 1));

        // A voice turned down to zero goes quiet; the other carries on.
        sources.set_volume(&id(1), 0.0);
        playout.fill(&mut out[..STEREO_SAMPLES]);
        assert_eq!(f32::from_bits(sources.lock().voices[0].level.load(Ordering::Relaxed)), 0.0);
        assert!(f32::from_bits(sources.lock().voices[1].level.load(Ordering::Relaxed)) > 0.05);

        // A removed voice stops contributing.
        sources.remove(&id(2));
        assert_eq!(sources.lock().voices.len(), 1);
    }

    #[test]
    fn music_keeps_its_stereo_and_its_own_volume() {
        let sources = Sources::default();
        let level = sources.add_music(id(3), music_left(440.0, 40)).unwrap();
        let controls = Arc::new(Controls::default());
        controls.set_music_volume(1.0);
        let mut playout = Playout::new(sources.clone(), controls.clone(), Arc::new(Levels::default()), None);

        let mut out = vec![0.0; STEREO_SAMPLES * 8];
        playout.fill(&mut out);
        let last = &out[STEREO_SAMPLES * 7..];
        assert!(rms(&channel(last, 0)) > 0.08, "music missing on the left");
        assert!(rms(&channel(last, 1)) < 0.02, "music leaked to the right");
        assert!(f32::from_bits(level.load(Ordering::Relaxed)) > 0.0);

        // Turning music down doesn't touch its level meter, only what's played.
        controls.set_music_volume(0.0);
        playout.fill(&mut out[..STEREO_SAMPLES]);
        assert!(out[..STEREO_SAMPLES].iter().all(|&s| s == 0.0));
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
