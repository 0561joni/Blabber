# MOSS worker

`build.sh` checks out `moss-transcribe.cpp` at commit
`190a569c13b4b247450f2fb3b2a431244e84833e` and builds it as an isolated
executable. The port's ggml symbols therefore never enter Blabber's Tauri
process or conflict with whisper.cpp.

The packaged directory contains `blabber-moss-worker` (the NDJSON adapter) and
`moss-transcribe` (the native CPU runtime). The Blabber patch adds `--prompt` so
the official diarization/timestamp prompt can receive `Hotwords: …`.
