#!/usr/bin/env python3
"""Generate explicit synthetic probes with installed macOS voices, offline."""
import argparse
import json
from pathlib import Path
import subprocess
import wave

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output-dir", type=Path, default=Path("src-tauri/target/r2t2-fixtures"))
parser.add_argument("--individual", action="store_true", help="Also generate 40 short clips and five-minute continuity fixtures")
args = parser.parse_args()
args.output_dir.mkdir(parents=True, exist_ok=True)
cases = json.loads(Path(__file__).with_name("corpus.json").read_text())["cases"]
for lang, voice in [("de", "Anna"), ("en", "Samantha")]:
    text = " ".join(case[lang] for case in cases)
    source = args.output_dir / f"{lang}-synthetic.txt"
    source.write_text(text + "\n")
    subprocess.run(["say", "-v", voice, "-r", "160", "-f", str(source), "-o",
                    str(args.output_dir / f"{lang}-synthetic.wav"),
                    "--file-format=WAVE", "--data-format=LEI16@16000"], check=True)
    with wave.open(str(args.output_dir / f"{lang}-synthetic.wav"), "rb") as audio:
        if audio.getnframes() < 16000:
            raise RuntimeError("The speech service produced no usable audio; run with access to installed system voices.")

if args.individual:
    clips = {"de": [], "en": []}
    for lang, voice in [("de", "Anna"), ("en", "Samantha")]:
        for case in cases:
            name = f"{lang}-{case['id']}"
            source = args.output_dir / f"{name}.txt"
            source.write_text(case[lang] + "\n")
            path = args.output_dir / f"{name}.wav"
            subprocess.run(["say", "-v", voice, "-r", "160", "-f", str(source), "-o", str(path),
                            "--file-format=WAVE", "--data-format=LEI16@16000"], check=True)
            with wave.open(str(path), "rb") as audio:
                if (audio.getframerate(), audio.getnchannels(), audio.getsampwidth()) != (16000, 1, 2) or audio.getnframes() < 1600:
                    raise RuntimeError(f"Invalid generated fixture: {name}")
                clips[lang].append((case[lang], audio.readframes(audio.getnframes()), name))

    # Join complete utterances, then pad only the short remainder. Their audio
    # boundaries deliberately do not coincide with the decoder's 8-second seams.
    for language in ("de", "en", "mixed"):
        data, expected, segments = bytearray(), [], []
        n = 0
        while True:
            lang = language if language != "mixed" else ("de" if n % 2 == 0 else "en")
            text, audio, name = clips[lang][n % len(cases)]
            if len(data) + len(audio) > 300 * 16000 * 2:
                break
            segments.append(dict(fixture=name, text=text, startSample=len(data) // 2,
                                 endSample=(len(data) + len(audio)) // 2))
            data.extend(audio)
            expected.append(text)
            n += 1
        speech_samples = len(data) // 2
        data.extend(bytes(300 * 16000 * 2 - len(data)))
        with wave.open(str(args.output_dir / f"{language}-five-minutes.wav"), "wb") as audio:
            audio.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            audio.writeframes(data)
        (args.output_dir / f"{language}-five-minutes.txt").write_text(" ".join(expected) + "\n")
        (args.output_dir / f"{language}-five-minutes.json").write_text(json.dumps(
            dict(synthetic=True, speechSamples=speech_samples, totalSamples=300 * 16000, segments=segments),
            ensure_ascii=False, indent=2) + "\n")
