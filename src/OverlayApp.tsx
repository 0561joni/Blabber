import { outputLanguageLabel } from "./lib/translationApi";
import type { OutputMode } from "./types/domain";
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getHealthCheck } from "./lib/api";
import { formatPasteShortcutForDisplay } from "./lib/formatting";
import { AppIcon } from "./components/IconButton";

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
}

export function OverlayApp() {
  const [status, setStatus] = useState<OverlayPayload>({
    phase: "hidden",
    audioLevel: 0,
  });
  const [platform, setPlatform] = useState<string | null>(null);
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
    phase === "mode" ? "Output language" : phase === "listening"
      ? "Listening"
      : phase === "processing"
        ? status.statusText || "Transcribing"
        : phase === "inserted"
          ? "Pasted"
          : phase === "clipboard_only"
            ? "Copied · " + formatPasteShortcutForDisplay(platform)
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
      <div className="overlay-capsule" role="status" aria-label={`${label} · ${outputLanguageLabel(status.outputMode ?? "original")}`}>
        <span className="overlay-language">{outputLanguageLabel(status.outputMode ?? "original")}</span>
        {phase === "mode" ? <span className="overlay-mode-hint">Output language</span> : phase === "processing" ? (
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
          <div className="overlay-bars" aria-hidden="true">
            {[0.4, 0.7, 1, 0.7, 0.4].map((weight, index) => (
              <span
                className="overlay-bar"
                key={index}
                style={{ height: 4 + 30 * level * weight + "px" }}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
