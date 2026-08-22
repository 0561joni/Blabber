import { useEffect, useRef, useState, type CSSProperties, type PointerEvent } from "react";
import {
  completeStartupHandoff,
  getStartupStatus,
  listenStartupStatus,
  quitApp,
  restartApp,
} from "./lib/api";
import type { StartupPhase, StartupStatus } from "./types/domain";

const INITIAL_STATUS: StartupStatus = {
  phase: "files",
  step: 1,
  totalSteps: 6,
};

export const STARTUP_PHASE_COPY: Record<
  Exclude<StartupPhase, "ready" | "failed">,
  { headline: string; detail: string }
> = {
  files: {
    headline: "Opening the studio",
    detail: "Preparing local files",
  },
  models: {
    headline: "Gathering the brain trust",
    detail: "Discovering installed speech models",
  },
  audio: {
    headline: "Mic check, one—two",
    detail: "Initializing audio and tray controls",
  },
  library: {
    headline: "Dusting off the word shelf",
    detail: "Updating settings, transcripts, and vocabulary",
  },
  shortcuts: {
    headline: "Untangling your shortcut",
    detail: "Registering keyboard controls",
  },
  workspace: {
    headline: "Putting the last word in place",
    detail: "Loading your workspace",
  },
};

function copyForStatus(status: StartupStatus) {
  if (status.phase === "ready") {
    return { headline: "Ready to blab", detail: "Your workspace is ready" };
  }
  if (status.phase === "failed") {
    return { headline: "That wasn’t in the script", detail: "Blabber couldn’t finish starting" };
  }
  return STARTUP_PHASE_COPY[status.phase];
}

export function SplashApp() {
  const [status, setStatus] = useState<StartupStatus>(INITIAL_STATUS);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [ambient, setAmbient] = useState(true);
  const [reacting, setReacting] = useState(false);
  const [bouncing, setBouncing] = useState(false);
  const [finishing, setFinishing] = useState(false);
  const [tilt, setTilt] = useState({ x: 0, y: 0, rotation: 0 });
  const mountedAt = useRef(Date.now());
  const handoffStarted = useRef(false);
  const copy = copyForStatus(status);
  const failed = status.phase === "failed";
  const showSlowActions = elapsedSeconds >= 30 && !failed;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    const acceptStatus = (nextStatus: StartupStatus) => {
      if (!disposed) {
        setStatus(nextStatus);
      }
    };

    void listenStartupStatus(acceptStatus).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    void getStartupStatus().then(acceptStatus).catch((error) => {
      acceptStatus({
        phase: "failed",
        step: 1,
        totalSteps: 6,
        errorMessage: error instanceof Error ? error.message : String(error),
      });
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setElapsedSeconds(Math.floor((Date.now() - mountedAt.current) / 1000));
    }, 1000);
    const ambientTimer = window.setTimeout(() => setAmbient(false), 4500);
    return () => {
      window.clearInterval(timer);
      window.clearTimeout(ambientTimer);
    };
  }, []);

  useEffect(() => {
    if (status.phase === "failed") return;
    setReacting(true);
    const timer = window.setTimeout(() => setReacting(false), 520);
    return () => window.clearTimeout(timer);
  }, [status.phase]);

  useEffect(() => {
    if (status.phase !== "ready" || handoffStarted.current) return;
    handoffStarted.current = true;
    const minimumRemaining = Math.max(0, 900 - (Date.now() - mountedAt.current));
    const finishTimer = window.setTimeout(() => {
      setFinishing(true);
      window.setTimeout(() => {
        void completeStartupHandoff().catch(() => undefined);
      }, 180);
    }, minimumRemaining);
    return () => window.clearTimeout(finishTimer);
  }, [status.phase]);

  function handlePointerMove(event: PointerEvent<HTMLButtonElement>) {
    if (failed) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const horizontal = (event.clientX - bounds.left) / bounds.width - 0.5;
    const vertical = (event.clientY - bounds.top) / bounds.height - 0.5;
    setTilt({
      x: horizontal * 10,
      y: vertical * 7,
      rotation: horizontal * 5,
    });
  }

  function bounceBuddy() {
    if (failed || bouncing) return;
    setBouncing(true);
    window.setTimeout(() => setBouncing(false), 340);
  }

  const buddyStyle = {
    "--buddy-x": `${tilt.x}px`,
    "--buddy-y": `${tilt.y}px`,
    "--buddy-rotation": `${tilt.rotation}deg`,
  } as CSSProperties;

  return (
    <main
      className={`splash-root${failed ? " is-failed" : ""}${finishing ? " is-finishing" : ""}`}
      aria-label="Blabber startup"
    >
      <div className="splash-orb splash-orb-one" aria-hidden="true" />
      <div className="splash-orb splash-orb-two" aria-hidden="true" />

      <header className="splash-brand">
        <span className="splash-brand-dot" aria-hidden="true" />
        <span>Blabber</span>
      </header>

      <section className="buddy-stage" aria-label="Animated Blabber microphone">
        <button
          type="button"
          className={`buddy-button${ambient ? " is-ambient" : ""}${reacting ? " is-reacting" : ""}${bouncing ? " is-bouncing" : ""}`}
          style={buddyStyle}
          onPointerMove={handlePointerMove}
          onPointerLeave={() => setTilt({ x: 0, y: 0, rotation: 0 })}
          onClick={bounceBuddy}
          aria-label="Make the Blabber buddy bounce"
          disabled={failed}
        >
          <span className="syllable syllable-one" aria-hidden="true" />
          <span className="syllable syllable-two" aria-hidden="true" />
          <span className="syllable syllable-three" aria-hidden="true" />
          <span className="syllable syllable-four" aria-hidden="true" />
          <span className="buddy-logo" aria-hidden="true">
            <svg viewBox="0 0 160 160" role="presentation">
              <defs>
                <linearGradient id="splash-bg" x1="18" y1="12" x2="142" y2="150">
                  <stop offset="0" stopColor="#34d6f4" />
                  <stop offset="1" stopColor="#2d4bff" />
                </linearGradient>
                <linearGradient id="splash-mic" x1="80" y1="50" x2="80" y2="132">
                  <stop offset="0" stopColor="#38c8f3" />
                  <stop offset="1" stopColor="#2d74f4" />
                </linearGradient>
              </defs>
              <rect x="4" y="4" width="152" height="152" rx="44" fill="url(#splash-bg)" />
              <rect x="51" y="25" width="58" height="104" rx="29" fill="white" />
              <g className="buddy-expression">
                <rect className="buddy-eye buddy-eye-left" x="64" y="57" width="13" height="6" rx="3" fill="url(#splash-mic)" />
                <rect className="buddy-eye buddy-eye-right" x="83" y="57" width="13" height="6" rx="3" fill="url(#splash-mic)" />
                <rect className="buddy-mouth" x="68" y="76" width="24" height="27" rx="12" fill="url(#splash-mic)" />
                <rect x="77" y="100" width="6" height="34" rx="3" fill="url(#splash-mic)" />
              </g>
            </svg>
          </span>
        </button>
      </section>

      <section className="startup-copy" role="status" aria-live="polite" aria-atomic="true">
        <h1>{copy.headline}</h1>
        <p>{copy.detail}</p>
      </section>

      {failed ? (
        <section className="startup-recovery" role="alert">
          <details>
            <summary>Technical details</summary>
            <p>{status.errorMessage ?? "An unknown startup error occurred."}</p>
          </details>
          <RecoveryActions />
        </section>
      ) : (
        <footer className="startup-progress">
          <div
            className="milestone-track"
            role="progressbar"
            aria-label={`Startup step ${status.step} of ${status.totalSteps}`}
            aria-valuemin={1}
            aria-valuemax={status.totalSteps}
            aria-valuenow={status.step}
          >
            {Array.from({ length: status.totalSteps }, (_, index) => (
              <span
                key={index}
                className={index < status.step ? "milestone is-complete" : "milestone"}
                aria-hidden="true"
              />
            ))}
          </div>
          <span className="step-label">{status.step} of {status.totalSteps}</span>
          {elapsedSeconds >= 15 ? (
            <p className="slow-note">Still warming up the local engine—thanks for hanging out.</p>
          ) : null}
          {showSlowActions ? <RecoveryActions /> : null}
        </footer>
      )}
    </main>
  );
}

function RecoveryActions() {
  return (
    <div className="recovery-actions">
      <button type="button" onClick={() => void restartApp()}>
        Restart Blabber
      </button>
      <button type="button" className="quiet-action" onClick={() => void quitApp()}>
        Quit
      </button>
    </div>
  );
}
