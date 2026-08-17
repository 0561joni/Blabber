use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::{Client, Response};
use reqwest::header::RANGE;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::asr::{discover_installed_models, LocalTranscriptionEngine, TranscriptionEngine};
use crate::diarization::DIARIZATION_MODEL_ID;
#[cfg(test)]
use crate::qwen_asr::QWEN_REQUIRED_ARTIFACTS;
use crate::qwen_asr::{
    self, QWEN_MODEL_DIR, QWEN_MODEL_ID, QWEN_MODEL_NAME, QWEN_MODEL_REVISION, QWEN_TOTAL_SIZE,
};
use crate::settings::ModelProfile;
use crate::storage;

const MODEL_DOWNLOAD_EVENT: &str = "model-download-status";
pub const VAD_MODEL_NAME: &str = "ggml-silero-v6.2.0.bin";
pub const DIARIZATION_MODEL_DIR: &str = "sherpa-diarization-pyannote3-eres2net-voxceleb-v2";
/// Shared completion marker for every directory-based model package.
/// Keep the historical filename so existing Qwen installations remain valid.
pub const MODEL_COMPLETE_FILE: &str = ".blabber-model.json";
pub const DIARIZATION_ARTIFACTS_REVIEWED: bool = true;
pub const DIARIZATION_TOTAL_SIZE: i64 = 32_478_041;
pub const DIARIZATION_REVISION: &str = "segmentation@340b52f1f5cd12d45a30fa284691417eaad2ff92+embedding@8be2a75c9ed7a590538b268e46fbb65e1aa9d208";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailability {
    Available,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    Asr,
    Vad,
    Diarization,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadableModel {
    pub id: String,
    pub engine: String,
    pub model_name: String,
    pub description: String,
    pub size_bytes: i64,
    pub profile: ModelProfile,
    pub availability: ModelAvailability,
    pub requirements: Option<String>,
    pub availability_reason: Option<String>,
    pub installed: bool,
    pub artifact_count: u32,
    pub capability: ModelCapability,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadState {
    Idle,
    Downloading,
    Completed,
    Canceled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDownloadStatus {
    pub model_id: String,
    pub model_name: String,
    pub state: ModelDownloadState,
    pub downloaded_bytes: i64,
    pub total_bytes: Option<i64>,
    pub progress_percent: Option<f32>,
    pub error_message: Option<String>,
    pub current_artifact: Option<String>,
    pub artifact_index: Option<u32>,
    pub artifact_count: u32,
}

#[derive(Clone)]
pub struct ModelDownloadManager {
    app: AppHandle,
    models_dir: PathBuf,
    db_path: PathBuf,
    engine: Arc<LocalTranscriptionEngine>,
    statuses: Arc<Mutex<HashMap<String, ModelDownloadStatus>>>,
    active_download: Arc<Mutex<Option<String>>>,
    cancellation_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl ModelDownloadManager {
    pub fn new(
        app: AppHandle,
        models_dir: PathBuf,
        db_path: PathBuf,
        engine: Arc<LocalTranscriptionEngine>,
    ) -> Self {
        let statuses = downloadable_specs()
            .into_iter()
            .map(|spec| (spec.id.to_string(), idle_status(&spec)))
            .collect();
        Self {
            app,
            models_dir,
            db_path,
            engine,
            statuses: Arc::new(Mutex::new(statuses)),
            active_download: Arc::new(Mutex::new(None)),
            cancellation_flags: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn statuses(&self) -> Vec<ModelDownloadStatus> {
        let mut values = self
            .statuses
            .lock()
            .map(|statuses| statuses.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        values.sort_by(|left, right| left.model_name.cmp(&right.model_name));
        values
    }

    pub fn start_download(&self, model_id: &str) -> Result<ModelDownloadStatus> {
        let spec = downloadable_specs()
            .into_iter()
            .find(|entry| entry.id == model_id)
            .ok_or_else(|| anyhow!("Unknown model download: {model_id}"))?;
        match spec.availability() {
            ModelAvailability::Available => {}
            ModelAvailability::UnsupportedPlatform => {
                return Err(anyhow!(
                    "MODEL_UNSUPPORTED_PLATFORM: {} is not available on this platform",
                    spec.model_name
                ))
            }
        }

        let mut active = self
            .active_download
            .lock()
            .map_err(|_| anyhow!("download state is unavailable"))?;
        if let Some(current) = &*active {
            if current == model_id {
                return self
                    .statuses
                    .lock()
                    .ok()
                    .and_then(|statuses| statuses.get(model_id).cloned())
                    .ok_or_else(|| anyhow!("Model download state is unavailable."));
            }
            return Err(anyhow!("Another model download is already in progress."));
        }
        *active = Some(model_id.to_string());
        drop(active);

        let cancelled = Arc::new(AtomicBool::new(false));
        self.cancellation_flags
            .lock()
            .map_err(|_| anyhow!("download cancellation state is unavailable"))?
            .insert(model_id.to_string(), Arc::clone(&cancelled));

        let initial = downloading_status(&spec, 0, None, None);
        self.update_status(initial.clone());

        let app = self.app.clone();
        let models_dir = self.models_dir.clone();
        let db_path = self.db_path.clone();
        let engine = Arc::clone(&self.engine);
        let statuses = Arc::clone(&self.statuses);
        let active_download = Arc::clone(&self.active_download);
        let cancellation_flags = Arc::clone(&self.cancellation_flags);
        thread::spawn(move || {
            let result = download_model(
                &spec,
                &models_dir,
                &cancelled,
                |downloaded, artifact, index| {
                    persist_status(
                        &app,
                        &statuses,
                        downloading_status(&spec, downloaded, Some(artifact), Some(index)),
                    );
                },
            );

            let status = match result {
                Ok(()) => {
                    match engine.refresh_from_disk() {
                        Ok(models) => {
                            let _ = storage::sync_installed_models_for_db_path(&db_path, &models);
                            let _ = storage::apply_preferred_model_defaults_for_db_path(
                                &db_path, &models,
                            );
                        }
                        Err(_) => {
                            if let Ok(models) = discover_installed_models(&models_dir) {
                                let _ =
                                    storage::sync_installed_models_for_db_path(&db_path, &models);
                            }
                        }
                    }
                    ModelDownloadStatus {
                        model_id: spec.id.to_string(),
                        model_name: spec.model_name.to_string(),
                        state: ModelDownloadState::Completed,
                        downloaded_bytes: spec.size_bytes,
                        total_bytes: Some(spec.size_bytes),
                        progress_percent: Some(100.0),
                        error_message: None,
                        current_artifact: None,
                        artifact_index: Some(spec.artifacts.len() as u32),
                        artifact_count: spec.artifacts.len() as u32,
                    }
                }
                Err(error) if is_cancelled_error(&error) => {
                    let downloaded_bytes = statuses
                        .lock()
                        .ok()
                        .and_then(|values| {
                            values.get(spec.id).map(|status| status.downloaded_bytes)
                        })
                        .unwrap_or(0);
                    ModelDownloadStatus {
                        model_id: spec.id.to_string(),
                        model_name: spec.model_name.to_string(),
                        state: ModelDownloadState::Canceled,
                        downloaded_bytes,
                        total_bytes: Some(spec.size_bytes),
                        progress_percent: Some(
                            (downloaded_bytes as f64 / spec.size_bytes.max(1) as f64 * 100.0)
                                as f32,
                        ),
                        error_message: None,
                        current_artifact: None,
                        artifact_index: None,
                        artifact_count: spec.artifacts.len() as u32,
                    }
                }
                Err(error) => ModelDownloadStatus {
                    model_id: spec.id.to_string(),
                    model_name: spec.model_name.to_string(),
                    state: ModelDownloadState::Failed,
                    downloaded_bytes: 0,
                    total_bytes: Some(spec.size_bytes),
                    progress_percent: None,
                    error_message: Some(error.to_string()),
                    current_artifact: None,
                    artifact_index: None,
                    artifact_count: spec.artifacts.len() as u32,
                },
            };
            persist_status(&app, &statuses, status);
            if let Ok(mut current) = active_download.lock() {
                *current = None;
            }
            if let Ok(mut flags) = cancellation_flags.lock() {
                flags.remove(spec.id);
            }
        });
        Ok(initial)
    }

    pub fn cancel_download(&self, model_id: &str) -> Result<ModelDownloadStatus> {
        let flag = self
            .cancellation_flags
            .lock()
            .map_err(|_| anyhow!("download cancellation state is unavailable"))?
            .get(model_id)
            .cloned()
            .ok_or_else(|| anyhow!("No active download exists for {model_id}."))?;
        flag.store(true, Ordering::Relaxed);
        self.statuses
            .lock()
            .map_err(|_| anyhow!("download state is unavailable"))?
            .get(model_id)
            .cloned()
            .ok_or_else(|| anyhow!("Model download state is unavailable."))
    }

    fn update_status(&self, status: ModelDownloadStatus) {
        persist_status(&self.app, &self.statuses, status);
    }
}

fn idle_status(spec: &DownloadableModelSpec) -> ModelDownloadStatus {
    ModelDownloadStatus {
        model_id: spec.id.to_string(),
        model_name: spec.model_name.to_string(),
        state: ModelDownloadState::Idle,
        downloaded_bytes: 0,
        total_bytes: Some(spec.size_bytes),
        progress_percent: Some(0.0),
        error_message: None,
        current_artifact: None,
        artifact_index: None,
        artifact_count: spec.artifacts.len() as u32,
    }
}

fn downloading_status(
    spec: &DownloadableModelSpec,
    downloaded_bytes: i64,
    artifact: Option<&str>,
    artifact_index: Option<u32>,
) -> ModelDownloadStatus {
    ModelDownloadStatus {
        model_id: spec.id.to_string(),
        model_name: spec.model_name.to_string(),
        state: ModelDownloadState::Downloading,
        downloaded_bytes,
        total_bytes: Some(spec.size_bytes),
        progress_percent: Some(
            (downloaded_bytes as f64 / spec.size_bytes.max(1) as f64 * 100.0) as f32,
        ),
        error_message: None,
        current_artifact: artifact.map(ToString::to_string),
        artifact_index,
        artifact_count: spec.artifacts.len() as u32,
    }
}

fn persist_status(
    app: &AppHandle,
    statuses: &Arc<Mutex<HashMap<String, ModelDownloadStatus>>>,
    status: ModelDownloadStatus,
) {
    if let Ok(mut map) = statuses.lock() {
        map.insert(status.model_id.clone(), status.clone());
    }
    let _ = app.emit(MODEL_DOWNLOAD_EVENT, status);
}

fn download_model(
    spec: &DownloadableModelSpec,
    models_dir: &Path,
    cancelled: &AtomicBool,
    mut on_progress: impl FnMut(i64, &str, u32),
) -> Result<()> {
    ensure_not_cancelled(cancelled)?;
    fs::create_dir_all(models_dir)?;
    match spec.layout {
        InstallLayout::File { file_name } => {
            let artifact = spec.artifacts.first().expect("file model artifact");
            let final_path = models_dir.join(file_name);
            if final_path.is_file() && verify_file(&final_path, artifact)? {
                return Ok(());
            }
            let temp_path = models_dir.join(format!("{file_name}.part"));
            download_artifact(artifact, &temp_path, 0, cancelled, &mut on_progress, 1)?;
            fs::rename(temp_path, final_path)?;
        }
        InstallLayout::Directory { directory_name } => {
            if model_is_installed(spec, models_dir) {
                return Ok(());
            }
            let final_dir = models_dir.join(directory_name);
            let staging_dir = models_dir.join(format!(".{directory_name}.part"));
            fs::create_dir_all(&staging_dir)?;
            let mut completed_bytes = 0_i64;
            for (index, artifact) in spec.artifacts.iter().enumerate() {
                ensure_not_cancelled(cancelled)?;
                let final_artifact = staging_dir.join(artifact.path);
                if final_artifact.is_file() && verify_file(&final_artifact, artifact)? {
                    completed_bytes += artifact.size_bytes;
                    on_progress(completed_bytes, artifact.path, index as u32 + 1);
                    continue;
                }
                if final_artifact.exists() {
                    fs::remove_file(&final_artifact)?;
                }
                let partial = staging_dir.join(format!("{}.part", artifact.path));
                download_artifact(
                    artifact,
                    &partial,
                    completed_bytes,
                    cancelled,
                    &mut on_progress,
                    index as u32 + 1,
                )?;
                fs::rename(partial, final_artifact)?;
                completed_bytes += artifact.size_bytes;
            }
            write_completion_manifest(spec, &staging_dir)?;
            if final_dir.exists() {
                fs::remove_dir_all(&final_dir)?;
            }
            fs::rename(staging_dir, final_dir)?;
        }
    }
    Ok(())
}

fn download_artifact(
    artifact: &ModelArtifactSpec,
    partial_path: &Path,
    completed_before: i64,
    cancelled: &AtomicBool,
    on_progress: &mut impl FnMut(i64, &str, u32),
    artifact_index: u32,
) -> Result<()> {
    ensure_not_cancelled(cancelled)?;
    let client = Client::builder().build()?;
    let mut existing = partial_path
        .metadata()
        .map(|value| value.len())
        .unwrap_or(0);
    if existing > artifact.size_bytes as u64 {
        fs::remove_file(partial_path)?;
        existing = 0;
    }

    let mut response = send_download_request(&client, artifact.url, existing)?;
    if existing > 0 && response.status() != StatusCode::PARTIAL_CONTENT {
        fs::remove_file(partial_path)?;
        existing = 0;
        response = send_download_request(&client, artifact.url, 0)?;
    }
    response = response
        .error_for_status()
        .with_context(|| format!("download failed for {}", artifact.path))?;

    let mut hasher = Sha256::new();
    if existing > 0 {
        let mut current = File::open(partial_path)?;
        let mut buffer = [0_u8; 256 * 1024];
        loop {
            let read = current.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(partial_path)?;
    if existing == 0 {
        file.set_len(0)?;
    }
    file.seek(SeekFrom::Start(existing))?;

    let mut downloaded = existing as i64;
    let mut buffer = [0_u8; 256 * 1024];
    loop {
        ensure_not_cancelled(cancelled)?;
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        downloaded += read as i64;
        on_progress(completed_before + downloaded, artifact.path, artifact_index);
    }
    file.flush()?;
    if downloaded != artifact.size_bytes {
        return Err(anyhow!(
            "Size mismatch for {}: expected {} bytes but received {}.",
            artifact.path,
            artifact.size_bytes,
            downloaded
        ));
    }
    let computed_hash = format!("{:x}", hasher.finalize());
    if computed_hash != artifact.sha256 {
        let _ = fs::remove_file(partial_path);
        return Err(anyhow!(
            "Checksum mismatch for {}. The partial download was removed; retry the model download.",
            artifact.path
        ));
    }
    Ok(())
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(anyhow!("MODEL_DOWNLOAD_CANCELED: download canceled"))
    } else {
        Ok(())
    }
}

fn is_cancelled_error(error: &anyhow::Error) -> bool {
    error.to_string().starts_with("MODEL_DOWNLOAD_CANCELED:")
}

fn send_download_request(client: &Client, url: &str, start: u64) -> Result<Response> {
    let mut request = client.get(url);
    if start > 0 {
        request = request.header(RANGE, format!("bytes={start}-"));
    }
    request
        .send()
        .with_context(|| format!("failed to download {url}"))
}

fn verify_file(path: &Path, artifact: &ModelArtifactSpec) -> Result<bool> {
    if path.metadata().map(|value| value.len() as i64).ok() != Some(artifact.size_bytes) {
        return Ok(false);
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()) == artifact.sha256)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompletionManifest {
    model_id: String,
    revision: String,
    artifacts: Vec<CompletionArtifact>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompletionArtifact {
    path: String,
    size_bytes: i64,
    sha256: String,
}

fn write_completion_manifest(spec: &DownloadableModelSpec, directory: &Path) -> Result<()> {
    let manifest = CompletionManifest {
        model_id: spec.id.to_string(),
        revision: spec.revision.unwrap_or("").to_string(),
        artifacts: spec
            .artifacts
            .iter()
            .map(|artifact| CompletionArtifact {
                path: artifact.path.to_string(),
                size_bytes: artifact.size_bytes,
                sha256: artifact.sha256.to_string(),
            })
            .collect(),
    };
    let partial = directory.join(format!("{MODEL_COMPLETE_FILE}.part"));
    let final_path = directory.join(MODEL_COMPLETE_FILE);
    let mut file = File::create(&partial)?;
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.write_all(b"\n")?;
    file.flush()?;
    fs::rename(partial, final_path)?;
    Ok(())
}

pub fn start_background_vad_download(models_dir: PathBuf) {
    let final_path = models_dir.join(VAD_MODEL_NAME);
    if final_path.exists() {
        return;
    }
    thread::spawn(move || {
        let spec = vad_model_spec();
        if let Err(error) =
            download_model(&spec, &models_dir, &AtomicBool::new(false), |_, _, _| {})
        {
            eprintln!(
                "[vad-model] download unavailable; using silence-based chunk boundaries: {error:#}"
            );
        }
    });
}

pub fn installed_vad_model_path(models_dir: &Path) -> Option<PathBuf> {
    let path = models_dir.join(VAD_MODEL_NAME);
    path.is_file().then_some(path)
}

#[derive(Clone, Copy)]
struct ModelArtifactSpec {
    path: &'static str,
    size_bytes: i64,
    url: &'static str,
    sha256: &'static str,
}

#[derive(Clone, Copy)]
enum InstallLayout {
    File { file_name: &'static str },
    Directory { directory_name: &'static str },
}

#[derive(Clone)]
struct DownloadableModelSpec {
    id: &'static str,
    engine: &'static str,
    model_name: &'static str,
    description: &'static str,
    requirements: Option<&'static str>,
    size_bytes: i64,
    profile: ModelProfile,
    layout: InstallLayout,
    revision: Option<&'static str>,
    artifacts: &'static [ModelArtifactSpec],
    qwen_platform_limited: bool,
    capability: ModelCapability,
}

impl DownloadableModelSpec {
    fn availability(&self) -> ModelAvailability {
        if self.qwen_platform_limited && !qwen_asr::platform_supported() {
            ModelAvailability::UnsupportedPlatform
        } else {
            ModelAvailability::Available
        }
    }
}

macro_rules! single_file_artifact {
    ($name:literal, $size:literal, $url:literal, $sha:literal) => {
        &[ModelArtifactSpec {
            path: $name,
            size_bytes: $size,
            url: $url,
            sha256: $sha,
        }]
    };
}

const QWEN_ARTIFACTS: &[ModelArtifactSpec] = &[
    ModelArtifactSpec { path: "config.json", size_bytes: 6_194, url: "https://huggingface.co/Qwen/Qwen3-ASR-1.7B/resolve/b188e100bd85038c06d2812d24a39776eba774ca/config.json?download=true", sha256: "2e74a751548b8ad7d7526d29365ad8144c345d8b412b1152d25dc6698452712f" },
    ModelArtifactSpec { path: "generation_config.json", size_bytes: 142, url: "https://huggingface.co/Qwen/Qwen3-ASR-1.7B/resolve/b188e100bd85038c06d2812d24a39776eba774ca/generation_config.json?download=true", sha256: "1da527824d81e07118facff437e03f2e24a23311e3bdeb2368973fe77e5f275c" },
    ModelArtifactSpec { path: "model.safetensors.index.json", size_bytes: 64_821, url: "https://huggingface.co/Qwen/Qwen3-ASR-1.7B/resolve/b188e100bd85038c06d2812d24a39776eba774ca/model.safetensors.index.json?download=true", sha256: "f994739fe38e5210b9e3e8ce6c6307315e2ceac3cb630e7b7414d69dce520f60" },
    ModelArtifactSpec { path: "model-00001-of-00002.safetensors", size_bytes: 4_220_320_824, url: "https://huggingface.co/Qwen/Qwen3-ASR-1.7B/resolve/b188e100bd85038c06d2812d24a39776eba774ca/model-00001-of-00002.safetensors?download=true", sha256: "a4cd1f1a04d90b757dc7f7dd26254e69a013b19e80efe590a83c6a3bde8608d6" },
    ModelArtifactSpec { path: "model-00002-of-00002.safetensors", size_bytes: 478_200_688, url: "https://huggingface.co/Qwen/Qwen3-ASR-1.7B/resolve/b188e100bd85038c06d2812d24a39776eba774ca/model-00002-of-00002.safetensors?download=true", sha256: "6e0b9d9e09e2e0238e7ef3cc8a484ab387e91b90f1900bedf88bc92d7929ccfc" },
    ModelArtifactSpec { path: "vocab.json", size_bytes: 2_776_833, url: "https://huggingface.co/Qwen/Qwen3-ASR-1.7B/resolve/b188e100bd85038c06d2812d24a39776eba774ca/vocab.json?download=true", sha256: "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910" },
    ModelArtifactSpec { path: "merges.txt", size_bytes: 1_671_853, url: "https://huggingface.co/Qwen/Qwen3-ASR-1.7B/resolve/b188e100bd85038c06d2812d24a39776eba774ca/merges.txt?download=true", sha256: "8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5" },
];

const DIARIZATION_ARTIFACTS: &[ModelArtifactSpec] = &[
    ModelArtifactSpec {
        path: "segmentation.onnx",
        size_bytes: 5_992_778,
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/340b52f1f5cd12d45a30fa284691417eaad2ff92/model.onnx?download=true",
        sha256: "fed22097bca974bad329a930b60865703766ff89f05fa09060bf6fd44e92e319",
    },
    ModelArtifactSpec {
        path: "embedding.onnx",
        size_bytes: 26_485_263,
        url: "https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/8be2a75c9ed7a590538b268e46fbb65e1aa9d208/3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx?download=true",
        sha256: "c59158379255ad66e161679cca6af8d52d51e389e3224ab7d7a7baae295c2db5",
    },
];

fn downloadable_specs() -> Vec<DownloadableModelSpec> {
    vec![
        whisper_spec("ggml-tiny-bin", "ggml-tiny.bin", "Smallest local model for quick tests and lightweight dictation.", 77_691_713, ModelProfile::Fast, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin?download=true", "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
        whisper_spec("ggml-small-bin", "ggml-small.bin", "Good balance when you want lower memory use with better quality than tiny.", 487_601_967, ModelProfile::Balanced, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin?download=true", "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
        whisper_spec("ggml-medium-bin", "ggml-medium.bin", "Strong default for shortcut dictation when you want better accuracy.", 1_533_763_059, ModelProfile::Balanced, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin?download=true", "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"),
        whisper_spec("ggml-large-v3-turbo-bin", "ggml-large-v3-turbo.bin", "Best full-size turbo model when you want top quality and speed.", 1_624_555_275, ModelProfile::Accurate, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin?download=true", "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
        whisper_spec("ggml-large-v3-turbo-q5_0-bin", "ggml-large-v3-turbo-q5_0.bin", "Quantized turbo model with lower memory use and a strong quality-speed tradeoff.", 574_041_195, ModelProfile::Accurate, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin?download=true", "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"),
        DownloadableModelSpec {
            id: QWEN_MODEL_ID,
            engine: "qwen3_asr_c",
            model_name: QWEN_MODEL_NAME,
            description: "High-quality multilingual and code-switch transcription with dictionary-aware spelling prompts. CPU-only.",
            requirements: Some("macOS or Linux · 16 GB RAM recommended · CPU-only"),
            size_bytes: QWEN_TOTAL_SIZE,
            profile: ModelProfile::Accurate,
            layout: InstallLayout::Directory { directory_name: QWEN_MODEL_DIR },
            revision: Some(QWEN_MODEL_REVISION),
            artifacts: QWEN_ARTIFACTS,
            qwen_platform_limited: true,
            capability: ModelCapability::Asr,
        },
        DownloadableModelSpec {
            id: DIARIZATION_MODEL_ID,
            engine: "sherpa-onnx",
            model_name: "Offline speaker diarization",
            description: "Local speaker separation using pyannote segmentation and VoxCeleb ERes2Net embeddings.",
            requirements: Some("CPU-only · approximately 32 MB download"),
            size_bytes: DIARIZATION_TOTAL_SIZE,
            profile: ModelProfile::Balanced,
            layout: InstallLayout::Directory { directory_name: DIARIZATION_MODEL_DIR },
            revision: Some(DIARIZATION_REVISION),
            artifacts: DIARIZATION_ARTIFACTS,
            qwen_platform_limited: false,
            capability: ModelCapability::Diarization,
        },
    ]
}

fn whisper_spec(
    id: &'static str,
    file_name: &'static str,
    description: &'static str,
    size_bytes: i64,
    profile: ModelProfile,
    url: &'static str,
    sha256: &'static str,
) -> DownloadableModelSpec {
    let artifacts: &'static [ModelArtifactSpec] = match id {
        "ggml-tiny-bin" => single_file_artifact!("ggml-tiny.bin", 77_691_713, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin?download=true", "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
        "ggml-small-bin" => single_file_artifact!("ggml-small.bin", 487_601_967, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin?download=true", "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
        "ggml-medium-bin" => single_file_artifact!("ggml-medium.bin", 1_533_763_059, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin?download=true", "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"),
        "ggml-large-v3-turbo-bin" => single_file_artifact!("ggml-large-v3-turbo.bin", 1_624_555_275, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin?download=true", "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
        _ => single_file_artifact!("ggml-large-v3-turbo-q5_0.bin", 574_041_195, "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin?download=true", "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"),
    };
    debug_assert_eq!(artifacts[0].path, file_name);
    debug_assert_eq!(artifacts[0].url, url);
    debug_assert_eq!(artifacts[0].sha256, sha256);
    DownloadableModelSpec {
        id,
        engine: "whisper.cpp",
        model_name: file_name,
        description,
        requirements: None,
        size_bytes,
        profile,
        layout: InstallLayout::File { file_name },
        revision: None,
        artifacts,
        qwen_platform_limited: false,
        capability: ModelCapability::Asr,
    }
}

fn vad_model_spec() -> DownloadableModelSpec {
    DownloadableModelSpec {
        id: "internal-silero-v6-2-0",
        engine: "whisper.cpp",
        model_name: VAD_MODEL_NAME,
        description: "Internal voice-activity model for safe long-recording boundaries.",
        requirements: None,
        size_bytes: 885_098,
        profile: ModelProfile::Fast,
        layout: InstallLayout::File {
            file_name: VAD_MODEL_NAME,
        },
        revision: None,
        artifacts: single_file_artifact!(
            "ggml-silero-v6.2.0.bin",
            885_098,
            "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin",
            "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987"
        ),
        qwen_platform_limited: false,
        capability: ModelCapability::Vad,
    }
}

pub fn list_downloadable_models(models_dir: Option<&Path>) -> Vec<DownloadableModel> {
    downloadable_specs()
        .into_iter()
        .map(|spec| DownloadableModel {
            id: spec.id.to_string(),
            engine: spec.engine.to_string(),
            model_name: spec.model_name.to_string(),
            description: spec.description.to_string(),
            size_bytes: spec.size_bytes,
            profile: spec.profile,
            availability: spec.availability(),
            requirements: spec.requirements.map(ToString::to_string),
            availability_reason: match spec.availability() {
                ModelAvailability::UnsupportedPlatform => {
                    Some("This runtime is not supported on the current platform.".to_string())
                }
                ModelAvailability::Available => None,
            },
            installed: models_dir.is_some_and(|dir| model_is_installed(&spec, dir)),
            artifact_count: spec.artifacts.len() as u32,
            capability: spec.capability,
        })
        .collect()
}

fn model_is_installed(spec: &DownloadableModelSpec, models_dir: &Path) -> bool {
    match spec.layout {
        InstallLayout::File { file_name } => models_dir.join(file_name).is_file(),
        InstallLayout::Directory { directory_name } => {
            let directory = models_dir.join(directory_name);
            let manifest = fs::read_to_string(directory.join(MODEL_COMPLETE_FILE))
                .ok()
                .and_then(|contents| serde_json::from_str::<CompletionManifest>(&contents).ok());
            let Some(manifest) = manifest else {
                return false;
            };
            manifest.model_id == spec.id
                && manifest.revision == spec.revision.unwrap_or("")
                && manifest.artifacts.len() == spec.artifacts.len()
                && spec.artifacts.iter().all(|artifact| {
                    manifest.artifacts.iter().any(|installed| {
                        installed.path == artifact.path
                            && installed.size_bytes == artifact.size_bytes
                            && installed.sha256 == artifact.sha256
                    }) && directory
                        .join(artifact.path)
                        .metadata()
                        .map(|metadata| {
                            metadata.is_file() && metadata.len() as i64 == artifact.size_bytes
                        })
                        .unwrap_or(false)
                })
        }
    }
}

pub fn installed_diarization_package_path(models_dir: &Path) -> Option<PathBuf> {
    if !DIARIZATION_ARTIFACTS_REVIEWED {
        return None;
    }
    let spec = downloadable_specs()
        .into_iter()
        .find(|spec| spec.id == DIARIZATION_MODEL_ID)?;
    model_is_installed(&spec, models_dir).then(|| models_dir.join(DIARIZATION_MODEL_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_HASH: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn test_directory_spec(
        id: &'static str,
        directory_name: &'static str,
        url: &'static str,
    ) -> DownloadableModelSpec {
        let artifacts = Box::leak(
            vec![ModelArtifactSpec {
                path: "model.bin",
                size_bytes: 5,
                url,
                sha256: TEST_HASH,
            }]
            .into_boxed_slice(),
        );
        DownloadableModelSpec {
            id,
            engine: "test",
            model_name: "Test directory model",
            description: "Test fixture",
            requirements: None,
            size_bytes: 5,
            profile: ModelProfile::Fast,
            layout: InstallLayout::Directory { directory_name },
            revision: Some("test-revision"),
            artifacts,
            qwen_platform_limited: false,
            capability: ModelCapability::Diarization,
        }
    }

    #[test]
    fn qwen_manifest_is_pinned_and_complete() {
        let spec = downloadable_specs()
            .into_iter()
            .find(|spec| spec.id == QWEN_MODEL_ID)
            .expect("Qwen model");
        assert_eq!(spec.revision, Some(QWEN_MODEL_REVISION));
        assert_eq!(spec.artifacts.len(), 7);
        assert_eq!(
            spec.artifacts
                .iter()
                .map(|artifact| artifact.size_bytes)
                .sum::<i64>(),
            QWEN_TOTAL_SIZE
        );
        assert!(spec
            .artifacts
            .iter()
            .all(|artifact| artifact.sha256.len() == 64));
        for (path, size, sha256) in QWEN_REQUIRED_ARTIFACTS {
            let artifact = spec
                .artifacts
                .iter()
                .find(|artifact| artifact.path == *path)
                .expect("pinned artifact");
            assert_eq!(artifact.size_bytes, *size);
            assert_eq!(artifact.sha256, *sha256);
        }
    }

    #[test]
    fn diarization_manifest_is_reviewed_pinned_and_complete() {
        let spec = downloadable_specs()
            .into_iter()
            .find(|spec| spec.id == DIARIZATION_MODEL_ID)
            .expect("diarization model card");
        assert_eq!(spec.capability, ModelCapability::Diarization);
        assert_eq!(spec.availability(), ModelAvailability::Available);
        assert_eq!(spec.revision, Some(DIARIZATION_REVISION));
        assert_eq!(spec.artifacts.len(), 2);
        assert_eq!(
            spec.artifacts
                .iter()
                .map(|artifact| artifact.size_bytes)
                .sum::<i64>(),
            DIARIZATION_TOTAL_SIZE
        );
        assert_eq!(
            spec.artifacts
                .iter()
                .map(|artifact| artifact.path)
                .collect::<Vec<_>>(),
            ["segmentation.onnx", "embedding.onnx"]
        );
        assert_eq!(spec.id, "sherpa-diarization-pyannote3-eres2net-voxceleb-v2");
        assert_eq!(spec.size_bytes, 32_478_041);
        assert_eq!(spec.artifacts[0].size_bytes, 5_992_778);
        assert_eq!(
            spec.artifacts[0].sha256,
            "fed22097bca974bad329a930b60865703766ff89f05fa09060bf6fd44e92e319"
        );
        assert_eq!(spec.artifacts[1].size_bytes, 26_485_263);
        assert_eq!(
            spec.artifacts[1].sha256,
            "c59158379255ad66e161679cca6af8d52d51e389e3224ab7d7a7baae295c2db5"
        );
        assert!(spec.artifacts[1]
            .url
            .contains("3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx"));
        assert!(spec.artifacts.iter().all(|artifact| {
            artifact.sha256.len() == 64
                && artifact
                    .sha256
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
                && artifact.url.contains("/resolve/")
                && !artifact.url.contains("/resolve/main/")
        }));
        assert!(DIARIZATION_ARTIFACTS_REVIEWED);
    }

    #[test]
    fn artifact_verification_rejects_size_and_hash_mismatches() {
        let path = std::env::temp_dir().join(format!(
            "blabber-artifact-verification-{}",
            uuid::Uuid::new_v4()
        ));
        fs::write(&path, b"hello").expect("fixture");
        let valid = ModelArtifactSpec {
            path: "fixture",
            size_bytes: 5,
            url: "https://example.invalid/fixture",
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        };
        assert!(verify_file(&path, &valid).expect("valid hash"));
        let wrong_size = ModelArtifactSpec {
            size_bytes: 6,
            ..valid
        };
        assert!(!verify_file(&path, &wrong_size).expect("wrong size"));
        let wrong_hash = ModelArtifactSpec {
            sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ..valid
        };
        assert!(!verify_file(&path, &wrong_hash).expect("wrong hash"));
        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn directory_installation_requires_matching_manifest_and_artifacts() {
        let models_dir = std::env::temp_dir().join(format!(
            "blabber-directory-validation-{}",
            uuid::Uuid::new_v4()
        ));
        let spec = test_directory_spec(
            "test-directory-model",
            "test-directory-model",
            "https://example.invalid/model.bin",
        );
        let package_dir = models_dir.join("test-directory-model");
        fs::create_dir_all(&package_dir).expect("package directory");
        fs::write(package_dir.join("model.bin"), b"hello").expect("artifact");

        assert!(!model_is_installed(&spec, &models_dir));
        write_completion_manifest(&spec, &package_dir).expect("completion manifest");
        assert!(model_is_installed(&spec, &models_dir));

        let manifest_path = package_dir.join(MODEL_COMPLETE_FILE);
        let mut manifest: CompletionManifest =
            serde_json::from_slice(&fs::read(&manifest_path).expect("manifest bytes"))
                .expect("manifest");
        manifest.revision = "stale-revision".to_string();
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("stale manifest"),
        )
        .expect("write stale manifest");
        assert!(!model_is_installed(&spec, &models_dir));

        write_completion_manifest(&spec, &package_dir).expect("restore manifest");
        let mut manifest: CompletionManifest =
            serde_json::from_slice(&fs::read(&manifest_path).expect("restored manifest bytes"))
                .expect("restored manifest");
        manifest.artifacts[0].sha256 =
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("wrong-hash manifest"),
        )
        .expect("write wrong-hash manifest");
        assert!(!model_is_installed(&spec, &models_dir));

        write_completion_manifest(&spec, &package_dir).expect("restore manifest again");
        fs::write(package_dir.join("model.bin"), b"tiny").expect("wrong-size artifact");
        assert!(!model_is_installed(&spec, &models_dir));

        fs::remove_dir_all(models_dir).expect("cleanup");
    }

    #[test]
    fn cancellation_keeps_a_resumable_partial_artifact() {
        let models_dir =
            std::env::temp_dir().join(format!("blabber-directory-cancel-{}", uuid::Uuid::new_v4()));
        let spec = test_directory_spec(
            "cancel-test-package",
            "cancel-test-package",
            "https://example.invalid/model.bin",
        );
        let partial_path = models_dir
            .join(".cancel-test-package.part")
            .join("model.bin.part");
        fs::create_dir_all(partial_path.parent().expect("partial parent"))
            .expect("partial directory");
        fs::write(&partial_path, b"he").expect("partial artifact");

        let error = download_model(&spec, &models_dir, &AtomicBool::new(true), |_, _, _| {})
            .expect_err("canceled download");
        assert!(is_cancelled_error(&error));
        assert_eq!(fs::read(&partial_path).expect("preserved partial"), b"he");
        assert!(!models_dir.join("cancel-test-package").exists());

        fs::remove_dir_all(models_dir).expect("cleanup");
    }

    #[test]
    fn unrelated_directory_package_does_not_block_resumed_atomic_install() {
        let models_dir = std::env::temp_dir().join(format!(
            "blabber-directory-download-{}",
            uuid::Uuid::new_v4()
        ));
        let unrelated = test_directory_spec(
            "qwen-test-package",
            QWEN_MODEL_DIR,
            "https://example.invalid/qwen.bin",
        );
        let unrelated_dir = models_dir.join(QWEN_MODEL_DIR);
        fs::create_dir_all(&unrelated_dir).expect("unrelated package directory");
        fs::write(unrelated_dir.join("model.bin"), b"hello").expect("unrelated artifact");
        write_completion_manifest(&unrelated, &unrelated_dir).expect("unrelated manifest");
        assert!(model_is_installed(&unrelated, &models_dir));

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("test server");
        let address = listener.local_addr().expect("server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("download connection");
            let mut request = [0_u8; 2048];
            let bytes_read = stream.read(&mut request).expect("request");
            let request = String::from_utf8_lossy(&request[..bytes_read]);
            assert!(request.contains("Range: bytes=2-") || request.contains("range: bytes=2-"));
            stream
                .write_all(
                    b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/5\r\nConnection: close\r\n\r\nllo",
                )
                .expect("response");
        });
        let url: &'static str = Box::leak(format!("http://{address}/model.bin").into_boxed_str());
        let requested =
            test_directory_spec("diarization-test-package", "diarization-test-package", url);
        let final_dir = models_dir.join("diarization-test-package");
        fs::create_dir_all(&final_dir).expect("stale final directory");
        fs::write(final_dir.join("model.bin"), b"stale").expect("stale artifact");
        let staging_dir = models_dir.join(".diarization-test-package.part");
        fs::create_dir_all(&staging_dir).expect("staging directory");
        fs::write(staging_dir.join("model.bin.part"), b"he").expect("partial artifact");

        download_model(
            &requested,
            &models_dir,
            &AtomicBool::new(false),
            |_, _, _| {},
        )
        .expect("directory download");
        server.join().expect("test server completed");

        assert_eq!(
            fs::read(final_dir.join("model.bin")).expect("installed artifact"),
            b"hello"
        );
        assert!(model_is_installed(&requested, &models_dir));
        fs::remove_dir_all(models_dir).expect("cleanup");
    }

    #[test]
    fn cancellation_is_typed_and_detectable() {
        let cancelled = AtomicBool::new(true);
        let error = ensure_not_cancelled(&cancelled).expect_err("cancellation error");
        assert!(is_cancelled_error(&error));
    }
}
