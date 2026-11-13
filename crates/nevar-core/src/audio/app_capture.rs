//! Capturing one app's sound for music sharing: Spotify, a browser tab's player, anything.
//!
//! Windows (10 2004 and later) can hand out the audio of a single process tree
//! ("process loopback"), before the system volume, without the game or our own voice chat
//! in it. The app keeps playing on your speakers as usual; this takes a copy.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
    time::Duration,
};

use anyhow::{Result, bail};
use tracing::info;

use super::{SAMPLE_RATE, STEREO_SAMPLES, StereoFrame};

/// An app making sound right now, or recently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioApp {
    /// The app's main process: capturing it includes all its helper processes.
    pub pid: u32,
    /// A readable name, e.g. "Spotify".
    pub name: String,
    /// Playing right now (not just open).
    pub active: bool,
}

/// Apps with sound on any speaker, playing ones first. Leaves out nevar itself.
pub fn list_apps() -> Result<Vec<AudioApp>> {
    on_com_thread(imp::list_apps)
}

/// Copies one app's sound, as 20 ms 48 kHz stereo frames, until dropped.
pub struct AppCapture {
    stop: Arc<AtomicBool>,
    pub name: String,
}

impl AppCapture {
    /// Starts sending `app`'s sound to `feed`. Fails if Windows won't capture it.
    pub fn start(app: &AudioApp, feed: SyncSender<StereoFrame>) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let pid = app.pid;
        let thread_stop = stop.clone();
        std::thread::Builder::new().name("app-capture".into()).spawn(move || {
            imp::with_com(|| imp::capture(pid, feed, &thread_stop, &ready_tx))
        })?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                info!(app = %app.name, pid, "sharing an app's sound");
                Ok(Self { stop, name: app.name.clone() })
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                bail!("Windows didn't start capturing {}", app.name)
            }
        }
    }
}

impl Drop for AppCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Runs `f` on a fresh thread with COM set up, so the caller's own COM setup (a UI
/// thread's, say) doesn't matter.
fn on_com_thread<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    std::thread::Builder::new()
        .name("audio-apps".into())
        .spawn(move || imp::with_com(f))?
        .join()
        .map_err(|_| anyhow::anyhow!("listing apps crashed"))?
}

/// Gathers samples into whole stereo frames.
struct Framer {
    frame: StereoFrame,
    pos: usize,
}

impl Framer {
    fn new() -> Self {
        Self { frame: [0.0; STEREO_SAMPLES], pos: 0 }
    }

    /// Adds interleaved stereo samples; returns false once nobody listens anymore.
    fn push(&mut self, samples: impl Iterator<Item = f32>, feed: &SyncSender<StereoFrame>) -> bool {
        for s in samples {
            self.frame[self.pos] = s;
            self.pos += 1;
            if self.pos == STEREO_SAMPLES {
                self.pos = 0;
                // A full queue means the encoder is behind: drop a frame rather than lag.
                if let Err(TrySendError::Disconnected(_)) = feed.try_send(self.frame) {
                    return false;
                }
            }
        }
        true
    }
}

/// "spotify.exe" → "Spotify", "msedge.exe" → "Microsoft Edge".
fn friendly_name(exe: &str) -> String {
    let stem = exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(exe);
    let known = [
        ("spotify", "Spotify"),
        ("chrome", "Google Chrome"),
        ("msedge", "Microsoft Edge"),
        ("firefox", "Firefox"),
        ("opera", "Opera"),
        ("opera_gx", "Opera GX"),
        ("brave", "Brave"),
        ("vlc", "VLC"),
        ("discord", "Discord"),
        ("applemusic", "Apple Music"),
        ("music.ui", "Medya Oynatıcı"),
        ("wmplayer", "Windows Media Player"),
        ("foobar2000", "foobar2000"),
        ("youtube music", "YouTube Music"),
        ("deezer", "Deezer"),
        ("tidal", "TIDAL"),
    ];
    let lower = stem.to_lowercase();
    if let Some((_, name)) = known.iter().find(|(k, _)| *k == lower) {
        return (*name).to_string();
    }
    let mut chars = stem.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// Walks up from `pid` while the parent is the same program, so a helper process (a
/// browser's audio process) maps to the app's main process, whose tree we capture.
fn root_of(pid: u32, processes: &HashMap<u32, (u32, String)>) -> u32 {
    let mut current = pid;
    for _ in 0..16 {
        let Some((parent, exe)) = processes.get(&current) else { break };
        match processes.get(parent) {
            Some((_, parent_exe)) if parent_exe.eq_ignore_ascii_case(exe) && *parent != current => current = *parent,
            _ => break,
        }
    }
    current
}

/// Whether `pid` is `ancestor` or one of its descendants.
fn descends_from(pid: u32, ancestor: u32, processes: &HashMap<u32, (u32, String)>) -> bool {
    let mut current = pid;
    for _ in 0..32 {
        if current == ancestor {
            return true;
        }
        match processes.get(&current) {
            Some((parent, _)) if *parent != current && *parent != 0 => current = *parent,
            _ => return false,
        }
    }
    false
}

#[cfg(windows)]
mod imp {
    use std::{
        collections::{BTreeMap, HashMap},
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc::{Sender, SyncSender},
        },
    };

    use anyhow::{Context, Result, anyhow};
    use windows::{
        Win32::{
            Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
            Media::Audio::{
                AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
                AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
                AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync, AudioSessionStateActive,
                DEVICE_STATE_ACTIVE, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
                IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
                IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
                PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
                WAVEFORMATEX, eRender,
            },
            Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT,
            System::{
                Com::{
                    BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
                    StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0},
                },
                Diagnostics::ToolHelp::{
                    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
                },
                Threading::{CreateEventW, WaitForSingleObject},
                Variant::VT_BLOB,
            },
        },
        core::{IUnknown, Interface, Ref, implement},
    };

    use super::{AudioApp, Framer, SAMPLE_RATE, descends_from, friendly_name, root_of};
    use crate::audio::StereoFrame;

    pub fn with_com<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
        // SAFETY: balanced with CoUninitialize on this same thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let result = f();
        if hr.is_ok() {
            unsafe { CoUninitialize() };
        }
        result
    }

    /// Every process: pid → (parent pid, exe name).
    fn processes() -> HashMap<u32, (u32, String)> {
        let mut map = HashMap::new();
        // SAFETY: plain Win32 snapshot walk; the entry struct is sized as required.
        unsafe {
            let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return map };
            let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
            while ok {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
                map.insert(entry.th32ProcessID, (entry.th32ParentProcessID, exe));
                ok = Process32NextW(snapshot, &mut entry).is_ok();
            }
            let _ = CloseHandle(snapshot);
        }
        map
    }

    pub fn list_apps() -> Result<Vec<AudioApp>> {
        let processes = processes();
        let me = std::process::id();
        // Root pid → playing right now.
        let mut found: BTreeMap<u32, bool> = BTreeMap::new();

        // SAFETY: COM calls on a thread where COM is initialized; interfaces are released on drop.
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            for i in 0..devices.GetCount()? {
                let Ok(device) = devices.Item(i) else { continue };
                let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                let Ok(sessions) = manager.GetSessionEnumerator() else { continue };
                for j in 0..sessions.GetCount().unwrap_or(0) {
                    let Ok(session) = sessions.GetSession(j) else { continue };
                    let Ok(session) = session.cast::<IAudioSessionControl2>() else { continue };
                    if session.IsSystemSoundsSession().0 == 0 {
                        continue; // S_OK means it *is* the system sounds session.
                    }
                    let Ok(pid) = session.GetProcessId() else { continue };
                    if pid == 0 || descends_from(pid, me, &processes) {
                        continue;
                    }
                    let active = session.GetState().is_ok_and(|s| s == AudioSessionStateActive);
                    let root = root_of(pid, &processes);
                    *found.entry(root).or_default() |= active;
                }
            }
        }

        let mut apps: Vec<AudioApp> = found
            .into_iter()
            .filter_map(|(pid, active)| {
                let (_, exe) = processes.get(&pid)?;
                Some(AudioApp { pid, name: friendly_name(exe), active })
            })
            .collect();
        apps.sort_by(|a, b| b.active.cmp(&a.active).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        apps.dedup_by(|a, b| a.name == b.name && a.active == b.active);
        Ok(apps)
    }

    #[implement(IActivateAudioInterfaceCompletionHandler)]
    struct Activated(Sender<windows::core::Result<IUnknown>>);

    impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
        fn ActivateCompleted(&self, operation: Ref<'_, IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
            let result = operation.ok().and_then(|op| {
                let mut hr = windows::core::HRESULT::default();
                let mut unknown: Option<IUnknown> = None;
                // SAFETY: the operation is complete when this callback runs.
                unsafe { op.GetActivateResult(&mut hr, &mut unknown)? };
                hr.ok()?;
                unknown.ok_or_else(|| windows::core::Error::from_hresult(windows::core::HRESULT(-1)))
            });
            let _ = self.0.send(result);
            Ok(())
        }
    }

    /// An audio client that hears only `pid` and its child processes.
    fn activate_for(pid: u32) -> Result<IAudioClient> {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        // ManuallyDrop: dropping a PROPVARIANT runs PropVariantClear, which would try to free
        // `params` (on our stack) as if Windows had allocated it.
        let blob = std::mem::ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                            pBlobData: (&raw mut params).cast(),
                        },
                    },
                    ..Default::default()
                }),
            },
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(tx).into();
        // SAFETY: `params` and `blob` outlive the call, which waits for completion below.
        unsafe {
            ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&*blob), &handler)
                .context("this Windows can't capture an app's sound (needs Windows 10 2004 or later)")?;
        }
        let unknown = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| anyhow!("Windows didn't answer"))?
            .context("Windows refused to capture that app")?;
        Ok(unknown.cast()?)
    }

    pub fn capture(
        pid: u32,
        feed: SyncSender<StereoFrame>,
        stop: &AtomicBool,
        ready: &Sender<Result<()>>,
    ) -> Result<()> {
        let started = start_client(pid);
        let (client, capture, event) = match started {
            Ok(parts) => {
                let _ = ready.send(Ok(()));
                parts
            }
            Err(e) => {
                let _ = ready.send(Err(e));
                return Ok(());
            }
        };

        let mut framer = Framer::new();
        // SAFETY: COM and Win32 calls on the thread that set them up; buffers are read only
        // between GetBuffer and ReleaseBuffer, for the length Windows reports.
        unsafe {
            'run: while !stop.load(Ordering::Relaxed) {
                if WaitForSingleObject(event, 100) != WAIT_OBJECT_0 {
                    continue;
                }
                while capture.GetNextPacketSize().unwrap_or(0) > 0 {
                    let mut data = std::ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    if capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).is_err() {
                        break 'run;
                    }
                    let n = frames as usize * 2;
                    let alive = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                        framer.push(std::iter::repeat_n(0.0, n), &feed)
                    } else {
                        let samples = std::slice::from_raw_parts(data.cast::<f32>(), n);
                        framer.push(samples.iter().copied(), &feed)
                    };
                    let _ = capture.ReleaseBuffer(frames);
                    if !alive {
                        break 'run;
                    }
                }
            }
            let _ = client.Stop();
            let _ = CloseHandle(event);
        }
        Ok(())
    }

    fn start_client(pid: u32) -> Result<(IAudioClient, IAudioCaptureClient, HANDLE)> {
        let client = activate_for(pid)?;
        // Process loopback doesn't report a mix format: ask for 48 kHz float stereo.
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
            nChannels: 2,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * 8,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        // SAFETY: COM calls with valid arguments; the event handle is closed by the caller.
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                // 100 ms of buffer, in 100 ns units.
                1_000_000,
                0,
                &format,
                None,
            )?;
            let event = CreateEventW(None, false, false, None)?;
            client.SetEventHandle(event)?;
            let capture: IAudioCaptureClient = client.GetService()?;
            client.Start()?;
            Ok((client, capture, event))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn with_com<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
        f()
    }

    pub fn list_apps() -> Result<Vec<AudioApp>> {
        Ok(Vec::new())
    }

    pub fn capture(
        _pid: u32,
        _feed: SyncSender<StereoFrame>,
        _stop: &AtomicBool,
        ready: &std::sync::mpsc::Sender<Result<()>>,
    ) -> Result<()> {
        let _ = ready.send(Err(anyhow::anyhow!("sharing an app's sound needs Windows")));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_well() {
        assert_eq!(friendly_name("Spotify.exe"), "Spotify");
        assert_eq!(friendly_name("msedge.exe"), "Microsoft Edge");
        assert_eq!(friendly_name("someplayer.exe"), "Someplayer");
    }

    #[test]
    fn helper_processes_map_to_the_main_one() {
        let processes: HashMap<u32, (u32, String)> = [
            (1, (0, "explorer.exe".to_string())),
            (10, (1, "chrome.exe".to_string())),
            (11, (10, "chrome.exe".to_string())),
            (12, (11, "chrome.exe".to_string())),
            (20, (1, "Spotify.exe".to_string())),
        ]
        .into();
        assert_eq!(root_of(12, &processes), 10);
        assert_eq!(root_of(20, &processes), 20);
        assert!(descends_from(12, 10, &processes));
        assert!(!descends_from(20, 10, &processes));
    }

    /// Lists what's making sound on this PC. Run by hand:
    /// `cargo test -p nevar-core list_audio_apps -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn list_audio_apps() {
        for app in list_apps().unwrap() {
            println!("{app:?}");
        }
    }

    /// Captures the first playing app for two seconds and prints its level (nothing is kept).
    /// Run by hand while something plays:
    /// `cargo test -p nevar-core capture_a_playing_app -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn capture_a_playing_app() {
        let apps = list_apps().unwrap();
        let app = apps.iter().find(|a| a.active).or(apps.first()).expect("no app has sound open").clone();
        let (feed, frames) = std::sync::mpsc::sync_channel(50);
        let capture = AppCapture::start(&app, feed).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let (mut count, mut loudest) = (0, 0.0f32);
        while let Ok(frame) = frames.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
            count += 1;
            loudest = loudest.max(crate::audio::rms(&frame));
        }
        drop(capture);
        println!("{}: {count} frames in 2 s, loudest {loudest:.3}", app.name);
        if app.active {
            assert!(count > 50, "expected about 100 frames, got {count}");
        }
    }
}
