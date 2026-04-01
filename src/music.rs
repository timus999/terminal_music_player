use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

#[derive(Debug)]
pub struct MusicFile {
    pub name: String,
    pub path: PathBuf,
}

/// Sends files as they are found instead of collecting them all first.
/// the ui can start rendering immediately - files trickle in over time.
pub fn scan_music_dir_async(dir: PathBuf, tx: Sender<MusicFile>) {
    std::thread::spawn(move || {
        scan_recursive(&dir, &tx);
    });
}

fn scan_recursive(dir: &Path, tx: &Sender<MusicFile>) {
    let extensions = ["mp3", "flac", "ogg", "wav", "m4a"];

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // silently skip dirs we can't read
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            scan_recursive(&path, tx);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if extensions.contains(&ext.to_lowercase().as_str()) {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                // if the receiver hung up (app closed), stop scanning
                if tx.send(MusicFile { name, path }).is_err() {
                    return;
                }
            }
        }
    }
}
