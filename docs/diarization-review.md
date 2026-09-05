# File diarization and audio review

Implemented in September 2026. The application changes are ready for review; recording-based accuracy and end-to-end resource acceptance remain open pending an annotated corpus.

## User workflow

- Files and Library open `ReviewWorkspace`. Back restores the originating screen, scroll offset, and focused control. Saved titles can still be renamed and saved transcripts deleted.
- Standalone identification publishes a saved or session review reference immediately after ASR. Text can be read and copied while speakers run. Stopping identification keeps the transcript and any corrections made in the meantime. Native speaker-capable engines keep their combined result.
- Playback uses the original recording: play/pause, seek, ±10 seconds, elapsed/total time, speed, clickable timestamps, active passage highlighting, and opt-in following after manual scrolling.
- Speaker corrections apply to existing passages: names, new speakers, assignment, overlap, Unknown, merges, explicit linking, and session undo. Text and timestamps remain immutable.
- “Known speaker count” means automatic detection or an exact integer from 1–20. A retry uses the previous selection (including failed or canceled retries during the current app session) and preserves corrections by default. An explicit reset takes effect only after successful replacement and clears the old undo history.
- Unmatched named speakers remain in the roster for explicit linking. “Likely” and “Uncertain” describe model decisions without displaying a probability. Manually corrected passages are distinguished and excluded from the review filter.

## Backend and persistence

`ReviewStore` addresses results by saved transcript ID or session job ID. `transcript_reviews` stores a revisioned machine result plus the correction layer. SQLite's existing speaker tables remain an atomic effective projection, so clipboard and all exports see the same corrected labels. Manual passage IDs are also included in review/export metadata. Legacy named speakers migrate lazily without dropping their names.

Speaker identities match using unioned speech-duration evidence. Only mutual best matches with at least 80% overlap in both directions reuse an identity. Numbering is never identity evidence. Manual overrides and merges remain authoritative across a replacement, and the replacement loads the latest correction layer after inference.

File jobs and speaker retries share FIFO admission for heavy work. Controllers own their lifetime independently of React screens. Starts are idempotent for file IDs; duplicate active retries are rejected. Revisioned references replace completed transcript payloads in status events. The frontend fetches content per revision and uses event subscriptions with non-overlapping polling as a fallback.

Retry failures, no speech, and cancellation preserve the prior machine result and corrections. A retry cancellation signals before waiting on the commit gate; SQLite checks that signal immediately before commit. If commit already won, the terminal completed state remains authoritative. Startup marks interrupted initial speaker processing as stopped while retaining the saved text. Session results and edits last until dismissed or app exit.

Audio preparation runs after queue admission. It records duration and SHA-256, reports decoding failures, and writes a temporary float WAV shared by compatible ASR and speaker workers. VibeVoice retains its original-file loader. Packet normalization avoids retaining a full source-rate multichannel sample buffer. Worker heartbeats convey liveness; their completion waits are interruptible.

Playback resolves to an unguessable token on a loopback HTTP endpoint with byte-range support and bounded concurrent connections. Relinking checks the source fingerprint. Unsupported playback codecs can use a temporary PCM WAV. Tokens and temporary files are released, and interrupted preparation/playback assets are cleaned at startup. Older transcripts without fingerprints remain readable/editable but cannot be silently relinked to unverified audio.

## Verification performed

Final checks: 94 frontend tests and 151 Rust tests passed. Six Rust tests are opt-in: three pre-existing model/hardware tests and three new benchmark/codec probes. The three new probes were run separately and passed. The frontend production build and native binary/example compilation passed; `git diff --check` is clean.

- Frontend tests cover shared saved/session review, publication before content hydration, correction operations, retry restoration, stale responses/events, polling, player controls, codec fallback, relinking errors, resource cleanup, keyboard focus, and a bounded 10,000-passage virtual window.
- Rust tests cover correction persistence and undo, concurrent edits/reruns, conservative and ambiguous identity matching, no-speech consistency, transaction rollback during cancellation, startup recovery, FIFO cancellation, HTTP ranges, source verification, and every export format.
- The native `review_workflow_smoke` example runs real Tauri controllers, SQLite, and subprocess protocols with deterministic inference. It verified early text, stopping initial speaker work, duplicate starts, edits during a retry, canceled/failed/empty retries, session-only results, and dismissal. It creates a temporary database and never initializes the user's AppState.
- Generated WAV, MP3, M4A, and OPUS fixtures passed native decoding, fingerprint checks, original media resolution, temporary WAV fallback, and cleanup. HTTP transport is tested separately. This is not an audible playback test in every supported operating system's webview.
- Browser inspection covered Files and Library navigation, restored focus, speaker editing, a 780-pixel window, and light/dark passage styling. The dedicated development fixture uses 10,000 passages with varying text lengths and is omitted from production entry points.

## Measurements on the development Mac

Measured on an Apple M3 Pro Mac with 18 GB memory (Mac15,6). These are synthetic measurements, not transcription accuracy or full-engine runtime results. Rust numbers use the debug test build.

| Synthetic interval fixture | Previous full scan | Boundary sweep |
| --- | ---: | ---: |
| 30 minutes, 2–8 speakers, 7,200 intervals | 364–383 ms | 7.4–7.7 ms |
| 60 minutes, 2–8 speakers, 14,400 intervals | 1,376–1,425 ms | 14.8–15.2 ms |
| 120 minutes, 2–8 speakers, 28,800 intervals | 5,547–5,768 ms | 31.3–31.4 ms |

Every optimized interval output equaled its full-scan reference output. Packet-normalization equivalence tests cover 8–96 kHz, 1/2/6 channels, short inputs, and arbitrary packet boundaries. Indexed passage reconciliation also matches the full interval scan.

The browser fixture measured selection at 15.8 ms, distant navigation at 12.2 ms, playback highlighting at 11.0 ms, and filtering at 12.5 ms, including two animation frames. These measurements exclude IPC and storage.

The saved 10,000-passage backend benchmark measured loading at 49 ms, renaming at 145 ms, and one passage assignment at 186 ms after removing redundant work. Before that change, rename/assignment measured 252/299 ms. These measurements exclude rendering and IPC, so they do not establish the complete interaction's latency target. Retry cancellation acknowledgement in the native workflow probe measured 0.053 ms; the test also bounds initial cancellation under one second.

Preparation, ASR, reconciliation, speaker processing, and saving log separate timings. Real-engine total runtime and peak memory have not yet been measured against the baseline.

## Reproduce

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline
cargo check --manifest-path src-tauri/Cargo.toml --bins --offline
cargo test --manifest-path src-tauri/Cargo.toml --lib benchmark_interval_sweep --offline -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib benchmark_saved_review_edits --offline -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib format_playback_assets --offline -- --ignored --nocapture
```

The codec fixture requires `ffmpeg`; media and download-server tests require loopback sockets. The three pre-existing hardware/model smoke tests remain opt-in. Run the native workflow probe with the installed models directory:

```sh
BLABBER_SMOKE_MODELS="/path/to/Blabber/models" cargo run --manifest-path src-tauri/Cargo.toml --example review_workflow_smoke --offline
```

For browser measurements, run `npm run dev`, open `/review-benchmark.html`, and use its visible controls. Production builds do not include this page.

## Recording acceptance still required

The prior recordings `New Recording 29.m4a` and `New Recording 31.m4a` were absent from their referenced local paths. No RTTM or TextGrid annotations were found in the project. Supply local audio paths and speaker-timing annotations for the fixed evaluation corpus; do not substitute synthetic tones or the workflow probe for an accuracy claim.

On the same reference Mac, run the unchanged baseline and this implementation over 30-, 60-, and 120-minute recordings spanning 2–8 speakers, while respecting each model's existing duration limit. Record model/version, exact-count selection, audio fingerprint, stage times, total runtime, peak memory, and cancellation acknowledgement. Measure the complete UI edit through persistence and repaint against the 200 ms target.

Score speaker-count errors, diarization error, and passage attribution against the annotations with a fixed scoring protocol, including explicit overlap/collar rules. Pure algorithm optimizations must have unchanged outputs; engine-level accuracy must show no regression on this corpus. Complete audible packaged-webview playback, relinking, and fallback checks on the supported operating systems before treating cross-platform media acceptance as complete.
