//! Everything nevar remembers between runs, as one JSON file.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{EndpointId, service::control::Relation, voice::TransmitMode};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Stored {
    /// My display name, shown to friends.
    pub name: String,
    /// Everyone I have any relation with, by ID.
    pub contacts: BTreeMap<EndpointId, Contact>,
    pub settings: Settings,
    /// How loud each person plays in calls, when not 1.0. Kept for guests too.
    pub volumes: BTreeMap<EndpointId, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    /// Their display name, as they last told us.
    pub name: String,
    pub state: ContactState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactState {
    /// I asked them; waiting for an answer.
    Outgoing,
    /// They asked me; waiting for my answer.
    Incoming,
    Friend,
    /// I turned them down.
    Declined,
}

impl ContactState {
    /// What I tell them about our relationship.
    pub fn relation(self) -> Relation {
        match self {
            ContactState::Outgoing => Relation::Requesting,
            ContactState::Incoming => Relation::None,
            ContactState::Friend => Relation::Friend,
            ContactState::Declined => Relation::Declined,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Device names; `None` follows the Windows default.
    pub mic: Option<String>,
    pub speakers: Option<String>,
    pub mode: Mode,
    /// Windows virtual-key code for push-to-talk.
    pub push_to_talk_key: Option<u16>,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    pub output_volume: f32,
    pub shortcuts: Shortcuts,
    pub overlay: OverlaySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mic: None,
            speakers: None,
            mode: Mode::VoiceActivity,
            push_to_talk_key: None,
            noise_suppression: true,
            echo_cancellation: true,
            auto_gain: true,
            output_volume: 1.0,
            shortcuts: Shortcuts::default(),
            overlay: OverlaySettings::default(),
        }
    }
}

/// A key combination: a Windows virtual-key code (keyboard or mouse button) plus modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: u16,
}

impl Hotkey {
    const fn ctrl_shift(key: u16) -> Self {
        Self { ctrl: true, shift: true, alt: false, key }
    }
}

/// Global shortcuts, working while a game has focus. `None` turns one off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shortcuts {
    pub quick_menu: Option<Hotkey>,
    pub toggle_mute: Option<Hotkey>,
    pub toggle_deafen: Option<Hotkey>,
    pub toggle_overlay: Option<Hotkey>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        // Not Shift+Tab: that's Steam's overlay.
        Self {
            quick_menu: Some(Hotkey::ctrl_shift(0x20)),    // Space
            toggle_mute: Some(Hotkey::ctrl_shift(0x4d)),   // M
            toggle_deafen: Some(Hotkey::ctrl_shift(0x44)), // D
            toggle_overlay: Some(Hotkey::ctrl_shift(0x4f)), // O
        }
    }
}

/// The see-through panel drawn over games in a corner of the screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub enabled: bool,
    pub corner: Corner,
    /// Who's in the call and who's talking.
    pub show_voice: bool,
    /// New messages, for a few seconds each.
    pub show_messages: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self { enabled: false, corner: Corner::TopLeft, show_voice: true, show_messages: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Serializable mirror of [`TransmitMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    VoiceActivity,
    PushToTalk,
    Open,
}

impl From<Mode> for TransmitMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::VoiceActivity => TransmitMode::VoiceActivity,
            Mode::PushToTalk => TransmitMode::PushToTalk,
            Mode::Open => TransmitMode::Open,
        }
    }
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(dir: &Path) -> Self {
        Self { path: dir.join("state.json") }
    }

    pub fn load(&self) -> Result<Stored> {
        if !self.path.exists() {
            return Ok(Stored::default());
        }
        let text = fs::read_to_string(&self.path).with_context(|| format!("reading {}", self.path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", self.path.display()))
    }

    /// Writes atomically, so a crash mid-save can't corrupt the file.
    pub fn save(&self, stored: &Stored) -> Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(stored)?)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings saved by 0.4, before shortcuts and the overlay existed, still load,
    /// and pick up the default shortcuts.
    #[test]
    fn old_settings_get_default_shortcuts() {
        let old = r#"{"mic":null,"speakers":"Kulaklık","mode":"pushToTalk","push_to_talk_key":5,
            "noise_suppression":true,"echo_cancellation":false,"auto_gain":true,"output_volume":1.2}"#;
        let settings: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.speakers.as_deref(), Some("Kulaklık"));
        assert_eq!(settings.push_to_talk_key, Some(5));
        assert_eq!(settings.shortcuts, Shortcuts::default());
        assert!(!settings.overlay.enabled);
    }

    #[test]
    fn a_cleared_shortcut_stays_cleared() {
        let mut settings = Settings::default();
        settings.shortcuts.toggle_mute = None;
        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.shortcuts.toggle_mute, None);
        assert!(back.shortcuts.quick_menu.is_some());
    }
}
