use crate::data::{self, Session, SessionType};
use crate::inbox::{self, Ack, InboxMessage};
use crate::mood::{self, Mood};
use crate::paths;
use crate::stats;
use crate::status::{StatusSnapshot, TodayTotals};
use anyhow::Result;
use chrono::{Duration, Local, NaiveDate, Utc};
use ratatui::widgets::TableState;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::SystemTime;
use tui_textarea::TextArea;

pub struct JournalEditor {
    /// Index into `sessions` containing the entry being added or edited.
    pub target_index: usize,
    /// `None` appends a new entry; `Some` edits an existing entry in place.
    pub entry_index: Option<usize>,
    pub textarea: TextArea<'static>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteTarget {
    Session(usize),
    JournalEntry {
        session_index: usize,
        entry_index: usize,
    },
}

pub struct App {
    pub sessions: Vec<Session>,
    pub current_session_index: usize,
    pub animation_index: usize,
    pub selected_date: NaiveDate,
    pub table_state: TableState,
    /// Real index into `sessions` for the one expanded work-log row.
    pub expanded_session_index: Option<usize>,
    /// Selected child inside the expanded row. `entries.len()` is "+ add note".
    /// `None` means keyboard focus is on timer-block rows.
    pub journal_selection: Option<usize>,
    pub editor: Option<JournalEditor>,
    pub pending_delete: Option<DeleteTarget>,
    pub cached_day_stats: (Duration, Duration),
    pub mood: Mood,
    pub mood_caption: String,
    pub last_error: Option<String>,
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
            table_state: TableState::default().with_selected(Some(0)),
            expanded_session_index: Some(current_session_index),
            journal_selection: None,
            editor: None,
            pending_delete: None,
            cached_day_stats: (Duration::zero(), Duration::zero()),
            mood: Mood::Neutral,
            mood_caption: String::new(),
            last_error: None,
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
        self.focus_session(self.current_session_index, false);
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

    /// Arm deletion of the selected note or timer block; a second call on the
    /// same selection confirms it.
    pub fn delete_selected_entry(&mut self) {
        let Some(target) = self.selected_delete_target() else {
            return;
        };
        if target == DeleteTarget::Session(self.current_session_index) {
            self.pending_delete = None;
            return;
        }
        if self.pending_delete == Some(target) {
            self.confirm_delete(target);
        } else {
            self.pending_delete = Some(target);
        }
    }

    pub fn cancel_delete_confirmation(&mut self) {
        self.pending_delete = None;
    }

    fn selected_delete_target(&self) -> Option<DeleteTarget> {
        if let (Some(session_index), Some(entry_index)) =
            (self.expanded_session_index, self.journal_selection)
        {
            if entry_index < self.sessions.get(session_index)?.entries.len() {
                return Some(DeleteTarget::JournalEntry {
                    session_index,
                    entry_index,
                });
            }
            return None;
        }

        let table_idx = self.table_state.selected()?;
        let real_idx = *self.visible_session_indices().get(table_idx)?;
        Some(DeleteTarget::Session(real_idx))
    }

    fn confirm_delete(&mut self, target: DeleteTarget) {
        self.pending_delete = None;
        match target {
            DeleteTarget::JournalEntry {
                session_index,
                entry_index,
            } => {
                let Some(session) = self.sessions.get_mut(session_index) else {
                    return;
                };
                if entry_index >= session.entries.len() {
                    return;
                }
                session.entries.remove(entry_index);
                self.journal_selection = Some(entry_index.min(session.entries.len()));
            }
            DeleteTarget::Session(real_idx) => {
                if real_idx >= self.sessions.len() || real_idx == self.current_session_index {
                    return;
                }
                self.sessions.remove(real_idx);
                if real_idx < self.current_session_index {
                    self.current_session_index -= 1;
                }
                self.expanded_session_index = match self.expanded_session_index {
                    Some(i) if i == real_idx => None,
                    Some(i) if i > real_idx => Some(i - 1),
                    other => other,
                };
                self.journal_selection = None;
                let count = self.visible_session_indices().len();
                let next = self
                    .table_state
                    .selected()
                    .and_then(|i| count.checked_sub(1).map(|last| i.min(last)));
                self.table_state.select(next);
            }
        }
        self.persist();
        self.update_stats_cache();
        self.write_status();
    }

    /// Reopen the selected (closed) session as the active one — the undo for
    /// accidental toggles: confirm deletion of junk blocks with 'd' twice,
    /// then press 'r' on the real block. The gap is absorbed into that block.
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
        self.focus_session(target, false);
        self.persist();
        self.update_stats_cache();
        self.refresh_mood();
        self.write_status();
    }

    // --- journal ---

    pub fn open_journal_for_current(&mut self) {
        self.begin_journal_add(self.current_session_index);
    }

    pub fn open_journal_for_selected(&mut self) {
        if let Some(table_idx) = self.table_state.selected()
            && let Some(&real_idx) = self.visible_session_indices().get(table_idx)
        {
            self.begin_journal_add(real_idx);
        }
    }

    fn begin_journal_add(&mut self, target_index: usize) {
        if target_index >= self.sessions.len() {
            return;
        }
        self.focus_session(target_index, true);
        let add_index = self.sessions[target_index].entries.len();
        self.journal_selection = Some(add_index);
        self.editor = Some(JournalEditor {
            target_index,
            entry_index: None,
            textarea: TextArea::default(),
        });
        self.pending_delete = None;
    }

    fn begin_journal_edit(&mut self, target_index: usize, entry_index: usize) {
        let Some(entry) = self
            .sessions
            .get(target_index)
            .and_then(|s| s.entries.get(entry_index))
        else {
            return;
        };
        let mut textarea = TextArea::from(entry.text.split('\n'));
        textarea.move_cursor(tui_textarea::CursorMove::Bottom);
        textarea.move_cursor(tui_textarea::CursorMove::End);
        self.editor = Some(JournalEditor {
            target_index,
            entry_index: Some(entry_index),
            textarea,
        });
        self.pending_delete = None;
    }

    /// Save the current inline edit. New notes keep a fresh blank bullet open;
    /// existing notes return to the child list after being updated.
    pub fn commit_journal_entry(&mut self) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let text = editor.textarea.lines().join("\n").trim().to_string();
        let target = editor.target_index;
        let entry_index = editor.entry_index;
        if text.is_empty() {
            return;
        }

        match entry_index {
            Some(i) => {
                let Some(entry) = self
                    .sessions
                    .get_mut(target)
                    .and_then(|s| s.entries.get_mut(i))
                else {
                    self.editor = None;
                    return;
                };
                entry.text = text;
                self.journal_selection = Some(i);
                self.editor = None;
            }
            None => {
                let Some(session) = self.sessions.get_mut(target) else {
                    self.editor = None;
                    return;
                };
                session.add_entry(text);
                self.journal_selection = Some(session.entries.len());
                if let Some(editor) = self.editor.as_mut() {
                    editor.textarea = TextArea::default();
                }
            }
        }
        self.persist();
        self.write_status();
    }

    /// Save a non-empty draft and leave inline editing.
    pub fn save_journal_entry(&mut self) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let text = editor.textarea.lines().join("\n").trim().to_string();
        let target = editor.target_index;
        let entry_index = editor.entry_index;

        if !text.is_empty() {
            match entry_index {
                Some(i) => {
                    if let Some(entry) = self
                        .sessions
                        .get_mut(target)
                        .and_then(|s| s.entries.get_mut(i))
                    {
                        entry.text = text;
                        self.journal_selection = Some(i);
                    }
                }
                None => {
                    if let Some(session) = self.sessions.get_mut(target) {
                        session.add_entry(text);
                        self.journal_selection = Some(session.entries.len());
                    }
                }
            }
            self.persist();
            self.write_status();
        }
        self.editor = None;
    }

    pub fn cancel_journal(&mut self) {
        self.editor = None;
    }

    /// Activate the selected timer row or journal child.
    pub fn activate_selected(&mut self) {
        self.pending_delete = None;
        if let (Some(session_index), Some(child_index)) =
            (self.expanded_session_index, self.journal_selection)
        {
            let Some(session) = self.sessions.get(session_index) else {
                return;
            };
            if child_index < session.entries.len() {
                self.begin_journal_edit(session_index, child_index);
            } else {
                self.begin_journal_add(session_index);
            }
            return;
        }

        let Some(table_idx) = self.table_state.selected() else {
            return;
        };
        let Some(&real_idx) = self.visible_session_indices().get(table_idx) else {
            return;
        };
        self.expanded_session_index = Some(real_idx);
        self.journal_selection = Some(self.sessions[real_idx].entries.len());
    }

    /// Back out one level: cancel delete, close an expanded journal, or clear
    /// the timer-block selection.
    pub fn escape(&mut self) {
        if self.pending_delete.take().is_some() {
            return;
        }
        if self.expanded_session_index.is_some() {
            self.expanded_session_index = None;
            self.journal_selection = None;
            return;
        }
        self.table_state.select(None);
    }

    // --- navigation ---

    pub fn nav(&mut self, down: bool) {
        self.pending_delete = None;
        if let (Some(session_index), Some(i)) =
            (self.expanded_session_index, self.journal_selection)
        {
            let last = self
                .sessions
                .get(session_index)
                .map_or(0, |s| s.entries.len());
            self.journal_selection = Some(if down {
                (i + 1).min(last)
            } else {
                i.saturating_sub(1)
            });
            return;
        }

        let count = self.visible_session_indices().len();
        let state = &mut self.table_state;
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
        self.expanded_session_index = None;
        self.journal_selection = None;
        self.editor = None;
        self.pending_delete = None;
        self.update_stats_cache();
    }

    fn focus_session(&mut self, real_index: usize, editing: bool) {
        self.selected_date = self.sessions[real_index].start_time_local().date_naive();
        let row = self
            .visible_session_indices()
            .iter()
            .position(|&index| index == real_index);
        self.table_state.select(row);
        self.expanded_session_index = Some(real_index);
        if !editing {
            self.journal_selection = None;
            self.editor = None;
        }
        self.pending_delete = None;
        self.update_stats_cache();
    }

    // --- agent inbox ---

    pub fn dismiss_message(&mut self) {
        if let Some(msg) = self.unread.pop_front()
            && let Err(e) = inbox::save_ack(
                &self.ack_path,
                Ack {
                    last_read_id: msg.id,
                },
            )
        {
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
            expanded_session_index: Some(current),
            journal_selection: None,
            editor: None,
            pending_delete: None,
            cached_day_stats: (Duration::zero(), Duration::zero()),
            mood: Mood::Neutral,
            mood_caption: String::new(),
            last_error: None,
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
        assert!(
            app.active_session().end_time.is_none(),
            "block is live again"
        );
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

        assert_eq!(
            app.sessions.len(),
            2,
            "journaled block is kept, not dropped"
        );
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

    #[test]
    fn new_timer_block_becomes_the_only_expanded_row() {
        let now = Utc::now();
        let current = Session::new(SessionType::Idle, now);
        let mut app = test_app("new-expanded", vec![current], 0);
        app.selected_date = Local::now().date_naive() - Duration::days(1);

        app.toggle_work_break();

        assert_eq!(app.current_session_index, 1);
        assert_eq!(app.expanded_session_index, Some(1));
        assert_eq!(app.journal_selection, None);
        assert_eq!(app.table_state.selected(), Some(0));
        assert_eq!(app.selected_date, Local::now().date_naive());
        assert_eq!(app.active_session().session_type, SessionType::Work);
    }

    #[test]
    fn inline_journal_adds_repeated_bullets_and_edits_in_place() {
        let now = Utc::now();
        let current = Session::new(SessionType::Work, now);
        let mut app = test_app("inline-journal", vec![current], 0);

        app.open_journal_for_current();
        app.editor.as_mut().unwrap().textarea.insert_str("first");
        app.commit_journal_entry();
        assert_eq!(app.sessions[0].entries.len(), 1);
        assert_eq!(app.sessions[0].entries[0].text, "first");
        assert!(app.editor.is_some(), "new-note editor stays open");
        assert_eq!(app.journal_selection, Some(1));

        app.editor.as_mut().unwrap().textarea.insert_str("second");
        app.save_journal_entry();
        assert_eq!(app.sessions[0].entries.len(), 2);
        assert!(app.editor.is_none());

        let original_time = app.sessions[0].entries[0].time;
        app.journal_selection = Some(0);
        app.activate_selected();
        app.editor.as_mut().unwrap().textarea.insert_str(" edited");
        app.commit_journal_entry();

        assert_eq!(app.sessions[0].entries[0].text, "first edited");
        assert_eq!(app.sessions[0].entries[0].time, original_time);
        assert!(app.editor.is_none(), "editing an existing note exits");
    }

    #[test]
    fn journal_delete_requires_the_same_target_twice() {
        let now = Utc::now();
        let mut current = Session::new(SessionType::Work, now);
        current.add_entry("keep until confirmed".to_string());
        let mut app = test_app("confirm-delete", vec![current], 0);
        app.table_state.select(Some(0));
        app.journal_selection = Some(0);

        app.delete_selected_entry();
        assert_eq!(
            app.pending_delete,
            Some(DeleteTarget::JournalEntry {
                session_index: 0,
                entry_index: 0
            })
        );
        assert_eq!(app.sessions[0].entries.len(), 1);

        app.delete_selected_entry();
        assert!(app.pending_delete.is_none());
        assert!(app.sessions[0].entries.is_empty());
        assert_eq!(app.journal_selection, Some(0), "selection moves to + add");
    }

    #[test]
    fn timer_block_delete_requires_confirmation_and_keeps_active_index_valid() {
        let now = Utc::now();
        let mut closed = Session::new(SessionType::Work, now - Duration::minutes(10));
        closed.end_time = Some(now - Duration::minutes(1));
        let active = Session::new(SessionType::Idle, now - Duration::minutes(1));
        let mut app = test_app("confirm-block-delete", vec![closed, active], 1);
        app.expanded_session_index = None;
        app.table_state.select(Some(1)); // Newest-first row 1 is the closed block.

        app.delete_selected_entry();
        assert_eq!(app.pending_delete, Some(DeleteTarget::Session(0)));
        assert_eq!(app.sessions.len(), 2);

        app.delete_selected_entry();
        assert_eq!(app.sessions.len(), 1);
        assert_eq!(app.current_session_index, 0);
        assert_eq!(app.active_session().session_type, SessionType::Idle);
    }

    #[test]
    fn cancel_inline_edit_keeps_saved_text() {
        let now = Utc::now();
        let mut current = Session::new(SessionType::Work, now);
        current.add_entry("original".to_string());
        let mut app = test_app("cancel-edit", vec![current], 0);
        app.journal_selection = Some(0);

        app.activate_selected();
        app.editor.as_mut().unwrap().textarea.insert_str(" changed");
        app.cancel_journal();

        assert_eq!(app.sessions[0].entries[0].text, "original");
    }
}
