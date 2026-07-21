use anyhow::Result;
use chrono::{DateTime, Duration, Local, Utc};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SessionType {
    Work,
    Break,
    Idle,
}

impl SessionType {
    pub fn color(&self) -> Color {
        match self {
            SessionType::Work => Color::Green,
            SessionType::Break => Color::Yellow,
            SessionType::Idle => Color::Red,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            SessionType::Work => "WORKING",
            SessionType::Break => "ON BREAK",
            SessionType::Idle => "IDLE",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JournalEntry {
    pub time: DateTime<Utc>,
    pub text: String,
}

impl JournalEntry {
    pub fn time_local(&self) -> DateTime<Local> {
        DateTime::from(self.time)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub session_type: SessionType,
    /// Legacy single-line note; migrated into `entries` on load.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default)]
    pub entries: Vec<JournalEntry>,
}

impl Session {
    pub fn new(kind: SessionType, start: DateTime<Utc>) -> Self {
        Session {
            start_time: start,
            end_time: None,
            session_type: kind,
            note: String::new(),
            entries: Vec::new(),
        }
    }

    pub fn duration(&self) -> Duration {
        match self.end_time {
            Some(end) => end - self.start_time,
            None => Utc::now() - self.start_time,
        }
    }

    pub fn start_time_local(&self) -> DateTime<Local> {
        DateTime::from(self.start_time)
    }

    pub fn end_time_local(&self) -> Option<DateTime<Local>> {
        self.end_time.map(DateTime::from)
    }

    pub fn latest_entry(&self) -> Option<&JournalEntry> {
        self.entries.last()
    }

    pub fn add_entry(&mut self, text: String) {
        self.entries.push(JournalEntry {
            time: Utc::now(),
            text,
        });
    }
}

pub fn load_sessions(path: &Path) -> Result<Vec<Session>> {
    let mut sessions: Vec<Session> = if path.exists() {
        let data = fs::read_to_string(path)?;
        serde_json::from_str(&data)?
    } else {
        Vec::new()
    };

    let now = Utc::now();
    for session in &mut sessions {
        // Migrate the legacy one-line note into the journal.
        if !session.note.is_empty() && session.entries.is_empty() {
            session.entries.push(JournalEntry {
                time: session.start_time,
                text: std::mem::take(&mut session.note),
            });
        }
        // Close any session left open by a crash. Anything under 24h is
        // closed at load time; older ones collapse to zero duration.
        if session.end_time.is_none() {
            let duration = now - session.start_time;
            if duration > Duration::hours(24) {
                session.end_time = Some(session.start_time);
                session.add_entry("[Auto-closed: Stale]".to_string());
            } else {
                session.end_time = Some(now);
            }
        }
    }
    Ok(sessions)
}

pub fn save_sessions(path: &Path, sessions: &[Session]) -> Result<()> {
    let data = serde_json::to_string_pretty(sessions)?;
    crate::paths::atomic_write(path, &data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("pet-timer-test-data");
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn legacy_note_migrates_into_entries() {
        let path = tmp_file("legacy.json");
        let legacy = r#"[{
            "start_time": "2026-07-20T10:00:00Z",
            "end_time": "2026-07-20T11:00:00Z",
            "session_type": "Work",
            "note": "old style note"
        }]"#;
        fs::write(&path, legacy).unwrap();
        let sessions = load_sessions(&path).unwrap();
        assert_eq!(sessions[0].entries.len(), 1);
        assert_eq!(sessions[0].entries[0].text, "old style note");
        assert!(sessions[0].note.is_empty());
        fs::remove_file(&path).ok();
    }

    #[test]
    fn save_load_roundtrip_keeps_entries() {
        let path = tmp_file("roundtrip.json");
        let mut s = Session::new(SessionType::Work, Utc::now());
        s.end_time = Some(Utc::now());
        s.add_entry("did a thing".to_string());
        save_sessions(&path, &[s]).unwrap();
        let loaded = load_sessions(&path).unwrap();
        assert_eq!(loaded[0].entries[0].text, "did a thing");
        fs::remove_file(&path).ok();
    }

    #[test]
    fn stale_open_session_collapses_to_zero() {
        let path = tmp_file("stale.json");
        let old_start = Utc::now() - Duration::hours(48);
        let json = serde_json::to_string(&[Session::new(SessionType::Work, old_start)]).unwrap();
        fs::write(&path, json).unwrap();
        let loaded = load_sessions(&path).unwrap();
        assert_eq!(loaded[0].end_time, Some(old_start));
        assert_eq!(loaded[0].duration(), Duration::zero());
        fs::remove_file(&path).ok();
    }
}
