mod app;
mod image_fetcher;
mod music;
mod player;
mod ui;
mod youtube;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dirs;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, path::PathBuf};

use crate::{
    app::{InputMode, SearchMode},
    ui::UiState,
};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let cookies_path = std::env::var("YT_COOKIES").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|h| h.join("cookies.txt").to_string_lossy().to_string())
            .unwrap_or_else(|| "cookies.txt".to_string())
    });
    let mut app = app::App::new(PathBuf::from("/home/timus/Music"), cookies_path);
    let mut ui_state = UiState::new();

    while app.running {
        app.poll_scan_results();
        app.poll_player_status();
        app.poll_youtube_results();
        app.poll_image();
        terminal.draw(|f| ui::draw(f, &mut app, &mut ui_state))?;

        // block until an event arrives - no busy-wait, no wasted CPU
        //

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match app.input_mode {
                    // ── Typing mode — almost everything goes to the search bar ──
                    InputMode::Typing => match key.code {
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Enter => {
                            app.handle_enter();
                        }
                        KeyCode::Char(c) => match app.search_mode {
                            SearchMode::Local => {
                                let mut q = app.search_query.clone();
                                q.push(c);
                                app.update_search(&q);
                            }
                            SearchMode::Youtube => {
                                app.youtube_query.push(c);
                            }
                        },
                        KeyCode::Backspace => match app.search_mode {
                            SearchMode::Local => {
                                let mut q = app.search_query.clone();
                                q.pop();
                                app.update_search(&q);
                            }
                            SearchMode::Youtube => {
                                app.youtube_query.pop();
                            }
                        },
                        _ => {}
                    },

                    // ── Normal mode — keys trigger commands ────────────────────
                    InputMode::Normal => match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('i') => {
                            // Enter typing mode — like vim
                            app.input_mode = InputMode::Typing;
                        }
                        KeyCode::Tab => app.toggle_search_mode(),
                        KeyCode::Enter => app.handle_enter(),
                        KeyCode::Char(' ') => app.toggle_pause(),
                        KeyCode::Char('s') => app.stop(),
                        KeyCode::Down => match app.search_mode {
                            SearchMode::Local => app.scroll_down(),
                            SearchMode::Youtube => app.youtube_scroll_down(),
                        },
                        KeyCode::Up => match app.search_mode {
                            SearchMode::Local => app.scroll_up(),
                            SearchMode::Youtube => app.youtube_scroll_up(),
                        },

                        KeyCode::Char('a') => app.add_to_playlist(),
                        KeyCode::Char('d') => app.remove_from_playlist(),
                        KeyCode::Char('p') => app.play_selected_playlist(),
                        KeyCode::Char('+') => app.volume_up(),
                        KeyCode::Char('-') => app.volume_down(),
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal; // no-op, already normal
                        }
                        // Ctrl+Down/Up navigate playlist
                        KeyCode::Char('j') => app.playlist_scroll_down(),

                        KeyCode::Char('k') => app.playlist_scroll_up(),

                        _ => {}
                    },
                }

                // match (key.code, key.modifiers) {

                //         // Quit
                //         (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => {
                //             app.running = false;
                //         }

                //         // Tab always switches mode regardless of what's focused
                //         (KeyCode::Tab, _) => app.toggle_search_mode(),
                //         (KeyCode::Char('s'), KeyModifiers::CONTROL) => app.submit_youtube_search(),

                //         // Enter - play selected file(hook in audio backend here)
                //         (KeyCode::Enter, _) => app.handle_enter(),

                //         // Pause / resume
                //         (KeyCode::Char(' '), _) => app.toggle_pause(),

                //         // stop
                //         (KeyCode::Char('s'), _) => app.stop(),

                //         // Navigation
                //         (KeyCode::Down, _) => match app.search_mode {
                //             app::SearchMode::Local => app.scroll_down(),
                //             app::SearchMode::Youtube => app.youtube_scroll_down(),
                //         },
                //         (KeyCode::Up, _) => match app.search_mode {
                //             app::SearchMode::Local => app.scroll_up(),
                //             app::SearchMode::Youtube => app.youtube_scroll_up(),
                //         },

                //         // Typing - update query and refilter
                //         (KeyCode::Char(c), _) => match app.search_mode {
                //             app::SearchMode::Local => {
                //                 let mut q = app.search_query.clone();
                //                 q.push(c);
                //                 app.update_search(&q);
                //             }
                //             app::SearchMode::Youtube => app.youtube_query.push(c),
                //         },

                //         // Backspace
                //         (KeyCode::Backspace, _) => match app.search_mode {
                //             app::SearchMode::Local => {
                //                 let mut q = app.search_query.clone();
                //                 q.pop();
                //                 app.update_search(&q);
                //             }
                //             app::SearchMode::Youtube => {
                //                 app.youtube_query.pop();
                //             }
                //         },

                //         _ => {}
                //     }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
