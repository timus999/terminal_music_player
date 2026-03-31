use crate::music::{scan_music_dir, MusicFile};
use ratatui::widgets::ListState;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

pub struct App {
    pub running: bool,

    // -- Search --
    pub search_query: String,

    // -- Music Library --
    pub music_files: Vec<MusicFile>,
    pub filtered_indices: Vec<usize>,

    // -- Selection --
    pub list_state: ListState,
}

impl App {
    pub fn new() -> Self {
        let music_dir = Path::new("/home/timus/Music");
        let music_files = scan_music_dir(music_dir);

        // initially show all
        let filtered_indices = (0..music_files.len()).collect();

        let mut list_state = ListState::default();

        if !music_files.is_empty() {
            list_state.select(Some(0)); // highlight first item by default
        }

        Self {
            running: true,
            search_query: String::new(),
            music_files,
            filtered_indices,
            list_state,
        }
    }

    pub fn update_search(&mut self, query: &str) {
        self.search_query = query.to_string();
        let q = query.to_lowercase();

        self.filtered_indices = self
            .music_files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();

        // Always reset to top result - most relevant after a query change
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
