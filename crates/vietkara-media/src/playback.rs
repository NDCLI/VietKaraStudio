//! Bounded command worker; device creation, decoding and seek stay off the UI thread.
use crate::{Player, PlayerState, device::DevicePlayer};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    time::Duration,
};

pub enum PlaybackCommand {
    Load(PathBuf),
    Unload,
    Play,
    Pause,
    Stop,
    Seek(f64),
}
#[derive(Debug, Clone)]
pub struct PlaybackSnapshot {
    pub path: Option<PathBuf>,
    pub loading: bool,
    pub loaded: bool,
    pub state: PlayerState,
    pub position: f64,
    pub duration: f64,
    pub error: Option<String>,
}
impl Default for PlaybackSnapshot {
    fn default() -> Self {
        Self {
            path: None,
            loading: false,
            loaded: false,
            state: PlayerState::Stopped,
            position: 0.,
            duration: 0.,
            error: None,
        }
    }
}
pub struct Playback {
    commands: SyncSender<PlaybackCommand>,
    snapshot: Arc<Mutex<PlaybackSnapshot>>,
    cancelled: Arc<AtomicBool>,
}
impl Playback {
    pub fn new() -> Result<Self, String> {
        let (commands, receiver) = mpsc::sync_channel(16);
        let snapshot = Arc::new(Mutex::new(PlaybackSnapshot::default()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let shared = snapshot.clone();
        let shutdown = cancelled.clone();
        std::thread::Builder::new()
            .name("vietkara-playback".into())
            .spawn(move || {
                let mut player: Option<DevicePlayer> = None;
                let mut current_path = None;
                let mut error = None;
                while !shutdown.load(Ordering::Acquire) {
                    let command = match receiver.recv_timeout(Duration::from_millis(20)) {
                        Ok(command) => Some(command),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    if command.is_some() {
                        error = None;
                    }
                    match command {
                        Some(PlaybackCommand::Load(path)) => {
                            player = None;
                            *shared.lock().unwrap_or_else(|e| e.into_inner()) = PlaybackSnapshot {
                                path: Some(path.clone()),
                                loading: true,
                                ..Default::default()
                            };
                            match DevicePlayer::open(&path) {
                                Ok(device) => player = Some(device),
                                Err(message) => error = Some(message),
                            }
                            current_path = Some(path);
                        }
                        Some(PlaybackCommand::Unload) => {
                            player = None;
                            current_path = None;
                        }
                        Some(command) => {
                            if let Some(device) = &mut player {
                                match command {
                                    PlaybackCommand::Play => device.play(),
                                    PlaybackCommand::Pause => device.pause(),
                                    PlaybackCommand::Stop => device.stop(),
                                    PlaybackCommand::Seek(seconds) => device.seek(seconds),
                                    PlaybackCommand::Load(_) | PlaybackCommand::Unload => {
                                        unreachable!()
                                    }
                                }
                            } else {
                                error = Some("Chưa có audio hợp lệ để phát".into());
                            }
                        }
                        None => {}
                    }
                    if let Some(device) = &mut player
                        && let Some(message) = device.take_error()
                    {
                        error = Some(message);
                    }
                    let mut state = PlaybackSnapshot {
                        path: current_path.clone(),
                        error: error.clone(),
                        ..Default::default()
                    };
                    if let Some(device) = &player {
                        state.loaded = true;
                        state.position = device.position();
                        state.duration = device.duration();
                        state.state = device.state();
                    }
                    *shared.lock().unwrap_or_else(|e| e.into_inner()) = state;
                }
                // Dropping the device stops its sink. No detached audio survives the controller.
                drop(player);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            commands,
            snapshot,
            cancelled,
        })
    }
    pub fn send(&self, command: PlaybackCommand) -> Result<(), String> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "Playback đang bận, hãy thử lại".into(),
                mpsc::TrySendError::Disconnected(_) => "Playback worker đã dừng".into(),
            })
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
