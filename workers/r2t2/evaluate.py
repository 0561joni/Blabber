#!/usr/bin/env python3
"""Local, opt-in native probe. No network, microphone, paste, or app database.

Reports are explicit evaluation artifacts and contain fixture transcripts.
Production worker diagnostics must not contain those transcripts.
"""
import argparse
import array
import hashlib
import json
import os
from pathlib import Path
import queue
import resource
import subprocess
import threading
import time
import uuid
import wave


def read_pcm(path):
    with wave.open(str(path), "rb") as wav:
        if (wav.getframerate(), wav.getnchannels(), wav.getsampwidth()) != (16000, 1, 2):
            raise ValueError("Fixture must be 16 kHz mono PCM16 WAV")
        samples = array.array("h", wav.readframes(wav.getnframes()))
    return [sample / 32768.0 for sample in samples]


def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def probe(args, samples, chunk_ms, rollback):
    sid = str(uuid.uuid4())
    began = time.monotonic()
    log = args.output.with_suffix(f".{chunk_ms}-{rollback}.stderr.log")
    result = dict(chunkMs=chunk_ms, rollbackTokens=rollback, language=args.language,
                  audioSeconds=len(samples) / 16000, mode="rolling16s8s", events=[])
    with open(log, "w") as stderr:
        process = subprocess.Popen([str(args.worker.resolve())], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=stderr, text=True,
                                   start_new_session=True, env={**os.environ, "BLABBER_R2T2_PROBE": "1"})
        incoming = queue.Queue()
        def reader():
            try:
                for line in process.stdout:
                    incoming.put(json.loads(line))
            except Exception as exc:
                incoming.put(exc)
            finally:
                incoming.put(None)
        threading.Thread(target=reader, daemon=True).start()
        sequence = 0
        last_output_sequence = 0
        def exchange(kind, **fields):
            nonlocal sequence, last_output_sequence
            message = dict(version=1, sessionId=sid, sequence=sequence, type=kind, **fields)
            sequence += 1
            process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
            process.stdin.flush()
            try:
                response = incoming.get(timeout=90)
            except queue.Empty as exc:
                raise RuntimeError("R2T2_WORKER_TIMEOUT: no response for 90 seconds") from exc
            if not isinstance(response, dict):
                raise RuntimeError(f"Unexpected worker EOF/parse failure: {response}")
            if (response.get("version") != 1 or response.get("sessionId") != sid or
                    response.get("sequence") != last_output_sequence + 1):
                raise RuntimeError("Worker returned a stale/unordered message")
            last_output_sequence += 1
            if response["type"] == "error":
                raise RuntimeError(response["code"])
            return response
        try:
            exchange("start", modelPath=str(args.model.resolve()), language=args.language,
                     chunkMs=chunk_ms, rollbackTokens=rollback)
            result["loadSeconds"] = time.monotonic() - began
            decode_start = time.monotonic()
            first_text = None
            previous = ""
            for start in range(0, len(samples), chunk_ms * 16):
                if args.realtime:
                    # Capture starts with the worker launch, before model readiness.
                    # Loading backlog is retained, then drained at decoder speed.
                    time.sleep(max(0, began + min(start + chunk_ms * 16, len(samples)) / 16000 - time.monotonic()))
                response = exchange("audio", startSample=start, samples=samples[start:start + chunk_ms * 16])
                if response["type"] != "progress":
                    raise RuntimeError("Missing audio acknowledgment")
                text = response["text"]
                if not text.startswith(previous):
                    raise RuntimeError("Visible text changed")
                now = time.monotonic() - decode_start
                if text and first_text is None:
                    first_text = now
                event = dict(sample=response["processedSamples"], elapsedSeconds=now,
                             delta=text[len(previous):])
                if args.realtime:
                    event["acknowledgmentLagSeconds"] = max(0, time.monotonic() - began - response["processedSamples"] / 16000)
                result["events"].append(event)
                previous = text
            stop_at = time.monotonic()
            response = exchange("finish", totalSamples=len(samples))
            if response["type"] != "result" or not response["text"].startswith(previous):
                raise RuntimeError("Missing or inconsistent final result")
            result.update(status="completed", text=response["text"], firstTextSeconds=first_text,
                          finalizeSeconds=time.monotonic() - stop_at,
                          streamSeconds=time.monotonic() - decode_start)
            result["realTimeFactor"] = result["streamSeconds"] / max(result["audioSeconds"], .001)
            if args.realtime:
                result["finishAfterCaptureSeconds"] = max(0, time.monotonic() - began - result["audioSeconds"])
        except Exception as exc:
            result.update(status="failed", error=str(exc), wallSeconds=time.monotonic() - began)
        finally:
            try:
                process.stdin.close()
                process.wait(timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                os.killpg(process.pid, 9)
                process.wait()
            result["exitCode"] = process.returncode
            if process.returncode != 0 and result.get("status") == "completed":
                result.update(status="failed", error="Worker exited unsuccessfully after final output")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, default=Path("src-tauri/bundle/r2t2/blabber-r2t2-worker"))
    parser.add_argument("--model", type=Path, default=Path("src-tauri/target/r2t2-model/r2t2-q8_0.gguf"))
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--language", default="English")
    parser.add_argument("--chunks", default="320,640,1280,2000")
    parser.add_argument("--rollbacks", default="1,5")
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument("--realtime", action="store_true")
    args = parser.parse_args()
    manifest = json.loads(Path(__file__).with_name("manifest.json").read_text())
    if args.repeat < 1:
        parser.error("repeat must be positive")
    if args.model.stat().st_size != manifest["modelBytes"] or digest(args.model) != manifest["modelSha256"]:
        raise ValueError("Unverified model file")
    samples = read_pcm(args.audio) * args.repeat
    if len(samples) > 300 * 16000:
        parser.error("fixture exceeds the five-minute limit")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = dict(manifest=manifest, workerSha256=digest(args.worker), audioSha256=digest(args.audio),
                  repeat=args.repeat, paced=args.realtime, results=[],
                  limitations="Fixture probe only; not German/English release acceptance or measured acoustic word latency.")
    for chunk in map(int, args.chunks.split(",")):
        for rollback in map(int, args.rollbacks.split(",")):
            result = probe(args, samples, chunk, rollback)
            report["results"].append(result)
            report["childrenPeakRssBytes"] = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
            args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
            print(json.dumps({k:v for k,v in result.items() if k not in ("text", "events")}), flush=True)
    return int(any(r["status"] != "completed" for r in report["results"]))


if __name__ == "__main__":
    raise SystemExit(main())
