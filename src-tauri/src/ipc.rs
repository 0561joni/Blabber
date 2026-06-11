//! Single-instance IPC over a Unix domain socket.
//!
//! Compiled only on Linux. Used to let `blabber --dictate-toggle` wake the
//! already-running instance and toggle dictation — giving Wayland users a
//! real keyboard-shortcut trigger via their compositor's native binding.
//!
//! ## Design
//!
//! At startup the app binds a Unix socket at
//! `$XDG_RUNTIME_DIR/blabber-dictate.sock` (falling back to
//! `/tmp/blabber-dictate-$USER.sock`).  Any stale file from a previous crash
//! is removed first.  A background thread accepts connections and dispatches
//! simple newline-terminated commands.
//!
//! When a second invocation starts with `--dictate-toggle`, it connects to
//! the socket, writes `toggle\n`, and exits immediately.  The running
//! instance reads the command and calls [`crate::dictation::QuickDictationController::ui_toggle`].

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use crate::dictation::QuickDictationController;

/// Return the canonical socket path for this user session.
pub fn socket_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        Some(PathBuf::from(dir).join("blabber-dictate.sock"))
    } else {
        // Fallback for non-systemd / bare-metal Linux environments.
        let user = std::env::var("USER").unwrap_or_else(|_| "blabber".into());
        Some(PathBuf::from(format!("/tmp/blabber-dictate-{user}.sock")))
    }
}

/// Send a `toggle` command to a running Blabber instance via the IPC socket.
///
/// Returns `0` on success, `1` if no running instance is found (socket absent
/// or connection refused).  Intended to be passed directly to
/// `std::process::exit`.
pub fn send_toggle_command() -> i32 {
    let path = match socket_path() {
        Some(p) => p,
        None => return 1,
    };

    match UnixStream::connect(&path) {
        Ok(mut stream) => {
            // Fire-and-forget: write the command and exit.
            let _ = stream.write_all(b"toggle\n");
            0
        }
        Err(_) => {
            eprintln!(
                "blabber --dictate-toggle: no running Blabber instance found \
                 (expected socket at {path:?})"
            );
            1
        }
    }
}

/// Bind the Unix socket and start a background listener thread.
///
/// Any stale socket file from a previous run is removed before binding.
/// Errors are logged and swallowed — a listener failure must never prevent
/// the app from starting.
pub fn start_ipc_listener(controller: QuickDictationController) {
    let path = match socket_path() {
        Some(p) => p,
        None => return,
    };

    // Remove stale socket left by a crash or force-kill.
    let _ = std::fs::remove_file(&path);

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("blabber IPC: could not bind socket at {path:?}: {e}");
            return;
        }
    };

    let cleanup_path = path.clone();
    std::thread::Builder::new()
        .name("blabber-ipc-listener".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let controller = controller.clone();
                        let _ = std::thread::spawn(move || handle_ipc_stream(stream, controller));
                    }
                    Err(e) => {
                        eprintln!("blabber IPC: accept error: {e}");
                        break;
                    }
                }
            }
            // Clean up socket file on graceful shutdown.
            let _ = std::fs::remove_file(&cleanup_path);
        })
        .expect("failed to spawn IPC listener thread");
}

fn handle_ipc_stream(stream: UnixStream, controller: QuickDictationController) {
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        match line.trim() {
            "toggle" => {
                let _ = controller.ui_toggle();
            }
            other => {
                eprintln!("blabber IPC: unknown command {other:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_uses_xdg_runtime_dir() {
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        let path = socket_path().expect("should return a path");
        assert_eq!(path, PathBuf::from("/run/user/1000/blabber-dictate.sock"));
    }

    #[test]
    fn socket_path_falls_back_to_tmp() {
        // Remove XDG_RUNTIME_DIR to exercise the fallback branch.
        std::env::remove_var("XDG_RUNTIME_DIR");
        std::env::set_var("USER", "testuser");
        let path = socket_path().expect("should return a path");
        assert_eq!(path, PathBuf::from("/tmp/blabber-dictate-testuser.sock"));
    }
}
