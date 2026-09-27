#!/usr/bin/env python3
"""Compare explicit evaluation transcripts to a local Whisper worker baseline."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import time
import unicodedata


def words(text):
    return re.findall(r"[^\W_]+", unicodedata.normalize("NFC", text).casefold())


def wer(expected, actual):
    ref, hyp = words(expected), words(actual)
    costs = list(range(len(hyp) + 1))
    for i, a in enumerate(ref, 1):
        next_costs = [i]
        for j, b in enumerate(hyp, 1):
            next_costs.append(min(costs[j] + 1, next_costs[-1] + 1, costs[j - 1] + (a != b)))
        costs = next_costs
    return costs[-1] / max(1, len(ref))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--worker", type=Path, default=Path("src-tauri/target/release/speech-to-text"))
    p.add_argument("--models-dir", type=Path, required=True)
    p.add_argument("--model-id", required=True)
    p.add_argument("--audio", type=Path, required=True)
    p.add_argument("--expected", type=Path, required=True)
    p.add_argument("--r2t2-report", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    request = dict(modelsDir=str(args.models_dir.resolve()), request=dict(
        useContext="shortcut_dictation", profile="accurate", selectedModelId=args.model_id,
        languageMode="auto", fixedLanguage=None, timestamps=False, preferGpu=True,
        filePath=str(args.audio.resolve()), contextPrompt=None, contextTerms=[]))
    began = time.monotonic()
    with args.output.with_suffix(".stderr.log").open("w") as err:
        completed = subprocess.run([str(args.worker.resolve()), "--transcribe-worker"],
                                   input=json.dumps(request), text=True, stdout=subprocess.PIPE,
                                   stderr=err, timeout=300)
    records = [json.loads(line) for line in completed.stdout.splitlines()]
    result = next((r["result"] for r in records if r["type"] == "result"), None)
    if completed.returncode or result is None:
        raise RuntimeError("Baseline worker failed; inspect its explicit evaluation log")
    reference = args.expected.read_text()
    baseline = dict(modelId=args.model_id, text=result["plainText"], seconds=time.monotonic() - began,
                    wer=wer(reference, result["plainText"]))
    candidates = json.loads(args.r2t2_report.read_text())["results"]
    comparisons = []
    for candidate in candidates:
        row = {k:v for k,v in candidate.items() if k not in ("text", "events")}
        if candidate["status"] == "completed":
            row["wer"] = wer(reference, candidate["text"])
            row["werDifferencePercentagePoints"] = (row["wer"] - baseline["wer"]) * 100
            row["withinToleranceOnThisFixture"] = row["werDifferencePercentagePoints"] <= 2
        comparisons.append(row)
    args.output.write_text(json.dumps(dict(baseline=baseline, candidates=comparisons,
        limitations="Synthetic speech probe; requires separate human recordings before release."),
        ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(dict(baselineWer=baseline["wer"], candidates=comparisons)), flush=True)


if __name__ == "__main__":
    main()
