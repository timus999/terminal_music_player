use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::{
    app::{App, SearchMode, YoutubeStatus},
    player::PlayerStatus,
};

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

    // --- Title with mode indicator ------------------------
    //
    let mode_label = match app.search_mode {
        SearchMode::Local => "[ Local ]  YouTube   Tab to switch",
        SearchMode::Youtube => "  Local  [ YouTube ]   Tab to switch",
    };
    // -- Title
    //
    let title = Paragraph::new(mode_label)
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().title(" Terminal Music Player"));

    f.render_widget(title, chunks[0]);

    // ---- Search ------ label changes by mode ------------
    // Show a blinking cursor effect by appending "" to the query.
    // the yellow border signals "this is the active input field".
    let (search_text, search_title, hint) = match app.search_mode {
        SearchMode::Local => (format!("{}_", app.search_query), " Search local ", ""),
        SearchMode::Youtube => (
            format!(" {}_", app.youtube_query),
            " Search YouTube ",
            "  press Enter to Search",
        ),
    };

    let search_display = if matches!(app.youtube_status, YoutubeStatus::Searching)
        && matches!(app.search_mode, SearchMode::Youtube)
    {
        "  Searching...".into()
    } else {
        format!("{search_text}{hint}")
    };
    let search = Paragraph::new(search_display).block(
        Block::default()
            .borders(Borders::ALL)
            .title(search_title)
            .border_style(Style::default().fg(match app.search_mode {
                SearchMode::Local => Color::Yellow,
                SearchMode::Youtube => Color::Red,
            })),
    );
    f.render_widget(search, chunks[1]);

    // -- Result Lists -- switches content based on mode ---------------
    //
    //
    match app.search_mode {
        SearchMode::Local => draw_local_results(f, app, chunks[2]),
        SearchMode::Youtube => draw_youtube_results(f, app, chunks[2]),
    }

    draw_now_playing(f, app, chunks[3]);
}

fn draw_local_results(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
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
    f.render_stateful_widget(lists, area, &mut app.list_state);
}

fn draw_youtube_results(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let title = match &app.youtube_status {
        YoutubeStatus::Idle => " YouTube - type a query and press Enter".into(),
        YoutubeStatus::Searching => " YouTube - searching...".into(),
        YoutubeStatus::Done => format!(" Youtube - {} results ", app.youtube_results.len()),
        YoutubeStatus::Error(e) => format!(" YouTube - error: {e}"),
    };

    let items: Vec<ListItem> = if app.youtube_results.is_empty() {
        let msg = match &app.youtube_status {
            YoutubeStatus::Idle => "  Type a query above and press Enter",
            YoutubeStatus::Searching => "  Fetching results...",
            YoutubeStatus::Done => "  No results found",
            YoutubeStatus::Error(_) => "  Search failed - check your connection",
        };
        vec![ListItem::new(msg).style(Style::default().fg(Color::DarkGray))]
    } else {
        app.youtube_results
            .iter()
            .map(|v| {
                // "Song Title                   ChannelName  3:45"
                // Title takes remaining space; channel + duration  right-aligned
                let right = format!("{}  {}", v.channel, v.duration_str());
                let line = Line::from(vec![
                    Span::styled(v.title.clone(), Style::default().fg(Color::White)),
                    Span::styled(" ", Style::default()),
                    Span::styled(right, Style::default().fg(Color::DarkGray)),
                ]);
                ListItem::new(line)
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Red)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Red)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ▶ ");

    if app.youtube_results.is_empty() {
        f.render_widget(list, area);
    } else {
        f.render_stateful_widget(list, area, &mut app.youtube_list_state);
    }
}

fn draw_now_playing(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    // -- Now Playing ---------------------

    let (now_playing_text, bar_color) = match &app.player_status {
        PlayerStatus::NowPlaying(Some(name)) => (format!("  Now playing: {name}"), Color::Green),
        PlayerStatus::NowPlaying(None) => ("   Enter to play".into(), Color::DarkGray),
        PlayerStatus::Paused => (
            format!("  Paused: {}", app.current_track_name),
            Color::Yellow,
        ),
        PlayerStatus::Stopped => (
            "  Stopped  -  press Enter to play, Space to pause, s to stop".into(),
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
    f.render_widget(now_playing, area);
}
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, ""),
    }
}
