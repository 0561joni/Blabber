# Third-party notices

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
