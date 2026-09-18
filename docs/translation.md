# Local dictation translation

## User behavior

Apple Silicon supports German/English dictation with Original, French (`fr`), or
Argentinian Spanish (`es-AR`) output. The output mode is deliberately not persisted.
The default global language shortcut is `CmdOrCtrl+Shift+Right`. It advances once
per key press, is editable in Settings → Models, and is suspended together with
the dictation shortcut during key capture. Parsed shortcut IDs detect conflicts;
registration failure restores both previous bindings.

Translation is disabled by default. A completed, checksum-verified model download
enables it and registers the additional shortcut. Readiness asynchronously verifies
the installed model on first use and after a file change. Missing, incomplete, or
damaged models cannot start a translated dictation. The Dictate screen links to setup.

The passive, non-focusable, click-through overlay displays the output language for
two seconds after switching and continuously through capture and processing.
Revision checks prevent old timers or microphone levels from replacing newer states.

## Inference and cancellation

`TranslationService` owns a session from capture admission through final output.
The session captures output language, history preference, GPU preference, recording
identity, and a cancellation flag. Manual transcription must claim the exact
recording ID/path once, after stop hands it off. Microphone tests do not create a
dictation session. Mode changes are rejected during capture, handoff, queuing,
ASR, model loading, and translation.

The common pipeline is:

```
capture → queue permit → isolated ASR → dictionary correction
  → [source + pending history, when enabled]
  → isolated translation → complete result/history → paste or copy
```

The parent alone owns the shared inference permit. Dictation takes priority over
waiting file/speaker jobs; the current job runs to completion. Active ownership is
separate from pending admission, so cancellation cannot release a running job's
permit early. A new capture may start while a previous canceled child is exiting;
its inference still waits for that child's permit to be released.

ASR uses the existing `--transcribe-worker` entry point. Parent ASR caches are
released before it starts, and its successful process exit is required before
translation. Translation uses a separate native executable; it is terminated and
reaped after each request. On Unix the parent owns an inference process group,
including any nested native workers, and cancellation terminates the whole group.
Texts are sent through stdin, never command arguments or diagnostic logs.

The dictation ASR budget is 90 seconds after queue admission. Translation model
preparation/loading has a 90-second budget and each chunk has 180 seconds; neither
queue waiting nor translation uses the old global 90-second inference watchdog.
Both ASR and translation allow at most ten seconds to exit after returning output.
All inference waits poll cancellation. Reset/shutdown invalidate the session;
the main-thread paste callback rechecks session identity, cancellation, generation,
and shutdown immediately before calling the existing insertion path.

## Runtime, model and prompt

| Item | Pin |
| --- | --- |
| Model | TranslateGemma 12B instruction-tuned, Q6_K GGUF |
| Download repository | `mradermacher/translategemma-12b-it-GGUF` |
| Model revision | `1076826a801dbc6cc8ad4ff4689a3272dcb8a378` |
| Artifact | `translategemma-12b-it.Q6_K.gguf` |
| Bytes | `9660827392` |
| SHA-256 | `c30995b3c145e6ef3b3a6fda63749186d83d9c8f16725ff1403c904b5e0ead8c` |
| llama.cpp | `972d2313bc0bf0a45f634f77d95c9fb03aeab12c` |
| Source archive SHA-256 | `1eea355e60e4898c7121764170fc23dd5a6735be2dc84e9cd45eab7ff4760775` |
| JSON protocol / prompt | `1` / `4` |

`workers/translation/main.cpp` is a text-only llama.cpp helper. Static linkage and
embedded Metal kernels avoid separate dylib/runtime installation and isolate ggml
from Whisper. `scripts/build-translation-worker.mjs` verifies source bytes, builds
the helper, copies its MIT license, and signs the binary. Tauri includes it at
`Contents/Resources/workers/blabber-translation-worker` on macOS. GGUF weights are
downloaded by the existing resumable model manager and are not inside the app.

On macOS the build wrappers use the separately installed Command Line Tools when
available; an explicit `DEVELOPER_DIR` takes precedence. This machine's native
builds succeeded with that toolchain without changing or accepting its Xcode license.

One newline-delimited stdin request contains `version`, `requestId`, `modelPath`,
`text`, `sourceLanguages`, `targetLanguage`, and `preferGpu`. Stdout contains
`ready`, sequential `progress`, and exactly one completed `result`, or a structured
`error`. Every record repeats protocol version, prompt version and request ID. Invalid protocol, omitted
chunks, nonzero exit, timeout, empty output, or `OUTPUT_TRUNCATED` fails the job.

The prompt uses Gemma user/model turns and an application-specific translation
instruction. It asks the model to preserve tone, formality, numbers, names, negation,
URLs and paragraphs; this is not a guarantee of linguistic accuracy. ASR language
hints are advisory because input may mix German and
English. English “you” defaults to informal address if context supplies no formal
signal. Argentine output uses informal `vos` and voseo, formal `usted`, and plural
`ustedes`, without added slang. Source control-token-like strings are tokenized
as literal text, not chat delimiters. Instruction-like source sentences remain
translation content; corpus evaluation includes such cases.

Chunks prefer paragraph/sentence boundaries, then spaces, with UTF-8-safe fallback.
The actual model tokenizer verifies the 768-source-token cap; fully formatted
prompts must fit 2048 tokens. The helper uses a 4096-token context and permits up
to 1536 output tokens per chunk. It requires an end-of-generation token and joins
all successful chunks before emitting any result. No partial text is pasted.

## Storage and output

`TranscriptResult` remains the original, including segments, timestamps and language
metadata. `DictationOutput` independently stores source/output text, target, status,
model/revision/prompt provenance, source language hints, and structured error code.

An idempotent migration adds settings and `transcript_translations` with a cascading
foreign key. The translation's searchable output has its own column; structured
metadata lives in a version-tolerant JSON payload. Source and pending translation
are committed atomically before inference. At restart interrupted pending rows
become failed, with the source available for retry. Completion updates cannot
recreate a transcript the user has deleted.

With history disabled, no source/translation is implicitly saved on translation
or paste errors. Failed output remains in the current session. The existing
Original-only clipboard fallback is preserved. Retrying uses the retained source
and original target, including after restarting from a saved failed translation;
it returns a copyable result and never triggers automatic paste.

History detail defaults to the translation with an Original switch and separate
copy actions. Search matches both texts. TXT/Markdown allow the selected version;
JSON includes both; SRT/VTT continue to use original timed segments. File imports
do not enter the translation pipeline.

## Reproduce evaluation

The checked-in corpus contains 40 paired German/English cases, each translated
to French and Argentinian Spanish (160 outputs). It includes informal/formal/plural
address, mixed language, negations, numbers, names, technical terms, URLs,
instruction-like content, paragraphs, and multi-chunk inputs.

```sh
npm run build:translation
python3 workers/translation/evaluate.py \
  --worker src-tauri/bundle/translation/blabber-translation-worker \
  --model /path/to/translategemma-12b-it.Q6_K.gguf \
  --output benchmark-results/translation-12b-q6-m3-pro.jsonl
```

Python is only a development evaluation tool. `--resume` retains successful rows
only when helper, prompt and model hashes match; stale or failed rows are rerun.
Use a new output file to preserve measurements from an earlier version. The report
records output, timings, maximum process RSS, chunk count, automatic checks and
unfilled human-review fields. Automatic checks cover successful complete output,
selected exact literals and absent translation prefaces. They do not prove semantic
accuracy, correct negation, or native Argentinian usage.

## Release acceptance still required

- A human reviewer marks all 160 outputs; at least 95% in each direction must be
  usable without necessary meaning correction. Any critical number, negation or
  instruction error blocks release. Argentinian forms require a qualified reviewer.
- Real microphone dictation into target applications validates both shortcuts,
  focus preservation, clipboard restoration and one-time insertion, including
  cancellation just before insertion and an inaccessible target application.
- Measure complete ASR-plus-translation latency and system memory pressure with
  the user's selected ASR model and typical other running applications.
- Signed distribution/notarization and Intel/Windows/Linux regression builds are
  separate from local Apple Silicon app packaging. Translation remains unavailable
  on those platforms; no silent smaller-model fallback is provided.

See the [measurement and acceptance report](../benchmark-results/translation-validation.md)
and [raw corpus results](../benchmark-results/translation-12b-q6-m3-pro.jsonl)
for the tests actually performed on this machine. The feature
is implemented but these human/real-application checks must not be represented as
already passed.
