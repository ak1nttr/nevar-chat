//! A network probe shaped like a voice call: one small datagram every 20 ms.
//!
//! The dialing side sends packets carrying a sequence number and its own send
//! time; the other side echoes them back unchanged. Since only the sender's
//! clock is used, no clock sync between the two PCs is needed.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use iroh::endpoint::Connection;
use tokio::time::{MissedTickBehavior, interval};

use crate::{
    node::{Route, current_route},
    protocol::{PROBE_REPLY, PROBE_REQUEST},
};

/// Same cadence as 20 ms Opus frames.
pub const PACKET_INTERVAL: Duration = Duration::from_millis(20);
/// Roughly the size of one Opus voice frame plus our header.
const PACKET_SIZE: usize = 120;
const HEADER_SIZE: usize = 13;

/// Answers probe packets until the connection closes. Other datagrams are ignored.
pub async fn echo(conn: &Connection) -> Result<()> {
    while let Ok(packet) = conn.read_datagram().await {
        if packet.first() == Some(&PROBE_REQUEST) {
            reply(conn, &packet)?;
        }
    }
    Ok(())
}

/// Sends a probe request back as a reply.
pub fn reply(conn: &Connection, request: &[u8]) -> Result<()> {
    let mut packet = request.to_vec();
    packet[0] = PROBE_REPLY;
    conn.send_datagram(packet.into())?;
    Ok(())
}

/// Stats for one reporting window.
#[derive(Debug, Clone, Copy)]
pub struct ProbeReport {
    pub route: Route,
    pub sent: u32,
    pub received: u32,
    pub rtt_avg: Duration,
    pub rtt_max: Duration,
    /// Mean variation between consecutive round trips (RFC 3550 style).
    pub jitter: Duration,
}

impl ProbeReport {
    pub fn loss_percent(&self) -> f64 {
        if self.sent == 0 {
            return 0.0;
        }
        100.0 * f64::from(self.sent.saturating_sub(self.received)) / f64::from(self.sent)
    }
}

#[derive(Default)]
struct Window {
    sent: u32,
    received: u32,
    rtt_sum: Duration,
    rtt_max: Duration,
    jitter: f64,
}

/// Runs the probe for `duration`, calling `on_report` once per `report_every`
/// and once more with the totals at the end.
pub async fn run(
    conn: &Connection,
    duration: Duration,
    report_every: Duration,
    mut on_report: impl FnMut(&ProbeReport, bool),
) -> Result<ProbeReport> {
    let start = Instant::now();
    let mut ticker = interval(PACKET_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut window = Window::default();
    let mut total = Window::default();
    let mut last_rtt: Option<f64> = None;
    let mut next_report = start + report_every;
    let mut seq: u32 = 0;
    let mut packet = vec![0u8; PACKET_SIZE];
    packet[0] = PROBE_REQUEST;

    let sending_until = start + duration;
    // Keep listening a little after the last send so late echoes are counted.
    let deadline = sending_until + Duration::from_millis(500);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                if now < sending_until {
                    let micros = now.duration_since(start).as_micros() as u64;
                    packet[1..5].copy_from_slice(&seq.to_le_bytes());
                    packet[5..HEADER_SIZE].copy_from_slice(&micros.to_le_bytes());
                    conn.send_datagram(packet.clone().into())?;
                    seq += 1;
                    window.sent += 1;
                    total.sent += 1;
                }
                if now >= next_report {
                    on_report(&summarize(conn, &window), false);
                    window = Window::default();
                    next_report += report_every;
                }
            }
            reply = conn.read_datagram() => {
                let reply = match reply {
                    Ok(reply) => reply,
                    Err(e) => bail!("connection lost: {e}"),
                };
                if reply.len() < HEADER_SIZE || reply[0] != PROBE_REPLY {
                    continue;
                }
                let sent_micros = u64::from_le_bytes(reply[5..HEADER_SIZE].try_into()?);
                let rtt = start.elapsed().saturating_sub(Duration::from_micros(sent_micros));
                let rtt_ms = rtt.as_secs_f64() * 1000.0;
                for w in [&mut window, &mut total] {
                    w.received += 1;
                    w.rtt_sum += rtt;
                    w.rtt_max = w.rtt_max.max(rtt);
                    if let Some(prev) = last_rtt {
                        w.jitter += ((rtt_ms - prev).abs() - w.jitter) / 16.0;
                    }
                }
                last_rtt = Some(rtt_ms);
            }
        }
    }

    let report = summarize(conn, &total);
    on_report(&report, true);
    Ok(report)
}

fn summarize(conn: &Connection, w: &Window) -> ProbeReport {
    ProbeReport {
        route: current_route(conn),
        sent: w.sent,
        received: w.received,
        rtt_avg: if w.received > 0 { w.rtt_sum / w.received } else { Duration::ZERO },
        rtt_max: w.rtt_max,
        jitter: Duration::from_secs_f64(w.jitter / 1000.0),
    }
}
