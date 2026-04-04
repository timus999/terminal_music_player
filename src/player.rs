use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, Player};

pub enum PlayerCommand {
    PlayFile(PathBuf),
    PlayUrl(String),
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
}

#[derive(Clone)]
pub enum PlayerStatus {
    NowPlaying(Option<String>),
    Paused,
    Stopped,
    FinishedNaturally,
    Position(f64),
    Error(String),
}

const MPV_SOCKET: &str = "/tmp/termusic_mpv.sock";

/// Ticks at 100ms each — 30 ticks = 3 seconds for yt-dlp + mpv to initialise
const MPV_STARTUP_GRACE_TICKS: u8 = 30;

// ── Backend ───────────────────────────────────────────────────────────────────

enum PlaybackBackend {
    Rodio,
    Mpv { child: Child, startup_ticks: u8 },
    Idle,
}

impl PlaybackBackend {
    fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    fn stop_current(&mut self, player: &Player) {
        match self {
            Self::Rodio => player.stop(),
            Self::Mpv { child, .. } => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(MPV_SOCKET);
            }
            Self::Idle => {}
        }
        *self = Self::Idle;
    }

    fn pause(&self, player: &Player) {
        match self {
            Self::Rodio => player.pause(),
            Self::Mpv { .. } => mpv_ipc(r#"{"command":["set_property","pause",true]}"#),
            Self::Idle => {}
        }
    }

    fn resume(&self, player: &Player) {
        match self {
            Self::Rodio => player.play(),
            Self::Mpv { .. } => mpv_ipc(r#"{"command":["set_property","pause",false]}"#),
            Self::Idle => {}
        }
    }

    /// Returns Ok(true) when playback ended naturally, Ok(false) when still
    /// running, Err when the backend exited with a failure code.
    fn check_finished(&mut self, player: &Player) -> Result<bool, String> {
        match self {
            Self::Rodio => {
                if player.empty() {
                    *self = Self::Idle;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }

            Self::Mpv {
                child,
                startup_ticks,
            } => {
                // Wait for mpv + yt-dlp to initialise before checking exit
                if *startup_ticks < MPV_STARTUP_GRACE_TICKS {
                    *startup_ticks += 1;
                    return Ok(false);
                }

                match child.try_wait() {
                    Ok(Some(status)) => {
                        let _ = std::fs::remove_file(MPV_SOCKET);
                        *self = Self::Idle;
                        if status.success() {
                            Ok(true)
                        } else {
                            Err(format!(
                                "mpv exited with code {}",
                                status.code().unwrap_or(-1)
                            ))
                        }
                    }
                    Ok(None) => Ok(false),
                    Err(e) => {
                        let _ = std::fs::remove_file(MPV_SOCKET);
                        *self = Self::Idle;
                        Err(format!("mpv wait error: {e}"))
                    }
                }
            }

            Self::Idle => Ok(false),
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn spawn_player(
    cmd_rx: Receiver<PlayerCommand>,
    status_tx: Sender<PlayerStatus>,
    cookies_path: String,
) {
    std::thread::spawn(move || {
        let handle = match DeviceSinkBuilder::open_default_sink() {
            Ok(mut h) => {
                h.log_on_drop(false);
                h
            }
            Err(e) => {
                let _ = status_tx.send(PlayerStatus::Error(format!("Audio device: {e}")));
                return;
            }
        };

        let player = Player::connect_new(&handle.mixer());

        // Resolve yt-dlp path once at thread startup — not on every PlayUrl
        let ytdlp_path =
            resolve_binary_path("yt-dlp").unwrap_or_else(|| "/usr/bin/yt-dlp".to_string());

        let mut backend = PlaybackBackend::Idle;
        let mut is_paused = false;
        let mut stop_requested = false;

        let mut ipc: Option<MpvIpc> = None;

        loop {
            // Poll for natural completion — skipped when paused or after
            // an explicit stop to avoid false FinishedNaturally signals
            if !backend.is_idle() && !is_paused && !stop_requested {
                match backend.check_finished(&player) {
                    Ok(true) => {
                        let _ = status_tx.send(PlayerStatus::FinishedNaturally);
                    }
                    Err(e) => {
                        let _ = status_tx.send(PlayerStatus::Error(e));
                    }
                    Ok(false) => {}
                }
            }

            // Poll mpv IPC - only when Mpv backend is active
            if matches!(backend, PlaybackBackend::Mpv { .. }) {
                // Try to connect if now yet connected
                if ipc.is_none() {
                    if let Some(mut conn) = MpvIpc::connect() {
                        conn.register_observers();
                        ipc = Some(conn);
                    }
                }

                if let Some(ref mut conn) = ipc {
                    let events = conn.drain();
                    for event in events {
                        // observe_property events have "event":"property-change"
                        if event["event"] == "property-change" {
                            let id = event["id"].as_u64();
                            let data = event["data"].as_f64();
                            match (id, data) {
                                (Some(1), Some(pos)) => {
                                    let _ = status_tx.send(PlayerStatus::Position(pos));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            } else {
                // not playing via mpv - drop the connection
                ipc = None;
            }

            match cmd_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) => handle_command(
                    cmd,
                    &player,
                    &mut ipc,
                    &mut backend,
                    &mut is_paused,
                    &mut stop_requested,
                    &status_tx,
                    &ytdlp_path,
                    &cookies_path,
                ),

                Err(mpsc::RecvTimeoutError::Timeout) => {}

                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // App exited — clean up and stop the thread
                    backend.stop_current(&player);
                    break;
                }
            }
        }
    });
}

// ── Command handler ───────────────────────────────────────────────────────────

fn handle_command(
    cmd: PlayerCommand,
    player: &Player,
    ipc: &mut Option<MpvIpc>,
    backend: &mut PlaybackBackend,
    is_paused: &mut bool,
    stop_requested: &mut bool,
    status_tx: &Sender<PlayerStatus>,
    ytdlp_path: &str,
    cookies_path: &str,
) {
    match cmd {
        PlayerCommand::PlayFile(path) => {
            backend.stop_current(player);
            *is_paused = false;
            *stop_requested = false;

            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let file = match File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    let _ = status_tx.send(PlayerStatus::Error(format!("Can't open file: {e}")));
                    return;
                }
            };

            let source = match Decoder::try_from(file) {
                Ok(s) => s,
                Err(e) => {
                    let _ = status_tx.send(PlayerStatus::Error(format!("Decode error: {e}")));
                    return;
                }
            };

            player.append(source);
            player.play();
            *backend = PlaybackBackend::Rodio;
            let _ = status_tx.send(PlayerStatus::NowPlaying(Some(name)));
        }

        PlayerCommand::PlayUrl(url) => {
            backend.stop_current(player);
            *ipc = None;
            *is_paused = false;
            *stop_requested = false;

            match launch_mpv(&url, ytdlp_path, cookies_path) {
                Ok(child) => {
                    *backend = PlaybackBackend::Mpv {
                        child,
                        startup_ticks: 0,
                    };
                    // NowPlaying is sent by App — it holds the video title
                }
                Err(e) => {
                    let _ = status_tx.send(PlayerStatus::Error(e));
                }
            }
        }

        PlayerCommand::Pause => {
            if !*is_paused && !backend.is_idle() {
                backend.pause(player);
                *is_paused = true;
                let _ = status_tx.send(PlayerStatus::Paused);
            }
        }

        PlayerCommand::Resume => {
            if *is_paused && !backend.is_idle() {
                backend.resume(player);
                *is_paused = false;
                // Don't clear stop_requested here — resume doesn't start new playback
                let _ = status_tx.send(PlayerStatus::NowPlaying(None));
            }
        }

        PlayerCommand::Stop => {
            // Set flag BEFORE stopping — the next tick must not fire FinishedNaturally
            *stop_requested = true;
            backend.stop_current(player);
            *is_paused = false;
            let _ = status_tx.send(PlayerStatus::Stopped);
        }

        PlayerCommand::SetVolume(vol) => {
            // rodio - set on player directly
            // mpv - send via IPC
            match backend {
                PlaybackBackend::Rodio => {
                    // rodio Player doesn't expose volume directly
                    // store and apply on next append
                }
                PlaybackBackend::Mpv { .. } => {
                    let pct = (vol * 100.0) as u32;

                    mpv_ipc(&format!(
                        r#"{{"command":["set_property","volume",{}]}}"#,
                        pct
                    ));
                }
                PlaybackBackend::Idle => {}
            }
        }
    }
}

// ── mpv helpers ───────────────────────────────────────────────────────────────

fn launch_mpv(target: &str, ytdlp_path: &str, cookies_path: &str) -> Result<Child, String> {
    // Small delay so the previous socket file is cleaned up before mpv
    // tries to create a new one at the same path
    std::thread::sleep(Duration::from_millis(200));

    Command::new("mpv")
        .args([
            "--no-video",
            "--really-quiet",
            &format!("--script-opts=ytdl_hook-ytdl_path={ytdlp_path}"),
            &format!(
                "--ytdl-raw-options=cookies={},js-runtimes=node,remote-components=ejs:github",
                cookies_path
            ),
            &format!("--input-ipc-server={MPV_SOCKET}"),
            target,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .spawn()
        .map_err(|e| format!("Failed to launch mpv: {e}. Is mpv installed?"))
}

/// Sends a JSON command to the running mpv instance over its IPC socket.
/// Silently does nothing if mpv isn't running yet.
fn mpv_ipc(json: &str) {
    if let Ok(mut stream) = UnixStream::connect(MPV_SOCKET) {
        let _ = stream.write_all(format!("{json}\n").as_bytes());
    }
}

/// Resolves a binary name to its full path using `which`.
/// Returns None if not found.
fn resolve_binary_path(name: &str) -> Option<String> {
    Command::new("which")
        .arg(name)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Queries a single numeric property from mpv via IPC.
/// Returns None if mpv isn't running or property isn't available yet.
// pub fn mpv_get_property(property: &str) -> Option<f64> {
//     let mut stream = UnixStream::connect(MPV_SOCKET).ok()?;
//     stream
//         .set_read_timeout(Some(Duration::from_millis(50)))
//         .ok()?;

//     let cmd = format!(
//         r#"{{"command":["get_property","{}"],"request_id":1}}{}"#,
//         property, "\n"
//     );
//     stream.write_all(cmd.as_bytes()).ok()?;

//     let mut buf = vec![0u8; 256];
//     let n = stream.read(&mut buf).ok()?;
//     let response = std::str::from_utf8(&buf[..n]).ok()?;

//     // mpv may send multiple newline-delimited JSON objects — find the one
//     // with our request_id
//     for line in response.lines() {
//         if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
//             if val["request_id"] == 1 && val["error"] == "success" {
//                 return val["data"].as_f64();
//             }
//         }
//     }
//     None
// }

struct MpvIpc {
    stream: UnixStream,
}

impl MpvIpc {
    /// Connect to the socket — retries until mpv creates it
    fn connect() -> Option<Self> {
        let stream = UnixStream::connect(MPV_SOCKET).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .ok()?;
        stream.set_nonblocking(false).ok()?;
        Some(Self { stream })
    }

    /// Register observers once after connecting — mpv will push updates
    /// automatically every time the property changes
    fn register_observers(&mut self) -> bool {
        // observe_property id=1 for time-pos,
        self.send(r#"{"command":["observe_property",1,"time-pos"]}"#)
    }
    fn send(&mut self, json: &str) -> bool {
        self.stream
            .write_all(format!("{json}\n").as_bytes())
            .is_ok()
    }

    /// Reads all pending lines — mpv sends events continuously
    fn drain(&mut self) -> Vec<serde_json::Value> {
        let mut buf = vec![0u8; 4096];
        let mut results = Vec::new();

        match self.stream.read(&mut buf) {
            Ok(n) if n > 0 => {
                let text = String::from_utf8_lossy(&buf[..n]);
                for line in text.lines() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        results.push(v);
                    }
                }
            }
            _ => {}
        }
        results
    }
}
