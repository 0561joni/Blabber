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
