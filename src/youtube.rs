use rustypipe::client::RustyPipe;
use rustypipe::model::VideoItem;
use std::sync::mpsc::Sender as StdSender;
use tokio::sync::mpsc::Receiver as TokioReceiver;


#[derive(Debug, Clone)]
pub struct YoutubeVideo {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub duration_secs: u64,
}

impl YoutubeVideo {
    pub fn duration_str(&self) -> String {
        let mins = self.duration_secs / 60;
        let secs = self.duration_secs % 60;
        format!("{mins}:{secs:02}")
    }
}


pub enum YoutubeCommand {
    Search(String),
}

pub enum YoutubeResult {
    SearchResults(Vec<YoutubeVideo>),
    StreamUrl(String), // ready-to-play URL -sent directly to player
    Error(String),
}


/// Spawns a background thread that owns a tokio runtime.
/// Commands come in via tokio::sync::mpsc (async-friendly receiver
///
///
/// /// Spawns a background thread that owns a tokio runtime.
/// Commands come in via tokio::sync::mpsc (async-friendly receiver).
/// Results go out via std::sync::mpsc (main thread polls with try_recv).
pub fn spawn_youtube_runtime(
    cmd_rx: TokioReceiver<YoutubeCommand>,
    result_tx: StdSender<YoutubeResult>
) {
    std::thread::spawn( move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        rt.block_on(run_youtube(cmd_rx, result_tx));
        });
}


async fn run_youtube(mut cmd_rx: TokioReceiver<YoutubeCommand>, result_tx: StdSender<YoutubeResult>) {
    // Build once, reuse across all requests - rustypipe manages
    // it's own HTTP client and caches API keys internally
    let rp = match RustyPipe::builder().build() {
        Ok(r) => r,
        Err(e) => {
            let _ = result_tx.send(YoutubeResult::Error(format!("RustyPipe init: {e}")));
            return;
        }
    };

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            YoutubeCommand::Search(query) => {

                match rp.query().search::<VideoItem, _>(&query).await {
                    Ok(results) => {
                        let videos = results
                            .items
                            .items
                            .into_iter()
                            .map(|v| YoutubeVideo {
                                video_id: v.id,
                                title: v.name,
                                channel: v.channel.map(|c| c.name).unwrap_or_default(),
                                duration_secs: v.duration.unwrap_or(0) as u64,
                            })
                            .collect();

                        let _ = result_tx.send(YoutubeResult::SearchResults(videos));
                    }
                    Err(e) => {
                        let _ = result_tx.send(YoutubeResult::Error(format!("Search Error: {e}")));
                    }
                }

            }

            

            
        }
    }
}
