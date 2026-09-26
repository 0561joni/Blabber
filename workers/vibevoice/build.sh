#!/usr/bin/env bash
set -euo pipefail

python_bin="${PYTHON312:-python3.12}"
output_dir="${1:-dist}"
build_dir="${2:-build}"
"$python_bin" -m PyInstaller \
  --noconfirm \
  --clean \
  --onedir \
  --name blabber-vibevoice-worker \
  --distpath "$output_dir" \
  --workpath "$build_dir/work" \
  --specpath "$build_dir/spec" \
  --collect-all mlx \
  --collect-all mlx_audio \
  --collect-all mlx_lm \
  --collect-submodules transformers.models.qwen2 \
  --hidden-import miniaudio \
  "$(dirname "$0")/blabber_vibevoice_worker.py"

bundle="$output_dir/blabber-vibevoice-worker"
if [[ -n "${BLABBER_CODESIGN_IDENTITY:-}" ]]; then
  codesign --force --deep --options runtime --timestamp \
    --sign "$BLABBER_CODESIGN_IDENTITY" "$bundle"
fi
