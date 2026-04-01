use crate::music::scan_music_dir_async;
use crate::music::MusicFile;
use ratatui::widgets::ListState;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;

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
}

impl App {
    pub fn new(music_dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();

        // kick off background scan immediately - does NOT block
        scan_music_dir_async(music_dir, tx);

        // initially show all

        Self {
            running: true,
            search_query: String::new(),
            music_files: Vec::new(),
            filtered_indices: Vec::new(),
            list_state: ListState::default(),
            is_scanning: true,
            scan_rx: Some(rx),
        }
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
    pub fn search_local_dir() -> io::Result<Vec<PathBuf>> {
        let music_files: Vec<_> = fs::read_dir(".")?
            .filter_map(|res| res.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .filter(|path| path.extension() == Some("mp3".as_ref()))
            .collect();

        Ok(music_files)
    }
}
