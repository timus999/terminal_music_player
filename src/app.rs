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

use crate::image_fetcher::{spawn_image_fetcher, ImageCommand, ImageResult};

use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::time::Duration;
use tokio::sync::mpsc::Sender as TokioSender;

#[derive(PartialEq)]
pub enum InputMode {
    Normal, // Keys trigger commands (navigate, pause, stop),
    Typing, // all chars go the active search bar
}

pub enum DisplayStatus {
    Idle,
    Playing,
    Paused,
    Stopped,
    Finished,
    Error(String),
}

#[derive(Clone)]
pub struct PlaylistEntry {
    pub name: String,
    pub path_or_url: String,
    pub is_youtube: bool,
    pub duration_secs: u64,
}

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
    pub is_youtube_playing: bool, // tracks whether current track is a Youtube stream

    pub input_mode: InputMode,
    pub volume: f32,        // 0.0 to 1.0
    pub animation_tick: u8, // increments each frame for the visualizer

    pub current_duration: Option<Duration>, // seconds, from mpv or symphonia
    pub current_position: f64,              // seconds, from mpv or estimated

    pub display_status: DisplayStatus,

    // Playlist
    pub playlist: Vec<PlaylistEntry>,
    pub playlist_state: ListState,

    // Image
    pub current_image: Option<image::DynamicImage>,
    pub current_image_id: usize,
    image_cmd_tx: std::sync::mpsc::Sender<ImageCommand>,
    image_result_rx: std::sync::mpsc::Receiver<ImageResult>,
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

        // --- Images ----
        let (img_cmd_tx, img_cmd_rx) = std::sync::mpsc::channel();
        let (img_result_tx, img_result_rx) = std::sync::mpsc::channel();
        spawn_image_fetcher(img_cmd_rx, img_result_tx);

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
            is_youtube_playing: false,

            input_mode: InputMode::Normal,
            volume: 1.0,
            animation_tick: 0,
            current_duration: None,
            current_position: 0.0,

            display_status: DisplayStatus::Idle,

            // playlist
            playlist: Vec::new(),
            playlist_state: ListState::default(),

            // Image
            current_image: None,
            current_image_id: 0,
            image_cmd_tx: img_cmd_tx,
            image_result_rx: img_result_rx,
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
            SearchMode::Local => {
                self.input_mode = InputMode::Normal;
                self.play_selected();
            }
            SearchMode::Youtube => {
                if self.input_mode == InputMode::Typing {
                    self.input_mode = InputMode::Normal;
                    self.submit_youtube_search();
                } else {
                    self.play_selected_youtube();
                }
            }
        }
    }
    /// Called on Enter in Youtube results - two-step;
    /// 1. Ask rustypipe for the stream URL (async, non-blocking)
    /// 2. When URL arrives in poll_youtube_results, send PlayUrl to mpv
    pub fn play_selected_youtube(&mut self) {
        if let Some(video) = self.selected_youtube_video() {
            let y_video = video.clone();
            self.current_track_name = video.title.clone();
            self.current_position = 0.0;
            self.current_duration = if y_video.duration_secs > 0 {
                Some(std::time::Duration::from_secs(y_video.duration_secs))
            } else {
                None
            };

            // blocking send is fine here - the tokio channel has capacity 32
            // and this returns immediately unless the buffer is full
            let watch_url = format!("https://www.youtube.com/watch?v={}", y_video.video_id);

            // Reset timer here - same logic as poll_player_status does for local_files
            self.is_paused = false;
            self.display_status = DisplayStatus::Playing;

            self.is_youtube_playing = true;

            // Fetch Youtube thumbnail
            let _ = self
                .image_cmd_tx
                .send(ImageCommand::FetchYoutube(y_video.video_id.clone()));

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
            self.current_duration = read_audio_duration(&path);
            self.current_position = 0.0;
            self.is_paused = false;
            self.is_youtube_playing = false;
            // Fetch cover art for the local file
            let _ = self
                .image_cmd_tx
                .send(ImageCommand::FetchLocal(path.clone()));
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
                    self.current_position = 0.0;
                    self.is_paused = false;

                    self.display_status = DisplayStatus::Playing;
                }
                PlayerStatus::NowPlaying(None) => {
                    // Restore cached name and skip the standard status update
                    self.player_status =
                        PlayerStatus::NowPlaying(Some(self.current_track_name.clone()));
                    self.display_status = DisplayStatus::Playing;
                    // Resume - start a new segment, keep accumulated
                    self.is_paused = false;
                    continue;
                }
                PlayerStatus::Position(pos) => {
                    self.current_position = pos;
                }
                PlayerStatus::Paused => {
                    self.is_paused = true;
                    self.display_status = DisplayStatus::Paused;
                }

                PlayerStatus::FinishedNaturally => {
                    self.is_paused = false;
                    // only auto-advance in local mode - Youtube handles it's own flow
                    self.current_position = 0.0;
                    self.current_duration = None;
                    self.is_youtube_playing = false;
                    self.display_status = DisplayStatus::Finished;
                    if matches!(self.search_mode, SearchMode::Local) {
                        self.scroll_down();
                        self.play_selected();
                    }
                    if matches!(self.search_mode, SearchMode::Youtube) {
                        self.youtube_scroll_down();
                        self.play_selected_youtube();
                    }
                }
                PlayerStatus::Error(_) | PlayerStatus::Stopped => {
                    self.is_paused = false;
                    self.current_duration = None;
                    self.current_position = 0.0;

                    self.display_status = DisplayStatus::Stopped;
                    self.is_youtube_playing = false;
                }
            }

            self.player_status = status;
        }
    }

    pub fn tick(&mut self) {
        // Called every frame - advances animation
        self.animation_tick = self.animation_tick.wrapping_add(1);
    }

    pub fn volume_up(&mut self) {
        self.volume = (self.volume + 0.05).min(1.0);
        let _ = self
            .player_cmd_tx
            .send(PlayerCommand::SetVolume(self.volume));
    }

    pub fn volume_down(&mut self) {
        self.volume = (self.volume - 0.05).max(0.0);
        let _ = self
            .player_cmd_tx
            .send(PlayerCommand::SetVolume(self.volume));
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.current_position
    }

    pub fn poll_image(&mut self) {
        while let Ok(result) = self.image_result_rx.try_recv() {
            match result {
                ImageResult::Loaded(img) => {
                    self.current_image = Some(img);
                    self.current_image_id += 1; // signal ui to rebuild protocol
                }
                ImageResult::NotFound | ImageResult::Error(_) => {
                    self.current_image = None;
                    self.current_image_id += 1;
                }
            }
        }
    }

    pub fn add_to_playlist(&mut self) {
        match self.search_mode {
            SearchMode::Local => {
                if let Some(file) = self.selected_file() {
                    let entry = PlaylistEntry {
                        name: file.name.clone(),
                        path_or_url: file.path.to_string_lossy().to_string(),
                        is_youtube: false,
                        duration_secs: 0,
                    };
                    if !self
                        .playlist
                        .iter()
                        .any(|e| e.path_or_url == entry.path_or_url)
                    {
                        self.playlist.push(entry);
                    }
                }
            }

            SearchMode::Youtube => {
                if let Some(video) = self.selected_youtube_video() {
                    let entry = PlaylistEntry {
                        name: video.title.clone(),
                        path_or_url: format!("https://www.youtube.com/watch?v={}", video.video_id),
                        is_youtube: true,
                        duration_secs: video.duration_secs,
                    };

                    if !self
                        .playlist
                        .iter()
                        .any(|e| e.path_or_url == entry.path_or_url)
                    {
                        self.playlist.push(entry);
                    }
                }
            }
        }
        // Select last added
        if !self.playlist.is_empty() {
            self.playlist_state.select(Some(self.playlist.len() - 1));
        }
    }

    pub fn remove_from_playlist(&mut self) {
        if let Some(i) = self.playlist_state.selected() {
            self.playlist.remove(i);
            let new_sel = i.saturating_sub(1);
            self.playlist_state.select(if self.playlist.is_empty() {
                None
            } else {
                Some(new_sel)
            });
        }
    }

    pub fn play_selected_playlist(&mut self) {
        if let Some(i) = self.playlist_state.selected() {
            if let Some(entry) = self.playlist.get(i) {
                self.current_track_name = entry.name.clone();
                self.current_position = 0.0;
                self.current_duration = if entry.duration_secs > 0 {
                    Some(Duration::from_secs(entry.duration_secs))
                } else {
                    None
                };

                if entry.is_youtube {
                    self.is_youtube_playing = true;
                    let url = entry.path_or_url.clone();
                    // Extract video_id for thumbnail
                    let video_id = url.split("v=").nth(1).unwrap_or("").to_string();
                    let _ = self.image_cmd_tx.send(ImageCommand::FetchYoutube(video_id));
                    let _ = self.player_cmd_tx.send(PlayerCommand::PlayUrl(url));
                } else {
                    self.is_youtube_playing = false;
                    let path = PathBuf::from(&entry.path_or_url);
                    let _ = self
                        .image_cmd_tx
                        .send(ImageCommand::FetchLocal(path.clone()));
                    let _ = self.player_cmd_tx.send(PlayerCommand::PlayFile(path));
                }
                self.display_status = DisplayStatus::Playing;
            }
        }
    }

    pub fn playlist_scroll_down(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        let next = match self.playlist_state.selected() {
            Some(i) => (i + 1).min(self.playlist.len() - 1),
            None => 0,
        };
        self.playlist_state.select(Some(next));
    }

    pub fn playlist_scroll_up(&mut self) {
        let prev = match self.playlist_state.selected() {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.playlist_state.select(Some(prev));
    }
}

pub fn read_audio_duration(path: &std::path::PathBuf) -> Option<std::time::Duration> {
    use std::fs::File;
    let file = File::open(path).ok()?;
    let mss = symphonia::core::io::MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = symphonia::core::probe::Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let meta = symphonia::default::get_probe()
        .format(&hint, mss, &Default::default(), &Default::default())
        .ok()?;

    let track = meta.format.default_track()?;
    let tb = track.codec_params.time_base?;
    let frames = track.codec_params.n_frames?;
    let time = tb.calc_time(frames);
    Some(std::time::Duration::from_secs_f64(
        time.seconds as f64 + time.frac,
    ))
}
