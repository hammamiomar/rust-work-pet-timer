use crate::App;
use crate::InputMode;
use crate::assets::*;
use crate::data::{Session, SessionType};
use chrono::Duration;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table},
};

pub fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Safety check
    if area.width < 60 || area.height < 20 {
        f.render_widget(
            Paragraph::new("Terminal too small.\nPlease resize.")
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }

    // --- MAIN LAYOUT ---
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(14), // Top Row (Pet + Dashboard)
            Constraint::Length(3),  // Active Note Bar
            Constraint::Min(10),    // History Table
            Constraint::Length(3),  // Footer
        ])
        .split(area);

    // --- TOP ROW SPLIT ---
    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(62), // Fixed width for Pet (Left)
            Constraint::Fill(1),    // Dashboard (Right - Fills remaining)
        ])
        .split(chunks[0]);

    // 1. PET COMPANION (LEFT)
    let active_session = app.get_active_session();
    let status_color = active_session.session_type.color();

    let frame_lines: &[&str] = match active_session.session_type {
        SessionType::Idle => &FRAME_DEAD,
        _ => FRAMES_ACTIVE[app.animation_index],
    };

    let pet_widget = Paragraph::new(frame_lines.join("\n"))
        .style(Style::default().fg(status_color))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title(" Companion "));

    f.render_widget(pet_widget, top_chunks[0]);

    // 2. DASHBOARD (RIGHT)
    let db_block = Block::default().borders(Borders::ALL).title(" Dashboard ");
    let db_inner = db_block.inner(top_chunks[1]);
    f.render_widget(db_block, top_chunks[1]);

    let db_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Status Label
            Constraint::Length(2), // Timer
            Constraint::Length(2), // Spacer
            Constraint::Length(2), // Gauge Label
            Constraint::Length(2), // Gauge
            Constraint::Fill(1),   // Stats Summary
        ])
        .split(db_inner);

    // A. Status Label
    let status_label = Paragraph::new(active_session.session_type.label())
        .style(
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    f.render_widget(status_label, db_layout[0]);

    // B. Numeric Timer
    let duration = active_session.duration();
    let time_str = format_duration_str(duration);
    let timer_widget = Paragraph::new(time_str)
        .style(
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    f.render_widget(timer_widget, db_layout[1]);

    // C. Work Ratio Gauge
    let (work_dur, break_dur) = app.cached_today_stats;
    let work_secs = work_dur.num_seconds() as f64;
    let break_secs = break_dur.num_seconds() as f64;
    let total_secs = work_secs + break_secs;

    let ratio = if total_secs > 0.0 {
        work_secs / total_secs
    } else {
        0.0
    };

    f.render_widget(
        Paragraph::new("Today's Work Ratio:").alignment(Alignment::Center),
        db_layout[3],
    );

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::NONE))
        .gauge_style(Style::default().fg(Color::Green).bg(Color::Red))
        .ratio(ratio)
        .label(format!("{:.0}% Work", ratio * 100.0))
        .use_unicode(true);
    f.render_widget(gauge, db_layout[4]);

    // --- MIDDLE: NOTE BAR ---
    let note_text = if !active_session.note.is_empty() {
        format!(" NOTE: {}", active_session.note)
    } else {
        " (No note for current session)".to_string()
    };

    let note_widget = Paragraph::new(note_text)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(note_widget, chunks[1]);

    // --- BOTTOM: HISTORY ---
    render_history_table(f, app, chunks[2]);

    // --- FOOTER ---
    render_footer(f, app, chunks[3]);
}

fn render_history_table(f: &mut Frame, app: &mut App, area: Rect) {
    let sessions_for_date: Vec<&Session> = app
        .sessions
        .iter()
        .filter(|s| s.start_time_local().date_naive() == app.selected_date)
        .rev()
        .collect();

    let (total_work, total_break) = app.cached_today_stats;

    let rows: Vec<Row> = sessions_for_date
        .iter()
        .map(|item| {
            let end_str = item
                .end_time_local()
                .map_or("Active".to_string(), |t| t.format("%H:%M:%S").to_string());

            let cells = vec![
                Cell::from(item.start_time_local().format("%H:%M").to_string()),
                Cell::from(end_str),
                Cell::from(item.session_type.label())
                    .style(Style::default().fg(item.session_type.color())),
                Cell::from(format_duration_str(item.duration())),
                Cell::from(item.note.clone()),
            ];
            Row::new(cells).height(1)
        })
        .collect();

    let date_header = format!(" Log: {} ", app.selected_date.format("%Y-%m-%d"));
    let stats_header = format!(
        " Daily Total | Work: {} | Break: {} ",
        format_duration_str(total_work),
        format_duration_str(total_break)
    );

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["Start", "End", "Type", "Time", "Note"])
            .style(Style::default().fg(Color::Cyan)),
    )
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(date_header)
            .title_bottom(stats_header),
    );

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn render_footer(f: &mut Frame, app: &mut App, area: Rect) {
    match app.input_mode {
        InputMode::Normal => {
            let help_text = "SPC:Toggle | 's':Stop | 'n':Note | 'd':Del | \u{2191}\u{2193}:Nav | Enter:Edit | Esc:Clear";
            let help = Paragraph::new(help_text)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::TOP));
            f.render_widget(help, area);
        }
        InputMode::EditingNote => {
            let title = if app.editing_history_index.is_some() {
                " Edit Past Log "
            } else {
                " Edit Current "
            };
            let input = Paragraph::new(format!("> {}", app.input_buffer))
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title(title));
            f.render_widget(input, area);
        }
    }
}

fn format_duration_str(d: Duration) -> String {
    let total_seconds = d.num_seconds();
    let h = total_seconds / 3600;
    let m = (total_seconds % 3600) / 60;
    let s = total_seconds % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}
