use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

use crate::audio_preprocess;
use crate::diarization::{RawDiarizationTurn, DIARIZATION_MODEL_SPEC_V1};

pub const WORKER_ARG: &str = "--diarize-worker";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRequest {
    pub job_id: String,
    pub audio_path: PathBuf,
    pub package_path: PathBuf,
    pub exact_speaker_count: Option<i32>,
    pub spec_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerOutput {
    Heartbeat,
    Result { turns: Vec<RawDiarizationTurn> },
    Error { message: String },
}

pub fn run_stdio_worker() -> i32 {
    match run_stdio_worker_inner() {
        Ok(()) => 0,
        Err(error) => {
            let _ = emit_output(&WorkerOutput::Error {
                message: error.to_string(),
            });
            1
        }
    }
}

fn run_stdio_worker_inner() -> Result<()> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("failed to read diarization worker request")?;
    let request: WorkerRequest =
        serde_json::from_str(&input).context("failed to parse diarization worker request")?;
    if request.spec_version != DIARIZATION_MODEL_SPEC_V1.manifest_version {
        return Err(anyhow!("Unsupported diarization manifest version."));
    }
    if request
        .exact_speaker_count
        .is_some_and(|count| !(1..=20).contains(&count))
    {
        return Err(anyhow!("Speaker count must be between 1 and 20."));
    }

    let finished = Arc::new(AtomicBool::new(false));
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let heartbeat = spawn_heartbeat(Arc::clone(&finished), Arc::clone(&stdout));
    let result = diarize(&request);
    finished.store(true, Ordering::SeqCst);
    let _ = heartbeat.join();
    emit_output_to(&stdout, &WorkerOutput::Result { turns: result? })
}

fn diarize(request: &WorkerRequest) -> Result<Vec<RawDiarizationTurn>> {
    let segmentation_path = request.package_path.join("segmentation.onnx");
    let embedding_path = request.package_path.join("embedding.onnx");
    if !segmentation_path.is_file() || !embedding_path.is_file() {
        return Err(anyhow!(
            "Diarization package is incomplete; segmentation.onnx and embedding.onnx are required."
        ));
    }
    let threads = thread::available_parallelism()
        .map(|value| value.get().min(4) as i32)
        .unwrap_or(1);
    let config = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(segmentation_path.to_string_lossy().into_owned()),
            },
            num_threads: threads,
            debug: false,
            provider: Some("cpu".to_string()),
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(embedding_path.to_string_lossy().into_owned()),
            num_threads: threads,
            debug: false,
            provider: Some("cpu".to_string()),
        },
        clustering: FastClusteringConfig {
            num_clusters: request.exact_speaker_count.unwrap_or(-1),
            threshold: DIARIZATION_MODEL_SPEC_V1.clustering_threshold,
        },
        min_duration_on: DIARIZATION_MODEL_SPEC_V1.min_duration_on_seconds,
        min_duration_off: DIARIZATION_MODEL_SPEC_V1.min_duration_off_seconds,
    };
    let diarizer = OfflineSpeakerDiarization::create(&config)
        .ok_or_else(|| anyhow!("sherpa-onnx could not initialize the diarization models."))?;
    let prepared = audio_preprocess::decode_audio_file(&request.audio_path)?;
    if diarizer.sample_rate() != prepared.sample_rate_hz as i32 {
        return Err(anyhow!(
            "Diarization model expects {} Hz audio, but preprocessing produced {} Hz.",
            diarizer.sample_rate(),
            prepared.sample_rate_hz
        ));
    }
    let result = diarizer
        .process(&prepared.samples)
        .ok_or_else(|| anyhow!("sherpa-onnx diarization failed."))?;
    let segments = result.sort_by_start_time();
    let intervals = segments
        .iter()
        .map(|segment| {
            (
                (segment.start * 1000.0).round() as i64,
                (segment.end * 1000.0).round() as i64,
                segment.speaker,
            )
        })
        .filter(|(start, end, _)| end > start)
        .collect::<Vec<_>>();
    Ok(atomic_turns(&intervals))
}

fn atomic_turns(intervals: &[(i64, i64, i32)]) -> Vec<RawDiarizationTurn> {
    let mut boundaries = intervals
        .iter()
        .flat_map(|(start, end, _)| [*start, *end])
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut turns: Vec<RawDiarizationTurn> = Vec::new();
    for window in boundaries.windows(2) {
        let start_ms = window[0];
        let end_ms = window[1];
        let mut cluster_ids = intervals
            .iter()
            .filter(|(start, end, _)| *start < end_ms && *end > start_ms)
            .map(|(_, _, speaker)| *speaker)
            .collect::<Vec<_>>();
        cluster_ids.sort_unstable();
        cluster_ids.dedup();
        if cluster_ids.is_empty() {
            continue;
        }
        if let Some(previous) = turns.last_mut() {
            if previous.end_ms == start_ms && previous.cluster_ids == cluster_ids {
                previous.end_ms = end_ms;
                continue;
            }
        }
        turns.push(RawDiarizationTurn {
            start_ms,
            end_ms,
            cluster_ids,
            confidence: None,
        });
    }
    turns
}

fn spawn_heartbeat(
    finished: Arc<AtomicBool>,
    stdout: Arc<Mutex<io::Stdout>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !finished.load(Ordering::SeqCst) {
            let _ = emit_output_to(&stdout, &WorkerOutput::Heartbeat);
            thread::sleep(Duration::from_secs(2));
        }
    })
}

fn emit_output(output: &WorkerOutput) -> Result<()> {
    emit_output_to(&Arc::new(Mutex::new(io::stdout())), output)
}

fn emit_output_to(stdout: &Arc<Mutex<io::Stdout>>, output: &WorkerOutput) -> Result<()> {
    let mut stdout = stdout
        .lock()
        .map_err(|_| anyhow!("diarization worker stdout lock poisoned"))?;
    serde_json::to_writer(&mut *stdout, output)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

pub fn parse_worker_output_line(line: &str) -> Result<WorkerOutput> {
    serde_json::from_str(line.trim()).context("failed to parse diarization worker output")
}

pub fn read_worker_output_lines<R: io::Read + Send + 'static>(
    reader: R,
    sender: std::sync::mpsc::Sender<Result<WorkerOutput, String>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in io::BufReader::new(reader).lines() {
            let output = line.map_err(|error| error.to_string()).and_then(|line| {
                parse_worker_output_line(&line).map_err(|error| error.to_string())
            });
            if sender.send(output).is_err() {
                break;
            }
        }
    })
}

pub fn run_subprocess_worker(
    request: &WorkerRequest,
    cancelled: Option<&AtomicBool>,
) -> Result<Vec<RawDiarizationTurn>> {
    let executable = std::env::current_exe().context("failed to locate app executable")?;
    let mut child = Command::new(executable)
        .arg(WORKER_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start isolated diarization worker")?;
    if let Some(mut stdin) = child.stdin.take() {
        serde_json::to_writer(&mut stdin, request)?;
        stdin.write_all(b"\n")?;
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("diarization worker stdout was unavailable"))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = read_worker_output_lines(stdout, sender);
    let mut last_output = Instant::now();
    loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(anyhow!("DIARIZATION_CANCELED: diarization canceled"));
        }
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(WorkerOutput::Heartbeat)) => last_output = Instant::now(),
            Ok(Ok(WorkerOutput::Result { turns })) => {
                let status = child.wait()?;
                let _ = reader.join();
                if status.success() {
                    return Ok(turns);
                }
                return Err(anyhow!("Diarization worker exited unsuccessfully."));
            }
            Ok(Ok(WorkerOutput::Error { message })) => {
                let _ = child.wait();
                let _ = reader.join();
                return Err(anyhow!(message));
            }
            Ok(Err(message)) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(anyhow!(message));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if last_output.elapsed() > Duration::from_secs(10 * 60) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(anyhow!("Diarization worker stopped responding."));
                }
                if let Some(status) = child.try_wait()? {
                    let _ = reader.join();
                    return Err(anyhow!("Diarization worker exited with {status}."));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let status = child.wait()?;
                let _ = reader.join();
                return Err(anyhow!(
                    "Diarization worker closed its output with {status}."
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_overlapping_segments_to_atomic_turns() {
        let turns = atomic_turns(&[(0, 1_000, 0), (500, 1_500, 1)]);
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[1].cluster_ids, vec![0, 1]);
        assert_eq!((turns[1].start_ms, turns[1].end_ms), (500, 1_000));
    }
}
