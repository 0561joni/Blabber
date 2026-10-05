# ASR model benchmark: implementation plan

> **Status:** implemented. For how to run it, see [asr-benchmark.md](asr-benchmark.md). The code lives in `src-tauri/src/bin/blabber-bench/` and `src-tauri/src/asr_scoring.rs`. The data formats are in `src-tauri/src/bin/blabber-bench/results-schema.md`.

**Goal:** one command that runs every installed speech-to-text model on the same recordings on this Mac and reports two things per model: **how fast** it is and **how accurate** it is (word error rate). It runs on the user's own recordings in German, English and mixed speech, including short dictations, long recordings and hard conditions.

**Shape:** a headless Rust CLI, `blabber-bench`, built from the app's own crate. It calls the same engine code the app ships, with no Tauri window and no AppHandle. Every run writes `results.json` and a self-contained `report.html`.

**Non-goals:**
- No changes to app behaviour or defaults.
- No cloud references or LLM judge. Ground truth is human-verified text.
- No GUI for now. The CLI is structured so a GUI could wrap it later.

---

## Why a new tool

There are already five harnesses: `tools/model-bench`, `workers/r2t2/{evaluate,compare_baseline}.py`, `workers/fluid/evaluate.py` and the Swift spike. They fall short in four ways:

- **Normalisers disagree.** Each harness normalises text its own way, so their numbers cannot be compared. Only the fluid one handles numbers and umlauts, and it has known bugs: `20.000` becomes "zwanzig null", mixed clips are normalised as German, and contractions are not handled.
- **No engine coverage.** None of them covers all engines. `model-bench` only runs ggml Whisper. Nothing runs Qwen, MOSS or VibeVoice.
- **Synthetic or second-hand references.** References are either macOS `say` voices or `gpt-4o-transcribe` output. Neither is ground truth.
- **Weak statistics.** Results are pooled WER only. There is no S/D/I breakdown, no spread across clips and no significance test.

`blabber-bench` replaces them for model comparison. The old scripts stay in place for protocol and lifecycle probes.

---

## Models under test

| Bench id | Engine | Path in the app code | Mode |
|---|---|---|---|
| `whisper-small`, `whisper-medium`, `whisper-large-v3`, `whisper-turbo`, `whisper-turbo-q5` | whisper.cpp, Metal | `LocalTranscriptionEngine::transcribe_file` (`asr.rs:337`) | batch |
| `qwen3-asr` | vendored C runtime, CPU/Accelerate | same dispatcher → `QwenAsrEngine::transcribe` | batch, chunked |
| `moss` | Python worker + C++ CLI | same dispatcher → `native_asr::transcribe_with_native_worker` | batch, subprocess |
| `vibevoice` | Python MLX worker | same as MOSS | batch, subprocess |
| `r2t2` | native C++ helper, Metal | new headless entry in `r2t2.rs` (see phase 2) | streaming |
| `live-pair` (+ `live-pair-stream`) | Swift FluidAudio, ANE | new headless entry in `live_pair.rs` (see phase 2) | streaming preview + Parakeet final |

`--models` takes ids or globs (`whisper-*`). The default is every model that `doctor` reports as runnable.

**Engine settings mirror the app:**
- Clips of 60 s or less use the **dictation** request (`use_context` = shortcut dictation, greedy decoding).
- Longer clips use the **file-transcription** request (beam 5, VAD chunking, stitching).
- MOSS and VibeVoice only run on long clips by default, because they are file-only diarising models. `--force-all-clips` overrides this.
- R2T2 and the live pair only run on clips of 300 s or less (their hard cap).

---

## What gets measured

### Speed

| Metric | Applies to | How it is measured |
|---|---|---|
| **Cold load** | all | Fresh process, model from disk to ready. The live pair's first CoreML compile is reported separately (`coreml_compile_ms`) and excluded from cold load. |
| **Warm load** | all | Second load in the same process; context-cache hit for Whisper. |
| **Release → text** (headline dictation metric) | short clips | Time from "user released the shortcut" to final text. Batch models: wall time of the warm transcription of the whole clip. Streaming models: audio fed at real-time pace, then time from `finish` to `final`. |
| **First visible word** | streaming | Paced feed start → first non-empty preview. |
| **Real-time factor** (RTF; also shown as ×-real-time) | all, headline for long clips | warm wall time ÷ audio duration |
| **Peak memory** | all | Peak `phys_footprint` of the isolated child process, sampled at 50 ms. For the live pair also the system wired-memory delta, because ANE memory is wired. |
| **Energy** (optional, `--power`) | all | `powermetrics` CPU/GPU/ANE mJ per audio second. Needs sudo, off by default. |

Every warm timing is repeated: 3 runs for short clips and 1 for long clips by default (`--repeats`). The report shows median and p95.

**Note on early ASR:** for dictations longer than about 31 s, the app's `early_asr` transcribes windows while the user is still recording, so real release→text is lower than the batch wall time. The bench reports both numbers: `release_to_text_batch` and `release_to_text_early` (the early figure simulated with `early_asr`'s window plan).

### Accuracy

- **WER** with a substitution/deletion/insertion (S/D/I) breakdown, plus **CER**.
  - Per clip.
  - Per language, pooled (Σerrors/Σwords) and also macro-averaged, so one 5-minute clip cannot dominate.
  - 95 % bootstrap confidence intervals, resampling whole clips.
- **Two text views:**
  - **Normalised WER:** only the words count. This is the main ranking.
  - **Formatted error rate:** case and punctuation are kept. This matters because dictated text is pasted as-is.
- **Paired model comparison:** a paired bootstrap on per-clip error differences for each model pair. The report can then say "turbo-q5 is better than medium on German (p = 0.03)" instead of just listing two numbers.
- **Term recall:** each clip can list key terms in its sidecar (names, product names, jargon). Term recall is the share found verbatim. It is reported with and without the user's vocabulary (see "Pipeline modes").
- **Number and date accuracy:** the share of numeric entities (times, amounts, dates) correct after normalisation.
- **Hallucination flags:**
  - text produced on silence or noise clips;
  - insertion bursts (5 or more consecutive inserted words);
  - repeated n-grams, flagged with the app's `transcription_quality::repetition_reason`.
- **Language ID:** the detected language compared with the sidecar language, per model. Mixed clips are reported separately.

### Pipeline modes (`--mode raw,app`)

- **`raw`:** engine output only, no prompt.
- **`app`:** what the user actually gets.
  - The vocabulary prompt comes from the user's SQLite database (opened **read-only**, through `vocabulary::build_asr_prompt_from_db_path`).
  - Then `vocabulary::correct_transcript_result` is applied.
  - The difference between `raw` and `app` shows what custom vocabulary is worth for each model.

---

## Corpus

**Location:** `_private/asr-corpus/` (already git-ignored).

```
_private/asr-corpus/
  manifest.json                 # one entry per clip
  audio/<id>.wav                # 16 kHz mono PCM16, normalised by the bench on import
  originals/<id>.<ext>          # untouched source file
```

**Manifest entry:**

```json
{
  "id": "de-short-007",
  "language": "de",               // de | en | fr | mixed | none
  "script": "de-s03",             // script id, if read from asr-benchmark-scripts.json
  "condition": null,              // noisy | far | fast | quiet | other-speaker
  "category": "short",            // short (≤60 s) | long
  "tags": ["noisy", "jargon"],    // free: noisy, accented, quiet, far-mic, fast, jargon, silence…
  "speaker": "joni",
  "durationMs": 14320,
  "audioSha256": "…",
  "reference": "Kannst du bitte das Meeting mit Frau Özdemir auf Dienstag, neun Uhr dreißig, verschieben?",
  "referenceStatus": "verified",  // draft | verified — only verified clips are scored
  "referenceDraftedBy": ["whisper-large-v3", "qwen3-asr"],
  "terms": ["Özdemir"],
  "notes": ""
}
```

**Read-aloud scripts:** [asr-benchmark-scripts.md](asr-benchmark-scripts.md). The source of truth is [asr-benchmark-scripts.json](asr-benchmark-scripts.json), with ids, language, per-sentence language segments for mixed scripts, tags and key terms. The set contains:
- 15 short German and 15 short English dictations;
- 10 short mixed clips: sentence-level switches between de and en, Denglisch inside a sentence, and two with French;
- 6 short French dictations;
- 6 long recordings of about 3 minutes each (de ×2, en ×2, mixed de/en, fr);
- 3 no-speech clips;
- 13 condition re-recordings of existing scripts (noisy, far, fast, quiet, other speaker). These give paired clean-vs-hard comparisons on identical words.

That is about 31 minutes of audio in total. Unscripted free speech can be imported alongside the scripted clips.

### Reference workflow

1. `blabber-bench corpus import <files or folder> [--lang de --tags noisy]`
   - Decodes any format (m4a, mp3, wav…) with `audio_preprocess::decode_audio_file` and writes the 16 kHz WAV.
   - Hashes the audio, assigns an id and adds a `draft` manifest entry.
   - **Scripted recordings:** a file named after a script id (`de-s05.m4a`, `de-s05@noisy.m4a`) takes its language, tags, terms, segments and draft reference from the scripts JSON. The next step then only flags clips where the speaker deviated from the script.
   - **Misread flag:** if the two drafting engines agree with each other but both differ from the script at the same spot, the speaker most likely misread. The review page jumps there first.
2. `blabber-bench corpus draft`
   - Transcribes each clip with **two different engines**: Whisper large-v3 and Qwen3-ASR.
   - Merges them into a draft and marks the words where they disagree.
3. `blabber-bench corpus review`
   - Starts a local page at `http://127.0.0.1:<port>`: an audio player, an editable reference, disagreements highlighted, a keyboard shortcut for "verified".
   - Includes a terms field, plus a "play from cursor" control that seeks via the drafts' word timestamps.
   - Saves straight back into `manifest.json`.
4. `blabber-bench corpus check` validates the manifest: hashes, missing audio, unverified clips, duration and category mismatches.

**Bias guard.** A draft from model X makes the reference look like X's output. To limit this:
- Drafts come from two engines.
- Every clip must be listened to before it is marked verified.
- The report lists the drafting models and flags their rows.
- The report also shows WER of each model against the *other* model's raw draft as a sanity check.

**Transcription convention** (shown at the top of the review page):
- Write what was said, and write numbers as you would type them.
- Do not transcribe filler words ("äh", "um").
- The normaliser makes "9:30" and "neun Uhr dreißig" equivalent, so either spelling is fine.

---

## Unified text normaliser (`asr_scoring.rs`)

There is one implementation, in the library crate, and it is unit-tested. It is a port of `workers/fluid/evaluate.py::normalise` with the known bugs fixed:

- **Common steps:** NFC → casefold → expand `€ % m² km` → convert numbers → split on word characters → expand abbreviations → fold umlauts (ä→ae, ß→ss) → drop fillers.
- **Numbers:**
  - Thousands separators (`20.000`, `1,000`) and decimals (`3,5` → "drei komma fünf", `3.5` → "three point five").
  - Times, ordinals, years (EN 1100–2099) and amounts.
  - Converts up to 10⁹.
- **Mixed clips:** each digit run is converted using the language of the neighbouring words, instead of always German. For scripted clips, the language comes from the segment language in the scripts JSON.
- **French:**
  - numbers with a space as thousands separator (`2 500`, `600 000`) and decimal commas;
  - 70–99 number words (*soixante-dix*, *quatre-vingt-dix*);
  - "pour cent" = "%";
  - elisions split (`l'unité` → "l unité");
  - accents kept, because they distinguish words.
- **Letter-digit tokens:** `Q1`, `p95`, `A9` and `SOC 2` are split at the letter-digit boundary, so "Q eins" and "Q1" match.
- **Contractions:** English contractions are expanded ("don't" → "do not").
- **Hyphens and compounds:** German hyphenated compounds are joined ("Milch-Frischprodukte" = "Milchfrischprodukte"). An optional compound-split check scores a split compound as one error rather than two.
- **Filler list:** äh, ähm, öhm, um, uh, hm.
- **Versioning:** a `normalisation_version` string is written into every result, so later normaliser changes never mix with old numbers.

**Golden tests** cover each rule. A parity test runs the synthetic fixtures and checks that the new scorer reproduces `live-pair-vs-r2t2-synthetic.json` within 0.3 pp, apart from the fixed bugs, so the old results stay comparable.

---

## Architecture

```
blabber-bench run
 ├─ plan: models × clips × modes × repeats (order randomised per seed)
 ├─ for each model: spawn `blabber-bench run-one --model X --clips … --json` (child process)
 │    ├─ cold load → warmup (1 s silence) → warm repeats per clip
 │    ├─ batch:     LocalTranscriptionEngine::transcribe_file(request, progress)
 │    ├─ streaming: r2t2::headless / live_pair::headless (paced or unpaced feed)
 │    └─ NDJSON rows on stdout: timings + text + segments
 ├─ sampler thread: child phys_footprint, wired memory, thermal state, every 50 ms
 ├─ scoring: asr_scoring (normalise, align, WER/CER/S-D-I, terms, numbers, flags)
 └─ write runs/<timestamp>/{results.json, report.html, run.log}
```

**Design points:**

- **One child process per model.** This gives honest peak-memory figures and a clean cold load. A crash, out-of-memory kill or hang (per-clip deadline) loses one model and not the whole run. The child is the same binary with a hidden `run-one` command.
- **Uses the app's code.**
  - Batch engines go through the public `LocalTranscriptionEngine`. `discover_installed_models` re-hashes Qwen (4.7 GB), so the bench does this once in `doctor` and passes `--trust-installed` to children. That skips the re-hash and keeps the size check.
  - Streaming engines need a small headless refactor (phase 2), so the bench and the app share the same protocol client.
- **Resumable.** Results are cached per (model weights sha, clip sha, mode, settings, code commit) in `runs/cache/`. A long run interrupted by sleep or a crash continues where it stopped. VibeVoice on long clips can take most of an hour.
- **Fair conditions.**
  - `doctor` refuses to start on battery power or in Low Power Mode unless `--allow-battery` is passed. It warns if `pmset -g therm` reports throttling.
  - There is a cool-down of 20 s between models, or until the thermal state is nominal.
  - Model order is randomised (seed recorded).
  - Other Blabber instances must be closed: the bench checks `desktop-instance.lock`.
- **Privacy.**
  - Full transcripts and per-clip diffs only go to `_private/asr-bench/runs/…`.
  - `--export-summary` writes an aggregate-only JSON (no text) into `benchmark-results/` that is safe to commit. This matches the existing `evaluate.py --include-text` policy.

### Where the code goes

| File | Content |
|---|---|
| `src-tauri/src/bin/blabber-bench/main.rs` (+ `cli.rs`, `runner.rs`, `corpus.rs`, `review_server.rs`, `report.rs`) | the CLI |
| `src-tauri/src/asr_scoring.rs` | normaliser, alignment, metrics, bootstrap; in the lib so `cargo test` covers it |
| `src-tauri/src/r2t2.rs` | `pub fn headless_transcribe(helper, model, samples, language, context, pacing) -> HeadlessResult` built from the existing `NativeWorker` (essentially the `real_native_adapter…` test promoted) |
| `src-tauri/src/live_pair.rs` | `pub fn headless_transcribe(helper, nemotron, parakeet, samples, language, chunk_ms, pacing) -> HeadlessResult { stream_text, final_text, first_word_ms, finish_to_final_ms, rss }` |
| `src-tauri/src/bin/blabber-bench/report.html` (template, `include_str!`) | the report page |
| `src-tauri/Cargo.toml` | `[features] bench = ["dep:tiny_http"]`; `[[bin]] name = "blabber-bench" required-features = ["bench"]`; `default-run = "speech-to-text"`, so `tauri build` and `tauri dev` still pick the app binary and never compile the bench |
| `package.json` | `"bench": "cargo run --release --manifest-path src-tauri/Cargo.toml --features bench --bin blabber-bench --"` |

**Gotchas already found:**
- **Worker lookup.** `native_asr::resolve_worker` only falls back to `bundle/…` paths in debug builds. The bench sets `BLABBER_MOSS_WORKER` / `BLABBER_VIBEVOICE_WORKER` itself (resolved by `doctor`).
- **`current_exe()`.** The persistent dictation worker re-launches `current_exe()`. The bench never uses that path; it calls the engine in-process inside its own child.
- **Qwen file lock.** `<models>/.qwen3-asr.lock` means Qwen cannot run while the app is transcribing with Qwen. This is another reason the bench checks that the app is closed.
- **Live-pair model path.** `doctor` looks first in `<models>/live-pair/` and falls back to `src-tauri/target/fluid-models/`.

---

## CLI

```bash
npm run bench -- doctor                       # machine, power, thermal, models found, workers found, warnings
npm run bench -- corpus import ~/Recordings/*.m4a --lang de --tags noisy
npm run bench -- corpus draft
npm run bench -- corpus review                # opens the review page
npm run bench -- corpus check
npm run bench -- run                          # everything runnable, default repeats
npm run bench -- run --models 'whisper-*,live-pair' --clips short --lang de --mode raw,app --repeats 5
npm run bench -- run --quick                  # 1 repeat, first 3 clips per language: about a 5-minute smoke run
npm run bench -- run --synthetic              # uses src-tauri/target/r2t2-fixtures (no corpus needed)
npm run bench -- report _private/asr-bench/runs/2026-10-05T10-00   # re-render HTML
npm run bench -- compare <runA> <runB>        # before/after, e.g. a whisper.cpp upgrade
```

---

## Report (`report.html`, single offline file)

1. **Header:** machine (M3 Pro, 18 GB), macOS, git commit (and whether the tree had uncommitted changes), power and thermal state, model weight hashes, normaliser version, seed, corpus summary.
2. **Accuracy vs speed chart:** one point per model. X axis is median release→text for short clips, or RTF for long clips (toggle). Y axis is WER. CI whiskers, Pareto frontier highlighted. This is the chart that answers "which model should be the default".
3. **Leaderboard table:** WER (normalised and formatted), CER, S/D/I, term recall, number accuracy, hallucination flags, language-ID accuracy, cold load, warm release→text p50/p95, RTF, peak memory. Sortable and filterable by language, category and tag.
4. **Per-language and per-tag breakdown:** grouped bars with CIs (de / en / mixed × clean / noisy / accented / jargon).
5. **Pairwise significance matrix:** which model beats which, per language.
6. **Latency distributions:** box plots per model. Streaming models also get first-word time and finish→final time.
7. **Per-clip explorer:** an audio player plus the reference and each model's output, with a word-level coloured diff (substitution/deletion/insertion).
8. **`raw` vs `app` delta:** what vocabulary correction gains or loses per model.

---

## Phases

| # | Work | Done when |
|---|---|---|
| 0 | Cargo feature, `[[bin]]`, `default-run`, CLI skeleton, `doctor` | `npm run bench -- doctor` lists all 10 installed engines; `npm run tauri build` is unaffected |
| 1 | `asr_scoring.rs`: normaliser, Levenshtein alignment with S/D/I, CER, term and number metrics, bootstrap | golden tests pass; the synthetic parity test passes |
| 2 | Headless entry points for R2T2 and the live pair; batch adapter around `LocalTranscriptionEngine` | `run --synthetic --quick` produces rows for every engine |
| 3 | Runner: child process per model, sampler, repeats, deadlines, resume cache, power and thermal guards | a full synthetic run survives a killed child and a resumed run |
| 4 | Corpus: import, draft (two engines and disagreement marks), review server, check | 3 test recordings go from import to verified |
| 5 | Report HTML and summary export | report renders offline; summary JSON contains no transcript text |
| 6 | **Human run:** the user records the corpus, verifies references, runs everything | a filled leaderboard. This also gives the human numbers that the live-pair and R2T2 acceptance gates in `docs/live-pair-plan.md` and `docs/r2t2.md` are still waiting for |

**Testing:**
- `cargo test asr_scoring` covers the normaliser and metrics.
- A runner test uses a fake engine to check crash, timeout and resume behaviour.
- The headless entry points are covered by the existing `#[ignore]` real-model tests, re-pointed to the new functions.

**Rough runtime for a full human run** (about 50 short + 6 long clips): roughly 1.5–3 hours, dominated by Whisper large-v3, MOSS and VibeVoice on long clips. `--quick` takes about 5 minutes. Exact figures come from the first synthetic run.

---

## Possible later additions

- **Real microphone path:** play clips through the speaker into the mic, through the app's real `audio_capture` path, to include capture and resampling effects. Off by default because it is slow and room-dependent.
- **Accuracy over time:** chart `compare` across commits, so engine upgrades show regressions.
- **GUI wrapper:** a minimal Tauri window that calls `blabber-bench run --json` and streams progress, reusing the report HTML.
- **Memory pressure:** measure release→text with TranslateGemma loaded at the same time (an open live-pair acceptance item).
