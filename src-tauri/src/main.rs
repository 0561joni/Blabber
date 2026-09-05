#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod asr;
mod audio_capture;
mod audio_chunks;
mod audio_files;
mod audio_preprocess;
mod autostart;
mod desktop_shell;
mod diarization;
mod diarization_worker;
mod dictation;
mod file_jobs;
mod insertion;
#[cfg(target_os = "linux")]
mod ipc;
mod model_downloads;
mod model_metadata;
mod native_asr;
mod platform;
mod qwen_asr;
mod review;
mod review_jobs;
mod review_media;
mod settings;
mod shutdown;
mod sound;
mod speaker_reconciliation;
mod startup;
mod storage;
mod system_volume;
mod transcript_commands;
mod transcript_stitching;
mod transcription_policy;
mod transcription_quality;
mod transcription_worker;
mod vocabulary;

use app_state::AppState;
use asr::{
    FileTranscriptionRequest, InstalledModel, TranscriptionEngine, TranscriptionPreviewRequest,
    TranscriptionPreviewResponse,
};
use audio_capture::{InputDeviceOption, RecordingResult, RecordingStatusResponse};
use audio_files::{
    FileTranscriptionRequest as UploadedFileTranscriptionRequest, SelectedSourceFile,
};
use dictation::QuickDictationStatusResponse;
use file_jobs::{FileTranscriptionStatusEvent, StartFileTranscriptionResponse};
use model_downloads::{DownloadableModel, ModelDownloadStatus};
use serde::{Deserialize, Serialize};
use settings::{AppSettings, HealthCheckResponse, InsertBehavior, SettingsPatch};
use startup::{StartupCoordinator, StartupPhase, StartupStatus};
use std::process::Command;
use std::time::Duration;
use storage::{TranscriptDetail, TranscriptSummary};
use tauri::{DragDropEvent, Emitter, Manager, WebviewEvent, Window, WindowEvent};
use tauri_plugin_clipboard_manager::ClipboardExt;
use transcript_commands::{TranscriptCopyVariant, TranscriptExportFormat, TranscriptExportResult};
use vocabulary::{CreateVocabularyTermInput, UpdateVocabularyTermInput, VocabularyTerm};

#[tauri::command]
fn health_check(state: tauri::State<'_, AppState>) -> Result<HealthCheckResponse, String> {
    Ok(state.health_check())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformInfo {
    os: &'static str,
    is_wayland: bool,
    is_gnome: bool,
    has_appindicator_hint: bool,
    auto_paste_supported: bool,
    global_shortcut_supported: bool,
    dictate_toggle_executable: Option<String>,
    dictate_toggle_command: Option<String>,
}

#[tauri::command]
fn dictate_press(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .dictation_controller
        .ui_press()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn dictate_release(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .dictation_controller
        .ui_release()
        .map_err(|error| error.to_string())
}

/// Toggle dictation on/off from outside Blabber (e.g. the in-app button on
/// Wayland, where the global shortcut is unavailable).  Does NOT force
/// clipboard-only — the caller is assumed to be working in another app.
#[tauri::command]
fn dictate_toggle(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state
        .dictation_controller
        .ui_toggle()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        is_wayland: platform::is_wayland(),
        is_gnome: platform::is_gnome(),
        has_appindicator_hint: platform::has_appindicator_hint(),
        auto_paste_supported: platform::auto_paste_supported(),
        global_shortcut_supported: platform::global_shortcut_supported(),
        dictate_toggle_executable: platform::dictate_toggle_executable(),
        dictate_toggle_command: platform::dictate_toggle_command(),
    }
}

#[tauri::command]
fn is_app_shutting_down() -> bool {
    shutdown::is_shutting_down()
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    shutdown::request_exit(&app, shutdown::ExitAction::Quit);
}

#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    shutdown::request_exit(&app, shutdown::ExitAction::Restart);
}

#[tauri::command]
fn get_startup_status(startup: tauri::State<'_, StartupCoordinator>) -> StartupStatus {
    startup.status()
}

#[tauri::command]
fn frontend_startup_complete(app: tauri::AppHandle, startup: tauri::State<'_, StartupCoordinator>) {
    if !startup.mark_frontend_ready(&app) {
        return;
    }

    let app_handle = app.clone();
    let startup = startup.inner().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        let _ = finish_startup_handoff(&app_handle, &startup);
    });
}

#[tauri::command]
fn report_startup_failure(
    app: tauri::AppHandle,
    startup: tauri::State<'_, StartupCoordinator>,
    message: String,
) {
    startup.fail(&app, message);
}

#[tauri::command]
fn complete_startup_handoff(
    app: tauri::AppHandle,
    startup: tauri::State<'_, StartupCoordinator>,
) -> Result<(), String> {
    finish_startup_handoff(&app, startup.inner())
}

fn finish_startup_handoff(
    app: &tauri::AppHandle,
    startup: &StartupCoordinator,
) -> Result<(), String> {
    if !startup.claim_handoff() {
        return Ok(());
    }

    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_string())?;
    main.show().map_err(|error| error.to_string())?;
    let _ = main.unminimize();
    let _ = main.set_focus();
    if let Some(splash) = app.get_webview_window("splashscreen") {
        splash.close().map_err(|error| error.to_string())?;
    }
    eprintln!("[startup] splash handoff completed");
    Ok(())
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> Result<AppSettings, String> {
    storage::get_settings(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<AppSettings, String> {
    let previous = storage::get_settings(state.inner()).map_err(|error| error.to_string())?;
    let sync_autostart = patch.launch_at_login_enabled.is_some();
    let sync_shortcut = patch.shortcut.is_some() || patch.shortcut_mode.is_some();
    let diarization_change = patch.file_diarization_enabled;
    let settings =
        storage::update_settings(state.inner(), patch).map_err(|error| error.to_string())?;
    let integration_result = (|| -> Result<(), String> {
        if sync_autostart {
            autostart::sync_launch_at_login(&app, settings.launch_at_login_enabled)
                .map_err(|error| error.to_string())?;
        }
        if sync_shortcut && platform::global_shortcut_supported() {
            state
                .dictation_controller
                .sync_shortcut_registration()
                .map_err(|error| error.to_string())?;
        } else if sync_shortcut {
            state
                .dictation_controller
                .mark_shortcut_unsupported(shortcut_unsupported_message())
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = integration_result {
        // Failed OS integration must not leave a different preference in the database.
        let restored = storage::update_settings(
            state.inner(),
            SettingsPatch {
                default_mode: Some(previous.default_mode.clone()),
                shortcut: Some(previous.shortcut.clone()),
                shortcut_mode: Some(previous.shortcut_mode.clone()),
                language_mode: Some(previous.language_mode.clone()),
                fixed_language: Some(previous.fixed_language.clone()),
                preferred_input_device: Some(previous.preferred_input_device.clone()),
                insert_behavior: Some(previous.insert_behavior.clone()),
                launch_at_login_enabled: Some(previous.launch_at_login_enabled.clone()),
                gpu_enabled: Some(previous.gpu_enabled.clone()),
                shortcut_dictation_model_profile: Some(
                    previous.shortcut_dictation_model_profile.clone(),
                ),
                shortcut_dictation_selected_model_id: Some(
                    previous.shortcut_dictation_selected_model_id.clone(),
                ),
                quick_dictate_model_profile: Some(previous.quick_dictate_model_profile.clone()),
                quick_dictate_selected_model_id: Some(
                    previous.quick_dictate_selected_model_id.clone(),
                ),
                file_transcribe_model_profile: Some(previous.file_transcribe_model_profile.clone()),
                file_transcribe_selected_model_id: Some(
                    previous.file_transcribe_selected_model_id.clone(),
                ),
                appearance: Some(previous.appearance.clone()),
                motion_preference: Some(previous.motion_preference.clone()),
                save_history: Some(previous.save_history.clone()),
                sounds_enabled: Some(previous.sounds_enabled.clone()),
                volume_ducking_enabled: Some(previous.volume_ducking_enabled.clone()),
                file_diarization_enabled: Some(previous.file_diarization_enabled.clone()),
            },
        )
        .map_err(|rollback| format!("{error}; could not restore previous settings: {rollback}"))?;
        if sync_autostart {
            let _ = autostart::sync_launch_at_login(&app, restored.launch_at_login_enabled);
        }
        if sync_shortcut {
            let _ = state.dictation_controller.sync_shortcut_registration();
        }
        return Err(error);
    }
    match diarization_change {
        Some(true)
            if model_downloads::installed_diarization_package_path(&state.models_dir).is_none() =>
        {
            if let Err(error) = state
                .model_download_manager
                .start_download(diarization::DIARIZATION_MODEL_ID)
            {
                eprintln!("[diarization-model] automatic download unavailable: {error:#}");
            }
        }
        Some(false) => {
            let _ = state
                .model_download_manager
                .cancel_download(diarization::DIARIZATION_MODEL_ID);
        }
        _ => {}
    }
    state
        .recording_controller
        .set_preferred_input_device(settings.preferred_input_device.clone());
    let _ = app.emit("settings-changed", &settings);
    Ok(settings)
}

#[tauri::command]
fn list_transcripts(
    state: tauri::State<'_, AppState>,
    query: Option<String>,
) -> Result<Vec<TranscriptSummary>, String> {
    storage::list_transcripts(state.inner(), query).map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_transcript(
    state: tauri::State<'_, AppState>,
    transcript_id: String,
) -> Result<(), String> {
    storage::delete_transcript(state.inner(), &transcript_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_all_transcripts(state: tauri::State<'_, AppState>) -> Result<(), String> {
    storage::delete_all_transcripts(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_transcript(
    state: tauri::State<'_, AppState>,
    transcript_id: String,
) -> Result<TranscriptDetail, String> {
    state
        .review_store
        .get(&review::ReviewRef::Saved { id: transcript_id })
        .map(|document| document.detail)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RediarizationRequest {
    job_id: String,
    transcript_id: String,
    source_file: Option<SelectedSourceFile>,
    speaker_count_hint: Option<i32>,
}

// Compatibility entry point; the shared controller owns lifetime and cancellation.
#[tauri::command]
async fn rediarize_transcript(
    state: tauri::State<'_, AppState>,
    request: RediarizationRequest,
) -> Result<TranscriptDetail, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<TranscriptDetail> {
        let reference = review::ReviewRef::Saved {
            id: request.transcript_id,
        };
        if let Some(source) = request.source_file {
            review_media::validated_source(
                &state.review_store,
                &reference,
                Some(source.file_path),
            )?;
        }
        let job = state.review_jobs.start(
            reference.clone(),
            request.speaker_count_hint,
            false,
            Some(request.job_id),
        )?;
        loop {
            if let Some(status) = state
                .review_jobs
                .statuses()
                .into_iter()
                .find(|s| s.job_id == job.job_id)
            {
                if !status.active() {
                    if status.stage == "completed" {
                        return Ok(state.review_store.get(&reference)?.detail);
                    }
                    anyhow::bail!(status
                        .error
                        .map(|e| format!("{}: {}", e.code, e.message))
                        .unwrap_or(status.status_text));
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_rediarization(state: tauri::State<'_, AppState>, job_id: String) -> Result<(), String> {
    state.review_jobs.cancel(&job_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_review(
    state: tauri::State<'_, AppState>,
    reference: review::ReviewRef,
) -> Result<review::ReviewDocument, review::ReviewError> {
    let store = state.review_store.clone();
    tauri::async_runtime::spawn_blocking(move || store.get(&reference))
        .await
        .map_err(|e| review::ReviewError::from(anyhow::anyhow!(e)))?
        .map_err(Into::into)
}

#[tauri::command]
async fn edit_review(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    reference: review::ReviewRef,
    expected_revision: u64,
    edit: review::ReviewEdit,
) -> Result<review::ReviewDocument, review::ReviewError> {
    let work = shutdown::begin_work(false).map_err(review::ReviewError::from)?;
    let store = state.review_store.clone();
    let document = tauri::async_runtime::spawn_blocking(move || {
        let _work = work;
        store.edit(&reference, expected_revision, edit)
    })
    .await
    .map_err(|e| review::ReviewError::from(anyhow::anyhow!(e)))?
    .map_err(review::ReviewError::from)?;
    let _ = app.emit("review-updated", &document.reference);
    Ok(document)
}

#[tauri::command]
async fn start_review_job(
    state: tauri::State<'_, AppState>,
    reference: review::ReviewRef,
    speaker_count: Option<i32>,
    reset: bool,
) -> Result<review_jobs::ReviewJobStatus, review::ReviewError> {
    let jobs = state.review_jobs.clone();
    tauri::async_runtime::spawn_blocking(move || jobs.start(reference, speaker_count, reset, None))
        .await
        .map_err(|e| review::ReviewError::from(anyhow::anyhow!(e)))?
        .map_err(Into::into)
}

#[tauri::command]
fn get_review_job_statuses(state: tauri::State<'_, AppState>) -> Vec<review_jobs::ReviewJobStatus> {
    state.review_jobs.statuses()
}
#[tauri::command]
fn cancel_review_job(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<(), review::ReviewError> {
    state.review_jobs.cancel(&job_id).map_err(Into::into)
}

#[tauri::command]
async fn resolve_review_audio(
    state: tauri::State<'_, AppState>,
    reference: review::ReviewRef,
    replacement_path: Option<String>,
    fallback: bool,
) -> Result<review_media::ReviewAudio, review::ReviewError> {
    let state = state.inner().clone();
    let work = shutdown::begin_work(false).map_err(review::ReviewError::from)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _work = work;
        state
            .review_media
            .resolve(&state.review_store, &reference, replacement_path, fallback)
    })
    .await
    .map_err(|e| review::ReviewError::from(anyhow::anyhow!(e)))?
    .map_err(Into::into)
}
#[tauri::command]
fn release_review_audio(state: tauri::State<'_, AppState>, token: String) {
    state.review_media.release(&token);
}
#[tauri::command]
async fn copy_review(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    reference: review::ReviewRef,
    variant: TranscriptCopyVariant,
) -> Result<(), review::ReviewError> {
    let store = state.review_store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        transcript_commands::copy(&app, &store.get(&reference)?.detail, variant)
    })
    .await
    .map_err(|e| review::ReviewError::from(anyhow::anyhow!(e)))?
    .map_err(Into::into)
}
#[tauri::command]
async fn export_review(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    reference: review::ReviewRef,
    format: TranscriptExportFormat,
) -> Result<TranscriptExportResult, review::ReviewError> {
    let store = state.review_store.clone();
    let work = shutdown::begin_work(false).map_err(review::ReviewError::from)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _work = work;
        transcript_commands::export_blocking(&app, &window, &store.get(&reference)?.detail, format)
    })
    .await
    .map_err(|e| review::ReviewError::from(anyhow::anyhow!(e)))?
    .map_err(Into::into)
}
#[tauri::command]
async fn get_file_transcription_result(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<file_jobs::FileTranscriptionResponse, String> {
    let controller = state.file_transcription_controller.clone();
    tauri::async_runtime::spawn_blocking(move || controller.result(&job_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn dismiss_file_transcription(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    if let Ok(result) = state.file_transcription_controller.result(&job_id) {
        let reference = result
            .saved_transcript
            .map(|s| review::ReviewRef::Saved { id: s.id })
            .unwrap_or(review::ReviewRef::Session { id: job_id.clone() });
        if state.review_jobs.active_for(&reference) {
            return Err("Stop the speaker retry before dismissing this result.".into());
        }
    }
    state
        .file_transcription_controller
        .dismiss(&job_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_transcript(
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    title: String,
) -> Result<TranscriptSummary, String> {
    state
        .review_store
        .rename_title(&transcript_id, &title)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn rename_transcript_speaker(
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    speaker_id: String,
    display_name: String,
) -> Result<TranscriptDetail, String> {
    let reference = review::ReviewRef::Saved { id: transcript_id };
    let document = state
        .review_store
        .get(&reference)
        .map_err(|e| e.to_string())?;
    state
        .review_store
        .edit(
            &reference,
            document.revision,
            review::ReviewEdit::Rename {
                speaker_id,
                name: display_name,
            },
        )
        .map(|document| document.detail)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn copy_transcript(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    variant: TranscriptCopyVariant,
) -> Result<(), String> {
    let detail = state
        .review_store
        .get(&review::ReviewRef::Saved { id: transcript_id })
        .map(|document| document.detail)
        .map_err(|error| error.to_string())?;
    transcript_commands::copy(&app, &detail, variant).map_err(|error| error.to_string())
}

#[tauri::command]
async fn export_transcript(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    format: TranscriptExportFormat,
) -> Result<TranscriptExportResult, String> {
    let app_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let detail = app_state
            .review_store
            .get(&review::ReviewRef::Saved { id: transcript_id })
            .map(|document| document.detail)
            .map_err(|error| error.to_string())?;
        transcript_commands::export_blocking(&app, &window, &detail, format)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn copy_text_to_clipboard(app: tauri::AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_installed_models(state: tauri::State<'_, AppState>) -> Result<Vec<InstalledModel>, String> {
    storage::list_installed_models(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
fn list_downloadable_models(state: tauri::State<'_, AppState>) -> Vec<DownloadableModel> {
    model_downloads::list_downloadable_models(Some(&state.models_dir))
}

#[tauri::command]
fn get_model_download_statuses(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ModelDownloadStatus>, String> {
    Ok(state.model_download_manager.statuses())
}

#[tauri::command]
fn start_model_download(
    state: tauri::State<'_, AppState>,
    model_id: String,
) -> Result<ModelDownloadStatus, String> {
    state
        .model_download_manager
        .start_download(&model_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_model_download(
    state: tauri::State<'_, AppState>,
    model_id: String,
) -> Result<ModelDownloadStatus, String> {
    state
        .model_download_manager
        .cancel_download(&model_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_models_folder(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(&state.models_dir);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("explorer");
        command.arg(&state.models_dir);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(&state.models_dir);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_input_devices() -> Result<Vec<InputDeviceOption>, String> {
    audio_capture::list_input_devices().map_err(|error| error.to_string())
}

#[tauri::command]
fn get_dictation_overlay_status(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(state.desktop_shell.overlay_payload()).map_err(|error| error.to_string())
}

#[tauri::command]
async fn preview_transcription(
    state: tauri::State<'_, AppState>,
    request: TranscriptionPreviewRequest,
) -> Result<TranscriptionPreviewResponse, String> {
    let work = shutdown::begin_work(true).map_err(|e| e.to_string())?;
    shutdown::set_manual_handoff(false);
    let app_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _work = work;
        shutdown::ensure_running().map_err(|e| e.to_string())?;
        let resolved_model = resolve_model_selection(
            app_state
                .engine
                .list_models()
                .map_err(|error| error.to_string())?,
            request.selected_model_id.as_deref(),
            request.profile,
        );

        let Some(file_path) = request.file_path.clone() else {
            return Ok(TranscriptionPreviewResponse {
                source_kind: request.source_kind,
                resolved_model,
                result: None,
                error: Some(asr::EngineErrorPayload {
                    code: "transcription_input_missing".to_string(),
                    message: "Record audio first so the app has a normalized WAV to transcribe."
                        .to_string(),
                }),
            });
        };

        let vocabulary_prompt = vocabulary::build_asr_prompt_from_db_path(&app_state.db_path)
            .map_err(|error| error.to_string())?;

        let result = app_state.engine.transcribe_file(
            FileTranscriptionRequest {
                use_context: Some(match request.source_kind {
                    asr::PreviewSourceKind::QuickDictate => {
                        model_metadata::ModelUseContext::QuickDictate
                    }
                    asr::PreviewSourceKind::FileUpload => {
                        model_metadata::ModelUseContext::FileTranscription
                    }
                }),
                profile: request.profile,
                selected_model_id: request.selected_model_id.clone(),
                language_mode: request.language_mode,
                fixed_language: request.fixed_language.clone(),
                timestamps: request.timestamps,
                prefer_gpu: request.prefer_gpu,
                file_path: file_path.clone(),
                context_prompt: vocabulary_prompt.as_ref().map(|prompt| prompt.text.clone()),
                context_terms: vocabulary_prompt
                    .as_ref()
                    .map(|prompt| prompt.terms.clone())
                    .unwrap_or_default(),
            },
            None,
        );

        Ok(match result {
            Ok(result) => {
                let corrected = vocabulary::correct_transcript_result(&app_state.db_path, result)
                    .map_err(|error| error.to_string())?;
                TranscriptionPreviewResponse {
                    source_kind: request.source_kind,
                    resolved_model,
                    result: Some(corrected),
                    error: None,
                }
            }
            Err(error) => TranscriptionPreviewResponse {
                source_kind: request.source_kind,
                resolved_model,
                result: None,
                error: Some(asr::engine_error_payload(&error)),
            },
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn pick_audio_files(window: Window) -> Result<Vec<SelectedSourceFile>, String> {
    audio_files::pick_audio_files(&window)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn prepare_dropped_audio_files(paths: Vec<String>) -> Result<Vec<SelectedSourceFile>, String> {
    audio_files::prepare_dropped_audio_files(paths).map_err(|error| error.to_string())
}

#[tauri::command]
fn start_file_transcription(
    state: tauri::State<'_, AppState>,
    request: UploadedFileTranscriptionRequest,
) -> Result<StartFileTranscriptionResponse, String> {
    let _work = shutdown::begin_work(true).map_err(|e| e.to_string())?;
    diarization::validate_speaker_count_hint(request.speaker_count_hint).map_err(str::to_string)?;
    Ok(state.file_transcription_controller.start(request))
}

#[tauri::command]
fn get_file_transcription_statuses(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileTranscriptionStatusEvent>, String> {
    Ok(state.file_transcription_controller.statuses())
}

#[tauri::command]
fn cancel_file_transcription(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    state
        .file_transcription_controller
        .cancel(&job_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_recording_status(
    state: tauri::State<'_, AppState>,
) -> Result<RecordingStatusResponse, String> {
    state
        .recording_controller
        .status()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_recording_input_level(state: tauri::State<'_, AppState>) -> Result<f32, String> {
    state
        .recording_controller
        .input_level()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_recording_session(
    state: tauri::State<'_, AppState>,
    feedback: Option<bool>,
) -> Result<RecordingStatusResponse, String> {
    let work = shutdown::begin_work(true).map_err(|e| e.to_string())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _work = work;
        shutdown::ensure_running().map_err(|e| e.to_string())?;
        if state
            .recording_controller
            .status()
            .map(|status| {
                matches!(
                    status.state,
                    audio_capture::RecordingOverlayState::Listening
                        | audio_capture::RecordingOverlayState::Paused
                )
            })
            .unwrap_or(false)
        {
            return Err("A recording is already active.".to_string());
        }
        let enabled = storage::get_settings(&state)
            .map(|settings| settings.sounds_enabled)
            .unwrap_or(false)
            && feedback.unwrap_or(true);
        if let Some(player) = state.sound_player.as_ref().as_ref() {
            player
                .prepare_capture(enabled)
                .map_err(|error| error.to_string())?;
        }
        let result = state
            .recording_controller
            .start()
            .map_err(|error| error.to_string());
        if result.is_err() {
            if let Some(player) = state.sound_player.as_ref().as_ref() {
                player.finish_capture(false, true);
            }
        }
        if shutdown::is_shutting_down() {
            let _ = state.recording_controller.cancel();
            return Err("APP_SHUTTING_DOWN: Blabber wird beendet.".into());
        }
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn pause_recording_session(
    state: tauri::State<'_, AppState>,
) -> Result<RecordingStatusResponse, String> {
    state
        .recording_controller
        .pause()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn resume_recording_session(
    state: tauri::State<'_, AppState>,
) -> Result<RecordingStatusResponse, String> {
    let _work = shutdown::begin_work(true).map_err(|e| e.to_string())?;
    state
        .recording_controller
        .resume()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_recording_session(
    state: tauri::State<'_, AppState>,
) -> Result<RecordingResult, String> {
    let work = shutdown::begin_work(true).map_err(|e| e.to_string())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _work = work;
        shutdown::ensure_running().map_err(|e| e.to_string())?;
        let result = state
            .recording_controller
            .stop()
            .map_err(|error| error.to_string());
        shutdown::set_manual_handoff(result.is_ok());
        let enabled = storage::get_settings(&state)
            .map(|settings| settings.sounds_enabled)
            .unwrap_or(false);
        if let Some(player) = state.sound_player.as_ref().as_ref() {
            player.finish_capture(enabled, result.is_err());
        }
        result
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn cancel_recording_session(
    state: tauri::State<'_, AppState>,
) -> Result<RecordingStatusResponse, String> {
    let result = state
        .recording_controller
        .cancel()
        .map_err(|error| error.to_string());
    if result.is_ok() {
        if let Some(player) = state.sound_player.as_ref().as_ref() {
            player.finish_capture(false, true);
        }
    }
    result
}

#[tauri::command]
fn preview_feedback_sound(
    state: tauri::State<'_, AppState>,
    cue: sound::FeedbackCue,
) -> Result<(), String> {
    if !storage::get_settings(state.inner())
        .map_err(|error| error.to_string())?
        .sounds_enabled
    {
        return Err("Enable feedback sounds first.".into());
    }
    let player = state
        .sound_player
        .as_ref()
        .as_ref()
        .ok_or("Sound output unavailable.")?;
    player.preview(cue).map_err(|error| error.to_string())
}

#[tauri::command]
fn report_manual_feedback(app: tauri::AppHandle, operation_id: String, failed: bool) {
    sound::notify(
        &app,
        if failed {
            sound::FeedbackCue::Error
        } else {
            sound::FeedbackCue::Complete
        },
        &format!("manual:{operation_id}"),
    );
}

#[tauri::command]
fn get_quick_dictate_status(
    state: tauri::State<'_, AppState>,
) -> Result<QuickDictationStatusResponse, String> {
    Ok(state.dictation_controller.status())
}

#[tauri::command]
fn reset_quick_dictation(
    state: tauri::State<'_, AppState>,
) -> Result<QuickDictationStatusResponse, String> {
    shutdown::set_manual_handoff(false);
    state
        .dictation_controller
        .force_reset()
        .map_err(|error| error.to_string())
}

/// Snapshot of everything that must be in place for shortcut dictation to work
/// end-to-end. Drives the Home readiness checklist so silent prerequisites
/// (no model, unbound shortcut, missing Accessibility) become visible.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DictationReadiness {
    has_model: bool,
    shortcut_registered: bool,
    auto_paste_enabled: bool,
    // True only when auto-paste is on AND the platform gates keystroke
    // synthesis behind a permission (macOS Accessibility).
    accessibility_required: bool,
    accessibility_granted: bool,
}

#[tauri::command]
fn get_dictation_readiness(
    state: tauri::State<'_, AppState>,
) -> Result<DictationReadiness, String> {
    let settings = storage::get_settings(state.inner()).map_err(|error| error.to_string())?;
    let models = state
        .engine
        .list_models()
        .map_err(|error| error.to_string())?;
    let status = state.dictation_controller.status();
    let auto_paste = matches!(settings.insert_behavior, InsertBehavior::Paste);
    Ok(DictationReadiness {
        has_model: !models.is_empty(),
        shortcut_registered: status.is_registered,
        auto_paste_enabled: auto_paste,
        accessibility_required: auto_paste && cfg!(target_os = "macos"),
        accessibility_granted: insertion::accessibility_trusted(),
    })
}

#[tauri::command]
fn open_accessibility_settings() {
    insertion::open_accessibility_settings();
}

#[tauri::command]
fn suspend_shortcut_capture(
    state: tauri::State<'_, AppState>,
) -> Result<QuickDictationStatusResponse, String> {
    state
        .dictation_controller
        .suspend_shortcut_registration()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn resume_shortcut_capture(
    state: tauri::State<'_, AppState>,
) -> Result<QuickDictationStatusResponse, String> {
    state
        .dictation_controller
        .resume_shortcut_registration()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_vocabulary_terms(state: tauri::State<'_, AppState>) -> Result<Vec<VocabularyTerm>, String> {
    vocabulary::list_vocabulary_terms(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
fn create_vocabulary_term(
    state: tauri::State<'_, AppState>,
    input: CreateVocabularyTermInput,
) -> Result<VocabularyTerm, String> {
    vocabulary::create_vocabulary_term(state.inner(), input).map_err(|error| error.to_string())
}

#[tauri::command]
fn update_vocabulary_term(
    state: tauri::State<'_, AppState>,
    term_id: String,
    input: UpdateVocabularyTermInput,
) -> Result<VocabularyTerm, String> {
    vocabulary::update_vocabulary_term(state.inner(), &term_id, input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_vocabulary_term(
    state: tauri::State<'_, AppState>,
    term_id: String,
) -> Result<(), String> {
    vocabulary::delete_vocabulary_term(state.inner(), &term_id).map_err(|error| error.to_string())
}

fn resolve_model_selection(
    models: Vec<InstalledModel>,
    selected_model_id: Option<&str>,
    profile: settings::ModelProfile,
) -> Option<InstalledModel> {
    if let Some(model_id) = selected_model_id {
        if let Some(model) = models.iter().find(|model| model.id == model_id) {
            return Some(model.clone());
        }
    }

    models
        .iter()
        .find(|model| model.profile == profile && model.is_default)
        .or_else(|| models.iter().find(|model| model.profile == profile))
        .cloned()
}

fn main() {
    if std::env::args().any(|arg| arg == transcription_worker::WORKER_ARG) {
        std::process::exit(transcription_worker::run_stdio_worker());
    }
    if std::env::args().any(|arg| arg == diarization_worker::WORKER_ARG) {
        std::process::exit(diarization_worker::run_stdio_worker());
    }

    // On Linux, handle `blabber --dictate-toggle` before Tauri initialises.
    // This connects to the running instance's IPC socket, sends a toggle
    // command, and exits — no window is ever opened.
    #[cfg(target_os = "linux")]
    if std::env::args().any(|arg| arg == "--dictate-toggle") {
        std::process::exit(ipc::send_toggle_command());
    }

    tauri::Builder::default()
        .manage(StartupCoordinator::new())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            shutdown::install_macos_termination_handler(app.handle())?;
            let startup_work = shutdown::begin_work(false)?;
            let app_handle = app.handle().clone();
            let startup = app.state::<StartupCoordinator>().inner().clone();
            tauri::async_runtime::spawn_blocking(move || {
                let _startup_work = startup_work;
                let progress_app = app_handle.clone();
                let progress_startup = startup.clone();
                match AppState::initialize(&app_handle, move |phase| {
                    progress_startup.advance(&progress_app, phase);
                }) {
                    Ok(app_state) => {
                        // Start the single-instance IPC listener on Linux so that
                        // subsequent `blabber --dictate-toggle` invocations can reach us.
                        #[cfg(target_os = "linux")]
                        ipc::start_ipc_listener(app_state.dictation_controller.clone());

                        if app_handle.manage(app_state) {
                            startup.advance(&app_handle, StartupPhase::Workspace);
                        } else {
                            startup.fail(&app_handle, "Application state was already initialized.");
                        }
                    }
                    Err(error) => startup.fail(&app_handle, format!("{error:#}")),
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }

            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        let _ = state.desktop_shell.handle_main_window_close(window);
                    }
                }
                WindowEvent::DragDrop(drag_event) => {
                    let app = window.app_handle();
                    match drag_event {
                        DragDropEvent::Enter { paths, .. } => {
                            let _ = app.emit("app://file-drag-enter", paths);
                        }
                        DragDropEvent::Over { .. } => {}
                        DragDropEvent::Drop { paths, .. } => {
                            let _ = app.emit("app://file-drop", paths);
                        }
                        DragDropEvent::Leave => {
                            let _ = app.emit("app://file-drag-leave", ());
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        })
        .on_webview_event(|webview, event| {
            if let WebviewEvent::DragDrop(drag_event) = event {
                let app = webview.app_handle();
                match drag_event {
                    DragDropEvent::Enter { paths, .. } => {
                        let _ = app.emit("app://file-drag-enter", paths);
                    }
                    DragDropEvent::Over { .. } => {}
                    DragDropEvent::Drop { paths, .. } => {
                        let _ = app.emit("app://file-drop", paths);
                    }
                    DragDropEvent::Leave => {
                        let _ = app.emit("app://file-drag-leave", ());
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            health_check,
            get_platform_info,
            quit_app,
            is_app_shutting_down,
            restart_app,
            get_startup_status,
            frontend_startup_complete,
            report_startup_failure,
            complete_startup_handoff,
            dictate_press,
            dictate_release,
            dictate_toggle,
            get_settings,
            update_settings,
            list_transcripts,
            delete_transcript,
            delete_all_transcripts,
            get_transcript,
            get_review,
            edit_review,
            start_review_job,
            get_review_job_statuses,
            cancel_review_job,
            resolve_review_audio,
            release_review_audio,
            copy_review,
            export_review,
            get_file_transcription_result,
            dismiss_file_transcription,
            rediarize_transcript,
            cancel_rediarization,
            rename_transcript,
            rename_transcript_speaker,
            copy_transcript,
            export_transcript,
            copy_text_to_clipboard,
            list_installed_models,
            list_downloadable_models,
            get_model_download_statuses,
            start_model_download,
            cancel_model_download,
            list_input_devices,
            open_models_folder,
            get_dictation_overlay_status,
            preview_transcription,
            pick_audio_files,
            prepare_dropped_audio_files,
            start_file_transcription,
            get_file_transcription_statuses,
            cancel_file_transcription,
            get_recording_status,
            get_recording_input_level,
            start_recording_session,
            preview_feedback_sound,
            report_manual_feedback,
            pause_recording_session,
            resume_recording_session,
            stop_recording_session,
            cancel_recording_session,
            get_quick_dictate_status,
            reset_quick_dictation,
            get_dictation_readiness,
            open_accessibility_settings,
            suspend_shortcut_capture,
            resume_shortcut_capture,
            list_vocabulary_terms,
            create_vocabulary_term,
            update_vocabulary_term,
            delete_vocabulary_term
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            if let tauri::RunEvent::ExitRequested { ref api, .. } = _event {
                if !shutdown::ready_to_exit() {
                    api.prevent_exit();
                    shutdown::request_exit(_app, shutdown::ExitAction::Quit);
                }
            }
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                // A visible dictation overlay does not mean the workspace is
                // open. Always restore it when macOS requests a Dock reopen.
                if let Err(error) = desktop_shell::show_main_window(_app) {
                    eprintln!("[desktop] could not reopen Blabber from the Dock: {error:#}");
                }
            }
        });
}

fn shortcut_unsupported_message() -> &'static str {
    if cfg!(target_os = "linux") && platform::is_wayland() {
        "Global shortcuts are inactive in this Wayland session. Use the command shown in Settings as a compositor shortcut."
    } else {
        "Global shortcuts are not supported on this platform."
    }
}
