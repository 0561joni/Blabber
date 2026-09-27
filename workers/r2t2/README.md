# R2T2 native streaming worker

This helper powers the optional live shortcut-dictation integration described in
[`docs/r2t2.md`](../../docs/r2t2.md). `manifest.json` pins the source and model and
controls normal-build availability. `releaseEnabled` is true for opt-in experimental
testing in the app; human-recording and interactive acceptance remain outstanding.
Apple Silicon app builds
bundle the signed helper and notices. The model is a separate explicit download;
existing model selections and defaults are unchanged.

## Reproduce

Apple Silicon, macOS 14+, CMake, Node and Apple Command Line Tools are required.

```sh
npm run build:r2t2
python3 -m unittest discover -s workers/r2t2 -p test_protocol.py
python3 workers/r2t2/make_probe_audio.py --individual
python3 workers/r2t2/evaluate.py \
  --audio src-tauri/target/r2t2-fixtures/de-synthetic.wav \
  --language German --output benchmark-results/r2t2-german-synthetic.json
python3 workers/r2t2/exercise_native.py \
  --output benchmark-results/r2t2-native-lifecycle.json
```

The model is a separate, explicit download. Use the revision and file in
`manifest.json` at `https://huggingface.co/<modelRepository>/resolve/<modelRevision>/<modelFile>`.
Store it at `src-tauri/target/r2t2-model/r2t2-q8_0.gguf`, or pass `--model` to
the probe. Both its exact size and SHA-256 are verified before evaluation. A
download interrupted before verification is not usable. Q4/Q5 are incompatible
with this runtime. No cloud inference is involved.

The builder verifies the complete upstream source archive before applying
`runtime.patch`. Its ignored source cache stamp includes the patch and rolling
ledger checksums. The helper statically links the C API in its own executable,
isolating its ggml/Metal runtime from Blabber's Whisper runtime. It embeds Metal
shaders and signs with `APPLE_SIGNING_IDENTITY`, or an ad-hoc development
signature when that variable is absent. This is not a notarized release build.

## Streaming protocol v1

The transport is UTF-8 newline-delimited JSON on stdin/stdout. Stderr is
diagnostics. Every request contains `version: 1`, a nonempty `sessionId` (at most
128 bytes), `sequence` and `type`. A session starts at request sequence zero;
subsequent requests increment it exactly once. Response sequences begin at one
and increase independently. One response acknowledges each request. Frames are
limited to 1 MiB. Errors terminate the worker with a nonzero exit code; they are
not final transcription results.

| Request | Fields | Response |
|---|---|---|
| `start` | `modelPath`, `chunkMs` (320/640/1280/2000), `rollbackTokens` (1/5), `language` (canonical name or empty for automatic), optional `context` | `ready` after load/reset |
| `audio` | `startSample`, `samples` (finite floats in [-1, 1], 16 kHz mono) | `progress`, cumulative committed `text`, `processedSamples` |
| `finish` | `totalSamples`, exactly the acknowledged sample count | `result`, complete final `text` |
| `cancel`, `reset` | No extra fields | `canceled` |

All responses repeat version, session ID and response sequence, and include
`elapsedMs` and process-lifetime `peakRssBytes`. An `error` carries a stable `code` and empty text. Exactly one
configured inference chunk is passed to each native streaming call. Only the
last audio frame can be shorter than a chunk; it must be followed by finish or
cancellation. Five minutes (4,800,000 samples) is the maximum accepted input.

Progress responses also contain `tentativeText`, a complete display-only suffix
that may change or disappear. `text` retains its cumulative committed semantics.
The native event's separate optional preview accessor distinguishes no decode
(preserve the previous preview for a buffered audio tail) from an empty preview
(clear it). Final results and session lifecycle responses carry an empty preview.
Older v1 helpers without this additive field fall back to stable-only rendering.

The overlay renders tentative text in a secondary color, including unfinished
words, without another inference call or an additional delay. Stable and draft
text share one owned, revisioned snapshot. Draft text never enters prompts,
history, translation, the clipboard, or the final transcript. Finalization clears
the draft before vocabulary-corrected source text is shown during translation.

The helper reuses loaded weights only after finish/reset has succeeded. The
parent must own admission, the 60-second idle timeout, cancellation during a
native call, memory-pressure eviction and process-group termination/reaping.
Sending a cancel frame alone cannot interrupt an in-flight synchronous native
decode. EOF is successful only between sessions; EOF in a session is an error.
The Rust parent invalidates session ownership before termination and rejects late
messages, history writes and insertion. Its queue owns cached workers and reaps
them before admitting a different model workload.

## Rolling implementation

The patch implements the official no-reset 16-second audio window with an
8-second advance. Decode samples identify retained text entries rather than the
reference's hardcoded 49/50 chunk counts. Completed text is independent of the
rolling prompt. The native parser removes leading whitespace, so only the
retained prompt is normalized at rollover. Committed display text is unchanged.

Each decode must extend the committed byte prefix. A shorter rollback result
that is itself an exact prefix represents withheld tokens, not new output. It
cannot replace committed text, and is never accepted as the final result.
Inconsistent prefixes and exhausted generation budgets fail explicitly. Finish
always decodes the withheld suffix, including when there is no partial audio
chunk. The final C-API result is consumed and checked against committed output.
There are no guessed sentence cuts or fuzzy transcript deduplication.

Automatic language detection may return speech with `language None`. The
streaming prompt retains that metadata separator whenever committed text
exists, even if the language is unknown. Otherwise the next decode mistakes
the transcript for metadata and finalization fails with `R2T2_PREFIX_MISMATCH`.
The native parser/ledger regression covers this case without relaxing prefix
validation. Initial empty sessions still allow language detection.

Emission sample positions are not acoustic word timestamps. A ledger unit test
cannot prove recognition at an audio seam: real recordings remain essential.

## Evaluation limits

`evaluate.py` measures loading, processing throughput and final flush and records
the exact binary/model/fixture identity. `--realtime` paces audio delivery; its
unpaced first-text time is compute time, not visible-word delay. Acoustic word
latency requires annotated speech. Probe reports deliberately contain the
synthetic fixture transcripts; production diagnostics must not contain audio
or transcript content. Raw native probe stderr is ignored by Git.

`make_probe_audio.py --individual` creates 20 synthetic clips per language and
five-minute English, German and alternating-language fixtures, with complete
utterances spanning the eight-second rolling boundaries. `exercise_native.py`
checks short clips in one warm process and also exercises zero samples, a
one-sample tail, an exact chunk, 32 seconds of silence, cancel and reset.
`compare_baseline.py` invokes an existing app transcription worker without
opening the app or modifying settings. Its basic WER normalization folds case
and punctuation, but does not equate spoken numbers and written numerals.
Synthetic comparisons are engineering probes, not the human release gate.

`verify_preview.py --worker PATH --output REPORT` records hashes of every
committed update and final result across the short and five-minute fixtures.
Repeat with `--baseline REPORT` and the new packaged helper to require exact
recognition parity and nonempty tentative updates. These reports contain only
hashes, counts, and timings. Run old and new helpers sequentially to avoid GPU
and memory contention; keep the baseline executable before rebuilding.
`--lifecycle-only` separately exercises silence, exact and short tails, repeated
cancel/reset with active previews, and clearing a draft on a protocol failure.

The tentative-preview comparison in
[`benchmark-results/r2t2-tentative-parity.json`](../../benchmark-results/r2t2-tentative-parity.json)
passed all 40 short clips and three five-minute fixtures: every committed update
and final transcript matched the previous helper. All short clips showed draft
text earlier, with a median first-preview lead of 1,280 ms of input audio. This
is not a measurement of acoustic word latency. The actual overlay component's
local browser checks rendered updates in approximately 9–17 ms after receipt;
the backend retains its existing 100 ms publication cycle. Physical multi-monitor
and microphone-to-paste acceptance remain manual checks.

## Attribution and terms

Runtime: audio.cpp, Apache-2.0. Rolling design: NetEase Youdao Confucius4-R2T2,
Apache-2.0 source at the official revision in `manifest.json`. Blabber's patch
changes window bookkeeping, prefix validation and finalization. Model weights:
the separate NetEase Youdao Model Use License Agreement in `MODEL_LICENSE`.

Any modifications made to the original model in this Derivative Work are not
endorsed, warranted, or guaranteed by the original right-holder of the original
model, and the original right-holder disclaims all liability related to this
Derivative Work.
