//! A sound card made of code, for the test bot: instead of speakers it collects what the
//! call plays, and instead of a mic it sends whatever a [`MicSource`] makes from that.

use std::{
    sync::mpsc::{RecvTimeoutError, SyncSender},
    time::{Duration, Instant},
};

use anyhow::Result;

use super::{FRAME_SAMPLES, Frame, SAMPLE_RATE};
use crate::voice::Playout;

/// 20 ms, the length of a frame.
const TICK: Duration = Duration::from_millis((FRAME_SAMPLES as u64 * 1000) / SAMPLE_RATE as u64);

/// Makes the "mic" signal, one frame at a time, given what the call just played.
pub trait MicSource: Send {
    fn next(&mut self, played: &Frame) -> Frame;
}

/// Runs until dropped.
pub struct VirtualCard {
    _stop: std::sync::mpsc::Sender<()>,
}

impl VirtualCard {
    /// Plays the call into nothing and feeds `source`'s frames to `mic`, in real time.
    pub fn start(mut playout: Playout, mic: SyncSender<Frame>, mut source: Box<dyn MicSource>) -> Result<Self> {
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        std::thread::Builder::new().name("virtual-card".into()).spawn(move || {
            let mut played: Frame = [0.0; FRAME_SAMPLES];
            let mut next = Instant::now();
            loop {
                playout.fill(&mut played);
                // A full queue means the sender is behind; drop like a real mic would.
                let _ = mic.try_send(source.next(&played));
                next += TICK;
                match stop_rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Err(RecvTimeoutError::Timeout) => {}
                    _ => break,
                }
            }
        })?;
        Ok(Self { _stop: stop_tx })
    }
}
