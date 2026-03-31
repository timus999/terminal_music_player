use std::{
    fs::{self},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct MusicFile {
    pub name: String,
    pub path: PathBuf,
}

pub fn scan_music_dir(dir: &Path) -> Vec<MusicFile> {
    let extensions = ["mp3", "flac", "ogg", "wav", "m4a"];

    let mut results = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                results.extend(scan_music_dir(&path));
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.contains(&ext.to_lowercase().as_str()) {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    results.push(MusicFile { name, path });
                }
            }
        }
    }
    results
}
