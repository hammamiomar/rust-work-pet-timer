use crate::app::{App, DeleteTarget};
use crate::assets::*;
use crate::data::SessionType;
use crate::stats;
use chrono::{Duration, Local};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap},
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

    let chunks = Layout::vertical([
        Constraint::Length(15), // Pet animation
        Constraint::Length(8),  // Timer dashboard
        Constraint::Min(5),     // Work log; preserve the full stack around 32 rows
        Constraint::Length(3),  // Two-line shortcut footer
    ])
    .split(area);

    render_pet(f, app, chunks[0]);
    render_dashboard(f, app, chunks[1]);
    render_work_log(f, app, chunks[2]);
    render_footer(f, app, chunks[3]);

    // Agent messages remain the only overlay. Do not cover an active editor.
    if app.editor.is_none() && !app.unread.is_empty() {
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
        .map(|line| Line::styled(*line, Style::default().fg(status_color)))
        .collect();
    text.push(Line::styled(
        format!("{}  {}", app.mood.face(), app.mood_caption),
        Style::default()
            .fg(app.mood.color())
            .add_modifier(Modifier::ITALIC),
    ));

    let borders = if area.width < 62 {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };
    f.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(Block::default().borders(borders).title(" ur brain ")),
        area,
    );
}

fn render_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let active_session = app.active_session();
    let status_color = active_session.session_type.color();

    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1), // Status
        Constraint::Length(1), // Elapsed time
        Constraint::Length(1), // Ratio label
        Constraint::Length(1), // Ratio gauge
        Constraint::Length(1), // Streak
        Constraint::Length(1), // Agent unread count
    ])
    .split(inner);

    f.render_widget(
        Paragraph::new(active_session.session_type.label())
            .style(
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(format_duration_str(active_session.duration()))
            .style(
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[1],
    );

    let (work_duration, break_duration) = app.cached_day_stats;
    let work_seconds = work_duration.num_seconds() as f64;
    let total_seconds = work_seconds + break_duration.num_seconds() as f64;
    let ratio = if total_seconds > 0.0 {
        work_seconds / total_seconds
    } else {
        0.0
    };
    f.render_widget(
        Paragraph::new("Work Ratio:").alignment(Alignment::Center),
        rows[2],
    );
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Green).bg(Color::Red))
            .ratio(ratio)
            .label(format!("{:.0}% Work", ratio * 100.0))
            .use_unicode(true),
        rows[3],
    );

    let today = Local::now().date_naive();
    let streak = stats::streak(&app.sessions, today);
    f.render_widget(
        Paragraph::new(format!(
            "streak: {} day{}",
            streak,
            if streak == 1 { "" } else { "s" }
        ))
        .alignment(Alignment::Center),
        rows[4],
    );
    if !app.unread.is_empty() {
        f.render_widget(
            Paragraph::new(format!("hermes: {} unread — m to read", app.unread.len()))
                .style(Style::default().fg(Color::Cyan))
                .alignment(Alignment::Center),
            rows[5],
        );
    }
}

fn render_work_log(f: &mut Frame, app: &mut App, area: Rect) {
    let (total_work, total_break) = app.cached_day_stats;
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " Work Log: {} ",
            app.selected_date.format("%Y-%m-%d")
        ))
        .title_bottom(format!(
            " Daily Total | Work: {} | Break: {} ",
            format_duration_str(total_work),
            format_duration_str(total_break)
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let header_height = inner.height.min(1);
    let header_area = Rect {
        height: header_height,
        ..inner
    };
    render_log_header(f, header_area);
    let body = Rect {
        y: inner.y.saturating_add(header_height),
        height: inner.height.saturating_sub(header_height),
        ..inner
    };
    if body.height == 0 {
        return;
    }

    let indices = app.visible_session_indices();
    if indices.is_empty() {
        f.render_widget(
            Paragraph::new("no timer blocks this day")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center),
            body,
        );
        return;
    }

    let group_heights: Vec<u16> = indices
        .iter()
        .map(|&session_index| log_group_height(app, session_index, body))
        .collect();
    let selected_group = app
        .table_state
        .selected()
        .unwrap_or(0)
        .min(indices.len().saturating_sub(1));
    let start_group = viewport_start(&group_heights, selected_group, body.height);

    let mut y = body.y;
    let bottom = body.bottom();
    for (table_index, &session_index) in indices.iter().enumerate().skip(start_group) {
        if y >= bottom {
            break;
        }
        let height = group_heights[table_index].min(bottom.saturating_sub(y));
        if height == 0 {
            break;
        }
        let group_area = Rect {
            x: body.x,
            y,
            width: body.width,
            height,
        };
        render_log_group(f, app, table_index, session_index, group_area);
        y = y.saturating_add(height);
    }
}

fn render_log_header(f: &mut Frame, area: Rect) {
    let style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let columns = log_columns(area);
    for (column, label) in columns
        .iter()
        .zip(["Start", "End", "Type", "Time", "Journal"])
    {
        f.render_widget(Paragraph::new(label).style(style), *column);
    }
}

fn log_columns(area: Rect) -> Vec<Rect> {
    Layout::horizontal([
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Length(10),
        Constraint::Length(9),
        Constraint::Min(8),
    ])
    .spacing(1)
    .split(area)
    .to_vec()
}

fn log_group_height(app: &App, session_index: usize, body: Rect) -> u16 {
    if app.expanded_session_index != Some(session_index) || body.height <= 1 {
        return 1;
    }
    let child_heights = journal_child_heights(app, session_index, body);
    let desired: u16 = child_heights.iter().copied().fold(0, u16::saturating_add);
    1 + desired.min(body.height.saturating_sub(1))
}

fn viewport_start(heights: &[u16], selected: usize, available: u16) -> usize {
    if heights.is_empty() {
        return 0;
    }
    let selected = selected.min(heights.len() - 1);
    let mut start = selected;
    let mut used = heights[selected].min(available);
    while start > 0 {
        let previous = heights[start - 1];
        if used.saturating_add(previous) > available {
            break;
        }
        start -= 1;
        used = used.saturating_add(previous);
    }
    start
}

fn render_log_group(
    f: &mut Frame,
    app: &mut App,
    table_index: usize,
    session_index: usize,
    area: Rect,
) {
    let parent_area = Rect { height: 1, ..area };
    render_session_row(f, app, table_index, session_index, parent_area);

    if app.expanded_session_index == Some(session_index) && area.height > 1 {
        let children = Rect {
            y: area.y.saturating_add(1),
            height: area.height.saturating_sub(1),
            ..area
        };
        render_journal_children(f, app, session_index, children);
    }
}

fn render_session_row(
    f: &mut Frame,
    app: &App,
    table_index: usize,
    session_index: usize,
    area: Rect,
) {
    let session = &app.sessions[session_index];
    let child_has_focus =
        app.expanded_session_index == Some(session_index) && app.journal_selection.is_some();
    let selected = app.table_state.selected() == Some(table_index) && !child_has_focus;
    let pending = app.pending_delete == Some(DeleteTarget::Session(session_index));
    let mut row_style = Style::default();
    if selected {
        row_style = row_style.add_modifier(Modifier::REVERSED);
    }
    if pending {
        row_style = row_style
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    }
    f.render_widget(Block::default().style(row_style), area);

    let end = session.end_time_local().map_or_else(
        || "Active".to_string(),
        |time| time.format("%H:%M:%S").to_string(),
    );
    let expanded = app.expanded_session_index == Some(session_index);
    let arrow = if expanded { "▾" } else { "▸" };
    let journal = match session.entries.len() {
        0 => format!("{arrow} (no notes)"),
        1 => format!("{arrow} {}", session.entries[0].text.replace('\n', " ")),
        count => format!(
            "{arrow} {} (+{})",
            session.entries.last().unwrap().text.replace('\n', " "),
            count - 1
        ),
    };
    let columns = log_columns(area);
    let values = [
        session.start_time_local().format("%H:%M").to_string(),
        end,
        session.session_type.label().to_string(),
        format_duration_str(session.duration()),
        journal,
    ];
    for (index, (column, value)) in columns.iter().zip(values).enumerate() {
        let style = if index == 2 {
            row_style.fg(session.session_type.color())
        } else {
            row_style
        };
        f.render_widget(Paragraph::new(value).style(style), *column);
    }
}

fn journal_child_heights(app: &App, session_index: usize, area: Rect) -> Vec<u16> {
    let session = &app.sessions[session_index];
    let mut heights: Vec<u16> = session
        .entries
        .iter()
        .enumerate()
        .map(|(entry_index, entry)| {
            if editor_matches(app, session_index, Some(entry_index)) {
                inline_editor_height(app, area.width)
            } else {
                wrapped_text_height(&journal_entry_text(entry), area.width)
            }
        })
        .collect();
    heights.push(if editor_matches(app, session_index, None) {
        inline_editor_height(app, area.width)
    } else {
        1
    });
    heights
        .into_iter()
        .map(|height| height.max(1).min(area.height.max(1)))
        .collect()
}

fn inline_editor_height(app: &App, width: u16) -> u16 {
    let Some(editor) = app.editor.as_ref() else {
        return 1;
    };
    let input_width = width.saturating_sub(15).max(1);
    wrapped_text_height(&editor.textarea.lines().join("\n"), input_width)
}

fn editor_matches(app: &App, session_index: usize, entry_index: Option<usize>) -> bool {
    app.editor.as_ref().is_some_and(|editor| {
        editor.target_index == session_index && editor.entry_index == entry_index
    })
}

fn render_journal_children(f: &mut Frame, app: &mut App, session_index: usize, area: Rect) {
    let heights = journal_child_heights(app, session_index, area);
    let add_index = app.sessions[session_index].entries.len();
    let selected = app.journal_selection.unwrap_or(add_index).min(add_index);
    let start = viewport_start(&heights, selected, area.height);
    let mut y = area.y;
    let bottom = area.bottom();

    for (child_index, &child_height) in heights.iter().enumerate().skip(start) {
        if y >= bottom {
            break;
        }
        let height = child_height.min(bottom.saturating_sub(y));
        let child_area = Rect {
            x: area.x,
            y,
            width: area.width,
            height,
        };
        if child_index < add_index {
            render_journal_entry(f, app, session_index, child_index, child_area);
        } else {
            render_add_note(f, app, session_index, child_area);
        }
        y = y.saturating_add(height);
    }
}

fn render_journal_entry(
    f: &mut Frame,
    app: &mut App,
    session_index: usize,
    entry_index: usize,
    area: Rect,
) {
    if editor_matches(app, session_index, Some(entry_index)) {
        let time = app.sessions[session_index].entries[entry_index]
            .time_local()
            .format("%H:%M")
            .to_string();
        render_inline_editor(f, app, format!("   • {time}  "), area);
        return;
    }

    let selected = app.journal_selection == Some(entry_index);
    let pending = app.pending_delete
        == Some(DeleteTarget::JournalEntry {
            session_index,
            entry_index,
        });
    let mut style = Style::default();
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    if pending {
        style = style
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED);
    }
    f.render_widget(Block::default().style(style), area);
    let lines = journal_entry_lines(&app.sessions[session_index].entries[entry_index]);
    f.render_widget(
        Paragraph::new(lines)
            .style(style)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn journal_entry_lines(entry: &crate::data::JournalEntry) -> Vec<Line<'static>> {
    journal_entry_text(entry)
        .split('\n')
        .map(|line| Line::from(line.to_string()))
        .collect()
}

fn journal_entry_text(entry: &crate::data::JournalEntry) -> String {
    let first_prefix = format!("   • {}  ", entry.time_local().format("%H:%M"));
    let continuation = "             ";
    entry
        .text
        .split('\n')
        .enumerate()
        .map(|(index, text)| {
            if index == 0 {
                format!("{first_prefix}{text}")
            } else {
                format!("{continuation}{text}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrapped_text_height(text: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    text.split('\n')
        .map(|line| line.chars().count().max(1).div_ceil(width))
        .sum::<usize>()
        .max(1)
        .min(u16::MAX as usize) as u16
}

fn render_add_note(f: &mut Frame, app: &mut App, session_index: usize, area: Rect) {
    if editor_matches(app, session_index, None) {
        render_inline_editor(f, app, "   + new note  ".to_string(), area);
        return;
    }
    let selected = app.journal_selection
        == app
            .sessions
            .get(session_index)
            .map(|session| session.entries.len());
    let style = if selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(Block::default().style(style), area);
    f.render_widget(Paragraph::new("   + add note").style(style), area);
}

fn render_inline_editor(f: &mut Frame, app: &mut App, prefix: String, area: Rect) {
    let prefix_width = 15.min(area.width);
    let parts =
        Layout::horizontal([Constraint::Length(prefix_width), Constraint::Min(1)]).split(area);
    f.render_widget(
        Paragraph::new(prefix).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        parts[0],
    );
    let Some(editor) = app.editor.as_mut() else {
        return;
    };
    editor.textarea.set_block(Block::default());
    editor.textarea.set_style(Style::default().fg(Color::Cyan));
    editor.textarea.set_cursor_line_style(Style::default());
    editor
        .textarea
        .set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(&editor.textarea, parts[1]);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::TOP);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (lines, style) = if let Some(target) = app.pending_delete {
        let item = match target {
            DeleteTarget::Session(index) => app
                .sessions
                .get(index)
                .map(|session| {
                    format!(
                        "{} block at {}",
                        session.session_type.label(),
                        session.start_time_local().format("%H:%M")
                    )
                })
                .unwrap_or_else(|| "timer block".to_string()),
            DeleteTarget::JournalEntry {
                session_index,
                entry_index,
            } => app
                .sessions
                .get(session_index)
                .and_then(|session| session.entries.get(entry_index))
                .map(|entry| format!("note at {}", entry.time_local().format("%H:%M")))
                .unwrap_or_else(|| "note".to_string()),
        };
        (
            vec![
                Line::from(format!("Delete {item}? Press d again to confirm.")),
                Line::from("Esc, navigation, or any other command cancels."),
            ],
            Style::default().fg(Color::Red),
        )
    } else if let Some(error) = &app.last_error {
        (
            vec![Line::from(error.as_str()), Line::from("Esc:back  q:quit")],
            Style::default().fg(Color::Red),
        )
    } else if let Some(editor) = &app.editor {
        let first = if editor.entry_index.is_some() {
            "Enter:save  Alt+Enter:newline  Ctrl+S:save/close"
        } else {
            "Enter:add next  Alt+Enter:newline  Ctrl+S:save/close"
        };
        (
            vec![
                Line::from(first),
                Line::from("Esc:cancel draft  Arrows:edit"),
            ],
            Style::default().fg(Color::DarkGray),
        )
    } else {
        (
            vec![
                Line::from("SPC:mode  s:stop  n:note  Enter:open/edit  Esc:back"),
                Line::from("↑↓:nav  ←→:day  r:resume  d d:delete  m:msg  q:quit"),
            ],
            Style::default().fg(Color::DarkGray),
        )
    };
    f.render_widget(
        Paragraph::new(lines)
            .style(style)
            .alignment(Alignment::Center),
        inner,
    );
}

fn render_speech_bubble(f: &mut Frame, app: &App, area: Rect) {
    let Some(message) = app.unread.front() else {
        return;
    };

    let width = 46.min(area.width.saturating_sub(6));
    let text_width = width.saturating_sub(2).max(1) as usize;
    let wrapped_lines = (message.text.chars().count() / text_width + 2) as u16;
    let height = (wrapped_lines + 2)
        .clamp(4, 9)
        .min(area.height.saturating_sub(4));
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
    f.render_widget(
        Paragraph::new(message.text.as_str())
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::Cyan))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(format!(
                        " hermes 🔔 {} ",
                        message.time.with_timezone(&Local).format("%H:%M")
                    ))
                    .title_bottom(bottom),
            ),
        bubble,
    );
}

fn format_duration_str(duration: Duration) -> String {
    let total_seconds = duration.num_seconds();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}
