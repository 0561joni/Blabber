# Running the ASR benchmark

`blabber-bench` runs every speech-to-text model installed on this Mac on the same recordings. For each model it reports speed (time from release to text, real-time factor, load time, memory) and accuracy (word error rate and more). The design is in [asr-benchmark-plan.md](asr-benchmark-plan.md). The texts to read aloud are in [asr-benchmark-scripts.md](asr-benchmark-scripts.md).

Every command goes through `npm run bench -- …`. The first call compiles a release build, which takes a few minutes; later calls start immediately.

## 1. Check the setup

```bash
npm run bench -- doctor
```

`doctor` reports:
- the machine, the power source and the thermal state;
- which models are runnable, and why any others are not (for example a missing helper or download);
- your vocabulary, the corpus and the next step to take.

Before a real run:
- **Quit Blabber.** It would compete for memory and the GPU, and it holds the Qwen lock.
- **Connect the power adapter.**

## 2. Record and import

1. Read the scripts in [asr-benchmark-scripts.md](asr-benchmark-scripts.md) and save each recording under its script id, for example `de-s05.m4a` or `de-s05@noisy.m4a`.
2. Import the folder:

   ```bash
   npm run bench -- corpus import ~/Desktop/blabber-recordings
   ```

   - **Scripted files** take their language, tags, key terms and reference from the script.
   - **Other files** become free-speech clips. Prefix their names with `de-`, `en-`, `fr-` or `mx-`, or pass `--lang`.
   - **Where things go:**
     - audio: `_private/asr-corpus/audio/` (16 kHz mono WAV)
     - your original files: `originals/`
     - the clip list: `manifest.json`
   - **Re-importing:** pass `--force` to replace a recording.

## 3. Draft and verify the references

```bash
npm run bench -- corpus draft
npm run bench -- corpus review
```

**`draft`** transcribes every unverified clip with two different engines: Whisper large-v3 and Qwen3-ASR by default, or pick others with `--models`.

**`review`** opens a local page with the audio, the script, both drafts and an editable reference. For each clip:
1. Listen to the recording.
2. Correct the reference to what was actually said.
3. Press ⌘↵ to save, mark the clip verified and move on.

The page highlights two kinds of differences:
- **Drafts vs reference:** words where the drafts differ from the reference.
- **Suspected misreads:** places where both engines agree with each other but not with the script. That usually means you read something differently from the script.

Only verified clips are scored. `corpus check` validates the corpus at any time.

## 4. Run

```bash
npm run bench -- run --quick              # ~5–15 minutes: sanity check of every model
npm run bench -- run --open               # the full benchmark, report opens when done
```

**Useful options:**
- `--models 'whisper-*,live-pair'` runs a subset of models.
- `--clips short`, `--lang de,en` and `--only de-s0*` run a subset of clips.
- `--mode raw,app` measures what your vocabulary changes. `app` is the default: your vocabulary prompt plus the app's correction, which is what you get in Blabber.
- `--fixed-language` tells each engine the clip language instead of letting it auto-detect.
- `--repeats 5` adds more warm runs for steadier latency numbers.
- `--resume <run dir>` continues a run interrupted by sleep, a crash or Ctrl+C. Add `--rerun whisper-large-v3,…` to measure some models again inside the same run, for example after something else competed for memory.
- `--synthetic` runs the macOS `say` fixtures in `src-tauri/target/r2t2-fixtures`. It needs no recordings.

### What happens in a run

**One process per model.** Each model runs in its own child process. That process:
1. loads the model twice, to give a first-load and a second-load time;
2. does an untimed warm-up decode;
3. transcribes every clip.

**Batch models (Whisper, Qwen, MOSS, VibeVoice)** are run with the app's real settings:
- Short clips use the shortcut-dictation request: greedy decoding, no timestamps.
- Long clips use the file-transcription request: beam search, VAD chunking, stitching.
- MOSS and VibeVoice run on long clips only, unless you pass `--force-all-clips`.

**Streaming models (R2T2 and the live pair)** get the audio at real-time pace on short clips, chunked exactly as in live dictation. This measures first-word and release-to-text latency. One extra unpaced run measures throughput.
- The live pair also produces `live-pair-stream`, the live preview text, scored as its own row.

**Memory** is the peak physical footprint of the child plus its helper processes. **Wired memory** (Neural Engine and GPU) is reported separately.

**Safety limits:**
- A crash or hang costs one clip; the next process continues after it.
- Models run in a shuffled order (the seed is recorded), with a cool-down between models.
- MOSS and VibeVoice always run last. VibeVoice peaks at about 14 GB, and on an 18 GB Mac that pushes the next models into swap and slows them down.

**Where results go:** `_private/asr-bench/runs/<timestamp>/`
- `report.html`: open it in a browser.
- `results.json`: all numbers and transcripts.
- `plan.json`, `events.jsonl`, `models.jsonl`: raw data. `npm run bench -- report <dir>` re-scores from these.
- `run.log`: engine output.

## 5. Read and share the results

The report contains:
- an accuracy-vs-speed chart with the Pareto frontier;
- a sortable leaderboard;
- breakdowns by language, tag and condition;
- a matrix of significant differences between models;
- latency distributions;
- a per-clip explorer with audio and word-level diffs.

**Sharing:** reports under `_private/` contain your transcripts. `--export-summary` (on `run` or `report`) writes `benchmark-results/asr-bench-<id>.json` and `.html` with no transcripts, references or audio links. That copy is safe to commit or share.

**Before/after comparison** (for example after updating whisper.cpp):

```bash
npm run bench -- compare _private/asr-bench/runs/<before> _private/asr-bench/runs/<after>
```

## How the numbers are defined

- **WER** = (substitutions + deletions + insertions) ÷ reference words, on normalised text. The normaliser lower-cases, removes punctuation and fillers, writes numbers as words, folds umlauts (German), and expands contractions and abbreviations. So "9:30" and "neun Uhr dreißig" are equal, and "Pull-Request" and "Pull Request" are equal. Its rules are versioned (`normalisationVersion`) and live in `src-tauri/src/asr_scoring.rs`.
- **Formatted error rate** keeps case and punctuation. It shows how much editing the pasted text would need.
- **Pooled WER** weights clips by length. **Macro WER** averages clips equally. The **95 % CI** comes from a bootstrap over clips. **p-values** in the pairwise matrix come from a paired bootstrap over the clips both models transcribed.
- **Release → text** is how long you wait after releasing the shortcut.
  - Batch models: the whole transcription, measured after loading and warm-up, as when the model is already loaded in the app.
  - Streaming models: the time from the last audio chunk to the final text, with real-time feeding.
  - Dictations over 31 s: Blabber's early ASR decodes full windows while you speak. The report adds a simulated figure (only the tail decoded at release).
- **RTF** = compute time ÷ audio duration. 0.05 means 20× faster than real time.
- **Term recall:** the share of each clip's key terms (from the scripts) that appear in the output.
- **Number accuracy:** the share of numbers written in the reference that come out right after normalisation.
- **Hallucinations** count three cases: text produced on a no-speech clip, a repetition loop, or five or more inserted words in a row.
