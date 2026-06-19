use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tauri::{AppHandle, Manager};

use crate::asr::{self, SharedWhisperEngine, TranscriptionEngine};
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
    pub engine: Arc<SharedWhisperEngine>,
    pub recording_controller: RecordingController,
    pub dictation_controller: QuickDictationController,
    pub file_transcription_controller: FileTranscriptionController,
    pub model_download_manager: ModelDownloadManager,
    pub desktop_shell: DesktopShellController,
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

        let engine_models = asr::discover_whisper_models(&models_dir)?;
        let engine = Arc::new(SharedWhisperEngine::new(
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
        };

        storage::initialize_database(&state)?;
        storage::sync_installed_models(&state, &engine_models)?;
        storage::apply_preferred_model_defaults(&state, &engine_models)?;
        vocabulary::seed_builtin_terms(&state)?;
        let settings = storage::get_settings(&state)?;
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
        }
    }
}

fn shortcut_unsupported_message() -> &'static str {
    if cfg!(target_os = "linux") && crate::platform::is_wayland() {
        "Global shortcuts are inactive in this Wayland session. Use the command shown in Settings as a compositor shortcut."
    } else {
        "Global shortcuts are not supported on this platform."
    }
}
