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

The pyannote segmentation 3.0 and 3D-Speaker ERes2Net weights are separate
downloadable artifacts and are not bundled with Blabber. Their redistribution
and local-commercial-use review is unresolved; consequently the package is not
yet exposed as installable. A future reviewed artifact manifest must record the
exact weight revision, SHA-256, license identifier, and reviewed license URL
before enabling installation.
