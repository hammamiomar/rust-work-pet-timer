use crate::data::{Session, SessionType};
use chrono::{DateTime, Duration, Utc};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Mood {
    Focused,
    Happy,
    Tired,
    Sleepy,
    Neutral,
}

impl Mood {
    pub fn label(&self) -> &'static str {
        match self {
            Mood::Focused => "focused",
            Mood::Happy => "happy",
            Mood::Tired => "tired",
            Mood::Sleepy => "sleepy",
            Mood::Neutral => "neutral",
        }
    }

    pub fn face(&self) -> &'static str {
        match self {
            Mood::Focused => "(o_o)",
            Mood::Happy => "(^_^)",
            Mood::Tired => "(x_x)",
            Mood::Sleepy => "(-_-) zZ",
            Mood::Neutral => "(._.)",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Mood::Focused => Color::Cyan,
            Mood::Happy => Color::Green,
            Mood::Tired => Color::Magenta,
            Mood::Sleepy => Color::DarkGray,
            Mood::Neutral => Color::Gray,
        }
    }
}

const LONG_STRETCH_MIN: i64 = 90;
const LONG_IDLE_MIN: i64 = 30;
const HEALTHY_MIN_WORK_SECS: i64 = 3600;

/// Pure function of the day's sessions + the currently open one.
/// `today_work`/`today_break` are the day's totals (current session included).
pub fn compute(
    current: &Session,
    today_work: Duration,
    today_break: Duration,
    now: DateTime<Utc>,
) -> Mood {
    let current_len = now - current.start_time;
    let healthy_day = {
        let w = today_work.num_seconds() as f64;
        let b = today_break.num_seconds() as f64;
        let total = w + b;
        total > 0.0
            && today_work.num_seconds() >= HEALTHY_MIN_WORK_SECS
            && (0.6..=0.9).contains(&(w / total))
    };

    match current.session_type {
        SessionType::Idle => {
            if current_len > Duration::minutes(LONG_IDLE_MIN) {
                Mood::Sleepy
            } else {
                Mood::Neutral
            }
        }
        SessionType::Break => {
            if healthy_day {
                Mood::Happy
            } else {
                Mood::Neutral
            }
        }
        SessionType::Work => {
            if current_len >= Duration::minutes(LONG_STRETCH_MIN) {
                Mood::Tired
            } else if healthy_day {
                Mood::Happy
            } else {
                Mood::Focused
            }
        }
    }
}

/// One-line caption for the pet panel and status.json.
pub fn caption(mood: Mood, current: &Session, now: DateTime<Utc>) -> String {
    let mins = (now - current.start_time).num_minutes();
    match mood {
        Mood::Tired => format!("tired — {}m straight, take a break?", mins),
        Mood::Sleepy => format!("sleepy — idle for {}m", mins),
        Mood::Happy => "happy — nice work/break balance".to_string(),
        Mood::Focused => "focused — in the zone".to_string(),
        Mood::Neutral => "neutral".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(kind: SessionType, mins_ago: i64, now: DateTime<Utc>) -> Session {
        Session::new(kind, now - Duration::minutes(mins_ago))
    }

    #[test]
    fn long_work_stretch_is_tired() {
        let now = Utc::now();
        let s = session(SessionType::Work, 95, now);
        assert_eq!(
            compute(&s, Duration::minutes(95), Duration::zero(), now),
            Mood::Tired
        );
    }

    #[test]
    fn short_work_is_focused() {
        let now = Utc::now();
        let s = session(SessionType::Work, 10, now);
        assert_eq!(
            compute(&s, Duration::minutes(10), Duration::zero(), now),
            Mood::Focused
        );
    }

    #[test]
    fn balanced_day_is_happy() {
        let now = Utc::now();
        let s = session(SessionType::Work, 10, now);
        // 80 min work / 20 min break = 0.8 ratio, over an hour of work
        assert_eq!(
            compute(&s, Duration::minutes(80), Duration::minutes(20), now),
            Mood::Happy
        );
    }

    #[test]
    fn long_idle_is_sleepy() {
        let now = Utc::now();
        let s = session(SessionType::Idle, 45, now);
        assert_eq!(
            compute(&s, Duration::zero(), Duration::zero(), now),
            Mood::Sleepy
        );
    }

    #[test]
    fn short_idle_is_neutral() {
        let now = Utc::now();
        let s = session(SessionType::Idle, 5, now);
        assert_eq!(
            compute(&s, Duration::zero(), Duration::zero(), now),
            Mood::Neutral
        );
    }
}
