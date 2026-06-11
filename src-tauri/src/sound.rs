use std::io::Cursor;
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

const LISTEN_START_WAV: &[u8] = include_bytes!("../assets/sounds/listen_start.wav");
const LISTEN_STOP_WAV: &[u8] = include_bytes!("../assets/sounds/listen_stop.wav");

enum SoundCommand {
    PlayListenStart,
    PlayListenStop,
}

pub struct SoundPlayer {
    tx: Sender<SoundCommand>,
}

impl SoundPlayer {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel::<SoundCommand>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), String>>(0);

        thread::Builder::new()
            .name("blabber-sound".into())
            .spawn(move || run_sound_thread(rx, init_tx))
            .map_err(|err| anyhow!("failed to spawn sound thread: {err}"))?;

        match init_rx.recv() {
            Ok(Ok(())) => Ok(SoundPlayer { tx }),
            Ok(Err(msg)) => Err(anyhow!("sound init failed: {msg}")),
            Err(_) => Err(anyhow!("sound thread exited before init")),
        }
    }

    pub fn play_listen_start(&self) {
        if self.tx.send(SoundCommand::PlayListenStart).is_err() {
            eprintln!("[sound] play_listen_start: channel closed");
        }
    }

    pub fn play_listen_stop(&self) {
        if self.tx.send(SoundCommand::PlayListenStop).is_err() {
            eprintln!("[sound] play_listen_stop: channel closed");
        }
    }
}

struct PlaybackState {
    samples: Vec<f32>,
    position: usize,
}

impl PlaybackState {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            position: 0,
        }
    }

    fn enqueue(&mut self, new_samples: &[f32]) {
        if self.position >= self.samples.len() {
            self.samples.clear();
            self.position = 0;
        }
        self.samples.extend_from_slice(new_samples);
    }

    fn pull(&mut self) -> Option<f32> {
        if self.position < self.samples.len() {
            let s = self.samples[self.position];
            self.position += 1;
            Some(s)
        } else {
            None
        }
    }
}

fn run_sound_thread(rx: mpsc::Receiver<SoundCommand>, init_tx: SyncSender<Result<(), String>>) {
    let listen_start_44k = match decode_wav(LISTEN_START_WAV) {
        Ok(samples) => samples,
        Err(err) => {
            let _ = init_tx.send(Err(format!("decode listen_start.wav: {err}")));
            return;
        }
    };
    let listen_stop_44k = match decode_wav(LISTEN_STOP_WAV) {
        Ok(samples) => samples,
        Err(err) => {
            let _ = init_tx.send(Err(format!("decode listen_stop.wav: {err}")));
            return;
        }
    };

    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => {
            let _ = init_tx.send(Err("no default output device".into()));
            return;
        }
    };

    let supported = match device.default_output_config() {
        Ok(c) => c,
        Err(err) => {
            let _ = init_tx.send(Err(format!("default_output_config: {err}")));
            return;
        }
    };

    let device_sample_rate = supported.sample_rate();
    let device_channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();

    let listen_start_resampled = resample_linear(&listen_start_44k, 44_100, device_sample_rate);
    let listen_stop_resampled = resample_linear(&listen_stop_44k, 44_100, device_sample_rate);

    let state = Arc::new(Mutex::new(PlaybackState::new()));
    let stream_state = Arc::clone(&state);

    let err_fn = |err| eprintln!("[sound] stream error: {err}");

    let stream_result: Result<Stream, cpal::BuildStreamError> = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            move |output: &mut [f32], _| {
                fill_f32(output, device_channels, &stream_state);
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            &config,
            move |output: &mut [i16], _| {
                fill_i16(output, device_channels, &stream_state);
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_output_stream(
            &config,
            move |output: &mut [u16], _| {
                fill_u16(output, device_channels, &stream_state);
            },
            err_fn,
            None,
        ),
        other => {
            let _ = init_tx.send(Err(format!("unsupported sample format: {other:?}")));
            return;
        }
    };

    let stream = match stream_result {
        Ok(s) => s,
        Err(err) => {
            let _ = init_tx.send(Err(format!("build_output_stream: {err}")));
            return;
        }
    };

    if let Err(err) = stream.play() {
        let _ = init_tx.send(Err(format!("stream.play: {err}")));
        return;
    }

    let _ = init_tx.send(Ok(()));

    while let Ok(cmd) = rx.recv() {
        match cmd {
            SoundCommand::PlayListenStart => {
                if let Ok(mut s) = state.lock() {
                    s.enqueue(&listen_start_resampled);
                }
            }
            SoundCommand::PlayListenStop => {
                if let Ok(mut s) = state.lock() {
                    s.enqueue(&listen_stop_resampled);
                }
            }
        }
    }

    drop(stream);
}

fn fill_f32(output: &mut [f32], channels: usize, state: &Arc<Mutex<PlaybackState>>) {
    let mut guard = match state.lock() {
        Ok(g) => g,
        Err(_) => {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }
    };
    for frame in output.chunks_mut(channels) {
        let v = guard.pull().unwrap_or(0.0);
        for s in frame.iter_mut() {
            *s = v;
        }
    }
}

fn fill_i16(output: &mut [i16], channels: usize, state: &Arc<Mutex<PlaybackState>>) {
    let mut guard = match state.lock() {
        Ok(g) => g,
        Err(_) => {
            for s in output.iter_mut() {
                *s = 0;
            }
            return;
        }
    };
    for frame in output.chunks_mut(channels) {
        let v = guard.pull().unwrap_or(0.0);
        let scaled = (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for s in frame.iter_mut() {
            *s = scaled;
        }
    }
}

fn fill_u16(output: &mut [u16], channels: usize, state: &Arc<Mutex<PlaybackState>>) {
    let mut guard = match state.lock() {
        Ok(g) => g,
        Err(_) => {
            for s in output.iter_mut() {
                *s = u16::MAX / 2;
            }
            return;
        }
    };
    for frame in output.chunks_mut(channels) {
        let v = guard.pull().unwrap_or(0.0);
        let scaled = ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
        for s in frame.iter_mut() {
            *s = scaled;
        }
    }
}

fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>> {
    let cursor = Cursor::new(bytes);
    let mut reader = hound::WavReader::new(cursor).map_err(|err| anyhow!("invalid WAV: {err}"))?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(anyhow!("expected mono WAV, got {} channels", spec.channels));
    }
    if spec.sample_rate != 44_100 {
        return Err(anyhow!(
            "expected 44100 Hz WAV, got {} Hz",
            spec.sample_rate
        ));
    }
    let samples = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| anyhow!("WAV sample read: {err}"))?,
        (hound::SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| anyhow!("WAV sample read: {err}"))?,
        other => return Err(anyhow!("unsupported WAV format: {other:?}")),
    };
    Ok(samples)
}

fn resample_linear(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let dst_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(dst_len);
    for i in 0..dst_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = input[idx.min(input.len() - 1)];
        let b = input[(idx + 1).min(input.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_feedback_sounds_decode() {
        assert!(!decode_wav(LISTEN_START_WAV)
            .expect("listen_start.wav should decode")
            .is_empty());
        assert!(!decode_wav(LISTEN_STOP_WAV)
            .expect("listen_stop.wav should decode")
            .is_empty());
    }
}
