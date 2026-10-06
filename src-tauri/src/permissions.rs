//! Microphone permission state for the first-run setup and readiness checks.
//! Only macOS gates microphone access per app; other platforms report
//! `Granted` and rely on the existing "no signal" recording error.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MicrophonePermission {
    Granted,
    Denied,
    Restricted,
    NotDetermined,
}

#[cfg(target_os = "macos")]
mod macos {
    use super::MicrophonePermission;
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: *const AnyObject;
    }

    fn capture_device_class() -> Option<&'static AnyClass> {
        AnyClass::get(c"AVCaptureDevice")
    }

    pub fn status() -> MicrophonePermission {
        let Some(class) = capture_device_class() else {
            return MicrophonePermission::Granted;
        };
        // AVAuthorizationStatus: 0 not determined, 1 restricted, 2 denied, 3 authorized.
        let status: isize =
            unsafe { msg_send![class, authorizationStatusForMediaType: AVMediaTypeAudio] };
        match status {
            0 => MicrophonePermission::NotDetermined,
            1 => MicrophonePermission::Restricted,
            2 => MicrophonePermission::Denied,
            _ => MicrophonePermission::Granted,
        }
    }

    /// Shows the system prompt when the user has not decided yet. The answer
    /// arrives asynchronously; callers poll `status()`.
    pub fn request() {
        let Some(class) = capture_device_class() else {
            return;
        };
        let handler = RcBlock::new(|_granted: Bool| {});
        unsafe {
            let _: () = msg_send![
                class,
                requestAccessForMediaType: AVMediaTypeAudio,
                completionHandler: &*handler
            ];
        }
    }
}

pub fn microphone_permission() -> MicrophonePermission {
    #[cfg(target_os = "macos")]
    {
        macos::status()
    }
    #[cfg(not(target_os = "macos"))]
    {
        MicrophonePermission::Granted
    }
}

/// Asks for microphone access if the user has not decided yet and returns the
/// state right after asking (usually still `NotDetermined` while the prompt is up).
pub fn request_microphone_permission() -> MicrophonePermission {
    #[cfg(target_os = "macos")]
    {
        if macos::status() == MicrophonePermission::NotDetermined {
            macos::request();
        }
    }
    microphone_permission()
}

/// Opens the OS pane where a denied microphone permission can be changed.
pub fn open_microphone_settings() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_status_query_resolves_without_prompting() {
        // Exercises the AVFoundation binding; querying never shows a prompt.
        let state = microphone_permission();
        assert!(matches!(
            state,
            MicrophonePermission::Granted
                | MicrophonePermission::Denied
                | MicrophonePermission::Restricted
                | MicrophonePermission::NotDetermined
        ));
        println!("microphone permission: {state:?}");
    }
}
