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
        }
    }
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
