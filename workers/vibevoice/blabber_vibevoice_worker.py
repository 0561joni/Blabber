#!/usr/bin/env python3
"""Offline, one-folder PyInstaller entry point for VibeVoice-ASR MLX."""

import json
import os
import sys
import tempfile
import threading
import time

PROTOCOL_VERSION = 1
ORIGINAL_PARENT = os.getppid()


def emit(record):
    print(json.dumps(record, ensure_ascii=False), flush=True)


def value(item, *names, default=None):
    for name in names:
        if isinstance(item, dict) and name in item:
            return item[name]
        if hasattr(item, name):
            return getattr(item, name)
    return default


def handle(request):
    if request.get("protocolVersion") != PROTOCOL_VERSION:
        raise ValueError("unsupported worker protocol version")
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    from mlx_audio.stt.generate import generate_transcription
    from mlx_audio.stt.utils import load_model

    emit({"type": "progress", "progress_percent": 1})
    model = load_model(request["modelPath"])
    max_tokens = int(request.get("maxTokens", 32768))
    with tempfile.TemporaryDirectory(prefix="blabber-vibevoice-") as output_dir:
        transcription = generate_transcription(
            model=model,
            audio=request["audioPath"],
            output_path=os.path.join(output_dir, "transcript"),
            format="json",
            verbose=False,
            max_tokens=max_tokens,
            context=request.get("prompt") or None,
        )
    raw_segments = value(transcription, "sentences", "segments", default=[]) or []
    segments = []
    for item in raw_segments:
        start = value(item, "start", "start_time", default=0)
        end = value(item, "end", "end_time", default=start)
        speaker = value(item, "speaker", "speaker_id")
        language = value(item, "language", "language_code")
        segments.append({
            "startMs": round(float(start) * 1000),
            "endMs": round(float(end) * 1000),
            "speaker": str(speaker) if speaker is not None else None,
            "text": str(value(item, "text", default="")),
            "languageCode": str(language) if language is not None else None,
        })
    text = str(value(transcription, "text", default=""))
    generated_tokens = int(value(transcription, "generation_tokens", default=0) or 0)
    truncated = bool(value(transcription, "truncated", "hit_token_limit", default=False)) or generated_tokens >= max_tokens
    emit({"type": "progress", "progress_percent": 100})
    emit({"type": "result", "result": {"text": text, "segments": segments, "truncated": truncated}})


def main():
    def stop_if_parent_exits():
        while True:
            if os.getppid() != ORIGINAL_PARENT:
                os._exit(130)
            time.sleep(0.5)

    threading.Thread(target=stop_if_parent_exits, daemon=True).start()
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            handle(json.loads(line))
        except MemoryError:
            emit({"type": "error", "code": "MODEL_OUT_OF_MEMORY", "message": "VibeVoice ran out of unified memory"})
        except Exception as error:
            emit({"type": "error", "code": "MODEL_WORKER_FAILED", "message": str(error)})


if __name__ == "__main__":
    main()
