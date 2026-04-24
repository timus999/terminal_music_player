use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

pub enum ImageCommand {
    FetchYoutube(String), // video_id
    FetchLocal(PathBuf),  // path to audio file
    Clear,
}

pub enum ImageResult {
    Loaded(image::DynamicImage),
    NotFound,
    Error(String),
}
pub fn spawn_image_fetcher(cmd_rx: Receiver<ImageCommand>, result_tx: Sender<ImageResult>) {
    std::thread::spawn(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .unwrap();

        for cmd in cmd_rx {
            match cmd {
                ImageCommand::Clear => {
                    let _ = result_tx.send(ImageResult::NotFound);
                }
                ImageCommand::FetchYoutube(video_id) => {
                    // Youtube thumbnails are public - no auth needed
                    // Try highest quality first, fall back to lower
                    let urls = [
                        format!("https://img.youtube.com/vi/{video_id}/maxresdefault.jpg"),
                        format!("https://img.youtube.com/vi/{video_id}/hqdefault.jpg"),
                        format!("https://img.youtube.com/vi/{video_id}/mqdefault.jpg"),
                    ];

                    let mut loaded = false;
                    for url in &urls {
                        match fetch_image(&client, url) {
                            Ok(img) => {
                                let _ = result_tx.send(ImageResult::Loaded(img));
                                loaded = true;
                                break;
                            }
                            Err(_) => continue,
                        }
                    }
                    if !loaded {
                        let _ = result_tx.send(ImageResult::NotFound);
                    }
                }
                ImageCommand::FetchLocal(path) => {
                    // Try embedded cover art first
                    if let Some(img) = load_embedded_cover(&path) {
                        let _ = result_tx.send(ImageResult::Loaded(img));
                        continue;
                    }

                    // Try cover.jpg / folder.jpg in the same directory
                    if let Some(img) = load_folder_cover(&path) {
                        let _ = result_tx.send(ImageResult::Loaded(img));
                        continue;
                    }

                    // fallback to MusicBrainz cover art by filename heuristic
                    let _ = result_tx.send(ImageResult::NotFound);
                }
            }
        }
    });
}

fn fetch_image(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<image::DynamicImage, String> {
    let bytes = client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .bytes()
        .map_err(|e| e.to_string())?;

    image::load_from_memory(&bytes).map_err(|e| e.to_string())
}

/// Reads embedded cover art from ID3/Vorbis tags
fn load_embedded_cover(path: &PathBuf) -> Option<image::DynamicImage> {
    // Try ID3(mp3)
    if let Ok(tag) = id3::Tag::read_from_path(path) {
        for pic in tag.pictures() {
            if let Ok(img) = image::load_from_memory(&pic.data) {
                return Some(img);
            }
        }
    }
    None
}

/// Looks for cover.jpg / folder.jpg /album.jpg next to the audio files
fn load_folder_cover(path: &PathBuf) -> Option<image::DynamicImage> {
    let dir = path.parent()?;
    let candidates = [
        "cover.jpg",
        "cover.png",
        "folder.png",
        "album.png",
        "front.png",
    ];

    for name in candidates {
        let candidate = dir.join(name);
        if candidate.exists() {
            if let Ok(img) = image::open(&candidate) {
                return Some(img);
            }
        }
    }
    None
}
