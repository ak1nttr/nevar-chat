//! A whole call between two services with virtual sound cards, the way the test bot runs:
//! one plays a tone, the other says back whatever it hears. Checks the tone makes the round
//! trip, and that the simulated bad network really loses packets.
//! Needs internet access: peers find each other through the public relays.

use std::{path::PathBuf, time::Duration};

use nevar_core::{
    EndpointId,
    audio::{FRAME_SAMPLES, Frame, SAMPLE_RATE, virtual_card::MicSource},
    service::{AppState, AudioMode, CallPhase, Service},
    store::{ContactState, Mode},
};

struct Tone(usize);

impl MicSource for Tone {
    fn next(&mut self, _played: &Frame) -> Frame {
        let mut frame = [0.0; FRAME_SAMPLES];
        for s in frame.iter_mut() {
            *s = 0.3 * (2.0 * std::f32::consts::PI * 440.0 * self.0 as f32 / SAMPLE_RATE as f32).sin();
            self.0 += 1;
        }
        frame
    }
}

/// Says back what it hears, right away.
struct Parrot;

impl MicSource for Parrot {
    fn next(&mut self, played: &Frame) -> Frame {
        *played
    }
}

async fn start(name: &str, mic: fn() -> Box<dyn MicSource>) -> (Service, PathBuf) {
    let dir = std::env::temp_dir().join(format!("nevar-vcall-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let service = Service::start_with(&dir, AudioMode::Virtual(Box::new(mic)), |_| {}).await.unwrap();
    service.set_name(name).unwrap();
    // A tone isn't speech: keep the mic open and the signal untouched.
    let mut settings = service.settings();
    settings.mode = Mode::Open;
    settings.noise_suppression = false;
    settings.echo_cancellation = false;
    settings.auto_gain = false;
    service.set_settings(settings);
    (service, dir)
}

fn id(service: &Service) -> EndpointId {
    service.state().me.id.parse().unwrap()
}

async fn wait_for(service: &Service, what: &str, check: impl Fn(&AppState) -> bool) {
    for _ in 0..600 {
        if check(&service.state()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for: {what}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tone_goes_to_the_bot_and_comes_back() {
    let ((me, me_dir), (bot, bot_dir)) = tokio::join!(
        start("me", || Box::new(Tone(0))),
        start("bot", || Box::new(Parrot))
    );
    let bot_id = id(&bot);

    me.add_friend(&bot_id.to_string()).unwrap();
    wait_for(&bot, "the friend request", |s| s.contacts.iter().any(|c| c.state == ContactState::Incoming)).await;
    bot.accept_request(&id(&me)).unwrap();
    wait_for(&me, "the bot online as a friend", |s| {
        s.contacts.iter().any(|c| c.state == ContactState::Friend && c.online)
    })
    .await;

    me.call(&bot_id).unwrap();
    wait_for(&bot, "the call ringing", |s| s.call.as_ref().is_some_and(|c| c.phase == CallPhase::Incoming)).await;
    bot.accept_call().unwrap();
    wait_for(&me, "the call", |s| s.call.as_ref().is_some_and(|c| c.phase == CallPhase::Active)).await;

    // Me → bot → me: the tone comes back loud.
    let mut heard = 0.0f32;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        heard = heard.max(me.call_stats().and_then(|s| s.peers.first().map(|p| p.level)).unwrap_or(0.0));
    }
    assert!(heard > 0.1, "the tone didn't come back, level {heard}");

    // The bot's pretend bad network loses packets on their way to me.
    bot.controls().set_simulated_network(30, 0);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let lost = me.call_stats().and_then(|s| s.peers.first().map(|p| p.lost)).unwrap_or(0);
    assert!(lost > 10, "expected losses from the simulated network, got {lost}");

    me.hang_up();
    tokio::join!(me.shutdown(), bot.shutdown());
    let _ = std::fs::remove_dir_all(me_dir);
    let _ = std::fs::remove_dir_all(bot_dir);
}
