#[cfg(target_os = "macos")]
use std::ffi::c_void;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(target_os = "windows")]
use tauri::Manager;

use crate::settings::InsertBehavior;

#[cfg(target_os = "macos")]
mod clipboard_restore;

#[cfg(target_os = "windows")]
mod windows_clipboard_restore;

#[cfg(target_os = "macos")]
use clipboard_restore::ClipboardSnapshot;

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

#[cfg(target_os = "macos")]
const KCG_HID_EVENT_TAP: u32 = 0;
#[cfg(target_os = "macos")]
const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;
#[cfg(target_os = "macos")]
const MACOS_KEYCODE_V: u16 = 0x09;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertionOutcome {
    Pasted,
    ClipboardOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertionWarning {
    AccessibilityRequired,
    PasteUnavailable,
}

impl InsertionWarning {
    pub fn message(self) -> &'static str {
        match self {
            Self::AccessibilityRequired => "Text copied. Auto-paste needs Accessibility access for this Blabber app. Use Grant access, then enable Blabber in System Settings. If it is already enabled, remove the old entry and add the current Blabber app again.",
            Self::PasteUnavailable => "Text copied, but Blabber could not paste into the target app. Use the paste shortcut to insert it.",
        }
    }

    pub fn overlay_label(self) -> &'static str {
        match self {
            Self::AccessibilityRequired => "Copied · Enable auto-paste",
            Self::PasteUnavailable => "Copied · Paste manually",
        }
    }
}

#[derive(Debug)]
pub struct InsertionReport {
    pub outcome: InsertionOutcome,
    pub warning: Option<InsertionWarning>,
}

impl InsertionReport {
    fn copied(warning: Option<InsertionWarning>) -> Self {
        Self {
            outcome: InsertionOutcome::ClipboardOnly,
            warning,
        }
    }

    fn pasted() -> Self {
        Self {
            outcome: InsertionOutcome::Pasted,
            warning: None,
        }
    }
}

fn paste_warning(supported: bool, trusted: bool) -> Option<InsertionWarning> {
    if !supported {
        Some(InsertionWarning::PasteUnavailable)
    } else if !trusted {
        Some(InsertionWarning::AccessibilityRequired)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub enum PasteTarget {
    #[cfg(target_os = "macos")]
    BundleId(String),
    #[cfg(target_os = "windows")]
    WindowHandle(isize),
}

pub fn insert_text(
    app: &AppHandle,
    text: &str,
    behavior: InsertBehavior,
    paste_target: Option<&PasteTarget>,
    check_active: impl Fn() -> Result<()>,
) -> Result<InsertionReport> {
    check_active()?;
    let warning = paste_warning(auto_paste_allowed(), accessibility_trusted());
    let can_paste = should_attempt_paste(behavior, warning.is_none());
    #[cfg(target_os = "macos")]
    let clipboard_snapshot = if can_paste {
        Some(ClipboardSnapshot::capture())
    } else {
        None
    };

    #[cfg(target_os = "windows")]
    let windows_clipboard_snapshot = if can_paste {
        Some(windows_clipboard_restore::ClipboardSnapshot::capture()?)
    } else {
        None
    };

    #[cfg(target_os = "windows")]
    let windows_clipboard_owner = if can_paste {
        windows_clipboard_owner(app)?
    } else {
        std::ptr::null_mut()
    };

    check_active()?;
    app.clipboard().write_text(text.to_string())?;

    #[cfg(target_os = "macos")]
    let dictated_clipboard_change_count = clipboard_restore::change_count();

    #[cfg(target_os = "windows")]
    let dictated_clipboard_sequence_number = windows_clipboard_restore::sequence_number();

    // A reset may arrive during clipboard propagation, refocusing, or a retry.
    // Restore only our unchanged temporary clipboard, then stop before the key event.
    macro_rules! ensure_current {
        () => {
            if let Err(error) = check_active() {
                #[cfg(target_os = "macos")]
                restore_macos_clipboard_after_paste(
                    clipboard_snapshot,
                    dictated_clipboard_change_count,
                );
                #[cfg(target_os = "windows")]
                restore_windows_clipboard_after_paste(
                    windows_clipboard_snapshot,
                    dictated_clipboard_sequence_number,
                    windows_clipboard_owner,
                );
                return Err(error);
            }
        };
    }

    match behavior {
        InsertBehavior::ClipboardOnly => Ok(InsertionReport::copied(None)),
        InsertBehavior::Paste => {
            if !can_paste {
                return Ok(InsertionReport::copied(warning));
            }

            // Give the system clipboard a moment to propagate before sending Cmd/Ctrl+V.
            thread::sleep(Duration::from_millis(120));
            ensure_current!();

            #[cfg(target_os = "windows")]
            {
                let focus = refocus_paste_target(paste_target);
                ensure_current!();
                if focus.is_err() {
                    return Ok(InsertionReport::copied(Some(
                        InsertionWarning::PasteUnavailable,
                    )));
                }

                if simulate_paste_with_retry(3, &check_active, simulate_paste).is_err() {
                    ensure_current!();
                    return Ok(InsertionReport::copied(Some(
                        InsertionWarning::PasteUnavailable,
                    )));
                }

                restore_windows_clipboard_after_paste(
                    windows_clipboard_snapshot,
                    dictated_clipboard_sequence_number,
                    windows_clipboard_owner,
                );
                Ok(InsertionReport::pasted())
            }

            #[cfg(not(target_os = "windows"))]
            {
                let focus = refocus_paste_target(paste_target);
                ensure_current!();
                if focus.is_err() {
                    return Ok(InsertionReport::copied(Some(
                        InsertionWarning::PasteUnavailable,
                    )));
                }

                match simulate_paste_with_retry(3, &check_active, simulate_paste) {
                    Ok(()) => {
                        #[cfg(target_os = "macos")]
                        restore_macos_clipboard_after_paste(
                            clipboard_snapshot,
                            dictated_clipboard_change_count,
                        );
                        Ok(InsertionReport::pasted())
                    }
                    Err(error) => {
                        ensure_current!();
                        let _ = error;
                        Ok(InsertionReport::copied(Some(
                            paste_warning(auto_paste_allowed(), accessibility_trusted())
                                .unwrap_or(InsertionWarning::PasteUnavailable),
                        )))
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn restore_windows_clipboard_after_paste(
    clipboard_snapshot: Option<windows_clipboard_restore::ClipboardSnapshot>,
    dictated_clipboard_sequence_number: u32,
    clipboard_owner: HWND,
) {
    let Some(snapshot) = clipboard_snapshot else {
        return;
    };

    // SendInput queues the shortcut, so the destination needs a moment to
    // consume the temporary dictation text before the clipboard is restored.
    thread::sleep(Duration::from_millis(300));

    // A sequence-number change means the user or another app copied something
    // newer while the paste was in flight. Never overwrite that content.
    if let Err(error) =
        snapshot.restore_if_unchanged(dictated_clipboard_sequence_number, clipboard_owner)
    {
        eprintln!("failed to restore clipboard after auto-paste: {error:#}");
    }
}

#[cfg(target_os = "windows")]
fn windows_clipboard_owner(app: &AppHandle) -> Result<HWND> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| anyhow!("main window is unavailable for Windows clipboard ownership"))?;
    let hwnd = window
        .hwnd()
        .map_err(|error| anyhow!("could not get main window handle: {error}"))?;
    Ok(hwnd.0)
}

#[cfg(target_os = "macos")]
fn restore_macos_clipboard_after_paste(
    clipboard_snapshot: Option<ClipboardSnapshot>,
    dictated_clipboard_change_count: isize,
) {
    let Some(snapshot) = clipboard_snapshot else {
        return;
    };

    // CGEventPost queues the shortcut. Give the target app time to consume
    // the dictated text before replacing the temporary clipboard contents.
    thread::sleep(Duration::from_millis(300));

    // The change-count guard protects content copied by the user (or another
    // app) while the paste was in flight.
    if let Err(error) = snapshot.restore_if_unchanged(dictated_clipboard_change_count) {
        eprintln!("failed to restore clipboard after auto-paste: {error:#}");
    }
}

fn auto_paste_allowed() -> bool {
    crate::platform::auto_paste_supported()
}

/// Whether the OS currently trusts Blabber to synthesize keystrokes (i.e.
/// auto-paste can work). macOS gates this behind Accessibility access; other
/// platforms don't have an equivalent runtime gate, so they report `true`.
#[cfg(target_os = "macos")]
pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

#[cfg(not(target_os = "macos"))]
pub fn accessibility_trusted() -> bool {
    true
}

/// Open the OS pane where the user grants Accessibility access. No-op on
/// platforms without that concept.
pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
}

fn should_attempt_paste(behavior: InsertBehavior, auto_paste_supported: bool) -> bool {
    matches!(behavior, InsertBehavior::Paste) && auto_paste_supported
}

fn simulate_paste_with_retry(
    attempts: usize,
    check_active: &impl Fn() -> Result<()>,
    mut paste: impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut last_error = None;
    for index in 0..attempts {
        check_active()?;
        match paste() {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if index + 1 < attempts {
                    thread::sleep(Duration::from_millis(120));
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("paste simulation failed")))
}

fn refocus_paste_target(paste_target: Option<&PasteTarget>) -> Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(PasteTarget::BundleId(bundle_id)) = paste_target {
        focus_bundle_id(bundle_id)?;
        thread::sleep(Duration::from_millis(180));
    }

    #[cfg(target_os = "windows")]
    if let Some(PasteTarget::WindowHandle(handle)) = paste_target {
        focus_window_handle(*handle)?;
        thread::sleep(Duration::from_millis(180));
    }

    Ok(())
}

fn simulate_paste() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        simulate_native_macos_paste()
    }

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL,
            VK_V,
        };

        unsafe {
            let mut inputs = [
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VK_CONTROL as u16,
                            wScan: 0,
                            dwFlags: 0,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VK_V as u16,
                            wScan: 0,
                            dwFlags: 0,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VK_V as u16,
                            wScan: 0,
                            dwFlags: KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                },
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VK_CONTROL as u16,
                            wScan: 0,
                            dwFlags: KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                },
            ];
            let sent = SendInput(
                inputs.len() as u32,
                inputs.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            );
            if sent == inputs.len() as u32 {
                return Ok(());
            }
        }
        Err(anyhow!(
            "SendInput returned {}",
            std::io::Error::last_os_error()
        ))
    }

    #[cfg(target_os = "linux")]
    {
        // This path is only reached when platform::auto_paste_supported() is true.
        // Linux currently enables it for X11 only; Wayland returns ClipboardOnly
        // before synthetic key input is attempted.
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|err| anyhow!("enigo init failed: {err:?}"))?;
        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|err| anyhow!("ctrl press failed: {err:?}"))?;
        let v_result = enigo.key(Key::Unicode('v'), Direction::Click);
        let release_result = enigo.key(Key::Control, Direction::Release);
        v_result.map_err(|err| anyhow!("v click failed: {err:?}"))?;
        release_result.map_err(|err| anyhow!("ctrl release failed: {err:?}"))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err(anyhow!("paste simulation is unsupported on this platform"))
    }
}

#[cfg(target_os = "macos")]
fn simulate_native_macos_paste() -> Result<()> {
    unsafe {
        if !AXIsProcessTrusted() {
            return Err(anyhow!(InsertionWarning::AccessibilityRequired.message()));
        }

        let key_down = CGEventCreateKeyboardEvent(std::ptr::null(), MACOS_KEYCODE_V, true);
        let key_up = CGEventCreateKeyboardEvent(std::ptr::null(), MACOS_KEYCODE_V, false);

        if key_down.is_null() || key_up.is_null() {
            if !key_down.is_null() {
                CFRelease(key_down);
            }
            if !key_up.is_null() {
                CFRelease(key_up);
            }
            return Err(anyhow!(
                "paste simulation failed: could not create keyboard event"
            ));
        }

        CGEventSetFlags(key_down, KCG_EVENT_FLAG_MASK_COMMAND);
        CGEventSetFlags(key_up, KCG_EVENT_FLAG_MASK_COMMAND);
        CGEventPost(KCG_HID_EVENT_TAP, key_down);
        CGEventPost(KCG_HID_EVENT_TAP, key_up);
        CFRelease(key_down);
        CFRelease(key_up);
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn detect_frontmost_paste_target() -> Option<PasteTarget> {
    let script = r#"tell application "System Events" to get bundle identifier of first application process whose frontmost is true"#;
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let bundle_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if bundle_id.is_empty() {
        None
    } else {
        Some(PasteTarget::BundleId(bundle_id))
    }
}

#[cfg(target_os = "windows")]
pub fn detect_frontmost_paste_target() -> Option<PasteTarget> {
    let handle = unsafe { GetForegroundWindow() };
    if handle.is_null() {
        None
    } else {
        Some(PasteTarget::WindowHandle(handle as isize))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn detect_frontmost_paste_target() -> Option<PasteTarget> {
    None
}

#[cfg(target_os = "macos")]
fn focus_bundle_id(bundle_id: &str) -> Result<()> {
    let status = Command::new("open").arg("-b").arg(bundle_id).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("failed to reactivate target app for paste"))
    }
}

#[cfg(target_os = "windows")]
fn focus_window_handle(handle: isize) -> Result<()> {
    let handle = handle as HWND;
    unsafe {
        if handle.is_null() || IsWindow(handle) == 0 {
            return Err(anyhow!("saved target window is no longer available"));
        }

        ShowWindow(handle, SW_RESTORE);
        if SetForegroundWindow(handle) == 0 {
            return Err(anyhow!(
                "could not reactivate the original target window before paste"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_accessibility_keeps_a_copy_and_explains_why_paste_was_skipped() {
        let warning = paste_warning(true, false);
        assert!(!should_attempt_paste(
            InsertBehavior::Paste,
            warning.is_none()
        ));
        let report = InsertionReport::copied(warning);
        assert!(matches!(report.outcome, InsertionOutcome::ClipboardOnly));
        assert_eq!(
            report.warning,
            Some(InsertionWarning::AccessibilityRequired)
        );
        assert!(report.warning.unwrap().message().contains("Grant access"));
        assert_eq!(paste_warning(true, true), None);
        assert_eq!(
            paste_warning(false, true),
            Some(InsertionWarning::PasteUnavailable)
        );
        assert!(InsertionReport::copied(None).warning.is_none());
    }

    #[test]
    fn cancellation_before_a_paste_or_retry_never_sends_another_key_event() {
        let active = std::cell::Cell::new(false);
        let attempts = std::cell::Cell::new(0);
        let check = || {
            if active.get() {
                Ok(())
            } else {
                Err(anyhow!("canceled"))
            }
        };
        let paste = || {
            attempts.set(attempts.get() + 1);
            active.set(false);
            Err(anyhow!("temporary insertion failure"))
        };
        assert!(simulate_paste_with_retry(3, &check, paste).is_err());
        assert_eq!(attempts.get(), 0);
        active.set(true);
        assert!(simulate_paste_with_retry(3, &check, paste).is_err());
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn paste_behavior_falls_back_when_auto_paste_is_unsupported() {
        assert!(!should_attempt_paste(InsertBehavior::Paste, false));
    }

    #[test]
    fn paste_behavior_attempts_when_auto_paste_is_supported() {
        assert!(should_attempt_paste(InsertBehavior::Paste, true));
        assert!(!should_attempt_paste(InsertBehavior::ClipboardOnly, true));
    }
}
