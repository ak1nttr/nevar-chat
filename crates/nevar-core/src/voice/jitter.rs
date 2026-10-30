//! Adaptive jitter buffer for one incoming voice stream.
//!
//! Packets arrive with uneven timing (and sometimes out of order or not at all),
//! but the speaker needs exactly one frame every 20 ms. The buffer holds a few
//! frames in reserve (`target`) to absorb that unevenness: it grows the reserve
//! when the stream runs dry and shrinks it again after a calm period, keeping
//! delay as low as the network allows.

use std::collections::BTreeMap;

use bytes::Bytes;

/// Frames buffered before playback starts, at 20 ms each.
const INITIAL_TARGET: usize = 2;
const MIN_TARGET: usize = 1;
const MAX_TARGET: usize = 10;
/// Extra frames tolerated above target before we skip one to cut delay.
const DRIFT_SLACK: usize = 2;
/// After this many calm frames (10 s) the reserve shrinks by one frame.
const RELAX_AFTER: u32 = 500;
/// After this many empty frames in a row (100 ms) we treat the stream as paused.
const STARVE_LIMIT: u32 = 5;
/// Hard cap on stored packets (1 s); anything beyond is stale.
const MAX_PACKETS: usize = 50;
/// A sequence jump this large means the sender restarted.
const RESTART_GAP: u32 = 1000;

/// What the decoder should produce for the next 20 ms.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// Decode this packet normally.
    Packet(Bytes),
    /// The frame was lost; rebuild it from the error-correction data in this following packet.
    Recover(Bytes),
    /// Nothing usable: let the decoder synthesize a plausible continuation.
    Conceal,
    /// Not playing (buffering or the other side is silent).
    Silence,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct JitterStats {
    /// Frames currently waiting.
    pub depth: usize,
    /// Frames we aim to keep in reserve.
    pub target: usize,
    pub played: u64,
    /// Frames that never arrived in time.
    pub lost: u64,
    /// Lost frames rebuilt from error-correction data.
    pub recovered: u64,
    /// Times the stream ran dry and we had to stretch.
    pub underruns: u64,
    /// Packets that arrived after their slot had already played.
    pub late: u64,
    /// Frames skipped to cut accumulated delay.
    pub skipped: u64,
}

#[derive(Debug)]
pub struct JitterBuffer {
    /// Packets by sequence number, with their end-of-speech flag.
    packets: BTreeMap<u32, (Bytes, bool)>,
    next: u32,
    playing: bool,
    /// The last packet played closed a talk spurt, so silence after it is expected.
    spurt_ended: bool,
    target: usize,
    starved: u32,
    calm: u32,
    stats: JitterStats,
}

impl Default for JitterBuffer {
    fn default() -> Self {
        Self {
            packets: BTreeMap::new(),
            next: 0,
            playing: false,
            spurt_ended: false,
            target: INITIAL_TARGET,
            starved: 0,
            calm: 0,
            stats: JitterStats::default(),
        }
    }
}

impl JitterBuffer {
    pub fn push(&mut self, seq: u32, packet: Bytes, end_of_speech: bool) {
        if self.playing {
            let ahead = seq.wrapping_sub(self.next);
            if ahead > u32::MAX / 2 {
                self.stats.late += 1;
                return;
            }
            if ahead > RESTART_GAP {
                self.packets.clear();
                self.playing = false;
            }
        }
        self.packets.insert(seq, (packet, end_of_speech));
        while self.packets.len() > MAX_PACKETS {
            self.packets.pop_first();
        }
    }

    pub fn pop(&mut self) -> Next {
        if !self.playing {
            match self.packets.first_key_value() {
                Some((&first, _)) if self.packets.len() >= self.target => {
                    self.playing = true;
                    self.next = first;
                    self.starved = 0;
                }
                _ => return Next::Silence,
            }
        }

        // Too much queued (sender's clock runs a bit fast, or a burst arrived): drop one frame.
        if self.packets.len() > self.target + DRIFT_SLACK {
            self.packets.remove(&self.next);
            self.next = self.next.wrapping_add(1);
            self.stats.skipped += 1;
        }

        let next = match self.packets.remove(&self.next) {
            Some((packet, end_of_speech)) => {
                self.next = self.next.wrapping_add(1);
                self.starved = 0;
                self.spurt_ended = end_of_speech;
                Next::Packet(packet)
            }
            None if self.packets.is_empty() && self.spurt_ended => {
                // The sender stopped talking on purpose: not a network problem.
                self.playing = false;
                self.spurt_ended = false;
                return Next::Silence;
            }
            None if self.packets.is_empty() => {
                // Nothing has arrived at all: the stream is late or paused, not lossy.
                // Conceal without advancing, so the late packet still plays when it lands.
                self.starved += 1;
                self.calm = 0;
                if self.starved == 1 {
                    self.stats.underruns += 1;
                    self.target = (self.target + 1).min(MAX_TARGET);
                }
                if self.starved > STARVE_LIMIT {
                    self.playing = false;
                    return Next::Silence;
                }
                return Next::Conceal;
            }
            None => {
                // Later packets are here but this one isn't: it was lost.
                self.stats.lost += 1;
                let following = self.packets.get(&self.next.wrapping_add(1)).map(|(p, _)| p.clone());
                self.next = self.next.wrapping_add(1);
                match following {
                    Some(packet) => {
                        self.stats.recovered += 1;
                        Next::Recover(packet)
                    }
                    None => Next::Conceal,
                }
            }
        };

        self.stats.played += 1;
        self.calm += 1;
        if self.calm >= RELAX_AFTER {
            self.calm = 0;
            self.target = (self.target - 1).max(MIN_TARGET);
        }
        next
    }

    pub fn stats(&self) -> JitterStats {
        JitterStats {
            depth: self.packets.len(),
            target: self.target,
            ..self.stats
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(seq: u32) -> Bytes {
        Bytes::from(seq.to_le_bytes().to_vec())
    }

    fn filled(seqs: impl IntoIterator<Item = u32>) -> JitterBuffer {
        let mut jb = JitterBuffer::default();
        for s in seqs {
            jb.push(s, pkt(s), false);
        }
        jb
    }

    #[test]
    fn waits_for_target_then_plays_in_order() {
        let mut jb = JitterBuffer::default();
        jb.push(10, pkt(10), false);
        assert_eq!(jb.pop(), Next::Silence);
        jb.push(11, pkt(11), false);
        assert_eq!(jb.pop(), Next::Packet(pkt(10)));
        assert_eq!(jb.pop(), Next::Packet(pkt(11)));
    }

    #[test]
    fn reorders_packets() {
        let mut jb = filled([2, 0, 1]);
        assert_eq!(jb.pop(), Next::Packet(pkt(0)));
        assert_eq!(jb.pop(), Next::Packet(pkt(1)));
        assert_eq!(jb.pop(), Next::Packet(pkt(2)));
    }

    #[test]
    fn lost_packet_is_recovered_from_the_next_one() {
        let mut jb = filled([0, 2, 3]);
        assert_eq!(jb.pop(), Next::Packet(pkt(0)));
        assert_eq!(jb.pop(), Next::Recover(pkt(2)));
        assert_eq!(jb.pop(), Next::Packet(pkt(2)));
        assert_eq!(jb.stats().lost, 1);
        assert_eq!(jb.stats().recovered, 1);
    }

    #[test]
    fn late_stream_is_stretched_not_skipped() {
        let mut jb = filled([0, 1]);
        jb.pop();
        jb.pop();
        assert_eq!(jb.pop(), Next::Conceal);
        // The late packet still gets its turn.
        jb.push(2, pkt(2), false);
        assert_eq!(jb.pop(), Next::Packet(pkt(2)));
        assert_eq!(jb.stats().underruns, 1);
        assert_eq!(jb.stats().target, INITIAL_TARGET + 1);
    }

    #[test]
    fn packets_after_their_slot_are_dropped() {
        let mut jb = filled([0, 1, 2]);
        jb.pop();
        jb.pop();
        jb.push(0, pkt(0), false);
        assert_eq!(jb.stats().late, 1);
        assert_eq!(jb.pop(), Next::Packet(pkt(2)));
    }

    #[test]
    fn long_silence_stops_playback_and_rebuffers() {
        let mut jb = filled([0, 1]);
        jb.pop();
        jb.pop();
        for _ in 0..STARVE_LIMIT {
            assert_eq!(jb.pop(), Next::Conceal);
        }
        assert_eq!(jb.pop(), Next::Silence);
        // Sender resumes later: buffer to the (now larger) target again.
        let target = jb.stats().target;
        for s in 100..100 + target as u32 - 1 {
            jb.push(s, pkt(s), false);
            assert_eq!(jb.pop(), Next::Silence);
        }
        let last = 100 + target as u32 - 1;
        jb.push(last, pkt(last), false);
        assert_eq!(jb.pop(), Next::Packet(pkt(100)));
    }

    #[test]
    fn deliberate_silence_does_not_grow_the_buffer() {
        let mut jb = JitterBuffer::default();
        jb.push(0, pkt(0), false);
        jb.push(1, pkt(1), true);
        jb.pop();
        jb.pop();
        assert_eq!(jb.pop(), Next::Silence);
        assert_eq!(jb.stats().underruns, 0);
        assert_eq!(jb.stats().target, INITIAL_TARGET);
        // The next talk spurt carries on with the following sequence numbers.
        jb.push(2, pkt(2), false);
        jb.push(3, pkt(3), false);
        assert_eq!(jb.pop(), Next::Packet(pkt(2)));
    }

    #[test]
    fn excess_backlog_is_trimmed() {
        let mut jb = filled(0..10);
        jb.pop();
        assert_eq!(jb.stats().skipped, 1);
    }

    #[test]
    fn calm_period_shrinks_the_reserve() {
        let mut jb = JitterBuffer::default();
        jb.target = 4;
        for s in 0..RELAX_AFTER + 4 {
            jb.push(s, pkt(s), false);
            jb.pop();
        }
        assert_eq!(jb.stats().target, 3);
    }
}
