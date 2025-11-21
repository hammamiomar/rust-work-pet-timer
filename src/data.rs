use anyhow::Result;
use chrono::{DateTime, Duration, Local, Utc};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const DB_PATH: &str = "work_log.json";

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
pub struct Session {
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub session_type: SessionType,
    pub note: String,
}

impl Session {
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
}

pub fn load_sessions() -> Result<Vec<Session>> {
    let mut sessions: Vec<Session> = if Path::new(DB_PATH).exists() {
        let data = fs::read_to_string(DB_PATH)?;
        serde_json::from_str(&data)?
    } else {
        Vec::new()
    };

    let now = Utc::now();
    for session in &mut sessions {
        if session.end_time.is_none() {
            let duration = now - session.start_time;
            if duration > Duration::hours(24) {
                session.end_time = Some(session.start_time);
                session.note.push_str(" [Auto-closed: Stale]");
            } else {
                session.end_time = Some(now);
            }
        }
    }
    Ok(sessions)
}

pub fn save_sessions(sessions: &[Session]) -> Result<()> {
    let data = serde_json::to_string_pretty(sessions)?;
    fs::write(DB_PATH, data)?;
    Ok(())
}
