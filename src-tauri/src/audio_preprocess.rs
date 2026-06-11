use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter};
use sha2::{Digest, Sha256};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};
use uuid::Uuid;

pub const TARGET_SAMPLE_RATE_HZ: u32 = 16_000;
pub const TARGET_CHANNELS: u16 = 1;
pub const MAX_AUDIO_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_AUDIO_DURATION_MS: i64 = 6 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct PreparedAudio {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct AudioFileInfo {
    pub duration_ms: i64,
    pub sha256: String,
}

pub fn inspect_audio_file(path: &Path) -> Result<AudioFileInfo> {
    validate_audio_file_size(path)?;
    let prepared = decode_audio_file(path)?;
    let duration_ms = prepared_duration_ms(&prepared);
    validate_audio_duration(duration_ms)?;
    let sha256 = sha256_file(path)?;
    Ok(AudioFileInfo {
        duration_ms,
        sha256,
    })
}

pub fn decode_audio_file(path: &Path) -> Result<PreparedAudio> {
    validate_audio_file_size(path)?;
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("wav"))
    {
        let prepared = decode_wav_file(path)?;
        validate_audio_duration(prepared_duration_ms(&prepared))?;
        return Ok(prepared);
    }

    let prepared = match decode_audio_file_native(path) {
        Ok(prepared) => Ok(prepared),
        Err(native_error) => decode_audio_file_with_fallback(path).with_context(|| {
            format!(
                "native decode failed for {}: {}",
                path.display(),
                native_error
            )
        }),
    }?;
    validate_audio_duration(prepared_duration_ms(&prepared))?;
    Ok(prepared)
}

fn decode_audio_file_native(path: &Path) -> Result<PreparedAudio> {
    let file = File::open(path)
        .with_context(|| format!("failed to open audio file {}", path.display()))?;
    let media_source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }

    let probed = get_probe()
        .format(
            &hint,
            media_source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("failed to probe audio format")?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("No supported audio track was found"))?;
    let track_id = track.id;
    let sample_rate_hz = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("Unable to determine audio sample rate"))?;
    let channels = track
        .codec_params
        .channels
        .map(|channels| channels.count() as u16)
        .ok_or_else(|| anyhow!("Unable to determine audio channel count"))?;

    let mut decoder = get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("failed to create audio decoder")?;
    let mut samples = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                return Err(anyhow!("Audio stream reset is not supported for this file"));
            }
            Err(error) => return Err(error).context("failed to read audio packet"),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder
            .decode(&packet)
            .with_context(|| format!("failed to decode {}", path.display()))?;

        let spec = *decoded.spec();
        let duration = decoded.capacity() as u64;
        let mut sample_buffer = SampleBuffer::<f32>::new(duration, spec);
        sample_buffer.copy_interleaved_ref(decoded);
        samples.extend_from_slice(sample_buffer.samples());
    }

    Ok(normalize_audio(&samples, sample_rate_hz, channels))
}

fn decode_audio_file_with_fallback(path: &Path) -> Result<PreparedAudio> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(prepared) = decode_audio_file_with_afconvert(path) {
            return Ok(prepared);
        }
    }

    decode_audio_file_with_ffmpeg(path)
}

#[cfg(target_os = "macos")]
fn decode_audio_file_with_afconvert(path: &Path) -> Result<PreparedAudio> {
    let temp_wav = transcoded_temp_wav_path(path, "afconvert");
    let output = Command::new("afconvert")
        .arg(path)
        .arg("-f")
        .arg("WAVE")
        .arg("-d")
        .arg("LEI16@16000")
        .arg("-c")
        .arg("1")
        .arg(&temp_wav)
        .output()
        .with_context(|| format!("failed to launch afconvert for {}", path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        cleanup_temp_file(&temp_wav);
        return Err(anyhow!(
            "afconvert failed for {}: {}{}",
            path.display(),
            stderr,
            if stdout.is_empty() {
                "".to_string()
            } else {
                format!(" {}", stdout)
            }
        ));
    }

    let decoded = decode_wav_file(&temp_wav);
    cleanup_temp_file(&temp_wav);
    decoded
}

fn decode_audio_file_with_ffmpeg(path: &Path) -> Result<PreparedAudio> {
    let temp_wav = transcoded_temp_wav_path(path, "ffmpeg");
    let ffmpeg_binary = find_command_binary("ffmpeg").ok_or_else(|| {
        anyhow!("No fallback audio decoder was found. Install ffmpeg or use WAV/MP3 input.")
    })?;
    let output = Command::new(&ffmpeg_binary)
        .arg("-y")
        .arg("-i")
        .arg(path)
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg("16000")
        .arg("-f")
        .arg("wav")
        .arg(&temp_wav)
        .output()
        .with_context(|| {
            format!(
                "failed to launch {} for {}",
                ffmpeg_binary.display(),
                path.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        cleanup_temp_file(&temp_wav);
        return Err(anyhow!("ffmpeg failed for {}: {}", path.display(), stderr));
    }

    let decoded = decode_wav_file(&temp_wav);
    cleanup_temp_file(&temp_wav);
    decoded
}

fn transcoded_temp_wav_path(path: &Path, tool_name: &str) -> std::path::PathBuf {
    let file_stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("transcode");
    std::env::temp_dir().join(format!(
        "{file_stem}-{tool_name}-{}-{}.wav",
        std::process::id(),
        Uuid::new_v4()
    ))
}

fn cleanup_temp_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn find_command_binary(command: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(command);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct);
    }

    let mut candidates = Vec::new();
    if let Some(path_env) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&path_env) {
            candidates.push(entry.join(command));
        }
    }

    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/opt/homebrew/bin").join(command));
        candidates.push(PathBuf::from("/usr/local/bin").join(command));
        candidates.push(PathBuf::from("/opt/local/bin").join(command));
        candidates.push(PathBuf::from("/usr/bin").join(command));
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

pub fn decode_wav_file(path: &Path) -> Result<PreparedAudio> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to read wav file {}", path.display()))?;
    let spec = reader.spec();
    let samples = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read float wav samples")?,
        SampleFormat::Int => {
            let scale = (1_i64 << (spec.bits_per_sample.saturating_sub(1) as u32)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to read pcm wav samples")?
        }
    };
    Ok(normalize_audio(&samples, spec.sample_rate, spec.channels))
}

pub fn normalize_audio(samples: &[f32], sample_rate_hz: u32, channels: u16) -> PreparedAudio {
    let mono = mix_to_mono(samples, channels);
    let resampled = if sample_rate_hz == TARGET_SAMPLE_RATE_HZ {
        mono
    } else {
        resample_linear(&mono, sample_rate_hz, TARGET_SAMPLE_RATE_HZ)
    };
    PreparedAudio {
        sample_rate_hz: TARGET_SAMPLE_RATE_HZ,
        channels: TARGET_CHANNELS,
        samples: resampled,
    }
}

pub fn write_wav(path: &Path, audio: &PreparedAudio) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let spec = WavSpec {
        channels: audio.channels,
        sample_rate: audio.sample_rate_hz,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec)
        .with_context(|| format!("failed to create wav {}", path.display()))?;
    for sample in &audio.samples {
        let clamped = sample.clamp(-1.0, 1.0);
        writer.write_sample((clamped * i16::MAX as f32).round() as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

fn mix_to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let channels = channels as usize;
    samples
        .chunks(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
        .collect()
}

fn resample_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == 0 || from_rate == to_rate {
        return samples.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let target_len = ((samples.len() as f64) * ratio).round() as usize;
    if target_len <= 1 {
        return samples.to_vec();
    }

    let mut output = Vec::with_capacity(target_len);
    for index in 0..target_len {
        let source_position = index as f64 / ratio;
        let left_index = source_position.floor() as usize;
        let right_index = (left_index + 1).min(samples.len().saturating_sub(1));
        let fraction = (source_position - left_index as f64) as f32;
        let left = samples[left_index];
        let right = samples[right_index];
        output.push(left + (right - left) * fraction);
    }
    output
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn validate_audio_file_size(path: &Path) -> Result<()> {
    let metadata =
        std::fs::metadata(path).with_context(|| format!("failed to read {}", path.display()))?;
    if metadata.len() > MAX_AUDIO_FILE_BYTES {
        return Err(anyhow!(
            "Audio file is too large: {:.2} GB. Blabber currently supports files up to 2.00 GB.",
            metadata.len() as f64 / 1_000_000_000.0
        ));
    }
    Ok(())
}

fn validate_audio_duration(duration_ms: i64) -> Result<()> {
    if duration_ms > MAX_AUDIO_DURATION_MS {
        return Err(anyhow!(
            "Audio file is too long: {:.1} hours. Blabber currently supports files up to 6 hours.",
            duration_ms as f64 / 3_600_000.0
        ));
    }
    Ok(())
}

fn prepared_duration_ms(prepared: &PreparedAudio) -> i64 {
    if prepared.samples.is_empty() {
        0
    } else {
        (((prepared.samples.len() as f64 / prepared.sample_rate_hz as f64) * 1000.0).round() as i64)
            .max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("blabber-audio-test-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn streaming_sha256_matches_known_digest() {
        let path = temp_path("sha256");
        std::fs::write(&path, b"abc").expect("write fixture");
        let digest = sha256_file(&path).expect("hash fixture");
        let _ = std::fs::remove_file(path);
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn transcoded_temp_paths_are_unique_for_same_source() {
        let source = Path::new("/tmp/same-name.m4a");
        let first = transcoded_temp_wav_path(source, "ffmpeg");
        let second = transcoded_temp_wav_path(source, "ffmpeg");
        assert_ne!(first, second);
    }

    #[test]
    fn wav_decode_mixes_stereo_float_wav_to_mono() {
        let path = temp_path("stereo-float").with_extension("wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: TARGET_SAMPLE_RATE_HZ,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, spec).expect("create wav");
        for sample in [0.2_f32, 0.6_f32, -0.4_f32, 0.0_f32] {
            writer.write_sample(sample).expect("write sample");
        }
        writer.finalize().expect("finalize wav");

        let prepared = decode_wav_file(&path).expect("decode wav");
        let _ = std::fs::remove_file(path);

        assert_eq!(prepared.sample_rate_hz, TARGET_SAMPLE_RATE_HZ);
        assert_eq!(prepared.channels, TARGET_CHANNELS);
        assert_eq!(prepared.samples.len(), 2);
        assert!((prepared.samples[0] - 0.4).abs() < 0.0001);
        assert!((prepared.samples[1] - -0.2).abs() < 0.0001);
    }
}
