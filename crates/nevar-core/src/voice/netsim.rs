//! A pretend bad network for the test bot: drops some outgoing voice packets and holds
//! others back by a random delay (which also reorders them), the way a poor connection
//! would. Off (and free) unless the bot turns it on.

use std::{
    sync::mpsc::{Receiver, Sender, channel},
    time::{Duration, Instant},
};

use bytes::Bytes;

use super::Destinations;

#[derive(Default)]
pub(super) struct NetSim {
    rng: Rng,
    /// Delayed packets go to a thread that sends each one when it's due. Started on first use.
    delayed: Option<Sender<(Instant, Bytes)>>,
}

impl NetSim {
    pub fn send(&mut self, destinations: &Destinations, packet: Bytes, (loss_percent, jitter_ms): (u32, u32)) {
        if loss_percent > 0 && self.rng.below(100) < loss_percent {
            return;
        }
        if jitter_ms == 0 {
            destinations.send(packet);
            return;
        }
        let due = Instant::now() + Duration::from_millis(u64::from(self.rng.below(jitter_ms + 1)));
        let delayed = self.delayed.get_or_insert_with(|| {
            let (tx, rx) = channel();
            let destinations = destinations.clone();
            let _ = std::thread::Builder::new().name("netsim".into()).spawn(move || deliver(rx, destinations));
            tx
        });
        let _ = delayed.send((due, packet));
    }
}

/// Sends each packet at its due time, earliest first, until the sender side goes away.
fn deliver(rx: Receiver<(Instant, Bytes)>, destinations: Destinations) {
    let mut waiting: Vec<(Instant, Bytes)> = Vec::new();
    loop {
        let now = Instant::now();
        waiting.sort_by_key(|(due, _)| *due);
        while waiting.first().is_some_and(|(due, _)| *due <= now) {
            destinations.send(waiting.remove(0).1);
        }
        let wait = waiting.first().map_or(Duration::from_secs(1), |(due, _)| due.saturating_duration_since(now));
        match rx.recv_timeout(wait) {
            Ok(item) => waiting.push(item),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// xorshift: plenty random for simulating packet trouble, no extra dependency.
struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9_7f4a_7c15, |d| d.as_nanos() as u64);
        Self(seed | 1)
    }
}

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % u64::from(n.max(1))) as u32
    }
}
