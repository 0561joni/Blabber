#!/usr/bin/env python3
"""
Reproducible generator for Blabber UI sounds.

Run: python3 tools/gen_sounds.py

Produces 16-bit PCM mono 44.1 kHz WAV files into src-tauri/assets/sounds/.
The output is meant to be committed; this script is not invoked at build time.

Design notes:
- `listen_start`: two-note ascending blip, C5 (523.25 Hz) -> G5 (783.99 Hz).
- `listen_stop`: two-note descending blip, G5 (783.99 Hz) -> C5 (523.25 Hz).
- Total duration ~140 ms.
- 5 ms attack, exponential decay -> avoids the "click" of a hard onset
  while staying short enough to not delay the user.
- Peak amplitude 0.35 (full-scale = 1.0) -> deliberately quieter than
  typical macOS system sounds; pleasant for many-times-a-day usage.
"""

from __future__ import annotations

import math
import os
import struct
import wave
from pathlib import Path

SAMPLE_RATE = 44_100
BIT_DEPTH = 16
PEAK_AMPLITUDE = 0.35  # of full-scale; intentionally subtle

REPO_ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = REPO_ROOT / "src-tauri" / "assets" / "sounds"


def envelope(t: float, total: float, attack: float = 0.005, decay_tau: float = 0.045) -> float:
    if t < 0.0 or t > total:
        return 0.0
    if t < attack:
        return t / attack
    return math.exp(-(t - attack) / decay_tau)


def tone(freq: float, start: float, duration: float, total_duration: float) -> list[float]:
    samples = [0.0] * int(total_duration * SAMPLE_RATE)
    for i in range(len(samples)):
        t_global = i / SAMPLE_RATE
        t_local = t_global - start
        if 0.0 <= t_local <= duration:
            env = envelope(t_local, duration)
            samples[i] += math.sin(2.0 * math.pi * freq * t_local) * env
    return samples


def mix(*tracks: list[float]) -> list[float]:
    n = max(len(t) for t in tracks)
    out = [0.0] * n
    for track in tracks:
        for i, v in enumerate(track):
            out[i] += v
    peak = max(abs(v) for v in out) or 1.0
    scale = PEAK_AMPLITUDE / peak
    return [v * scale for v in out]


def write_wav(path: Path, samples: list[float]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    max_int = 2 ** (BIT_DEPTH - 1) - 1
    frames = b"".join(
        struct.pack("<h", max(-max_int - 1, min(max_int, int(v * max_int))))
        for v in samples
    )
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(BIT_DEPTH // 8)
        wav.setframerate(SAMPLE_RATE)
        wav.writeframes(frames)


def build_listen_start() -> list[float]:
    total = 0.140
    note_a = tone(freq=523.25, start=0.000, duration=0.080, total_duration=total)
    note_b = tone(freq=783.99, start=0.055, duration=0.085, total_duration=total)
    return mix(note_a, note_b)


def build_listen_stop() -> list[float]:
    total = 0.140
    note_a = tone(freq=783.99, start=0.000, duration=0.075, total_duration=total)
    note_b = tone(freq=523.25, start=0.052, duration=0.088, total_duration=total)
    return mix(note_a, note_b)


def main() -> None:
    sounds = {
        "listen_start.wav": build_listen_start(),
        "listen_stop.wav": build_listen_stop(),
    }
    for filename, samples in sounds.items():
        out = OUT_DIR / filename
        write_wav(out, samples)
        print(f"wrote {out} ({len(samples)} samples, {len(samples)/SAMPLE_RATE*1000:.1f} ms)")


if __name__ == "__main__":
    main()
