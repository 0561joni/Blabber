#!/usr/bin/env python3
"""Opt-in real-model short-clip, warm-session and lifecycle probes (no app state)."""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time
import uuid

from evaluate import digest, read_pcm
from compare_baseline import wer


class Client:
    def __init__(self, worker, stderr):
        self.process = subprocess.Popen([str(worker.resolve())], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=stderr, text=True, start_new_session=True,
            env={**os.environ, "BLABBER_R2T2_PROBE": "1"})
        self.incoming = queue.Queue()
        def read():
            try:
                for line in self.process.stdout:
                    self.incoming.put(json.loads(line))
            except Exception as error:
                self.incoming.put(error)
            finally:
                self.incoming.put(None)
        threading.Thread(target=read, daemon=True).start()

    def send(self, kind, **fields):
        if kind == "start":
            self.sid, self.sequence, self.received = str(uuid.uuid4()), 0, 0
        message = dict(version=1, sessionId=self.sid, sequence=self.sequence, type=kind, **fields)
        self.sequence += 1
        self.process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.process.stdin.flush()
        response = self.incoming.get(timeout=90)
        if not isinstance(response, dict) or (response.get("version"), response.get("sessionId"),
            response.get("sequence")) != (1, self.sid, self.received + 1):
            raise RuntimeError("Invalid, missing or stale worker message")
        self.received += 1
        if response["type"] == "error":
            raise RuntimeError(response["code"])
        expected = {"start": "ready", "audio": "progress", "finish": "result", "cancel": "canceled", "reset": "canceled"}[kind]
        if response["type"] != expected:
            raise RuntimeError("Unexpected worker message type")
        return response

    def close(self):
        try:
            self.process.stdin.close()
            self.process.wait(timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            if self.process.poll() is None:
                os.killpg(self.process.pid, 9)
            self.process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, default=Path("src-tauri/bundle/r2t2/blabber-r2t2-worker"))
    parser.add_argument("--model", type=Path, default=Path("src-tauri/target/r2t2-model/r2t2-q8_0.gguf"))
    parser.add_argument("--fixtures", type=Path, default=Path("src-tauri/target/r2t2-fixtures"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads(Path(__file__).with_name("manifest.json").read_text())
    if args.model.stat().st_size != manifest["modelBytes"] or digest(args.model) != manifest["modelSha256"]:
        raise ValueError("Unverified model file")
    report = dict(workerSha256=digest(args.worker), synthetic=True, results=[],
                  limitations="Local native probes only; not packaged-app or human-recording acceptance.")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    def checkpoint():
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    with args.output.with_suffix(".stderr.log").open("w") as log:
        client = Client(args.worker, log)
        try:
            cases = [(f"{lang}-{n:02}", language) for lang, language in [("de", "German"), ("en", "English")] for n in range(1, 21)]
            for name, language in cases:
                samples = read_pcm(args.fixtures / f"{name}.wav")
                expected = (args.fixtures / f"{name}.txt").read_text().strip()
                began = time.monotonic()
                client.send("start", modelPath=str(args.model.resolve()), language=language, chunkMs=640, rollbackTokens=5)
                load = time.monotonic() - began
                previous = ""
                for start in range(0, len(samples), 10240):
                    text = client.send("audio", startSample=start, samples=samples[start:start + 10240])["text"]
                    if not text.startswith(previous):
                        raise RuntimeError("Committed text changed")
                    previous = text
                text = client.send("finish", totalSamples=len(samples))["text"]
                if not text.startswith(previous):
                    raise RuntimeError("Final text changed")
                report["results"].append(dict(fixture=name, expected=expected, text=text, wer=wer(expected, text),
                    loadSeconds=load, totalSeconds=time.monotonic() - began))
                checkpoint()

            # No audio, sub-chunk input and an exact multiple of the decode chunk.
            for count in (0, 1, 10240, 512000):
                client.send("start", modelPath=str(args.model.resolve()), language="", chunkMs=640, rollbackTokens=5)
                for start in range(0, count, 10240):
                    client.send("audio", startSample=start, samples=[0.0] * min(10240, count - start))
                text = client.send("finish", totalSamples=count)["text"]
                if text.strip():
                    raise RuntimeError(f"Silence produced text ({count} samples)")
                report["results"].append(dict(silenceSamples=count, status="passed"))
                checkpoint()

            for action in ("cancel", "reset"):
                for _ in range(3):
                    client.send("start", modelPath=str(args.model.resolve()), language="German", chunkMs=640, rollbackTokens=5)
                    client.send("audio", startSample=0, samples=read_pcm(args.fixtures / "de-01.wav")[:10240])
                    client.send(action)
                report["results"].append(dict(action=action, repetitions=3, status="passed"))
                checkpoint()
            report["status"] = "passed"
        except Exception as error:
            report.update(status="failed", error=str(error) or type(error).__name__)
        finally:
            client.close()
            report["exitCode"] = client.process.returncode
            checkpoint()
    print(json.dumps({k:v for k,v in report.items() if k != "results"}), flush=True)
    return int(report["status"] != "passed")


if __name__ == "__main__":
    raise SystemExit(main())
