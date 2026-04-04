use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::{
    app::{App, DisplayStatus, InputMode, SearchMode, YoutubeStatus},
    player::PlayerStatus,
};

// Unicode block chars for the visualiser — 8 levels of fill
const BARS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn draw(f: &mut Frame, app: &mut App) {
    app.tick();
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
            Constraint::Length(5), // now playing bar + progress
            Constraint::Length(1), // key hints
        ])
        .split(f.area());

    draw_title(f, app, chunks[0]);
    draw_search(f, app, chunks[1]);

    match app.search_mode {
        SearchMode::Local => draw_local_results(f, app, chunks[2]),
        SearchMode::Youtube => draw_youtube_results(f, app, chunks[2]),
    }

    draw_now_playing(f, app, chunks[3]);
    draw_key_hints(f, app, chunks[4]);
}

// ------ Title ------------------------
fn draw_title(f: &mut Frame, app: &App, area: Rect) {
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

    f.render_widget(title, area);
}

fn draw_search(f: &mut Frame, app: &App, area: Rect) {
    // ---- Search ------ label changes by mode ------------
    // Show a blinking cursor effect by appending "" to the query.
    // the yellow border signals "this is the active input field".
    //
    let (query, label, border_color) = match app.search_mode {
        SearchMode::Local => (
            &app.search_query,
            " Search local  [i] to type, [Esc] to stop typing ",
            Color::Yellow,
        ),
        SearchMode::Youtube => (
            &app.youtube_query,
            " Search YouTube  [i] to type, [Enter] to search, [Esc] to stop ",
            Color::Red,
        ),
    };

    // Show cursor only when typing
    let display = if app.input_mode == InputMode::Typing {
        format!("{query}▋")
    } else {
        query.clone()
    };

    let border_style = if app.input_mode == InputMode::Typing {
        Style::default()
            .fg(border_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let search = Paragraph::new(format!("  {display}")).block(
        Block::default()
            .borders(Borders::ALL)
            .title(label)
            .border_style(border_style),
    );
    f.render_widget(search, area);
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

    if is_empty {
        f.render_widget(lists, area);
    } else {
        // render_stateful_widget mutates list_state (scroll offset), so it needs &mut
        f.render_stateful_widget(lists, area, &mut app.list_state);
    }
}

fn draw_youtube_results(f: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let title = match &app.youtube_status {
        YoutubeStatus::Idle => " YouTube - press [i] to type, [Enter] to search ".into(),
        YoutubeStatus::Searching => " YouTube - searching...".into(),
        YoutubeStatus::Done => format!(" Youtube - {} results ", app.youtube_results.len()),
        YoutubeStatus::Error(e) => format!(" YouTube - error: {e}"),
    };

    let items: Vec<ListItem> = if app.youtube_results.is_empty() {
        let msg = match &app.youtube_status {
            YoutubeStatus::Idle => "  Press [i] to type your search, then [Enter]",
            YoutubeStatus::Searching => "  Searching...",
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

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // visualizer animation
            Constraint::Length(1), // track name
            Constraint::Length(1), // progress bar
        ])
        .margin(1)
        .split(area);

    let (track_color, is_active) = match &app.display_status {
        DisplayStatus::Playing => (Color::Green, true),
        DisplayStatus::Paused => (Color::Yellow, false),
        DisplayStatus::Stopped | DisplayStatus::Finished => (Color::DarkGray, false),
        DisplayStatus::Error(_) => (Color::Red, false),
        DisplayStatus::Idle => (Color::DarkGray, false),
    };

    // Outer block
    let block_title = match &app.display_status {
        DisplayStatus::Playing => " Now playing ",
        DisplayStatus::Paused => " Paused ",
        DisplayStatus::Stopped => " Player ",
        DisplayStatus::Finished => " Finished ",
        DisplayStatus::Error(_) => " Error ",
        DisplayStatus::Idle => " Player ",
    };

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(block_title)
        .border_style(Style::default().fg(track_color));
    f.render_widget(outer, area);

    // Visualiser — animated bars when playing, flat when not

    // Visualiser
    let viz_width = inner[0].width as usize;
    f.render_widget(
        Paragraph::new(build_visualiser(app.animation_tick, is_active, viz_width)),
        inner[0],
    );

    // Track name + volume on the same line
    let vol_pct = (app.volume * 100.0) as u8;
    let vol_bar = build_volume_bar(vol_pct);
    let track_line = Line::from(vec![
        Span::styled(
            format!(" {}", app.current_track_name),
            Style::default()
                .fg(track_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("Vol: {vol_bar} {vol_pct}%"),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(track_line), inner[1]);
    draw_progress(f, app, inner[2], Color::DarkGray, is_active);

    // let (now_playing_text, bar_color) = match &app.player_status {
    //     PlayerStatus::NowPlaying(Some(name)) => (format!("  Now playing: {name}"), Color::Green),
    //     PlayerStatus::NowPlaying(None) => ("   Enter to play".into(), Color::DarkGray),
    //     PlayerStatus::Paused => (
    //         format!("  Paused: {}", app.current_track_name),
    //         Color::Yellow,
    //     ),
    //     PlayerStatus::Stopped => (
    //         "  Stopped  -  press Enter to play, Space to pause, s to stop".into(),
    //         Color::DarkGray,
    //     ),
    //     PlayerStatus::FinishedNaturally => (
    //         format!(
    //             "  Finished: {} - press Enter to play again",
    //             app.current_track_name
    //         ),
    //         Color::DarkGray,
    //     ),
    //     PlayerStatus::Error(msg) => (format!("  Error: {msg}"), Color::Red),
    // };

    // let now_playing = Paragraph::new(now_playing_text)
    //     .style(Style::default().fg(bar_color))
    //     .block(
    //         Block::default()
    //             .borders(Borders::ALL)
    //             .title(" Player ")
    //             .border_style(Style::default().fg(bar_color)),
    //     );
    // f.render_widget(now_playing, area);
}

fn draw_progress(f: &mut Frame, app: &App, area: Rect, track_color: Color, is_active: bool) {
    let elapsed = app.elapsed_secs();
    let bar_width = (area.width as usize).saturating_sub(14); // room for timestamps

    let (progress_bar, elapsed_str, total_str) = match app.current_duration {
        Some(total) if total.as_secs() > 0 => {
            // Known duration - show filled progress bar
            let ratio = (elapsed / total.as_secs_f64()).clamp(0.0, 1.0);
            let filled = (ratio * bar_width as f64) as usize;
            let empty = bar_width.saturating_sub(filled);
            let bar = format!("{}{}", "█".repeat(filled), "-".repeat(empty),);
            (
                bar,
                format_duration_secs(elapsed),
                format_duration_secs(total.as_secs_f64()),
            )
        }

        _ => {
            // Unknown duration - scrolling dot
            let pos = if bar_width > 0 {
                (elapsed as usize) % bar_width
            } else {
                0
            };

            let mut chars = vec!['-'; bar_width];
            if !chars.is_empty() && is_active {
                chars[pos] = '●';
            }
            let bar: String = chars.into_iter().collect();
            (bar, format_duration_secs(elapsed), "--:--".to_string())
        }
    };

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {elapsed_str} / {total_str} "),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(progress_bar, Style::default().fg(track_color)),
        ])),
        area,
    );
}

fn draw_key_hints(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.input_mode {
        InputMode::Typing => Line::from(vec![
            hint("[Esc]", " stop typing "),
            hint("[Enter]", " Confirm "),
            hint("[Backspace]", " delete "),
        ]),
        InputMode::Normal => Line::from(vec![
            hint("[i]", " type  "),
            hint("[Tab]", " switch mode  "),
            hint("[Enter]", " play  "),
            hint("[Space]", " pause  "),
            hint("[s]", " stop  "),
            hint("[↑↓]", " navigate  "),
            hint("[+/-]", " volume  "),
            hint("[q]", " quit  "),
        ]),
    };

    f.render_widget(Paragraph::new(hints), area);
}

fn hint<'a>(key: &'a str, label: &'a str) -> Span<'a> {
    Span::styled(
        format!("{key}{label}"),
        Style::default().fg(Color::DarkGray),
    )
}

fn build_visualiser(tick: u8, active: bool, width: usize) -> Line<'static> {
    if !active || width == 0 {
        return Line::from(Span::styled(
            "─".repeat(width),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let t = tick as f32;

    let bars: String = (0..width)
        .map(|i| {
            let x = i as f32;

            // Four sine waves at different frequencies and phases —
            // their sum produces a complex, varied waveform
            let wave = (x * 0.13 + t * 0.25).sin() * 2.5   // slow wide wave
              + (x * 0.31 + t * 0.40).sin() * 1.8   // mid frequency
              + (x * 0.57 + t * 0.60).sin() * 1.2   // faster ripple
              + (x * 0.89 + t * 0.15).sin() * 0.8; // high freq shimmer

            // Shift from [-6.3, 6.3] to [0, 8]
            let normalised = ((wave + 6.3) / 12.6 * 8.0) as usize;
            let idx = normalised.clamp(0, BARS.len() - 1);
            BARS[idx]
        })
        .collect();

    Line::from(Span::styled(bars, Style::default().fg(Color::Green)))
}
// ----- Volume bar -------------
fn build_volume_bar(pct: u8) -> String {
    let filled = (pct as usize * 10) / 100;
    let empty = 10 - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn format_duration_secs(secs: f64) -> String {
    let s = secs as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, ""),
    }
}
