//! Voice between everyone in a call, over one iroh connection per person (a mesh).
//!
//! Mic frames are cleaned up, gated and encoded once on a dedicated thread, and the
//! same datagram goes to every connection in [`Destinations`]. Each person's incoming
//! datagrams land in their own jitter buffer; the speaker callback decodes and mixes
//! them all through [`Playout`].

pub mod call;
pub mod controls;
pub mod gate;
pub mod jitter;
pub mod playout;

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::Receiver,
    },
    thread::JoinHandle,
};

use anyhow::Result;
use bytes::Bytes;
use iroh::{
    EndpointId,
    endpoint::{Connection, SendDatagramError},
};
use tracing::{debug, warn};

pub use call::{AudioDevices, Call, CallStats, PeerStats};
pub use controls::{Controls, TransmitMode};
pub use jitter::{JitterBuffer, JitterStats};
pub use playout::{Playout, Sources};

use crate::{
    audio::{
        Frame,
        codec::{Encoder, MAX_PACKET},
        processing::{ProcessingSettings, Processor},
        rms,
    },
    probe,
    protocol::{FLAG_END_OF_SPEECH, PROBE_REQUEST, VOICE, VOICE_HEADER},
};
use gate::VoiceGate;

/// Frames kept from just before speech is detected (40 ms), so first syllables aren't clipped.
const PREROLL_FRAMES: usize = 2;

/// Live audio levels for meters, stored as `f32` bits.
#[derive(Debug, Default)]
pub struct Levels {
    mic: AtomicU32,
    speaker: AtomicU32,
    transmitting: AtomicBool,
}

impl Levels {
    /// Level of the cleaned-up mic signal, whether or not it is being sent.
    pub fn mic(&self) -> f32 {
        f32::from_bits(self.mic.load(Ordering::Relaxed))
    }

    pub fn speaker(&self) -> f32 {
        f32::from_bits(self.speaker.load(Ordering::Relaxed))
    }

    /// Whether our mic is currently going out to the other side.
    pub fn transmitting(&self) -> bool {
        self.transmitting.load(Ordering::Relaxed)
    }
}

/// Where our voice goes: one connection per person in the call.
#[derive(Clone, Default)]
pub struct Destinations(Arc<Mutex<Vec<Connection>>>);

impl Destinations {
    pub fn add(&self, conn: Connection) {
        let mut conns = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let id = conn.remote_id();
        conns.retain(|c| c.remote_id() != id);
        conns.push(conn);
    }

    pub fn remove(&self, id: &EndpointId) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).retain(|c| c.remote_id() != *id);
    }

    fn send(&self, packet: Bytes) {
        for conn in self.0.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            match conn.send_datagram(packet.clone()) {
                Ok(()) => {}
                // The service notices the dropped connection and takes it out of the call.
                Err(SendDatagramError::ConnectionLost(_)) => {
                    debug!(peer = %conn.remote_id().fmt_short(), "voice to a closed connection")
                }
                Err(e) => warn!("send failed: {e}"),
            }
        }
    }
}

impl From<Connection> for Destinations {
    fn from(conn: Connection) -> Self {
        let d = Self::default();
        d.add(conn);
        d
    }
}

/// Processes, gates, encodes and sends mic frames until the frame source closes.
/// `played` carries what the speakers played, for echo cancellation.
pub fn spawn_sender(
    destinations: Destinations,
    frames: Receiver<Frame>,
    played: Receiver<Frame>,
    controls: Arc<Controls>,
    levels: Arc<Levels>,
) -> Result<JoinHandle<()>> {
    let mut encoder = Encoder::new()?;
    let handle = std::thread::Builder::new()
        .name("voice-sender".into())
        .spawn(move || {
            let mut processor = Processor::new(processing_settings(&controls), played);
            let mut gate = VoiceGate::default();
            let mut recent: VecDeque<Vec<u8>> = VecDeque::with_capacity(PREROLL_FRAMES + 1);
            let mut sending = false;
            let mut seq: u32 = 0;
            let mut buf = [0u8; MAX_PACKET];

            while let Ok(mut frame) = frames.recv() {
                processor.update(processing_settings(&controls));
                let speech = processor.process(&mut frame);
                levels.mic.store(rms(&frame).to_bits(), Ordering::Relaxed);

                // Always encode, so the codec state stays continuous and pre-roll is ready.
                let len = match encoder.encode(&frame, &mut buf) {
                    Ok(len) => len,
                    Err(e) => {
                        warn!("encode failed: {e:#}");
                        continue;
                    }
                };
                if recent.len() > PREROLL_FRAMES {
                    recent.pop_front();
                }
                recent.push_back(buf[..len].to_vec());

                let gate_open = gate.update(speech);
                let wants_to_send = !controls.muted()
                    && !controls.deafened()
                    && match controls.mode() {
                        TransmitMode::VoiceActivity => gate_open,
                        TransmitMode::PushToTalk => controls.push_to_talk_held(),
                        TransmitMode::Open => true,
                    };

                let to_send: Vec<(&[u8], u8)> = match (sending, wants_to_send) {
                    // Just started talking: the pre-roll, then this frame.
                    (false, true) => recent.iter().map(|p| (p.as_slice(), 0)).collect(),
                    (true, true) => vec![(recent.back().unwrap().as_slice(), 0)],
                    // Just stopped: this frame closes the talk spurt.
                    (true, false) => vec![(recent.back().unwrap().as_slice(), FLAG_END_OF_SPEECH)],
                    (false, false) => vec![],
                };
                sending = wants_to_send;
                levels.transmitting.store(sending, Ordering::Relaxed);

                for (opus, flags) in to_send {
                    let mut packet = Vec::with_capacity(VOICE_HEADER + opus.len());
                    packet.push(VOICE);
                    packet.extend_from_slice(&seq.to_le_bytes());
                    packet.push(flags);
                    packet.extend_from_slice(opus);
                    seq = seq.wrapping_add(1);
                    destinations.send(packet.into());
                }
            }
        })?;
    Ok(handle)
}

fn processing_settings(controls: &Controls) -> ProcessingSettings {
    ProcessingSettings {
        echo_cancellation: controls.echo_cancellation(),
        noise_suppression: controls.noise_suppression(),
        auto_gain: controls.auto_gain(),
    }
}

/// Routes incoming datagrams until the connection closes: voice into the jitter
/// buffer, probes straight back to the sender.
pub async fn receive(conn: &Connection, jitter: &Mutex<JitterBuffer>) -> Result<()> {
    while let Ok(datagram) = conn.read_datagram().await {
        match datagram.first() {
            Some(&VOICE) if datagram.len() > VOICE_HEADER => {
                let seq = u32::from_le_bytes(datagram[1..5].try_into()?);
                let end_of_speech = datagram[5] & FLAG_END_OF_SPEECH != 0;
                let payload = datagram.slice(VOICE_HEADER..);
                jitter.lock().unwrap_or_else(|e| e.into_inner()).push(seq, payload, end_of_speech);
            }
            Some(&PROBE_REQUEST) => probe::reply(conn, &datagram)?,
            _ => {}
        }
    }
    Ok(())
}
