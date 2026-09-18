#!/usr/bin/env python3
"""Run the fixed offline corpus. Automated checks do not replace human review."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument("--worker", required=True, type=Path)
parser.add_argument("--model", required=True, type=Path)
parser.add_argument("--output", required=True, type=Path)
parser.add_argument("--limit", type=int)
parser.add_argument("--resume", action="store_true")
parser.add_argument("--match", help="Only case IDs matching this regular expression")
args = parser.parse_args()
with args.worker.open("rb") as worker:
    worker_sha = hashlib.file_digest(worker, "sha256").hexdigest()
expected = "c30995b3c145e6ef3b3a6fda63749186d83d9c8f16725ff1403c904b5e0ead8c"
with args.model.open("rb") as model:
    if hashlib.file_digest(model, "sha256").hexdigest() != expected:
        raise SystemExit("Model checksum mismatch")
cases = json.loads(Path(__file__).with_name("acceptance-corpus.json").read_text())
args.output.parent.mkdir(parents=True, exist_ok=True)
done = {}
if args.resume and args.output.exists():
    for row in map(json.loads, args.output.read_text().splitlines()):
        if row.get("workerSha256") == worker_sha and row.get("promptVersion") == 4 and row.get("modelSha256") == expected and not row.get("automatedFailures"):
            done[row["id"]] = row
    # Do not combine old helper/prompt results with a new benchmark silently.
    args.output.write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in done.values()))
count = 0
with args.output.open("a" if args.resume else "w") as report:
    for case in cases:
        for source in ("de", "en"):
            for target in ("fr", "es-AR"):
                case_id = f"{case['id']}-{source}-{target}"
                if args.match and not re.search(args.match, case_id):
                    continue
                if case_id in done:
                    continue
                if args.limit is not None and count >= args.limit:
                    raise SystemExit(0)
                request = {"version": 1, "requestId": case_id, "modelPath": str(args.model.resolve()),
                           "text": case[source], "sourceLanguages": [source], "targetLanguage": target, "preferGpu": True}
                started = time.monotonic()
                proc = subprocess.Popen(["/usr/bin/time", "-l", str(args.worker.resolve())], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                        start_new_session=True)
                timed_out = False
                try:
                    stdout, metrics = proc.communicate(json.dumps(request) + "\n", timeout=600)
                except subprocess.TimeoutExpired:
                    timed_out = True
                    os.killpg(proc.pid, signal.SIGKILL)
                    stdout, metrics = proc.communicate()
                try:
                    records = [json.loads(line) for line in stdout.splitlines()]
                except json.JSONDecodeError:
                    records = []
                result = next((r for r in records if r.get("type") == "result"), {})
                ready = next((r for r in records if r.get("type") == "ready"), {})
                text = result.get("text", "")
                failures = []
                if timed_out or proc.returncode != 0 or not result.get("completed") or not text.strip():
                    failures.append("incomplete")
                if ready.get("promptVersion") != 4:
                    failures.append("prompt_version_mismatch")
                for literal in case.get("preserve", []):
                    if literal not in text:
                        failures.append("missing_literal:" + literal)
                if (ready.get("chunkCount") or 0) < case.get("minimumChunks", 1):
                    failures.append("chunking_not_exercised")
                if "paragraphs" in case and len(re.split(r"\n\s*\n", text.strip())) != case["paragraphs"]:
                    failures.append("paragraph_count_changed")
                if re.match(r"^(Translation|Here is the translation|Traduction|Traducción)\s*:", text, re.I):
                    failures.append("unrequested_preface")
                rss = re.search(r"(\d+)\s+maximum resident set size", metrics)
                row = {"id": case_id, "category": case["category"], "source": case[source],
                       "targetLanguage": target, "output": text, "exitCode": proc.returncode,
                       "elapsedSeconds": round(time.monotonic() - started, 3),
                       "loadMs": ready.get("loadMs"), "chunkCount": ready.get("chunkCount"),
                       "maxRssBytes": int(rss[1]) if rss else None, "automatedFailures": failures,
                       "promptVersion": ready.get("promptVersion"), "modelSha256": expected, "workerSha256": worker_sha,
                       "humanReview": {"meaningCorrect": None, "toneCorrect": None,
                                       "argentinianUsageCorrect": None, "criticalError": None, "accepted": None}}
                report.write(json.dumps(row, ensure_ascii=False) + "\n")
                report.flush()
                count += 1
                print(f"{case_id}: {row['elapsedSeconds']}s; checks={'PASS' if not failures else failures}", flush=True)
