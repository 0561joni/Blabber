#!/usr/bin/env bash
set -euo pipefail

revision=190a569c13b4b247450f2fb3b2a431244e84833e
source_dir="${1:-build/source}"
output_dir="${2:-dist}"
git clone --recursive https://github.com/localai-org/moss-transcribe.cpp.git "$source_dir"
git -C "$source_dir" checkout "$revision"
git -C "$source_dir" submodule update --init --recursive
git -C "$source_dir" apply "$(cd "$(dirname "$0")" && pwd)/moss-prompt.patch"
cmake -S "$source_dir" -B "$source_dir/build-blabber" -DMT_BUILD_TESTS=OFF -DGGML_NATIVE=OFF
cmake --build "$source_dir/build-blabber" --config Release --parallel
mkdir -p "$output_dir"
cp "$source_dir/build-blabber/moss-transcribe" "$output_dir/moss-transcribe"
cp "$(dirname "$0")/blabber_moss_worker.py" "$output_dir/blabber-moss-worker"
chmod +x "$output_dir/moss-transcribe" "$output_dir/blabber-moss-worker"
