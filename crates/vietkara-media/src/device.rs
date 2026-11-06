//! Real device backend. Construct and operate on the playback worker, not the UI thread.
//! API reference: https://docs.rs/rodio/0.21.1/rodio/struct.Sink.html
use crate::{Player, PlayerState};
use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct DevicePlayer {
    sink: Sink,
    _stream: OutputStream,
    path: PathBuf,
    duration: f64,
    stopped: bool,
    error: Option<String>,
}
impl DevicePlayer {
    pub fn open(path: &Path, output_device: Option<&str>) -> Result<Self, String> {
        let decoder = Decoder::try_from(File::open(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let duration = decoder
            .total_duration()
            .ok_or("Không đọc được thời lượng audio")?
            .as_secs_f64();
        let mut stream = if let Some(output_device) = output_device {
            let device = cpal::default_host()
                .output_devices()
                .map_err(|e| e.to_string())?
                .find(|device| device.name().is_ok_and(|name| name == output_device))
                .ok_or_else(|| format!("Không tìm thấy thiết bị âm thanh: {output_device}"))?;
            OutputStreamBuilder::from_device(device)
                .and_then(OutputStreamBuilder::open_stream)
                .map_err(|e| e.to_string())?
        } else {
            OutputStreamBuilder::open_default_stream().map_err(|e| e.to_string())?
        };
        stream.log_on_drop(false);
        let sink = Sink::connect_new(stream.mixer());
        sink.pause();
        sink.append(decoder);
        Ok(Self {
            sink,
            _stream: stream,
            path: path.into(),
            duration,
            stopped: true,
            error: None,
        })
    }
    pub fn output_device_names() -> Result<(Vec<String>, Option<String>), String> {
        let host = cpal::default_host();
        let default_name = host
            .default_output_device()
            .and_then(|device| device.name().ok());
        let mut names = host
            .output_devices()
            .map_err(|e| e.to_string())?
            .filter_map(|device| device.name().ok())
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        // Keep default first so the UI can offer a stable “system default” choice.
        if let Some(default_name) = default_name.as_ref()
            && let Some(index) = names.iter().position(|name| name == default_name)
        {
            names.remove(index);
            names.insert(0, default_name.clone());
        }
        Ok((names, default_name))
    }
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
    fn reload(&mut self) -> Result<(), String> {
        let decoder = Decoder::try_from(File::open(&self.path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        self.sink = Sink::connect_new(self._stream.mixer());
        self.sink.pause();
        self.sink.append(decoder);
        Ok(())
    }
    pub fn try_seek(&mut self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() {
            return Err("Mốc seek không hữu hạn".into());
        }
        let playing = self.state() == PlayerState::Playing;
        if self.sink.empty() {
            self.reload()?;
        }
        self.sink
            .try_seek(Duration::from_secs_f64(seconds.clamp(0., self.duration)))
            .map_err(|e| e.to_string())?;
        self.stopped = false;
        if playing {
            self.sink.play();
        }
        Ok(())
    }
}
impl Player for DevicePlayer {
    fn play(&mut self) {
        if self.sink.empty()
            && let Err(error) = self.reload()
        {
            self.error = Some(error);
            return;
        }
        self.stopped = false;
        self.sink.play();
    }
    fn pause(&mut self) {
        self.sink.pause();
    }
    fn stop(&mut self) {
        self.sink.pause();
        if let Err(error) = self.try_seek(0.) {
            self.error = Some(error);
        }
        self.stopped = true;
    }
    fn seek(&mut self, seconds: f64) {
        if let Err(error) = self.try_seek(seconds) {
            self.error = Some(error);
        }
    }
    fn position(&self) -> f64 {
        if self.stopped {
            0.
        } else if self.sink.empty() {
            self.duration
        } else {
            self.sink.get_pos().as_secs_f64().min(self.duration)
        }
    }
    fn duration(&self) -> f64 {
        self.duration
    }
    fn state(&self) -> PlayerState {
        if self.stopped || self.sink.empty() {
            PlayerState::Stopped
        } else if self.sink.is_paused() {
            PlayerState::Paused
        } else {
            PlayerState::Playing
        }
    }
}
