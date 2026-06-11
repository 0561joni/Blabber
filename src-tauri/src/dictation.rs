use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::asr::{FileTranscriptionRequest, TranscriptionEngine};
use crate::audio_capture::RecordingController;
use crate::desktop_shell::{DesktopShellController, DictationOverlayPayload, OverlayPhase};
use crate::insertion::{detect_frontmost_paste_target, insert_text, InsertionOutcome, PasteTarget};
use crate::settings::{AppSettings, ShortcutMode};
use crate::sound::SoundPlayer;
use crate::storage;
use crate::system_volume::{self, VolumeSnapshot};
use crate::vocabulary;

const QUICK_DICTATE_STATUS_EVENT: &str = "quick-dictate-status";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuickDictationState {
    Idle,
    Listening,
    Processing,
    Inserted,
    ClipboardOnly,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickDictationStatusResponse {
    pub state: QuickDictationState,
    pub registered_shortcut: Option<String>,
    pub shortcut_mode: ShortcutMode,
    pub is_registered: bool,
    pub last_transcript_text: Option<String>,
    pub last_transcript_id: Option<String>,
    pub last_recording_path: Option<String>,
    pub last_error_message: Option<String>,
    pub last_model_name: Option<String>,
    pub last_insert_outcome: Option<InsertionOutcome>,
    pub last_duration_ms: Option<i64>,
}

impl Default for QuickDictationStatusResponse {
    fn default() -> Self {
        Self {
            state: QuickDictationState::Idle,
            registered_shortcut: None,
            shortcut_mode: ShortcutMode::PushToTalk,
            is_registered: false,
            last_transcript_text: None,
            last_transcript_id: None,
            last_recording_path: None,
            last_error_message: None,
            last_model_name: None,
            last_insert_outcome: None,
            last_duration_ms: None,
        }
    }
}

#[derive(Clone)]
pub struct QuickDictationController {
    app: AppHandle,
    engine: Arc<dyn TranscriptionEngine>,
    recording_controller: RecordingController,
    db_path: std::path::PathBuf,
    desktop_shell: DesktopShellController,
    sound_player: Arc<Option<SoundPlayer>>,
    status: Arc<Mutex<QuickDictationStatusResponse>>,
    is_suspended: Arc<Mutex<bool>>,
    registered_shortcut: Arc<Mutex<Option<String>>>,
    paste_target: Arc<Mutex<Option<PasteTarget>>>,
    volume_snapshot: Arc<Mutex<Option<VolumeSnapshot>>>,
    // Set when the next dictation was triggered from the in-app PTT button
    // instead of the global shortcut. Forces ClipboardOnly because Blabber
    // itself has focus, so auto-paste would target our own window.
    force_clipboard_only: Arc<AtomicBool>,
}

impl QuickDictationController {
    pub fn new(
        app: AppHandle,
        engine: Arc<dyn TranscriptionEngine>,
        recording_controller: RecordingController,
        db_path: std::path::PathBuf,
        desktop_shell: DesktopShellController,
        sound_player: Arc<Option<SoundPlayer>>,
    ) -> Self {
        Self {
            app,
            engine,
            recording_controller,
            db_path,
            desktop_shell,
            sound_player,
            status: Arc::new(Mutex::new(QuickDictationStatusResponse::default())),
            is_suspended: Arc::new(Mutex::new(false)),
            registered_shortcut: Arc::new(Mutex::new(None)),
            paste_target: Arc::new(Mutex::new(None)),
            volume_snapshot: Arc::new(Mutex::new(None)),
            force_clipboard_only: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn status(&self) -> QuickDictationStatusResponse {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    pub fn sync_shortcut_registration(&self) -> Result<QuickDictationStatusResponse> {
        let settings = storage::get_settings_from_db_path(&self.db_path)?;
        self.app.global_shortcut().unregister_all()?;

        let is_suspended = *self
            .is_suspended
            .lock()
            .map_err(|_| anyhow!("shortcut state unavailable"))?;
        if is_suspended {
            self.update_status(|status| {
                status.registered_shortcut = Some(settings.shortcut.clone());
                status.shortcut_mode = settings.shortcut_mode;
                status.is_registered = false;
            })?;
            return Ok(self.status());
        }

        let controller = self.clone();
        self.app.global_shortcut().on_shortcut(
            settings.shortcut.as_str(),
            move |_app, _shortcut, event| {
                let _ = controller.handle_shortcut_event(event.state(), event.id);
            },
        )?;

        if let Ok(mut registered) = self.registered_shortcut.lock() {
            *registered = Some(settings.shortcut.clone());
        }
        self.update_status(|status| {
            status.registered_shortcut = Some(settings.shortcut.clone());
            status.shortcut_mode = settings.shortcut_mode;
            status.is_registered = true;
            status.last_error_message = None;
        })?;
        Ok(self.status())
    }

    pub fn mark_shortcut_unsupported(
        &self,
        message: impl Into<String>,
    ) -> Result<QuickDictationStatusResponse> {
        let settings = storage::get_settings_from_db_path(&self.db_path)?;
        let message = message.into();
        self.app.global_shortcut().unregister_all()?;
        self.update_status(|status| {
            status.registered_shortcut = Some(settings.shortcut.clone());
            status.shortcut_mode = settings.shortcut_mode;
            status.is_registered = false;
            status.last_error_message = Some(message.clone());
        })?;
        Ok(self.status())
    }

    pub fn suspend_shortcut_registration(&self) -> Result<QuickDictationStatusResponse> {
        if let Ok(mut suspended) = self.is_suspended.lock() {
            *suspended = true;
        }
        self.app.global_shortcut().unregister_all()?;
        self.update_status(|status| {
            status.is_registered = false;
        })?;
        Ok(self.status())
    }

    pub fn resume_shortcut_registration(&self) -> Result<QuickDictationStatusResponse> {
        if let Ok(mut suspended) = self.is_suspended.lock() {
            *suspended = false;
        }
        self.sync_shortcut_registration()
    }

    fn handle_shortcut_event(
        &self,
        shortcut_state: ShortcutState,
        _shortcut_id: u32,
    ) -> Result<()> {
        let settings = storage::get_settings_from_db_path(&self.db_path)?;
        match settings.shortcut_mode {
            ShortcutMode::PushToTalk => match shortcut_state {
                ShortcutState::Pressed => self.begin_listening(),
                ShortcutState::Released => self.finish_dictation(),
            },
            ShortcutMode::Toggle => {
                if shortcut_state == ShortcutState::Pressed {
                    if self.status().state == QuickDictationState::Listening {
                        self.finish_dictation()
                    } else {
                        self.begin_listening()
                    }
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Trigger dictation from the in-app push-to-talk button.
    /// Behaves like the global shortcut press, but forces ClipboardOnly
    /// because Blabber's window is focused (so auto-paste would target
    /// Blabber itself instead of the user's previous app).
    pub fn ui_press(&self) -> Result<()> {
        self.force_clipboard_only.store(true, Ordering::SeqCst);
        self.begin_listening()
    }

    /// Release counterpart for `ui_press`.
    pub fn ui_release(&self) -> Result<()> {
        self.finish_dictation()
    }

    /// Toggle dictation from an external trigger — e.g. `blabber --dictate-toggle`
    /// delivered via the IPC socket, or the `dictate_toggle` Tauri command.
    ///
    /// Behaves like a Toggle-mode shortcut press: starts listening if idle,
    /// stops if already listening.  Unlike [`Self::ui_press`], this does NOT
    /// set `force_clipboard_only` because the trigger always originates from
    /// outside Blabber (the user has focus in another app), so auto-paste is
    /// the right behaviour.
    pub fn ui_toggle(&self) -> Result<()> {
        if self.status().state == QuickDictationState::Listening {
            self.finish_dictation()
        } else {
            self.begin_listening()
        }
    }

    fn begin_listening(&self) -> Result<()> {
        if self.status().state == QuickDictationState::Listening {
            return Ok(());
        }

        let settings = storage::get_settings_from_db_path(&self.db_path).ok();
        if settings
            .as_ref()
            .map(|settings| settings.sounds_enabled)
            .unwrap_or(true)
        {
            if let Some(player) = (*self.sound_player).as_ref() {
                player.play_listen_start();
            }
        }

        if settings
            .as_ref()
            .map(|settings| settings.volume_ducking_enabled)
            .unwrap_or(false)
        {
            self.duck_system_volume();
        }

        if let Ok(mut paste_target) = self.paste_target.lock() {
            *paste_target = detect_frontmost_paste_target();
        }

        if let Err(error) = self.recording_controller.start() {
            self.restore_system_volume();
            return Err(error);
        }
        self.desktop_shell
            .set_overlay_payload(DictationOverlayPayload {
                phase: OverlayPhase::Listening,
                audio_level: 0.0,
            })?;
        self.update_status(|status| {
            status.state = QuickDictationState::Listening;
            status.last_error_message = None;
            status.last_transcript_text = None;
            status.last_transcript_id = None;
            status.last_insert_outcome = None;
        })?;
        self.spawn_overlay_level_poller();
        Ok(())
    }

    fn finish_dictation(&self) -> Result<()> {
        if self.status().state != QuickDictationState::Listening {
            self.restore_system_volume();
            return Ok(());
        }
        self.restore_system_volume();
        self.play_listen_stop_feedback();

        self.desktop_shell
            .set_overlay_payload(DictationOverlayPayload {
                phase: OverlayPhase::Processing,
                audio_level: 0.0,
            })?;
        self.update_status(|status| {
            status.state = QuickDictationState::Processing;
            status.last_error_message = None;
        })?;

        let controller = self.clone();
        thread::spawn(move || {
            let _ = controller.finish_dictation_worker();
        });
        Ok(())
    }

    fn finish_dictation_worker(&self) -> Result<()> {
        let recording = match self.recording_controller.stop() {
            Ok(result) => result,
            Err(error) => {
                self.set_error(error.to_string())?;
                return Ok(());
            }
        };

        let settings = storage::get_settings_from_db_path(&self.db_path)?;
        let resolved_model_name = resolve_model_name(self.engine.as_ref(), &settings)?;
        let transcript = match self.engine.transcribe_file(
            FileTranscriptionRequest {
                profile: settings.shortcut_dictation_model_profile,
                selected_model_id: settings.shortcut_dictation_selected_model_id.clone(),
                language_mode: settings.language_mode,
                fixed_language: settings.fixed_language.clone(),
                timestamps: false,
                prefer_gpu: settings.gpu_enabled,
                file_path: recording.file_path.clone(),
            },
            None,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.set_error(error.to_string())?;
                return Ok(());
            }
        };

        let corrected = match vocabulary::correct_transcript_result(&self.db_path, transcript) {
            Ok(result) => result,
            Err(error) => {
                self.set_error(error.to_string())?;
                return Ok(());
            }
        };

        let mut saved_transcript = if settings.save_history {
            storage::save_quick_dictation_transcript(
                &self.db_path,
                &corrected,
                recording.duration_ms,
            )
            .ok()
        } else {
            None
        };

        let force_clipboard = self.force_clipboard_only.swap(false, Ordering::SeqCst);
        let effective_behavior = if force_clipboard {
            crate::settings::InsertBehavior::ClipboardOnly
        } else {
            settings.insert_behavior
        };

        let insert_outcome = match self
            .perform_insertion_on_main_thread(corrected.plain_text.clone(), effective_behavior)
        {
            Ok(outcome) => outcome,
            Err(error) => {
                if saved_transcript.is_none() {
                    saved_transcript = storage::save_quick_dictation_transcript(
                        &self.db_path,
                        &corrected,
                        recording.duration_ms,
                    )
                    .ok();
                }
                self.update_status(|status| {
                    status.last_transcript_text = Some(corrected.plain_text.clone());
                    status.last_transcript_id =
                        saved_transcript.as_ref().map(|item| item.id.clone());
                    status.last_recording_path = Some(recording.file_path.clone());
                    status.last_model_name = resolved_model_name.clone();
                    status.last_duration_ms = Some(recording.duration_ms);
                })?;
                self.set_error(error.to_string())?;
                return Ok(());
            }
        };

        if saved_transcript.is_none() && matches!(insert_outcome, InsertionOutcome::ClipboardOnly) {
            saved_transcript = storage::save_quick_dictation_transcript(
                &self.db_path,
                &corrected,
                recording.duration_ms,
            )
            .ok();
        }

        let next_state = match insert_outcome {
            InsertionOutcome::Pasted => QuickDictationState::Inserted,
            InsertionOutcome::ClipboardOnly => QuickDictationState::ClipboardOnly,
        };
        let result_phase = match insert_outcome {
            InsertionOutcome::Pasted => OverlayPhase::Inserted,
            InsertionOutcome::ClipboardOnly => OverlayPhase::ClipboardOnly,
        };
        self.update_status(|status| {
            status.state = next_state;
            status.last_transcript_text = Some(corrected.plain_text.clone());
            status.last_transcript_id = saved_transcript.as_ref().map(|item| item.id.clone());
            status.last_recording_path = Some(recording.file_path.clone());
            status.last_error_message = None;
            status.last_model_name = resolved_model_name.clone();
            status.last_insert_outcome = Some(insert_outcome);
            status.last_duration_ms = Some(recording.duration_ms);
        })?;
        // Flash the result on the overlay so users get feedback even when
        // they're focused on a different app (the in-window toast can't reach
        // them there). The hide is scheduled in `schedule_idle_reset`.
        self.desktop_shell
            .set_overlay_payload(DictationOverlayPayload {
                phase: result_phase,
                audio_level: 0.0,
            })?;
        self.schedule_idle_reset();
        Ok(())
    }

    fn perform_insertion_on_main_thread(
        &self,
        text: String,
        behavior: crate::settings::InsertBehavior,
    ) -> Result<InsertionOutcome> {
        let app = self.app.clone();
        let desktop_shell = self.desktop_shell.clone();
        let paste_target = self
            .paste_target
            .lock()
            .ok()
            .and_then(|target| target.clone());
        let (response_tx, response_rx) = mpsc::channel();
        self.app.run_on_main_thread(move || {
            let _ = desktop_shell.set_overlay_payload(DictationOverlayPayload::default());
            let result = insert_text(&app, &text, behavior, paste_target.as_ref())
                .map_err(|error| error.to_string());
            let _ = response_tx.send(result);
        })?;

        response_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| anyhow!("timed out while inserting shortcut dictation"))?
            .map_err(anyhow::Error::msg)
    }

    fn spawn_overlay_level_poller(&self) {
        let controller = self.clone();
        thread::spawn(move || {
            while controller.status().state == QuickDictationState::Listening {
                let level = controller.recording_controller.input_level().unwrap_or(0.0);
                let _ = controller
                    .desktop_shell
                    .set_overlay_payload(DictationOverlayPayload {
                        phase: OverlayPhase::Listening,
                        audio_level: level,
                    });
                thread::sleep(Duration::from_millis(50));
            }
        });
    }

    fn schedule_idle_reset(&self) {
        let controller = self.clone();
        thread::spawn(move || {
            // Long enough to read the result chip; short enough to feel snappy
            // and not block subsequent dictations.
            thread::sleep(Duration::from_millis(1800));
            let current = controller.status();
            if matches!(
                current.state,
                QuickDictationState::Inserted
                    | QuickDictationState::ClipboardOnly
                    | QuickDictationState::Error
            ) {
                let _ = controller
                    .desktop_shell
                    .set_overlay_payload(DictationOverlayPayload::default());
                let _ = controller.update_status(|status| {
                    status.state = QuickDictationState::Idle;
                });
            }
        });
    }

    fn set_error(&self, message: String) -> Result<()> {
        self.restore_system_volume();
        self.desktop_shell
            .set_overlay_payload(DictationOverlayPayload {
                phase: OverlayPhase::Failed,
                audio_level: 0.0,
            })?;
        self.update_status(|status| {
            status.state = QuickDictationState::Error;
            status.last_error_message = Some(message.clone());
        })?;
        // Auto-hide the error chip too — same path as success outcomes.
        self.schedule_idle_reset();
        Ok(())
    }

    fn update_status(
        &self,
        mut apply: impl FnMut(&mut QuickDictationStatusResponse),
    ) -> Result<()> {
        let next_status = {
            let mut status = self
                .status
                .lock()
                .map_err(|_| anyhow!("quick dictation status unavailable"))?;
            apply(&mut status);
            status.clone()
        };
        self.app.emit(QUICK_DICTATE_STATUS_EVENT, next_status)?;
        Ok(())
    }

    fn duck_system_volume(&self) {
        if self
            .volume_snapshot
            .lock()
            .map(|snapshot| snapshot.is_some())
            .unwrap_or(true)
        {
            return;
        }

        match system_volume::duck_to_30_percent_of_current() {
            Ok(Some(snapshot)) => {
                if let Ok(mut current) = self.volume_snapshot.lock() {
                    *current = Some(snapshot);
                } else if let Err(error) = system_volume::restore(snapshot) {
                    eprintln!("[volume] restore after state lock failure failed: {error:?}");
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("[volume] ducking failed: {error:?}");
            }
        }
    }

    fn restore_system_volume(&self) {
        let snapshot = self
            .volume_snapshot
            .lock()
            .ok()
            .and_then(|mut snapshot| snapshot.take());

        if let Some(snapshot) = snapshot {
            if let Err(error) = system_volume::restore(snapshot) {
                eprintln!("[volume] restore failed: {error:?}");
            }
        }
    }

    fn play_listen_stop_feedback(&self) {
        let sounds_enabled = storage::get_settings_from_db_path(&self.db_path)
            .map(|settings| settings.sounds_enabled)
            .unwrap_or(true);
        if sounds_enabled {
            if let Some(player) = (*self.sound_player).as_ref() {
                player.play_listen_stop();
            }
        }
    }
}

fn resolve_model_name(
    engine: &dyn TranscriptionEngine,
    settings: &AppSettings,
) -> Result<Option<String>> {
    let models = engine.list_models()?;
    Ok(
        if let Some(model_id) = settings.shortcut_dictation_selected_model_id.as_deref() {
            models
                .iter()
                .find(|model| model.id == model_id)
                .map(|model| model.model_name.clone())
        } else {
            models
                .iter()
                .find(|model| {
                    model.profile == settings.shortcut_dictation_model_profile && model.is_default
                })
                .or_else(|| {
                    models
                        .iter()
                        .find(|model| model.profile == settings.shortcut_dictation_model_profile)
                })
                .map(|model| model.model_name.clone())
        },
    )
}
