# Live-pair helper (`blabber-fluid-worker`)

This resident CoreML helper powers the experimental **live pair** for shortcut
dictation (see [`docs/live-pair-plan.md`](../../docs/live-pair-plan.md)):

- **Nemotron 3.5 ASR Streaming 0.6B** (latin bundle, 560 or 1120 ms chunks)
  produces the live overlay text.
- **Parakeet TDT 0.6B v3** (`Encoder_v2`) rereads the whole recording on release.
  Its text is what gets pasted.

Both models run on the Apple Neural Engine through
[FluidAudio](https://github.com/FluidInference/FluidAudio). FluidAudio is pinned
to an exact revision in `Package.swift` and `manifest.json`, with its prebuilt
NemoTextProcessing trait disabled. The helper is loaded once, after Blabber
starts, and stays loaded. A shortcut press only resets streaming state.

## Build and test

Apple Silicon, macOS 14+ and Xcode (Swift 6.2+) are required.

```sh
npm run build:fluid                     # → src-tauri/bundle/fluid/, signed
python3 -m unittest discover -s workers/fluid -p test_protocol.py
cd src-tauri && cargo test live_pair    # Rust side, against a scripted fake helper
```

The build script runs `swift build -c release --arch arm64` and copies the binary
to `src-tauri/bundle/fluid/`. It signs the binary with `APPLE_SIGNING_IDENTITY`,
the local "Blabber Local Signing" certificate, or an ad-hoc signature. It also
bundles the FluidAudio licence and `manifest.json`. CoreML compiles each model
on first load (about 30 s, once per macOS build) and caches the result in
`~/Library/Caches/blabber-fluid-worker/`.

## Models and fixtures

The app downloads the models from Settings → Models → Live pair. Every file is
pinned by revision, size and SHA-256 in `manifest.json` (about 1.85 GB, both
preview tiers included). Development tests, the ignored real-model test and the
benchmarks expect the same layout under `src-tauri/target/fluid-models/`:

```
src-tauri/target/fluid-models/
  nemotron/latin/560ms/…   nemotron/latin/1120ms/…
  parakeet/{Preprocessor,Encoder_v2,Decoder,JointDecisionv3}.mlmodelc, parakeet_vocab.json
```

Synthetic fixtures come from `python3 workers/r2t2/make_probe_audio.py
--individual`. Put human recordings in `_private/asr-corpus/` (git-ignored) as
`de-*.wav`, `en-*.wav` and `mixed-*.wav` (16 kHz mono PCM16), each with a `.txt`
reference.

```sh
cd src-tauri && cargo test real_worker_cold_and_warm -- --ignored --nocapture
cd workers/fluid/spike && swift build -c release   # phase 0 measurement CLI
python3 workers/fluid/evaluate.py --paced \
  --output benchmark-results/live-pair-vs-r2t2-synthetic.json
python3 workers/fluid/evaluate.py --paced --corpus _private/asr-corpus \
  --output benchmark-results/live-pair-vs-r2t2-human.json
```

`evaluate.py` runs the live pair and R2T2 on identical audio, with identical
real-time pacing and text normalisation. By default it writes aggregate numbers
only. `--include-text` is refused unless the report goes under `_private/`.

## Protocol v1

The transport is UTF-8 newline-delimited JSON on stdin/stdout, and stderr carries
error codes only. Every request has `version: 1`, `type`, a non-empty `sessionId`
(at most 128 bytes) and `sequence`. Every request gets exactly one response, with
the same `sessionId` and `sequence + 1`. Each response also carries `code`
(empty unless the response is an error), `rssBytes` (the physical footprint) and
`peakRssBytes`. Frames are limited to 1 MiB, and audio frames to 2 s.

Control requests each use their own session ID with sequence 0. They also drop
any unfinished dictation session.

| Request | Fields | Response |
|---|---|---|
| `load` | `nemotronPath`, `parakeetPath` (local directories; never downloads) | `ready{loadMs, chunkMs}` |
| `warmup` | — | `warm{warmupMs}` (1 s of silence through both models) |
| `ping` | — | `pong{loaded}` |
| `unload` | — | `unloaded` (process stays alive) |

A dictation session starts at sequence 0, and each following request increments
it by one.

| Request | Fields | Response |
|---|---|---|
| `start` | `language` (`auto`, `de`, `en`, …), `chunkMs` (must match the loaded tier) | `started{language}` |
| `audio` | `startSample` (must equal the acknowledged count), `samples` (finite, [-1, 1], 16 kHz mono) | `progress{committedText, tentativeText, ackSample, streamError}` |
| `finish` | `expectedSamples` | `final{streamText, finalText, language, streamError, finalError, ackSample, timings{audioMs, flushMs, finalMs}}` |
| `cancel`, `reset` | — | `canceled` (idempotent) |

**Preview text:** `committedText` only ever grows. `tentativeText` is the last,
possibly unfinished word and may change. Only `final` may rewrite the preview.

**Final pass failure:** if Nemotron fails mid-session, `streamError` is set and
the preview stops, but the final pass still runs. If Parakeet fails, `finalError`
is `FLUID_FINAL_FAILED` and `finalText` is empty. Blabber then pastes `streamText`
and records a warning in history.

**Errors** are `error{code}` responses:

- **Session errors** (the worker stays resident): `FLUID_MODEL_MISSING`,
  `FLUID_LOAD_FAILED`, `FLUID_NOT_LOADED`, `FLUID_STALE_SESSION`,
  `FLUID_SAMPLES_MISMATCH`, `FLUID_INVALID_AUDIO`, `FLUID_LIMIT`,
  `FLUID_CHUNK_MISMATCH`, `FLUID_LANGUAGE_UNSUPPORTED`.
- **Fatal error** (the worker exits nonzero): `FLUID_PROTOCOL`, for malformed,
  oversized or truncated frames, unknown versions or types, and invalid
  envelopes.

EOF between sessions exits 0.

**Parent responsibilities** (`src-tauri/src/live_pair.rs`): deadlines,
stale-session rejection, process-group kill and reaping, restart with backoff
(1 s, 5 s, 30 s, then an error), `ping` after wake, `unload` on critical memory
pressure, and lazy reload on the next press or 60 s after the pressure clears.
