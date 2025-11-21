mod assets;
mod data;
mod ui;

use crate::data::*;
use anyhow::Result;
use chrono::{Duration, Local, NaiveDate, Utc};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::*, widgets::TableState};
use std::{io, time::Instant};

struct App {
    sessions: Vec<Session>,
    current_session_index: Option<usize>,
    input_mode: InputMode,
    input_buffer: String,
    animation_index: usize,
    selected_date: NaiveDate,
    table_state: TableState,
    editing_history_index: Option<usize>,
    cached_today_stats: (Duration, Duration),
}

#[derive(PartialEq)]
enum InputMode {
    Normal,
    EditingNote,
}

impl App {
    fn new() -> Self {
        let mut sessions = load_sessions().unwrap_or_default();

        // Create new idle session
        let idle_session = Session {
            start_time: Utc::now(),
            end_time: None,
            session_type: SessionType::Idle,
            note: String::new(),
        };

        sessions.push(idle_session);
        let idx = sessions.len() - 1;

        let mut app = App {
            sessions,
            current_session_index: Some(idx),
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            animation_index: 0,
            selected_date: Local::now().date_naive(),
            table_state: TableState::default(),
            editing_history_index: None,
            cached_today_stats: (Duration::zero(), Duration::zero()),
        };

        app.update_stats_cache();
        app
    }

    fn update_stats_cache(&mut self) {
        let mut total_work = Duration::zero();
        let mut total_break = Duration::zero();

        for s in self
            .sessions
            .iter()
            .filter(|s| s.start_time_local().date_naive() == self.selected_date)
        {
            let dur = s.duration();
            match s.session_type {
                SessionType::Work => total_work = total_work + dur,
                SessionType::Break => total_break = total_break + dur,
                _ => {}
            }
        }
        self.cached_today_stats = (total_work, total_break);
    }

    fn start_new_session(&mut self, kind: SessionType) {
        let now = Utc::now();
        if let Some(idx) = self.current_session_index {
            if self.sessions[idx].end_time.is_none() {
                self.sessions[idx].end_time = Some(now);
            }
        }
        let new_session = Session {
            start_time: now,
            end_time: None,
            session_type: kind,
            note: String::new(),
        };
        self.sessions.push(new_session);
        self.current_session_index = Some(self.sessions.len() - 1);
        save_sessions(&self.sessions).ok();
        self.update_stats_cache();
    }

    fn toggle_work_break(&mut self) {
        if let Some(idx) = self.current_session_index {
            match self.sessions[idx].session_type {
                SessionType::Work => self.start_new_session(SessionType::Break),
                SessionType::Break => self.start_new_session(SessionType::Work),
                SessionType::Idle => self.start_new_session(SessionType::Work),
            }
        }
    }

    fn stop_working(&mut self) {
        if let Some(idx) = self.current_session_index {
            if self.sessions[idx].session_type != SessionType::Idle {
                self.start_new_session(SessionType::Idle);
            }
        }
    }

    fn get_active_session(&self) -> &Session {
        &self.sessions[self.current_session_index.unwrap()]
    }

    fn delete_selected_entry(&mut self) {
        if let Some(table_idx) = self.table_state.selected() {
            let date_indices: Vec<usize> = self
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| s.start_time_local().date_naive() == self.selected_date)
                .map(|(i, _)| i)
                .rev()
                .collect();

            if let Some(&real_idx) = date_indices.get(table_idx) {
                if Some(real_idx) == self.current_session_index {
                    return;
                }
                self.sessions.remove(real_idx);
                if let Some(curr) = self.current_session_index {
                    if real_idx < curr {
                        self.current_session_index = Some(curr - 1);
                    }
                }
                save_sessions(&self.sessions).ok();
                self.update_stats_cache();
                self.table_state.select(None);
            }
        }
    }

    fn save_note(&mut self) {
        if let Some(idx) = self.editing_history_index {
            self.sessions[idx].note = self.input_buffer.clone();
        } else if let Some(idx) = self.current_session_index {
            self.sessions[idx].note = self.input_buffer.clone();
        }
        save_sessions(&self.sessions).ok();
        self.editing_history_index = None;
    }

    fn on_tick(&mut self) {
        self.animation_index = (self.animation_index + 1) % crate::assets::FRAMES_ACTIVE.len();
        if self.selected_date == Local::now().date_naive() {
            self.update_stats_cache();
        }
    }

    fn change_date(&mut self, days: i64) {
        self.selected_date = self.selected_date + Duration::days(days);
        self.table_state.select(None);
        self.update_stats_cache();
    }
}

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let tick_rate = std::time::Duration::from_millis(200);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui::ui(f, &mut app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(std::time::Duration::ZERO);
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char(' ') => app.toggle_work_break(),
                            KeyCode::Char('s') => app.stop_working(),
                            KeyCode::Left => app.change_date(-1),
                            KeyCode::Right => app.change_date(1),
                            KeyCode::Down => {
                                let i = match app.table_state.selected() {
                                    Some(i) => {
                                        let count = app
                                            .sessions
                                            .iter()
                                            .filter(|s| {
                                                s.start_time_local().date_naive()
                                                    == app.selected_date
                                            })
                                            .count();
                                        if count == 0 {
                                            0
                                        } else if i >= count - 1 {
                                            0
                                        } else {
                                            i + 1
                                        }
                                    }
                                    None => 0,
                                };
                                app.table_state.select(Some(i));
                            }
                            KeyCode::Up => {
                                let i = match app.table_state.selected() {
                                    Some(i) => {
                                        let count = app
                                            .sessions
                                            .iter()
                                            .filter(|s| {
                                                s.start_time_local().date_naive()
                                                    == app.selected_date
                                            })
                                            .count();
                                        if count == 0 {
                                            0
                                        } else if i == 0 {
                                            count - 1
                                        } else {
                                            i - 1
                                        }
                                    }
                                    None => 0,
                                };
                                app.table_state.select(Some(i));
                            }
                            KeyCode::Esc => app.table_state.select(None),
                            KeyCode::Char('d') => app.delete_selected_entry(),
                            KeyCode::Char('n') => {
                                app.input_mode = InputMode::EditingNote;
                                app.input_buffer = app.get_active_session().note.clone();
                                app.editing_history_index = None;
                            }
                            KeyCode::Enter => {
                                if let Some(selected_idx) = app.table_state.selected() {
                                    let date_indices: Vec<usize> = app
                                        .sessions
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, s)| {
                                            s.start_time_local().date_naive() == app.selected_date
                                        })
                                        .map(|(i, _)| i)
                                        .rev()
                                        .collect();
                                    if let Some(&real_idx) = date_indices.get(selected_idx) {
                                        app.input_mode = InputMode::EditingNote;
                                        app.input_buffer = app.sessions[real_idx].note.clone();
                                        app.editing_history_index = Some(real_idx);
                                    }
                                }
                            }
                            _ => {}
                        },
                        InputMode::EditingNote => match key.code {
                            KeyCode::Enter => {
                                app.save_note();
                                app.input_mode = InputMode::Normal;
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                                app.editing_history_index = None;
                            }
                            KeyCode::Backspace => {
                                app.input_buffer.pop();
                            }
                            KeyCode::Char(c) => {
                                app.input_buffer.push(c);
                            }
                            _ => {}
                        },
                    }
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}
