//! Mic and speaker streams via cpal (WASAPI on Windows).

use std::{sync::mpsc::SyncSender, time::Duration};

use anyhow::{Context, Result, bail};
use cpal::{
    BufferSize, Device, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedStreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use tracing::{debug, info, warn};

use super::{FRAME_SAMPLES, Frame, SAMPLE_RATE};
use crate::voice::Playout;

/// A running audio stream. Audio stops when this is dropped.
///
/// The stream itself lives on the thread that opened it, which waits until this handle is
/// dropped and then closes it. cpal sets up COM per thread on Windows, so a stream is opened
/// and closed on one thread that stays alive the whole time.
pub struct AudioStream {
    /// Dropping this wakes the owning thread, which closes the stream.
    _stop: std::sync::mpsc::Sender<()>,
    pub device_name: String,
    pub config: StreamConfig,
}

/// What a device thread reports back once its stream is running.
type Opened = (String, StreamConfig);

pub struct DeviceList {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub default_input: Option<String>,
    pub default_output: Option<String>,
}

pub fn list_devices() -> Result<DeviceList> {
    let host = cpal::default_host();
    let names = |devices: Vec<Device>| devices.iter().map(device_name).collect();
    Ok(DeviceList {
        inputs: names(host.input_devices()?.collect()),
        outputs: names(host.output_devices()?.collect()),
        default_input: host.default_input_device().as_ref().map(device_name),
        default_output: host.default_output_device().as_ref().map(device_name),
    })
}

/// Which device to use: `None` follows the Windows default.
/// Otherwise a 1-based number from [`list_devices`], or part of the device's name.
pub type DeviceChoice = Option<String>;

fn find_device(
    choice: &DeviceChoice,
    devices: impl Iterator<Item = Device>,
    default: Option<Device>,
    kind: &str,
) -> Result<Device> {
    let Some(wanted) = choice else {
        return default.with_context(|| format!("no {kind} found"));
    };
    let devices: Vec<Device> = devices.collect();
    if let Ok(n) = wanted.parse::<usize>() {
        return n
            .checked_sub(1)
            .and_then(|i| devices.get(i).cloned())
            .with_context(|| format!("there is no {kind} number {n}"));
    }
    let wanted_lower = wanted.to_lowercase();
    devices
        .iter()
        .find(|d| device_name(d) == *wanted)
        .or_else(|| devices.iter().find(|d| device_name(d).to_lowercase().contains(&wanted_lower)))
        .cloned()
        .with_context(|| format!("no {kind} matching \"{wanted}\""))
}

fn device_name(device: &Device) -> String {
    device
        .description()
        .map(|d| d.name().to_owned())
        .unwrap_or_else(|_| "unknown device".into())
}

/// Opening a device takes milliseconds. A driver that never answers (a stuck USB mic, a
/// device another app holds) must fail the call with a message instead of freezing it.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens a stream with `open` on a thread of its own, which then keeps it until the returned
/// handle is dropped. Gives up after [`OPEN_TIMEOUT`]; if the device answers later after all,
/// that thread closes the stream right away.
fn open_on_own_thread(
    what: &'static str,
    open: impl FnOnce() -> Result<(Stream, Opened)> + Send + 'static,
) -> Result<AudioStream> {
    let (opened_tx, opened_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new().name(format!("audio-{what}")).spawn(move || match open() {
        Ok((stream, opened)) => {
            if opened_tx.send(Ok(opened)).is_ok() {
                // Runs until the handle is dropped (the channel closes).
                let _ = stop_rx.recv();
            }
            drop(stream);
        }
        Err(e) => {
            let _ = opened_tx.send(Err(e));
        }
    })?;
    match opened_rx.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok((device_name, config))) => Ok(AudioStream { _stop: stop_tx, device_name, config }),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            warn!("the {what} didn't respond within {OPEN_TIMEOUT:?}");
            bail!("the {what} didn't respond (another app may be holding it, or it was unplugged)")
        }
    }
}

/// Opens a mic and sends 20 ms mono 48 kHz frames to `frames`.
/// Frames are dropped (not queued) if the receiver falls behind.
pub fn start_capture(choice: &DeviceChoice, frames: SyncSender<Frame>) -> Result<AudioStream> {
    let choice = choice.clone();
    open_on_own_thread("microphone", move || open_capture(&choice, frames))
}

fn open_capture(choice: &DeviceChoice, frames: SyncSender<Frame>) -> Result<(Stream, Opened)> {
    let host = cpal::default_host();
    let device = find_device(choice, host.input_devices()?, host.default_input_device(), "microphone")?;
    let supported = pick_input_config(&device)?;
    let format = supported.sample_format();
    let mut config = supported.config();
    config.buffer_size = BufferSize::Default;
    let framer = Framer::new(config.sample_rate, config.channels as usize, frames);

    let stream = match format {
        SampleFormat::F32 => build_input::<f32>(&device, &config, framer),
        SampleFormat::I16 => build_input::<i16>(&device, &config, framer),
        SampleFormat::I32 => build_input::<i32>(&device, &config, framer),
        SampleFormat::U16 => build_input::<u16>(&device, &config, framer),
        other => bail!("unsupported microphone sample format {other}"),
    }?;
    stream.play()?;

    let device_name = device_name(&device);
    info!(%device_name, ?config, %format, "mic started");
    Ok((stream, (device_name, config)))
}

/// Prefers a native 48 kHz config (no resampling); otherwise takes the device default.
fn pick_input_config(device: &Device) -> Result<SupportedStreamConfig> {
    let at_48k: Vec<_> = device
        .supported_input_configs()?
        .filter(|c| (c.min_sample_rate()..=c.max_sample_rate()).contains(&SAMPLE_RATE))
        .collect();
    let best = at_48k
        .iter()
        .find(|c| c.sample_format() == SampleFormat::F32)
        .or_else(|| at_48k.first());
    match best {
        Some(range) => Ok(range.with_sample_rate(SAMPLE_RATE)),
        None => Ok(device.default_input_config()?),
    }
}

fn build_input<T>(device: &Device, config: &StreamConfig, mut framer: Framer) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = device.build_input_stream(
        *config,
        move |data: &[T], _| framer.push_interleaved(data),
        |e| report_stream_error("mic", &e),
        None,
    )?;
    Ok(stream)
}

fn report_stream_error(which: &str, e: &cpal::Error) {
    // A single glitch when Windows starts a stream is normal and not worth alarming anyone.
    if e.kind() == cpal::ErrorKind::Xrun {
        debug!("{which} stream glitch: {e}");
    } else {
        warn!("{which} stream error: {e}");
    }
}

/// Turns interleaved device samples of any rate into mono 48 kHz frames.
struct Framer {
    channels: usize,
    resampler: Option<Linear>,
    frame: Frame,
    pos: usize,
    out: SyncSender<Frame>,
}

impl Framer {
    fn new(rate: u32, channels: usize, out: SyncSender<Frame>) -> Self {
        Self {
            channels,
            resampler: (rate != SAMPLE_RATE).then(|| Linear::new(rate, SAMPLE_RATE)),
            frame: [0.0; FRAME_SAMPLES],
            pos: 0,
            out,
        }
    }

    fn push_interleaved<T>(&mut self, data: &[T])
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        for chunk in data.chunks_exact(self.channels) {
            let mono = chunk.iter().map(|&s| f32::from_sample(s)).sum::<f32>() / self.channels as f32;
            match self.resampler.as_mut() {
                Some(r) => {
                    let mut emitted = [0.0; 4];
                    let n = r.process(mono, &mut emitted);
                    for &s in &emitted[..n] {
                        self.push_sample(s);
                    }
                }
                None => self.push_sample(mono),
            }
        }
    }

    fn push_sample(&mut self, sample: f32) {
        self.frame[self.pos] = sample;
        self.pos += 1;
        if self.pos == FRAME_SAMPLES {
            self.pos = 0;
            // A full queue means the encoder is behind; dropping keeps latency bounded.
            let _ = self.out.try_send(self.frame);
        }
    }
}

/// Linear-interpolation resampler. Plenty for speech from the usual 44.1 kHz mics.
struct Linear {
    /// Input samples consumed per output sample.
    step: f64,
    /// Position of the next output, between `prev` (0.0) and the incoming sample (1.0).
    pos: f64,
    prev: f32,
}

impl Linear {
    fn new(from: u32, to: u32) -> Self {
        Self { step: f64::from(from) / f64::from(to), pos: 0.0, prev: 0.0 }
    }

    /// Feeds one input sample, writing the outputs it completes; returns how many.
    fn process(&mut self, sample: f32, out: &mut [f32; 4]) -> usize {
        let mut n = 0;
        while self.pos < 1.0 && n < out.len() {
            out[n] = self.prev + (sample - self.prev) * self.pos as f32;
            n += 1;
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = sample;
        n
    }
}

/// Opens speakers or headphones and plays whatever `playout` produces.
pub fn start_playback(choice: &DeviceChoice, playout: Playout) -> Result<AudioStream> {
    let choice = choice.clone();
    open_on_own_thread("speakers", move || open_playback(&choice, playout))
}

fn open_playback(choice: &DeviceChoice, mut playout: Playout) -> Result<(Stream, Opened)> {
    let host = cpal::default_host();
    let device = find_device(choice, host.output_devices()?, host.default_output_device(), "speaker")?;
    let channels = device.default_output_config()?.channels();
    // Windows converts our 48 kHz float stream to whatever the device runs at.
    let config = StreamConfig {
        channels,
        sample_rate: SAMPLE_RATE,
        buffer_size: BufferSize::Default,
    };

    let channels = channels as usize;
    let mut mono = vec![0.0f32; SAMPLE_RATE as usize];
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _| {
            let frames = data.len() / channels;
            // Callbacks are a few ms; the buffer is sized for a full second.
            let mono = &mut mono[..frames.min(SAMPLE_RATE as usize)];
            playout.fill(mono);
            for (out, &s) in data.chunks_exact_mut(channels).zip(mono.iter()) {
                out.fill(s);
            }
        },
        |e| report_stream_error("speaker", &e),
        None,
    )?;
    stream.play()?;

    let device_name = device_name(&device);
    info!(%device_name, ?config, "speakers started");
    Ok((stream, (device_name, config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays an inaudibly quiet tone on the default speakers and checks that the device keeps
    /// pulling audio from the stream's own thread. Needs a sound card:
    /// `cargo test -p nevar-core speakers_keep_playing -- --ignored`.
    #[test]
    #[ignore]
    fn speakers_keep_playing() {
        use std::sync::{Arc, Mutex};

        use crate::{
            audio::codec::{Encoder, MAX_PACKET},
            voice::{Controls, JitterBuffer, Levels, Sources},
        };

        let jitter = Arc::new(Mutex::new(JitterBuffer::default()));
        let sources = Sources::default();
        sources.add(iroh::SecretKey::from_bytes(&[9; 32]).public(), jitter.clone()).unwrap();
        let levels = Arc::new(Levels::default());
        let playout = Playout::new(sources, Arc::new(Controls::default()), levels.clone(), None);
        let speakers = start_playback(&None, playout).unwrap();

        // Feed a frame every 20 ms for a second, like the network would.
        let mut encoder = Encoder::new().unwrap();
        let mut buf = [0u8; MAX_PACKET];
        let mut heard = 0.0f32;
        for i in 0..50u32 {
            let mut frame = [0.0; FRAME_SAMPLES];
            for (n, s) in frame.iter_mut().enumerate() {
                *s = 0.002 * ((i as usize * FRAME_SAMPLES + n) as f32 * 0.06).sin();
            }
            let len = encoder.encode(&frame, &mut buf).unwrap();
            jitter.lock().unwrap().push(i, bytes::Bytes::copy_from_slice(&buf[..len]), false);
            std::thread::sleep(Duration::from_millis(20));
            heard = heard.max(levels.speaker());
        }
        drop(speakers);
        assert!(heard > 0.0, "the speaker callback never played our audio");
    }

    #[test]
    fn resampler_produces_the_right_number_of_samples() {
        let mut r = Linear::new(44_100, 48_000);
        let mut out = [0.0; 4];
        let total: usize = (0..44_100).map(|i| r.process((i as f32 * 0.01).sin(), &mut out)).sum();
        assert!((47_999..=48_001).contains(&total), "got {total}");
    }

    #[test]
    fn framer_downmixes_stereo_into_frames() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut framer = Framer::new(SAMPLE_RATE, 2, tx);
        let stereo: Vec<f32> = (0..FRAME_SAMPLES).flat_map(|_| [1.0, 0.0]).collect();
        framer.push_interleaved(&stereo);
        let frame = rx.try_recv().unwrap();
        assert!(frame.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }
}
