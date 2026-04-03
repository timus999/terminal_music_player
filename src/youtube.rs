use serde::Deserialize;
use std::sync::mpsc::{Receiver, Sender};

// Youtube Data API v3 response shape
#[derive(Debug, Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    id: VideoId,
    snippet: Snippet,
}

#[derive(Debug, Deserialize)]
struct VideoId {
    #[serde(rename = "videoId")]
    video_id: Option<String>, // Option because playlists/channels have different id shapes
}

#[derive(Debug, Deserialize)]
struct Snippet {
    title: String,
    #[serde(rename = "channelTitle")]
    channel_title: String,
}

// Separate call needed for duration - snippet doesn't include it
#[derive(Debug, Deserialize)]
struct VideoDetailsResponse {
    items: Vec<VideoDetailItem>,
}

#[derive(Debug, Deserialize)]
struct VideoDetailItem {
    id: String,
    #[serde(rename = "contentDetails")]
    content_details: ContentDetails,
}

#[derive(Debug, Deserialize)]
struct ContentDetails {
    duration: String,
}
// Matches the shape Invidious returns for each video
#[derive(Debug, Clone)]
pub struct YoutubeVideo {
    pub video_id: String,

    pub title: String,

    pub channel: String,

    pub duration_secs: u64,
}

impl YoutubeVideo {
    pub fn url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.video_id)
    }

    // formats raw seconds -> "3:45"
    pub fn duration_str(&self) -> String {
        let mins = self.duration_secs / 60;
        let secs = self.duration_secs % 60;
        format!("{mins}:{secs:02}")
    }
}

pub enum YoutubeSearchResult {
    Results(Vec<YoutubeVideo>),
    Error(String),
}

// Public Invidious instance - swap this if the instance goes down.
// Full list at: https://docs.invidious.io/instances/

const YT_API_BASE: &str = "https://www.googleapis.com/youtube/v3";

/// Spawn the search thread. App sends queries as Strings,
/// gets back YoutubeSearchResult. Blocks on recv() - zero CPU when idle.
pub fn spawn_youtube_search(
    query_rx: Receiver<String>,
    result_tx: Sender<YoutubeSearchResult>,
    api_key: String,
) {
    std::thread::spawn(move || {
        // Reuse one client across all requests - keeps Tcp connection alive
        let client = reqwest::blocking::Client::new();

        for query in query_rx {
            if query.is_empty() {
                let _ = result_tx.send(YoutubeSearchResult::Results(vec![]));
                continue;
            }

            match search_youtube(&client, &query, &api_key) {
                Err(e) => {
                    let _ = result_tx.send(YoutubeSearchResult::Error(e));
                }
                Ok(videos) => {
                    let _ = result_tx.send(YoutubeSearchResult::Results(videos));
                }
            }
        }
    });
}

fn search_youtube(
    client: &reqwest::blocking::Client,
    query: &str,
    api_key: &str,
) -> Result<Vec<YoutubeVideo>, String> {
    // Step 1 — search for video IDs and basic snippet info
    let search_resp = client
        .get(format!("{YT_API_BASE}/search"))
        .query(&[
            ("part", "snippet"),
            ("q", query),
            ("type", "video"),
            ("maxResults", "15"),
            ("key", api_key),
        ])
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .map_err(|e| format!("Network error: {e}"))?;

    let search_body = search_resp
        .text()
        .map_err(|e| format!("Body read error: {e}"))?;

    let search_data: SearchResponse = serde_json::from_str(&search_body)
        .map_err(|e| format!("Parse error: {e}\nBody: {search_body}"))?;

    // Collect only actual video IDs (filter out playlists/channels)
    let ids: Vec<String> = search_data
        .items
        .iter()
        .filter_map(|item| item.id.video_id.clone())
        .collect();

    if ids.is_empty() {
        return Ok(vec![]);
    }

    // Step 2 — fetch durations for all IDs in one batch call
    let ids_joined = ids.join(",");
    let details_resp = client
        .get(format!("{YT_API_BASE}/videos"))
        .query(&[
            ("part", "contentDetails"),
            ("id", ids_joined.as_str()),
            ("key", api_key),
        ])
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .map_err(|e| format!("Details network error: {e}"))?;

    let details_body = details_resp
        .text()
        .map_err(|e| format!("Details body error: {e}"))?;

    let details_data: VideoDetailsResponse =
        serde_json::from_str(&details_body).map_err(|e| format!("Details parse error: {e}"))?;

    // Build a lookup map: video_id → duration_secs
    let duration_map: std::collections::HashMap<String, u64> = details_data
        .items
        .into_iter()
        .map(|item| {
            let secs = parse_iso8601_duration(&item.content_details.duration);
            (item.id, secs)
        })
        .collect();

    // Zip search results with durations
    let videos = search_data
        .items
        .into_iter()
        .filter_map(|item| {
            let video_id = item.id.video_id?;
            let duration_secs = *duration_map.get(&video_id).unwrap_or(&0);
            Some(YoutubeVideo {
                video_id,
                title: item.snippet.title,
                channel: item.snippet.channel_title,
                duration_secs,
            })
        })
        .collect();

    Ok(videos)
}

/// Parses ISO 8601 duration "PT3M45S" → seconds
/// Handles PT1H2M3S, PT45S, PT3M, PT1H etc.
fn parse_iso8601_duration(duration: &str) -> u64 {
    let mut secs = 0u64;
    let mut current = String::new();

    for ch in duration.chars() {
        match ch {
            'H' => {
                secs += current.parse::<u64>().unwrap_or(0) * 3600;
                current.clear();
            }
            'M' => {
                secs += current.parse::<u64>().unwrap_or(0) * 60;
                current.clear();
            }
            'S' => {
                secs += current.parse::<u64>().unwrap_or(0);
                current.clear();
            }
            'P' | 'T' => {}
            c if c.is_ascii_digit() => current.push(c),
            _ => {}
        }
    }
    secs
}
