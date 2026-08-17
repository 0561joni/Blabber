use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tauri::{AppHandle, Manager};

use crate::asr::{self, LocalTranscriptionEngine, TranscriptionEngine};
use crate::audio_capture::RecordingController;
use crate::autostart;
use crate::desktop_shell::DesktopShellController;
use crate::dictation::QuickDictationController;
use crate::file_jobs::FileTranscriptionController;
use crate::model_downloads::ModelDownloadManager;
use crate::settings::HealthCheckResponse;
use crate::sound::SoundPlayer;
use crate::storage;
use crate::vocabulary;

#[derive(Clone)]
pub struct AppState {
    pub app_name: String,
    pub app_version: String,
    pub temp_dir: PathBuf,
    pub models_dir: PathBuf,
    pub db_path: PathBuf,
    pub engine: Arc<LocalTranscriptionEngine>,
    pub recording_controller: RecordingController,
    pub dictation_controller: QuickDictationController,
    pub file_transcription_controller: FileTranscriptionController,
    pub model_download_manager: ModelDownloadManager,
    pub desktop_shell: DesktopShellController,
    pub startup_notices: Arc<Mutex<Vec<String>>>,
    pub rediarization_cancellations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl AppState {
    pub fn initialize(app: &AppHandle) -> Result<Self> {
        let app_name = app.package_info().name.clone();
        let app_version = app.package_info().version.to_string();
        let app_data_dir = app
            .path()
            .app_data_dir()
            .context("failed to resolve app data directory")?;
        let temp_dir = app_data_dir.join("temp");
        let models_dir = app_data_dir.join("models");
        let db_path = app_data_dir.join("speech_to_text.sqlite");

        fs::create_dir_all(&app_data_dir)?;
        fs::create_dir_all(&temp_dir)?;
        fs::create_dir_all(&models_dir)?;
        crate::model_downloads::start_background_vad_download(models_dir.clone());

        let engine_models = asr::discover_installed_models(&models_dir)?;
        let engine = Arc::new(LocalTranscriptionEngine::new(
            models_dir.clone(),
            engine_models.clone(),
        ));
        let transcription_engine: Arc<dyn TranscriptionEngine> = engine.clone();
        let recording_controller = RecordingController::new(temp_dir.clone());
        let desktop_shell = DesktopShellController::initialize(app)?;
        let sound_player = Arc::new(match SoundPlayer::new() {
            Ok(player) => Some(player),
            Err(err) => {
                eprintln!("[sound] disabled (init failed): {err:?}");
                None
            }
        });
        let dictation_controller = QuickDictationController::new(
            app.clone(),
            Arc::clone(&transcription_engine),
            recording_controller.clone(),
            db_path.clone(),
            desktop_shell.clone(),
            Arc::clone(&sound_player),
        );
        let file_transcription_controller = FileTranscriptionController::new(
            app.clone(),
            Arc::clone(&transcription_engine),
            models_dir.clone(),
            db_path.clone(),
        );
        let model_download_manager = ModelDownloadManager::new(
            app.clone(),
            models_dir.clone(),
            db_path.clone(),
            Arc::clone(&engine),
        );

        let state = Self {
            app_name,
            app_version,
            temp_dir,
            models_dir,
            db_path,
            engine,
            recording_controller,
            dictation_controller,
            file_transcription_controller,
            model_download_manager,
            desktop_shell,
            startup_notices: Arc::new(Mutex::new(Vec::new())),
            rediarization_cancellations: Arc::new(Mutex::new(HashMap::new())),
        };

        storage::initialize_database(&state)?;
        if let Some(notice) = migrate_windows_qwen_selection(&state, &engine_models, &app_data_dir)?
        {
            if let Ok(mut notices) = state.startup_notices.lock() {
                notices.push(notice);
            }
        }
        storage::sync_installed_models(&state, &engine_models)?;
        storage::apply_preferred_model_defaults(&state, &engine_models)?;
        vocabulary::seed_builtin_terms(&state)?;
        let settings = storage::get_settings(&state)?;
        if settings.file_diarization_enabled
            && crate::model_downloads::installed_diarization_package_path(&state.models_dir)
                .is_none()
        {
            if let Err(error) = state
                .model_download_manager
                .start_download(crate::diarization::DIARIZATION_MODEL_ID)
            {
                eprintln!("[diarization-model] startup resume unavailable: {error:#}");
            }
        }
        state
            .recording_controller
            .set_preferred_input_device(settings.preferred_input_device.clone());
        autostart::sync_launch_at_login(app, settings.launch_at_login_enabled)?;
        if crate::platform::global_shortcut_supported() {
            state.dictation_controller.sync_shortcut_registration()?;
        } else {
            state
                .dictation_controller
                .mark_shortcut_unsupported(shortcut_unsupported_message())?;
        }
        // Last-resort safety net: auto-recovers dictation if it ever gets stuck.
        state.dictation_controller.spawn_watchdog();
        Ok(state)
    }

    pub fn health_check(&self) -> HealthCheckResponse {
        HealthCheckResponse {
            app_name: self.app_name.clone(),
            app_version: self.app_version.clone(),
            platform: std::env::consts::OS.to_string(),
            db_path: self.db_path.display().to_string(),
            temp_dir: self.temp_dir.display().to_string(),
            models_dir: self.models_dir.display().to_string(),
            startup_notices: self
                .startup_notices
                .lock()
                .map(|notices| notices.clone())
                .unwrap_or_default(),
        }
    }
}

#[cfg(target_os = "windows")]
fn migrate_windows_qwen_selection(
    state: &AppState,
    models: &[crate::asr::InstalledModel],
    app_data_dir: &std::path::Path,
) -> Result<Option<String>> {
    let replaced = storage::replace_qwen_selections_with_whisper(&state.db_path, models)?;
    if !replaced {
        return Ok(None);
    }
    let marker = app_data_dir.join(".qwen-windows-fallback-notice-v1");
    if marker.exists() {
        return Ok(None);
    }
    fs::write(&marker, b"shown")?;
    Ok(Some(
        "Qwen3-ASR is not supported on Windows, so your affected model selection was changed to the Accurate Whisper default."
            .to_string(),
    ))
}

#[cfg(not(target_os = "windows"))]
fn migrate_windows_qwen_selection(
    _state: &AppState,
    _models: &[crate::asr::InstalledModel],
    _app_data_dir: &std::path::Path,
) -> Result<Option<String>> {
    Ok(None)
}

fn shortcut_unsupported_message() -> &'static str {
    if cfg!(target_os = "linux") && crate::platform::is_wayland() {
        "Global shortcuts are inactive in this Wayland session. Use the command shown in Settings as a compositor shortcut."
    } else {
        "Global shortcuts are not supported on this platform."
    }
}
