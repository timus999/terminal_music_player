mod app;
mod music;
mod player;
mod ui;

use std::{io, path::PathBuf};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = app::App::new(PathBuf::from("/home/timus/Music"));

    while app.running {
        app.poll_scan_results();
        app.poll_player_status();
        terminal.draw(|f| ui::draw(f, &mut app))?;

        // block until an event arrives - no busy-wait, no wasted CPU
        //

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    // Quit
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => {
                        app.running = false;
                    }

                    // Enter - play selected file(hook in audio backend here)
                    (KeyCode::Enter, _) => app.play_selected(),

                    // Pause / resume
                    (KeyCode::Char(' '), _) => app.toggle_pause(),

                    // stop
                    (KeyCode::Char('s'), _) => app.stop(),

                    // Navigation
                    (KeyCode::Down, _) => app.scroll_down(),
                    (KeyCode::Up, _) => app.scroll_up(),

                    // Typing - update query and refilter
                    (KeyCode::Char(c), _) => {
                        let mut q = app.search_query.clone();
                        q.push(c);
                        app.update_search(&q);
                    }

                    // Backspace
                    (KeyCode::Backspace, _) => {
                        let mut q = app.search_query.clone();
                        q.pop();
                        app.update_search(&q);
                    }

                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
