//! Separate-process streaming ASR. Capture owns the complete audio; inference
//! advances an acknowledged sample cursor and never drops microphone packets.
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::audio_capture::CaptureTap;
use crate::audio_preprocess::StreamingNormalizer;
use crate::desktop_shell::{DesktopShellController, StreamingState};
use crate::managed_process::ManagedChild;
use crate::review_jobs::{ProcessingPermit, ProcessingQueue};

pub const MODEL_ID: &str = "confucius4-r2t2-q8-0";
pub const MODEL_NAME: &str = "R2T2 Q8 · Experimental";
pub const MODEL_FILE: &str = "r2t2-q8_0.gguf";
pub const MODEL_BYTES: i64 = 2_477_512_064;
pub const MODEL_REVISION: &str = "a8e6b385d7df7eae9519363e07034a209004797a";
pub const MODEL_SHA256: &str = "19f5ccd624484bcb5d44301437de41560b0ecc40c430e8850dfeefefbe82ccf5";
pub const CHUNK_SAMPLES: usize = 10_240; // Internal: 640 ms / five-token rollback.
pub const MAX_SAMPLES: usize = 300 * 16_000;
const MAX_MESSAGE: u64 = 1024 * 1024;

#[cfg(target_os = "macos")]
pub(crate) struct MemoryPressureWatch(dispatch2::DispatchRetained<dispatch2::DispatchSource>);
#[cfg(target_os = "macos")]
impl MemoryPressureWatch {
    pub fn new(queue: ProcessingQueue) -> Self {
        use dispatch2::{DispatchObject, DispatchSource};
        extern "C" fn pressure(context: *mut std::ffi::c_void) {
            // The source owns this Box until its finalizer, which runs after
            // all handlers. Eviction uses the same lock as queue admission.
            unsafe {
                (&*context.cast::<ProcessingQueue>()).evict_idle();
            }
        }
        extern "C" fn finalize(context: *mut std::ffi::c_void) {
            unsafe {
                drop(Box::from_raw(context.cast::<ProcessingQueue>()));
            }
        }
        unsafe {
            let source = DispatchSource::new(
                std::ptr::addr_of!(dispatch2::_dispatch_source_type_memorypressure).cast_mut(),
                0,
                0x2 | 0x4,
                None,
            );
            source.set_context(Box::into_raw(Box::new(queue)).cast());
            source.set_event_handler_f(pressure);
            source.set_finalizer_f(finalize);
            source.activate();
            Self(source)
        }
    }
}
#[cfg(target_os = "macos")]
impl Drop for MemoryPressureWatch {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
#[cfg(not(target_os = "macos"))]
pub(crate) struct MemoryPressureWatch;
#[cfg(not(target_os = "macos"))]
impl MemoryPressureWatch {
    pub fn new(_: ProcessingQueue) -> Self {
        Self
    }
}

pub fn release_enabled() -> bool {
    let manifest: Value = serde_json::from_str(include_str!("../../workers/r2t2/manifest.json"))
        .expect("Pinned R2T2 manifest");
    manifest["releaseEnabled"] == true
        || (cfg!(debug_assertions)
            && std::env::var("BLABBER_R2T2_VALIDATION").as_deref() == Ok("1"))
}
pub fn platform_supported() -> bool {
    crate::model_metadata::vibevoice_platform_supported()
}
pub fn model_path(models_dir: &Path) -> PathBuf {
    models_dir.join(MODEL_ID).join(MODEL_FILE)
}
pub(crate) fn source_language(settings: &crate::settings::AppSettings) -> Result<String> {
    if !settings.gpu_enabled {
        bail!("R2T2_SETUP: Enable GPU acceleration in Settings to use R2T2 with Metal.");
    }
    if settings.language_mode == crate::settings::LanguageMode::Auto {
        return Ok(String::new());
    }
    match settings.fixed_language.as_deref() {
        Some("de" | "German") => Ok("German".into()), Some("en" | "English") => Ok("English".into()),
        _ => bail!("R2T2_SETUP: Choose automatic, German or English as the source language for experimental R2T2."),
    }
}
pub(crate) fn transcript(
    id: &str,
    text: String,
    duration_ms: i64,
    language: Option<String>,
) -> crate::asr::TranscriptResult {
    use crate::asr::{TranscriptQualityStatus, TranscriptResult, TranscriptSegment};
    TranscriptResult {
        job_id: id.into(),
        model_name: MODEL_NAME.into(),
        full_text: text.clone(),
        plain_text: text.clone(),
        timestamped_text: text.clone(),
        detected_languages: language.iter().cloned().collect(),
        segments: vec![TranscriptSegment {
            id: format!("{id}:0"),
            start_ms: 0,
            end_ms: duration_ms,
            text,
            language_code: language.unwrap_or_else(|| "und".into()),
            segment_order: 0,
            confidence: None,
            speaker_id: None,
            speaker_ids: None,
            speaker_attribution: Default::default(),
            speaker_confidence: None,
        }],
        quality_status: TranscriptQualityStatus::Clean,
        recovered_region_count: 0,
        warnings: vec![],
        diarization_status: Default::default(),
        diarization_model_id: None,
        diarization_source: Default::default(),
        diarization_warning: None,
        diarization_policy_version: None,
        diarization_clustering_threshold: None,
        diarization_speaker_count_hint: None,
        speakers: vec![],
        diarization_turns: vec![],
    }
}
pub fn helper_path(app: &AppHandle) -> Option<PathBuf> {
    let packaged = app
        .path()
        .resource_dir()
        .ok()?
        .join("workers/r2t2/blabber-r2t2-worker");
    if packaged.is_file() {
        return Some(packaged);
    }
    if cfg!(debug_assertions) {
        let development =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bundle/r2t2/blabber-r2t2-worker");
        if development.is_file() {
            return Some(development);
        }
    }
    None
}
pub fn check_setup(app: &AppHandle, models_dir: &Path) -> Result<(PathBuf, PathBuf)> {
    if !platform_supported() {
        bail!("R2T2_SETUP: R2T2 requires Apple Silicon, Metal and macOS 14 or newer.");
    }
    if !release_enabled() {
        bail!("R2T2_VALIDATION_PENDING: R2T2 is unavailable until its accuracy, continuity and packaged-app acceptance checks pass.");
    }
    let helper = helper_path(app).ok_or_else(|| {
        anyhow!("R2T2_SETUP: The signed R2T2 helper is missing. Reinstall Blabber.")
    })?;
    let model = model_path(models_dir);
    if std::fs::metadata(&model).map(|m| m.len()).ok() != Some(MODEL_BYTES as u64) {
        bail!("R2T2_SETUP: Download or repair R2T2 in Settings → Models.");
    }
    Ok((helper, model))
}

fn active(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::SeqCst) || crate::shutdown::is_shutting_down() {
        bail!("DICTATION_CANCELED: Dictation was canceled.");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    version: u32,
    session_id: String,
    sequence: u64,
    #[serde(rename = "type")]
    kind: String,
    text: String,
    code: String,
    processed_samples: usize,
    #[serde(default)]
    peak_rss_bytes: Option<u64>,
}
#[derive(Default)]
struct Protocol {
    id: String,
    sequence: u64,
    text: String,
}
impl Protocol {
    fn accept(&mut self, message: Message, expected: &str, samples: usize) -> Result<String> {
        if message.version != 1
            || message.session_id != self.id
            || message.sequence != self.sequence + 1
        {
            bail!("R2T2_PROTOCOL: Stale or unordered worker response.");
        }
        self.sequence += 1;
        if message.kind == "error" {
            let code = if !message.code.is_empty()
                && message.code.len() <= 64
                && message
                    .code
                    .bytes()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
            {
                message.code.as_str()
            } else {
                "R2T2_RUNTIME_ERROR"
            };
            bail!(
                "{code}: Live transcription failed. The recording is available to retry or save."
            );
        }
        if message.kind != expected
            || message.processed_samples != samples
            || !message.text.starts_with(&self.text)
        {
            bail!("R2T2_PROTOCOL: Incomplete or inconsistent worker response. Nothing was pasted.");
        }
        self.text = message.text;
        Ok(self.text.clone())
    }
}
enum IoEvent {
    Written,
    Message(Message),
    Failed,
}
pub(crate) struct NativeWorker {
    child: ManagedChild,
    input: mpsc::SyncSender<Vec<u8>>,
    output: mpsc::Receiver<IoEvent>,
    protocol: Protocol,
    input_sequence: u64,
    fingerprint: (u64, std::time::SystemTime),
    peak_rss_bytes: Option<u64>,
}
impl NativeWorker {
    fn spawn(
        helper: &Path,
        model: &Path,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<Self> {
        // Verify before loading; the idle worker's fingerprint is checked on reuse.
        let metadata = std::fs::metadata(model)?;
        let fingerprint = (metadata.len(), metadata.modified()?);
        let mut file = std::fs::File::open(model)?;
        let mut hash = Sha256::new();
        let mut block = vec![0; 1024 * 1024];
        loop {
            active(cancelled)?;
            if Instant::now() >= deadline {
                bail!("R2T2_LOAD_TIMEOUT: Verifying the model exceeded the loading deadline.");
            }
            let count = file.read(&mut block)?;
            if count == 0 {
                break;
            }
            hash.update(&block[..count]);
        }
        if fingerprint.0 != MODEL_BYTES as u64 || format!("{:x}", hash.finalize()) != MODEL_SHA256 {
            bail!("R2T2_SETUP: R2T2 failed its checksum. Repair it in Settings → Models.");
        }
        Self::launch(helper, fingerprint)
    }
    fn launch(helper: &Path, fingerprint: (u64, std::time::SystemTime)) -> Result<Self> {
        let mut command = Command::new(helper);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        crate::managed_process::isolate(&mut command);
        let mut child = ManagedChild::new(
            command
                .spawn()
                .context("R2T2_SETUP: Could not start the native helper")?,
        );
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("R2T2 input unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("R2T2 output unavailable"))?;
        let (input, writes) = mpsc::sync_channel::<Vec<u8>>(1);
        let (events, output) = mpsc::sync_channel(4);
        let written = events.clone();
        std::thread::spawn(move || {
            while let Ok(bytes) = writes.recv() {
                let result = stdin.write_all(&bytes).and_then(|_| stdin.flush());
                let failed = result.is_err();
                if written
                    .send(if failed {
                        IoEvent::Failed
                    } else {
                        IoEvent::Written
                    })
                    .is_err()
                    || failed
                {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.by_ref().take(MAX_MESSAGE + 1).read_line(&mut line) {
                    Ok(0) => {
                        let _ = events.send(IoEvent::Failed);
                        break;
                    }
                    Ok(_) if line.len() as u64 <= MAX_MESSAGE && line.ends_with('\n') => {
                        match serde_json::from_str::<Message>(&line) {
                            Ok(message) => {
                                if events.send(IoEvent::Message(message)).is_err() {
                                    break;
                                }
                            }
                            Err(_) => {
                                let _ = events.send(IoEvent::Failed);
                                break;
                            }
                        }
                    }
                    _ => {
                        let _ = events.send(IoEvent::Failed);
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            input,
            output,
            protocol: Protocol::default(),
            input_sequence: 0,
            fingerprint,
            peak_rss_bytes: None,
        })
    }
    fn reusable(&mut self, model: &Path) -> bool {
        self.child.try_wait().ok() == Some(None)
            && self.output.try_recv().is_err()
            && std::fs::metadata(model)
                .ok()
                .and_then(|m| Some((m.len(), m.modified().ok()?)))
                == Some(self.fingerprint)
    }
    fn exchange(
        &mut self,
        mut request: Value,
        expected: &str,
        samples: usize,
        timeout: Duration,
        check: impl Fn() -> Result<()>,
    ) -> Result<String> {
        check()?;
        request["version"] = json!(1);
        request["sessionId"] = json!(self.protocol.id);
        request["sequence"] = json!(self.input_sequence);
        self.input_sequence += 1;
        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        self.input
            .try_send(bytes)
            .map_err(|_| anyhow!("R2T2_PROTOCOL: Worker input is blocked."))?;
        let deadline = Instant::now() + timeout;
        let mut written = false;
        let mut reply = None;
        loop {
            check()?;
            if Instant::now() >= deadline {
                bail!("R2T2_TIMEOUT: Local processing stopped responding. The recording can be recovered.");
            }
            match self.output.recv_timeout(Duration::from_millis(50)) {
                Ok(IoEvent::Written) if !written => written = true,
                Ok(IoEvent::Message(message)) if reply.is_none() => {
                    self.peak_rss_bytes = self.peak_rss_bytes.max(message.peak_rss_bytes);
                    reply = Some(self.protocol.accept(message, expected, samples)?);
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => bail!(
                    "R2T2_WORKER_FAILED: Local processing ended unexpectedly. Nothing was pasted."
                ),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if written && reply.is_some() {
                return Ok(reply.unwrap());
            }
        }
    }
}

#[derive(Default)]
struct AudioState {
    samples: Vec<f32>,
    finished: bool,
    stop_requested: Option<Instant>,
    processing_started: Option<Instant>,
    error: Option<String>,
    processed: usize,
    maximum_lag: usize,
}
#[derive(Clone)]
pub(crate) struct LiveSession {
    id: String,
    recording_id: String,
    state: Arc<Mutex<AudioState>>,
    result: Arc<Mutex<mpsc::Receiver<Result<Completion>>>>,
    cancelled: Arc<AtomicBool>,
    armed: Arc<AtomicBool>,
}
pub(crate) struct Completion {
    pub text: String,
    // Rust drops fields in declaration order: reap the process before another
    // queue owner may allocate, including when a canceled result is discarded.
    worker: NativeWorker,
    pub permit: ProcessingPermit,
    // Keep shutdown waiting while a completed child is still in the channel.
    _work: crate::shutdown::WorkGuard,
}
impl Completion {
    pub fn release_worker(
        self,
        queue: &ProcessingQueue,
        keep_warm: bool,
    ) -> (String, ProcessingPermit) {
        let Self {
            text,
            permit,
            worker,
            _work,
        } = self;
        if keep_warm {
            queue.cache_idle(&permit, "r2t2", worker);
        } else {
            drop(worker);
        }
        (text, permit)
    }
}
impl LiveSession {
    pub fn activate(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
    fn check(&self) -> Result<()> {
        active(&self.cancelled)?;
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow!("Live audio state unavailable"))?;
        if let Some(error) = &state.error {
            bail!("{error}");
        }
        if state
            .stop_requested
            .zip(state.processing_started)
            .is_some_and(|(stop, admitted)| stop.max(admitted).elapsed() > Duration::from_secs(90))
        {
            bail!("R2T2_FINALIZE_TIMEOUT: Processing exceeded 90 seconds after stopping. The recording can be recovered.");
        }
        Ok(())
    }
    /// Called only once capture has stopped and its last callback was drained.
    pub fn finish(&self, recording_id: &str, expected_samples: usize) -> Result<Completion> {
        if recording_id != self.recording_id {
            bail!("R2T2_CAPTURE_ORDER: Recording belongs to another session.");
        }
        self.state
            .lock()
            .map_err(|_| anyhow!("Live audio state unavailable"))?
            .stop_requested = Some(Instant::now());
        loop {
            self.check()?;
            match self
                .result
                .lock()
                .map_err(|_| anyhow!("Live result unavailable"))?
                .recv_timeout(Duration::from_millis(50))
            {
                Ok(result) => {
                    let completion = result?;
                    let state = self
                        .state
                        .lock()
                        .map_err(|_| anyhow!("Live audio state unavailable"))?;
                    if !state.finished
                        || state.samples.len() != expected_samples
                        || state.processed != expected_samples
                    {
                        bail!("R2T2_CAPTURE_ORDER: The final recording and streamed audio disagree. Nothing was pasted.");
                    }
                    return Ok(completion);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => bail!(
                    "R2T2_WORKER_FAILED: No final transcript. The recording can be recovered."
                ),
            }
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start(
    id: String,
    tap: CaptureTap,
    cancelled: Arc<AtomicBool>,
    queue: ProcessingQueue,
    shell: DesktopShellController,
    helper: PathBuf,
    model: PathBuf,
    language: String,
    context: String,
    release_parent: Arc<dyn Fn() + Send + Sync>,
    stop_capture: Arc<dyn Fn() + Send + Sync>,
) -> Result<LiveSession> {
    let work = crate::shutdown::begin_work(true)?;
    let (tx, rx) = mpsc::channel();
    let session = LiveSession {
        id: id.clone(),
        recording_id: tap.session_id.clone(),
        state: Default::default(),
        result: Arc::new(Mutex::new(rx)),
        cancelled,
        armed: Default::default(),
    };
    queue.enqueue_priority(&id);
    let capture = session.clone();
    let capture_shell = shell.clone();
    let stop = stop_capture.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut normalizer = StreamingNormalizer::new(tap.rate, tap.channels)?;
            let mut cursor = 0;
            let mut published = 0;
            let mut last_capture = Instant::now();
            let mut limit_requested = false;
            loop {
                active(&capture.cancelled)?;
                let part = tap.read_since(cursor, 32_768)?;
                if !part.is_empty() {
                    cursor += part.len();
                    last_capture = Instant::now();
                    normalizer.push(&part)?;
                }
                let mut state = capture
                    .state
                    .lock()
                    .map_err(|_| anyhow!("Live audio state unavailable"))?;
                let stable = normalizer.stable_samples();
                state.samples.extend_from_slice(&stable[published..]);
                published = stable.len();
                state.maximum_lag = state.maximum_lag.max(state.samples.len() - state.processed);
                if state.stop_requested.is_some() && cursor == tap.sample_count() {
                    let final_audio = normalizer.finish()?;
                    state
                        .samples
                        .extend_from_slice(&final_audio.samples[published..]);
                    state.finished = true;
                    break;
                }
                let limit = cursor >= tap.rate as usize * tap.channels as usize * 300;
                let lag = ((state.samples.len() - state.processed) * 1000 / 16000) as u64;
                drop(state);
                let _ = capture_shell.update_stream_lag(&capture.id, lag);
                if limit && !limit_requested {
                    limit_requested = true;
                    capture_shell.stream_duration_limit(&capture.id);
                    stop();
                }
                if !limit && last_capture.elapsed() > Duration::from_secs(5) {
                    bail!("R2T2_CAPTURE_STALLED: The microphone stopped delivering audio. The captured recording can be recovered.");
                }
                if cursor == tap.sample_count() {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            if !capture.cancelled.load(Ordering::SeqCst) {
                if let Ok(mut state) = capture.state.lock() {
                    state.error = Some(error.to_string());
                }
                let _ = capture_shell.update_stream(&id, StreamingState::Failed, None, 0);
                stop();
            }
        }
    });
    let run = session.clone();
    std::thread::spawn(move || {
        let outcome = (|| -> Result<Completion> {
            while !run.armed.load(Ordering::SeqCst) {
                active(&run.cancelled)?;
                std::thread::sleep(Duration::from_millis(1));
            }
            let queued_at = Instant::now();
            let _ = shell.update_stream(&run.id, StreamingState::Waiting, None, 0);
            let (permit, cached) =
                queue.acquire_with_idle(&run.id, &run.cancelled, Some("r2t2"))?;
            let queue_ms = queued_at.elapsed().as_millis();
            let loading = Instant::now();
            run.state
                .lock()
                .map_err(|_| anyhow!("Live audio state unavailable"))?
                .processing_started = Some(Instant::now());
            run.check()?;
            release_parent();
            let _ = shell.update_stream(&run.id, StreamingState::Preparing, None, 0);
            let mut worker = cached
                .and_then(|value| value.downcast::<NativeWorker>().ok())
                .map(|w| *w)
                .filter(|w| {
                    std::fs::metadata(&model).map(|m| m.len()).ok() == Some(w.fingerprint.0)
                });
            if worker.as_mut().is_some_and(|w| !w.reusable(&model)) {
                worker = None;
            }
            let mut worker = match worker {
                Some(worker) => worker,
                None => NativeWorker::spawn(
                    &helper,
                    &model,
                    &run.cancelled,
                    loading + Duration::from_secs(90),
                )?,
            };
            worker.protocol = Protocol {
                id: run.id.clone(),
                ..Default::default()
            };
            worker.input_sequence = 0;
            worker.exchange(
                json!({"type":"start", "modelPath":model, "language":language, "context":context,
                "chunkMs":640, "rollbackTokens":5}),
                "ready",
                0,
                Duration::from_secs(90).saturating_sub(loading.elapsed()),
                || run.check(),
            )?;
            eprintln!(
                "[r2t2] ready queue_ms={queue_ms} load_ms={}",
                loading.elapsed().as_millis()
            );
            let mut cursor = 0;
            loop {
                run.check()?;
                let state = run
                    .state
                    .lock()
                    .map_err(|_| anyhow!("Live audio state unavailable"))?;
                let available = state.samples.len() - cursor;
                let finished = state.finished;
                let lag = (available * 1000 / 16000) as u64;
                let phase = if finished {
                    StreamingState::Finishing
                } else if lag > 2000 {
                    StreamingState::CatchingUp
                } else {
                    StreamingState::Listening
                };
                let _ = shell.update_stream(&run.id, phase, None, lag);
                if available == 0 && finished {
                    break;
                }
                if available < CHUNK_SAMPLES && !finished {
                    drop(state);
                    std::thread::sleep(Duration::from_millis(25));
                    continue;
                }
                let count = available.min(CHUNK_SAMPLES);
                let samples: Vec<f32> = state.samples[cursor..cursor + count]
                    .iter()
                    .map(|sample| sample.clamp(-1.0, 1.0))
                    .collect();
                drop(state);
                let text = worker.exchange(
                    json!({"type":"audio", "startSample":cursor, "samples":samples}),
                    "progress",
                    cursor + count,
                    Duration::from_secs(30),
                    || run.check(),
                )?;
                cursor += count;
                let mut state = run
                    .state
                    .lock()
                    .map_err(|_| anyhow!("Live audio state unavailable"))?;
                state.processed = cursor;
                let lag = ((state.samples.len() - cursor) * 1000 / 16000) as u64;
                let phase = if state.finished {
                    StreamingState::Finishing
                } else if lag > 2000 {
                    StreamingState::CatchingUp
                } else {
                    StreamingState::Listening
                };
                drop(state);
                let _ = shell.update_stream(&run.id, phase, Some(text), lag);
            }
            let text = worker.exchange(
                json!({"type":"finish", "totalSamples":cursor}),
                "result",
                cursor,
                Duration::from_secs(90),
                || run.check(),
            )?;
            run.check()?;
            let state = run
                .state
                .lock()
                .map_err(|_| anyhow!("Live audio state unavailable"))?;
            let finalization = state
                .stop_requested
                .zip(state.processing_started)
                .map(|(stop, admitted)| stop.max(admitted).elapsed().as_millis())
                .unwrap_or(0);
            eprintln!("[r2t2] complete audio_ms={} finish_ms={finalization} peak_lag_ms={} peak_rss_bytes={:?}", cursor * 1000 / 16000, state.maximum_lag * 1000 / 16000, worker.peak_rss_bytes);
            Ok(Completion {
                text,
                permit,
                worker,
                _work: work,
            })
        })();
        queue.remove(&run.id);
        let failed = outcome.is_err();
        let _ = tx.send(outcome);
        if failed && !run.cancelled.load(Ordering::SeqCst) {
            stop_capture();
        }
    });
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(sequence: u64, text: &str, samples: usize) -> Message {
        Message {
            version: 1,
            session_id: "current".into(),
            sequence,
            kind: "progress".into(),
            text: text.into(),
            code: String::new(),
            processed_samples: samples,
            peak_rss_bytes: None,
        }
    }
    #[test]
    fn protocol_rejects_stale_duplicate_missing_audio_and_rewritten_text() {
        let mut protocol = Protocol {
            id: "current".into(),
            ..Default::default()
        };
        assert_eq!(
            protocol
                .accept(message(1, "Größe", 10240), "progress", 10240)
                .unwrap(),
            "Größe"
        );
        assert!(protocol
            .accept(message(1, "Größe", 10240), "progress", 10240)
            .is_err());
        assert!(protocol
            .accept(message(2, "changed", 20480), "progress", 20480)
            .is_err());
        let mut protocol = Protocol {
            id: "other".into(),
            ..Default::default()
        };
        assert!(protocol.accept(message(1, "", 0), "ready", 0).is_err());
        let mut protocol = Protocol {
            id: "current".into(),
            ..Default::default()
        };
        assert!(protocol
            .accept(message(1, "", 5120), "progress", 10240)
            .is_err());
    }
    #[test]
    fn artifact_pins_match_native_manifest() {
        let manifest: Value =
            serde_json::from_str(include_str!("../../workers/r2t2/manifest.json")).unwrap();
        assert_eq!(manifest["modelId"], MODEL_ID);
        assert_eq!(manifest["modelBytes"], MODEL_BYTES);
        assert_eq!(manifest["modelSha256"], MODEL_SHA256);
        assert_eq!(manifest["modelRevision"], MODEL_REVISION);
        assert_eq!(manifest["modelFile"], MODEL_FILE);
    }
    #[cfg(unix)]
    #[test]
    fn stalled_and_malformed_workers_fail_and_are_reaped() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("r2t2-worker-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("worker");
        for source in [
            "#!/bin/sh\nread request\nprintf '%s\\n' 'not JSON'\n",
            "#!/bin/sh\nread request\nsleep 30\n",
        ] {
            std::fs::write(&script, source).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut worker = NativeWorker::launch(&script, (0, std::time::UNIX_EPOCH)).unwrap();
            worker.protocol.id = "current".into();
            let began = Instant::now();
            assert!(worker
                .exchange(
                    json!({"type":"start"}),
                    "ready",
                    0,
                    Duration::from_millis(100),
                    || Ok(())
                )
                .is_err());
            worker.child.kill().unwrap();
            assert!(worker.child.try_wait().unwrap().is_some());
            assert!(began.elapsed() < Duration::from_secs(2));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn discarded_completion_reaps_worker_before_releasing_admission() {
        let queue = ProcessingQueue::default();
        queue.enqueue("finished");
        let permit = queue.acquire("finished", &AtomicBool::new(false)).unwrap();
        let worker =
            NativeWorker::launch(Path::new("/bin/cat"), (0, std::time::UNIX_EPOCH)).unwrap();
        let pid = worker.child.id() as i32;
        queue.enqueue("next");
        let next_queue = queue.clone();
        let next = std::thread::spawn(move || {
            let _permit = next_queue.acquire("next", &AtomicBool::new(false)).unwrap();
            // kill(0) also sees zombies, so this verifies both termination and reap.
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        });
        drop(Completion {
            text: String::new(),
            worker,
            permit,
            _work: crate::shutdown::begin_work(true).unwrap(),
        });
        next.join().unwrap();
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "Requires the pinned 2.48 GB model, native helper, synthetic fixture and Metal access"]
    fn real_native_adapter_completes_and_reuses_a_warm_session() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let model = root.join("target/r2t2-model").join(MODEL_FILE);
        let helper = std::env::var_os("BLABBER_R2T2_TEST_HELPER")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("bundle/r2t2/blabber-r2t2-worker"));
        let audio = crate::audio_preprocess::decode_audio_file(
            &root.join("target/r2t2-fixtures/en-01.wav"),
        )
        .unwrap();
        let cancelled = AtomicBool::new(false);
        let mut worker = NativeWorker::spawn(
            &helper,
            &model,
            &cancelled,
            Instant::now() + Duration::from_secs(90),
        )
        .unwrap();
        let pid = worker.child.id();
        for id in ["cold", "warm"] {
            assert!(worker.reusable(&model));
            worker.protocol = Protocol {
                id: id.into(),
                ..Default::default()
            };
            worker.input_sequence = 0;
            worker
                .exchange(
                    json!({"type":"start", "modelPath":model, "language":"English", "context":"",
                "chunkMs":640, "rollbackTokens":5}),
                    "ready",
                    0,
                    Duration::from_secs(90),
                    || Ok(()),
                )
                .unwrap();
            let mut cursor = 0;
            for chunk in audio.samples.chunks(CHUNK_SAMPLES) {
                worker
                    .exchange(
                        json!({"type":"audio", "startSample":cursor, "samples":chunk}),
                        "progress",
                        cursor + chunk.len(),
                        Duration::from_secs(30),
                        || Ok(()),
                    )
                    .unwrap();
                cursor += chunk.len();
            }
            let text = worker
                .exchange(
                    json!({"type":"finish", "totalSamples":cursor}),
                    "result",
                    cursor,
                    Duration::from_secs(90),
                    || Ok(()),
                )
                .unwrap();
            assert_eq!(
                text.trim(),
                "Please send the updated invoice to Dr. Miller tomorrow."
            );
            assert_eq!(worker.child.id(), pid);
        }
    }
    #[test]
    fn queue_waiting_is_not_counted_as_finalization_and_cancel_invalidates_immediately() {
        let (_tx, rx) = mpsc::channel();
        let session = LiveSession {
            id: "a".into(),
            recording_id: "audio".into(),
            result: Arc::new(Mutex::new(rx)),
            state: Default::default(),
            cancelled: Default::default(),
            armed: Default::default(),
        };
        session.state.lock().unwrap().stop_requested =
            Some(Instant::now() - Duration::from_secs(100));
        assert!(session.check().is_ok());
        session.state.lock().unwrap().processing_started = Some(Instant::now());
        assert!(session.check().is_ok());
        session.state.lock().unwrap().processing_started =
            Some(Instant::now() - Duration::from_secs(100));
        assert!(session.check().is_err());
        session.cancel();
        assert!(session.check().is_err());
        assert!(session.finish("different-audio", 0).is_err());
    }
}
