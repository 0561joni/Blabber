use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::asr::{
    FileTranscriptionRequest as EngineFileTranscriptionRequest, InstalledModel, PreviewSourceKind,
    TranscriptResult, TranscriptionEngine,
};
use crate::audio_files::{FileTranscriptionRequest, SelectedSourceFile};
use crate::settings::AppSettings;
use crate::storage::{self, TranscriptSummary};
use crate::transcription_worker::{self, WorkerOutput, WorkerRequest};
use crate::vocabulary;
use crate::{diarization, diarization_worker, model_downloads};

const FILE_TRANSCRIPTION_STATUS_EVENT: &str = "file-transcription-status";
const WATCHDOG_TIMEOUT_MS: i64 = 120_000;
const WORKER_STALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileTranscriptionJobStage {
    Queued,
    Preparing,
    Transcribing,
    Diarizing,
    Saving,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartFileTranscriptionResponse {
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTranscriptionResponse {
    pub source_file: SelectedSourceFile,
    pub resolved_model: Option<InstalledModel>,
    pub result: TranscriptResult,
    pub saved_transcript: Option<TranscriptSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTranscriptionStatusEvent {
    pub job_id: String,
    pub source_file: SelectedSourceFile,
    pub stage: FileTranscriptionJobStage,
    pub progress_percent: Option<f32>,
    pub processed_ms: Option<i64>,
    pub total_ms: Option<i64>,
    pub eta_seconds: Option<i64>,
    pub status_text: String,
    pub result: Option<FileTranscriptionResponse>,
    pub error_message: Option<String>,
    pub started_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone)]
struct ActiveFileTranscriptionRun {
    job_id: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct FileTranscriptionController {
    app: AppHandle,
    engine: Arc<dyn TranscriptionEngine>,
    models_dir: PathBuf,
    db_path: PathBuf,
    statuses: Arc<Mutex<HashMap<String, FileTranscriptionStatusEvent>>>,
    queued_requests: Arc<Mutex<VecDeque<FileTranscriptionRequest>>>,
    active_run: Arc<Mutex<Option<ActiveFileTranscriptionRun>>>,
    log_path: PathBuf,
}

impl FileTranscriptionController {
    pub fn new(
        app: AppHandle,
        engine: Arc<dyn TranscriptionEngine>,
        models_dir: PathBuf,
        db_path: PathBuf,
    ) -> Self {
        let log_path = db_path
            .parent()
            .map(|dir| dir.join("file-transcription.log"))
            .unwrap_or_else(|| PathBuf::from("file-transcription.log"));
        Self {
            app,
            engine,
            models_dir,
            db_path,
            statuses: Arc::new(Mutex::new(HashMap::new())),
            queued_requests: Arc::new(Mutex::new(VecDeque::new())),
            active_run: Arc::new(Mutex::new(None)),
            log_path,
        }
    }

    pub fn start(&self, request: FileTranscriptionRequest) -> StartFileTranscriptionResponse {
        let now = now_ms();
        let job_id = request.job_id.clone();
        let source_file = request.source_file.clone();
        let queued_status = FileTranscriptionStatusEvent {
            job_id: job_id.clone(),
            source_file,
            stage: FileTranscriptionJobStage::Queued,
            progress_percent: None,
            processed_ms: None,
            total_ms: None,
            eta_seconds: None,
            status_text: "Queued for local transcription.".to_string(),
            result: None,
            error_message: None,
            started_at_ms: now,
            updated_at_ms: now,
        };

        self.persist_status(queued_status);
        self.log_job(&job_id, "accepted", "Queued for local transcription.");
        self.queued_requests
            .lock()
            .expect("file transcription queue mutex poisoned")
            .push_back(request);
        self.maybe_spawn_next();

        StartFileTranscriptionResponse { job_id }
    }

    pub fn statuses(&self) -> Vec<FileTranscriptionStatusEvent> {
        let mut statuses = self
            .statuses
            .lock()
            .expect("file transcription status mutex poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        statuses.sort_by(|left, right| right.started_at_ms.cmp(&left.started_at_ms));
        statuses
    }

    pub fn cancel(&self, job_id: &str) -> Result<()> {
        let removed_from_queue = {
            let mut queue = self
                .queued_requests
                .lock()
                .expect("file transcription queue mutex poisoned");
            let original_len = queue.len();
            queue.retain(|request| request.job_id != job_id);
            queue.len() != original_len
        };

        let active_cancelled = {
            let active = self
                .active_run
                .lock()
                .expect("file transcription active mutex poisoned");
            if let Some(run) = active.as_ref() {
                if run.job_id == job_id {
                    run.cancelled.store(true, Ordering::SeqCst);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if removed_from_queue || active_cancelled {
            self.cancel_job(
                job_id,
                "File transcription canceled.",
                "The file transcription was canceled by the user.".to_string(),
            );
            if removed_from_queue && !active_cancelled {
                self.maybe_spawn_next();
            }
            Ok(())
        } else {
            Err(anyhow!(
                "No queued or active file transcription job matched {job_id}"
            ))
        }
    }

    fn maybe_spawn_next(&self) {
        {
            let active = self
                .active_run
                .lock()
                .expect("file transcription active mutex poisoned");
            if active.is_some() {
                return;
            }
        }

        let Some(request) = self
            .queued_requests
            .lock()
            .expect("file transcription queue mutex poisoned")
            .pop_front()
        else {
            return;
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        {
            let mut active = self
                .active_run
                .lock()
                .expect("file transcription active mutex poisoned");
            *active = Some(ActiveFileTranscriptionRun {
                job_id: request.job_id.clone(),
                cancelled: Arc::clone(&cancelled),
            });
        }

        let controller = self.clone();
        thread::spawn(move || {
            controller.run_job(request, cancelled);
        });
    }

    fn run_job(&self, request: FileTranscriptionRequest, cancelled: Arc<AtomicBool>) {
        let watchdog = self.spawn_watchdog(request.job_id.clone(), Arc::clone(&cancelled));

        let result = self.process_file_job(&request, Arc::clone(&cancelled));
        cancelled.store(true, Ordering::SeqCst);

        let _ = watchdog.join();

        if let Err(error) = result {
            self.fail_job(
                &request.job_id,
                "File transcription failed.",
                error.to_string(),
            );
        }

        self.finish_run(&request.job_id);
        self.maybe_spawn_next();
    }

    fn process_file_job(
        &self,
        request: &FileTranscriptionRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        self.update_status(
            &request.job_id,
            FileTranscriptionJobStage::Preparing,
            "Preparing audio file.",
            None,
            None,
            None,
            None,
            None,
            None,
        )?;
        self.log_job(
            &request.job_id,
            "preparing",
            &request.source_file.original_name,
        );

        let settings = storage::get_settings_from_db_path(&self.db_path)?;
        let vocabulary_prompt = vocabulary::build_asr_prompt_from_db_path(
            &self.db_path,
            settings.language_mode,
            settings.fixed_language.as_deref(),
        )?;
        if let Some(prompt) = &vocabulary_prompt {
            self.log_job(
                &request.job_id,
                "dictionary-prompt",
                &format!(
                    "included={} truncated={}",
                    prompt.included_count, prompt.truncated_count
                ),
            );
        }
        let resolved_model = resolve_model_for_settings(
            self.engine.as_ref(),
            &settings,
            PreviewSourceKind::FileUpload,
        )?;

        self.update_status(
            &request.job_id,
            FileTranscriptionJobStage::Transcribing,
            &format!(
                "Transcribing {} locally...",
                request.source_file.original_name
            ),
            Some(0.0),
            Some(0),
            request.source_file.duration_ms,
            None,
            None,
            None,
        )?;
        self.log_job(
            &request.job_id,
            "transcribing",
            &request.source_file.original_name,
        );

        let started_at = Instant::now();
        let transcript = self.run_transcription_worker(
            request,
            EngineFileTranscriptionRequest {
                profile: settings.file_transcribe_model_profile,
                selected_model_id: settings.file_transcribe_selected_model_id.clone(),
                language_mode: settings.language_mode,
                fixed_language: settings.fixed_language.clone(),
                timestamps: true,
                prefer_gpu: settings.gpu_enabled,
                file_path: request.source_file.file_path.clone(),
                context_prompt: vocabulary_prompt.as_ref().map(|prompt| prompt.text.clone()),
                context_terms: vocabulary_prompt
                    .as_ref()
                    .map(|prompt| prompt.terms.clone())
                    .unwrap_or_default(),
            },
            Arc::clone(&cancelled),
            started_at,
        )?;

        if cancelled.load(Ordering::SeqCst) {
            return Err(anyhow!(
                "File transcription watchdog expired before completion."
            ));
        }

        let mut corrected = vocabulary::correct_transcript_result(&self.db_path, transcript)?;
        if !corrected.warnings.is_empty() {
            self.log_job(
                &request.job_id,
                "recovery",
                &format!(
                    "quality={:?} recovered_regions={} warnings={}",
                    corrected.quality_status,
                    corrected.recovered_region_count,
                    corrected.warnings.len()
                ),
            );
        }
        if settings.file_diarization_enabled {
            self.update_status(
                &request.job_id,
                FileTranscriptionJobStage::Diarizing,
                "Identifying speakers locally...",
                None,
                request.source_file.duration_ms,
                request.source_file.duration_ms,
                None,
                None,
                None,
            )?;
            if let Some(package_path) =
                model_downloads::installed_diarization_package_path(&self.models_dir)
            {
                let worker_request = diarization_worker::WorkerRequest {
                    job_id: request.job_id.clone(),
                    audio_path: request.source_file.file_path.clone().into(),
                    package_path,
                    exact_speaker_count: None,
                    spec_version: diarization::DIARIZATION_MODEL_SPEC_V1.manifest_version,
                };
                match diarization_worker::run_subprocess_worker(
                    &worker_request,
                    Some(cancelled.as_ref()),
                ) {
                    Ok(turns) => diarization::apply_turns_to_transcript(&mut corrected, turns),
                    Err(error) if error.to_string().starts_with("DIARIZATION_CANCELED:") => {
                        return Err(error)
                    }
                    Err(error) => diarization::mark_failure(&mut corrected, error.to_string()),
                }
            } else {
                diarization::mark_failure(
                    &mut corrected,
                    "Speaker identification is enabled, but its model is still installing or unavailable. The transcript was saved without speaker labels.",
                );
            }
        }
        let wall_duration_ms = started_at.elapsed().as_millis() as i64;

        let completion_message = match corrected.quality_status {
            crate::asr::TranscriptQualityStatus::Clean => "Transcription completed.",
            crate::asr::TranscriptQualityStatus::Recovered => {
                "Transcription completed after decoder recovery."
            }
            crate::asr::TranscriptQualityStatus::Partial => {
                "Transcription completed with a section that needs review."
            }
        };
        self.update_status(
            &request.job_id,
            FileTranscriptionJobStage::Saving,
            "Saving transcript to local history...",
            Some(100.0),
            request.source_file.duration_ms,
            request.source_file.duration_ms,
            Some(0),
            None,
            None,
        )?;
        self.log_job(
            &request.job_id,
            "saving",
            &request.source_file.original_name,
        );

        let saved_transcript = if settings.save_history {
            Some(storage::save_file_transcription(
                &self.db_path,
                &request.source_file,
                &corrected,
            )?)
        } else {
            None
        };

        if let Some(model_id) =
            resolve_model_id_for_job(&self.db_path, PreviewSourceKind::FileUpload)
                .ok()
                .flatten()
        {
            if let Some(audio_duration_ms) = request.source_file.duration_ms {
                let audio_ms = audio_duration_ms.max(1) as f64;
                let wall_ms = wall_duration_ms.max(1) as f64;
                let _ = storage::record_file_transcription_performance(
                    &self.db_path,
                    &model_id,
                    audio_ms / wall_ms,
                );
            }
        }

        let response = FileTranscriptionResponse {
            source_file: request.source_file.clone(),
            resolved_model,
            result: corrected,
            saved_transcript,
        };

        self.update_status(
            &request.job_id,
            FileTranscriptionJobStage::Completed,
            completion_message,
            Some(100.0),
            request.source_file.duration_ms,
            request.source_file.duration_ms,
            Some(0),
            Some(response),
            None,
        )?;
        self.log_job(
            &request.job_id,
            "completed",
            &request.source_file.original_name,
        );
        Ok(())
    }

    fn run_transcription_worker(
        &self,
        request: &FileTranscriptionRequest,
        engine_request: EngineFileTranscriptionRequest,
        cancelled: Arc<AtomicBool>,
        started_at: Instant,
    ) -> Result<TranscriptResult> {
        let executable = std::env::current_exe()
            .map_err(|error| anyhow!("failed to resolve app executable: {error}"))?;
        let worker_request = WorkerRequest {
            models_dir: self.models_dir.clone(),
            request: engine_request,
        };
        let request_json = serde_json::to_vec(&worker_request)?;

        let mut child = Command::new(executable)
            .arg(transcription_worker::WORKER_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| anyhow!("failed to start transcription worker: {error}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&request_json)?;
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("transcription worker stdout was unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("transcription worker stderr was unavailable"))?;

        let (output_tx, output_rx) = mpsc::channel();
        let output_reader = transcription_worker::read_worker_output_lines(stdout, output_tx);
        let stderr_reader = thread::spawn(move || {
            let mut stderr = stderr;
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
            text
        });

        let mut last_progress = -1;
        let mut last_decoder_activity = Instant::now();

        loop {
            if cancelled.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = output_reader.join();
                let stderr_text = stderr_reader.join().unwrap_or_default();
                let detail = stderr_text.trim();
                return Err(anyhow!(
                    "File transcription watchdog expired before completion.{}",
                    if detail.is_empty() {
                        "".to_string()
                    } else {
                        format!(" Worker output: {detail}")
                    }
                ));
            }

            match output_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Ok(WorkerOutput::Progress { progress_percent })) => {
                    if progress_percent > last_progress {
                        last_progress = progress_percent;
                        last_decoder_activity = Instant::now();
                    }
                    self.update_worker_progress(request, progress_percent, started_at)?;
                }
                Ok(Ok(WorkerOutput::Heartbeat { progress_percent })) => {
                    if progress_percent > last_progress {
                        last_progress = progress_percent;
                        last_decoder_activity = Instant::now();
                    }
                    if last_decoder_activity.elapsed() > WORKER_STALL_TIMEOUT {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = output_reader.join();
                        let _ = stderr_reader.join();
                        return Err(anyhow!(
                            "File transcription worker stopped advancing for ten minutes."
                        ));
                    }
                    self.update_worker_progress(request, progress_percent.max(0), started_at)?;
                }
                Ok(Ok(WorkerOutput::Result { result })) => {
                    let _ = child.wait();
                    let _ = output_reader.join();
                    let _ = stderr_reader.join();
                    return Ok(result);
                }
                Ok(Ok(WorkerOutput::Error { message })) => {
                    let _ = child.wait();
                    let _ = output_reader.join();
                    let _ = stderr_reader.join();
                    return Err(anyhow!(message));
                }
                Ok(Err(error)) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = output_reader.join();
                    let _ = stderr_reader.join();
                    return Err(anyhow!(
                        "transcription worker emitted invalid output: {error}"
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(status) = child.try_wait()? {
                        let _ = output_reader.join();
                        let stderr_text = stderr_reader.join().unwrap_or_default();
                        return Err(anyhow!(
                            "transcription worker exited before returning a result (status: {status}). {}",
                            stderr_text.trim()
                        ));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(status) = child.try_wait()? {
                        let stderr_text = stderr_reader.join().unwrap_or_default();
                        return Err(anyhow!(
                            "transcription worker output closed before returning a result (status: {status}). {}",
                            stderr_text.trim()
                        ));
                    }
                }
            }
        }
    }

    fn update_worker_progress(
        &self,
        request: &FileTranscriptionRequest,
        raw_progress_percent: i32,
        started_at: Instant,
    ) -> Result<()> {
        let pct = (raw_progress_percent as f32).clamp(0.0, 100.0);
        let processed_ms = request
            .source_file
            .duration_ms
            .map(|total_ms| (total_ms as f64 * pct as f64 / 100.0) as i64);
        let eta_seconds = if pct > 0.0 {
            let elapsed_secs = started_at.elapsed().as_secs_f64();
            let remaining_pct = 100.0 - pct as f64;
            Some((elapsed_secs * remaining_pct / pct as f64).ceil() as i64)
        } else {
            None
        };

        self.update_status(
            &request.job_id,
            FileTranscriptionJobStage::Transcribing,
            &format!(
                "Transcribing {} locally...",
                request.source_file.original_name
            ),
            Some(pct),
            processed_ms,
            request.source_file.duration_ms,
            eta_seconds,
            None,
            None,
        )
    }

    fn spawn_watchdog(&self, job_id: String, cancelled: Arc<AtomicBool>) -> thread::JoinHandle<()> {
        let controller = self.clone();
        thread::spawn(move || loop {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(1500));
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            let Some(status) = controller.current_status(&job_id) else {
                break;
            };
            if is_terminal(&status.stage) {
                break;
            }
            if now_ms() - status.updated_at_ms > WATCHDOG_TIMEOUT_MS {
                cancelled.store(true, Ordering::SeqCst);
                controller.fail_job(
                    &job_id,
                    "File transcription timed out.",
                    "The file job stopped reporting progress and was cancelled.".to_string(),
                );
                controller.log_job(&job_id, "failed", "watchdog timeout");
                break;
            }
        })
    }

    fn update_status(
        &self,
        job_id: &str,
        stage: FileTranscriptionJobStage,
        status_text: &str,
        progress_percent: Option<f32>,
        processed_ms: Option<i64>,
        total_ms: Option<i64>,
        eta_seconds: Option<i64>,
        result: Option<FileTranscriptionResponse>,
        error_message: Option<String>,
    ) -> Result<()> {
        let current = self
            .current_status(job_id)
            .ok_or_else(|| anyhow!("Missing file transcription status for job {}", job_id))?;
        let next = FileTranscriptionStatusEvent {
            job_id: current.job_id.clone(),
            source_file: current.source_file.clone(),
            stage,
            progress_percent,
            processed_ms,
            total_ms,
            eta_seconds,
            status_text: status_text.to_string(),
            result,
            error_message,
            started_at_ms: current.started_at_ms,
            updated_at_ms: now_ms(),
        };
        self.persist_status(next);
        Ok(())
    }

    fn fail_job(&self, job_id: &str, status_text: &str, error_message: String) {
        if let Some(current) = self.current_status(job_id) {
            if is_terminal(&current.stage) {
                return;
            }
        }

        let _ = self.update_status(
            job_id,
            FileTranscriptionJobStage::Failed,
            status_text,
            None,
            None,
            None,
            None,
            None,
            Some(error_message.clone()),
        );
        self.log_job(job_id, "failed", &error_message);
    }

    fn cancel_job(&self, job_id: &str, status_text: &str, error_message: String) {
        if self
            .current_status(job_id)
            .is_some_and(|status| is_terminal(&status.stage))
        {
            return;
        }
        let _ = self.update_status(
            job_id,
            FileTranscriptionJobStage::Canceled,
            status_text,
            None,
            None,
            None,
            None,
            None,
            Some(error_message.clone()),
        );
        self.log_job(job_id, "canceled", &error_message);
    }

    fn persist_status(&self, status: FileTranscriptionStatusEvent) {
        {
            let mut statuses = self
                .statuses
                .lock()
                .expect("file transcription status mutex poisoned");
            statuses.insert(status.job_id.clone(), status.clone());
        }

        if let Err(error) = self
            .app
            .emit(FILE_TRANSCRIPTION_STATUS_EVENT, status.clone())
        {
            self.log_job(&status.job_id, "emit_error", &error.to_string());
        }
    }

    fn current_status(&self, job_id: &str) -> Option<FileTranscriptionStatusEvent> {
        self.statuses
            .lock()
            .expect("file transcription status mutex poisoned")
            .get(job_id)
            .cloned()
    }

    fn finish_run(&self, job_id: &str) {
        {
            let mut active = self
                .active_run
                .lock()
                .expect("file transcription active mutex poisoned");
            if let Some(run) = active.as_ref() {
                if run.job_id == job_id {
                    run.cancelled.store(true, Ordering::SeqCst);
                }
            }
            *active = None;
        }
    }

    fn log_job(&self, job_id: &str, stage: &str, message: &str) {
        let line = format!("[{}] {} {} {}\n", now_ms(), job_id, stage, message);
        eprintln!("[file-job] {}", line.trim_end());
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

fn resolve_model_for_settings(
    engine: &dyn TranscriptionEngine,
    settings: &AppSettings,
    source_kind: PreviewSourceKind,
) -> Result<Option<InstalledModel>> {
    let models = engine.list_models()?;
    let (selected_model_id, profile) = match source_kind {
        PreviewSourceKind::QuickDictate => (
            settings.quick_dictate_selected_model_id.as_deref(),
            settings.quick_dictate_model_profile,
        ),
        PreviewSourceKind::FileUpload => (
            settings.file_transcribe_selected_model_id.as_deref(),
            settings.file_transcribe_model_profile,
        ),
    };

    Ok(if let Some(model_id) = selected_model_id {
        models.iter().find(|model| model.id == model_id).cloned()
    } else {
        models
            .iter()
            .find(|model| model.profile == profile && model.is_default)
            .or_else(|| models.iter().find(|model| model.profile == profile))
            .cloned()
    })
}

fn resolve_model_id_for_job(
    db_path: &Path,
    source_kind: PreviewSourceKind,
) -> Result<Option<String>> {
    let settings = storage::get_settings_from_db_path(db_path)?;
    Ok(match source_kind {
        PreviewSourceKind::QuickDictate => settings.quick_dictate_selected_model_id,
        PreviewSourceKind::FileUpload => settings.file_transcribe_selected_model_id,
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn is_terminal(stage: &FileTranscriptionJobStage) -> bool {
    matches!(
        stage,
        FileTranscriptionJobStage::Completed
            | FileTranscriptionJobStage::Failed
            | FileTranscriptionJobStage::Canceled
    )
}
