# Release notes

## Native long-form transcription models

- Added MOSS Transcribe + Diarize 0.9B F16 on macOS, Windows, and Linux for dictation and files up to 90 minutes.
- Added VibeVoice-ASR 8-bit MLX for file transcription on Apple Silicon with macOS 14 or newer; 32 GB unified memory is recommended.
- Native timestamps and speaker labels are now preserved automatically and identified as built-in in History. Explicit speaker retry still replaces them with standalone post-processing.
- Blabber vocabulary is passed as MOSS hotwords and VibeVoice context. Fixed-language settings remain informational for these automatic, code-switching models.
- Retired Whisper Tiny. Existing managed weights and partials are removed once, saved selections migrate to an installed fallback, and later manually added Tiny files are ignored.
