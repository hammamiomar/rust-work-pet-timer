use crate::data::{self, Session, SessionType};
use crate::inbox::{self, Ack, InboxMessage};
use crate::mood::{self, Mood};
use crate::paths;
use crate::status::{StatusSnapshot, TodayTotals};
use crate::stats;
use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use ratatui::widgets::TableState;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::SystemTime;
use tui_textarea::TextArea;

#[derive(Clone, Copy, PartialEq)]
pub enum BottomView {
    History,
    Journal,
    Stats,
}

impl BottomView {
    pub fn next(self) -> Self {
        match self {
            BottomView::History => BottomView::Journal,
            BottomView::Journal => BottomView::Stats,
            BottomView::Stats => BottomView::History,
        }
    }
}

pub struct JournalEditor {
    /// Index into `sessions` the entry will be appended to.
    pub target_index: usize,
    pub textarea: TextArea<'static>,
}

pub struct App {
    pub sessions: Vec<Session>,
    pub current_session_index: usize,
    pub animation_index: usize,
    pub selected_date: NaiveDate,
    pub table_state: TableState,
    pub journal_state: TableState,
    pub bottom_view: BottomView,
    pub editor: Option<JournalEditor>,
    pub cached_day_stats: (Duration, Duration),
    pub mood: Mood,
    pub mood_caption: String,
    pub last_error: Option<String>,
    pub last_keypress_at: DateTime<Utc>,
    pub unread: VecDeque<InboxMessage>,
    inbox_mtime: Option<SystemTime>,
    log_path: PathBuf,
    status_path: PathBuf,
    inbox_path: PathBuf,
    ack_path: PathBuf,
    tick_count: u64,
}

impl App {
    pub fn new() -> Result<Self> {
        paths::migrate_legacy_log()?;
        let log_path = paths::work_log_path()?;
        let mut sessions = data::load_sessions(&log_path)?;

        sessions.push(Session::new(SessionType::Idle, Utc::now()));
        let current_session_index = sessions.len() - 1;

        let mut app = App {
            sessions,
            current_session_index,
            animation_index: 0,
            selected_date: Local::now().date_naive(),
            table_state: TableState::default(),
            journal_state: TableState::default(),
            bottom_view: BottomView::History,
            editor: None,
            cached_day_stats: (Duration::zero(), Duration::zero()),
            mood: Mood::Neutral,
            mood_caption: String::new(),
            last_error: None,
            last_keypress_at: Utc::now(),
            unread: VecDeque::new(),
            inbox_mtime: None,
            log_path,
            status_path: paths::status_path()?,
            inbox_path: paths::inbox_path()?,
            ack_path: paths::inbox_ack_path()?,
            tick_count: 0,
        };
        app.update_stats_cache();
        app.refresh_mood();
        app.refresh_inbox();
        app.write_status();
        Ok(app)
    }

    pub fn active_session(&self) -> &Session {
        &self.sessions[self.current_session_index]
    }

    /// Vec-indices of the selected day's sessions in display order (newest first).
    pub fn visible_session_indices(&self) -> Vec<usize> {
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.start_time_local().date_naive() == self.selected_date)
            .map(|(i, _)| i)
            .rev()
            .collect()
    }

    pub fn update_stats_cache(&mut self) {
        let day = stats::day_stat(&self.sessions, self.selected_date);
        self.cached_day_stats = (day.work, day.brk);
    }

    fn persist(&mut self) {
        match data::save_sessions(&self.log_path, &self.sessions) {
            Ok(()) => self.last_error = None,
            Err(e) => self.last_error = Some(format!("save failed: {e:#}")),
        }
    }

    fn start_new_session(&mut self, kind: SessionType) {
        let now = Utc::now();
        let current = &mut self.sessions[self.current_session_index];
        if current.end_time.is_none() {
            current.end_time = Some(now);
        }
        self.sessions.push(Session::new(kind, now));
        self.current_session_index = self.sessions.len() - 1;
        self.persist();
        self.update_stats_cache();
        self.refresh_mood();
        self.write_status();
    }

    pub fn toggle_work_break(&mut self) {
        match self.active_session().session_type {
            SessionType::Work => self.start_new_session(SessionType::Break),
            SessionType::Break | SessionType::Idle => self.start_new_session(SessionType::Work),
        }
    }

    pub fn stop_working(&mut self) {
        if self.active_session().session_type != SessionType::Idle {
            self.start_new_session(SessionType::Idle);
        }
    }

    pub fn delete_selected_entry(&mut self) {
        let Some(table_idx) = self.table_state.selected() else {
            return;
        };
        let Some(&real_idx) = self.visible_session_indices().get(table_idx) else {
            return;
        };
        if real_idx == self.current_session_index {
            return;
        }
        self.sessions.remove(real_idx);
        if real_idx < self.current_session_index {
            self.current_session_index -= 1;
        }
        self.persist();
        self.update_stats_cache();
        self.table_state.select(None);
    }

    /// Reopen the selected (closed) session as the active one — the undo for
    /// accidental toggles: delete the junk blocks with 'd', then 'r' on the
    /// real block to continue it. The gap is absorbed into the resumed block.
    /// Today-only, so an old block can't be resurrected into a monster.
    pub fn resume_selected(&mut self) {
        let Some(table_idx) = self.table_state.selected() else {
            return;
        };
        let Some(&real_idx) = self.visible_session_indices().get(table_idx) else {
            return;
        };
        if real_idx == self.current_session_index {
            return;
        }
        if self.sessions[real_idx].start_time_local().date_naive() != Local::now().date_naive() {
            self.last_error = Some("can only resume one of today's blocks".to_string());
            return;
        }

        let mut target = real_idx;
        let cur = self.current_session_index;
        // The current block is usually the seconds-old accident: drop it if it
        // holds no journal, otherwise close it honestly.
        if self.sessions[cur].entries.is_empty() {
            self.sessions.remove(cur);
            if cur < target {
                target -= 1;
            }
        } else {
            self.sessions[cur].end_time = Some(Utc::now());
        }

        self.sessions[target].end_time = None;
        self.current_session_index = target;
        self.table_state.select(None);
        self.persist();
        self.update_stats_cache();
        self.refresh_mood();
        self.write_status();
    }

    // --- journal ---

    pub fn open_journal_for_current(&mut self) {
        self.open_journal(self.current_session_index);
    }

    pub fn open_journal_for_selected(&mut self) {
        if let Some(table_idx) = self.table_state.selected()
            && let Some(&real_idx) = self.visible_session_indices().get(table_idx) {
                self.open_journal(real_idx);
            }
    }

    fn open_journal(&mut self, target_index: usize) {
        self.editor = Some(JournalEditor {
            target_index,
            textarea: TextArea::default(),
        });
    }

    /// Append the editor's text as a timestamped entry (empty text is a no-op)
    /// and clear the box so the popup works chat-style: type, Enter, keep going.
    pub fn commit_journal_entry(&mut self) {
        let Some(editor) = self.editor.as_mut() else {
            return;
        };
        let text = editor.textarea.lines().join("\n").trim().to_string();
        editor.textarea = TextArea::default();
        let target = editor.target_index;
        if !text.is_empty() {
            self.sessions[target].add_entry(text);
            self.persist();
            self.write_status();
        }
    }

    /// Commit whatever is in the box, then close the popup.
    pub fn save_journal_entry(&mut self) {
        self.commit_journal_entry();
        self.editor = None;
    }

    pub fn cancel_journal(&mut self) {
        self.editor = None;
    }

    /// The selected day's entries across all sessions, chronological.
    pub fn journal_timeline(&self) -> Vec<(DateTime<Local>, SessionType, String)> {
        let mut items: Vec<_> = self
            .sessions
            .iter()
            .filter(|s| s.start_time_local().date_naive() == self.selected_date)
            .flat_map(|s| {
                s.entries
                    .iter()
                    .map(|e| (e.time_local(), s.session_type, e.text.clone()))
            })
            .collect();
        items.sort_by_key(|(t, _, _)| *t);
        items
    }

    // --- navigation ---

    pub fn nav(&mut self, down: bool) {
        let (state, count) = match self.bottom_view {
            BottomView::History => {
                let count = self.visible_session_indices().len();
                (&mut self.table_state, count)
            }
            BottomView::Journal => {
                let count = self.journal_timeline().len();
                (&mut self.journal_state, count)
            }
            BottomView::Stats => return,
        };
        if count == 0 {
            state.select(None);
            return;
        }
        let i = match state.selected() {
            Some(i) if down => (i + 1) % count,
            Some(i) => (i + count - 1) % count,
            None => 0,
        };
        state.select(Some(i));
    }

    pub fn change_date(&mut self, days: i64) {
        self.selected_date += Duration::days(days);
        self.table_state.select(None);
        self.journal_state.select(None);
        self.update_stats_cache();
    }

    pub fn cycle_view(&mut self) {
        self.bottom_view = self.bottom_view.next();
    }

    // --- agent inbox ---

    pub fn dismiss_message(&mut self) {
        if let Some(msg) = self.unread.pop_front()
            && let Err(e) = inbox::save_ack(&self.ack_path, Ack { last_read_id: msg.id }) {
                self.last_error = Some(format!("ack failed: {e:#}"));
            }
    }

    fn refresh_inbox(&mut self) {
        let mtime = std::fs::metadata(&self.inbox_path)
            .and_then(|m| m.modified())
            .ok();
        if mtime == self.inbox_mtime && mtime.is_some() {
            return;
        }
        self.inbox_mtime = mtime;
        let messages = inbox::load(&self.inbox_path);
        let ack = inbox::load_ack(&self.ack_path);
        self.unread = inbox::unread(&messages, ack).into();
    }

    // --- heartbeat / mood ---

    fn refresh_mood(&mut self) {
        let now = Utc::now();
        let today = stats::day_stat(&self.sessions, Local::now().date_naive());
        self.mood = mood::compute(self.active_session(), today.work, today.brk, now);
        self.mood_caption = mood::caption(self.mood, self.active_session(), now);
    }

    fn write_status(&mut self) {
        let today = stats::day_stat(&self.sessions, Local::now().date_naive());
        let current = self.active_session();
        let snapshot = StatusSnapshot {
            schema_version: 1,
            app_alive_at: Utc::now(),
            last_keypress_at: self.last_keypress_at,
            state: current.session_type,
            session_started_at: current.start_time,
            mood: self.mood.label().to_string(),
            mood_caption: self.mood_caption.clone(),
            today: TodayTotals {
                work_secs: today.work.num_seconds(),
                break_secs: today.brk.num_seconds(),
            },
            latest_journal_entry: current.latest_entry().cloned(),
            unread_agent_messages: self.unread.len() as u32,
            pid: std::process::id(),
        };
        if let Err(e) = crate::status::write(&self.status_path, &snapshot) {
            self.last_error = Some(format!("status write failed: {e:#}"));
        }
    }

    pub fn note_keypress(&mut self) {
        self.last_keypress_at = Utc::now();
    }

    pub fn on_tick(&mut self) {
        self.animation_index = (self.animation_index + 1) % crate::assets::FRAMES_ACTIVE.len();
        self.tick_count += 1;
        if self.selected_date == Local::now().date_naive() {
            self.update_stats_cache();
        }
        // Roughly once a second at the 200ms tick rate.
        if self.tick_count.is_multiple_of(5) {
            self.refresh_mood();
            self.refresh_inbox();
            self.write_status();
        }
    }

    /// Close the open session honestly instead of leaving it to inflate on next load.
    pub fn on_quit(&mut self) {
        let now = Utc::now();
        let current = &mut self.sessions[self.current_session_index];
        if current.end_time.is_none() {
            current.end_time = Some(now);
        }
        self.persist();
        crate::status::remove(&self.status_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an App around explicit temp paths so tests avoid env vars.
    fn test_app(name: &str, sessions: Vec<Session>, current: usize) -> App {
        let dir = std::env::temp_dir().join(format!("pet-timer-app-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        App {
            sessions,
            current_session_index: current,
            animation_index: 0,
            selected_date: Local::now().date_naive(),
            table_state: TableState::default(),
            journal_state: TableState::default(),
            bottom_view: BottomView::History,
            editor: None,
            cached_day_stats: (Duration::zero(), Duration::zero()),
            mood: Mood::Neutral,
            mood_caption: String::new(),
            last_error: None,
            last_keypress_at: Utc::now(),
            unread: VecDeque::new(),
            inbox_mtime: None,
            log_path: dir.join("work_log.json"),
            status_path: dir.join("status.json"),
            inbox_path: dir.join("inbox.json"),
            ack_path: dir.join("inbox_ack.json"),
            tick_count: 0,
        }
    }

    #[test]
    fn resume_reopens_block_and_drops_empty_accident() {
        let now = Utc::now();
        let mut work = Session::new(SessionType::Work, now - Duration::minutes(60));
        work.end_time = Some(now - Duration::minutes(1));
        let junk = Session::new(SessionType::Idle, now - Duration::minutes(1));

        let mut app = test_app("resume", vec![work, junk], 1);
        // Display order is newest-first: row 0 = junk idle, row 1 = the work block.
        app.table_state.select(Some(1));
        app.resume_selected();

        assert_eq!(app.sessions.len(), 1, "empty accidental block is dropped");
        assert_eq!(app.current_session_index, 0);
        assert_eq!(app.active_session().session_type, SessionType::Work);
        assert!(app.active_session().end_time.is_none(), "block is live again");
        assert!(app.active_session().duration() >= Duration::minutes(59));
    }

    #[test]
    fn resume_keeps_accident_with_journal() {
        let now = Utc::now();
        let mut work = Session::new(SessionType::Work, now - Duration::minutes(30));
        work.end_time = Some(now - Duration::minutes(2));
        let mut brk = Session::new(SessionType::Break, now - Duration::minutes(2));
        brk.add_entry("actually took a real break".to_string());

        let mut app = test_app("resume-keep", vec![work, brk], 1);
        app.table_state.select(Some(1));
        app.resume_selected();

        assert_eq!(app.sessions.len(), 2, "journaled block is kept, not dropped");
        assert!(app.sessions[1].end_time.is_some(), "kept block is closed");
        assert_eq!(app.current_session_index, 0);
        assert!(app.active_session().end_time.is_none());
    }

    #[test]
    fn resume_rejects_past_days() {
        let now = Utc::now();
        let mut old = Session::new(SessionType::Work, now - Duration::days(2));
        old.end_time = Some(now - Duration::days(2) + Duration::hours(1));
        let current = Session::new(SessionType::Idle, now);

        let mut app = test_app("resume-old", vec![old, current], 1);
        app.selected_date = Local::now().date_naive() - Duration::days(2);
        app.table_state.select(Some(0));
        app.resume_selected();

        assert_eq!(app.current_session_index, 1, "nothing changed");
        assert!(app.sessions[0].end_time.is_some());
        assert!(app.last_error.is_some());
    }
}
