import { useEffect, useRef, useState } from "react";
import {
  ActionButton,
  Button,
  PageHeader,
  Progress,
} from "../components/Feedback";
import { AppIcon } from "../components/IconButton";
import {
  TranscriptReader,
  formatTimestamp,
} from "../components/TranscriptReader";
import { getRecordingInputLevel, copyTextToClipboard } from "../lib/api";
import {
  formatShortcutForDisplay,
  formatPasteShortcutForDisplay,
} from "../lib/formatting";
import type {
  AppSettings,
  DictationReadiness,
  ManualTranscriptionUiState,
  QuickDictationStatusResponse,
  RecordingStatusResponse,
  TranscriptionPreviewResponse,
} from "../types/domain";

interface Props {
  dictationError?: string | null;
  settings: AppSettings | null;
  platform: string | null;
  preview: TranscriptionPreviewResponse | null;
  recordingStatus: RecordingStatusResponse | null;
  manualTranscriptionState: ManualTranscriptionUiState;
  quickDictationStatus: QuickDictationStatusResponse | null;
  readiness: DictationReadiness | null;
  isPollingAccessibility: boolean;
  onResolveReadiness: (item: "model" | "shortcut" | "accessibility") => void;
  onStartRecording: () => Promise<void>;
  onStopAndTranscribeRecording: () => Promise<void>;
  onCancelRecording: () => Promise<void>;
  onResetDictation: () => Promise<void>;
}

export function DictateScreen(props: Props) {
  const {
    settings,
    recordingStatus: recording,
    quickDictationStatus: quick,
    manualTranscriptionState: manual,
    preview,
    readiness,
  } = props;
  const inFlight = useRef(false);
  const operationSequence = useRef(0);
  const [pending, setPending] = useState(false);
  const [actionError, setActionError] = useState("");
  const [level, setLevel] = useState(0);
  const [levelAvailable, setLevelAvailable] = useState(true);
  const quickActive =
    quick?.state === "listening" || quick?.state === "processing";
  const listening = quickActive
    ? quick?.state === "listening"
    : recording?.state === "listening";
  const processing =
    manual.stage === "processing" || quick?.state === "processing";
  const canStop =
    !quickActive &&
    (recording?.state === "listening" || recording?.state === "paused");
  const error =
    actionError ||
    manual.errorMessage ||
    props.dictationError ||
    (quick?.state === "error" ? quick.lastErrorMessage : null) ||
    preview?.error?.message;
  const shortcut = formatShortcutForDisplay(
    quick?.registeredShortcut ?? settings?.shortcut ?? "",
    props.platform,
  );
  const result = preview?.result;
  const quickText =
    !result && !listening && !processing ? quick?.lastTranscriptText : null;
  const outcome = result
    ? result.plainText.trim()
      ? "Transcript ready"
      : "No speech detected"
    : quick?.state === "inserted"
      ? "Pasted"
      : quick?.state === "clipboard_only"
        ? "Copied"
        : "Last dictation";
  const stateLabel = listening
    ? "Listening"
    : processing
      ? "Transcribing"
      : error
        ? "Needs attention"
        : manual.statusText || "Ready to dictate";

  useEffect(() => {
    if (!listening) {
      setLevel(0);
      return;
    }
    let disposed = false;
    let timer = 0;
    const poll = async () => {
      try {
        const next = await getRecordingInputLevel();
        if (!disposed) {
          setLevel(Math.max(0, Math.min(1, next)));
          setLevelAvailable(true);
        }
      } catch {
        if (!disposed) {
          setLevel(0);
          setLevelAvailable(false);
        }
      }
      if (!disposed) timer = window.setTimeout(poll, 100);
    };
    void poll();
    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, [listening]);

  async function act(action: () => Promise<void>) {
    if (inFlight.current) return;
    inFlight.current = true;
    const sequence = ++operationSequence.current;
    setPending(true);
    setActionError("");
    try {
      await action();
    } catch (reason) {
      if (sequence === operationSequence.current)
        setActionError(
          reason instanceof Error
            ? reason.message
            : "Could not complete the action.",
        );
    } finally {
      if (sequence === operationSequence.current) {
        inFlight.current = false;
        setPending(false);
      }
    }
  }

  async function reset() {
    await props.onResetDictation();
    operationSequence.current += 1;
    inFlight.current = false;
    setPending(false);
    setActionError("");
  }

  return (
    <section className="screen dictate-screen">
      <PageHeader
        eyebrow="YOUR VOICE, IN WORDS"
        title="Dictate"
        description="A thought, a message, a little more momentum."
      >
        <span className="local-badge">
          <span /> On-device transcription
        </span>
      </PageHeader>
      {readiness &&
      (!readiness.hasModel ||
        !readiness.shortcutRegistered ||
        (readiness.accessibilityRequired &&
          !readiness.accessibilityGranted)) ? (
        <aside className="setup-panel" aria-label="Dictation setup">
          <div>
            <AppIcon name="info" />
            <strong>Make yourself heard</strong>
          </div>
          {!readiness.hasModel ? (
            <div className="setup-row">
              <span>Download a speech model to start transcribing.</span>
              <Button onClick={() => props.onResolveReadiness("model")}>
                Download a model
              </Button>
            </div>
          ) : null}
          {!readiness.shortcutRegistered ? (
            <div className="setup-row">
              <span>
                Use the record button here, or set up your keyboard shortcut.
              </span>
              <Button onClick={() => props.onResolveReadiness("shortcut")}>
                Set a shortcut
              </Button>
            </div>
          ) : null}
          {readiness.accessibilityRequired &&
          !readiness.accessibilityGranted ? (
            <div className="setup-row">
              <span>
                Allow auto-paste to insert words into another app. You can still
                copy them.
              </span>
              <Button onClick={() => props.onResolveReadiness("accessibility")}>
                {props.isPollingAccessibility ? "Check again" : "Grant access"}
              </Button>
            </div>
          ) : null}
        </aside>
      ) : null}
      <article
        className={
          "dictation-studio surface" +
          (listening ? " is-listening" : processing ? " is-processing" : "")
        }
      >
        <div className="studio-topline">
          <span
            className={"state-label" + (listening ? " live" : "")}
            role="status"
          >
            <span />
            {stateLabel}
          </span>
          <span className="studio-device">
            {recording?.activeInputDevice ??
              settings?.preferredInputDevice ??
              "System microphone"}
          </span>
        </div>
        <div className="recording-stage">
          <div
            className="voice-meter"
            role="meter"
            aria-label="Microphone input level"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(level * 100)}
          >
            {Array.from({ length: 29 }, (_, index) => (
              <span
                key={index}
                style={{
                  height:
                    4 +
                    (listening
                      ? Math.pow(level, 0.65) *
                        (22 + 46 * Math.sin(((index + 1) / 30) * Math.PI))
                      : 0) +
                    "px",
                }}
              />
            ))}
          </div>
          <span className="recording-time">
            {formatTimestamp(
              listening || canStop ? (recording?.durationMs ?? 0) : 0,
            )}
          </span>
          <h2>
            {listening
              ? "Go ahead. We’re listening."
              : processing
                ? "Finding your words…"
                : "What’s on your mind?"}
          </h2>
          <p className="muted studio-hint">
            {listening
              ? !levelAvailable
                ? "Input meter unavailable. Recording is still active."
                : "Speak naturally. Stop when you’re finished."
              : processing
                ? manual.statusText ||
                  "Your audio is being transcribed on this device."
                : "Start recording and let your words take shape."}
          </p>
          <Button
            className="record-control"
            variant={canStop ? "danger" : "primary"}
            icon={canStop ? "stop" : "microphone"}
            disabled={
              !settings ||
              readiness?.hasModel === false ||
              quickActive ||
              processing
            }
            busy={pending && !processing}
            onClick={() =>
              void act(
                canStop
                  ? props.onStopAndTranscribeRecording
                  : props.onStartRecording,
              )
            }
          >
            {canStop
              ? "Stop and transcribe"
              : processing
                ? "Transcribing…"
                : quickActive
                  ? "Shortcut dictation active"
                  : "Start recording"}
          </Button>
          <div className="recording-secondary">
            {canStop ? (
              <Button
                variant="ghost"
                onClick={() => void act(props.onCancelRecording)}
                disabled={pending}
              >
                Cancel recording
              </Button>
            ) : (
              <span className="shortcut-hint">
                Or use <kbd>{shortcut || "your shortcut"}</kbd> from any app
              </span>
            )}
          </div>
        </div>
        {processing ? <Progress label="Transcribing recording" /> : null}
        <footer className="studio-footer">
          <span>
            <AppIcon name="window" /> Your audio stays on your device
          </span>
          <span>
            {settings?.saveHistory ? "History enabled" : "History off"}
          </span>
        </footer>
      </article>
      {error ? (
        <div className="error-panel" role="alert">
          <strong>Let’s try that again</strong>
          <p>{error}</p>
          <ActionButton icon="reset" action={reset} success="Reset">
            Reset dictation
          </ActionButton>
        </div>
      ) : null}
      {processing || quickActive ? (
        <ActionButton
          variant="ghost"
          icon="reset"
          action={reset}
          success="Reset"
        >
          Reset stuck dictation
        </ActionButton>
      ) : null}
      {(result || quickText) && !listening && !processing ? (
        <article className="surface result-panel">
          <div className="section-header">
            <div>
              <p className="eyebrow">YOUR WORDS</p>
              <h2 role="status">{outcome}</h2>
            </div>
            <ActionButton
              icon="copy"
              disabled={!String(result?.plainText ?? quickText ?? "").trim()}
              action={() =>
                copyTextToClipboard(result?.plainText ?? quickText ?? "")
              }
              success="Copied"
            >
              Copy text
            </ActionButton>
          </div>
          {quick?.state === "clipboard_only" && !result ? (
            <p className="muted">
              Press {formatPasteShortcutForDisplay(props.platform)} to paste.
            </p>
          ) : null}
          {result && !result.plainText.trim() ? (
            <p className="muted">
              Try speaking closer to the microphone, then record again.
            </p>
          ) : null}
          {result ? (
            <TranscriptReader result={result} />
          ) : (
            <p className="transcript-body">{quickText}</p>
          )}
        </article>
      ) : (
        <div className="workspace-tip">
          <AppIcon name="keyboardEdit" />
          <div>
            <strong>A shortcut to your next sentence</strong>
            <p>Use dictation in emails, documents, and anywhere you write.</p>
          </div>
        </div>
      )}
    </section>
  );
}
