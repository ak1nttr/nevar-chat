//! Global shortcuts (quick menu, mute, ...) that work while a game has focus.
//!
//! Like push-to-talk, these poll key states instead of registering system hotkeys, so a
//! shortcut never takes its keys away from the game or other apps, and mouse side buttons work.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::{ptt::is_key_down, store::Hotkey};

/// Fast enough that a tap is never missed, rare enough to cost nothing.
const POLL: Duration = Duration::from_millis(25);
const IDLE_POLL: Duration = Duration::from_millis(250);

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12; // Alt

/// Watches a set of shortcuts and calls back with `A` each time one is pressed.
pub struct Hotkeys<A> {
    bindings: Arc<Mutex<Vec<(A, Hotkey)>>>,
    paused: Arc<AtomicBool>,
}

impl<A: Copy + Send + 'static> Hotkeys<A> {
    pub fn start(on_press: impl Fn(A) + Send + 'static) -> Self {
        let bindings: Arc<Mutex<Vec<(A, Hotkey)>>> = Arc::default();
        let paused = Arc::new(AtomicBool::new(false));
        let (watched, watched_paused) = (bindings.clone(), paused.clone());
        std::thread::Builder::new()
            .name("hotkeys".into())
            .spawn(move || {
                // Which bindings were held at the last poll, so holding a combo fires once.
                let mut held: Vec<bool> = Vec::new();
                loop {
                    let current = watched.lock().unwrap().clone();
                    if current.is_empty() || watched_paused.load(Ordering::Relaxed) {
                        held.clear();
                        std::thread::sleep(IDLE_POLL);
                        continue;
                    }
                    held.resize(current.len(), true);
                    let mods = (is_key_down(VK_CONTROL), is_key_down(VK_SHIFT), is_key_down(VK_MENU));
                    for (i, (action, hotkey)) in current.iter().enumerate() {
                        let down = mods == (hotkey.ctrl, hotkey.shift, hotkey.alt) && is_key_down(hotkey.key);
                        if down && !held[i] {
                            on_press(*action);
                        }
                        held[i] = down;
                    }
                    std::thread::sleep(POLL);
                }
            })
            .expect("spawning hotkeys thread");
        Self { bindings, paused }
    }

    /// Replaces the watched shortcuts. Ones held right now don't fire until pressed again.
    pub fn set(&self, bindings: Vec<(A, Hotkey)>) {
        *self.bindings.lock().unwrap() = bindings;
    }

    /// Stops watching for a while, e.g. while the user is picking a new shortcut.
    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}
