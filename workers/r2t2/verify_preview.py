#!/usr/bin/env python3
"""Local parity/preview probe. Reports hashes and timings, never speech text."""
import argparse
import hashlib
import json
from pathlib import Path
import time

from evaluate import digest, read_pcm
from exercise_native import Client


def text_hash(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worker", type=Path, required=True)
    parser.add_argument("--model", type=Path, default=Path("src-tauri/target/r2t2-model/r2t2-q8_0.gguf"))
    parser.add_argument("--fixtures", type=Path, default=Path("src-tauri/target/r2t2-fixtures"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--case", action="append", dest="cases")
    parser.add_argument("--lifecycle-only", action="store_true")
    args = parser.parse_args()
    manifest = json.loads(Path(__file__).with_name("manifest.json").read_text())
    if args.model.stat().st_size != manifest["modelBytes"] or digest(args.model) != manifest["modelSha256"]:
        raise ValueError("Unverified model")
    baseline = json.loads(args.baseline.read_text()) if args.baseline else None
    cases = [(f"{lang}-{n:02}", language) for lang, language in [("de", "German"), ("en", "English")] for n in range(1, 21)]
    cases += [("de-five-minutes", "German"), ("en-five-minutes", "English"), ("mixed-five-minutes", "")]
    if args.cases:
        cases = [(name, language) for name, language in cases if name in args.cases]
        if len(cases) != len(set(args.cases)):
            raise ValueError("Unknown case")
    if args.lifecycle_only:
        cases = []
    report = dict(workerSha256=digest(args.worker), modelSha256=manifest["modelSha256"], cases={}, status="running")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    def save():
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    with args.output.with_suffix(".stderr.log").open("w") as log:
        client = Client(args.worker, log)
        try:
            for name, language in cases:
                samples = read_pcm(args.fixtures / f"{name}.wav")
                started = time.monotonic()
                ready = client.send("start", modelPath=str(args.model.resolve()), language=language,
                                    chunkMs=640, rollbackTokens=5)
                assert not ready["text"] and not ready.get("tentativeText", "")
                row = dict(audioSha256=digest(args.fixtures / f"{name}.wav"), audioSamples=len(samples),
                           loadSeconds=time.monotonic() - started, committedHashes=[],
                           previewUpdates=0, previewOnlyUpdates=0, firstPreviewSample=None, firstCommittedSample=None)
                previous, draft = "", ""
                for start in range(0, len(samples), 10240):
                    chunk = samples[start:start + 10240]
                    response = client.send("audio", startSample=start, samples=chunk)
                    text, tentative = response["text"], response.get("tentativeText", "")
                    assert isinstance(tentative, str) and text.startswith(previous)
                    if len(chunk) < 10240:
                        assert tentative == draft, "A buffered tail must preserve the last preview"
                    if tentative != draft:
                        row["previewUpdates"] += 1
                        row["previewOnlyUpdates"] += int(text == previous)
                    if tentative and row["firstPreviewSample"] is None:
                        row["firstPreviewSample"] = start + len(chunk)
                    if text and row["firstCommittedSample"] is None:
                        row["firstCommittedSample"] = start + len(chunk)
                    row["committedHashes"].append(text_hash(text))
                    previous, draft = text, tentative
                response = client.send("finish", totalSamples=len(samples))
                assert response["text"].startswith(previous) and not response.get("tentativeText", "")
                row.update(finalHash=text_hash(response["text"]), totalSeconds=time.monotonic() - started,
                           peakRssBytes=response.get("peakRssBytes"))
                if baseline:
                    before = baseline["cases"][name]
                    for field in ("audioSha256", "audioSamples", "committedHashes", "finalHash"):
                        assert row[field] == before[field], f"{name}: changed {field}"
                    assert row["previewUpdates"] > 0, f"{name}: packaged helper produced no preview"
                report["cases"][name] = row
                save()
                print(json.dumps(dict(case=name, seconds=round(row["totalSeconds"], 2),
                                      previewUpdates=row["previewUpdates"], parity="passed" if baseline else "baseline")), flush=True)
            if args.lifecycle_only:
                checks = []
                def start(language=""):
                    ready = client.send("start", modelPath=str(args.model.resolve()), language=language,
                                        chunkMs=640, rollbackTokens=5)
                    assert not ready["text"] and ready.get("tentativeText") == ""
                for count in (0, 1, 10240, 512000):
                    start()
                    for offset in range(0, count, 10240):
                        client.send("audio", startSample=offset, samples=[0.0] * min(10240, count - offset))
                    final = client.send("finish", totalSamples=count)
                    assert not final["text"].strip() and final.get("tentativeText") == ""
                    checks.append(dict(silenceSamples=count, status="passed"))
                speech = read_pcm(args.fixtures / "en-01.wav")[:40960]
                for action in ("cancel", "reset"):
                    for _ in range(3):
                        start("English")
                        saw_draft = False
                        for offset in range(0, len(speech), 10240):
                            event = client.send("audio", startSample=offset, samples=speech[offset:offset + 10240])
                            saw_draft |= bool(event.get("tentativeText"))
                        assert saw_draft
                        canceled = client.send(action)
                        assert not canceled["text"] and canceled.get("tentativeText") == ""
                    checks.append(dict(action=action, repetitions=3, status="passed"))
                start("English")
                for offset in range(0, len(speech), 10240):
                    client.send("audio", startSample=offset, samples=speech[offset:offset + 10240])
                # A protocol failure during an active preview must clear it too.
                client.process.stdin.write(json.dumps(dict(version=1, sessionId=client.sid,
                    sequence=client.sequence + 10, type="finish", totalSamples=len(speech))) + "\n")
                client.process.stdin.flush()
                failure = client.incoming.get(timeout=10)
                assert failure["type"] == "error" and failure["code"] == "R2T2_PROTOCOL"
                assert not failure["text"] and failure.get("tentativeText") == ""
                checks.append(dict(action="active-preview-error", status="passed"))
                report["lifecycle"] = checks
                print(json.dumps(dict(lifecycle="passed", checks=len(checks))), flush=True)
            report["status"] = "passed"
        except Exception as error:
            report.update(status="failed", error=str(error) or type(error).__name__)
            raise
        finally:
            client.close()
            save()


if __name__ == "__main__":
    main()
