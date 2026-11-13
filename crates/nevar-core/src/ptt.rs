//! Push-to-talk key watcher.
//!
//! Polls the key's state instead of registering a global hotkey: hotkeys swallow
//! the key system-wide (a push-to-talk on V would stop you typing V anywhere),
//! polling doesn't. Works with mouse side buttons too.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
    time::Duration,
};

use crate::voice::{Controls, TransmitMode};

const POLL: Duration = Duration::from_millis(10);
const IDLE_POLL: Duration = Duration::from_millis(100);

pub struct PushToTalk {
    /// Windows virtual-key code; 0 means none.
    key: Arc<AtomicU16>,
}

impl PushToTalk {
    pub fn start(controls: Arc<Controls>) -> Self {
        let key = Arc::new(AtomicU16::new(0));
        let watched = key.clone();
        std::thread::Builder::new()
            .name("push-to-talk".into())
            .spawn(move || loop {
                let vk = watched.load(Ordering::Relaxed);
                if vk == 0 || controls.mode() != TransmitMode::PushToTalk {
                    controls.set_push_to_talk_held(false);
                    std::thread::sleep(IDLE_POLL);
                    continue;
                }
                controls.set_push_to_talk_held(is_key_down(vk));
                std::thread::sleep(POLL);
            })
            .expect("spawning push-to-talk thread");
        Self { key }
    }

    pub fn set_key(&self, vk: Option<u16>) {
        self.key.store(vk.unwrap_or(0), Ordering::Relaxed);
    }
}

#[cfg(windows)]
pub(crate) fn is_key_down(vk: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // The high bit is set while the key is down.
    unsafe { GetAsyncKeyState(i32::from(vk)) as u16 & 0x8000 != 0 }
}

#[cfg(not(windows))]
pub(crate) fn is_key_down(_vk: u16) -> bool {
    false
}
