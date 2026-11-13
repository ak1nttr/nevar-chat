//! The test bot: a pretend friend for trying nevar alone. It runs the app's real service
//! with a virtual sound card, accepts friend requests, answers calls and says back what it
//! heard, replies to messages, and takes commands typed into its terminal.

use std::{
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use nevar_core::{
    EndpointId,
    audio::{FRAME_SAMPLES, Frame, SAMPLE_RATE, STEREO_SAMPLES, StereoFrame, virtual_card::MicSource},
    service::{AppState, AudioMode, CallPhase, Event, Notice, Service},
    store::{ContactState, Mode},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc,
};

/// How long the bot waits before saying back what it heard.
const ECHO_SECONDS: f32 = 1.5;

/// What the bot sends in a call.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Voice {
    /// Your own voice, back after [`ECHO_SECONDS`].
    Echo = 0,
    /// A short tune on a loop: dropouts and glitches are easy to hear in it.
    Tune = 1,
    Silent = 2,
}

pub async fn run(profile: &str) -> Result<()> {
    let data_dir = data_dir(profile)?;
    let voice = Arc::new(AtomicU8::new(Voice::Echo as u8));
    let (events_tx, mut events) = mpsc::unbounded_channel();

    println!("Test botu başlıyor...");
    let audio = AudioMode::Virtual(Box::new({
        let voice = voice.clone();
        move || Box::new(BotMic::new(voice.clone())) as Box<dyn MicSource>
    }));
    let service = Service::start_with(&data_dir, audio, move |e| {
        let _ = events_tx.send(e);
    })
    .await?;

    // Like a friend with a clean mic that's always on: what comes in goes out untouched.
    let name = if profile == "default" { "Test Botu".to_string() } else { format!("Test Botu {profile}") };
    service.set_name(&name)?;
    let mut settings = service.settings();
    settings.mode = Mode::Open;
    settings.noise_suppression = false;
    settings.echo_cancellation = false;
    settings.auto_gain = false;
    service.set_settings(settings);

    let me = service.state().me;
    println!();
    println!("  {name} hazır.");
    println!("  Arkadaş kodu: {}", me.id);
    println!();
    println!("  nevar'da \"Arkadaş ekle\"ye bu kodu yapıştır, bot isteği kendisi kabul eder.");
    println!("  Komutlar için: yardım");
    println!();

    let mut bot = Bot { service: service.clone(), voice, last_chat: None, answering: false };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            event = events.recv() => match event {
                Some(event) => bot.on_event(event),
                None => break,
            },
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    if !bot.on_command(line.trim()) {
                        break;
                    }
                }
                // Input closed (e.g. started without a terminal): keep running until Ctrl+C.
                _ => std::future::pending::<()>().await,
            },
        }
    }

    println!("Kapanıyor...");
    service.shutdown().await;
    Ok(())
}

/// Each bot keeps its identity and friends in its own folder.
fn data_dir(profile: &str) -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join("nevar").join("bots").join(profile))
}

struct Bot {
    service: Service,
    voice: Arc<AtomicU8>,
    /// Whom to send typed messages to: whoever wrote last.
    last_chat: Option<EndpointId>,
    /// An answer is on its way, so the incoming call isn't answered twice.
    answering: bool,
}

impl Bot {
    fn on_event(&mut self, event: Event) {
        match event {
            Event::StateChanged => self.on_state(self.service.state()),
            Event::Notice(notice) => self.on_notice(notice),
        }
    }

    fn on_state(&mut self, state: AppState) {
        for c in state.contacts.iter().filter(|c| c.state == ContactState::Incoming) {
            match EndpointId::from_str(&c.id).map_err(anyhow::Error::from).and_then(|id| self.service.accept_request(&id)) {
                Ok(()) => println!("✓ {} artık arkadaşım.", c.name),
                Err(e) => println!("! {} isteği kabul edilemedi: {e:#}", c.name),
            }
        }

        let ringing = state.call.as_ref().is_some_and(|c| c.phase == CallPhase::Incoming);
        if ringing && !self.answering {
            self.answering = true;
            let caller = state.call.as_ref().and_then(|c| c.caller.as_ref()).map_or(String::new(), |p| p.name.clone());
            println!("☎ {caller} arıyor, 1 saniye sonra açıyorum...");
            let service = self.service.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                match service.accept_call() {
                    Ok(()) => println!("☎ Görüşmedeyim. {}", voice_hint(Voice::Echo)),
                    Err(e) => println!("! Açamadım: {e:#}"),
                }
            });
        } else if !ringing {
            self.answering = false;
        }
    }

    fn on_notice(&mut self, notice: Notice) {
        match notice {
            Notice::Message { from, name, text } => {
                println!("✉ {name}: {text}");
                if let Ok(id) = EndpointId::from_str(&from) {
                    self.last_chat = Some(id);
                    let reply: String = format!("aldım: {text}").chars().take(500).collect();
                    if let Err(e) = self.service.send_message(&id, &reply) {
                        println!("! Cevap gidemedi: {e:#}");
                    }
                }
            }
            Notice::FriendRequest { .. } => {}
            Notice::FriendAccepted { name } => println!("✓ {name} isteğimi kabul etti."),
            Notice::CallDeclined { name } => println!("☎ {name} reddetti ya da meşgul."),
            Notice::NoAnswer { name } => println!("☎ {name} açmadı."),
            Notice::MissedCall { name } => println!("☎ Cevapsız arama: {name}"),
            Notice::ConnectionLost { name } => println!("! {name} ile bağlantı koptu."),
            Notice::Removed { name } => println!("☎ {name} beni görüşmeden çıkardı."),
            Notice::Silenced { name } => println!("☎ {name} mikrofonumu kapattı. Açmak için: konuş"),
            Notice::Unsilenced { name } => println!("☎ {name} mikrofonumu geri açtı."),
            Notice::Error { message } => println!("! {message}"),
        }
    }

    /// Runs a typed command. Returns false to quit.
    fn on_command(&mut self, line: &str) -> bool {
        let (cmd, arg) = line.split_once(' ').map_or((line, ""), |(c, a)| (c, a.trim()));
        // ASCII spellings too, for consoles that mangle Turkish letters.
        match fold(cmd).as_str() {
            "" => {}
            "yardim" | "help" | "?" => print_help(),
            "kod" => println!("Arkadaş kodu: {}", self.service.state().me.id),
            "ara" => self.call(arg),
            "kapat" => {
                self.service.hang_up();
                println!("☎ Görüşmeden çıktım.");
            }
            "mesaj" => self.message(arg),
            "yanki" => self.set_voice(Voice::Echo),
            "melodi" | "ton" => self.set_voice(Voice::Tune),
            "muzik" => match fold(arg).as_str() {
                "dur" | "kapat" | "durdur" => {
                    self.service.stop_sharing();
                    println!("♪ Müzik paylaşımı durdu.");
                }
                _ => self.share_music(),
            },
            "sessiz" => self.set_voice(Voice::Silent),
            "kayip" => match arg.trim_end_matches('%').parse::<u32>() {
                Ok(loss) => self.set_network(Some(loss), None),
                Err(_) => println!("Kullanım: kayıp 5   (paketlerin %5'i kaybolur)"),
            },
            "titreme" | "gecikme" => match arg.trim_end_matches("ms").trim().parse::<u32>() {
                Ok(jitter) => self.set_network(None, Some(jitter)),
                Err(_) => println!("Kullanım: titreme 80   (her paket 0-80 ms arası geç gelir)"),
            },
            "kotu" => self.set_network(Some(5), Some(80)),
            "berbat" => self.set_network(Some(15), Some(200)),
            "normal" | "iyi" => self.set_network(Some(0), Some(0)),
            "durum" => self.print_status(),
            "at" | "cikar" => self.host_action(arg, HostAction::Remove),
            "sustur" => self.host_action(arg, HostAction::Silence),
            "ac" => self.host_action(arg, HostAction::Unsilence),
            "konus" => {
                self.service.set_muted(false);
                println!("🎙 Mikrofonum açık.");
            }
            "cik" | "quit" | "exit" => return false,
            _ => println!("Bilinmeyen komut: {cmd}. Liste için: yardım"),
        }
        true
    }

    fn call(&self, who: &str) {
        let state = self.service.state();
        let wanted = fold(who);
        let friend = state
            .contacts
            .iter()
            .filter(|c| c.state == ContactState::Friend && c.online)
            .find(|c| wanted.is_empty() || fold(&c.name).contains(&wanted));
        let Some(friend) = friend else {
            println!("Arayacak çevrimiçi arkadaş yok{}.", if who.is_empty() { "" } else { " (bu isimde)" });
            return;
        };
        match EndpointId::from_str(&friend.id).map_err(anyhow::Error::from).and_then(|id| self.service.call(&id)) {
            Ok(()) => println!("☎ {} aranıyor...", friend.name),
            Err(e) => println!("! Arayamadım: {e:#}"),
        }
    }

    /// As the call's host: takes someone out of the call, or turns their mic off or back on.
    fn host_action(&self, who: &str, action: HostAction) {
        let wanted = fold(who);
        let member = self
            .service
            .state()
            .call
            .and_then(|c| c.members.into_iter().find(|m| wanted.is_empty() || fold(&m.name).contains(&wanted)));
        let Some(member) = member else {
            println!("Görüşmede {}kimse yok.", if who.is_empty() { "" } else { "bu isimde " });
            return;
        };
        let result = EndpointId::from_str(&member.id).map_err(anyhow::Error::from).and_then(|id| match action {
            HostAction::Remove => self.service.remove_from_call(&id),
            HostAction::Silence => self.service.silence(&id, true),
            HostAction::Unsilence => self.service.silence(&id, false),
        });
        match (result, action) {
            (Ok(()), HostAction::Remove) => println!("☎ {} görüşmeden çıkarıldı.", member.name),
            (Ok(()), HostAction::Silence) => println!("☎ {} susturuldu.", member.name),
            (Ok(()), HostAction::Unsilence) => println!("☎ {} mikrofonu geri açıldı.", member.name),
            (Err(e), _) => println!("! Olmadı: {e:#}"),
        }
    }

    fn message(&self, text: &str) {
        if text.is_empty() {
            println!("Kullanım: mesaj selam nasılsın");
            return;
        }
        let state = self.service.state();
        let to = self.last_chat.or_else(|| {
            state
                .contacts
                .iter()
                .find(|c| c.state == ContactState::Friend && c.online)
                .and_then(|c| EndpointId::from_str(&c.id).ok())
        });
        let Some(to) = to else {
            println!("Mesaj atacak çevrimiçi arkadaş yok.");
            return;
        };
        match self.service.send_message(&to, text) {
            Ok(()) => println!("✉ Gönderildi."),
            Err(e) => println!("! Gidemedi: {e:#}"),
        }
    }

    /// Shares a stereo tune on the music channel, the way a friend sharing Spotify would.
    fn share_music(&self) {
        let mut tune = StereoTune::default();
        match self.service.share_generated("Test müziği", move || tune.next()) {
            Ok(()) => println!("♪ Müzik kanalından stereo melodi paylaşıyorum: notalar sırayla sol ve sağdan gelir."),
            Err(e) => println!("! Paylaşamadım (görüşmede miyiz?): {e:#}"),
        }
    }

    fn set_voice(&self, voice: Voice) {
        self.voice.store(voice as u8, Ordering::Relaxed);
        println!("♪ {}", voice_hint(voice));
    }

    fn set_network(&self, loss: Option<u32>, jitter: Option<u32>) {
        let controls = self.service.controls();
        let (old_loss, old_jitter) = controls.simulated_network();
        controls.set_simulated_network(loss.unwrap_or(old_loss), jitter.unwrap_or(old_jitter));
        let (loss, jitter) = controls.simulated_network();
        if loss == 0 && jitter == 0 {
            println!("≈ Ağ normal.");
        } else {
            println!("≈ Sana giden ses: paketlerin %{loss}'i kayboluyor, her paket 0-{jitter} ms geç geliyor.");
        }
    }

    fn print_status(&self) {
        let state = self.service.state();
        let friends: Vec<String> = state
            .contacts
            .iter()
            .filter(|c| c.state == ContactState::Friend)
            .map(|c| format!("{}{}", c.name, if c.online { "" } else { " (çevrimdışı)" }))
            .collect();
        println!("Ben: {}{}", state.me.name, if state.me.online { "" } else { " (bağlanıyor)" });
        println!("Arkadaşlar: {}", if friends.is_empty() { "yok".into() } else { friends.join(", ") });
        let (loss, jitter) = self.service.controls().simulated_network();
        println!("Taklit ağ: %{loss} kayıp, 0-{jitter} ms titreme");
        match self.service.call_stats() {
            Some(stats) => {
                for p in &stats.peers {
                    let name = state
                        .call
                        .as_ref()
                        .and_then(|c| c.members.iter().find(|m| m.id == p.id))
                        .map_or("?", |m| m.name.as_str());
                    println!(
                        "Görüşme: {name} · {} · ping {} ms · tampon {} ms · gelen kayıp {} (kurtarılan {})",
                        if p.route == "direct" { "doğrudan" } else { p.route.as_str() },
                        p.ping_ms.map_or("–".into(), |v| v.to_string()),
                        p.buffer_ms,
                        p.lost,
                        p.recovered
                    );
                }
            }
            None => println!("Görüşmede değilim."),
        }
    }
}

fn voice_hint(voice: Voice) -> String {
    match voice {
        Voice::Echo => format!("Konuş, {ECHO_SECONDS} saniye sonra sesini geri yollarım."),
        Voice::Tune => "Ses kanalından melodi çalıyorum. Kesinti ya da cızırtı duyarsan ağda sorun var.".into(),
        Voice::Silent => "Sessizim.".into(),
    }
}

#[derive(Clone, Copy)]
enum HostAction {
    Remove,
    Silence,
    Unsilence,
}

fn print_help() {
    println!(
        "\
Komutlar:
  ara [isim]        seni arar (gelen arama ekranını denemek için)
  kapat             görüşmeden çıkar
  at [isim]         görüşmeyi ben başlattıysam seni görüşmeden çıkarır
  sustur [isim]     görüşmeyi ben başlattıysam mikrofonunu kapatır
  aç [isim]         susturduğum mikrofonunu geri açar
  konuş             biri beni susturduysa mikrofonumu geri açar
  mesaj <metin>     sana mesaj atar
  yankı             sesini {ECHO_SECONDS} sn sonra geri yollar (görüşmede varsayılan)
  müzik             müzik kanalından stereo melodi paylaşır (müzik dur: durdurur)
  melodi            ses kanalından melodi çalar
  sessiz            hiçbir şey yollamaz
  kayıp <yüzde>     sana giden paketlerin bir kısmını düşürür, örn: kayıp 5
  titreme <ms>      paketleri rastgele geciktirir, örn: titreme 80
  kötü / berbat     hazır kötü ağ: %5 + 80 ms / %15 + 200 ms
  normal            ağ taklidini kapatır
  durum             arkadaşlar, görüşme, ping, kayıplar
  kod               arkadaş kodunu tekrar yazar
  çık               botu kapatır"
    );
}

/// Lowercase without Turkish letters, so "Kayıp", "kayip" and "KAYIP" all match.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ı' => 'i',
            'ğ' => 'g',
            'ü' => 'u',
            'ş' => 's',
            'ö' => 'o',
            'ç' => 'c',
            other => other,
        })
        .filter(|c| *c != '\u{307}') // the dot left over from lowercasing "İ"
        .collect()
}

/// A stereo arpeggio for the music channel: each note alternates left and right, with a soft
/// bass in the middle, so both stereo and dropouts are easy to hear.
#[derive(Default)]
struct StereoTune {
    t: usize,
}

impl StereoTune {
    fn next(&mut self) -> StereoFrame {
        const NOTES: [f32; 8] = [392.0, 493.88, 587.33, 739.99, 783.99, 739.99, 587.33, 493.88];
        const NOTE_SAMPLES: usize = SAMPLE_RATE as usize / 5;
        let mut frame = [0.0; STEREO_SAMPLES];
        for lr in frame.chunks_exact_mut(2) {
            let index = self.t / NOTE_SAMPLES;
            let within = (self.t % NOTE_SAMPLES) as f32 / NOTE_SAMPLES as f32;
            let envelope = (within * 40.0).min(1.0) * (1.0 - within).powi(2);
            let time = self.t as f32 / SAMPLE_RATE as f32;
            let note = 0.22 * envelope * (std::f32::consts::TAU * NOTES[index % NOTES.len()] * time).sin();
            let bass = 0.08 * (std::f32::consts::TAU * 98.0 * time).sin();
            let (left, right) = if index.is_multiple_of(2) { (note, 0.25 * note) } else { (0.25 * note, note) };
            lr[0] = left + bass;
            lr[1] = right + bass;
            self.t += 1;
        }
        frame
    }
}

/// The bot's "mic": what it says depends on the voice picked in the terminal.
struct BotMic {
    voice: Arc<AtomicU8>,
    /// What it heard, [`ECHO_SECONDS`] long, played back in a loop behind the writer.
    echo: Vec<Frame>,
    pos: usize,
    /// Samples of the tune so far.
    t: usize,
}

impl BotMic {
    fn new(voice: Arc<AtomicU8>) -> Self {
        let frames = (ECHO_SECONDS * SAMPLE_RATE as f32 / FRAME_SAMPLES as f32) as usize;
        Self { voice, echo: vec![[0.0; FRAME_SAMPLES]; frames], pos: 0, t: 0 }
    }

    /// A soft four-note arpeggio, a quarter second per note, with gaps between notes.
    fn tune(&mut self) -> Frame {
        const NOTES: [f32; 4] = [523.25, 659.25, 783.99, 1046.5];
        const NOTE_SAMPLES: usize = SAMPLE_RATE as usize / 4;
        let mut frame = [0.0; FRAME_SAMPLES];
        for s in frame.iter_mut() {
            let note = NOTES[(self.t / NOTE_SAMPLES) % NOTES.len()];
            let within = (self.t % NOTE_SAMPLES) as f32 / NOTE_SAMPLES as f32;
            // Quick attack, gentle fade, silent last 20%.
            let envelope = if within > 0.8 { 0.0 } else { (within * 50.0).min(1.0) * (1.0 - within) };
            let phase = self.t as f32 / SAMPLE_RATE as f32;
            *s = 0.25 * envelope * (2.0 * std::f32::consts::PI * note * phase).sin();
            self.t += 1;
        }
        frame
    }
}

impl MicSource for BotMic {
    fn next(&mut self, played: &Frame) -> Frame {
        // Always listen, so switching to echo plays back the last moments right away.
        let said = std::mem::replace(&mut self.echo[self.pos], *played);
        self.pos = (self.pos + 1) % self.echo.len();
        match self.voice.load(Ordering::Relaxed) {
            0 => said,
            1 => self.tune(),
            _ => [0.0; FRAME_SAMPLES],
        }
    }
}
