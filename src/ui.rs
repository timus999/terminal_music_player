use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::{app::App, player::PlayerStatus};

pub fn draw(f: &mut Frame, app: &mut App) {
    // -- Layout -----------------------------
    // Three vertical sections:
    // [0] Title bar         -- fixed height, branding only
    // [1] Search input      -- fixed height, one line of text
    // [2] Results list      -- fills remaining space
    // [3] Now Playing       -- now playing bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // title: exactly 3 rows (border + 1 line + border)
            Constraint::Length(3), // search input : same
            Constraint::Min(0),    // results : everything else - scales with terminal size
            Constraint::Length(3), // now playing bar
        ])
        .split(f.area());

    // -- Title
    //
    let title = Paragraph::new(" Terminal Music Player")
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default());

    f.render_widget(title, chunks[0]);

    // ---- Search ------
    // Show a blinking cursor effect by appending "" to the query.
    // the yellow border signals "this is the active input field".
    let search_text = format!(" {}_", app.search_query); // underscore = simple cursor

    let search = Paragraph::new(search_text)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Search (type to filter) ")
                .border_style(Style::default().fg(Color::Yellow)),
        );
    f.render_widget(search, chunks[1]);

    // -- Result Lists --
    //
    //
    let is_empty = app.filtered_indices.is_empty();
    let items: Vec<ListItem> = if is_empty {
        // Render a single greyed-out hint row instead of nothing
        vec![ListItem::new(if app.music_files.is_empty() {
            "  No music files found. Check your music directory."
        } else {
            "  No results match your search."
        })
        .style(Style::default().fg(Color::DarkGray))]
    } else {
        app.filtered_indices
            .iter()
            .enumerate()
            .map(|(pos, &file_idx)| {
                let file = &app.music_files[file_idx];

                // Dim the extension so the song name pops visually
                let (stem, ext) = split_extension(&file.name);
                let line = Line::from(vec![
                    Span::styled(stem, Style::default().fg(Color::White)),
                    Span::styled(format!(".{ext}"), Style::default().fg(Color::DarkGray)),
                ]);

                // Top result gets a subtle marker to indicate it's the default pick
                if pos == 0 {
                    ListItem::new(line).style(Style::default())
                } else {
                    ListItem::new(line)
                }
            })
            .collect()
    };

    let results_title = if app.is_scanning {
        format!(" Results ({} / scanning...) ", app.filtered_indices.len())
    } else {
        format!(
            " Results ({} / {}) ",
            app.filtered_indices.len(),
            app.music_files.len()
        )
    };

    let lists = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(results_title))
        // Highlight row style - green background, black text, bold
        .highlight_style(
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▶ "); // clear visual indicator of selected row

    // render_stateful_widget mutates list_state (scroll offset), so it needs &mut
    f.render_stateful_widget(lists, chunks[2], &mut app.list_state);

    // -- Now Playing ---------------------

    let (now_playing_text, bar_color) = match &app.player_status {
        PlayerStatus::NowPlaying(name) => (format!("  Now playing: {name}"), Color::Green),
        PlayerStatus::Paused => (
            format!("  Paused: {}", app.current_track_name),
            Color::Yellow,
        ),
        PlayerStatus::Stopped => (
            "  Stopped  -  press Enter to play, Space to pause, s to stop".to_string(),
            Color::DarkGray,
        ),
        PlayerStatus::FinishedNaturally => (
            format!(
                "  Finished: {} - press Enter to play again",
                app.current_track_name
            ),
            Color::DarkGray,
        ),
        PlayerStatus::Error(msg) => (format!("  Error: {msg}"), Color::Red),
    };

    let now_playing = Paragraph::new(now_playing_text)
        .style(Style::default().fg(bar_color))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Player ")
                .border_style(Style::default().fg(bar_color)),
        );
    f.render_widget(now_playing, chunks[3]);
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, ""),
    }
}
