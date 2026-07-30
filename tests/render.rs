//! Headless render tests for the stacked layout and expandable work log.

use hamba_timer::app::{App, DeleteTarget};
use ratatui::{Terminal, backend::TestBackend};
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn test_app(name: &str) -> App {
    let dir = std::env::temp_dir().join(format!("pet-timer-render-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    // Safety: tests in this file run in one process; nothing else reads this var.
    unsafe { std::env::set_var("PET_TIMER_DATA_DIR", &dir) };
    App::new().unwrap()
}

fn draw(app: &mut App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| hamba_timer::ui::ui(frame, app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn dashboard_is_stacked_and_narrow_footer_is_complete() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut app = test_app("stacked");
    let screen = draw(&mut app, 60, 40);
    let pet_y = screen
        .lines()
        .position(|line| line.contains("ur brain"))
        .unwrap();
    let timer_y = screen
        .lines()
        .position(|line| line.contains("IDLE"))
        .unwrap();
    let log_y = screen
        .lines()
        .position(|line| line.contains("Work Log:"))
        .unwrap();

    assert!(pet_y < timer_y, "dashboard should be below the pet");
    assert!(timer_y < log_y, "work log should be below the dashboard");
    assert!(screen.contains("d d:delete"));
    assert!(screen.contains("m:msg"));
    assert!(screen.contains("q:quit"));
}

#[test]
fn renders_inline_journal_and_all_edge_sizes() {
    let _guard = ENV_LOCK.lock().unwrap();
    let mut app = test_app("journal");

    for (width, height) in [(100, 40), (60, 20), (40, 10), (200, 60)] {
        let screen = draw(&mut app, width, height);
        if width < 60 || height < 20 {
            assert!(screen.contains("Terminal too small."));
        }
    }

    app.open_journal_for_current();
    app.editor
        .as_mut()
        .unwrap()
        .textarea
        .insert_str("testing the inline journal");
    let editing = draw(&mut app, 60, 40);
    assert!(editing.contains("+ new note"));
    assert!(editing.contains("testing the inline journal"));
    assert!(editing.contains("Enter:add next"));

    app.commit_journal_entry();
    let saved = draw(&mut app, 60, 40);
    assert!(saved.contains("testing the inline journal"));
    assert!(saved.contains("+ new note"));
    app.save_journal_entry(); // Empty draft: close editing.

    app.journal_selection = Some(0);
    app.activate_selected();
    app.editor.as_mut().unwrap().textarea.insert_str(" updated");
    app.commit_journal_entry();
    let edited = draw(&mut app, 60, 40);
    assert!(edited.contains("updated"));

    app.journal_selection = Some(0);
    app.delete_selected_entry();
    assert!(matches!(
        app.pending_delete,
        Some(DeleteTarget::JournalEntry { .. })
    ));
    let confirming = draw(&mut app, 60, 40);
    assert!(confirming.contains("Press d again to confirm"));
    app.delete_selected_entry();

    for index in 0..12 {
        app.sessions[app.current_session_index].add_entry(format!("scroll note {index}"));
    }
    app.journal_selection = Some(12);
    let journal_bottom = draw(&mut app, 60, 40);
    assert!(journal_bottom.contains("scroll note 11"));
    assert!(journal_bottom.contains("+ add note"));
    app.journal_selection = Some(0);
    let journal_top = draw(&mut app, 60, 40);
    assert!(journal_top.contains("scroll note 0"));

    app.unread.push_back(hamba_timer::inbox::InboxMessage {
        id: 1,
        time: chrono::Utc::now(),
        text: "you have been working for a while — take a break and drink some water".to_string(),
    });
    let message = draw(&mut app, 60, 40);
    assert!(message.contains("hermes"));
    app.dismiss_message();

    app.on_tick();
    app.on_quit();
}
