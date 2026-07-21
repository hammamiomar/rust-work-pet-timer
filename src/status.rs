use crate::data::{JournalEntry, SessionType};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Heartbeat older than this means the TUI is gone (it writes every ~1s).
pub const STALE_AFTER_SECS: i64 = 10;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TodayTotals {
    pub work_secs: i64,
    pub break_secs: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StatusSnapshot {
    pub schema_version: u32,
    pub app_alive_at: DateTime<Utc>,
    pub last_keypress_at: DateTime<Utc>,
    pub state: SessionType,
    pub session_started_at: DateTime<Utc>,
    pub mood: String,
    pub mood_caption: String,
    pub today: TodayTotals,
    pub latest_journal_entry: Option<JournalEntry>,
    pub unread_agent_messages: u32,
    pub pid: u32,
}

pub fn write(path: &Path, snapshot: &StatusSnapshot) -> Result<()> {
    let data = serde_json::to_string_pretty(snapshot)?;
    crate::paths::atomic_write(path, &data)
}

pub fn read(path: &Path) -> Option<StatusSnapshot> {
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn is_online(snapshot: &StatusSnapshot, now: DateTime<Utc>) -> bool {
    (now - snapshot.app_alive_at).num_seconds() <= STALE_AFTER_SECS
}

/// Removed on clean TUI exit so the agent sees "offline" immediately.
pub fn remove(path: &Path) {
    fs::remove_file(path).ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn snapshot(alive_at: DateTime<Utc>) -> StatusSnapshot {
        StatusSnapshot {
            schema_version: 1,
            app_alive_at: alive_at,
            last_keypress_at: alive_at,
            state: SessionType::Work,
            session_started_at: alive_at,
            mood: "focused".to_string(),
            mood_caption: String::new(),
            today: TodayTotals { work_secs: 0, break_secs: 0 },
            latest_journal_entry: None,
            unread_agent_messages: 0,
            pid: 0,
        }
    }

    #[test]
    fn fresh_heartbeat_is_online() {
        let now = Utc::now();
        assert!(is_online(&snapshot(now - Duration::seconds(3)), now));
    }

    #[test]
    fn stale_heartbeat_is_offline() {
        let now = Utc::now();
        assert!(!is_online(&snapshot(now - Duration::seconds(60)), now));
    }
}
