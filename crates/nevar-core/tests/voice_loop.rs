//! Streams a tone between two local nodes through the real voice pipeline
//! (minus the sound card) and checks what comes out the other end.
//! Needs internet access: nodes start via the public relays.

use std::{
    sync::{Arc, Mutex, mpsc::sync_channel},
    time::Duration,
};

use iroh::SecretKey;
use nevar_core::{
    Node,
    audio::{FRAME_SAMPLES, Frame, SAMPLE_RATE, rms},
    voice::{Controls, JitterBuffer, Levels, Playout, TransmitMode, receive, spawn_sender},
};

const TONE_HZ: f32 = 440.0;
const SECONDS: usize = 3;

fn tone(index: usize) -> Frame {
    let mut frame = [0.0; FRAME_SAMPLES];
    for (i, s) in frame.iter_mut().enumerate() {
        let t = (index * FRAME_SAMPLES + i) as f32 / SAMPLE_RATE as f32;
        *s = 0.5 * (2.0 * std::f32::consts::PI * TONE_HZ * t).sin();
    }
    frame
}

/// Estimates the dominant frequency from zero crossings.
fn frequency(samples: &[f32]) -> f32 {
    let crossings = samples.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
    crossings as f32 / 2.0 / (samples.len() as f32 / SAMPLE_RATE as f32)
}

#[tokio::test(flavor = "multi_thread")]
async fn tone_survives_the_trip() {
    let (a, b) = tokio::join!(
        Node::start(SecretKey::generate()),
        Node::start(SecretKey::generate())
    );
    let (a, b) = (a.unwrap(), b.unwrap());

    let (conn_a, conn_b) = tokio::join!(a.connect(b.id()), async { b.accept().await.unwrap() });
    let (conn_a, conn_b) = (conn_a.unwrap(), conn_b.unwrap());

    // B: receive into a jitter buffer.
    let jitter = Arc::new(Mutex::new(JitterBuffer::default()));
    let levels = Arc::new(Levels::default());
    let receiver = tokio::spawn({
        let jitter = jitter.clone();
        async move { receive(&conn_b, &jitter).await }
    });
    // A pure tone isn't speech: force the mic open and skip clean-up so it arrives untouched.
    let controls = Arc::new(Controls::default());
    controls.set_mode(TransmitMode::Open);
    controls.set_noise_suppression(false);
    controls.set_echo_cancellation(false);
    controls.set_auto_gain(false);
    let mut playout = Playout::new(jitter.clone(), controls.clone(), levels, None).unwrap();

    // A: a fake mic producing a frame every 20 ms.
    let (tx, rx) = sync_channel(3);
    let (_played_tx, played_rx) = sync_channel(1);
    let sender =
        spawn_sender(conn_a.clone(), rx, played_rx, controls, Arc::new(Levels::default())).unwrap();

    // B: a fake speaker pulling a frame every 20 ms.
    let mut heard = Vec::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(20));
    for i in 0..SECONDS * 50 {
        ticker.tick().await;
        tx.send(tone(i)).unwrap();
        let mut out = [0.0; FRAME_SAMPLES];
        playout.fill(&mut out);
        heard.extend_from_slice(&out);
    }
    drop(tx);
    sender.join().unwrap();

    // Skip the first half second: connection warm-up and initial buffering.
    let steady = &heard[SAMPLE_RATE as usize / 2..];
    let level = rms(steady);
    let freq = frequency(steady);
    let stats = jitter.lock().unwrap().stats();
    println!("level {level:.3}, frequency {freq:.0} Hz, {stats:?}");

    assert!(level > 0.25, "output too quiet: {level}");
    assert!((TONE_HZ - 10.0..TONE_HZ + 10.0).contains(&freq), "wrong pitch: {freq}");
    assert_eq!(stats.lost, 0, "frames lost on a local link");

    conn_a.close(0u32.into(), b"done");
    receiver.abort();
    tokio::join!(a.shutdown(), b.shutdown());
}
