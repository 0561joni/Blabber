import { outputLanguageLabel } from "./lib/translationApi";
import type { OutputMode } from "./types/domain";
import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getHealthCheck } from "./lib/api";
import { formatPasteShortcutForDisplay } from "./lib/formatting";
import { AppIcon } from "./components/IconButton";
import { splitLiveTranscript } from "./lib/liveTranscript";

type OverlayPhase =
  | "mode"
  | "hidden"
  | "listening"
  | "processing"
  | "inserted"
  | "clipboard_only"
  | "failed";
interface OverlayPayload {
  phase: OverlayPhase;
  audioLevel: number;
  outputMode?: OutputMode;
  statusText?: string | null;
  revision?: number;
  sessionId?: string | null;
  liveText?: string;
  tentativeText?: string;
  streamingState?: "preparing" | "waiting" | "listening" | "catching_up" | "finishing" | "translating" | "failed" | null;
  lagMs?: number;
  durationLimitReached?: boolean;
}

export function OverlayApp() {
  const [status, setStatus] = useState<OverlayPayload>({
    phase: "hidden",
    audioLevel: 0,
  });
  const [platform, setPlatform] = useState<string | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const [overflow, setOverflow] = useState(false);
  const committed = status.liveText ?? "";
  const tentative = status.tentativeText ?? "";
  const displayedText = committed + tentative;
  const styledText = useMemo(() => splitLiveTranscript(committed, tentative), [committed, tentative]);
  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    setOverflow(element.scrollWidth > element.clientWidth);
    const reducedMotion = document.documentElement.dataset.motion === "reduced"
      || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    const behavior = reducedMotion ? "auto" : "smooth";
    if (element.scrollTo) element.scrollTo({ left: element.scrollWidth, behavior });
    else element.scrollLeft = element.scrollWidth;
  }, [displayedText, status.sessionId]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let receivedEvent = false;
    let unlisten: (() => void) | undefined;
    void getHealthCheck()
      .then((health) => {
        if (!disposed) setPlatform(health.platform);
      })
      .catch(() => undefined);
    void listen<OverlayPayload>("quick-dictation-overlay", ({ payload }) => {
      receivedEvent = true;
      if (!disposed) setStatus((current) => (payload.revision ?? 0) >= (current.revision ?? 0) ? payload : current);
    })
      .then(async (cleanup) => {
        if (disposed) {
          cleanup();
          return;
        }
        unlisten = cleanup;
        const snapshot = await invoke<OverlayPayload>(
          "get_dictation_overlay_status",
        );
        if (!disposed && !receivedEvent) setStatus(snapshot);
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const { phase } = status;
  const label =
    status.streamingState && (phase === "listening" || phase === "processing") ? status.statusText || "Listening" : phase === "mode" ? "Output language" : phase === "listening"
      ? status.statusText || "Listening"
      : phase === "processing"
        ? status.statusText || "Transcribing"
        : phase === "inserted"
          ? status.statusText || "Pasted" + (status.durationLimitReached ? " · 5-minute limit" : "")
          : phase === "clipboard_only"
            ? status.statusText || "Copied · " + formatPasteShortcutForDisplay(platform)
            : phase === "failed"
              ? "Needs attention"
              : "";
  const result =
    phase === "inserted" || phase === "clipboard_only" || phase === "failed";
  const level = Math.pow(Math.max(0, Math.min(1, status.audioLevel)), 0.65);
  return (
    <div
      className={"overlay-root" + (phase === "hidden" ? " is-hidden" : "")}
      aria-hidden={phase === "hidden"}
    >
      <div className={"overlay-capsule" + (status.streamingState ? " is-streaming" : "")} role="group" aria-label="Dictation overlay">
        <span className="sr-only" role="status" aria-live="polite" aria-atomic="true">{`${label} · ${outputLanguageLabel(status.outputMode ?? "original")}`}{status.streamingState && !result && committed ? ` · ${committed}` : ""}</span>
        <span className="overlay-language">{outputLanguageLabel(status.outputMode ?? "original")}</span>
        {status.streamingState && !result ? <>
          <span className="overlay-live-indicator" aria-hidden="true" style={{ opacity: phase === "listening" ? 0.45 + 0.55 * level : 0.6 }}><AppIcon name="microphone" /></span>
          <div ref={viewport} className={"overlay-live-text" + (overflow ? " has-overflow" : "")} aria-label="Live transcript" aria-live="off">
            {displayedText ? <><span className="overlay-live-committed">{styledText.committed}</span><span className="overlay-live-tentative" aria-label={styledText.tentative ? "Tentative text, may change" : undefined}>{styledText.tentative}</span></> : <span className="overlay-live-placeholder">{label}</span>}
          </div>
          {displayedText ? <span className="overlay-live-state" title={label}>{label}</span> : null}
        </> : phase === "mode" ? <span className="overlay-mode-hint">Output language</span> : phase === "processing" ? (
          <>
            <span className="overlay-spinner" aria-hidden="true" />
            <span className="overlay-phase-label">{label}</span>
          </>
        ) : result ? (
          <span
            className={
              "overlay-result" +
              (phase === "failed"
                ? " is-error"
                : phase === "clipboard_only"
                  ? " is-copied"
                  : "")
            }
          >
            <AppIcon name={phase === "failed" ? "info" : "check"} />
            {label}
          </span>
        ) : (
          <>
            <div className="overlay-bars" aria-hidden="true">
              {[0.4, 0.7, 1, 0.7, 0.4].map((weight, index) => (
                <span
                  className="overlay-bar"
                  key={index}
                  style={{ height: 4 + 30 * level * weight + "px" }}
                />
              ))}
            </div>
            {phase === "listening" && status.statusText ? (
              <span className="overlay-phase-label overlay-limit-notice">
                {status.statusText}
              </span>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}
