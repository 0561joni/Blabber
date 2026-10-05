#!/usr/bin/env python3
"""Side-by-side live-dictation benchmark: live pair (fluid) vs R2T2.

Both engines get identical audio, identical real-time pacing and identical
text normalisation (case, punctuation, umlauts, numbers, abbreviations). No
network, microphone, paste or app database.

Corpus: a directory of `<lang>-<id>.wav` (16 kHz mono PCM16) with matching
`.txt` references, where `<lang>` is de, en or mixed. Human recordings belong in
`_private/asr-corpus/` (git-ignored). By default only aggregate numbers are
written; `--include-text` adds transcripts and must only target `_private/`.
"""
import argparse
import array
import json
import os
from pathlib import Path
import queue
import re
import statistics
import subprocess
import sys
import threading
import time
import unicodedata
import uuid
import wave

ROOT = Path(__file__).resolve().parents[2]
SAMPLE_RATE = 16000

# MARK: Normalisation

ABBREVIATIONS = {
    "dr": "doctor", "doktor": "doctor", "mr": "mister", "mrs": "missus", "st": "saint",
    "z b": "zum beispiel", "bzw": "beziehungsweise", "usw": "und so weiter", "ca": "circa",
    "nr": "nummer", "eur": "euro", "€": "euro", "%": "prozent", "km": "kilometer",
    "m²": "quadratmeter", "qm": "quadratmeter",
}
DE_UNITS = ["null", "eins", "zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun",
            "zehn", "elf", "zwölf", "dreizehn", "vierzehn", "fünfzehn", "sechzehn", "siebzehn",
            "achtzehn", "neunzehn"]
DE_TENS = ["", "", "zwanzig", "dreißig", "vierzig", "fünfzig", "sechzig", "siebzig", "achtzig", "neunzig"]
EN_UNITS = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
            "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen",
            "eighteen", "nineteen"]
EN_TENS = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]


def german_number(n):
    if n < 20:
        return DE_UNITS[n]
    if n < 100:
        unit = "ein" if n % 10 == 1 else DE_UNITS[n % 10]
        return (unit + "und" if n % 10 else "") + DE_TENS[n // 10]
    if n < 1000:
        head = "ein" if n // 100 == 1 else DE_UNITS[n // 100]
        return head + "hundert" + (german_number(n % 100) if n % 100 else "")
    if n < 1_000_000:
        head = "ein" if n // 1000 == 1 else german_number(n // 1000)
        return head + "tausend" + (german_number(n % 1000) if n % 1000 else "")
    return str(n)


def english_number(n):
    if n < 20:
        return EN_UNITS[n]
    if n < 100:
        return EN_TENS[n // 10] + (" " + EN_UNITS[n % 10] if n % 10 else "")
    if n < 1000:
        return EN_UNITS[n // 100] + " hundred" + (" " + english_number(n % 100) if n % 100 else "")
    if n < 1_000_000:
        return english_number(n // 1000) + " thousand" + (" " + english_number(n % 1000) if n % 1000 else "")
    return str(n)


EN_ORDINALS = {"one": "first", "two": "second", "three": "third", "five": "fifth", "eight": "eighth",
               "nine": "ninth", "twelve": "twelfth"}


def english_ordinal(n):
    words = english_number(n).split()
    last = words[-1]
    if last in EN_ORDINALS:
        words[-1] = EN_ORDINALS[last]
    elif last.endswith("y"):
        words[-1] = last[:-1] + "ieth"
    else:
        words[-1] = last + "th"
    return " ".join(words)


def german_ordinal(n):
    special = {1: "erste", 3: "dritte", 7: "siebte", 8: "achte"}
    if n in special:
        return special[n]
    return german_number(n) + ("ste" if n >= 20 else "te")


def english_year(n):
    if 1100 <= n <= 2099 and n % 100 and not (2000 <= n < 2010):
        return english_number(n // 100) + " " + english_number(n % 100)
    return english_number(n)


def normalise(text, language):
    """Written and spoken forms of times, dates and numbers score alike."""
    text = unicodedata.normalize("NFC", text).casefold()
    for symbol in ("€", "%", "m²"):
        text = text.replace(symbol, " " + ABBREVIATIONS[symbol] + " ")
    if language == "de":
        # "9.30 Uhr" / "9:30" is spoken "neun Uhr dreißig".
        text = re.sub(r"\b(\d{1,2})[:.](\d{2})(\s*uhr)?\b",
                      lambda m: f" {german_number(int(m.group(1)))} uhr "
                      + (german_number(int(m.group(2))) if int(m.group(2)) else ""), text)
        text = re.sub(r"\b(\d{1,2})\.(?=\s+[^\W\d_])", lambda m: " " + german_ordinal(int(m.group(1))) + " ", text)
        text = re.sub(r"\d+", lambda m: " " + german_number(int(m.group())) + " ", text)
    else:
        text = re.sub(r"\b(\d{1,2})[:.](\d{2})\b",
                      lambda m: f" {english_number(int(m.group(1)))} "
                      + (english_number(int(m.group(2))) if int(m.group(2)) else "o'clock"), text)
        text = re.sub(r"\b(\d+)(st|nd|rd|th)\b", lambda m: " " + english_ordinal(int(m.group(1))) + " ", text)
        text = re.sub(r"\b(1[1-9]\d\d|20\d\d)\b", lambda m: " " + english_year(int(m.group())) + " ", text)
        text = re.sub(r"\d+", lambda m: " " + english_number(int(m.group())) + " ", text)
    words = re.findall(r"[^\W_]+", text)
    joined = " " + " ".join(words) + " "
    for short, long in ABBREVIATIONS.items():
        joined = joined.replace(" " + short + " ", " " + long + " ")
    folded = (joined.replace("ä", "ae").replace("ö", "oe").replace("ü", "ue").replace("ß", "ss"))
    # Filler "and" in English numbers ("two hundred and five") is optional.
    return [w for w in folded.split() if not (language == "en" and w == "and")]


def errors(reference, hypothesis):
    costs = list(range(len(hypothesis) + 1))
    for i, a in enumerate(reference, 1):
        row = [i]
        for j, b in enumerate(hypothesis, 1):
            row.append(min(costs[j] + 1, row[-1] + 1, costs[j - 1] + (a != b)))
        costs = row
    return costs[-1]


# MARK: Engines

def read_pcm(path):
    with wave.open(str(path), "rb") as wav:
        if (wav.getframerate(), wav.getnchannels(), wav.getsampwidth()) != (SAMPLE_RATE, 1, 2):
            raise ValueError(f"{path}: fixture must be 16 kHz mono PCM16 WAV")
        samples = array.array("h", wav.readframes(wav.getnframes()))
    return [sample / 32768.0 for sample in samples]


class Worker:
    """One helper process with strict request/response ordering."""

    def __init__(self, command, stderr):
        self.process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=stderr, text=True, start_new_session=True)
        self.incoming = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        try:
            for line in self.process.stdout:
                self.incoming.put(json.loads(line))
        except Exception as exc:  # noqa: BLE001 - surfaced to the caller
            self.incoming.put(exc)
        finally:
            self.incoming.put(None)

    def send(self, session, sequence, kind, timeout=180, **fields):
        message = dict(version=1, sessionId=session, sequence=sequence, type=kind, **fields)
        self.process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
        self.process.stdin.flush()
        response = self.incoming.get(timeout=timeout)
        if not isinstance(response, dict):
            raise RuntimeError(f"worker ended: {response}")
        if response.get("type") == "error":
            raise RuntimeError(response.get("code"))
        return response

    def close(self):
        try:
            self.process.stdin.close()
            self.process.wait(timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            os.killpg(self.process.pid, 9)
            self.process.wait()

    def cpu_seconds(self):
        out = subprocess.run(["ps", "-o", "time=", "-p", str(self.process.pid)], capture_output=True, text=True).stdout.strip()
        if not out:
            return None
        parts = [float(p) for p in out.replace("-", ":").split(":")]
        seconds = 0.0
        for part in parts:
            seconds = seconds * 60 + part
        return seconds


def wired_bytes():
    out = subprocess.run(["vm_stat"], capture_output=True, text=True).stdout
    page = int(re.search(r"page size of (\d+)", out).group(1))
    wired = int(re.search(r"Pages wired down:\s+(\d+)", out).group(1))
    swapouts = int(re.search(r"Swapouts:\s+(\d+)", out).group(1))
    return wired * page, swapouts


class Fluid:
    name = "fluid"

    def __init__(self, args, log):
        self.chunk_ms = args.fluid_chunk_ms
        self.wired_before, _ = wired_bytes()
        began = time.monotonic()
        self.worker = Worker([str(args.fluid_worker.resolve())], log)
        models = args.fluid_models.resolve()
        ready = self.worker.send("ctl-load", 0, "load", nemotronPath=str(models / f"nemotron/latin/{self.chunk_ms}ms"),
                                 parakeetPath=str(models / "parakeet"))
        self.worker.send("ctl-warm", 0, "warmup")
        self.load_seconds = time.monotonic() - began
        self.wired_after, _ = wired_bytes()
        self.rss = [ready["rssBytes"]]

    def run(self, samples, language, paced):
        session = str(uuid.uuid4())
        hint = {"de": "de", "en": "en"}.get(language, "auto") if language != "auto" else "auto"
        self.worker.send(session, 0, "start", language=hint, chunkMs=self.chunk_ms)
        chunk = self.chunk_ms * 16
        events, sequence, began = [], 1, time.monotonic()
        for start in range(0, len(samples), chunk):
            end = min(len(samples), start + chunk)
            if paced:
                time.sleep(max(0, began + end / SAMPLE_RATE - time.monotonic()))
            reply = self.worker.send(session, sequence, "audio", startSample=start, samples=samples[start:end])
            sequence += 1
            events.append((time.monotonic() - began, reply["committedText"] + reply["tentativeText"]))
            self.rss.append(reply["rssBytes"])
        stop = time.monotonic()
        final = self.worker.send(session, sequence, "finish", expectedSamples=len(samples))
        release = time.monotonic() - stop
        self.rss.append(final["rssBytes"])
        return dict(stream=final["streamText"], final=final["finalText"], events=events,
                    releaseSeconds=release, finalError=final["finalError"])

    def memory(self):
        return dict(processPeakBytes=max(self.rss), processSteadyBytes=self.rss[-1],
                    systemWiredDeltaBytes=self.wired_after - self.wired_before)

    def idle_cpu(self, seconds):
        before = self.worker.cpu_seconds()
        time.sleep(seconds)
        after = self.worker.cpu_seconds()
        return None if before is None or after is None else (after - before) / seconds * 100


class R2T2:
    name = "r2t2"

    def __init__(self, args, log):
        self.args, self.log, self.rss = args, log, []
        self.worker = Worker([str(args.r2t2_worker.resolve())], log)
        self.load_seconds = None

    def run(self, samples, language, paced):
        session = str(uuid.uuid4())
        canonical = {"de": "German", "en": "English"}.get(language, "")
        began = time.monotonic()
        self.worker.send(session, 0, "start", modelPath=str(self.args.r2t2_model.resolve()), language=canonical,
                         chunkMs=640, rollbackTokens=5)
        if self.load_seconds is None:
            self.load_seconds = time.monotonic() - began
        chunk, events, sequence, began = 640 * 16, [], 1, time.monotonic()
        for start in range(0, len(samples), chunk):
            end = min(len(samples), start + chunk)
            if paced:
                time.sleep(max(0, began + end / SAMPLE_RATE - time.monotonic()))
            reply = self.worker.send(session, sequence, "audio", startSample=start, samples=samples[start:end])
            sequence += 1
            events.append((time.monotonic() - began, reply["text"] + reply.get("tentativeText", "")))
            if reply.get("peakRssBytes"):
                self.rss.append(reply["peakRssBytes"])
        stop = time.monotonic()
        result = self.worker.send(session, sequence, "finish", totalSamples=len(samples))
        release = time.monotonic() - stop
        # R2T2's stream and final text are the same rolling transcript.
        return dict(stream=result["text"], final=result["text"], events=events, releaseSeconds=release, finalError="")

    def memory(self):
        return dict(processPeakBytes=max(self.rss) if self.rss else None)

    def idle_cpu(self, seconds):
        return None


# MARK: Metrics

def word_delays(events, reference_words, audio_seconds):
    """Estimated delay from a word being spoken to it being visible. Without
    forced alignment, word end times are spread over the clip by character
    count; reported as an estimate."""
    if not reference_words:
        return []
    total = sum(len(w) + 1 for w in reference_words)
    ends, cursor = [], 0
    for word in reference_words:
        cursor += len(word) + 1
        ends.append(audio_seconds * cursor / total)
    delays, shown = [], 0
    for at, text in events:
        visible = len(text.split())
        while shown < min(visible, len(ends)):
            delays.append(max(0.0, at - ends[shown]))
            shown += 1
    return delays


def p95(values):
    if not values:
        return None
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(round(0.95 * (len(ordered) - 1))))]


def summarise(results, engine):
    out = {}
    for language in sorted({r["language"] for r in results}):
        rows = [r for r in results if r["language"] == language]
        words = sum(r["words"] for r in rows)
        out[language] = dict(
            clips=len(rows),
            werStream=round(100 * sum(r["streamErrors"] for r in rows) / max(1, words), 2),
            werFinal=round(100 * sum(r["finalErrors"] for r in rows) / max(1, words), 2),
        )
    short = [r for r in results if r["audioSeconds"] <= 60]
    first = [r["firstWordSeconds"] for r in results if r["firstWordSeconds"] is not None]
    delays = [d for r in results for d in r["delays"]]
    releases = [r["releaseSeconds"] for r in short]
    out["latency"] = dict(
        firstVisibleWordSecondsMedian=round(statistics.median(first), 3) if first else None,
        firstVisibleWordSecondsP95=round(p95(first), 3) if first else None,
        estimatedWordDelaySecondsMedian=round(statistics.median(delays), 3) if delays else None,
        estimatedWordDelaySecondsP95=round(p95(delays), 3) if delays else None,
        releaseToTextSecondsMedianUpTo60s=round(statistics.median(releases), 3) if releases else None,
        releaseToTextSecondsP95UpTo60s=round(p95(releases), 3) if releases else None,
        finalPassFailures=sum(1 for r in results if r["finalError"]),
    )
    out["memory"] = engine.memory()
    out["loadSeconds"] = round(engine.load_seconds, 3) if engine.load_seconds else None
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, action="append",
                        help="clip directory; repeatable (default: synthetic fixtures)")
    parser.add_argument("--engines", default="fluid,r2t2")
    parser.add_argument("--language-mode", choices=["auto", "fixed"], default="auto")
    parser.add_argument("--paced", action="store_true", help="feed audio in real time (needed for latency)")
    parser.add_argument("--limit", type=int, default=0, help="clips per language (0: all)")
    parser.add_argument("--idle-seconds", type=int, default=60)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--include-text", action="store_true")
    parser.add_argument("--fluid-worker", type=Path, default=ROOT / "src-tauri/bundle/fluid/blabber-fluid-worker")
    parser.add_argument("--fluid-models", type=Path, default=ROOT / "src-tauri/target/fluid-models")
    parser.add_argument("--fluid-chunk-ms", type=int, default=560)
    parser.add_argument("--r2t2-worker", type=Path, default=ROOT / "src-tauri/bundle/r2t2/blabber-r2t2-worker")
    parser.add_argument("--r2t2-model", type=Path, default=ROOT / "src-tauri/target/r2t2-model/r2t2-q8_0.gguf")
    args = parser.parse_args()
    if args.include_text and "_private" not in args.output.resolve().parts:
        parser.error("--include-text reports must be written under _private/")
    corpora = args.corpus or [ROOT / "src-tauri/target/r2t2-fixtures"]
    clips = []
    for corpus in corpora:
        by_language = {}
        for wav in sorted(corpus.glob("*.wav")):
            language = wav.stem.split("-")[0]
            text = wav.with_suffix(".txt")
            if language in ("de", "en", "mixed") and text.exists() and "synthetic" not in wav.stem:
                by_language.setdefault(language, []).append((wav, text.read_text().strip()))
        for language, items in by_language.items():
            clips += [(language, wav, ref) for wav, ref in (items[: args.limit] if args.limit else items)]
    if not clips:
        parser.error("no clips found")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = dict(date=time.strftime("%Y-%m-%d"), paced=args.paced, languageMode=args.language_mode,
                  corpora=[str(c.relative_to(ROOT)) if c.is_relative_to(ROOT) else c.name for c in corpora],
                  clipCount=len(clips), normalisation="NFC casefold; umlauts folded; digits, times, ordinals and English years spelled per language; common abbreviations expanded",
                  engines={})
    for name in args.engines.split(","):
        log = open(args.output.with_suffix(f".{name}.stderr.log"), "w")
        engine = (Fluid if name == "fluid" else R2T2)(args, log)
        results = []
        for language, wav, reference in clips:
            samples = read_pcm(wav)
            hint = language if args.language_mode == "fixed" and language in ("de", "en") else "auto"
            run = engine.run(samples, hint, args.paced)
            norm_language = "de" if language == "mixed" else language
            ref_words = normalise(reference, norm_language)
            first = next((at for at, text in run["events"] if text.strip()), None)
            row = dict(language=language, clip=wav.stem, audioSeconds=len(samples) / SAMPLE_RATE,
                       words=len(ref_words),
                       streamErrors=errors(ref_words, normalise(run["stream"], norm_language)),
                       finalErrors=errors(ref_words, normalise(run["final"], norm_language)),
                       firstWordSeconds=first if args.paced else None,
                       delays=word_delays(run["events"], reference.split(), len(samples) / SAMPLE_RATE) if args.paced else [],
                       releaseSeconds=run["releaseSeconds"], finalError=run["finalError"])
            if args.include_text:
                row.update(reference=reference, stream=run["stream"], final=run["final"])
            results.append(row)
            print(f"[{name}] {wav.stem}: final {row['finalErrors']}/{row['words']} release {row['releaseSeconds']:.2f}s",
                  file=sys.stderr, flush=True)
        summary = summarise(results, engine)
        summary["idleCpuPercent"] = engine.idle_cpu(args.idle_seconds) if args.idle_seconds else None
        if args.include_text:
            summary["clips"] = results
        report["engines"][name] = summary
        engine.worker.close()
        log.close()
        args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    _, swapouts = wired_bytes()
    report["systemSwapoutsAtEnd"] = swapouts
    report["notMeasured"] = [
        "GPU/ANE idle power: run `sudo powermetrics --samplers cpu_power,gpu_power,ane_power -i 1000 -n 60` with the app idle",
        "Memory with TranslateGemma: dictate in a translation mode in the app and compare `vm_stat` swapouts before/after",
    ]
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({k: v for k, v in report["engines"].items()}, indent=1))


if __name__ == "__main__":
    raise SystemExit(main())
