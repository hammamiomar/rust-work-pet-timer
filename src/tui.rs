use crate::app::App;
use crate::ui;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent,
        KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use std::{io, time::Instant};

pub fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let result = match app {
        Ok(ref mut app) => event_loop(&mut terminal, app),
        Err(e) => Err(e),
    };

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    let tick_rate = std::time::Duration::from_millis(200);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui::ui(f, app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(std::time::Duration::ZERO);
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press {
                    let quit = if app.editor.is_some() {
                        handle_editor_key(app, key);
                        false
                    } else {
                        handle_normal_key(app, key)
                    };
                    if quit {
                        break;
                    }
                }
        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = Instant::now();
        }
    }
    app.on_quit();
    Ok(())
}

fn handle_editor_key(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        // Chat-style: Enter logs the entry and keeps the popup open.
        KeyCode::Enter if !alt => app.commit_journal_entry(),
        KeyCode::Char('s') if ctrl => app.save_journal_entry(),
        KeyCode::Esc => app.cancel_journal(),
        _ => {
            if let Some(editor) = app.editor.as_mut() {
                if key.code == KeyCode::Enter {
                    editor.textarea.insert_newline();
                } else {
                    editor.textarea.input(key);
                }
            }
        }
    }
}

/// Returns true when the app should quit.
fn handle_normal_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') => return true,
        KeyCode::Char(' ') => app.toggle_work_break(),
        KeyCode::Char('s') => app.stop_working(),
        KeyCode::Left => app.change_date(-1),
        KeyCode::Right => app.change_date(1),
        KeyCode::Down => app.nav(true),
        KeyCode::Up => app.nav(false),
        KeyCode::Tab => app.cycle_view(),
        KeyCode::Esc => {
            app.table_state.select(None);
            app.journal_state.select(None);
        }
        KeyCode::Char('d') => app.delete_selected_entry(),
        KeyCode::Char('r') => app.resume_selected(),
        KeyCode::Char('n') | KeyCode::Char('j') => app.open_journal_for_current(),
        KeyCode::Char('m') => app.dismiss_message(),
        KeyCode::Enter => app.open_journal_for_selected(),
        _ => {}
    }
    false
}
