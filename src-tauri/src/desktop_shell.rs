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
const OVERLAY_WIDTH: f64 = 220.0;
const OVERLAY_HEIGHT: f64 = 64.0;
const OVERLAY_MARGIN_TOP: f64 = 32.0;
const TRAY_UNAVAILABLE_CLOSE_EVENT: &str = "app://tray-unavailable-close-requested";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPhase {
    Hidden,
    Listening,
    Processing,
    Inserted,
    ClipboardOnly,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationOverlayPayload {
    pub phase: OverlayPhase,
    pub audio_level: f32,
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
        }
    }
}

#[derive(Clone)]
pub struct DesktopShellController {
    app: AppHandle,
    overlay_payload: Arc<Mutex<DictationOverlayPayload>>,
    tray_close_explained: Arc<AtomicBool>,
    _tray: Arc<TrayIcon>,
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
        })
    }

    pub fn overlay_payload(&self) -> DictationOverlayPayload {
        self.overlay_payload
            .lock()
            .map(|payload| payload.clone())
            .unwrap_or_default()
    }

    pub fn set_overlay_payload(&self, payload: DictationOverlayPayload) -> Result<()> {
        if let Ok(mut current) = self.overlay_payload.lock() {
            *current = payload.clone();
        }

        if let Some(window) = self.app.get_webview_window(OVERLAY_LABEL) {
            position_overlay_window(&window)?;
            match payload.phase {
                OverlayPhase::Hidden => {
                    let _ = window.hide();
                }
                OverlayPhase::Listening
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
                self.app.exit(0);
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
            .build()?;
    position_overlay_window(&window)?;
    Ok(())
}

fn position_overlay_window(window: &tauri::WebviewWindow) -> Result<()> {
    if let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) {
        let size = monitor.size();
        let position = monitor.position();
        let x = position.x as f64 + ((size.width as f64 - OVERLAY_WIDTH) / 2.0).max(0.0);
        let y = position.y as f64 + OVERLAY_MARGIN_TOP;
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
            "quit" => app.exit(0),
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

fn show_main_window(app: &AppHandle) -> Result<()> {
    if let Some(startup) = app.try_state::<StartupCoordinator>() {
        if startup.status().phase != StartupPhase::Ready {
            if let Some(splash) = app.get_webview_window("splashscreen") {
                let _ = splash.show();
                let _ = splash.unminimize();
                let _ = splash.set_focus();
            }
            return Ok(());
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
