use std::{
    sync::{Arc, Mutex, mpsc::sync_channel},
    time::Duration,
};

use anyhow::Result;
use iroh::endpoint::Connection;
use tokio::task::JoinHandle;

use super::{Controls, JitterBuffer, JitterStats, Levels, Playout, receive, spawn_sender};
use crate::{
    audio::device::{AudioStream, DeviceChoice, start_capture, start_playback},
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

/// A live voice call with one peer.
/// Hangs up when dropped.
pub struct Call {
    conn: Connection,
    jitter: Arc<Mutex<JitterBuffer>>,
    levels: Arc<Levels>,
    receiver: JoinHandle<()>,
    pub mic: AudioStream,
    pub speakers: AudioStream,
}

#[derive(Debug, Clone, Copy)]
pub struct CallStats {
    pub route: Route,
    pub rtt: Option<Duration>,
    pub jitter: JitterStats,
    pub mic_level: f32,
    pub speaker_level: f32,
    /// Our mic is going out right now (voice detected, push-to-talk held, ...).
    pub transmitting: bool,
}

impl Call {
    /// Starts sending and receiving audio on an established connection.
    /// `controls` can be changed at any time while the call runs.
    pub fn start(conn: Connection, devices: &AudioDevices, controls: Arc<Controls>) -> Result<Self> {
        let jitter = Arc::new(Mutex::new(JitterBuffer::default()));
        let levels = Arc::new(Levels::default());

        let (played_tx, played_rx) = sync_channel(ECHO_REFERENCE_QUEUE);
        let playout = Playout::new(jitter.clone(), controls.clone(), levels.clone(), Some(played_tx))?;
        let speakers = start_playback(&devices.speakers, playout)?;

        let (frames_tx, frames_rx) = sync_channel(CAPTURE_QUEUE);
        // The sender thread exits on its own once the mic stream (and its sender) is dropped.
        spawn_sender(conn.clone(), frames_rx, played_rx, controls, levels.clone())?;
        let mic = start_capture(&devices.mic, frames_tx)?;

        let receiver = tokio::spawn({
            let conn = conn.clone();
            let jitter = jitter.clone();
            async move {
                if let Err(e) = receive(&conn, &jitter).await {
                    tracing::warn!("voice receive stopped: {e:#}");
                }
            }
        });

        Ok(Self { conn, jitter, levels, receiver, mic, speakers })
    }

    pub fn stats(&self) -> CallStats {
        let (route, rtt) = current_path(&self.conn);
        CallStats {
            route,
            rtt,
            jitter: self.jitter.lock().unwrap_or_else(|e| e.into_inner()).stats(),
            mic_level: self.levels.mic(),
            speaker_level: self.levels.speaker(),
            transmitting: self.levels.transmitting(),
        }
    }

    /// Resolves when the other side hangs up or the connection drops for good.
    pub async fn ended(&self) {
        self.conn.closed().await;
    }

    pub fn hang_up(self) {
        self.conn.close(0u32.into(), b"bye");
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        self.receiver.abort();
    }
}
