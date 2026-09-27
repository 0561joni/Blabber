use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tauri::AppHandle;

pub fn sync_launch_at_login(app: &AppHandle, enabled: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        sync_macos_launch_agent(app, enabled)
    }
    #[cfg(target_os = "windows")]
    {
        sync_windows_run_key(app, enabled)
    }
    #[cfg(target_os = "linux")]
    {
        sync_linux_autostart(app, enabled)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (app, enabled);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn sync_macos_launch_agent(app: &AppHandle, enabled: bool) -> Result<()> {
    let identifier = app.config().identifier.clone();
    let plist_path = launch_agent_path(&identifier)?;
    if enabled {
        let executable = std::env::current_exe().context("failed to resolve current executable")?;
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{identifier}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
            executable.display()
        );
        if let Some(parent) = plist_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(plist_path, plist)?;
    } else if plist_path.exists() {
        fs::remove_file(plist_path)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_agent_path(identifier: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{identifier}.plist")))
}

#[cfg(target_os = "windows")]
fn sync_windows_run_key(app: &AppHandle, enabled: bool) -> Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, REG_BINARY};
    use winreg::{RegKey, RegValue};

    const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const STARTUP_APPROVED_RUN_KEY_PATH: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
    const STARTUP_APPROVED_ENABLED_VALUE: &[u8] = &[
        0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey(RUN_KEY_PATH)
        .context("failed to open Windows Run startup registry key")?;
    let (startup_approved_key, _) = hkcu
        .create_subkey(STARTUP_APPROVED_RUN_KEY_PATH)
        .context("failed to open Windows StartupApproved registry key")?;
    let identifier = app.config().identifier.clone();

    if enabled {
        let executable = std::env::current_exe().context("failed to resolve current executable")?;
        run_key
            .set_value(&identifier, &format!("\"{}\"", executable.display()))
            .context("failed to write Windows Run startup value")?;
        let startup_approved_enabled = RegValue {
            vtype: REG_BINARY,
            bytes: STARTUP_APPROVED_ENABLED_VALUE.to_vec(),
        };
        startup_approved_key
            .set_raw_value(&identifier, &startup_approved_enabled)
            .context("failed to enable Windows StartupApproved value")?;
    } else {
        let _ = run_key.delete_value(&identifier);
        let _ = startup_approved_key.delete_value(&identifier);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn sync_linux_autostart(app: &AppHandle, enabled: bool) -> Result<()> {
    let identifier = app.config().identifier.clone();
    let path = linux_autostart_path_from(
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        &identifier,
    )?;
    if enabled {
        let executable = std::env::current_exe().context("failed to resolve current executable")?;
        let contents = linux_autostart_contents(&executable);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_autostart_path_from(
    xdg_config_home: Option<&str>,
    home: Option<&str>,
    identifier: &str,
) -> Result<PathBuf> {
    let base = xdg_config_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|value| !value.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })
        .context("neither XDG_CONFIG_HOME nor HOME is set")?;
    Ok(base.join("autostart").join(format!("{identifier}.desktop")))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_autostart_contents(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Name=Blabber\n\
Exec={}\n\
X-GNOME-Autostart-enabled=true\n\
NoDisplay=false\n",
        quote_exec(&executable.display().to_string())
    )
}

/// Quote a path for the XDG Desktop Entry `Exec=` field.
/// Per the freedesktop spec, reserved characters (notably space, tab, quote,
/// backslash) must be escaped inside double quotes, otherwise launchers split
/// the value into multiple argv entries on whitespace.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn quote_exec(path: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    if !path.chars().any(|c| RESERVED.contains(&c)) {
        return path.to_string();
    }
    let mut out = String::with_capacity(path.len() + 2);
    out.push('"');
    for ch in path.chars() {
        if ch == '"' || ch == '\\' || ch == '`' || ch == '$' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn xdg_config_home_takes_precedence_over_home() {
        let path =
            linux_autostart_path_from(Some("/tmp/xdg"), Some("/home/alice"), "com.example.blabber")
                .expect("path");
        assert_eq!(
            path,
            PathBuf::from("/tmp/xdg/autostart/com.example.blabber.desktop")
        );
    }

    #[test]
    fn falls_back_to_home_when_xdg_unset() {
        let path = linux_autostart_path_from(None, Some("/home/alice"), "com.example.blabber")
            .expect("path");
        assert_eq!(
            path,
            PathBuf::from("/home/alice/.config/autostart/com.example.blabber.desktop")
        );
    }

    #[test]
    fn empty_xdg_config_home_falls_back_to_home() {
        let path = linux_autostart_path_from(Some(""), Some("/home/alice"), "com.example.blabber")
            .expect("path");
        assert_eq!(
            path,
            PathBuf::from("/home/alice/.config/autostart/com.example.blabber.desktop")
        );
    }

    #[test]
    fn errors_when_neither_xdg_nor_home_set() {
        let result = linux_autostart_path_from(None, None, "com.example.blabber");
        assert!(result.is_err());
    }

    #[test]
    fn desktop_contents_have_required_keys() {
        let contents = linux_autostart_contents(Path::new("/usr/bin/blabber"));
        // XDG Desktop Entry spec required fields
        assert!(contents.starts_with("[Desktop Entry]\n"));
        assert!(contents.contains("Type=Application\n"));
        assert!(contents.contains("Name=Blabber\n"));
        assert!(contents.contains("Exec=/usr/bin/blabber\n"));
        // GNOME-specific opt-in
        assert!(contents.contains("X-GNOME-Autostart-enabled=true\n"));
        // Newline-terminated for POSIX cleanliness
        assert!(contents.ends_with('\n'));
    }

    #[test]
    fn desktop_contents_quote_paths_with_spaces() {
        let contents = linux_autostart_contents(Path::new("/opt/My Apps/blabber"));
        assert!(contents.contains("Exec=\"/opt/My Apps/blabber\"\n"));
    }

    #[test]
    fn desktop_contents_escape_reserved_characters() {
        let contents = linux_autostart_contents(Path::new("/opt/weird $name/`exe`"));
        // Backticks and dollar signs must be backslash-escaped inside the quotes
        // so that XDG launchers don't subshell them.
        assert!(contents.contains("Exec=\"/opt/weird \\$name/\\`exe\\`\"\n"));
    }

    #[test]
    fn quote_exec_leaves_simple_paths_unchanged() {
        assert_eq!(quote_exec("/usr/bin/blabber"), "/usr/bin/blabber");
        assert_eq!(
            quote_exec("/home/alice/.local/bin/blabber"),
            "/home/alice/.local/bin/blabber"
        );
    }

    #[test]
    fn quote_exec_handles_embedded_quotes_and_backslashes() {
        assert_eq!(quote_exec(r#"/opt/has"quote"#), r#""/opt/has\"quote""#);
        assert_eq!(quote_exec(r"/opt/has\back"), r#""/opt/has\\back""#);
    }
}
