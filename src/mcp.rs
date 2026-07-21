//! MCP server (`hamba_timer serve`) — gives the Hermes agent live visibility
//! into the work timer. Every tool re-reads the data files, so answers are
//! correct whether or not the TUI is currently running.

use crate::data::{self, Session, SessionType};
use crate::{inbox, paths, stats, status};
use anyhow::Result;
use chrono::{Duration, Local, NaiveDate, Utc};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use serde_json::json;

pub fn serve() -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let service = PetTimerServer::new().serve(stdio()).await?;
        service.waiting().await?;
        Ok(())
    })
}

#[derive(Clone)]
pub struct PetTimerServer {
    tool_router: ToolRouter<Self>,
}

impl Default for PetTimerServer {
    fn default() -> Self {
        Self::new()
    }
}

fn human_duration(d: Duration) -> String {
    let mins = d.num_minutes();
    if mins >= 60 {
        format!("{}h{:02}m", mins / 60, mins % 60)
    } else {
        format!("{}m", mins)
    }
}

fn load_all_sessions() -> Result<Vec<Session>> {
    data::load_sessions(&paths::work_log_path()?)
}

fn day_json(sessions: &[Session], date: NaiveDate) -> serde_json::Value {
    let day = stats::day_stat(sessions, date);
    json!({
        "date": date.to_string(),
        "work_secs": day.work.num_seconds(),
        "break_secs": day.brk.num_seconds(),
        "work_human": human_duration(day.work),
        "break_human": human_duration(day.brk),
        "work_ratio": (day.ratio() * 100.0).round() / 100.0,
    })
}

fn session_json(s: &Session) -> serde_json::Value {
    json!({
        "start": s.start_time_local().to_rfc3339(),
        "end": s.end_time_local().map(|t| t.to_rfc3339()),
        "type": s.session_type.label(),
        "duration_secs": s.duration().num_seconds(),
        "duration_human": human_duration(s.duration()),
        "journal": s.entries.iter().map(|e| json!({
            "time": e.time_local().format("%H:%M").to_string(),
            "text": e.text,
        })).collect::<Vec<_>>(),
    })
}

fn to_pretty(value: serde_json::Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

fn error_json(e: impl std::fmt::Display) -> String {
    to_pretty(json!({ "error": e.to_string() }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HistoryParams {
    /// A specific day, formatted YYYY-MM-DD. Defaults to today.
    pub date: Option<String>,
    /// Alternatively: summarize this many days back from today (max 90).
    pub days_back: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SendMessageParams {
    /// The message to show the user. It appears as a speech bubble from their
    /// desk pet in the timer TUI. Keep it short and friendly.
    pub text: String,
}

#[tool_router]
impl PetTimerServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Live snapshot of what the user is doing right now: timer state (WORKING / ON BREAK / IDLE / OFFLINE), how long the current session has run, the pet's mood, today's work/break totals, and the latest journal entry. Call this to answer 'what is the user up to?'"
    )]
    fn get_current_status(&self) -> String {
        let now = Utc::now();
        let snapshot = paths::status_path().ok().and_then(|p| status::read(&p));
        let online = snapshot
            .as_ref()
            .map(|s| status::is_online(s, now))
            .unwrap_or(false);

        let inbox_info = || -> serde_json::Value {
            let (Ok(inbox_path), Ok(ack_path)) = (paths::inbox_path(), paths::inbox_ack_path())
            else {
                return json!(null);
            };
            let messages = inbox::load(&inbox_path);
            let ack = inbox::load_ack(&ack_path);
            let last_sent = messages.messages.iter().map(|m| m.id).max().unwrap_or(0);
            json!({
                "unread_count": inbox::unread(&messages, ack).len(),
                "last_sent_message_read": last_sent > 0 && last_sent <= ack.last_read_id,
            })
        };

        match snapshot {
            Some(s) if online => to_pretty(json!({
                "online": true,
                "state": s.state.label(),
                "mood": s.mood,
                "mood_caption": s.mood_caption,
                "session_started_at": s.session_started_at.with_timezone(&Local).to_rfc3339(),
                "session_elapsed_secs": (now - s.session_started_at).num_seconds(),
                "session_elapsed_human": human_duration(now - s.session_started_at),
                "latest_journal_entry": s.latest_journal_entry.as_ref().map(|e| json!({
                    "time": e.time_local().format("%H:%M").to_string(),
                    "text": e.text,
                })),
                "today": {
                    "work_secs": s.today.work_secs,
                    "break_secs": s.today.break_secs,
                    "work_human": human_duration(Duration::seconds(s.today.work_secs)),
                    "break_human": human_duration(Duration::seconds(s.today.break_secs)),
                },
                "agent_inbox": inbox_info(),
            })),
            _ => {
                // TUI closed — report last known day totals from the log.
                let today = Local::now().date_naive();
                match load_all_sessions() {
                    Ok(sessions) => to_pretty(json!({
                        "online": false,
                        "state": "OFFLINE",
                        "note": "the timer TUI is not running; totals reflect the saved log",
                        "today": day_json(&sessions, today),
                        "agent_inbox": inbox_info(),
                    })),
                    Err(e) => error_json(e),
                }
            }
        }
    }

    #[tool(
        description = "Today's full picture: work/break totals and ratio, number of sessions, and the complete journal timeline of what the user logged doing today."
    )]
    fn get_today_summary(&self) -> String {
        let today = Local::now().date_naive();
        let sessions = match load_all_sessions() {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let todays: Vec<&Session> = sessions
            .iter()
            .filter(|s| s.start_time_local().date_naive() == today)
            .collect();
        let journal: Vec<_> = {
            let mut entries: Vec<_> = todays
                .iter()
                .flat_map(|s| {
                    s.entries.iter().map(|e| (e.time, s.session_type, &e.text))
                })
                .collect();
            entries.sort_by_key(|(t, _, _)| *t);
            entries
                .into_iter()
                .map(|(t, kind, text)| json!({
                    "time": t.with_timezone(&Local).format("%H:%M").to_string(),
                    "mode": kind.label(),
                    "text": text,
                }))
                .collect()
        };
        to_pretty(json!({
            "summary": day_json(&sessions, today),
            "session_count": todays.len(),
            "work_sessions": todays.iter().filter(|s| s.session_type == SessionType::Work).count(),
            "first_activity": todays.first().map(|s| s.start_time_local().format("%H:%M").to_string()),
            "journal": journal,
        }))
    }

    #[tool(
        description = "Session history. Pass 'date' (YYYY-MM-DD) for one day's detailed sessions with journal entries, or 'days_back' for per-day summaries over a range. Defaults to today's detail."
    )]
    fn get_history(&self, Parameters(params): Parameters<HistoryParams>) -> String {
        let sessions = match load_all_sessions() {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        if let Some(days_back) = params.days_back {
            let days_back = days_back.min(90) as i64;
            let today = Local::now().date_naive();
            let days: Vec<_> = (0..days_back)
                .rev()
                .map(|back| day_json(&sessions, today - Duration::days(back)))
                .collect();
            return to_pretty(json!({ "days": days }));
        }
        let date = match &params.date {
            Some(d) => match d.parse::<NaiveDate>() {
                Ok(d) => d,
                Err(e) => return error_json(format!("bad date '{d}': {e}")),
            },
            None => Local::now().date_naive(),
        };
        let day_sessions: Vec<_> = sessions
            .iter()
            .filter(|s| s.start_time_local().date_naive() == date)
            .map(session_json)
            .collect();
        to_pretty(json!({
            "summary": day_json(&sessions, date),
            "sessions": day_sessions,
        }))
    }

    #[tool(
        description = "Weekly rhythm: per-day work totals for the last 7 days, current daily streak, week total, average per day, and best day."
    )]
    fn get_weekly_stats(&self) -> String {
        let sessions = match load_all_sessions() {
            Ok(s) => s,
            Err(e) => return error_json(e),
        };
        let today = Local::now().date_naive();
        let days = stats::last_n_days(&sessions, today, 7);
        let week_total = days.iter().fold(Duration::zero(), |acc, d| acc + d.work);
        let best = days.iter().max_by_key(|d| d.work.num_seconds());
        to_pretty(json!({
            "days": days.iter().map(|d| json!({
                "date": d.date.to_string(),
                "weekday": d.date.format("%a").to_string(),
                "work_secs": d.work.num_seconds(),
                "work_human": human_duration(d.work),
                "work_ratio": (d.ratio() * 100.0).round() / 100.0,
            })).collect::<Vec<_>>(),
            "streak_days": stats::streak(&sessions, today),
            "week_work_human": human_duration(week_total),
            "avg_per_day_human": human_duration(week_total / 7),
            "best_day": best.filter(|d| d.work > Duration::zero()).map(|d| json!({
                "date": d.date.to_string(),
                "work_human": human_duration(d.work),
            })),
        }))
    }

    #[tool(
        description = "Send the user a short message. It pops up as a speech bubble from their desk pet in the timer TUI (they press 'm' to dismiss; get_current_status shows whether it was read). Use for gentle nudges: overdue breaks, encouragement, reminders."
    )]
    fn send_message(&self, Parameters(params): Parameters<SendMessageParams>) -> String {
        let inbox_path = match paths::inbox_path() {
            Ok(p) => p,
            Err(e) => return error_json(e),
        };
        let now = Utc::now();
        let tui_online = paths::status_path()
            .ok()
            .and_then(|p| status::read(&p))
            .map(|s| status::is_online(&s, now))
            .unwrap_or(false);
        // Serialize concurrent send_message calls: append is read-modify-write.
        static INBOX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = INBOX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match inbox::append(&inbox_path, params.text) {
            Ok(msg) => to_pretty(json!({
                "delivered": true,
                "message_id": msg.id,
                "tui_online": tui_online,
                "note": if tui_online {
                    "the pet is showing your message now"
                } else {
                    "the TUI is closed; the message will appear next time it opens"
                },
            })),
            Err(e) => error_json(e),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for PetTimerServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info.name = "hamba-timer".to_string();
        info.server_info.version = env!("CARGO_PKG_VERSION").to_string();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Work-pet timer: the user's TUI work tracker with an ASCII desk pet. \
             Use get_current_status for a live 'what are they doing right now' answer, \
             get_today_summary / get_history / get_weekly_stats for tracking questions, \
             and send_message to speak to the user through the pet's speech bubble."
                .to_string(),
        );
        info
    }
}
