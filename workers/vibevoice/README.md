# VibeVoice MLX worker

Build this worker on Apple Silicon with Python 3.12 and the exact packages in
`requirements.lock`. Use PyInstaller's one-folder mode, include the MLX native
libraries and Metal resources, then sign the entire resulting folder with the
same identity as the app. The worker forces Hugging Face and Transformers
offline mode and accepts only the verified local model directory supplied by
Blabber.

Install the locked requirements into the Python 3.12 build environment, then run:

```sh
./build.sh
```

Set `BLABBER_CODESIGN_IDENTITY` for release builds. The script signs the complete
one-folder bundle after PyInstaller has collected MLX, its native libraries, and
Metal resources.
