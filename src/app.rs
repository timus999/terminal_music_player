use crate::music::scan_music_dir_async;
use crate::music::MusicFile;
use crate::player::spawn_player;
use crate::player::PlayerCommand;
use crate::player::PlayerStatus;
use crate::youtube::spawn_youtube_runtime;
use crate::youtube::YoutubeCommand;
use crate::youtube::YoutubeResult;
use crate::youtube::YoutubeVideo;
use ratatui::widgets::ListState;
use std::path::PathBuf;

use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use tokio::sync::mpsc::Sender as TokioSender;

pub struct App {
    pub running: bool,

    // -- Search --
    pub search_query: String,

    // -- Music Library --
    pub music_files: Vec<MusicFile>,
    pub filtered_indices: Vec<usize>,

    pub is_scanning: bool, // shows a spinner / indicator in UI

    // -- Selection --
    pub list_state: ListState,

    // Channel receiver - None once scanning is complete
    scan_rx: Option<Receiver<MusicFile>>,

    // -- Player ------
    pub player_status: PlayerStatus, // last known status - drives the UI bar
    pub current_track_name: String,  // cached so Resume can re-display it
    pub is_paused: bool,
    player_cmd_tx: Sender<PlayerCommand>, // sends commands to audio thread
    player_status_rx: Receiver<PlayerStatus>, // receive status back

    // --- YouTube search -----------
    pub search_mode: SearchMode,
    pub youtube_query: String,
    // pub youtube_results: Vec<YoutubeVideo>,
    // pub youtube_list_state: ListState,
    // pub youtube_status: YoutubeStatus,
    // youtube_query_tx: Sender<String>,
    // youtube_result_rx: Receiver<YoutubeSearchResult>,

    // Youtube - tokio side
    pub youtube_results: Vec<YoutubeVideo>,
    pub youtube_list_state: ListState,
    pub youtube_status: YoutubeStatus,

    youtube_cmd_tx: TokioSender<YoutubeCommand>,
    youtube_result_rx: Receiver<YoutubeResult>,
}

// Tab key toggles between these two modes
pub enum SearchMode {
    Local,
    Youtube,
}

// What the Youtube panel is currently showing
pub enum YoutubeStatus {
    Idle,
    Searching,
    Done,
    Error(String),
}

impl App {
    pub fn new(music_dir: PathBuf, cookies_path: String) -> Self {
        // --- Scan channel ---
        let (scan_tx, scan_rx) = mpsc::channel();

        // kick off background scan immediately - does NOT block
        scan_music_dir_async(music_dir, scan_tx);

        // --- Player channel ---
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCommand>();
        let (status_tx, status_rx) = mpsc::channel::<PlayerStatus>();
        spawn_player(cmd_rx, status_tx, cookies_path.clone());

        // --- Youtube channels - tokio::sync::mpsc into runtime, std out
        let (yt_cmd_tx, yt_cmd_rx) = tokio::sync::mpsc::channel(32);
        let (yt_result_tx, yt_result_rx) = mpsc::channel();
        spawn_youtube_runtime(yt_cmd_rx, yt_result_tx);

        Self {
            running: true,
            search_query: String::new(),
            music_files: Vec::new(),
            filtered_indices: Vec::new(),
            list_state: ListState::default(),
            is_scanning: true,
            scan_rx: Some(scan_rx),

            player_status: PlayerStatus::Stopped,
            current_track_name: String::new(),
            is_paused: false,
            player_cmd_tx: cmd_tx,
            player_status_rx: status_rx,

            search_mode: SearchMode::Local,
            youtube_query: String::new(),
            youtube_results: Vec::new(),
            youtube_list_state: ListState::default(),
            youtube_status: YoutubeStatus::Idle,
            youtube_cmd_tx: yt_cmd_tx,
            youtube_result_rx: yt_result_rx,
        }
    }

    /// Switch between Local and Youtube mode with Tab
    pub fn toggle_search_mode(&mut self) {
        self.search_mode = match self.search_mode {
            SearchMode::Local => SearchMode::Youtube,
            SearchMode::Youtube => SearchMode::Local,
        };
    }

    // app.rs
    pub fn handle_enter(&mut self) {
        match self.search_mode {
            SearchMode::Local => self.play_selected(),
            SearchMode::Youtube => self.play_selected_youtube(),
        }
    }
    /// Called on Enter in Youtube results - two-step;
    /// 1. Ask rustypipe for the stream URL (async, non-blocking)
    /// 2. When URL arrives in poll_youtube_results, send PlayUrl to mpv
    pub fn play_selected_youtube(&mut self) {
        if let Some(video) = self.selected_youtube_video() {
            let y_video = video.clone();
            self.current_track_name = video.title.clone();

            // blocking send is fine here - the tokio channel has capacity 32
            // and this returns immediately unless the buffer is full
            let watch_url = format!("https://www.youtube.com/watch?v={}", y_video.video_id);
            let _ = self.player_cmd_tx.send(PlayerCommand::PlayUrl(watch_url));

            self.player_status = PlayerStatus::NowPlaying(Some(self.current_track_name.clone()));
        }
    }

    /// Drain pending Youtube results - call once per frame
    pub fn poll_youtube_results(&mut self) {
        while let Ok(result) = self.youtube_result_rx.try_recv() {
            match result {
                YoutubeResult::SearchResults(videos) => {
                    self.youtube_results = videos;
                    self.youtube_status = YoutubeStatus::Done;
                    // Auto-select first result
                    if !self.youtube_results.is_empty() {
                        self.youtube_list_state.select(Some(0));
                    }
                }

                // Stream URL arrived - hand it straight to mpv
                YoutubeResult::StreamUrl(url) => {
                    let _ = self.player_cmd_tx.send(PlayerCommand::PlayUrl(url));
                    // Update status bar immediately - mpv will start in - 100ms
                    self.player_status =
                        PlayerStatus::NowPlaying(Some(self.current_track_name.clone()));
                }
                YoutubeResult::Error(e) => {
                    self.player_status = PlayerStatus::Error(e.clone());
                    self.youtube_status = YoutubeStatus::Error(e);
                }
            }
        }
    }

    pub fn submit_youtube_search(&mut self) {
        if self.youtube_query.is_empty() {
            return;
        }
        self.youtube_status = YoutubeStatus::Searching;
        self.youtube_results.clear();
        let _ = self
            .youtube_cmd_tx
            .blocking_send(YoutubeCommand::Search(self.youtube_query.clone()));
    }
    pub fn youtube_scroll_down(&mut self) {
        if self.youtube_results.is_empty() {
            return;
        }
        let next = match self.youtube_list_state.selected() {
            Some(i) => (i + 1).min(self.youtube_results.len() - 1),
            None => 0,
        };
        self.youtube_list_state.select(Some(next));
    }

    pub fn youtube_scroll_up(&mut self) {
        let prev = match self.youtube_list_state.selected() {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.youtube_list_state.select(Some(prev));
    }

    pub fn selected_youtube_video(&self) -> Option<&YoutubeVideo> {
        let i = self.youtube_list_state.selected()?;
        self.youtube_results.get(i)
    }
    // Call this every frame in the event loop - drains all pending files
    // that arrived since the last tick without blocking.
    pub fn poll_scan_results(&mut self) {
        loop {
            let result = {
                let rx = match self.scan_rx.as_ref() {
                    Some(r) => r,
                    None => return,
                };
                rx.try_recv()
            };

            // try_recv is non-blocking - drains the queue then returns immediately
            match result {
                Ok(file) => {
                    self.music_files.push(file);
                    // Re-apply the current search to include the new file
                    self.refilter();
                }
                Err(mpsc::TryRecvError::Empty) => break, // nothing new yet, carry on
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Sender dropped - scan thread finished
                    self.is_scanning = false;
                    self.scan_rx = None;
                    break;
                }
            }
        }
    }

    fn refilter(&mut self) {
        let q = self.search_query.to_lowercase();
        self.filtered_indices = self
            .music_files
            .iter()
            .enumerate()
            .filter(|(_, f)| q.is_empty() || f.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();

        // keep selection valid - if nothing selected yet, select first
        if self.list_state.selected().is_none() && !self.filtered_indices.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    pub fn update_search(&mut self, query: &str) {
        self.search_query = query.to_string();
        self.refilter();

        self.list_state.select(if self.filtered_indices.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    pub fn selected_file(&self) -> Option<&MusicFile> {
        let idx = self.list_state.selected()?;
        let file_idx = self.filtered_indices.get(idx)?;
        self.music_files.get(*file_idx)
    }

    pub fn scroll_down(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let next = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.filtered_indices.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(next));
    }

    pub fn scroll_up(&mut self) {
        let prev = match self.list_state.selected() {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(prev));
    }

    /// Play the currently highlighted file. Called on Enter.
    pub fn play_selected(&mut self) {
        if let Some(file) = self.selected_file() {
            let path = file.path.clone();
            self.is_paused = false;
            // Silently ignore send errors - player thread may have exited
            let _ = self.player_cmd_tx.send(PlayerCommand::PlayFile(path));
        }
    }

    // Toggle pause/resume. Called on Space.
    pub fn toggle_pause(&mut self) {
        if self.is_paused {
            let _ = self.player_cmd_tx.send(PlayerCommand::Resume);

            // Re-emit NowPlaying so UI shows the track name again
            self.player_status = PlayerStatus::NowPlaying(Some(self.current_track_name.clone()));
            self.is_paused = false;
        } else {
            let _ = self.player_cmd_tx.send(PlayerCommand::Pause);
            self.is_paused = true;
        }
    }

    /// Stop playback. Called on 's'.
    pub fn stop(&mut self) {
        let _ = self.player_cmd_tx.send(PlayerCommand::Stop);
        self.is_paused = false;
    }

    /// Drain all pending player status update. Call once per frame.
    pub fn poll_player_status(&mut self) {
        while let Ok(status) = self.player_status_rx.try_recv() {
            // Cache track name so toggle_pause can re-display it after resume
            match status {
                PlayerStatus::NowPlaying(Some(ref name)) => {
                    self.current_track_name = name.clone();
                    self.is_paused = false;
                }
                PlayerStatus::NowPlaying(None) => {
                    // Restore cached name and skip the standard status update
                    self.player_status =
                        PlayerStatus::NowPlaying(Some(self.current_track_name.clone()));
                    self.is_paused = false;
                    continue;
                }

                PlayerStatus::FinishedNaturally => {
                    // only auto-advance in local mode - Youtube handles it's own flow
                    if matches!(self.search_mode, SearchMode::Local) {
                        self.scroll_down();
                        self.play_selected();
                    }
                    self.is_paused = false;
                }
                PlayerStatus::Error(_) => {
                    self.is_paused = false;
                }
                _ => {}
            }

            self.player_status = status;
        }
    }
}
