# SpeechToText

Local-first Tauri transcription app for macOS, Windows, and Linux.

Current scope:

- Tauri 2 + React + TypeScript scaffold
- Rust backend module layout for later audio/ASR work
- SQLite-backed settings and transcript history
- Minimal Home, Settings, and History screens
- Phase 4 whisper.cpp integration through `whisper-rs`

## Local development

Use the repo's supported Node version before installing dependencies:

```bash
node -v
cat .nvmrc
```

Recommended runtime: `Node 22 LTS` (`22.19.0` in `.nvmrc`).

Frontend only:

```bash
npm install
npm run dev
```

Full Tauri app:

```bash
npm install
npm run tauri dev
```

Platform prerequisites:

- macOS: working Rust toolchain, accepted Xcode license, `macOS 11.0+`
- Windows: working Rust MSVC toolchain, Visual Studio C++ build tools, WebView2 runtime
- Linux: working Rust toolchain plus the system packages below

The macOS app targets `macOS 11.0+` because the current native Whisper/ggml toolchain for Apple Silicon release builds requires a newer macOS deployment target.

### Linux build dependencies

**Debian / Ubuntu** (matches the canonical Tauri 2 list, plus ALSA for the
microphone and chime):

```bash
sudo apt install \
  libwebkit2gtk-4.1-dev \
  libasound2-dev \
  libxdo-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  build-essential \
  pkg-config \
  curl wget file
```

`libwebkit2gtk-4.1-dev` transitively pulls in GTK 3, GDK, ATK, Cairo, Pango,
GLib, libsoup 3, and JavaScriptCore — there's no need to list them
individually on Debian-based distros.

**Fedora / RHEL:**

```bash
sudo dnf install \
  webkit2gtk4.1-devel \
  alsa-lib-devel \
  libxdo-devel \
  libayatana-appindicator-gtk3-devel \
  librsvg2-devel \
  openssl-devel \
  gcc gcc-c++ pkgconf-pkg-config \
  curl wget file
```

If your `dnf` version doesn't find `libayatana-appindicator-gtk3-devel`, try
`libappindicator-gtk3-devel` instead (older repos use that name).

### Linux runtime dependencies

Packaged Debian builds declare the runtime packages Blabber expects:

- GTK 3 and WebKitGTK 4.1 for the Tauri webview.
- AppIndicator/Ayatana for tray icons. GNOME also needs the
  [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/)
  because GNOME hides tray icons by default.
- ALSA, with PulseAudio or PipeWire compatibility available from the desktop
  session, for microphone capture and chimes.
- `ffmpeg` for fallback decoding of audio files that the native decoder cannot
  read directly.
- `xdg-utils` / `xdg-open` for opening folders and system locations from the app.

AppImage and manual installs cannot force host packages, so install the matching
packages through your distro if the app cannot start, open folders, capture
audio, show a tray icon, or decode a specific audio file.

### Linux session: X11 vs Wayland

Most modern distros default to Wayland. Check with:

```bash
echo $XDG_SESSION_TYPE   # prints "wayland" or "x11"
```

**On X11:** everything works — global push-to-talk shortcut, auto-paste, the works.

**On Wayland:**
- The **global push-to-talk shortcut is inactive** (Wayland doesn't expose the
  required hooks for Tauri's plugin yet). Use the **Hold to dictate** button on the
  Home screen instead — it dictates straight to your clipboard, ready for Ctrl+V.
- To use a compositor-level shortcut, open Settings and bind the exact command
  shown there, for example `"/path/to/Blabber" --dictate-toggle`. This avoids
  assuming a `blabber` wrapper exists on `PATH`.
- Auto-paste is disabled on Wayland for now. Dictation copies to the clipboard;
  press Ctrl+V in the target app to paste.

If you really need the global shortcut, switch login session: at the login screen,
click the gear icon and choose "Ubuntu on Xorg" / "GNOME on Xorg" before logging in.

## Platform support

| Feature | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Tray app | Yes | Yes | KDE/XFCE: yes; GNOME: needs the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/) |
| Main window | Yes | Yes | Yes |
| Global shortcut dictation | Yes | Yes | **X11 only** (Wayland: use the in-app Hold-to-dictate button) |
| Shortcut overlay | Yes | Yes | Yes |
| Auto paste after dictation | Yes | Yes | X11: yes; Wayland: clipboard-only |
| Launch at login | Yes | Yes | Yes (XDG autostart `.desktop`) |
| GPU acceleration | Metal | CUDA | None (CPU only) |
| Model downloads and file transcription | Yes | Yes | Yes |
| MOSS Transcribe + Diarize 0.9B F16 | Yes | Yes | Yes |
| VibeVoice-ASR 8-bit MLX | Apple Silicon, macOS 14+ | No | No |

Windows packaged builds use the bundled `.ico` app icon. macOS packaged builds use the `.icns`
icon.

On Linux, fresh installs prefer Whisper Small so CPU inference stays responsive. You can
pick larger models in Settings if your CPU can handle them.

## Phase 4 model requirement

The app now uses a real local Whisper backend. To transcribe audio, place at least one
whisper.cpp `ggml-*.bin` model file into the app's models directory and restart the app.

Examples:

- `ggml-small.bin` -> `balanced`
- `ggml-medium.bin` or `ggml-large-v3.bin` -> `accurate`

MOSS Transcribe + Diarize is a 1.83 GB CPU model available for shortcut dictation,
Quick Dictate, and files up to 90 minutes. VibeVoice-ASR is a 9.52 GB file-only model
for Apple Silicon on macOS 14 or newer; 32 GB unified memory is recommended. Both use
their own timestamps, speakers, automatic language detection, and Blabber vocabulary
context. Their native speaker labels are preserved even when standalone speaker
post-processing is off.

The active models directory is shown in the Home screen diagnostics once the Tauri app is running.
