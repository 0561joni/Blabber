# Live pair dictation: implementation plan

**Goal:** a press of the shortcut shows live text immediately, and the pasted text appears less than a second after release. Both models stay loaded for as long as the Mac is running.

**Engine:** "Live pair". It uses two models:

- **Nemotron 3.5 ASR Streaming Multilingual 0.6B** produces the live overlay text while you speak.
- **Parakeet TDT 0.6B v3** runs a final pass over the whole recording when you release the shortcut. Its output is what gets pasted.

Both run in CoreML on the Apple Neural Engine through **FluidAudio** (Swift, Apache-2.0). Scope is the shortcut dictation only. File transcription, translation and the other engines stay as they are. R2T2 remains selectable until the live pair passes the acceptance tests.

---

## Architecture

```
Blabber (Rust/Tauri)                              blabber-fluid-worker (Swift, resident)
────────────────────                              ──────────────────────────────────────
app start ── spawn + load + warmup ─────────────▶ load Nemotron + Parakeet (local paths only)
shortcut ↓  ── start{session, lang} ────────────▶ reset streaming state (no load)
capture  ── audio{seq, 16 kHz mono f32} ────────▶ Nemotron chunk → progress{text}
           ◀── progress (≤10/s) → overlay
shortcut ↑ ── finish{expectedSamples} ──────────▶ flush Nemotron; Parakeet on full buffer
           ◀── final{streamText, finalText, timings}
vocabulary → (translation) → history → paste
```

- **One long-lived helper process.** It is spawned once, after the splash screen. It is not tied to the 60 s idle cache in `ProcessingQueue`, so it is never evicted after each dictation.
- **Same pattern as R2T2.** It reuses the R2T2 framing: newline-delimited JSON (one message per line) over stdin/stdout, with session and sequence IDs, deadlines and process-group kill. It extends protocol v1 with `load`, `warmup`, `ping` and `unload`.
- **Final text comes from Parakeet.** The overlay text is a preview. Rewriting the preview prefix is allowed only in `final`, never in `progress`.
- **No silent fallback.** If the helper or a model is missing, the user gets a setup error, as with R2T2. If Parakeet fails at finish, Blabber pastes Nemotron's stream text and adds a warning to history.

---

## Phase 0: Spike and go/no-go (do first, about 1 day)

Write a throwaway SwiftPM CLI in `workers/fluid/spike/`, pinned to a FluidAudio tag and commit. Run it on the M3 Pro (18 GB, macOS 27). Measure:

1. Cold CoreML compile time on the first load, warm load time when the compile cache already exists, and where the compile cache is stored.
2. Steady RSS: Nemotron alone, Parakeet alone, and both together. **The gate is 1.8 GB or less for both together.**
3. Nemotron streaming at 560 / 1120 / 2240 ms chunks: real-time factor (RTF), first-text time, and German + English with auto language mode. Check whether the CoreML repo ships an `auto` bundle or only per-language `<lang>/<tier>ms` folders. If it only has per-language folders, decide between loading de + en (RAM) and an explicit language setting.
4. Parakeet v3 final pass on 10 s, 30 s, 2 min and 5 min clips. **The gate is p95 of 0.8 s or less for 30 s of audio.**
5. Loading works fully offline from a local directory, with no Hugging Face download at runtime.
6. Behaviour after sleep and wake, and after a `critical` memory-pressure event (models still usable? ANE re-init time?).
7. Whether FluidAudio's custom-vocabulary or context-biasing works with Parakeet v3, and with German.

**Output:** add `benchmark-results/fluid-spike-m3-pro.json` and a short section in this document. **Stop or adjust** if RAM is above 1.8 GB, finalisation p95 is above 1.5 s, or Nemotron cannot keep up at 1120 ms.

### Phase 0 result (2026-10-04, M3 Pro 18 GB, macOS 27.0.1): go

Measured with `workers/fluid/spike` against FluidAudio v0.17.5 (`0b1f462…`, NemoTextProcessing trait off) on the synthetic fixtures. There are no human recordings yet. Full numbers are in `benchmark-results/fluid-spike-m3-pro.json`.

| Question | Result |
|---|---|
| 1. Load | The cold CoreML compile is 16 s for Parakeet and 12–15 s for Nemotron, once per executable name and macOS build. A warm load is 0.1 s for Parakeet and 0.12–0.2 s for Nemotron. The cache is `~/Library/Caches/<executable>/com.apple.e5rt.e5bundlecache/<OS build>/`, which is writable for the ad-hoc signed helper. |
| 2. RAM | Both models together (Encoder_v2 + Nemotron 560 ms) use **1.24 GB** of wired ANE memory (owned by `aned`) plus 0.13 GB resident in the worker, about **1.37 GB** in all. **This passes the 1.8 GB gate.** `aned` keeps programs cached after an unload until it needs the memory. |
| 3. Nemotron | The CoreML repo ships **no per-language folders**. A single `latin/<tier>ms` bundle serves de, en, es, fr, it and pt, with `auto` via the prompt ID, so it is the one used. RTF is 0.03 at 560 ms, 0.019 at 1120 ms and 0.014 at 2240 ms, so every tier keeps up over 5 minutes. First text arrives at a median of 1.14 s for 560 and 1120 ms (p95 1.14 s and 2.26 s), but 2.27 s for 2240 ms. The model's ~1 s right-context lookahead sets the floor. Preview WER (fixed language) is de 13.4% / en 1.6% at 560 ms and de 9.5% / en 1.6% at 1120 ms. Auto mode costs about 1 point on German. |
| 4. Parakeet v3 | **p95 for 30 s of audio is 0.17 s** (gate 0.8 s). 10 s takes 0.1 s, 2 min 0.37–0.52 s and 5 min 0.94 s. `Encoder_v2` (int8 per-channel, which fixes FluidAudio issue #760) matches the speed and has lower English WER, so it is used. |
| 5. Offline | All runs were done under `sandbox-exec` with the network denied. |
| 6. Memory pressure / sleep | An unload followed by a reload takes 0.2 s, and the models are usable afterwards. A real `critical` event needs root to simulate, and sleep/wake needs an interactive session, so both are covered by `ping` after wake and the supervisor instead. |
| 7. Biasing | Nemotron's `setCustomVocabulary` works with German, but it only affects the preview. FluidAudio's Parakeet biasing needs an extra CTC model. **Not used:** Blabber's post-hoc vocabulary step stays. |

**Adjustments:** the chunk size defaults to **560 ms**. It meets the first-word p95 target, and its weaker German preview is replaced by Parakeet before paste. 1120 ms remains a setting, so the download contains both tiers (only one is loaded). The language setting reuses Blabber's existing Automatic / fixed language choice.

## Phase 1: Quick win for R2T2 (independent, small)

- In `r2t2.rs`, make the idle-cache duration configurable: 60 s (current), 15 min, or "until memory pressure". Add the setting under Settings → Models.
- Keep evicting on memory pressure, translation load and model changes.
- Acceptance: a second dictation within the window starts without a load. The existing r2t2 tests pass.

## Phase 2: Swift worker `workers/fluid/`

| File | Content |
|---|---|
| `Package.swift` | Swift 6 package; macOS 14 minimum; FluidAudio pinned to an exact revision |
| `Sources/BlabberFluidWorker/main.swift` | stdin reader thread, stdout writer, message loop |
| `Protocol.swift` | Codable messages; v1 framing plus the new `load`, `warmup`, `ping` and `unload`; 1 MiB message cap |
| `Engine.swift` | `StreamingNemotronMultilingualAsrManager` and the Parakeet batch manager; sample-cursor ledger; full-buffer retention (5 min cap) |
| `manifest.json` | FluidAudio revision; for each model: repo, revision, file list, sizes, SHA-256; licence URLs |
| `test_protocol.py` | Protocol tests in the same style as `workers/r2t2/test_protocol.py` |
| `README.md` | Build, fixtures, benchmark commands |

Behaviour:

- **`load`:** takes the model directories. It never downloads, compiles Nemotron and Parakeet, and replies `ready{loadMs, rssBytes}`.
- **`warmup`:** runs 1 s of silence through both models, so the first real dictation pays no warm-up cost.
- **`start{session, language: auto|de|en, chunkMs}`:** resets the streaming state. The models stay loaded.
- **`audio{seq, samples}`:** rejects gaps and duplicates, feeds whole chunks to Nemotron, and emits `progress{committedText, tentativeText, ackSample}`.
- **`finish{expectedSamples}`:** checks that the sample count matches, flushes Nemotron, runs Parakeet on the full buffer, and replies `final{streamText, finalText, language, timings}`.
- **`cancel` / `reset`:** drop the session immediately.
- **`ping`:** health check after wake.
- **`unload`:** frees the models but keeps the process alive.
- **Errors:** stable error codes, e.g. `FLUID_MODEL_MISSING`, `FLUID_LOAD_FAILED`, `FLUID_SAMPLES_MISMATCH`.

## Phase 3: Build and bundle

- Add `scripts/build-fluid-worker.mjs`, mirroring `build-r2t2-worker.mjs`. It runs `swift build -c release --arch arm64`, copies the binary to `src-tauri/bundle/fluid/`, and ad-hoc signs it (no paid developer account).
- Add `"build:fluid"` to `package.json`, and hook it into `prepare-tauri-build.mjs` for Apple Silicon only.
- `.gitignore`: add `src-tauri/bundle/fluid/*` with a `!.gitkeep` exception.
- `tauri.conf.json`: add the bundle resource.
- `THIRD_PARTY_NOTICES.md`: add FluidAudio (Apache-2.0), Parakeet v3 (CC-BY-4.0) and Nemotron 3.5 ASR (OpenMDW-1.1).

## Phase 4: Model download

- In `model_downloads.rs`, add two `DownloadableModelSpec` directory specs with pinned revisions and SHA-256 per file:
  - `nemotron-3.5-streaming-multilingual-coreml`, including only the bundles chosen in phase 0
  - `parakeet-tdt-0.6b-v3-coreml`
- Use the existing resumable, verified downloader and completion manifest.
- Verify the SHA-256 once, after download. Warm reuse checks only size and modification time, because a full hash on every launch would add delay.
- Settings → Models gets one entry, "Live pair (Nemotron + Parakeet)", which downloads both models and shows the total size and expected RAM.

## Phase 5: Rust integration

- **New module `src-tauri/src/live_pair.rs`.** Copy the hardened parts of `r2t2.rs`: the `Protocol` and `IoEvent` threads, deadlines, process-group kill and stale-session rejection.
  - `ResidentWorker`: the process is owned by `AppState`, not by `ProcessingQueue`.
  - `start_resident(app)`: called after the splash screen when the live pair is selected and installed. It runs spawn → `load` → `warmup` in the background, and the status shows in the app (Preparing / Ready / Error).
  - `LiveSession` API, the same shape as R2T2's: `activate`, `push audio`, `finish`, `cancel`.
  - Supervisor: if the process crashes, restart it with backoff (1 s, 5 s, 30 s, then give up and show an error). Send `ping` after a system wake.
- **Memory policy:**
  - Memory-pressure `warning` (the existing `MemoryPressureWatch`): keep the models loaded.
  - Memory-pressure `critical`: send `unload`, then reload lazily on the next shortcut press, or 60 s after the pressure clears.
  - Translation (TranslateGemma, 9.66 GB) coexists with the pair and does **not** evict it. Measure combined memory in phase 7, and evict only if the measurement shows swapping.
- **`dictation.rs`:** add a `live-pair` branch next to `crate::r2t2::MODEL_ID` (lines ~430, 543, 731, 772, 1087–1146).
  - Press: `start` (no queue admission needed, because the models are resident).
  - Release: `finish`, then the existing pipeline: snapshotted vocabulary → optional translation → history → single paste.
  - The retry and recovery path reuses the same session handling with the stored recording.
  - The 5-minute cap stays.
- **Settings:** add `shortcut_model = live-pair`; language Auto / German / English; chunk size, defaulting to the phase 0 result; "Keep models loaded" (default on).
- **Overlay (`OverlayApp.tsx`):** no structural change. Add a "Finalising…" state between stop and the final text. If `finalText` differs from the preview, replace the preview with it.
- **Launch at login:** already exists in `autostart.rs`. When the live pair is selected, Settings should suggest enabling it, so the models are loaded from login onwards.

## Phase 6: Tests

- **Rust unit tests:**
  - protocol ordering and stale-session rejection
  - crash → restart
  - memory-pressure unload → lazy reload
  - finish falls back to stream text when Parakeet errors
  - cancel immediately before paste
  - setting changes reload the worker
- **Ignored real-model test:** `live_pair::tests::real_worker_cold_and_warm`, which needs the models and the ANE.
- **Python `test_protocol.py`:** the helper's message ordering and error codes.
- **Frontend:** the overlay "Finalising" state, and the Settings entry with RAM display.

## Phase 7: Benchmark against R2T2

- Extend the evaluation harness (`workers/r2t2/evaluate.py` style) so it can run `fluid` and `r2t2` on identical fixtures with identical text normalisation (numbers, abbreviations, umlauts).
- **Corpus:** the existing synthetic fixtures, plus **human recordings**: at least 20 clips each for German, English and mixed, and a 5-minute run of each.
  - Store recordings and reference transcripts in `_private/asr-corpus/`, which is git-ignored, so personal vocabulary never reaches the public repo.
  - Only aggregate numbers go into `benchmark-results/`.
- **Measures:**
  - WER per language, for the stream text and the final text
  - time from shortcut to first visible word
  - median and p95 delay until a spoken word appears
  - p95 time from release to paste
  - peak and steady RSS
  - CPU, GPU and ANE use while idle
  - memory while TranslateGemma is also loaded

## Acceptance (switch the default shortcut model when all pass)

| Metric | Target |
|---|---|
| Shortcut to listening (models resident) | ≤ 150 ms |
| First visible word | median ≤ 1 s, p95 ≤ 2 s |
| Release to paste (Original mode) | p95 ≤ 1 s for dictations up to 60 s |
| Final WER, German and English (human recordings) | at most 2 points worse than R2T2 for each language |
| Resident RAM, both models | ≤ 1.8 GB (10% of 18 GB) |
| Idle cost | ~0% CPU; no measurable battery drain over 1 h idle |
| Robustness | sleep/wake, crash restart, critical-memory unload and reload, cancel during finalisation: no stale paste |

## Order and effort (rough)

| Phase | Effort | Depends on |
|---|---|---|
| 0 Spike | 1 day | none |
| 1 R2T2 cache quick win | 0.5 day | none |
| 2 Swift worker | 2–3 days | 0 |
| 3 Build and bundle | 0.5 day | 2 |
| 4 Model download | 0.5–1 day | 0 |
| 5 Rust integration | 2–3 days | 2, 4 |
| 6 Tests | 1–2 days | 5 |
| 7 Benchmark | 1 day, plus recording time | 5 |

## Risks

- **Nemotron CoreML bundles may be per-language only.** Auto mode would then need two loaded models or a German/English setting. Phase 0 decides.
- **German live preview is noticeably weaker** (about 8% WER). This is acceptable because Parakeet replaces the text before pasting. If it is distracting, show only committed words.
- **No end-of-utterance detection in Nemotron.** This doesn't matter here, because the shortcut defines start and end.
- **Vocabulary biasing** is weaker than prompting R2T2 or Whisper. Blabber's existing post-hoc vocabulary step stays. Use FluidAudio biasing if phase 0 confirms it works.
- **The ad-hoc signed helper** may need its CoreML compile cache to stay in a writable app-support directory. Phase 0 checks the location.

---

## Implementation status (2026-10-04)

Phases 0–6 are implemented. Phase 7 has its harness and a synthetic-corpus run.
**The default shortcut model is unchanged.** The live pair is an opt-in model
("Live pair · Experimental") until the human-recording acceptance below passes.

| Phase | Where | Notes |
|---|---|---|
| 0 Spike | `workers/fluid/spike/`, `benchmark-results/fluid-spike-m3-pro.json` | Go (see the result above). |
| 1 R2T2 cache | `review_jobs.rs` (`cache_idle_for`), setting `r2t2_idle_cache` | 1 min / 15 min / until memory pressure. Pressure, translation and model changes still evict. |
| 2 Swift worker | `workers/fluid/` | Protocol in `README.md`. Session errors keep the worker resident; only framing errors are fatal. |
| 3 Build | `scripts/build-fluid-worker.mjs`, `npm run build:fluid`, `tauri.macos.conf.json`, notices | Signed with the local identity (ad-hoc fallback). |
| 4 Download | `model_downloads.rs` (`live-pair`) | **One** spec holds both models (both Nemotron tiers + Parakeet, 1.85 GB). This gives one Settings entry and one resumable download. SHA-256 is checked once; warm checks use size. |
| 5 Integration | `live_pair.rs`, `dictation.rs`, `app_state.rs` | The resident helper is owned by `AppState`, with a supervisor (backoff 1/5/30 s, then error), ping after wake (wall-clock vs monotonic drift), and critical-pressure unload with lazy reload. Translation acquires queue admission but never evicts the pair. Overlay states are "Loading models" and "Finalising…". |
| 6 Tests | `live_pair/tests.rs` (13 + 1 ignored real-model test), `test_protocol.py` (9), Settings/overlay vitest | All pass. |
| 7 Benchmark | `workers/fluid/evaluate.py`, `benchmark-results/live-pair-vs-r2t2-synthetic.json` | Synthetic only; human recordings outstanding. |

### Synthetic benchmark (paced in real time, auto language, 560 ms)

| Metric | Live pair | R2T2 | Target |
|---|---|---|---|
| Final WER de / en (20 clips + 5 min each) | 1.15 % / 0.67 % | 1.15 % / 0.19 % | ≤ R2T2 + 2 pts ✅ (synthetic) |
| Preview WER de / en | 8.4 % / 0.95 % | (same as final) | — |
| First visible word, median / p95 | 1.17 s / 1.18 s | 0.84 s / 0.86 s | ≤ 1 s / ≤ 2 s: **median misses** |
| Estimated word delay, median / p95 | ~0 s / 2.6 s | 2.5 s / 7.8 s | — |
| Release → text, p95 (≤ 60 s) | 0.11 s | 0.31 s | ≤ 1 s ✅ (paste excluded) |
| Memory | 1.26 GB wired + 0.11 GB process | 4.96 GB process peak | ≤ 1.8 GB ✅ |
| Helper idle CPU (60 s) | 0.0 % | — | ~0 % ✅ |
| **Mixed de/en, 5 min: final / preview WER** | **18.7 % / 10.0 %** | 5.8 % | — |

### Open issues before switching the default

1. **Code-switching in the final pass.** Within one dictation that switches
   language, Parakeet v3 (with no language hint) sometimes anglicises German
   ("Die/Der" → "The"). In one English→German pair it **dropped a whole English
   sentence**. A short two-sentence mixed probe gave 13.8 % final WER, while the
   Nemotron preview kept both languages. Possible mitigations include a guard
   that pastes the preview (with a warning) when the final text is much shorter
   than the preview, or per-sentence language hints. Decide after the human
   mixed recordings.
2. **First visible word.** The median is 1.17 s, above the 1 s target. The ~1 s
   right-context lookahead of the Nemotron bundles sets this floor; it is not a
   chunk-size effect.
3. **Not yet measured:**
   - human recordings (`_private/asr-corpus/`), and press-to-listening in the
     running app;
   - real sleep/wake and critical-pressure events, and combined memory with
     TranslateGemma;
   - ANE/GPU idle power (`sudo powermetrics`).
