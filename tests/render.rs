//! Headless render smoke tests: draw every view into a TestBackend buffer
//! so layout math panics are caught by `cargo test`.

use ratatui::{Terminal, backend::TestBackend};
use hamba_timer::app::App;

fn test_app() -> App {
    let dir = std::env::temp_dir().join(format!("pet-timer-render-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Safety: tests in this file run in one process; nothing else reads this var.
    unsafe { std::env::set_var("PET_TIMER_DATA_DIR", &dir) };
    App::new().unwrap()
}

fn draw(app: &mut App, width: u16, height: u16) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| hamba_timer::ui::ui(f, app)).unwrap();
}

#[test]
fn renders_all_views_and_overlays() {
    let mut app = test_app();

    for (w, h) in [(100, 32), (62, 20), (40, 10), (200, 60)] {
        draw(&mut app, w, h);
    }

    app.toggle_work_break();
    app.open_journal_for_current();
    draw(&mut app, 100, 32);
    if let Some(editor) = app.editor.as_mut() {
        editor.textarea.insert_str("testing the journal editor");
    }
    draw(&mut app, 100, 32);
    app.save_journal_entry();

    app.cycle_view(); // journal
    app.nav(true);
    draw(&mut app, 100, 32);
    app.cycle_view(); // stats
    draw(&mut app, 100, 32);
    app.cycle_view(); // back to history
    app.nav(true);
    draw(&mut app, 100, 32);

    app.unread.push_back(hamba_timer::inbox::InboxMessage {
        id: 1,
        time: chrono::Utc::now(),
        text: "you have been working for a while — take a break, drink some water, and pet a real animal".to_string(),
    });
    draw(&mut app, 100, 32);
    draw(&mut app, 62, 20);
    app.dismiss_message();

    app.on_tick();
    app.on_quit();
}
