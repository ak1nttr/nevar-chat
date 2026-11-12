//! Command-line test tool for nevar's networking, used before the desktop app exists,
//! and the test bot for trying the app alone.

mod bot;

use std::{io::Write, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use nevar_core::{
    Connection, EndpointId, Node,
    audio::device,
    identity, probe,
    voice::{AudioDevices, Call, Controls, Levels, Playout, Sources},
};

#[derive(Parser)]
#[command(about = "nevar network test tool")]
struct Cli {
    /// Use a separate identity, so two copies can run on one PC.
    #[arg(long, global = true, default_value = "default")]
    profile: String,
    /// Microphone to use: its number from `devices`, or part of its name.
    #[arg(long, global = true)]
    mic: Option<String>,
    /// Speakers/headphones to use: their number from `devices`, or part of the name.
    #[arg(long, global = true)]
    speakers: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print your ID (the code a friend needs to reach you).
    Id,
    /// Wait for friends to connect and answer their probes.
    Listen,
    /// Connect to a friend and measure a voice-like stream of packets.
    Probe {
        /// The friend's ID, as printed by their `listen`.
        peer: EndpointId,
        /// How long to run, in seconds.
        #[arg(long, default_value_t = 30)]
        secs: u64,
    },
    /// Voice-call a friend who is running `answer`.
    Call {
        /// The friend's ID, as printed by their `answer`.
        peer: EndpointId,
    },
    /// Wait for a friend to call you, then talk.
    Answer,
    /// Check your mic and speakers (plays no sound).
    Devices,
    /// Run a test bot: a pretend friend that answers calls and echoes your voice.
    /// Run more with different --profile values for group calls.
    Bot,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let cli = Cli::parse();
    if let Command::Bot = cli.command {
        return bot::run(&cli.profile).await;
    }
    let devices = AudioDevices { mic: cli.mic, speakers: cli.speakers };
    let key = identity::load_or_create(&identity_path(&cli.profile)?)?;

    match cli.command {
        Command::Id => {
            println!("{}", key.public());
            return Ok(());
        }
        Command::Devices => return check_devices(&devices).await,
        _ => {}
    }

    println!("starting up...");
    let node = Node::start(key).await?;
    println!("your ID: {}", node.id());

    match cli.command {
        Command::Id | Command::Devices | Command::Bot => unreachable!(),
        Command::Listen => listen(&node).await?,
        Command::Probe { peer, secs } => run_probe(&node, peer, Duration::from_secs(secs)).await?,
        Command::Call { peer } => {
            println!("calling {}...", peer.fmt_short());
            let conn = node.connect(peer).await?;
            talk(conn, &devices).await?;
        }
        Command::Answer => {
            println!("waiting for a call (Ctrl+C to stop)...");
            let conn = tokio::select! {
                _ = tokio::signal::ctrl_c() => None,
                incoming = node.accept() => incoming,
            };
            if let Some(conn) = conn {
                talk(conn?, &devices).await?;
            }
        }
    }

    node.shutdown().await;
    Ok(())
}

fn identity_path(profile: &str) -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join("nevar").join(format!("{profile}.key")))
}

async fn listen(node: &Node) -> Result<()> {
    println!("waiting for connections (Ctrl+C to stop)...");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            incoming = node.accept() => {
                let Some(incoming) = incoming else { return Ok(()) };
                let conn = match incoming {
                    Ok(conn) => conn,
                    Err(e) => {
                        eprintln!("incoming connection failed: {e:#}");
                        continue;
                    }
                };
                let peer = conn.remote_id().fmt_short();
                println!("{peer} connected");
                tokio::spawn(async move {
                    if let Err(e) = probe::echo(&conn).await {
                        eprintln!("{peer}: {e:#}");
                    }
                    println!("{peer} disconnected");
                });
            }
        }
    }
}

async fn run_probe(node: &Node, peer: EndpointId, duration: Duration) -> Result<()> {
    println!("connecting to {}...", peer.fmt_short());
    let started = std::time::Instant::now();
    let conn = node.connect(peer).await?;
    println!(
        "connected in {} ms, probing for {} s\n",
        started.elapsed().as_millis(),
        duration.as_secs()
    );

    probe::run(&conn, duration, Duration::from_secs(1), |r, is_total| {
        let line = format!(
            "{:<8} ping {:>4} ms (max {:>4})  jitter {:>5.1} ms  loss {:>5.1}%",
            r.route.to_string(),
            r.rtt_avg.as_millis(),
            r.rtt_max.as_millis(),
            r.jitter.as_secs_f64() * 1000.0,
            r.loss_percent(),
        );
        if is_total {
            println!("\nTOTAL    {line}  ({} sent, {} back)", r.sent, r.received);
        } else {
            println!("{line}");
        }
    })
    .await?;

    conn.close(0u32.into(), b"done");
    Ok(())
}

async fn talk(conn: Connection, devices: &AudioDevices) -> Result<()> {
    let peer = conn.remote_id().fmt_short();
    let controls = std::sync::Arc::new(Controls::default());
    let call = Call::start(devices, controls)?;
    call.add_peer(conn.clone())?;
    println!("in a call with {peer}");
    if let Some((mic, speakers)) = call.device_names() {
        println!("  mic:      {mic}");
        println!("  speakers: {speakers}");
    }
    println!("Ctrl+C to hang up\n");

    let mut ticker = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                conn.close(0u32.into(), b"bye");
                println!("\nhung up");
                return Ok(());
            }
            _ = conn.closed() => {
                println!("\n{peer} hung up");
                return Ok(());
            }
            _ = ticker.tick() => {
                let s = call.stats();
                let Some(p) = s.peers.first() else { continue };
                let ping = p.rtt.map_or("?".into(), |r| r.as_millis().to_string());
                print!(
                    "\r{:<7} ping {:>3} ms  buffer {:>3} ms  lost {} (fixed {})  you {}{} them {}   ",
                    p.route.to_string(),
                    ping,
                    p.jitter.target * 20,
                    p.jitter.lost,
                    p.jitter.recovered,
                    meter(s.mic_level),
                    if s.transmitting { "*" } else { " " },
                    meter(s.speaker_level),
                );
                std::io::stdout().flush()?;
            }
        }
    }
}

/// A small text level meter.
fn meter(level: f32) -> String {
    // Speech sits around 0.02-0.2 RMS; map that range onto 8 cells, logarithmically.
    let db = 20.0 * level.max(1e-5).log10();
    let filled = (((db + 50.0) / 40.0) * 8.0).clamp(0.0, 8.0) as usize;
    format!("[{}{}]", "#".repeat(filled), ".".repeat(8 - filled))
}

async fn check_devices(devices: &AudioDevices) -> Result<()> {
    let list = device::list_devices()?;
    let print = |names: &[String], default: &Option<String>| {
        for (i, name) in names.iter().enumerate() {
            let mark = if Some(name) == default.as_ref() { "*" } else { " " };
            println!("  {mark} {}. {name}", i + 1);
        }
    };
    println!("microphones:");
    print(&list.inputs, &list.default_input);
    println!("speakers:");
    print(&list.outputs, &list.default_output);
    println!("(* = Windows default, used unless you pass --mic / --speakers)");
    println!("example: nevar-cli --mic 2 --speakers 1 call <ID>\n");

    // Open both like a call would, but feed the speakers silence.
    let levels = std::sync::Arc::new(Levels::default());
    let controls = std::sync::Arc::new(Controls::default());
    let speakers =
        device::start_playback(&devices.speakers, Playout::new(Sources::default(), controls, levels, None))?;
    println!("speakers OK: {} ({} ch)", speakers.device_name, speakers.config.channels);

    let (tx, rx) = std::sync::mpsc::sync_channel(8);
    let mic = device::start_capture(&devices.mic, tx)?;
    println!(
        "mic OK: {} ({} Hz, {} ch)",
        mic.device_name, mic.config.sample_rate, mic.config.channels
    );

    println!("\nsay something for 5 seconds...");
    let frames = tokio::task::spawn_blocking(move || {
        let mut peak = 0f32;
        let mut count = 0;
        while let Ok(frame) = rx.recv_timeout(Duration::from_secs(1)) {
            let level = nevar_core::audio::rms(&frame);
            peak = peak.max(level);
            count += 1;
            print!("\r  level {}", meter(level));
            let _ = std::io::stdout().flush();
            if count == 250 {
                break;
            }
        }
        (count, peak)
    });
    let (count, peak) = frames.await?;
    drop(mic);
    println!("\n\ngot {count} frames ({} ms of audio), loudest {}", count * 20, meter(peak));
    if count < 200 {
        println!("WARNING: the mic delivered less audio than expected");
    }
    if peak < 0.005 {
        println!("WARNING: the mic seems silent. Check it isn't muted in Windows sound settings.");
    }
    Ok(())
}
