
mod music;
mod app;
mod ui;

use std::{fs, io, path::PathBuf, process::Command};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers}, execute, terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode}
};
use ratatui::{backend::CrosstermBackend, Terminal};


#[tokio::main]
async fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = app::App::new(PathBuf::from("/home/timus/Music"));




    while app.running {
        app.poll_scan_results();
        terminal.draw(|f| ui::draw(f, &mut app))?;


        // block until an event arrives - no busy-wait, no wasted CPU
        // 

        if event::poll(std::time::Duration::from_millis(50))? {
            
        
        if let Event::Key(key)  = event::read()? {
            match(key.code, key.modifiers) {
                // Quit
                (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Esc, _) => {
                    app.running = false;
                }
            

            // Enter - play selected file(hook in audio backend here)
            (KeyCode::Enter, _) => {
                if let Some(file) = app.selected_file() {
                    let _ = file.path.to_string_lossy().to_string();
                    // TODO: send file.path to audio engine
                    
                }
            }

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
