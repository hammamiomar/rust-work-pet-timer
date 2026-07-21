use crate::app::{App, BottomView};
use crate::assets::*;
use crate::data::SessionType;
use crate::stats;
use chrono::{Duration, Local};
use ratatui::{
    prelude::*,
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Cell, Clear, Gauge, Paragraph, Row,
        Table, Wrap},
};

pub fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    if area.width < 60 || area.height < 20 {
        f.render_widget(
            Paragraph::new("Terminal too small.\nPlease resize.")
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(15), // Pet + Dashboard
            Constraint::Length(3),  // Latest journal entry
            Constraint::Min(10),    // History / Journal / Stats
            Constraint::Length(3),  // Footer
        ])
        .split(area);

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(62), Constraint::Fill(1)])
        .split(chunks[0]);

    render_pet(f, app, top_chunks[0]);
    render_dashboard(f, app, top_chunks[1]);
    render_journal_bar(f, app, chunks[1]);

    match app.bottom_view {
        BottomView::History => render_history_table(f, app, chunks[2]),
        BottomView::Journal => render_journal_timeline(f, app, chunks[2]),
        BottomView::Stats => render_stats(f, app, chunks[2]),
    }

    render_footer(f, app, chunks[3]);

    // Overlays last, painter's order.
    if app.editor.is_some() {
        render_journal_editor(f, app, area);
    } else if !app.unread.is_empty() {
        render_speech_bubble(f, app, area);
    }
}

fn render_pet(f: &mut Frame, app: &App, area: Rect) {
    let active_session = app.active_session();
    let status_color = active_session.session_type.color();

    let frame_lines: &[&str] = match active_session.session_type {
        SessionType::Idle => &FRAME_DEAD,
        _ => FRAMES_ACTIVE[app.animation_index],
    };

    let mut text: Vec<Line> = frame_lines
        .iter()
        .map(|l| Line::styled(*l, Style::default().fg(status_color)))
        .collect();
    text.push(Line::styled(
        format!("{}  {}", app.mood.face(), app.mood_caption),
        Style::default().fg(app.mood.color()).add_modifier(Modifier::ITALIC),
    ));

    let pet_widget = Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title(" ur brain "));
    f.render_widget(pet_widget, area);
}

fn render_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let active_session = app.active_session();
    let status_color = active_session.session_type.color();

    let db_block = Block::default().borders(Borders::ALL);
    let db_inner = db_block.inner(area);
    f.render_widget(db_block, area);

    let db_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Status label
            Constraint::Length(2), // Timer
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Gauge label
            Constraint::Length(2), // Gauge
            Constraint::Fill(1),   // Streak / agent info
        ])
        .split(db_inner);

    let status_label = Paragraph::new(active_session.session_type.label())
        .style(Style::default().fg(status_color).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    f.render_widget(status_label, db_layout[0]);

    let timer_widget = Paragraph::new(format_duration_str(active_session.duration()))
        .style(Style::default().fg(status_color).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    f.render_widget(timer_widget, db_layout[1]);

    let (work_dur, break_dur) = app.cached_day_stats;
    let work_secs = work_dur.num_seconds() as f64;
    let total_secs = work_secs + break_dur.num_seconds() as f64;
    let ratio = if total_secs > 0.0 { work_secs / total_secs } else { 0.0 };

    f.render_widget(
        Paragraph::new("Work Ratio:").alignment(Alignment::Center),
        db_layout[3],
    );
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::NONE))
        .gauge_style(Style::default().fg(Color::Green).bg(Color::Red))
        .ratio(ratio)
        .label(format!("{:.0}% Work", ratio * 100.0))
        .use_unicode(true);
    f.render_widget(gauge, db_layout[4]);

    let today = Local::now().date_naive();
    let streak = stats::streak(&app.sessions, today);
    let mut info = vec![Line::from(format!("streak: {} day{}", streak, if streak == 1 { "" } else { "s" }))];
    if !app.unread.is_empty() {
        info.push(Line::styled(
            format!("hermes: {} unread — 'm' to read", app.unread.len()),
            Style::default().fg(Color::Cyan),
        ));
    }
    f.render_widget(
        Paragraph::new(info).alignment(Alignment::Center),
        db_layout[5],
    );
}

fn render_journal_bar(f: &mut Frame, app: &App, area: Rect) {
    let text = match app.active_session().latest_entry() {
        Some(e) => format!(
            " {} ▸ {}",
            e.time_local().format("%H:%M"),
            e.text.replace('\n', " ")
        ),
        None => " (no journal entries this session — press 'n' to log what you're doing)".to_string(),
    };
    let widget = Paragraph::new(text)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::ALL).title(" journal "));
    f.render_widget(widget, area);
}

fn render_history_table(f: &mut Frame, app: &mut App, area: Rect) {
    let indices = app.visible_session_indices();
    let (total_work, total_break) = app.cached_day_stats;

    let rows: Vec<Row> = indices
        .iter()
        .map(|&i| {
            let item = &app.sessions[i];
            let end_str = item
                .end_time_local()
                .map_or("Active".to_string(), |t| t.format("%H:%M:%S").to_string());
            let journal = match item.entries.len() {
                0 => String::new(),
                1 => item.entries[0].text.replace('\n', " "),
                n => format!(
                    "{} (+{})",
                    item.entries.last().unwrap().text.replace('\n', " "),
                    n - 1
                ),
            };

            Row::new(vec![
                Cell::from(item.start_time_local().format("%H:%M").to_string()),
                Cell::from(end_str),
                Cell::from(item.session_type.label())
                    .style(Style::default().fg(item.session_type.color())),
                Cell::from(format_duration_str(item.duration())),
                Cell::from(journal),
            ])
            .height(1)
        })
        .collect();

    let date_header = format!(" Log: {}  [Tab: journal ▸ stats] ", app.selected_date.format("%Y-%m-%d"));
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
        Row::new(vec!["Start", "End", "Type", "Time", "Journal"])
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

fn render_journal_timeline(f: &mut Frame, app: &mut App, area: Rect) {
    let timeline = app.journal_timeline();

    let rows: Vec<Row> = timeline
        .iter()
        .map(|(time, kind, text)| {
            Row::new(vec![
                Cell::from(time.format("%H:%M").to_string()),
                Cell::from(kind.label()).style(Style::default().fg(kind.color())),
                Cell::from(text.replace('\n', " ")),
            ])
            .height(1)
        })
        .collect();

    let empty = rows.is_empty();
    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Min(10),
        ],
    )
    .header(Row::new(vec!["Time", "Mode", "What you were doing"]).style(Style::default().fg(Color::Cyan)))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " Journal: {}  [Tab: stats ▸ history] ",
                app.selected_date.format("%Y-%m-%d")
            )),
    );

    f.render_stateful_widget(table, area, &mut app.journal_state);

    if empty {
        let hint = Paragraph::new("no journal entries this day — press 'n' while working to log")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        let inner = area.inner(Margin::new(1, 2));
        f.render_widget(hint, inner);
    }
}

fn render_stats(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Stats — last 7 days  [Tab: history ▸ journal] ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1), Constraint::Length(34)])
        .split(inner);

    let today = Local::now().date_naive();
    let days = stats::last_n_days(&app.sessions, today, 7);

    let bars: Vec<Bar> = days
        .iter()
        .map(|d| {
            let mins = d.work.num_minutes().max(0) as u64;
            Bar::default()
                .label(Line::from(d.date.format("%a").to_string()))
                .value(mins)
                .text_value(format_hours_short(d.work))
                .style(Style::default().fg(if d.date == today {
                    Color::Cyan
                } else {
                    Color::Green
                }))
        })
        .collect();

    let chart = BarChart::default()
        .data(BarGroup::default().bars(&bars))
        .bar_width(7)
        .bar_gap(1);
    f.render_widget(chart, cols[0]);

    let week_total = days
        .iter()
        .fold(Duration::zero(), |acc, d| acc + d.work);
    let avg = week_total / 7;
    let best = days.iter().max_by_key(|d| d.work.num_seconds());
    let streak = stats::streak(&app.sessions, today);
    let today_stat = days.last().unwrap();

    let mut lines = vec![
        Line::from(""),
        Line::from(format!("streak     {} day{}", streak, if streak == 1 { "" } else { "s" })),
        Line::from(format!("week work  {}", format_hours_short(week_total))),
        Line::from(format!("avg / day  {}", format_hours_short(avg))),
    ];
    if let Some(best) = best
        && best.work > Duration::zero() {
            lines.push(Line::from(format!(
                "best day   {} ({})",
                best.date.format("%a"),
                format_hours_short(best.work)
            )));
        }
    lines.push(Line::from(format!(
        "today      {} work / {:.0}% ratio",
        format_hours_short(today_stat.work),
        today_stat.ratio() * 100.0
    )));

    f.render_widget(Paragraph::new(lines), cols[1]);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some(err) = &app.last_error {
        let widget = Paragraph::new(err.as_str())
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::TOP).title(" error "));
        f.render_widget(widget, area);
        return;
    }
    let help_text = if app.editor.is_some() {
        "Enter: log entry (stays open) | Alt+Enter: newline | Ctrl+S: log+close | Esc: close"
    } else {
        "SPC:Toggle | s:Stop | n:Journal | Tab:View | \u{2190}\u{2192}:Day | \u{2191}\u{2193}:Nav | Enter:Add-to-row | r:Resume | d:Del | m:Msg | q:Quit"
    };
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(help, area);
}

fn render_journal_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(editor) = app.editor.as_mut() else {
        return;
    };
    let session = &app.sessions[editor.target_index];

    let width = 72.min(area.width.saturating_sub(4));
    let height = 14.min(area.height.saturating_sub(4));
    let popup = centered_rect(area, width, height);
    f.render_widget(Clear, popup);

    let title = format!(
        " journal — {} @ {} ",
        session.session_type.label(),
        session.start_time_local().format("%H:%M")
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title);
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(5)])
        .split(inner);

    let previous: Vec<Line> = session
        .entries
        .iter()
        .rev()
        .take(4)
        .rev()
        .map(|e| {
            Line::styled(
                format!("{} ▸ {}", e.time_local().format("%H:%M"), e.text.replace('\n', " ")),
                Style::default().fg(Color::DarkGray),
            )
        })
        .collect();
    let previous = if previous.is_empty() {
        vec![Line::styled(
            "what are you doing right now?",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )]
    } else {
        previous
    };
    f.render_widget(Paragraph::new(previous).wrap(Wrap { trim: false }), parts[0]);

    editor.textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(" new entry (Enter: log · Alt+Enter: newline · Esc: close) "),
    );
    editor.textarea.set_cursor_line_style(Style::default());
    f.render_widget(&editor.textarea, parts[1]);
}

fn render_speech_bubble(f: &mut Frame, app: &App, area: Rect) {
    let Some(msg) = app.unread.front() else {
        return;
    };

    let width = 46.min(area.width.saturating_sub(6));
    let text_width = width.saturating_sub(2).max(1) as usize;
    let wrapped_lines = (msg.text.chars().count() / text_width + 2) as u16;
    let height = (wrapped_lines + 2).clamp(4, 9).min(area.height.saturating_sub(4));

    let bubble = Rect {
        x: 4,
        y: 1,
        width,
        height,
    };
    f.render_widget(Clear, bubble);

    let queued = app.unread.len() - 1;
    let bottom = if queued > 0 {
        format!(" m: next ({} more) ", queued)
    } else {
        " m: dismiss ".to_string()
    };
    let widget = Paragraph::new(msg.text.as_str())
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::Cyan))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(format!(" hermes 🔔 {} ", msg.time.with_timezone(&Local).format("%H:%M")))
                .title_bottom(bottom),
        );
    f.render_widget(widget, bubble);
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn format_duration_str(d: Duration) -> String {
    let total_seconds = d.num_seconds();
    let h = total_seconds / 3600;
    let m = (total_seconds % 3600) / 60;
    let s = total_seconds % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn format_hours_short(d: Duration) -> String {
    let mins = d.num_minutes();
    if mins >= 60 {
        format!("{}h{:02}m", mins / 60, mins % 60)
    } else {
        format!("{}m", mins)
    }
}
