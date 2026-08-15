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
mod platform;
mod qwen_asr;
mod settings;
mod sound;
mod speaker_reconciliation;
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
use serde::Serialize;
use settings::{AppSettings, HealthCheckResponse, InsertBehavior, SettingsPatch};
use std::process::Command;
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
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
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
    if (patch.file_diarization_enabled == Some(true)
        || patch.quick_dictate_diarization_enabled == Some(true))
        && model_downloads::installed_diarization_package_path(&state.models_dir).is_none()
    {
        return Err(
            "Install a verified diarization model package before enabling speaker identification."
                .to_string(),
        );
    }
    let settings =
        storage::update_settings(state.inner(), patch).map_err(|error| error.to_string())?;
    state
        .recording_controller
        .set_preferred_input_device(settings.preferred_input_device.clone());
    autostart::sync_launch_at_login(&app, settings.launch_at_login_enabled)
        .map_err(|error| error.to_string())?;
    if platform::global_shortcut_supported() {
        state
            .dictation_controller
            .sync_shortcut_registration()
            .map_err(|error| error.to_string())?;
    } else {
        state
            .dictation_controller
            .mark_shortcut_unsupported(shortcut_unsupported_message())
            .map_err(|error| error.to_string())?;
    }
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
    storage::get_transcript(state.inner(), &transcript_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn rename_transcript(
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    title: String,
) -> Result<TranscriptSummary, String> {
    storage::rename_transcript(state.inner(), &transcript_id, &title)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn rename_transcript_speaker(
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    speaker_id: String,
    display_name: String,
) -> Result<TranscriptDetail, String> {
    storage::rename_transcript_speaker(state.inner(), &transcript_id, &speaker_id, &display_name)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn copy_transcript(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    variant: TranscriptCopyVariant,
) -> Result<(), String> {
    let detail = storage::get_transcript(state.inner(), &transcript_id)
        .map_err(|error| error.to_string())?;
    transcript_commands::copy(&app, &detail, variant).map_err(|error| error.to_string())
}

#[tauri::command]
fn export_transcript(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    transcript_id: String,
    format: TranscriptExportFormat,
) -> Result<TranscriptExportResult, String> {
    let detail = storage::get_transcript(state.inner(), &transcript_id)
        .map_err(|error| error.to_string())?;
    transcript_commands::export(&app, &detail, format).map_err(|error| error.to_string())
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
    let app_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
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

        let vocabulary_prompt = vocabulary::build_asr_prompt_from_db_path(
            &app_state.db_path,
            request.language_mode,
            request.fixed_language.as_deref(),
        )
        .map_err(|error| error.to_string())?;

        let result = app_state.engine.transcribe_file(
            FileTranscriptionRequest {
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
                let mut corrected =
                    vocabulary::correct_transcript_result(&app_state.db_path, result)
                        .map_err(|error| error.to_string())?;
                if request.source_kind == asr::PreviewSourceKind::QuickDictate {
                    let settings = storage::get_settings_from_db_path(&app_state.db_path)
                        .map_err(|error| error.to_string())?;
                    if settings.quick_dictate_diarization_enabled {
                        if let Some(package_path) =
                            model_downloads::installed_diarization_package_path(
                                &app_state.models_dir,
                            )
                        {
                            let worker_request = diarization_worker::WorkerRequest {
                                job_id: corrected.job_id.clone(),
                                audio_path: file_path.into(),
                                package_path,
                                exact_speaker_count: settings.diarization_speaker_count,
                                spec_version: diarization::DIARIZATION_MODEL_SPEC_V1
                                    .manifest_version,
                            };
                            match diarization_worker::run_subprocess_worker(&worker_request, None) {
                                Ok(turns) => {
                                    diarization::apply_turns_to_transcript(&mut corrected, turns)
                                }
                                Err(error) => {
                                    diarization::mark_failure(&mut corrected, error.to_string())
                                }
                            }
                        } else {
                            diarization::mark_failure(
                                &mut corrected,
                                "The verified diarization model package is not installed.",
                            );
                        }
                    }
                }
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
fn start_recording_session(
    state: tauri::State<'_, AppState>,
) -> Result<RecordingStatusResponse, String> {
    state
        .recording_controller
        .start()
        .map_err(|error| error.to_string())
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
    state
        .recording_controller
        .resume()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn stop_recording_session(
    state: tauri::State<'_, AppState>,
) -> Result<RecordingResult, String> {
    let recording_controller = state.recording_controller.clone();
    tauri::async_runtime::spawn_blocking(move || {
        recording_controller
            .stop()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn cancel_recording_session(
    state: tauri::State<'_, AppState>,
) -> Result<RecordingStatusResponse, String> {
    state
        .recording_controller
        .cancel()
        .map_err(|error| error.to_string())
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let app_state = AppState::initialize(app.handle())?;

            // Start the single-instance IPC listener on Linux so that
            // subsequent `blabber --dictate-toggle` invocations can reach us.
            #[cfg(target_os = "linux")]
            ipc::start_ipc_listener(app_state.dictation_controller.clone());

            app.manage(app_state);
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
            dictate_press,
            dictate_release,
            dictate_toggle,
            get_settings,
            update_settings,
            list_transcripts,
            delete_transcript,
            delete_all_transcripts,
            get_transcript,
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn shortcut_unsupported_message() -> &'static str {
    if cfg!(target_os = "linux") && platform::is_wayland() {
        "Global shortcuts are inactive in this Wayland session. Use the command shown in Settings as a compositor shortcut."
    } else {
        "Global shortcuts are not supported on this platform."
    }
}
