//! Text messages with friends. They go straight to the friend over the open connection,
//! so both must be online; each side keeps its own history in one JSON file.

use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::EndpointId;

/// Longest message, in characters.
pub const MAX_MESSAGE_CHARS: usize = 2000;
/// Messages kept per friend; older ones are forgotten.
const KEEP: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: u64,
    pub from_me: bool,
    pub text: String,
    /// Unix milliseconds, by this PC's clock, when it was sent or arrived.
    pub at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Chat {
    pub messages: VecDeque<Message>,
    /// Arrived since the chat was last open.
    pub unread: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub chats: BTreeMap<EndpointId, Chat>,
}

impl History {
    /// Adds a message. Returns false for one we already have (the same ID arriving twice).
    pub fn push(&mut self, with: EndpointId, message: Message) -> bool {
        let chat = self.chats.entry(with).or_default();
        if chat.messages.iter().rev().take(50).any(|m| m.id == message.id && m.from_me == message.from_me) {
            return false;
        }
        if !message.from_me {
            chat.unread += 1;
        }
        chat.messages.push_back(message);
        while chat.messages.len() > KEEP {
            chat.messages.pop_front();
        }
        true
    }

    pub fn unread(&self, with: &EndpointId) -> u32 {
        self.chats.get(with).map_or(0, |c| c.unread)
    }

    /// Returns whether anything changed.
    pub fn mark_read(&mut self, with: &EndpointId) -> bool {
        match self.chats.get_mut(with) {
            Some(chat) if chat.unread > 0 => {
                chat.unread = 0;
                true
            }
            _ => false,
        }
    }
}

/// Trims a message for sending; `None` if nothing is left.
pub fn clean_text(text: &str) -> Option<String> {
    // Keep line breaks and tabs, drop other control characters, then trim what's left.
    let text: String = text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
    let text: String = text.trim().chars().take(MAX_MESSAGE_CHARS).collect();
    (!text.is_empty()).then_some(text)
}

pub struct HistoryStore {
    path: PathBuf,
}

impl HistoryStore {
    pub fn new(dir: &Path) -> Self {
        Self { path: dir.join("messages.json") }
    }

    pub fn load(&self) -> Result<History> {
        if !self.path.exists() {
            return Ok(History::default());
        }
        let text = fs::read_to_string(&self.path).with_context(|| format!("reading {}", self.path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", self.path.display()))
    }

    /// Writes atomically, so a crash mid-save can't corrupt the file.
    pub fn save(&self, history: &History) -> Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(history)?)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> EndpointId {
        iroh::SecretKey::from_bytes(&[n; 32]).public()
    }

    fn msg(id: u64, from_me: bool) -> Message {
        Message { id, from_me, text: format!("m{id}"), at_ms: id }
    }

    #[test]
    fn counts_unread_and_drops_duplicates() {
        let mut h = History::default();
        assert!(h.push(id(1), msg(1, false)));
        assert!(h.push(id(1), msg(2, true)));
        assert!(!h.push(id(1), msg(1, false)), "same message twice");
        assert_eq!(h.unread(&id(1)), 1);
        assert!(h.mark_read(&id(1)));
        assert!(!h.mark_read(&id(1)));
        assert_eq!(h.unread(&id(1)), 0);
    }

    #[test]
    fn keeps_only_recent_messages() {
        let mut h = History::default();
        for i in 0..(KEEP as u64 + 20) {
            h.push(id(1), msg(i, true));
        }
        let chat = &h.chats[&id(1)];
        assert_eq!(chat.messages.len(), KEEP);
        assert_eq!(chat.messages.front().unwrap().id, 20);
    }

    #[test]
    fn cleans_text() {
        assert_eq!(clean_text("  \n "), None);
        assert_eq!(clean_text(" selam\nnasılsın \u{7} ").as_deref(), Some("selam\nnasılsın"));
        assert_eq!(clean_text(&"a".repeat(5000)).unwrap().chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn history_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("nevar-chat-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let store = HistoryStore::new(&dir);
        let mut h = History::default();
        h.push(id(3), msg(7, false));
        store.save(&h).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.chats[&id(3)].messages[0], msg(7, false));
        assert_eq!(loaded.unread(&id(3)), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
