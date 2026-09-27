use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::asr::{FileTranscriptionRequest, TranscriptionEngine};
use crate::audio_capture::RecordingController;
use crate::desktop_shell::{DesktopShellController, DictationOverlayPayload, OverlayPhase};
use crate::insertion::{
    detect_frontmost_paste_target, InsertionOutcome, InsertionReport, PasteTarget,
};
use crate::settings::{AppSettings, ShortcutMode};
use crate::sound::SoundPlayer;
use crate::storage;
use crate::system_volume::{self, VolumeSnapshot};
use crate::vocabulary;

const QUICK_DICTATE_STATUS_EVENT: &str = "quick-dictate-status";
/// Shortcut dictation stops and transcribes automatically at five minutes,
/// matching the live R2T2 limit. The overlay warns 30 seconds before.
pub(crate) const DICTATION_CAPTURE_LIMIT_MS: u64 = 300_000;
const DICTATION_LIMIT_WARNING_MS: u64 = 270_000;

fn shortcut_pair(primary: &str, secondary: Option<&str>) -> Result<Vec<String>> {
    let first = Shortcut::from_str(primary)?;
    let mut pair = vec![primary.to_owned()];
    if let Some(value) = secondary {
        if first.id() == Shortcut::from_str(value)?.id() {
            return Err(anyhow!(
                "Dictation and language switching need different shortcuts."
            ));
        }
        pair.push(value.to_owned());
    }
    Ok(pair)
}

fn cycle_key_transition(pressed: &AtomicBool, event: ShortcutState) -> bool {
    match event {
        ShortcutState::Released => {
            pressed.store(false, Ordering::SeqCst);
            false
        }
        ShortcutState::Pressed => !pressed.swap(true, Ordering::SeqCst),
    }
}

fn insertion_behavior_for_session(
    configured: crate::settings::InsertBehavior,
    session_id: &str,
    clipboard_only_session: Option<&str>,
) -> crate::settings::InsertBehavior {
    if clipboard_only_session == Some(session_id) {
        crate::settings::InsertBehavior::ClipboardOnly
    } else {
        configured
    }
}

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
    pub last_insert_warning: Option<String>,
    pub last_duration_ms: Option<i64>,
    pub can_retry_streaming: bool,
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
            last_insert_warning: None,
            last_duration_ms: None,
            can_retry_streaming: false,
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
    // The in-app PTT session must copy because Blabber itself has focus.
    // Bind the override to that session so failed UI attempts cannot change
    // insertion behavior for an active or later global-shortcut recording.
    clipboard_only_session: Arc<Mutex<Option<String>>>,
    // Bumped on every `begin_listening`. The overlay-level poller captures the
    // generation it was spawned for and exits as soon as it is superseded, so
    // exactly one poller runs at a time (no zombie threads on intensive use).
    poller_generation: Arc<AtomicU64>,
    // Unix-millis timestamp of the last state transition, used by the watchdog
    // to detect a dictation that has been stuck in Listening/Processing.
    state_since_ms: Arc<AtomicI64>,
    translation: crate::translation::TranslationService,
    shortcut_pair: Arc<Mutex<Vec<String>>>,
    cycle_pressed: Arc<AtomicBool>,
    live: Arc<Mutex<Option<LiveDictation>>>,
    recovery: Arc<Mutex<Option<LiveRecovery>>>,
}

#[derive(Clone)]
struct LiveDictation {
    session_id: String,
    settings: AppSettings,
    vocabulary: Vec<vocabulary::VocabularyTerm>,
    handle: crate::r2t2::LiveSession,
}
#[derive(Clone)]
struct LiveRecovery {
    recording: crate::audio_capture::RecordingResult,
    settings: AppSettings,
    vocabulary: Vec<vocabulary::VocabularyTerm>,
}

impl QuickDictationController {
    pub fn new(
        app: AppHandle,
        engine: Arc<dyn TranscriptionEngine>,
        recording_controller: RecordingController,
        db_path: std::path::PathBuf,
        desktop_shell: DesktopShellController,
        sound_player: Arc<Option<SoundPlayer>>,
        translation: crate::translation::TranslationService,
    ) -> Self {
        Self {
            app,
            translation,
            shortcut_pair: Default::default(),
            cycle_pressed: Default::default(),
            live: Default::default(),
            recovery: Default::default(),
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
            clipboard_only_session: Default::default(),
            poller_generation: Arc::new(AtomicU64::new(0)),
            state_since_ms: Arc::new(AtomicI64::new(now_ms())),
        }
    }

    pub fn status(&self) -> QuickDictationStatusResponse {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    fn register_pair(&self, pair: &[String]) -> Result<()> {
        for (index, shortcut) in pair.iter().enumerate() {
            let controller = self.clone();
            self.app.global_shortcut().on_shortcut(
                shortcut.as_str(),
                move |_app, _shortcut, event| {
                    if controller
                        .is_suspended
                        .lock()
                        .map(|value| *value)
                        .unwrap_or(true)
                    {
                        return;
                    }
                    if index == 1 {
                        if cycle_key_transition(&controller.cycle_pressed, event.state()) {
                            if let Err(error) = controller.translation.cycle() {
                                let _ = controller
                                    .app
                                    .emit("dictation-mode-error", error.to_string());
                            }
                        }
                    } else if let Err(error) =
                        controller.handle_shortcut_event(event.state(), event.id)
                    {
                        eprintln!("[dictation] shortcut failed: {error}");
                    }
                },
            )?;
        }
        Ok(())
    }
    pub fn sync_shortcut_registration(&self) -> Result<QuickDictationStatusResponse> {
        let settings = storage::get_settings_from_db_path(&self.db_path)?;
        // A settings/model change must not leave a previous helper resident.
        self.translation.processing_queue().evict_idle();
        let mut registered = self
            .shortcut_pair
            .lock()
            .map_err(|_| anyhow!("Shortcut state unavailable"))?;
        let next = shortcut_pair(
            &settings.shortcut,
            (settings.translation_enabled && crate::translation::platform_supported())
                .then_some(settings.translation_cycle_shortcut.as_str()),
        )?;
        if *self
            .is_suspended
            .lock()
            .map_err(|_| anyhow!("Shortcut state unavailable"))?
        {
            return Ok(self.status());
        }
        let previous = registered.clone();
        self.app.global_shortcut().unregister_all()?;
        if let Err(error) = self.register_pair(&next) {
            let _ = self.app.global_shortcut().unregister_all();
            if let Err(restore) = self.register_pair(&previous) {
                let _ = self.app.global_shortcut().unregister_all();
                registered.clear();
                self.update_status(|status| status.is_registered = false)?;
                return Err(anyhow!(
                    "{error}; could not restore previous shortcuts: {restore}"
                ));
            }
            return Err(error);
        }
        *registered = next;
        self.cycle_pressed.store(false, Ordering::SeqCst);
        *self
            .registered_shortcut
            .lock()
            .map_err(|_| anyhow!("Shortcut state unavailable"))? = Some(settings.shortcut.clone());
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
        if self.translation.snapshot().busy {
            return Err(anyhow!(
                "Finish or cancel dictation before changing shortcuts."
            ));
        }
        let registration = self
            .shortcut_pair
            .lock()
            .map_err(|_| anyhow!("Shortcut state unavailable"))?;
        self.cycle_pressed.store(false, Ordering::SeqCst);
        if let Ok(mut suspended) = self.is_suspended.lock() {
            *suspended = true;
        }
        if let Err(error) = self.app.global_shortcut().unregister_all() {
            if let Ok(mut suspended) = self.is_suspended.lock() {
                *suspended = false;
            }
            let _ = self.app.global_shortcut().unregister_all();
            if let Err(restore) = self.register_pair(&registration) {
                self.update_status(|status| status.is_registered = false)?;
                return Err(anyhow!("{error}; restoring shortcuts failed: {restore}"));
            }
            return Err(error.into());
        }
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
                ShortcutState::Pressed => self.begin_listening(false),
                ShortcutState::Released => self.finish_dictation(),
            },
            ShortcutMode::Toggle => {
                if shortcut_state == ShortcutState::Pressed {
                    if self.status().state == QuickDictationState::Listening {
                        self.finish_dictation()
                    } else {
                        self.begin_listening(false)
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
        self.begin_listening(true)
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
    /// force clipboard-only insertion because the trigger always originates from
    /// outside Blabber (the user has focus in another app), so auto-paste is
    /// the right behaviour.
    pub fn ui_toggle(&self) -> Result<()> {
        if self.status().state == QuickDictationState::Listening {
            self.finish_dictation()
        } else {
            self.begin_listening(false)
        }
    }

    fn begin_listening(&self, from_ui: bool) -> Result<()> {
        let _work = crate::shutdown::begin_work(true)?;
        match self.status().state {
            QuickDictationState::Listening => return Ok(()),
            QuickDictationState::Processing => {
                return Err(anyhow!("Dictation is still transcribing."))
            }
            _ => {}
        }
        if self
            .recording_controller
            .status()
            .map(|status| {
                matches!(
                    status.state,
                    crate::audio_capture::RecordingOverlayState::Listening
                        | crate::audio_capture::RecordingOverlayState::Paused
                )
            })
            .unwrap_or(false)
        {
            return Err(anyhow!("Another recording is already active."));
        }

        let mut session = self.translation.begin()?;
        let generation = {
            let _status = self
                .status
                .lock()
                .map_err(|_| anyhow!("Dictation state unavailable"))?;
            self.poller_generation.fetch_add(1, Ordering::SeqCst) + 1
        };
        let reservation = self.translation.guard(&session);
        let settings = Some(storage::get_settings_from_db_path(&self.db_path)?);
        let live_setup = if settings
            .as_ref()
            .and_then(|s| s.shortcut_dictation_selected_model_id.as_deref())
            == Some(crate::r2t2::MODEL_ID)
        {
            let settings = settings.as_ref().unwrap().clone();
            let (helper, model) = self.translation.r2t2_setup().map_err(|error| {
                let _ = self.set_error(error.to_string());
                error
            })?;
            let language = crate::r2t2::source_language(&settings).map_err(|error| {
                let _ = self.set_error(error.to_string());
                error
            })?;
            let terms = vocabulary::list_vocabulary_terms_from_db_path(&self.db_path)?;
            Some((settings, terms, helper, model, language))
        } else {
            None
        };
        if let Some(player) = self.sound_player.as_ref().as_ref() {
            player.prepare_capture(
                settings
                    .as_ref()
                    .map(|settings| settings.sounds_enabled)
                    .unwrap_or(false),
            )?;
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

        let capture = match self.recording_controller.start() {
            Ok(capture) => capture,
            Err(error) => {
                self.restore_system_volume();
                // Surface the failure (overlay "Failed" + status + log) so a wedged
                // microphone never silently swallows the dictation, then reset.
                let _ = self.set_error(error.to_string());
                return Err(error);
            }
        };
        session.recording_id = capture.current_session_id.clone();
        *self
            .clipboard_only_session
            .lock()
            .map_err(|_| anyhow!("Dictation insertion state unavailable"))? =
            from_ui.then(|| session.id.clone());
        if let Some(id) = &session.recording_id {
            self.translation.bind_recording(id.clone(), None);
        }
        *self
            .recovery
            .lock()
            .map_err(|_| anyhow!("Recovery state unavailable"))? = None;
        if crate::shutdown::is_shutting_down() {
            let _ = self.recording_controller.cancel();
            self.restore_system_volume();
            return Ok(());
        }
        self.desktop_shell
            .set_overlay_payload(DictationOverlayPayload {
                phase: OverlayPhase::Listening,
                audio_level: 0.0,
                session_id: Some(session.id.clone()),
                streaming_state: live_setup
                    .as_ref()
                    .map(|_| crate::desktop_shell::StreamingState::Preparing),
                ..Default::default()
            })?;
        self.update_status(|status| {
            status.state = QuickDictationState::Listening;
            status.last_error_message = None;
            status.last_transcript_text = None;
            status.last_transcript_id = None;
            status.last_insert_outcome = None;
            status.last_insert_warning = None;
            status.can_retry_streaming = false;
        })?;
        // Bump the generation so any previous poller exits, then start the one
        // poller that belongs to this listening session.
        self.spawn_overlay_level_poller(generation, session.id.clone());
        if let Some((settings, terms, helper, model, language)) = live_setup {
            let tap = match self.recording_controller.live_tap() {
                Ok(tap) => tap,
                Err(error) => {
                    let _ = self.recording_controller.cancel();
                    let _ = self.set_error_owned(error.to_string(), generation);
                    return Err(error);
                }
            };
            let controller = self.clone();
            let owner = session.clone();
            let stop = Arc::new(move || {
                if controller.poller_generation.load(Ordering::SeqCst) == generation
                    && controller.translation.ensure_active(&owner).is_ok()
                {
                    let _ = controller.finish_dictation();
                }
            });
            let translation = self.translation.clone();
            let context = vocabulary::build_asr_prompt(&terms)
                .map(|p| p.text)
                .unwrap_or_default();
            let handle = crate::r2t2::start(
                session.id.clone(),
                tap,
                session.cancelled.clone(),
                self.translation.processing_queue(),
                self.desktop_shell.clone(),
                helper,
                model,
                language,
                context,
                Arc::new(move || translation.release_asr_resources()),
                stop,
            )?;
            *self
                .live
                .lock()
                .map_err(|_| anyhow!("Live dictation state unavailable"))? = Some(LiveDictation {
                session_id: session.id.clone(),
                settings,
                vocabulary: terms,
                handle: handle.clone(),
            });
            handle.activate();
        }
        reservation.disarm();
        Ok(())
    }

    fn finish_dictation(&self) -> Result<()> {
        let Ok(session) = self.translation.current() else {
            return Ok(());
        };
        let generation = self.translation.with_active(&session, || {
            let mut status = self
                .status
                .lock()
                .map_err(|_| anyhow!("Dictation state unavailable"))?;
            if status.state != QuickDictationState::Listening {
                return Ok(None);
            }
            status.state = QuickDictationState::Processing;
            status.last_error_message = None;
            self.state_since_ms.store(now_ms(), Ordering::SeqCst);
            drop(status);
            self.restore_system_volume();
            self.desktop_shell
                .processing_for_session(&session.id, "Finishing", false)?;
            self.update_status(|_| {})?;
            Ok(Some(self.poller_generation.load(Ordering::SeqCst)))
        })?;
        let Some(generation) = generation else {
            return Ok(());
        };

        let work = crate::shutdown::begin_work(true)?;
        let controller = self.clone();
        thread::spawn(move || {
            let _work = work;
            if let Err(error) = controller.finish_dictation_worker(generation, session) {
                if controller.poller_generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                eprintln!("[dictation] finish worker failed: {error:?}");
                let _ = controller.set_error_owned(error.to_string(), generation);
            }
        });
        Ok(())
    }

    fn finish_dictation_worker(
        &self,
        generation: u64,
        session: crate::translation::DictationSession,
    ) -> Result<()> {
        let _session_guard = self.translation.guard(&session);
        if self.poller_generation.load(Ordering::SeqCst) != generation {
            return Ok(());
        }
        let recording = match self.recording_controller.stop_owned(
            session
                .recording_id
                .as_deref()
                .ok_or_else(|| anyhow!("Missing capture identity"))?,
        ) {
            Ok(result) => result,
            Err(error) => {
                if let Ok(mut live) = self.live.lock() {
                    if live
                        .as_ref()
                        .is_some_and(|live| live.session_id == session.id)
                    {
                        if let Some(live) = live.take() {
                            live.handle.cancel();
                        }
                    }
                }
                return Err(error);
            }
        };

        if self.poller_generation.load(Ordering::SeqCst) != generation {
            return Ok(());
        }
        let live = {
            let mut live = self
                .live
                .lock()
                .map_err(|_| anyhow!("Live dictation state unavailable"))?;
            if live
                .as_ref()
                .is_some_and(|live| live.session_id == session.id)
            {
                live.take()
            } else {
                None
            }
        };
        if live.is_some() {
            self.translation
                .phase(&session, "streaming_finishing", "Finishing")?;
        }
        let settings = match &live {
            Some(live) => live.settings.clone(),
            None => storage::get_settings_from_db_path(&self.db_path)?,
        };
        // Keep recovery material even when streaming failed before stop. Retry
        // is explicit and must use a copy action rather than the old paste target.
        self.translation.with_active(&session, || {
            self.update_status(|status| {
                status.last_recording_path = Some(recording.file_path.clone());
                status.last_duration_ms = Some(recording.duration_ms);
            })
        })?;
        if let Some(player) = self.sound_player.as_ref().as_ref() {
            player.finish_capture(settings.sounds_enabled, false);
        }
        let resolved_model_name = if live.is_some() {
            Some(crate::r2t2::MODEL_NAME.into())
        } else {
            resolve_model_name(self.engine.as_ref(), &settings)?
        };
        let _permit;
        let corrected = if let Some(live) = live {
            let result = live
                .handle
                .finish(&recording.session_id, recording.sample_count);
            let completed = match result {
                Ok(result) => result,
                Err(error) => {
                    // Store only if this session still owns dictation; a stale
                    // worker cannot replace a newer recording's recovery material.
                    let _ = self.translation.with_active(&session, || {
                        *self
                            .recovery
                            .lock()
                            .map_err(|_| anyhow!("Recovery unavailable"))? = Some(LiveRecovery {
                            recording: recording.clone(),
                            settings: live.settings.clone(),
                            vocabulary: live.vocabulary.clone(),
                        });
                        self.update_status(|status| status.can_retry_streaming = true)?;
                        Ok(())
                    });
                    live.handle.cancel();
                    return Err(error);
                }
            };
            self.translation.ensure_active(&session)?;
            let (text, permit) = completed.release_worker(
                &self.translation.processing_queue(),
                session.mode == crate::translation::OutputMode::Original,
            );
            _permit = permit;
            let language = (settings.language_mode == crate::settings::LanguageMode::Fixed)
                .then(|| settings.fixed_language.clone())
                .flatten();
            vocabulary::correct_transcript_with_terms(
                &live.vocabulary,
                crate::r2t2::transcript(&session.id, text, recording.duration_ms, language),
            )?
        } else {
            let (permit, warm_worker) = self.translation.acquire_for_asr(&session)?;
            _permit = permit;
            let vocabulary_prompt = vocabulary::build_asr_prompt_from_db_path(&self.db_path)?;
            if let Some(prompt) = &vocabulary_prompt {
                eprintln!(
                    "[dictation] dictionary prompt enabled: included={} truncated={}",
                    prompt.included_count, prompt.truncated_count
                );
            }
            let transcript = match self.translation.transcribe(
                &session,
                FileTranscriptionRequest {
                    use_context: Some(crate::model_metadata::ModelUseContext::ShortcutDictation),
                    profile: settings.shortcut_dictation_model_profile,
                    selected_model_id: settings.shortcut_dictation_selected_model_id.clone(),
                    language_mode: settings.language_mode,
                    fixed_language: settings.fixed_language.clone(),
                    timestamps: false,
                    prefer_gpu: settings.gpu_enabled,
                    file_path: recording.file_path.clone(),
                    context_prompt: vocabulary_prompt.as_ref().map(|prompt| prompt.text.clone()),
                    context_terms: vocabulary_prompt
                        .as_ref()
                        .map(|prompt| prompt.terms.clone())
                        .unwrap_or_default(),
                },
                &_permit,
                warm_worker,
            ) {
                Ok(result) => result,
                Err(error) => {
                    return Err(error);
                }
            };

            match vocabulary::correct_transcript_result(&self.db_path, transcript) {
                Ok(result) => result,
                Err(error) => {
                    return Err(error);
                }
            }
        };

        if self.poller_generation.load(Ordering::SeqCst) != generation {
            return Ok(());
        }
        self.translation.with_active(&session, || {
            self.desktop_shell
                .finalize_stream_text(&session.id, corrected.plain_text.clone())
        })?;
        let output = self
            .translation
            .process(&session, &corrected, recording.duration_ms)?;
        self.translation.ensure_active(&session)?;
        let translated = session.mode != crate::translation::OutputMode::Original;
        let mut saved_transcript_id = output.transcript_id.clone();
        // A translation that failed its checks twice falls back to the original.
        let fallback_warning = (output.status != "completed" && output.fallback_to_original)
            .then(|| {
                output
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "Translation check failed. The original was used.".into())
            });
        if output.status != "completed" && fallback_warning.is_none() {
            self.translation.with_active(&session, || {
                self.update_status(|status| {
                    status.last_transcript_text = Some(corrected.plain_text.clone());
                    status.last_transcript_id = saved_transcript_id.clone();
                })
            })?;
            return Err(anyhow!(output
                .error_message
                .unwrap_or_else(|| "Translation failed.".into())));
        }
        let output_text = if fallback_warning.is_some() {
            corrected.plain_text.clone()
        } else {
            output.output_text.unwrap_or_default()
        };
        if output_text.trim().is_empty() {
            self.translation.with_active(&session, || {
                self.update_status(|status| status.state = QuickDictationState::Idle)?;
                self.desktop_shell.set_overlay_payload(Default::default())
            })?;
            return Ok(());
        }
        self.translation.ensure_active(&session)?;
        if !translated && session.save_history {
            saved_transcript_id = self
                .translation
                .with_active(&session, || {
                    storage::save_quick_dictation_transcript(
                        &self.db_path,
                        &corrected,
                        recording.duration_ms,
                    )
                })
                .ok()
                .map(|s| s.id);
        }

        let effective_behavior = insertion_behavior_for_session(
            settings.insert_behavior,
            &session.id,
            self.clipboard_only_session
                .lock()
                .map_err(|_| anyhow!("Dictation insertion state unavailable"))?
                .as_deref(),
        );

        let insert_report = match self.perform_insertion_on_main_thread(
            output_text.clone(),
            effective_behavior,
            generation,
            &session,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                if self.poller_generation.load(Ordering::SeqCst) != generation {
                    return Ok(());
                }
                self.translation.ensure_active(&session)?;
                if saved_transcript_id.is_none() && !translated && session.save_history {
                    saved_transcript_id = self
                        .translation
                        .with_active(&session, || {
                            storage::save_quick_dictation_transcript(
                                &self.db_path,
                                &corrected,
                                recording.duration_ms,
                            )
                        })
                        .ok()
                        .map(|s| s.id);
                }
                self.translation.with_active(&session, || {
                    self.update_status(|status| {
                        status.last_transcript_text = Some(output_text.clone());
                        status.last_transcript_id = saved_transcript_id.clone();
                        status.last_recording_path = Some(recording.file_path.clone());
                        status.last_model_name = resolved_model_name.clone();
                        status.last_duration_ms = Some(recording.duration_ms);
                    })
                })?;
                return Err(error);
            }
        };
        let insert_outcome = insert_report.outcome;

        if self.poller_generation.load(Ordering::SeqCst) != generation {
            return Ok(());
        }
        if saved_transcript_id.is_none()
            && !translated
            && session.save_history
            && matches!(insert_outcome, InsertionOutcome::ClipboardOnly)
        {
            saved_transcript_id = self
                .translation
                .with_active(&session, || {
                    storage::save_quick_dictation_transcript(
                        &self.db_path,
                        &corrected,
                        recording.duration_ms,
                    )
                })
                .ok()
                .map(|s| s.id);
        }

        let next_state = match insert_outcome {
            InsertionOutcome::Pasted => QuickDictationState::Inserted,
            InsertionOutcome::ClipboardOnly => QuickDictationState::ClipboardOnly,
        };
        let result_phase = match insert_outcome {
            InsertionOutcome::Pasted => OverlayPhase::Inserted,
            InsertionOutcome::ClipboardOnly => OverlayPhase::ClipboardOnly,
        };
        self.translation.with_active(&session, || {
            self.update_status(|status| {
                status.state = next_state;
                status.last_transcript_text = Some(output_text.clone());
                status.last_transcript_id = saved_transcript_id.clone();
                status.last_recording_path = Some(recording.file_path.clone());
                status.last_error_message = None;
                status.last_model_name = resolved_model_name.clone();
                status.last_insert_outcome = Some(insert_outcome);
                status.last_insert_warning = insert_report
                    .warning
                    .map(|warning| warning.message().to_string())
                    .or_else(|| fallback_warning.clone());
                status.last_duration_ms = Some(recording.duration_ms);
            })
        })?;
        // Flash the result on the overlay so users get feedback even when
        // they're focused on a different app (the in-window toast can't reach
        // them there). The hide is scheduled in `schedule_idle_reset`.
        self.translation.with_active(&session, || {
            self.desktop_shell
                .set_overlay_payload(DictationOverlayPayload {
                    phase: result_phase,
                    audio_level: 0.0,
                    status_text: insert_report
                        .warning
                        .map(|warning| warning.overlay_label().to_string())
                        .or_else(|| {
                            fallback_warning
                                .as_ref()
                                .map(|_| "Original pasted · translation check failed".to_string())
                        }),
                    duration_limit_reached: recording.duration_ms as u64
                        >= DICTATION_CAPTURE_LIMIT_MS - 1_000,
                    ..Default::default()
                })
        })?;
        crate::sound::notify(
            &self.app,
            crate::sound::FeedbackCue::Complete,
            &format!("dictation:{}", recording.file_path),
        );
        self.schedule_idle_reset();
        Ok(())
    }

    fn perform_insertion_on_main_thread(
        &self,
        text: String,
        behavior: crate::settings::InsertBehavior,
        generation: u64,
        session: &crate::translation::DictationSession,
    ) -> Result<InsertionReport> {
        let app = self.app.clone();
        let desktop_shell = self.desktop_shell.clone();
        let paste_target = self
            .paste_target
            .lock()
            .ok()
            .and_then(|target| target.clone());
        let (response_tx, response_rx) = mpsc::channel();
        let active_generation = self.poller_generation.clone();
        let service = self.translation.clone();
        let session = session.clone();
        let canceled = session.cancelled.clone();
        self.app.run_on_main_thread(move || {
            if crate::shutdown::is_shutting_down()
                || active_generation.load(Ordering::SeqCst) != generation
                || service.ensure_active(&session).is_err()
            {
                let _ = response_tx.send(Err("Dictation was reset.".to_string()));
                return;
            }
            if let Err(error) = service.with_active(&session, || {
                desktop_shell.set_overlay_payload(DictationOverlayPayload::default())
            }) {
                let _ = response_tx.send(Err(error.to_string()));
                return;
            }
            let result =
                crate::insertion::insert_text(&app, &text, behavior, paste_target.as_ref(), || {
                    if active_generation.load(Ordering::SeqCst) != generation {
                        return Err(anyhow!("Dictation was reset."));
                    }
                    service.ensure_active(&session)
                })
                .map_err(|error| error.to_string());
            let _ = response_tx.send(result);
        })?;

        response_rx
            .recv_timeout(Duration::from_secs(5))
            .inspect_err(|_| canceled.store(true, Ordering::SeqCst))
            .map_err(|_| anyhow!("timed out while inserting shortcut dictation"))?
            .map_err(anyhow::Error::msg)
    }

    /// Explicit recovery returns text for the user to copy. It never uses the
    /// original application's paste target or changes the clipboard.
    pub fn retry_streaming(&self) -> Result<QuickDictationStatusResponse> {
        let _work = crate::shutdown::begin_work(true)?;
        let recovery = self
            .recovery
            .lock()
            .map_err(|_| anyhow!("Recovery unavailable"))?
            .clone()
            .ok_or_else(|| anyhow!("No failed live recording is available."))?;
        let session = self.translation.begin()?;
        let _guard = self.translation.guard(&session);
        let generation = {
            let _status = self
                .status
                .lock()
                .map_err(|_| anyhow!("Dictation state unavailable"))?;
            self.poller_generation.fetch_add(1, Ordering::SeqCst) + 1
        };
        let result = (|| -> Result<()> {
            let (helper, model) = self.translation.r2t2_setup()?;
            let prepared = crate::audio_preprocess::decode_audio_file(std::path::Path::new(
                &recovery.recording.file_path,
            ))?;
            if prepared.samples.len() > crate::r2t2::MAX_SAMPLES {
                return Err(anyhow!("R2T2_LIMIT: This recording exceeds five minutes."));
            }
            let count = prepared.samples.len();
            let tap =
                crate::audio_capture::CaptureTap::from_recording(session.id.clone(), prepared);
            self.translation.with_active(&session, || {
                self.update_status(|status| {
                    status.state = QuickDictationState::Processing;
                    status.last_error_message = None;
                })?;
                self.desktop_shell
                    .set_overlay_payload(DictationOverlayPayload {
                        session_id: Some(session.id.clone()),
                        phase: OverlayPhase::Processing,
                        streaming_state: Some(crate::desktop_shell::StreamingState::Preparing),
                        ..Default::default()
                    })
            })?;
            self.translation.phase(
                &session,
                "streaming_finishing",
                "Retrying live transcription",
            )?;
            let translation = self.translation.clone();
            let handle = crate::r2t2::start(
                session.id.clone(),
                tap,
                session.cancelled.clone(),
                self.translation.processing_queue(),
                self.desktop_shell.clone(),
                helper,
                model,
                crate::r2t2::source_language(&recovery.settings)?,
                vocabulary::build_asr_prompt(&recovery.vocabulary)
                    .map(|p| p.text)
                    .unwrap_or_default(),
                Arc::new(move || translation.release_asr_resources()),
                Arc::new(|| {}),
            )?;
            handle.activate();
            let completion = match handle.finish(&session.id, count) {
                Ok(done) => done,
                Err(error) => {
                    handle.cancel();
                    return Err(error);
                }
            };
            self.translation.ensure_active(&session)?;
            let (text, _permit) = completion.release_worker(
                &self.translation.processing_queue(),
                session.mode == crate::translation::OutputMode::Original,
            );
            let source = vocabulary::correct_transcript_with_terms(
                &recovery.vocabulary,
                crate::r2t2::transcript(&session.id, text, recovery.recording.duration_ms, None),
            )?;
            let output =
                self.translation
                    .process(&session, &source, recovery.recording.duration_ms)?;
            self.translation.ensure_active(&session)?;
            if output.status != "completed" {
                return Err(anyhow!(output.error_message.unwrap_or_else(|| {
                    "Translation failed. The original text is available.".into()
                })));
            }
            let mut saved = output.transcript_id;
            if session.mode == crate::translation::OutputMode::Original && session.save_history {
                saved = Some(
                    self.translation
                        .with_active(&session, || {
                            storage::save_quick_dictation_transcript(
                                &self.db_path,
                                &source,
                                recovery.recording.duration_ms,
                            )
                        })?
                        .id,
                );
            }
            self.translation.with_active(&session, || {
                self.update_status(|status| {
                    status.state = QuickDictationState::Idle;
                    status.last_transcript_text = output.output_text.clone();
                    status.last_transcript_id = saved.clone();
                    status.last_model_name = Some(crate::r2t2::MODEL_NAME.into());
                    status.last_error_message = None;
                    status.last_insert_outcome = None;
                    status.last_insert_warning = None;
                    status.can_retry_streaming = false;
                })?;
                *self
                    .recovery
                    .lock()
                    .map_err(|_| anyhow!("Recovery unavailable"))? = None;
                self.desktop_shell.set_overlay_payload(Default::default())
            })?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = self.set_error_owned(error.to_string(), generation);
            return Err(error);
        }
        Ok(self.status())
    }

    fn spawn_overlay_level_poller(&self, generation: u64, session_id: String) {
        let controller = self.clone();
        thread::spawn(move || {
            let started = std::time::Instant::now();
            let mut warned = false;
            // Exit as soon as this poller is superseded by a newer listening
            // session OR the state leaves Listening. Both guards together make
            // it impossible to accumulate zombie pollers under intensive use.
            while controller.poller_generation.load(Ordering::SeqCst) == generation
                && controller.status().state == QuickDictationState::Listening
            {
                let level = controller.recording_controller.input_level().unwrap_or(0.0);
                let _ = controller.desktop_shell.update_level(&session_id, level);
                // Live R2T2 sessions enforce the same limit in their own stream.
                let live = controller
                    .live
                    .lock()
                    .map(|live| live.is_some())
                    .unwrap_or(false);
                let elapsed_ms = started.elapsed().as_millis() as u64;
                if !live && !warned && elapsed_ms >= DICTATION_LIMIT_WARNING_MS {
                    warned = true;
                    let _ = controller
                        .desktop_shell
                        .set_listening_notice(&session_id, "30 s left · 5-minute limit");
                }
                if !live && elapsed_ms >= DICTATION_CAPTURE_LIMIT_MS {
                    controller.desktop_shell.stream_duration_limit(&session_id);
                    if let Err(error) = controller.finish_dictation() {
                        eprintln!("[dictation] limit finish failed: {error:?}");
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        });
    }

    fn schedule_idle_reset(&self) {
        let revision = self.desktop_shell.overlay_revision();
        let generation = self.poller_generation.load(Ordering::SeqCst);
        let transition = self.state_since_ms.load(Ordering::SeqCst);
        self.schedule_idle_reset_at(revision, generation, transition);
    }
    fn schedule_idle_reset_at(&self, revision: u64, generation: u64, transition: i64) {
        let controller = self.clone();
        thread::spawn(move || {
            // Long enough to read the result chip; short enough to feel snappy
            // and not block subsequent dictations.
            thread::sleep(Duration::from_millis(1800));
            if controller.poller_generation.load(Ordering::SeqCst) != generation
                || controller.state_since_ms.load(Ordering::SeqCst) != transition
            {
                return;
            }
            let current = controller.status();
            if matches!(
                current.state,
                QuickDictationState::Inserted
                    | QuickDictationState::ClipboardOnly
                    | QuickDictationState::Error
            ) {
                let _ = controller
                    .desktop_shell
                    .set_overlay_if_revision(revision, DictationOverlayPayload::default());
                if let Err(error) = controller.update_status(|status| {
                    if controller.poller_generation.load(Ordering::SeqCst) == generation
                        && controller.state_since_ms.load(Ordering::SeqCst) == transition
                        && matches!(
                            status.state,
                            QuickDictationState::Inserted
                                | QuickDictationState::ClipboardOnly
                                | QuickDictationState::Error
                        )
                    {
                        status.state = QuickDictationState::Idle;
                    }
                }) {
                    eprintln!("[dictation] failed to reset to idle: {error:?}");
                }
            }
        });
    }

    fn set_error(&self, message: String) -> Result<()> {
        self.set_error_owned(message, self.poller_generation.load(Ordering::SeqCst))
    }
    fn set_error_owned(&self, message: String, generation: u64) -> Result<()> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| anyhow!("Dictation state unavailable"))?;
        if self.poller_generation.load(Ordering::SeqCst) != generation {
            return Ok(());
        }
        self.restore_system_volume();
        if let Some(player) = self.sound_player.as_ref().as_ref() {
            player.finish_capture(false, true);
        }
        crate::sound::notify(
            &self.app,
            crate::sound::FeedbackCue::Error,
            &format!(
                "dictation-error:{}",
                self.state_since_ms.load(Ordering::SeqCst)
            ),
        );
        self.desktop_shell
            .set_overlay_payload(DictationOverlayPayload {
                phase: OverlayPhase::Failed,
                audio_level: 0.0,
                ..Default::default()
            })?;
        status.state = QuickDictationState::Error;
        status.last_error_message = Some(message);
        let transition = now_ms();
        self.state_since_ms.store(transition, Ordering::SeqCst);
        let revision = self.desktop_shell.overlay_revision();
        self.app.emit(QUICK_DICTATE_STATUS_EVENT, status.clone())?;
        drop(status);
        // Auto-hide the error chip too — same path as success outcomes.
        self.schedule_idle_reset_at(revision, generation, transition);
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
            let previous_state = status.state;
            apply(&mut status);
            if status.state != previous_state {
                // Stamp every transition so the watchdog can tell how long the
                // controller has been parked in Listening/Processing.
                self.state_since_ms.store(now_ms(), Ordering::SeqCst);
            }
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

    /// Stop capture and insertion without re-registering shortcuts.
    pub fn prepare_shutdown(&self) {
        self.translation.cancel();
        if let Some(live) = self.live.lock().ok().and_then(|mut live| live.take()) {
            live.handle.cancel();
        }
        self.translation.processing_queue().evict_idle();
        self.poller_generation.fetch_add(1, Ordering::SeqCst);
        let _ = self.suspend_shortcut_registration();
        let _ = self.recording_controller.cancel();
        if let Some(player) = self.sound_player.as_ref().as_ref() {
            player.finish_capture(false, true);
        }
        self.restore_system_volume();
        let _ = self.update_status(|status| status.state = QuickDictationState::Idle);
        let _ = self
            .desktop_shell
            .set_overlay_payload(DictationOverlayPayload::default());
    }

    /// Force the controller back to a clean Idle state, abandoning any wedged
    /// recording worker and tearing down overlay/volume side effects. Used by
    /// both the manual reset command and the watchdog.
    pub fn force_reset(&self) -> Result<QuickDictationStatusResponse> {
        self.translation.cancel();
        if let Some(live) = self.live.lock().ok().and_then(|mut live| live.take()) {
            live.handle.cancel();
        }
        self.translation.processing_queue().evict_idle();
        if crate::shutdown::is_shutting_down() {
            return Ok(self.status());
        }
        // Supersede any running poller and drop a possibly-wedged worker.
        self.poller_generation.fetch_add(1, Ordering::SeqCst);
        let _ = self.recording_controller.cancel();
        self.recording_controller.recover();
        if let Some(player) = self.sound_player.as_ref().as_ref() {
            player.finish_capture(false, true);
        }
        self.restore_system_volume();
        if let Ok(mut session) = self.clipboard_only_session.lock() {
            *session = None;
        }
        let _ = self
            .desktop_shell
            .set_overlay_payload(DictationOverlayPayload::default());
        self.update_status(|status| {
            status.state = QuickDictationState::Idle;
        })?;
        // Re-arm the shortcut in case registration was affected.
        let _ = self.sync_shortcut_registration();
        Ok(self.status())
    }

    /// Spawn the single watchdog thread that auto-recovers a dictation stuck in
    /// Listening or Processing for longer than `STUCK_THRESHOLD`. This is the
    /// last-resort safety net behind the worker-level self-healing.
    pub fn spawn_watchdog(&self) {
        const STUCK_THRESHOLD_MS: i64 = 90_000;
        const POLL_INTERVAL: Duration = Duration::from_secs(5);
        let controller = self.clone();
        thread::spawn(move || loop {
            thread::sleep(POLL_INTERVAL);
            let state = controller.status().state;
            let phase = controller.translation.snapshot().stage;
            // Live capture has sample-progress checks, a five-minute cap, and
            // separate loading/finalization deadlines in its streaming service.
            if controller
                .live
                .lock()
                .map(|live| live.is_some())
                .unwrap_or(false)
            {
                continue;
            }
            if matches!(
                phase.as_str(),
                "waiting" | "transcribing" | "loading" | "translating" | "streaming_finishing"
            ) {
                continue;
            }
            let is_active = matches!(
                state,
                QuickDictationState::Listening | QuickDictationState::Processing
            );
            if !is_active {
                continue;
            }
            // Listening ends by itself at the five-minute limit; only treat it
            // as stuck well after that, so long dictations are never discarded.
            let threshold = if state == QuickDictationState::Listening {
                DICTATION_CAPTURE_LIMIT_MS as i64 + 30_000
            } else {
                STUCK_THRESHOLD_MS
            };
            let since = controller.state_since_ms.load(Ordering::SeqCst);
            if now_ms() - since >= threshold {
                eprintln!(
                    "[dictation] watchdog: stuck in {:?} for too long — forcing reset",
                    state
                );
                if let Err(error) = controller.force_reset() {
                    eprintln!("[dictation] watchdog reset failed: {error:?}");
                }
            }
        });
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
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

#[cfg(test)]
mod translation_shortcut_tests {
    use super::*;
    #[test]
    fn in_app_copy_override_cannot_leak_into_a_later_shortcut_session() {
        use crate::settings::InsertBehavior;
        assert!(matches!(
            insertion_behavior_for_session(InsertBehavior::Paste, "ui", Some("ui")),
            InsertBehavior::ClipboardOnly
        ));
        // Failed, empty, and canceled UI recordings may leave an old identity,
        // but they must never consume the next shortcut's paste behavior.
        assert!(matches!(
            insertion_behavior_for_session(InsertBehavior::Paste, "shortcut", Some("ui")),
            InsertBehavior::Paste
        ));
        assert!(matches!(
            insertion_behavior_for_session(InsertBehavior::Paste, "shortcut", None),
            InsertBehavior::Paste
        ));
        assert!(matches!(
            insertion_behavior_for_session(InsertBehavior::ClipboardOnly, "shortcut", None),
            InsertBehavior::ClipboardOnly
        ));
    }
    #[test]
    fn parsed_shortcut_conflicts_ignore_modifier_order() {
        assert!(shortcut_pair("CmdOrCtrl+Shift+Right", Some("Shift+CmdOrCtrl+Right")).is_err());
        assert_eq!(
            shortcut_pair(
                "CmdOrCtrl+Shift+Space",
                Some(crate::translation::DEFAULT_SHORTCUT)
            )
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            shortcut_pair("CmdOrCtrl+Shift+Space", None).unwrap().len(),
            1
        );
    }
    #[test]
    fn holding_the_language_key_cycles_only_once() {
        let pressed = AtomicBool::new(false);
        assert!(cycle_key_transition(&pressed, ShortcutState::Pressed));
        assert!(!cycle_key_transition(&pressed, ShortcutState::Pressed));
        assert!(!cycle_key_transition(&pressed, ShortcutState::Released));
        assert!(cycle_key_transition(&pressed, ShortcutState::Pressed));
    }
}
