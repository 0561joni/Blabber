# Blabber workspace redesign

The main window now separates Dictate, Transcribe files, Library, and Vocabulary, with Settings at the bottom of the sidebar. Recording and file-job state live above the screens so navigation preserves ongoing work. The launch workflow still follows the saved preference.

Shared styles define light and dark surfaces, typography, focus rings, control feedback, progress, and reduced motion. Appearance follows the system by default; persisted overrides synchronize with the startup window and floating overlay. Existing databases receive `system` defaults for `appearance` and `motion_preference` without changing existing preferences.

Dictation uses actual microphone levels and explicit recording, processing, result, and recovery states. File rows preserve failure details and retry the source file, requesting reselection if it is missing. Library provides search, workflow filters, a reading pane, and Back navigation at narrow widths while retaining transcript and speaker actions. Settings are grouped by purpose, and unsuccessful saves preserve editable input without confirming success.

Start, stop, completion, and error cues share one native sound service. It deduplicates operation outcomes, coalesces nearby background outcomes, and gates playback during microphone capture. Cues can be previewed in Appearance & feedback. Reset invalidates late manual results and shortcut insertion work. Text feedback remains available with sounds or animation disabled.

## Verification performed

- `npm test`: 74 tests passed across 14 files, including navigation during recording, reset during transcription, failed saves, file progress/cancellation/retry states, search/filtering, transcript actions, appearance synchronization, and reduced motion.
- `npm run build`: TypeScript and the production build passed for the main window, startup screen, and overlay.
- `cargo check --manifest-path src-tauri/Cargo.toml --offline`: passed on the available macOS host.
- `cargo test --manifest-path src-tauri/Cargo.toml --offline --lib`: 123 passed; one real-model Qwen test is ignored unless its model and audio environment variables are supplied. The download-resume test used localhost socket permission.
- Browser preview: inspected both themes, recording feedback and navigation, multiple file jobs, Library, Vocabulary, and the reader at 760px width. Back restored focus to the selected transcript.
- `git diff --check`: passed.

## Native smoke checks still required

The browser preview uses mock data and cannot validate microphone hardware, operating-system permissions, paste targets, or audible playback. Run `npm run tauri dev` for these checks:

1. Record through completion, cancellation, silence, and microphone permission denial. Switch screens during recording and processing; reset processing and confirm no late result is pasted.
2. Exercise shortcut insertion in another app, clipboard fallback, and insertion failure. Confirm the main window and overlay distinguish Transcript ready, Copied, and Pasted.
3. Listen to all four cues. Finish background jobs during capture and confirm unrelated cues are suppressed and the recorded audio contains no feedback tones.
4. Transcribe multiple real files, cancel and retry them, remove an original source before retry, and repeat with history disabled.
5. Change the operating-system theme while all windows are available. Verify startup recovery, reduced motion, keyboard focus, and status announcements with a screen reader.

Windows and Linux native smoke checks were not available in this environment. Installed-model behavior, local processing, and transcript-retention policy remain in the existing backend paths.
