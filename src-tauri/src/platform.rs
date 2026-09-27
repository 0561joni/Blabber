pub fn is_wayland() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    is_wayland_from_env(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
    )
}

pub fn is_gnome() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    is_gnome_from_env(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref())
}

pub fn has_appindicator_hint() -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    if !is_gnome() {
        return true;
    }
    has_appindicator_from_env(std::env::vars().map(|(k, _)| k))
}

pub fn auto_paste_supported() -> bool {
    auto_paste_supported_for(
        cfg!(target_os = "linux"),
        cfg!(any(target_os = "macos", target_os = "windows")),
        is_wayland(),
    )
}

pub fn global_shortcut_supported() -> bool {
    global_shortcut_supported_for(
        cfg!(target_os = "linux"),
        cfg!(any(target_os = "macos", target_os = "windows")),
        is_wayland(),
    )
}

pub fn dictate_toggle_executable() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

pub fn dictate_toggle_command() -> Option<String> {
    dictate_toggle_executable().map(|executable| format_dictate_toggle_command(&executable))
}

fn format_dictate_toggle_command(executable: &str) -> String {
    format!("{} --dictate-toggle", shell_quote(executable))
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '+'))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

fn auto_paste_supported_for(is_linux: bool, is_macos_or_windows: bool, is_wayland: bool) -> bool {
    if is_linux {
        !is_wayland
    } else {
        is_macos_or_windows
    }
}

fn global_shortcut_supported_for(
    is_linux: bool,
    is_macos_or_windows: bool,
    is_wayland: bool,
) -> bool {
    if is_linux {
        !is_wayland
    } else {
        is_macos_or_windows
    }
}

fn is_wayland_from_env(xdg_session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    if let Some(value) = xdg_session_type {
        if value.eq_ignore_ascii_case("wayland") {
            return true;
        }
        if value.eq_ignore_ascii_case("x11") {
            return false;
        }
    }
    matches!(wayland_display, Some(value) if !value.is_empty())
}

fn is_gnome_from_env(xdg_current_desktop: Option<&str>) -> bool {
    let Some(value) = xdg_current_desktop else {
        return false;
    };
    value.split(':').any(|token| {
        matches!(
            token.trim().to_ascii_uppercase().as_str(),
            "GNOME" | "GNOME-CLASSIC" | "GNOME-FLASHBACK" | "UNITY"
        )
    })
}

fn has_appindicator_from_env<I>(mut env_keys: I) -> bool
where
    I: Iterator<Item = String>,
{
    env_keys.any(|key| key.starts_with("GNOME_SHELL_EXTENSION_") || key == "APPINDICATOR_SUPPORT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_detected_from_xdg_session_type() {
        assert!(is_wayland_from_env(Some("wayland"), None));
        assert!(is_wayland_from_env(Some("Wayland"), None));
        assert!(is_wayland_from_env(Some("WAYLAND"), None));
    }

    #[test]
    fn x11_detected_from_xdg_session_type() {
        assert!(!is_wayland_from_env(Some("x11"), None));
        assert!(!is_wayland_from_env(Some("X11"), None));
    }

    #[test]
    fn x11_overrides_wayland_display_fallback() {
        assert!(!is_wayland_from_env(Some("x11"), Some("wayland-0")));
    }

    #[test]
    fn linux_wayland_disables_global_shortcut_and_auto_paste() {
        assert!(!global_shortcut_supported_for(true, false, true));
        assert!(!auto_paste_supported_for(true, false, true));
    }

    #[test]
    fn linux_x11_keeps_global_shortcut_and_auto_paste_enabled() {
        assert!(global_shortcut_supported_for(true, false, false));
        assert!(auto_paste_supported_for(true, false, false));
    }

    #[test]
    fn wayland_inferred_from_display_when_session_type_missing() {
        assert!(is_wayland_from_env(None, Some("wayland-0")));
        assert!(!is_wayland_from_env(None, Some("")));
        assert!(!is_wayland_from_env(None, None));
    }

    #[test]
    fn unknown_session_type_falls_back_to_display() {
        assert!(is_wayland_from_env(Some("tty"), Some("wayland-1")));
        assert!(!is_wayland_from_env(Some("tty"), None));
    }

    #[test]
    fn gnome_detected_in_simple_value() {
        assert!(is_gnome_from_env(Some("GNOME")));
        assert!(is_gnome_from_env(Some("Unity")));
    }

    #[test]
    fn gnome_detected_in_colon_separated_value() {
        assert!(is_gnome_from_env(Some("GNOME-Classic:GNOME")));
        assert!(is_gnome_from_env(Some("ubuntu:GNOME")));
        assert!(is_gnome_from_env(Some("GNOME-Flashback:GNOME")));
    }

    #[test]
    fn non_gnome_desktops_not_detected() {
        assert!(!is_gnome_from_env(Some("KDE")));
        assert!(!is_gnome_from_env(Some("XFCE")));
        assert!(!is_gnome_from_env(Some("X-Cinnamon")));
        assert!(!is_gnome_from_env(Some("MATE")));
        assert!(!is_gnome_from_env(None));
        assert!(!is_gnome_from_env(Some("")));
    }

    #[test]
    fn appindicator_detected_via_extension_env_var() {
        let keys = vec![
            "PATH".to_string(),
            "GNOME_SHELL_EXTENSION_appindicatorsupport".to_string(),
            "HOME".to_string(),
        ];
        assert!(has_appindicator_from_env(keys.into_iter()));
    }

    #[test]
    fn appindicator_detected_via_explicit_marker() {
        let keys = vec!["PATH".to_string(), "APPINDICATOR_SUPPORT".to_string()];
        assert!(has_appindicator_from_env(keys.into_iter()));
    }

    #[test]
    fn appindicator_absent_in_bare_gnome_session() {
        let keys = vec![
            "PATH".to_string(),
            "HOME".to_string(),
            "XDG_CURRENT_DESKTOP".to_string(),
        ];
        assert!(!has_appindicator_from_env(keys.into_iter()));
    }

    #[test]
    fn dictate_toggle_command_leaves_safe_paths_unquoted() {
        assert_eq!(
            format_dictate_toggle_command("/usr/bin/blabber"),
            "/usr/bin/blabber --dictate-toggle"
        );
    }

    #[test]
    fn dictate_toggle_command_quotes_paths_with_spaces() {
        assert_eq!(
            format_dictate_toggle_command("/opt/My Apps/Blabber"),
            "'/opt/My Apps/Blabber' --dictate-toggle"
        );
    }

    #[test]
    fn dictate_toggle_command_escapes_single_quotes() {
        assert_eq!(
            format_dictate_toggle_command("/opt/Bob's Apps/Blabber"),
            "'/opt/Bob'\\''s Apps/Blabber' --dictate-toggle"
        );
    }

    #[test]
    fn dictate_toggle_command_resolves_current_executable() {
        let command = dictate_toggle_command().expect("test process executable should resolve");
        assert!(command.ends_with(" --dictate-toggle"));
        assert!(command.len() > " --dictate-toggle".len());
    }
}
