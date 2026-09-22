# Third-party notices

## R2T2 and audio.cpp

The isolated R2T2 helper embeds audio.cpp (Apache-2.0) at
`eb8e21bd7b71dc104d6be7a88f5641bcd5373617`. Blabber's separately tracked patch
adds sample-indexed 16-second rolling windows, committed-prefix checks and
complete final flushing. The rolling design derives from NetEase Youdao's
Apache-2.0 source at `c4611929bc3592b38dab34e96a8c9940d6da3755`.
Runtime and bundled dependency licenses accompany the helper under
`workers/r2t2/` in the app's resources.

Weights are downloaded separately from
`davidxifeng/Confucius4-R2T2-gguf`, revision
`a8e6b385d7df7eae9519363e07034a209004797a`, unchanged Q8_0 quantization.
They use the **NetEase Youdao Model Use License Agreement**, not Apache-2.0.
The complete terms are retained in `workers/r2t2/MODEL_LICENSE`, bundled with the
helper, and linked in model download details.

Any modifications made to the original model in this Derivative Work are not
endorsed, warranted, or guaranteed by the original right-holder of the original
model, and the original right-holder disclaims all liability related to this
Derivative Work.

Upstream: https://github.com/netease-youdao/Confucius4-R2T2
Runtime: https://github.com/0xShug0/audio.cpp

## TranslateGemma and llama.cpp

The translation helper statically embeds llama.cpp at commit
`972d2313bc0bf0a45f634f77d95c9fb03aeab12c` (MIT), including its Metal backend.
The full license is bundled at `src-tauri/licenses/llama.cpp-MIT.txt`.
Upstream: https://github.com/ggml-org/llama.cpp

TranslateGemma 12B is downloaded separately. Original model:
https://huggingface.co/google/translategemma-12b-it

The Q6_K GGUF is an upstream quantization by mradermacher, downloaded unchanged
from `mradermacher/translategemma-12b-it-GGUF`, revision
`1076826a801dbc6cc8ad4ff4689a3272dcb8a378`.
SHA-256: `c30995b3c145e6ef3b3a6fda63749186d83d9c8f16725ff1403c904b5e0ead8c`.
Blabber adds its own text prompt for mixed German/English and Argentinian usage;
these adaptations are not Google's official quality claims.

Gemma is provided under and subject to the Gemma Terms of Use found at
https://ai.google.dev/gemma/terms. Downloading and using these model weights is
subject to those terms and their incorporated use restrictions at
https://ai.google.dev/gemma/prohibited_use_policy.
Copies are included in `src-tauri/licenses/Gemma-Terms-of-Use.txt`,
`Gemma-Prohibited-Use-Policy.txt`, and `Notice.txt`, and in the app resources.

## qwen-asr

Blabber vendors a modified snapshot of `antirez/qwen-asr` at commit
`b00b789b17051aea61e9717458171100662318a4`.

Upstream: https://github.com/antirez/qwen-asr

License: MIT. The complete license text is retained at
`src-tauri/vendor/qwen-asr/LICENSE`.

Local modifications expose the language detected for each offline audio chunk
and provide an allocator-safe function for releasing returned transcript text.

## Qwen3-ASR-1.7B

The model is downloaded separately from the pinned Hugging Face revision
`b188e100bd85038c06d2812d24a39776eba774ca` and is licensed under Apache-2.0.

## sherpa-onnx

The offline speaker-diarization runtime is pinned to sherpa-onnx 1.13.5 and
runs in an isolated local worker process. sherpa-onnx is licensed under
Apache-2.0.

Upstream: https://github.com/k2-fsa/sherpa-onnx/tree/v1.13.5

License: https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.5/LICENSE

## Speaker diarization model weights

The speaker-diarization weights are downloaded separately and are not bundled
with Blabber. Blabber verifies their byte sizes and SHA-256 hashes before
installing them.

The segmentation artifact is an ungated sherpa-compatible ONNX conversion of
the MIT-licensed pyannote segmentation 3.0 model, pinned to revision
`340b52f1f5cd12d45a30fa284691417eaad2ff92`. The original pyannote repository
uses a contact-sharing download gate; this private-use build downloads the
public conversion instead. The MIT license text is retained at
`src-tauri/licenses/pyannote-segmentation-3.0-MIT.txt`.

The ERes2Net VoxCeleb speaker-embedding artifact comes from 3D-Speaker and is pinned to
revision `8be2a75c9ed7a590538b268e46fbb65e1aa9d208`. It is licensed under
Apache-2.0; the license text is retained at
`src-tauri/licenses/3D-Speaker-APACHE-2.0.txt`.

The reviewed artifact manifest is
`src-tauri/model-manifests/sherpa-diarization-pyannote3-eres2net-voxceleb-v2.json`.
The provenance decision must be reviewed again before distributing Blabber to
other users.

## MOSS Transcribe-Diarize and moss-transcribe.cpp

MOSS Transcribe-Diarize 0.9B F16 weights are downloaded separately from the
pinned `mudler/moss-transcribe.cpp-gguf` revision
`54e4bbd17da3f84adf1c1bcf7791b9b9266f741e`. The weights retain the upstream
Apache-2.0 license.

Blabber builds the native `moss-transcribe.cpp` runtime as an isolated worker at
commit `190a569c13b4b247450f2fb3b2a431244e84833e`. The port is MIT-licensed. Blabber's
local patch exposes the existing model prompt so vocabulary can be appended as
hotwords without linking the worker's ggml symbols into the app.

Upstream: https://github.com/localai-org/moss-transcribe.cpp

## VibeVoice-ASR, MLX, and mlx-audio

VibeVoice-ASR 8-bit MLX weights are downloaded separately from the pinned
`mlx-community/VibeVoice-ASR-8bit` revision
`725c72e54d6ef875472c27fbc50fab470a960940`. The model card declares the model
MIT-licensed.

The Apple Silicon worker uses `mlx-audio` 0.4.8 and its MLX runtime in a signed,
one-folder bundle. MLX and mlx-audio are MIT-licensed.

Upstreams: https://github.com/ml-explore/mlx and https://github.com/Blaizzy/mlx-audio
