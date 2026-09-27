use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Window};

use crate::startup::{StartupCoordinator, StartupPhase};

const OVERLAY_LABEL: &str = "dictation-overlay";
const OVERLAY_EVENT: &str = "quick-dictation-overlay";
const OVERLAY_WIDTH: f64 = 350.0;
const STREAMING_OVERLAY_WIDTH: f64 = 480.0;
const OVERLAY_HEIGHT: f64 = 64.0;
const OVERLAY_MARGIN_TOP: f64 = 32.0;
const TRAY_UNAVAILABLE_CLOSE_EVENT: &str = "app://tray-unavailable-close-requested";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPhase {
    Hidden,
    Mode,
    Listening,
    Processing,
    Inserted,
    ClipboardOnly,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamingState {
    Preparing,
    Waiting,
    Listening,
    CatchingUp,
    Finishing,
    Translating,
    Failed,
}
impl StreamingState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing R2T2",
            Self::Waiting => "Waiting for local processing",
            Self::Listening => "Listening",
            Self::CatchingUp => "Catching up",
            Self::Finishing => "Finishing",
            Self::Translating => "Translating",
            Self::Failed => "Needs attention",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationOverlayPayload {
    pub phase: OverlayPhase,
    pub audio_level: f32,
    pub output_mode: crate::translation::OutputMode,
    pub status_text: Option<String>,
    pub revision: u64,
    pub session_id: Option<String>,
    pub live_text: String,
    #[serde(default)]
    pub tentative_text: String,
    #[serde(skip)]
    pub stream_text_final: bool,
    pub streaming_state: Option<StreamingState>,
    pub lag_ms: u64,
    pub duration_limit_reached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayUnavailableClosePayload {
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainWindowCloseAction {
    HideToTray,
    ExplainMissingTray,
    Exit,
}

impl Default for DictationOverlayPayload {
    fn default() -> Self {
        Self {
            phase: OverlayPhase::Hidden,
            audio_level: 0.0,
            output_mode: Default::default(),
            status_text: None,
            revision: 0,
            session_id: None,
            live_text: String::new(),
            tentative_text: String::new(),
            stream_text_final: false,
            streaming_state: None,
            lag_ms: 0,
            duration_limit_reached: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StreamingText {
    pub committed: String,
    pub tentative: String,
}

impl DictationOverlayPayload {
    fn owns_stream(&self, session_id: &str) -> bool {
        self.session_id.as_deref() == Some(session_id)
            && matches!(
                self.phase,
                OverlayPhase::Listening | OverlayPhase::Processing
            )
    }

    fn apply_stream(
        &mut self,
        session_id: &str,
        state: StreamingState,
        text: Option<StreamingText>,
        lag_ms: u64,
    ) -> bool {
        if !self.owns_stream(session_id)
            || (self.stream_text_final && state != StreamingState::Failed)
            || self.streaming_state == Some(StreamingState::Failed)
        {
            return false;
        }
        if let Some(text) = text {
            self.live_text = text.committed;
            self.tentative_text = text.tentative;
        }
        if state == StreamingState::Failed {
            self.tentative_text.clear();
            self.stream_text_final = true;
        }
        self.streaming_state = Some(state);
        self.status_text = Some(
            if self.duration_limit_reached && state == StreamingState::Finishing {
                "5-minute limit · Finishing"
            } else {
                state.label()
            }
            .into(),
        );
        self.lag_ms = lag_ms;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    fn finalize_stream_text(&mut self, session_id: &str, text: String) -> bool {
        if !self.owns_stream(session_id)
            || self.streaming_state.is_none()
            || self.streaming_state == Some(StreamingState::Failed)
        {
            return false;
        }
        self.live_text = text;
        self.tentative_text.clear();
        self.stream_text_final = true;
        self.revision = self.revision.wrapping_add(1);
        true
    }
}

#[derive(Clone)]
pub struct DesktopShellController {
    app: AppHandle,
    overlay_payload: Arc<Mutex<DictationOverlayPayload>>,
    tray_close_explained: Arc<AtomicBool>,
    _tray: Arc<TrayIcon>,
    overlay_dispatch_pending: Arc<AtomicBool>,
}

impl DesktopShellController {
    pub fn initialize(app: &AppHandle) -> Result<Self> {
        ensure_overlay_window(app)?;
        let tray = Arc::new(build_tray_icon(app)?);
        Ok(Self {
            app: app.clone(),
            overlay_payload: Arc::new(Mutex::new(DictationOverlayPayload::default())),
            tray_close_explained: Arc::new(AtomicBool::new(false)),
            _tray: tray,
            overlay_dispatch_pending: Default::default(),
        })
    }

    pub fn overlay_payload(&self) -> DictationOverlayPayload {
        self.overlay_payload
            .lock()
            .map(|payload| payload.clone())
            .unwrap_or_default()
    }

    pub fn set_output_mode(&self, mode: crate::translation::OutputMode) {
        if let Ok(mut current) = self.overlay_payload.lock() {
            current.output_mode = mode;
        }
    }
    pub fn overlay_revision(&self) -> u64 {
        self.overlay_payload().revision
    }
    pub fn flash_mode(&self) -> Result<()> {
        self.set_overlay_payload(DictationOverlayPayload {
            phase: OverlayPhase::Mode,
            ..Default::default()
        })?;
        let revision = self.overlay_revision();
        let shell = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = shell.set_overlay_if_revision(revision, DictationOverlayPayload::default());
        });
        Ok(())
    }
    /// Short notice shown in place of "Listening" (e.g. the 5-minute warning).
    pub fn set_listening_notice(&self, session_id: &str, text: &str) -> Result<()> {
        let mut current = self
            .overlay_payload
            .lock()
            .map_err(|_| anyhow::anyhow!("Overlay unavailable"))?;
        if current.session_id.as_deref() != Some(session_id)
            || current.phase != OverlayPhase::Listening
        {
            return Ok(());
        }
        current.status_text = Some(text.into());
        current.revision = current.revision.wrapping_add(1);
        let payload = current.clone();
        drop(current);
        self.dispatch_overlay(payload)
    }
    pub fn update_level(&self, session_id: &str, audio_level: f32) -> Result<()> {
        let mut current = self
            .overlay_payload
            .lock()
            .map_err(|_| anyhow::anyhow!("Overlay unavailable"))?;
        if current.session_id.as_deref() != Some(session_id)
            || current.phase != OverlayPhase::Listening
        {
            return Ok(());
        }
        current.audio_level = audio_level;
        current.revision = current.revision.wrapping_add(1);
        let payload = current.clone();
        drop(current);
        self.dispatch_overlay(payload)
    }
    pub(crate) fn update_stream(
        &self,
        session_id: &str,
        state: StreamingState,
        text: Option<StreamingText>,
        lag_ms: u64,
    ) -> Result<()> {
        let mut current = self
            .overlay_payload
            .lock()
            .map_err(|_| anyhow::anyhow!("Overlay unavailable"))?;
        if !current.apply_stream(session_id, state, text, lag_ms) {
            return Ok(());
        }
        let payload = current.clone();
        drop(current);
        self.dispatch_overlay(payload)
    }
    pub fn finalize_stream_text(&self, session_id: &str, text: String) -> Result<()> {
        let mut current = self
            .overlay_payload
            .lock()
            .map_err(|_| anyhow::anyhow!("Overlay unavailable"))?;
        if !current.finalize_stream_text(session_id, text) {
            return Ok(());
        }
        let payload = current.clone();
        drop(current);
        self.dispatch_overlay(payload)
    }
    pub fn update_stream_lag(&self, session_id: &str, lag_ms: u64) -> Result<()> {
        let mut current = self
            .overlay_payload
            .lock()
            .map_err(|_| anyhow::anyhow!("Overlay unavailable"))?;
        if current.session_id.as_deref() != Some(session_id)
            || current.streaming_state.is_none()
            || !matches!(
                current.phase,
                OverlayPhase::Listening | OverlayPhase::Processing
            )
        {
            return Ok(());
        }
        current.lag_ms = lag_ms;
        if matches!(
            current.streaming_state,
            Some(StreamingState::Listening | StreamingState::CatchingUp)
        ) {
            let state = if lag_ms > 2000 {
                StreamingState::CatchingUp
            } else {
                StreamingState::Listening
            };
            current.streaming_state = Some(state);
            current.status_text = Some(state.label().into());
        }
        current.revision = current.revision.wrapping_add(1);
        let payload = current.clone();
        drop(current);
        self.dispatch_overlay(payload)
    }
    pub fn stream_duration_limit(&self, session_id: &str) {
        if let Ok(mut current) = self.overlay_payload.lock() {
            if current.session_id.as_deref() == Some(session_id) {
                current.duration_limit_reached = true;
            }
        }
    }
    pub fn processing_for_session(
        &self,
        session_id: &str,
        text: &str,
        translating: bool,
    ) -> Result<()> {
        let mut current = self
            .overlay_payload
            .lock()
            .map_err(|_| anyhow::anyhow!("Overlay unavailable"))?;
        if current
            .session_id
            .as_deref()
            .is_some_and(|id| id != session_id)
        {
            return Ok(());
        }
        current.session_id = Some(session_id.into());
        current.phase = OverlayPhase::Processing;
        current.audio_level = 0.0;
        current.status_text = Some(
            if current.duration_limit_reached && !translating {
                "5-minute limit · Finishing"
            } else {
                text
            }
            .into(),
        );
        if current.streaming_state.is_some() {
            if translating {
                current.tentative_text.clear();
                current.stream_text_final = true;
            }
            current.streaming_state = Some(if translating {
                StreamingState::Translating
            } else {
                StreamingState::Finishing
            });
        }
        current.revision = current.revision.wrapping_add(1);
        let payload = current.clone();
        drop(current);
        self.dispatch_overlay(payload)
    }
    pub fn set_overlay_if_revision(
        &self,
        revision: u64,
        payload: DictationOverlayPayload,
    ) -> Result<()> {
        self.replace_overlay(Some(revision), payload)
    }
    pub fn set_overlay_payload(&self, payload: DictationOverlayPayload) -> Result<()> {
        self.replace_overlay(None, payload)
    }
    fn replace_overlay(
        &self,
        expected: Option<u64>,
        mut payload: DictationOverlayPayload,
    ) -> Result<()> {
        let mut current = self
            .overlay_payload
            .lock()
            .map_err(|_| anyhow::anyhow!("Overlay unavailable"))?;
        if expected.is_some_and(|revision| revision != current.revision) {
            return Ok(());
        }
        payload.output_mode = current.output_mode;
        if matches!(
            payload.phase,
            OverlayPhase::Inserted | OverlayPhase::ClipboardOnly
        ) {
            payload.duration_limit_reached |= current.duration_limit_reached;
        }
        payload.revision = current.revision.wrapping_add(1);
        *current = payload.clone();
        drop(current);
        self.dispatch_overlay(payload)
    }
    fn dispatch_overlay(&self, _payload: DictationOverlayPayload) -> Result<()> {
        if self.overlay_dispatch_pending.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let shell = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let apply = shell.clone();
            if shell
                .app
                .run_on_main_thread(move || {
                    apply
                        .overlay_dispatch_pending
                        .store(false, Ordering::SeqCst);
                    let payload = apply.overlay_payload();
                    let _ = apply.apply_overlay(payload);
                })
                .is_err()
            {
                shell
                    .overlay_dispatch_pending
                    .store(false, Ordering::SeqCst);
            }
        });
        Ok(())
    }
    fn apply_overlay(&self, payload: DictationOverlayPayload) -> Result<()> {
        // Serialize window effects and recheck ownership on the main thread.
        let current = self
            .overlay_payload
            .lock()
            .map_err(|_| anyhow::anyhow!("Overlay unavailable"))?;
        if current.revision != payload.revision {
            return Ok(());
        }

        let payload = if crate::shutdown::is_shutting_down() {
            DictationOverlayPayload::default()
        } else {
            payload
        };
        if let Some(window) = self.app.get_webview_window(OVERLAY_LABEL) {
            let width = if payload.streaming_state.is_some() {
                STREAMING_OVERLAY_WIDTH
            } else {
                OVERLAY_WIDTH
            };
            window.set_size(tauri::LogicalSize::new(width, OVERLAY_HEIGHT))?;
            position_overlay_window_with_width(&window, width)?;
            match payload.phase {
                OverlayPhase::Hidden => {
                    let _ = window.hide();
                }
                OverlayPhase::Mode
                | OverlayPhase::Listening
                | OverlayPhase::Processing
                | OverlayPhase::Inserted
                | OverlayPhase::ClipboardOnly
                | OverlayPhase::Failed => {
                    let _ = window.show();
                }
            }
        }

        self.app.emit(OVERLAY_EVENT, payload)?;
        Ok(())
    }

    pub fn handle_main_window_close(&self, window: &Window) -> Result<()> {
        match main_window_close_action(tray_is_invisible(), &self.tray_close_explained) {
            MainWindowCloseAction::ExplainMissingTray => {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
                self.app.emit(
                    TRAY_UNAVAILABLE_CLOSE_EVENT,
                    TrayUnavailableClosePayload {
                        title: "Blabber needs to stay open".to_string(),
                        message: "GNOME is not exposing tray icons in this session, so hiding Blabber would make it difficult to reopen. Install the AppIndicator extension for tray behavior, keep this window open, or quit explicitly.".to_string(),
                    },
                )?;
                return Ok(());
            }
            MainWindowCloseAction::Exit => {
                crate::shutdown::request_exit(&self.app, crate::shutdown::ExitAction::Quit);
                return Ok(());
            }
            MainWindowCloseAction::HideToTray => {
                window.hide()?;
            }
        }
        Ok(())
    }
}

fn tray_is_invisible() -> bool {
    tray_is_invisible_for(
        cfg!(target_os = "linux"),
        crate::platform::is_gnome(),
        crate::platform::has_appindicator_hint(),
    )
}

fn tray_is_invisible_for(is_linux: bool, is_gnome: bool, has_appindicator_hint: bool) -> bool {
    is_linux && is_gnome && !has_appindicator_hint
}

fn main_window_close_action(
    tray_is_invisible: bool,
    tray_close_explained: &AtomicBool,
) -> MainWindowCloseAction {
    if !tray_is_invisible {
        return MainWindowCloseAction::HideToTray;
    }
    if tray_close_explained.swap(true, Ordering::SeqCst) {
        MainWindowCloseAction::Exit
    } else {
        MainWindowCloseAction::ExplainMissingTray
    }
}

fn ensure_overlay_window(app: &AppHandle) -> Result<()> {
    if app.get_webview_window(OVERLAY_LABEL).is_some() {
        return Ok(());
    }

    let window =
        WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("overlay.html".into()))
            .title("Blabber Overlay")
            .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .visible(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .build()?;
    window.set_ignore_cursor_events(true)?;
    position_overlay_window(&window)?;
    Ok(())
}

fn position_overlay_window(window: &tauri::WebviewWindow) -> Result<()> {
    position_overlay_window_with_width(window, OVERLAY_WIDTH)
}
fn position_overlay_window_with_width(window: &tauri::WebviewWindow, width: f64) -> Result<()> {
    if let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) {
        let size = monitor.size();
        let position = monitor.position();
        let scale = monitor.scale_factor();
        let x = position.x as f64 + ((size.width as f64 - width * scale) / 2.0).max(0.0);
        let y = position.y as f64 + OVERLAY_MARGIN_TOP * scale;
        window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x.round() as i32,
            y.round() as i32,
        )))?;
    }
    Ok(())
}

fn build_tray_icon(app: &AppHandle) -> Result<TrayIcon> {
    let menu = MenuBuilder::new(app)
        .text("show", "Open Blabber")
        .text("quit", "Quit Blabber")
        .build()?;

    let default_icon = app.default_window_icon().cloned();
    let mut builder = TrayIconBuilder::with_id("blabber-tray")
        .menu(&menu)
        .tooltip("Blabber")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                let _ = show_main_window(app);
            }
            "quit" => crate::shutdown::request_exit(app, crate::shutdown::ExitAction::Quit),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = default_icon {
        builder = builder.icon(icon);
    }

    builder.build(app).map_err(Into::into)
}

pub(crate) fn show_main_window(app: &AppHandle) -> Result<()> {
    if let Some(startup) = app.try_state::<StartupCoordinator>() {
        if startup.status().phase != StartupPhase::Ready {
            if let Some(splash) = app.get_webview_window("splashscreen") {
                splash.show()?;
                splash.unminimize()?;
                splash.set_focus()?;
            }
            return Ok(());
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_atomic_owned_and_cannot_return_after_finalization() {
        let mut state = DictationOverlayPayload {
            phase: OverlayPhase::Listening,
            session_id: Some("live".into()),
            streaming_state: Some(StreamingState::Listening),
            ..Default::default()
        };
        let preview = || {
            Some(StreamingText {
                committed: "Größe".into(),
                tentative: " über".into(),
            })
        };
        assert!(!state.apply_stream("stale", StreamingState::Listening, preview(), 0));
        assert_eq!(state.revision, 0);
        assert!(state.apply_stream("live", StreamingState::Listening, preview(), 50));
        assert_eq!(
            (&*state.live_text, &*state.tentative_text, state.revision),
            ("Größe", " über", 1)
        );
        assert!(state.apply_stream("live", StreamingState::CatchingUp, None, 2100));
        assert_eq!(
            (&*state.live_text, &*state.tentative_text, state.revision),
            ("Größe", " über", 2)
        );
        state.phase = OverlayPhase::Processing;
        assert!(state.apply_stream("live", StreamingState::Finishing, None, 0));
        assert_eq!(state.tentative_text, " über");
        assert!(state.finalize_stream_text("live", "Größe über die Straße.".into()));
        assert!(state.tentative_text.is_empty());
        let revision = state.revision;
        assert!(!state.apply_stream("live", StreamingState::Listening, preview(), 0));
        assert_eq!(state.revision, revision);
        // Vocabulary corrections can replace only the completed source display.
        assert!(state.finalize_stream_text("live", "Corrected source.".into()));
        assert_eq!(state.live_text, "Corrected source.");
        assert!(!state.finalize_stream_text("stale", "old".into()));
    }
    #[test]
    fn failed_hidden_and_new_sessions_cannot_retain_tentative_text() {
        let mut state = DictationOverlayPayload {
            phase: OverlayPhase::Listening,
            session_id: Some("live".into()),
            streaming_state: Some(StreamingState::Listening),
            tentative_text: "draft".into(),
            ..Default::default()
        };
        assert!(state.apply_stream("live", StreamingState::Failed, None, 0));
        assert!(state.tentative_text.is_empty());
        assert!(!state.apply_stream(
            "live",
            StreamingState::Listening,
            Some(StreamingText {
                committed: "".into(),
                tentative: "late".into()
            }),
            0
        ));
        state = Default::default();
        assert!(state.tentative_text.is_empty());
        assert!(!state.apply_stream("live", StreamingState::Listening, None, 0));
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["tentativeText"], "");
        assert!(json.get("streamTextFinal").is_none());
    }

    #[test]
    fn tray_is_invisible_only_for_linux_gnome_without_appindicator() {
        assert!(tray_is_invisible_for(true, true, false));
        assert!(!tray_is_invisible_for(true, true, true));
        assert!(!tray_is_invisible_for(true, false, false));
        assert!(!tray_is_invisible_for(false, true, false));
    }

    #[test]
    fn invisible_tray_close_explains_once_then_exits() {
        let explained = AtomicBool::new(false);
        assert_eq!(
            main_window_close_action(true, &explained),
            MainWindowCloseAction::ExplainMissingTray
        );
        assert_eq!(
            main_window_close_action(true, &explained),
            MainWindowCloseAction::Exit
        );
    }

    #[test]
    fn visible_tray_close_hides_without_marking_explained() {
        let explained = AtomicBool::new(false);
        assert_eq!(
            main_window_close_action(false, &explained),
            MainWindowCloseAction::HideToTray
        );
        assert!(!explained.load(Ordering::SeqCst));
    }
}
