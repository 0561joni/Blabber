import { TranslationResult } from "../components/TranslationResult";
import { OUTPUT_MODES } from "../lib/translationApi";
import { useEffect, useRef, useState } from "react";
import {
  ActionButton,
  Button,
  PageHeader,
  Progress,
  ShortcutKeys,
} from "../components/Feedback";
import {
  TranscriptReader,
  formatTimestamp,
} from "../components/TranscriptReader";
import { getRecordingInputLevel, copyTextToClipboard } from "../lib/api";
import {
  formatShortcutForDisplay,
  formatPasteShortcutForDisplay,
  formatListDuration,
  formatTime,
  readableError,
} from "../lib/formatting";
import type {
  AppSettings,
  DictationOutputState,
  OutputMode,
  DictationReadiness,
  ManualTranscriptionUiState,
  QuickDictationStatusResponse,
  RecordingStatusResponse,
  TranscriptionPreviewResponse,
  TranscriptSummary,
} from "../types/domain";

interface Props {
  outputState?: DictationOutputState | null;
  onOutputModeChange?: (mode: OutputMode) => Promise<void>;
  onRetryTranslation?: () => Promise<void>;
  onRetryLive?: () => Promise<void>;
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
  recentDictations?: TranscriptSummary[];
  onOpenTranscript?: (id: string) => void;
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
    manual.stage === "processing" || quick?.state === "processing" || (props.outputState?.busy === true && props.outputState.stage !== "recording");
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
  const output = preview?.dictationOutput ?? props.outputState?.lastOutput;
  const translatedOutput = output?.targetLanguage !== "original" ? output : null;
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
      ? props.outputState?.statusText || "Transcribing"
      : error
        ? "Failed"
        : manual.statusText || "Ready";

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

  const holdOrPress = settings?.shortcutMode === "toggle" ? "Press" : "Hold";
  const lastId = quick?.lastTranscriptId ?? null;
  const today = new Date().toDateString();
  const earlier = (props.recentDictations ?? [])
    .filter(
      (item) =>
        item.sourceType === "quick_dictate" &&
        item.id !== lastId &&
        new Date(item.createdAt).toDateString() === today,
    )
    .slice(0, 5);
  const hasResult = Boolean(result || quickText || translatedOutput) && !listening && !processing;
  const outcomeTone = result && !result.plainText.trim() ? "warning" : "success";

  return (
    <section className="screen dictate-screen">
      <PageHeader title="Dictate">
        {props.outputState ? (
          <div className="output-picker">
            <label htmlFor="dictation-output-language">Output language</label>
            <select
              id="dictation-output-language"
              value={props.outputState.outputMode}
              disabled={props.outputState.busy || pending}
              title={`${formatShortcutForDisplay(settings?.translationCycleShortcut ?? "CmdOrCtrl+Shift+Right", props.platform)} switches language from any app`}
              onChange={(event) => void act(() => props.onOutputModeChange?.(event.target.value as OutputMode) ?? Promise.resolve())}
            >
              {OUTPUT_MODES.map((item) => <option key={item.value} value={item.value} disabled={item.value !== "original" && !props.outputState?.ready}>{item.label}</option>)}
            </select>
            {!props.outputState.ready ? (
              <Button variant="ghost" onClick={() => props.onResolveReadiness("model")}>Set up translation</Button>
            ) : props.outputState.outputMode !== "original" ? (
              <span className="output-picker-note">Back to Original after 1 min idle</span>
            ) : null}
          </div>
        ) : null}
      </PageHeader>
      {settings?.translationEnabled && props.outputState && !props.outputState.ready && props.outputState.errorMessage ? (
        <p className="notice-text" role="status">{readableError(props.outputState.errorMessage)}</p>
      ) : null}
      {readiness &&
      (!readiness.hasModel ||
        !readiness.shortcutRegistered ||
        (readiness.accessibilityRequired &&
          !readiness.accessibilityGranted)) ? (
        <aside className="setup-panel" aria-label="Dictation setup">
          <strong>Finish setup</strong>
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
                Set a keyboard shortcut to dictate from any app. The record button works without one.
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
                Allow Accessibility access so Blabber can paste into other apps. Without it, text is copied.
              </span>
              <Button onClick={() => props.onResolveReadiness("accessibility")}>
                {props.isPollingAccessibility ? "Check again" : "Grant access"}
              </Button>
            </div>
          ) : null}
        </aside>
      ) : null}
      <div
        className={
          "recorder" +
          (listening ? " is-listening" : processing ? " is-processing" : "")
        }
      >
        <div className="recorder-controls">
          <Button
            className="record-control"
            size="large"
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
          {canStop ? (
            <Button
              variant="ghost"
              onClick={() => void act(props.onCancelRecording)}
              disabled={pending}
            >
              Cancel recording
            </Button>
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
        </div>
        <span className="recording-time">
          {formatTimestamp(
            listening || canStop ? (recording?.durationMs ?? 0) : 0,
          )}
        </span>
        <div
          className="voice-meter"
          role="meter"
          aria-label="Microphone input level"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(level * 100)}
        >
          {Array.from({ length: 40 }, (_, index) => (
            <span
              key={index}
              style={{
                height:
                  2 +
                  (listening
                    ? Math.pow(level, 0.65) *
                      (6 + 20 * Math.abs(Math.sin((index + 1) * 1.7)))
                    : 0) +
                  "px",
              }}
            />
          ))}
        </div>
        <div className="recorder-status">
          <span
            className={
              "state-label" +
              (listening ? " is-live" : error && !processing ? " is-error" : processing ? " is-busy" : "")
            }
            role="status"
          >
            {stateLabel}
          </span>
          <span className="studio-device">
            {recording?.activeInputDevice ??
              settings?.preferredInputDevice ??
              "System microphone"}
          </span>
        </div>
      </div>
      {processing ? <Progress label={props.outputState?.statusText || "Transcribing recording"} /> : null}
      <p className="recorder-hint">
        {listening
          ? !levelAvailable
            ? "Input meter unavailable. Recording is still active."
            : "Speak normally. Stop when you’re done."
          : processing
            ? props.outputState?.statusText || manual.statusText ||
              "Transcribing on this Mac."
            : shortcut
              ? <>Or {holdOrPress.toLowerCase()} <ShortcutKeys shortcut={shortcut} /> in any app</>
              : "Set a shortcut in Settings to dictate from any app."}
      </p>
      {error ? (
        <div className="error-panel" role="alert">
          <strong>Dictation failed</strong>
          <p>{readableError(error)}</p>
          {quick?.canRetryStreaming && props.onRetryLive ? (
            <p className="error-panel-note">
              The recording was kept. Retry shows the text here to copy; it won’t paste into another app.
            </p>
          ) : null}
          <div className="error-panel-actions">
            {quick?.canRetryStreaming && props.onRetryLive ? (
              <ActionButton icon="reset" action={props.onRetryLive} success="Transcript ready">Retry transcription</ActionButton>
            ) : null}
            <ActionButton icon="reset" action={reset} success="Reset">
              Reset dictation
            </ActionButton>
          </div>
        </div>
      ) : null}
      {hasResult ? (
        <section className="dictation-result" aria-labelledby="last-dictation-heading">
          <div className="section-header">
            <h2 id="last-dictation-heading" className="section-title">Last dictation</h2>
            <span className={"status-text is-" + outcomeTone} role="status">{outcome}</span>
            <span className="section-header-spacer" />
            {!translatedOutput ? <ActionButton
              icon="copy"
              disabled={!String(result?.plainText ?? quickText ?? "").trim()}
              action={() =>
                copyTextToClipboard(result?.plainText ?? quickText ?? "")
              }
              success="Copied"
            >
              Copy text
            </ActionButton> : null}
          </div>
          {quick?.lastInsertWarning && !result ? (
            <p className="notice-text" role="status">{quick.lastInsertWarning}</p>
          ) : null}
          {quick?.lastInsertOutcome === "clipboard_only" && !result ? (
            <p className="muted">
              Press {formatPasteShortcutForDisplay(props.platform)} to paste.
            </p>
          ) : null}
          {result && !result.plainText.trim() ? (
            <p className="muted">
              Try speaking closer to the microphone, then record again.
            </p>
          ) : null}
          {translatedOutput ? <TranslationResult output={translatedOutput} onRetry={props.onRetryTranslation} /> : result ? (
            <TranscriptReader result={result} />
          ) : (
            <p className="transcript-body">{quickText}</p>
          )}
        </section>
      ) : null}
      {earlier.length > 0 ? (
        <section className="recent-dictations" aria-labelledby="earlier-heading">
          <h2 id="earlier-heading" className="section-title">Earlier today</h2>
          <ul>
            {earlier.map((item) => (
              <li key={item.id}>
                <button
                  type="button"
                  className="recent-row"
                  onClick={() => props.onOpenTranscript?.(item.id)}
                >
                  <span className="recent-text">{item.plainText.trim() || item.title}</span>
                  <span className="recent-num">{formatTime(item.createdAt)}</span>
                  <span className="recent-num">{item.durationMs != null ? formatListDuration(item.durationMs) : ""}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      ) : !hasResult && !error && settings && !settings.saveHistory ? (
        <p className="muted recent-empty">History is off, so past dictations aren’t kept.</p>
      ) : null}
    </section>
  );
}
