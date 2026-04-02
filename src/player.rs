use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, Player};

/// Commands App sends to the player thread
pub enum PlayerCommand {
    Play(PathBuf),
    Pause,
    Resume,
    Stop,
}

/// Status the Player thread sends back to App
#[derive(Clone)]
pub enum PlayerStatus {
    NowPlaying(String), // Display name of the file
    Paused,
    Stopped,
    FinishedNaturally,
    Error(String),
}

/// Spawns the audio thread. Returns the command sender and status receiver.
/// The thread owns the Sink and OutputStream - keeping them here ensures
/// the audio device stays open for the lifetime of the thread.
pub fn spawn_player(cmd_rx: Receiver<PlayerCommand>, status_tx: Sender<PlayerStatus>) {
    std::thread::spawn(move || {
        // open_default_sink() opens the OS audio device.
        // The handle MUST stay alive in this  thread - dropping it
        // immediately stops all playback.

        let handle = match DeviceSinkBuilder::open_default_sink() {
            Ok(h) => h,
            Err(e) => {
                let _ = status_tx.send(PlayerStatus::Error(format!("Audio device: {e}")));
                return;
            }
        };

        // Player gives us pause / resume / stop control over the mixer.
        let player = Player::connect_new(&handle.mixer());

        let mut is_playing = false; // track whether we expect audio to be running

        loop {
            // check if a song just finished naturally
            if is_playing && player.empty() {
                is_playing = false;
                let _ = status_tx.send(PlayerStatus::FinishedNaturally);
            }

            // Block on each command - the thread only  wakes when App sends one.
            //  This keeps CPU at zero between commands.
            match cmd_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(cmd) => match cmd {
                    PlayerCommand::Play(path) => {
                        // Grab the display name before we move the path
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        // stop whatever is currently playing
                        player.stop();

                        let file = match File::open(&path) {
                            Ok(f) => f,
                            Err(e) => {
                                let _ = status_tx
                                    .send(PlayerStatus::Error(format!("Can't open file: {e}")));
                                continue;
                            }
                        };

                        let source = match Decoder::try_from(file) {
                            Ok(s) => s,
                            Err(e) => {
                                let _ = status_tx
                                    .send(PlayerStatus::Error(format!("Decode error: {e}")));
                                continue;
                            }
                        };

                        player.append(source);
                        player.play();
                        is_playing = true;
                        let _ = status_tx.send(PlayerStatus::NowPlaying(name));
                    }

                    PlayerCommand::Pause => {
                        player.pause();
                        let _ = status_tx.send(PlayerStatus::Paused);
                    }

                    PlayerCommand::Resume => {
                        player.play();

                        // Re-send NowPlaying so the UI status bar stays correct
                        // (App caches the current name for this)
                    }

                    PlayerCommand::Stop => {
                        player.stop();
                        let _ = status_tx.send(PlayerStatus::Stopped);
                    }
                },

                // No command waiting - do nothing this tick
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Nothing arrived - loop back and check player.empty() again
                }

                // App dropped the sender - exit the thread cleanly
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        // cmd_rx disconnected - App exited, thread exits cleanly
    });
}
