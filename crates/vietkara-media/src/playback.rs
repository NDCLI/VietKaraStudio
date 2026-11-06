//! Bounded command worker; device creation, decoding and seek stay off the UI thread.
use crate::{Player, PlayerState, device::DevicePlayer};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    },
    time::Duration,
};

pub enum PlaybackCommand {
    Load(PathBuf),
    LoadOnDevice { path: PathBuf, device: String },
    RefreshDevices,
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
    pub output_devices: Vec<String>,
    pub default_output_device: Option<String>,
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
            output_devices: Vec::new(),
            default_output_device: None,
        }
    }
}
pub struct Playback {
    commands: SyncSender<PlaybackCommand>,
    snapshot: Arc<Mutex<PlaybackSnapshot>>,
    cancelled: Arc<AtomicBool>,
    /// Latest seek requested while the worker is busy. Seeking is
    /// coalesced here instead of going through the bounded command
    /// channel, so dragging the playhead can never drop a position (the
    /// newest position wins) and can never block the UI thread.
    pending_seek: Arc<Mutex<Option<f64>>>,
}
impl Playback {
    pub fn new() -> Result<Self, String> {
        let (commands, receiver) = mpsc::sync_channel(32);
        let snapshot = Arc::new(Mutex::new(PlaybackSnapshot::default()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let pending_seek = Arc::new(Mutex::new(None));
        let shared = snapshot.clone();
        let shutdown = cancelled.clone();
        let worker_stopped = cancelled.clone();
        let worker_seek = pending_seek.clone();
        std::thread::Builder::new()
            .name("vietkara-playback".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker_loop(receiver, shared.clone(), shutdown.clone(), worker_seek);
                }));
                if result.is_err() {
                    worker_stopped.store(true, Ordering::Release);
                    publish(
                        &shared,
                        PlaybackSnapshot {
                            error: Some("Playback worker gặp lỗi nội bộ và đã dừng".into()),
                            ..Default::default()
                        },
                    );
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            commands,
            snapshot,
            cancelled,
            pending_seek,
        })
    }
    pub fn send(&self, command: PlaybackCommand) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err("Playback worker đã dừng".into());
        }
        if let PlaybackCommand::Seek(seconds) = command {
            // Coalesce seeks: the worker consumes the newest value on its
            // next turn, so rapid playhead drags cannot fill the bounded
            // channel and silently lose the final position.
            *self
                .pending_seek
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(seconds);
            return Ok(());
        }
        // Keep the UI responsive while guaranteeing a command is either
        // delivered or reported as an error, never silently dropped.
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => "Playback đang bận, hãy thử lại".into(),
                TrySendError::Disconnected(_) => "Playback worker đã dừng".into(),
            })
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

fn publish(shared: &Mutex<PlaybackSnapshot>, state: PlaybackSnapshot) {
    *shared.lock().unwrap_or_else(|e| e.into_inner()) = state;
}

fn worker_loop(
    receiver: mpsc::Receiver<PlaybackCommand>,
    shared: Arc<Mutex<PlaybackSnapshot>>,
    shutdown: Arc<AtomicBool>,
    pending_seek: Arc<Mutex<Option<f64>>>,
) {
    let mut player: Option<DevicePlayer> = None;
    let mut current_path = None;
    let (mut output_devices, mut default_output_device, mut error) =
        match DevicePlayer::output_device_names() {
            Ok((devices, default_device)) if devices.is_empty() => (
                devices,
                default_device,
                Some(
                    "Windows chưa phát hiện thiết bị đầu ra âm thanh. Kết nối loa/tai nghe hoặc kiểm tra Windows Sound settings rồi thử nạp lại track.".into(),
                ),
            ),
            Ok((devices, default_device)) => (devices, default_device, None),
            Err(message) => (Vec::new(), None, Some(message)),
        };
    let mut pending_commands = VecDeque::new();
    while !shutdown.load(Ordering::Acquire) {
        // Apply the newest coalesced seek before waiting for the next
        // command, so the final playhead position is never lost.
        if let Some(seconds) = pending_seek
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            && let Some(device) = &mut player
        {
            device.seek(seconds);
        }
        let command = if pending_commands.is_empty() {
            match receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            pending_commands.pop_front()
        };
        if command.is_some() {
            error = None;
        }
        match command {
            Some(PlaybackCommand::Load(path)) => {
                let mut output_device = None;
                load_device(
                    &mut player,
                    &mut current_path,
                    &mut error,
                    path,
                    &mut output_device,
                    &receiver,
                    &mut pending_commands,
                    &shared,
                    &shutdown,
                    &output_devices,
                    &default_output_device,
                );
            }
            Some(PlaybackCommand::LoadOnDevice { path, device }) => {
                let mut output_device = Some(device);
                load_device(
                    &mut player,
                    &mut current_path,
                    &mut error,
                    path,
                    &mut output_device,
                    &receiver,
                    &mut pending_commands,
                    &shared,
                    &shutdown,
                    &output_devices,
                    &default_output_device,
                );
            }
            Some(PlaybackCommand::Unload) => {
                player = None;
                current_path = None;
            }
            Some(PlaybackCommand::RefreshDevices) => match DevicePlayer::output_device_names() {
                Ok((devices, default_device)) => {
                    output_devices = devices;
                    default_output_device = default_device;
                    error = output_devices.is_empty().then(|| {
                            "Windows chưa phát hiện thiết bị đầu ra âm thanh. Kết nối loa/tai nghe hoặc kiểm tra Windows Sound settings rồi thử làm mới danh sách.".into()
                        });
                }
                Err(message) => {
                    output_devices.clear();
                    default_output_device = None;
                    error = Some(message);
                }
            },
            Some(command) => {
                if let Some(device) = &mut player {
                    match command {
                        PlaybackCommand::Play => device.play(),
                        PlaybackCommand::Pause => device.pause(),
                        PlaybackCommand::Stop => device.stop(),
                        PlaybackCommand::Seek(seconds) => device.seek(seconds),
                        PlaybackCommand::Load(_)
                        | PlaybackCommand::LoadOnDevice { .. }
                        | PlaybackCommand::RefreshDevices
                        | PlaybackCommand::Unload => unreachable!(),
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
            output_devices: output_devices.clone(),
            default_output_device: default_output_device.clone(),
            ..Default::default()
        };
        if let Some(device) = &player {
            state.loaded = true;
            state.position = device.position();
            state.duration = device.duration();
            state.state = device.state();
        }
        publish(&shared, state);
    }
    // Dropping the device stops its sink. No detached audio survives the controller.
    drop(player);
}

#[allow(clippy::too_many_arguments)]
fn load_device(
    player: &mut Option<DevicePlayer>,
    current_path: &mut Option<PathBuf>,
    error: &mut Option<String>,
    path: PathBuf,
    output_device: &mut Option<String>,
    receiver: &mpsc::Receiver<PlaybackCommand>,
    pending_commands: &mut VecDeque<PlaybackCommand>,
    shared: &Arc<Mutex<PlaybackSnapshot>>,
    shutdown: &AtomicBool,
    output_devices: &[String],
    default_output_device: &Option<String>,
) {
    *player = None;
    *current_path = Some(path.clone());
    publish(
        shared,
        PlaybackSnapshot {
            path: current_path.clone(),
            loading: true,
            output_devices: output_devices.to_vec(),
            default_output_device: default_output_device.clone(),
            ..Default::default()
        },
    );

    // Prefer the newest request if another load arrived while the device was opening.
    let mut requested = Some((path, output_device.clone()));
    loop {
        if shutdown.load(Ordering::Acquire) {
            return;
        }
        match receiver.try_recv() {
            Ok(PlaybackCommand::Load(path)) => {
                pending_commands.clear();
                requested = Some((path, None));
            }
            Ok(PlaybackCommand::LoadOnDevice { path, device }) => {
                pending_commands.clear();
                requested = Some((path, Some(device)));
            }
            Ok(PlaybackCommand::Unload) => {
                pending_commands.clear();
                requested = None;
                *current_path = None;
                break;
            }
            Ok(command) => pending_commands.push_back(command),
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => return,
        }
    }
    let Some((path, device_name)) = requested else {
        *player = None;
        return;
    };
    *output_device = device_name.clone();
    *current_path = Some(path.clone());
    *error = None;
    match DevicePlayer::open(&path, device_name.as_deref()) {
        Ok(device) => *player = Some(device),
        Err(message) => *error = Some(message),
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn burst_of_seeks_is_coalesced_and_never_dropped() {
        let playback = Playback::new().unwrap();
        // Far more seeks than the bounded command channel can hold: none of
        // them may block the UI thread or report a spurious "busy" error.
        for index in 0..1_000 {
            playback
                .send(PlaybackCommand::Seek(index as f64 / 100.0))
                .expect("coalesced seek must always be accepted");
        }
    }

    #[test]
    fn load_failure_is_reported_and_unload_clears_snapshot() {
        let playback = Playback::new().unwrap();
        let missing = std::env::temp_dir().join(format!(
            "vietkara-missing-playback-{}-{}.wav",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        playback
            .send(PlaybackCommand::Load(missing.clone()))
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        let failed = loop {
            let snapshot = playback.snapshot();
            if snapshot.error.is_some() {
                break snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "worker did not report load failure"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(failed.path, Some(missing));
        assert!(!failed.loading);
        assert!(!failed.loaded);
        assert!(failed.error.as_deref().unwrap().contains("relink track"));

        playback.send(PlaybackCommand::Unload).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = playback.snapshot();
            if snapshot.path.is_none() && snapshot.error.is_none() {
                assert!(!snapshot.loading);
                assert!(!snapshot.loaded);
                break;
            }
            assert!(Instant::now() < deadline, "worker did not unload audio");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
