import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

type OverlayPhase =
  | "hidden"
  | "listening"
  | "processing"
  | "inserted"
  | "clipboard_only"
  | "failed";

interface OverlayPayload {
  phase: OverlayPhase;
  audioLevel: number;
}

const RESULT_PHASES = new Set<OverlayPhase>([
  "inserted",
  "clipboard_only",
  "failed",
]);

const OVERLAY_EVENT = "quick-dictation-overlay";
const POLL_INTERVAL_MS = 50;
const ANIMATION_INTERVAL_MS = 42;

function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function OverlayApp() {
  const [phase, setPhase] = useState<OverlayPhase>("hidden");
  const [targetLevel, setTargetLevel] = useState(0);
  const [displayLevel, setDisplayLevel] = useState(0);
  const [speechPulse, setSpeechPulse] = useState(0);
  const [wavePhase, setWavePhase] = useState(0);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let mounted = true;
    let unlisten: (() => void) | undefined;
    let pollId = 0;

    const syncOverlayStatus = async () => {
      try {
        const payload = await invoke<OverlayPayload>("get_dictation_overlay_status");
        if (!mounted) {
          return;
        }
        setPhase(payload.phase);
        setTargetLevel(payload.audioLevel ?? 0);
      } catch {
        // Ignore polling failures; the next tick can recover.
      }
    };

    void syncOverlayStatus();
    pollId = window.setInterval(() => {
      void syncOverlayStatus();
    }, POLL_INTERVAL_MS);

    void listen<OverlayPayload>(OVERLAY_EVENT, (event) => {
      if (!mounted) {
        return;
      }
      setPhase(event.payload.phase);
      setTargetLevel(event.payload.audioLevel ?? 0);
    }).then((cleanup) => {
      unlisten = cleanup;
    });

    return () => {
      mounted = false;
      window.clearInterval(pollId);
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const intervalId = window.setInterval(() => {
      setDisplayLevel((current) => {
        const nextTarget = phase === "listening" ? targetLevel : 0;
        const factor = nextTarget > current ? 0.56 : 0.22;
        return current + (nextTarget - current) * factor;
      });
      setSpeechPulse((current) => {
        const rise = Math.max(0, targetLevel - displayLevel) * 2.6;
        const decayed = current * 0.74;
        return phase === "listening" ? Math.max(decayed, rise) : decayed * 0.4;
      });
      setWavePhase((current) => {
        const velocity = phase === "listening" ? 0.06 + targetLevel * 0.14 : 0.04;
        return current + velocity;
      });
    }, ANIMATION_INTERVAL_MS);

    return () => window.clearInterval(intervalId);
  }, [phase, targetLevel]);

  const baseHeights = [8, 10, 14, 10, 8];
  const bodyMultipliers = [16, 24, 34, 24, 16];
  const pulseMultipliers = [6, 10, 14, 10, 6];
  const normalizedLevel = phase === "listening" ? Math.min(1, displayLevel * 2.8) : 0;
  const pulseLevel = phase === "listening" ? Math.min(1, speechPulse) : 0;
  const responsiveLevel = Math.pow(normalizedLevel, 0.62);

  const isResult = RESULT_PHASES.has(phase);
  const resultLabel =
    phase === "inserted"
      ? "Pasted"
      : phase === "clipboard_only"
        ? "Copied — Ctrl+V"
        : phase === "failed"
          ? "Failed"
          : "";
  const resultColor =
    phase === "inserted"
      ? "#4b9d77"
      : phase === "clipboard_only"
        ? "#5d96dd"
        : phase === "failed"
          ? "#c36157"
          : undefined;

  return (
    <div className={phase === "hidden" ? "overlay-root is-hidden" : "overlay-root"}>
      <div
        className={
          phase === "processing"
            ? "overlay-capsule is-processing"
            : isResult
              ? "overlay-capsule is-result"
              : "overlay-capsule"
        }
        aria-label={
          phase === "listening"
            ? "Blabber is listening"
            : phase === "processing"
              ? "Blabber is transcribing"
              : phase === "inserted"
                ? "Pasted"
                : phase === "clipboard_only"
                  ? "Copied to clipboard, press Ctrl+V"
                  : phase === "failed"
                    ? "Dictation failed"
                    : "Blabber overlay"
        }
      >
        {phase === "processing" ? (
          <div className="overlay-spinner" aria-hidden="true" />
        ) : isResult ? (
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 8,
              fontSize: "0.92rem",
              fontWeight: 700,
              letterSpacing: "-0.01em",
              color: resultColor,
              whiteSpace: "nowrap",
            }}
          >
            <span
              aria-hidden="true"
              style={{
                width: 8,
                height: 8,
                borderRadius: 999,
                background: resultColor,
              }}
            />
            {resultLabel}
          </span>
        ) : (
          <div className="overlay-bars" aria-hidden="true">
            {baseHeights.map((baseHeight, index) => {
              const ambientMotion =
                Math.sin(wavePhase + index * 0.85) * (0.45 + responsiveLevel * 1.4);
              const height = Math.max(
                8,
                baseHeight +
                  responsiveLevel * bodyMultipliers[index] +
                  pulseLevel * pulseMultipliers[index] +
                  ambientMotion,
              );
              return (
                <span
                  key={index}
                  className="overlay-bar"
                  style={{
                    height: `${height}px`,
                  }}
                />
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
