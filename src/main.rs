mod app;
mod music;
mod player;
mod ui;
mod youtube;

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

    let api_key = std::env::var("YT_API_KEY").unwrap_or_else(|_| "".into());
    let mut app = app::App::new(PathBuf::from("/home/timus/Music"), api_key);

    while app.running {
        app.poll_scan_results();
        app.poll_player_status();
        app.poll_youtube_results();
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

                    // Tab always switches mode regardless of what's focused
                    (KeyCode::Tab, _) => app.toggle_search_mode(),

                    // Enter - play selected file(hook in audio backend here)
                    (KeyCode::Enter, _) => match app.search_mode {
                        app::SearchMode::Local => app.play_selected(),
                        app::SearchMode::Youtube => app.submit_youtube_search(),
                    },

                    // Pause / resume
                    (KeyCode::Char(' '), _) => app.toggle_pause(),

                    // stop
                    (KeyCode::Char('s'), _) => app.stop(),

                    // Navigation
                    (KeyCode::Down, _) => match app.search_mode {
                        app::SearchMode::Local => app.scroll_down(),
                        app::SearchMode::Youtube => app.youtube_scroll_down(),
                    },
                    (KeyCode::Up, _) => match app.search_mode {
                        app::SearchMode::Local => app.scroll_up(),
                        app::SearchMode::Youtube => app.youtube_scroll_up(),
                    },

                    // Typing - update query and refilter
                    (KeyCode::Char(c), _) => match app.search_mode {
                        app::SearchMode::Local => {
                            let mut q = app.search_query.clone();
                            q.push(c);
                            app.update_search(&q);
                        }
                        app::SearchMode::Youtube => app.youtube_query.push(c),
                    },

                    // Backspace
                    (KeyCode::Backspace, _) => match app.search_mode {
                        app::SearchMode::Local => {
                            let mut q = app.search_query.clone();
                            q.pop();
                            app.update_search(&q);
                        }
                        app::SearchMode::Youtube => {
                            app.youtube_query.pop();
                        }
                    },

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
