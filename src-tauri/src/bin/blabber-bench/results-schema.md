# blabber-bench data formats

This file describes the data `blabber-bench` hands to its two HTML pages. Field names are camelCase. Times are in milliseconds, memory in bytes, and rates are fractions (0.123 means 12.3 %). `null` means not measured or not applicable.

---

## `results.json` (embedded in `report.html`)

The report template contains the literal placeholder `/*__BENCH_DATA__*/null`. The CLI replaces it with this JSON object.

```ts
type Results = {
  schemaVersion: 1;
  tool: "blabber-bench";
  textIncluded: boolean;          // false in the shareable summary: no text, alignment, audioHref, reference or terms
  run: {
    id: string;                   // "2026-10-05T10-00-00"
    startedAt: string;            // ISO 8601
    finishedAt: string | null;
    machine: { cpu: string; memoryBytes: number; cores: number; os: string };
    git: { commit: string | null; dirty: boolean | null };
    power: { source: string | null; lowPowerMode: boolean | null };
    thermalAtStart: string | null;   // "nominal" | "throttled: …" | null
    seed: number;
    normalisationVersion: string;
    settings: {
      repeats: number;            // warm runs per short clip, batch models
      longRepeats: number;        // warm runs per long clip
      streamRepeats: number;      // paced runs per short clip, streaming models
      modes: ("raw" | "app")[];
      languageMode: "auto" | "fixed";
      synthetic: boolean;
      quick: boolean;
      cooldownSeconds: number;
    };
    vocabularyTermCount: number;  // user vocabulary terms used in "app" mode
  };
  corpus: { path: string; clipCount: number };
  clips: Clip[];
  models: Model[];
  results: Row[];
  aggregates: Aggregates;
};

type Lang = "de" | "en" | "fr";

type Clip = {
  id: string;                     // "de-s05", "de-s05@noisy", "de-free-003"
  language: Lang | "mixed" | "none";
  languages: Lang[];              // [] for "none"
  category: "short" | "long";
  tags: string[];
  condition: string | null;       // "noisy" | "far" | "fast" | "quiet" | "other-speaker" | null
  script: string | null;          // script id if read from docs/asr-benchmark-scripts.json
  durationMs: number;
  referenceWords: number;         // normalised token count
  referenceDraftedBy: string[];   // ["script"] or the engines whose draft seeded the reference (bias check)
  reference?: string;             // omitted when textIncluded = false
  segments?: { lang: Lang; text: string }[];   // mixed scripts only
  terms?: string[];
  audioHref?: string;             // path to the 16 kHz WAV, relative to report.html
};

type Model = {
  id: string;                     // bench id, e.g. "whisper-turbo-q5", "live-pair", "live-pair-stream"
  appModelId: string;             // id inside Blabber, e.g. "ggml-large-v3-turbo-q5_0-bin"
  name: string;                   // display name
  engine: string;                 // "whisper.cpp" | "qwen3_asr_c" | "moss-transcribe-cpp" | "vibevoice-mlx" | "audio.cpp-r2t2" | "fluidaudio-live-pair"
  kind: "batch" | "streaming";
  derivedFrom: string | null;     // "live-pair-stream" is derived from "live-pair" (preview text of the same runs)
  sizeBytes: number | null;
  load: {
    coldMs: number | null;        // fresh process → model ready
    warmMs: number | null;        // second load in the same process
    coremlCompileMs: number | null;
    perTranscription: boolean;    // MOSS/VibeVoice start a worker per transcription (load is inside every timing)
  };
  peakMemoryBytes: number | null;     // peak phys_footprint of the bench child + its helper processes
  wiredDeltaBytes: number | null;     // peak system wired-memory increase (ANE models)
  errors: { clip: string | null; message: string }[];
  skippedClips: { clip: string; reason: string }[];
};

type Row = {                      // one per model × clip × mode
  model: string;
  clip: string;
  mode: "raw" | "app";
  text?: string;                  // omitted when textIncluded = false
  detectedLanguage: string | null;
  languageCorrect: boolean | null;
  timings: {
    runsMs: number[];             // wall time of each warm run (batch); release→final of each paced run (streaming)
    releaseToTextMs: number | null;   // median of runsMs for short clips; batch: whole transcription, streaming: release → final text
    releaseToTextEarlyMs: number | null; // batch, clips > 31 s: simulated with early ASR windows
    computeMs: number | null;     // median compute wall time (unpaced) → rtf
    rtf: number | null;           // computeMs / durationMs
    firstWordMs: number | null;   // streaming: paced feed start → first non-empty preview
  };
  scores: {
    normalised: Counts;
    formatted: Counts;
    characters: Counts;           // CER on the normalised text
    termsFound: number;
    termsTotal: number;
    numbersCorrect: number;
    numbersTotal: number;
    insertionBurst: number;       // longest run of consecutive inserted words
    repetitionLoop: boolean;
    textOnSilence: boolean;       // reference empty but text produced
  } | null;                       // null when the run failed
  alignment?: [op: "=" | "S" | "D" | "I", ref: string | null, hyp: string | null][];
  error: string | null;
};

// alignment tokens are normalised words; percentiles use linear interpolation.
type Counts = { referenceWords: number; substitutions: number; deletions: number; insertions: number; rate: number | null };

type Group = {                    // aggregate over a set of rows of one model and mode
  clips: number;
  referenceWords: number;
  substitutions: number; deletions: number; insertions: number;
  werPooled: number | null;       // Σerrors / Σreference words
  werMacro: number | null;        // mean of per-clip WER
  ci95: [number, number] | null;  // bootstrap over clips, pooled WER
  formattedPooled: number | null;
  cerPooled: number | null;
  termRecall: number | null;
  numberAccuracy: number | null;
  hallucinations: number;         // rows with textOnSilence, repetitionLoop or insertionBurst ≥ 5
  languageAccuracy: number | null;
  releaseToTextP50: number | null; releaseToTextP95: number | null;   // short clips only
  firstWordP50: number | null; firstWordP95: number | null;
  rtfMedian: number | null;
  failures: number;
};

type Aggregates = {
  // byModel[modelId][mode]
  byModel: Record<string, Record<string, {
    overall: Group;
    byLanguage: Record<string, Group>;     // "de" | "en" | "fr" | "mixed" | "none"
    byCategory: Record<string, Group>;     // "short" | "long"
    byTag: Record<string, Group>;          // every tag, plus "condition:<name>" and "clean" (clips with no condition)
  }>>;
  // pairwise[mode][language] (language includes "all"): paired bootstrap on clips both models scored
  pairwise: Record<string, Record<string, {
    a: string; b: string;
    clips: number;
    werA: number; werB: number;
    difference: number;           // werA − werB
    ci95: [number, number];
    pValue: number;
  }[]>>;
  // Same text read clean vs under a condition: paired, per model and condition
  conditions: Record<string, Record<string, { clips: number; cleanWer: number | null; conditionWer: number | null }>>;
};
```

---

## Review API (`blabber-bench corpus review`)

The server listens on `127.0.0.1` only and serves `review.html` at `/`.

- **`GET /api/clips`** returns:

  ```ts
  { conventions: string;
    clips: {
      id; language; languages; category; tags; condition; script; durationMs;
      reference: string; referenceStatus: "draft" | "verified";
      scriptText: string | null;              // original script text, when scripted
      terms: string[]; notes: string;
      drafts: { model: string; text: string; segments: { startMs: number; endMs: number; text: string }[] }[];
    }[] }
  ```

- **`GET /audio/<id>`** returns `audio/wav` and supports `Range` requests, so the player can seek.

- **`POST /api/clips/<id>`** takes the JSON body `{ reference: string; referenceStatus: "draft" | "verified"; terms: string[]; notes: string }` and returns `{ ok: true }`. On failure it returns `{ ok: false, error: string }` with status 400 or 404.
