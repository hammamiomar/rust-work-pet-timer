use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Keep only the newest messages; the inbox is a nudge channel, not an archive.
const MAX_MESSAGES: usize = 50;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InboxMessage {
    pub id: u64,
    pub time: DateTime<Utc>,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Inbox {
    pub messages: Vec<InboxMessage>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct Ack {
    pub last_read_id: u64,
}

pub fn load(path: &Path) -> Inbox {
    fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

/// Append a message (MCP server side — the only writer of inbox.json).
pub fn append(path: &Path, text: String) -> Result<InboxMessage> {
    let mut inbox = load(path);
    let id = inbox.messages.iter().map(|m| m.id).max().unwrap_or(0) + 1;
    let msg = InboxMessage {
        id,
        time: Utc::now(),
        text,
    };
    inbox.messages.push(msg.clone());
    if inbox.messages.len() > MAX_MESSAGES {
        let drop = inbox.messages.len() - MAX_MESSAGES;
        inbox.messages.drain(..drop);
    }
    let data = serde_json::to_string_pretty(&inbox)?;
    crate::paths::atomic_write(path, &data)?;
    Ok(msg)
}

pub fn load_ack(path: &Path) -> Ack {
    fs::read_to_string(path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

/// Record the highest message id the user has seen (TUI side).
pub fn save_ack(path: &Path, ack: Ack) -> Result<()> {
    let data = serde_json::to_string(&ack)?;
    crate::paths::atomic_write(path, &data)
}

pub fn unread(inbox: &Inbox, ack: Ack) -> Vec<InboxMessage> {
    inbox
        .messages
        .iter()
        .filter(|m| m.id > ack.last_read_id)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("pet-timer-test-inbox");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::remove_file(&path).ok();
        path
    }

    #[test]
    fn append_assigns_monotonic_ids() {
        let path = tmp_file("ids.json");
        let a = append(&path, "one".into()).unwrap();
        let b = append(&path, "two".into()).unwrap();
        assert_eq!(a.id, 1);
        assert_eq!(b.id, 2);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn unread_respects_ack_cursor() {
        let path = tmp_file("ack.json");
        append(&path, "one".into()).unwrap();
        append(&path, "two".into()).unwrap();
        let inbox = load(&path);
        let fresh = unread(&inbox, Ack { last_read_id: 1 });
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].text, "two");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn inbox_caps_message_count() {
        let path = tmp_file("cap.json");
        for i in 0..60 {
            append(&path, format!("m{}", i)).unwrap();
        }
        let inbox = load(&path);
        assert_eq!(inbox.messages.len(), MAX_MESSAGES);
        assert_eq!(inbox.messages.last().unwrap().id, 60);
        fs::remove_file(&path).ok();
    }
}
