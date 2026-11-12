use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc::sync_channel,
    },
    time::Duration,
};

use anyhow::Result;
use iroh::{EndpointId, endpoint::Connection};
use tokio::task::JoinHandle;

use super::{
    Controls, Destinations, JitterBuffer, JitterStats, Levels, Playout, Sources, receive, spawn_music_sender,
    spawn_sender,
};
use crate::{
    audio::{
        device::{AudioStream, DeviceChoice, start_capture, start_playback},
        virtual_card::{MicSource, VirtualCard},
    },
    node::{Route, current_path},
};

/// Mic frames allowed to queue for the sender (60 ms) before new ones are dropped.
const CAPTURE_QUEUE: usize = 3;
/// Played frames buffered for the echo canceller (200 ms).
const ECHO_REFERENCE_QUEUE: usize = 10;

/// Which mic and speakers a call uses.
#[derive(Debug, Clone, Default)]
pub struct AudioDevices {
    pub mic: DeviceChoice,
    pub speakers: DeviceChoice,
}

/// One person we're talking with.
struct Link {
    conn: Connection,
    jitter: Arc<Mutex<JitterBuffer>>,
    level: Arc<AtomicU32>,
    /// The music they share, as last played (`f32` bits); zero when they aren't sharing.
    music_level: Arc<AtomicU32>,
    receiver: JoinHandle<()>,
}

/// Feeds shared music into a call: send 20 ms stereo frames in real time. Dropping it
/// stops the sharing.
pub type MusicFeed = std::sync::mpsc::SyncSender<crate::audio::StereoFrame>;

/// Music frames allowed to queue for the encoder (100 ms).
const MUSIC_QUEUE: usize = 5;

/// Live voice with any number of people: our mic goes to all of them, and their
/// voices are mixed on our speakers. People join and leave with [`Call::add_peer`]
/// and [`Call::remove_peer`]; the mic and speakers close when this is dropped.
pub struct Call {
    links: Mutex<HashMap<EndpointId, Link>>,
    sources: Sources,
    destinations: Destinations,
    levels: Arc<Levels>,
    controls: Arc<Controls>,
    /// Numbers our music packets for the whole call (see [`spawn_music_sender`]).
    music_seq: Arc<AtomicU32>,
    io: CallIo,
}

/// Where a call's sound comes from and goes to.
enum CallIo {
    Devices { mic: AudioStream, speakers: AudioStream },
    /// No sound card: the test bot.
    Virtual(#[allow(dead_code, reason = "kept for its Drop")] VirtualCard),
}

#[derive(Debug, Clone)]
pub struct PeerStats {
    pub id: EndpointId,
    pub route: Route,
    pub rtt: Option<Duration>,
    pub jitter: JitterStats,
    /// Their voice as we last played it.
    pub level: f32,
    /// Music they share, as last played; zero when they aren't sharing.
    pub music_level: f32,
}

#[derive(Debug, Clone)]
pub struct CallStats {
    pub mic_level: f32,
    /// Everyone's voices together.
    pub speaker_level: f32,
    /// Our mic is going out right now (voice detected, push-to-talk held, ...).
    pub transmitting: bool,
    pub peers: Vec<PeerStats>,
}

impl Call {
    /// Opens the mic and speakers. Nobody hears anything until people are added.
    /// `controls` can be changed at any time while the call runs.
    pub fn start(devices: &AudioDevices, controls: Arc<Controls>) -> Result<Self> {
        Self::start_with(controls, |playout, frames_tx| {
            let speakers = start_playback(&devices.speakers, playout)?;
            let mic = start_capture(&devices.mic, frames_tx)?;
            Ok(CallIo::Devices { mic, speakers })
        })
    }

    /// Like [`Call::start`] without a sound card: what we hear goes nowhere, and `source`
    /// makes what we say. For the test bot.
    pub fn start_virtual(source: Box<dyn MicSource>, controls: Arc<Controls>) -> Result<Self> {
        Self::start_with(controls, |playout, frames_tx| {
            Ok(CallIo::Virtual(VirtualCard::start(playout, frames_tx, source)?))
        })
    }

    fn start_with(
        controls: Arc<Controls>,
        open: impl FnOnce(Playout, std::sync::mpsc::SyncSender<crate::audio::Frame>) -> Result<CallIo>,
    ) -> Result<Self> {
        let sources = Sources::default();
        let destinations = Destinations::default();
        let levels = Arc::new(Levels::default());

        let (played_tx, played_rx) = sync_channel(ECHO_REFERENCE_QUEUE);
        let playout = Playout::new(sources.clone(), controls.clone(), levels.clone(), Some(played_tx));
        let (frames_tx, frames_rx) = sync_channel(CAPTURE_QUEUE);
        // The sender thread exits on its own once the mic (and its frame sender) is dropped.
        spawn_sender(destinations.clone(), frames_rx, played_rx, controls.clone(), levels.clone())?;
        let io = open(playout, frames_tx)?;

        Ok(Self { links: Mutex::default(), sources, destinations, levels, controls, music_seq: Arc::default(), io })
    }

    /// The mic's and the speakers' names, when real devices are open.
    pub fn device_names(&self) -> Option<(&str, &str)> {
        match &self.io {
            CallIo::Devices { mic, speakers } => Some((&mic.device_name, &speakers.device_name)),
            CallIo::Virtual(_) => None,
        }
    }

    /// Starts talking with whoever is on the other end of `conn`.
    /// Must be called inside a tokio runtime (it spawns the receive task).
    pub fn add_peer(&self, conn: Connection) -> Result<()> {
        let id = conn.remote_id();
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());
        if links.contains_key(&id) {
            return Ok(());
        }
        let jitter = Arc::new(Mutex::new(JitterBuffer::default()));
        let music = Arc::new(Mutex::new(JitterBuffer::for_music()));
        let level = self.sources.add(id, jitter.clone())?;
        let music_level = self.sources.add_music(id, music.clone())?;
        let receiver = tokio::spawn({
            let conn = conn.clone();
            let jitter = jitter.clone();
            async move {
                if let Err(e) = receive(&conn, &jitter, &music).await {
                    tracing::warn!(peer = %conn.remote_id().fmt_short(), "voice receive stopped: {e:#}");
                }
            }
        });
        self.destinations.add(conn.clone());
        links.insert(id, Link { conn, jitter, level, music_level, receiver });
        Ok(())
    }

    /// Starts sharing music with everyone in the call, now and later. Returns where to send
    /// the music; dropping that stops it.
    pub fn share_music(&self) -> Result<MusicFeed> {
        let (feed, frames) = std::sync::mpsc::sync_channel(MUSIC_QUEUE);
        spawn_music_sender(self.destinations.clone(), frames, self.controls.clone(), self.music_seq.clone())?;
        Ok(feed)
    }

    /// For a mic test: plays our own voice back, after the full trip through cleanup,
    /// the codec and a jitter buffer, so we hear what others would.
    pub fn set_loopback(&self, on: bool) {
        // Our own voice gets a mixer slot of its own, under a key nobody else has.
        let me = loopback_id();
        if on {
            let jitter = Arc::new(Mutex::new(JitterBuffer::default()));
            if self.sources.add(me, jitter.clone()).is_ok() {
                self.destinations.set_loopback(Some(jitter));
            }
        } else {
            self.destinations.set_loopback(None);
            self.sources.remove(&me);
        }
    }

    /// Scales one person's voice (1.0 as sent, 0 silent, up to 2.0).
    pub fn set_volume(&self, id: &EndpointId, volume: f32) {
        self.sources.set_volume(id, volume);
    }

    pub fn remove_peer(&self, id: &EndpointId) {
        let Some(link) = self.links.lock().unwrap_or_else(|e| e.into_inner()).remove(id) else { return };
        self.destinations.remove(id);
        self.sources.remove(id);
        link.receiver.abort();
    }

    pub fn stats(&self) -> CallStats {
        let links = self.links.lock().unwrap_or_else(|e| e.into_inner());
        let peers = links
            .iter()
            .map(|(id, link)| {
                let (route, rtt) = current_path(&link.conn);
                PeerStats {
                    id: *id,
                    route,
                    rtt,
                    jitter: link.jitter.lock().unwrap_or_else(|e| e.into_inner()).stats(),
                    level: f32::from_bits(link.level.load(Ordering::Relaxed)),
                    music_level: f32::from_bits(link.music_level.load(Ordering::Relaxed)),
                }
            })
            .collect();
        CallStats {
            mic_level: self.levels.mic(),
            speaker_level: self.levels.speaker(),
            transmitting: self.levels.transmitting(),
            peers,
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        for link in self.links.lock().unwrap_or_else(|e| e.into_inner()).values() {
            link.receiver.abort();
        }
    }
}

/// The mixer slot for our own voice during a mic test. Derived from a fixed key, so it
/// can't collide with a real person.
fn loopback_id() -> EndpointId {
    iroh::SecretKey::from_bytes(&[0x6c; 32]).public()
}
