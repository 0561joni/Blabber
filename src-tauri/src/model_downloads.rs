use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::asr::{discover_whisper_models, SharedWhisperEngine, TranscriptionEngine};
use crate::settings::ModelProfile;
use crate::storage;

const MODEL_DOWNLOAD_EVENT: &str = "model-download-status";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadableModel {
    pub id: String,
    pub model_name: String,
    pub description: String,
    pub size_bytes: i64,
    pub profile: ModelProfile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadState {
    Idle,
    Downloading,
    Completed,
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
}

#[derive(Clone)]
pub struct ModelDownloadManager {
    app: AppHandle,
    models_dir: PathBuf,
    db_path: PathBuf,
    engine: Arc<SharedWhisperEngine>,
    statuses: Arc<Mutex<HashMap<String, ModelDownloadStatus>>>,
    active_download: Arc<Mutex<Option<String>>>,
}

impl ModelDownloadManager {
    pub fn new(
        app: AppHandle,
        models_dir: PathBuf,
        db_path: PathBuf,
        engine: Arc<SharedWhisperEngine>,
    ) -> Self {
        let statuses = downloadable_specs()
            .into_iter()
            .map(|spec| {
                (
                    spec.id.to_string(),
                    ModelDownloadStatus {
                        model_id: spec.id.to_string(),
                        model_name: spec.model_name.to_string(),
                        state: ModelDownloadState::Idle,
                        downloaded_bytes: 0,
                        total_bytes: Some(spec.size_bytes),
                        progress_percent: Some(0.0),
                        error_message: None,
                    },
                )
            })
            .collect();

        Self {
            app,
            models_dir,
            db_path,
            engine,
            statuses: Arc::new(Mutex::new(statuses)),
            active_download: Arc::new(Mutex::new(None)),
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

        let mut active = self
            .active_download
            .lock()
            .map_err(|_| anyhow!("download state is unavailable"))?;
        if let Some(current) = &*active {
            if current != model_id {
                return Err(anyhow!("Another model download is already in progress."));
            }
        }
        *active = Some(model_id.to_string());
        drop(active);

        let initial = ModelDownloadStatus {
            model_id: spec.id.to_string(),
            model_name: spec.model_name.to_string(),
            state: ModelDownloadState::Downloading,
            downloaded_bytes: 0,
            total_bytes: Some(spec.size_bytes),
            progress_percent: Some(0.0),
            error_message: None,
        };
        self.update_status(initial.clone());

        let app = self.app.clone();
        let models_dir = self.models_dir.clone();
        let db_path = self.db_path.clone();
        let engine = Arc::clone(&self.engine);
        let statuses = Arc::clone(&self.statuses);
        let active_download = Arc::clone(&self.active_download);

        thread::spawn(move || {
            let result = download_model(&spec, &models_dir, |downloaded, total| {
                let progress_percent = total.map(|total_bytes| {
                    if total_bytes <= 0 {
                        0.0
                    } else {
                        ((downloaded as f64 / total_bytes as f64) * 100.0) as f32
                    }
                });
                let status = ModelDownloadStatus {
                    model_id: spec.id.to_string(),
                    model_name: spec.model_name.to_string(),
                    state: ModelDownloadState::Downloading,
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                    progress_percent,
                    error_message: None,
                };
                persist_status(&app, &statuses, status);
            });

            match result {
                Ok(()) => {
                    let refreshed = engine.refresh_from_disk();
                    if let Ok(models) = refreshed {
                        let _ = storage::sync_installed_models_for_db_path(&db_path, &models);
                        let _ =
                            storage::apply_preferred_model_defaults_for_db_path(&db_path, &models);
                    } else if let Ok(models) = discover_whisper_models(&models_dir) {
                        let _ = storage::sync_installed_models_for_db_path(&db_path, &models);
                    }

                    persist_status(
                        &app,
                        &statuses,
                        ModelDownloadStatus {
                            model_id: spec.id.to_string(),
                            model_name: spec.model_name.to_string(),
                            state: ModelDownloadState::Completed,
                            downloaded_bytes: spec.size_bytes,
                            total_bytes: Some(spec.size_bytes),
                            progress_percent: Some(100.0),
                            error_message: None,
                        },
                    );
                }
                Err(error) => {
                    persist_status(
                        &app,
                        &statuses,
                        ModelDownloadStatus {
                            model_id: spec.id.to_string(),
                            model_name: spec.model_name.to_string(),
                            state: ModelDownloadState::Failed,
                            downloaded_bytes: 0,
                            total_bytes: Some(spec.size_bytes),
                            progress_percent: None,
                            error_message: Some(error.to_string()),
                        },
                    );
                }
            }

            if let Ok(mut current) = active_download.lock() {
                *current = None;
            }
        });

        Ok(initial)
    }

    fn update_status(&self, status: ModelDownloadStatus) {
        persist_status(&self.app, &self.statuses, status);
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
    models_dir: &PathBuf,
    mut on_progress: impl FnMut(i64, Option<i64>),
) -> Result<()> {
    fs::create_dir_all(models_dir)?;
    let final_path = models_dir.join(spec.model_name);
    if final_path.exists() {
        return Ok(());
    }

    let temp_path = models_dir.join(format!("{}.part", spec.model_name));
    if temp_path.exists() {
        fs::remove_file(&temp_path)?;
    }

    let client = Client::builder().build()?;
    let mut response = client
        .get(spec.url)
        .send()
        .with_context(|| format!("failed to download {}", spec.model_name))?
        .error_for_status()
        .with_context(|| format!("download failed for {}", spec.model_name))?;

    let total = response.content_length().map(|value| value as i64);
    let mut file = File::create(&temp_path)?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0_i64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        file.write_all(chunk)?;
        hasher.update(chunk);
        downloaded += read as i64;
        on_progress(downloaded, total);
    }
    file.flush()?;

    let computed_hash = format!("{:x}", hasher.finalize());
    if computed_hash != spec.sha256 {
        let _ = fs::remove_file(&temp_path);
        return Err(anyhow!(
            "Checksum mismatch for {}: expected {} but got {}. The download may be corrupted.",
            spec.model_name,
            spec.sha256,
            computed_hash
        ));
    }

    fs::rename(&temp_path, &final_path)?;
    Ok(())
}

#[derive(Clone, Copy)]
struct DownloadableModelSpec {
    id: &'static str,
    model_name: &'static str,
    description: &'static str,
    size_bytes: i64,
    profile: ModelProfile,
    url: &'static str,
    sha256: &'static str,
}

fn downloadable_specs() -> Vec<DownloadableModelSpec> {
    vec![
        DownloadableModelSpec {
            id: "ggml-tiny-bin",
            model_name: "ggml-tiny.bin",
            description: "Smallest local model for quick tests and lightweight dictation.",
            size_bytes: 75_000_000,
            profile: ModelProfile::Fast,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin?download=true",
            sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        },
        DownloadableModelSpec {
            id: "ggml-small-bin",
            model_name: "ggml-small.bin",
            description: "Good balance when you want lower memory use with better quality than tiny.",
            size_bytes: 466_000_000,
            profile: ModelProfile::Balanced,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin?download=true",
            sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        },
        DownloadableModelSpec {
            id: "ggml-medium-bin",
            model_name: "ggml-medium.bin",
            description: "Strong default for shortcut dictation when you want better accuracy.",
            size_bytes: 1_530_000_000,
            profile: ModelProfile::Balanced,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin?download=true",
            sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        },
        DownloadableModelSpec {
            id: "ggml-large-v3-turbo-bin",
            model_name: "ggml-large-v3-turbo.bin",
            description: "Best full-size turbo model when you want top quality and speed.",
            size_bytes: 1_620_000_000,
            profile: ModelProfile::Accurate,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin?download=true",
            sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        },
        DownloadableModelSpec {
            id: "ggml-large-v3-turbo-q5_0-bin",
            model_name: "ggml-large-v3-turbo-q5_0.bin",
            description: "Quantized turbo model with lower memory use and a strong quality-speed tradeoff.",
            size_bytes: 1_210_000_000,
            profile: ModelProfile::Accurate,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin?download=true",
            sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        },
    ]
}

pub fn list_downloadable_models() -> Vec<DownloadableModel> {
    downloadable_specs()
        .into_iter()
        .map(|spec| DownloadableModel {
            id: spec.id.to_string(),
            model_name: spec.model_name.to_string(),
            description: spec.description.to_string(),
            size_bytes: spec.size_bytes,
            profile: spec.profile,
        })
        .collect()
}
