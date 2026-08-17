import { useCallback, useEffect, useRef, useState } from "react";
import { dictatePress, dictateRelease } from "../lib/api";
import { formatDuration, formatShortcutForDisplay } from "../lib/formatting";
import { IconButton } from "../components/IconButton";
import type {
  AppSettings,
  DictationReadiness,
  FileQueueItem,
  ManualTranscriptionUiState,
  QuickDictationStatusResponse,
  RecordingStatusResponse,
  TranscriptionPreviewResponse,
  TranscriptResult,
} from "../types/domain";

type ReadinessItem = "model" | "shortcut" | "accessibility";

interface HomeScreenProps {
  settings: AppSettings | null;
  platform: string | null;
  preview: TranscriptionPreviewResponse | null;
  recordingStatus: RecordingStatusResponse | null;
  manualTranscriptionState: ManualTranscriptionUiState;
  quickDictationStatus: QuickDictationStatusResponse | null;
  readiness: DictationReadiness | null;
  isPollingAccessibility: boolean;
  onResolveReadiness: (item: ReadinessItem) => void;
  fileQueueItems: FileQueueItem[];
  isFileDragActive: boolean;
  speakerCountHint: number | null;
  onSpeakerCountHintChange: (speakerCountHint: number | null) => void;
  onStartRecording: () => void;
  onStopAndTranscribeRecording: () => void;
  onCancelRecording: () => void;
  onResetDictation: () => void;
  onPickFiles: (speakerCountHint: number | null) => void;
  onDropFiles: (files: FileList, speakerCountHint: number | null) => void;
  onSetFileDragActive: (active: boolean) => void;
  onToggleFileTranscript: (itemId: string) => void;
  onCopyFileTranscript: (itemId: string, text: string) => void;
}

export function HomeScreen({
  settings,
  platform,
  preview,
  recordingStatus,
  manualTranscriptionState,
  quickDictationStatus,
  readiness,
  isPollingAccessibility,
  onResolveReadiness,
  fileQueueItems,
  isFileDragActive,
  speakerCountHint,
  onSpeakerCountHintChange,
  onStartRecording,
  onStopAndTranscribeRecording,
  onCancelRecording,
  onResetDictation,
  onPickFiles,
  onDropFiles,
  onSetFileDragActive,
  onToggleFileTranscript,
  onCopyFileTranscript,
}: HomeScreenProps) {
  const isListening = recordingStatus?.state === "listening";
  const isPaused = recordingStatus?.state === "paused";
  const isManualProcessing = manualTranscriptionState.stage === "processing";
  const isBusy = recordingStatus?.state === "processing" || isManualProcessing;
  const shouldDisableRecordButton = isBusy && !isManualProcessing;
  const canStop = isListening || isPaused;
  const [isPttActive, setIsPttActive] = useState(false);
  const [speakerHintOpen, setSpeakerHintOpen] = useState(false);
  const pttActiveRef = useRef(false);
  pttActiveRef.current = isPttActive;
  const dictationState = quickDictationStatus?.state ?? "idle";
  const isShortcutDictating =
    dictationState === "listening" || dictationState === "processing";
  // Disable the PTT button when the global shortcut is mid-dictation, so the
  // user can't accidentally cut someone off. While we're holding it ourselves,
  // it stays enabled so we can release.
  const pttDisabled =
    (!isPttActive && isShortcutDictating) || isBusy || canStop;

  const handlePttPress = useCallback(
    async (event: React.PointerEvent<HTMLButtonElement>) => {
      if (pttActiveRef.current || pttDisabled) return;
      // Capture pointer so pointerup fires even if the cursor leaves the button.
      try {
        event.currentTarget.setPointerCapture(event.pointerId);
      } catch {
        // Some platforms (or pointer types) reject capture; ignore.
      }
      setIsPttActive(true);
      try {
        await dictatePress();
      } catch {
        setIsPttActive(false);
      }
    },
    [pttDisabled],
  );

  const handlePttRelease = useCallback(async () => {
    if (!pttActiveRef.current) return;
    setIsPttActive(false);
    try {
      await dictateRelease();
    } catch {
      // Swallow; the controller will time out / reset on its own.
    }
  }, []);
  const shortcut = formatShortcutForDisplay(
    quickDictationStatus?.registeredShortcut ?? settings?.shortcut ?? "Not configured",
    platform,
  );
  const hasShortcutResult =
    !!quickDictationStatus?.lastTranscriptText &&
    (!!quickDictationStatus?.lastTranscriptId || !!quickDictationStatus?.lastInsertOutcome);
  const liveState =
    quickDictationStatus && quickDictationStatus.state !== "idle"
      ? quickDictationStatus.state
      : recordingStatus?.state ?? "idle";

  const liveTitle =
    liveState === "listening"
      ? "Listening for dictation"
      : liveState === "paused"
        ? "Recording paused"
        : liveState === "processing"
          ? "Transcribing locally"
          : liveState === "success" || liveState === "inserted"
            ? "Dictation delivered"
            : liveState === "clipboard_only"
              ? "Copied for manual paste"
              : liveState === "error"
                ? "Dictation needs attention"
                : hasShortcutResult
                  ? "Last shortcut dictation"
                : "Background dictation is ready";

  const liveCopy =
    quickDictationStatus?.lastTranscriptText &&
    (quickDictationStatus.state === "inserted" ||
      quickDictationStatus.state === "clipboard_only" ||
      quickDictationStatus.state === "error" ||
      (quickDictationStatus.state === "idle" && hasShortcutResult))
      ? quickDictationStatus.lastErrorMessage
        ? `${quickDictationStatus.lastTranscriptText} ${quickDictationStatus.lastErrorMessage}`
        : quickDictationStatus.lastTranscriptText
      : quickDictationStatus?.lastErrorMessage ??
        recordingStatus?.lastErrorMessage ??
        (recordingStatus?.state === "paused"
          ? "Manual recording is paused. Cancel and start again if needed."
          : recordingStatus?.state === "listening"
            ? "Press the same control again to stop and transcribe."
            : recordingStatus?.state === "success"
              ? "Transcript ready."
              : `Use ${shortcut} or tap the record control to start manual dictation.`);

  // Offer the manual reset whenever dictation could be wedged: after an error,
  // or while it is mid-flight (so the user can always escape a stuck session).
  const showResetAction =
    liveState === "error" || liveState === "processing" || liveState === "listening";

  const readinessItems = buildReadinessItems(readiness, isPollingAccessibility);
  const showReadinessCard = readinessItems.some((item) => !item.ok);

  const showRecordingMeta =
    recordingStatus?.state === "listening" ||
    recordingStatus?.state === "paused" ||
    recordingStatus?.state === "success" ||
    recordingStatus?.state === "error";
  const showManualTranscriptionCard = manualTranscriptionState.stage !== "idle";
  const elapsedSeconds = useElapsedSeconds(
    manualTranscriptionState.stage === "processing" ? manualTranscriptionState.startedAt : null,
  );

  // HTML5 drag-and-drop handlers for the capture panel.
  // dragCounter ref tracks nested dragenter/dragleave events correctly.
  const dragCounter = useRef(0);

  const handleDragEnter = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragCounter.current += 1;
      if (dragCounter.current === 1) {
        onSetFileDragActive(true);
      }
    },
    [onSetFileDragActive],
  );

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDragLeave = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragCounter.current -= 1;
      if (dragCounter.current <= 0) {
        dragCounter.current = 0;
        onSetFileDragActive(false);
      }
    },
    [onSetFileDragActive],
  );

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragCounter.current = 0;
      onSetFileDragActive(false);
      if (e.dataTransfer.files.length > 0) {
        onDropFiles(e.dataTransfer.files, speakerCountHint);
      }
    },
    [onSetFileDragActive, onDropFiles, speakerCountHint],
  );

  return (
    <section className="screen home-screen">
      <div className="home-stack">
        {showReadinessCard ? (
          <article className="glass-panel readiness-card apple-panel">
            <div className="section-header section-header-compact">
              <div>
                <p className="eyebrow">Get set up</p>
                <h2>Finish setting up dictation</h2>
                <p className="muted">
                  A few things need attention before shortcut dictation works end to end.
                </p>
              </div>
            </div>
            <ul className="readiness-list">
              {readinessItems.map((item) => (
                <li
                  key={item.key}
                  className={`readiness-item ${item.ok ? "is-ok" : "is-pending"}`}
                >
                  <span className="readiness-status" aria-hidden="true">
                    {item.ok ? (
                      <svg viewBox="0 0 24 24">
                        <path d="M20 6 9 17l-5-5" />
                      </svg>
                    ) : (
                      <svg viewBox="0 0 24 24">
                        <path d="M12 8v5" />
                        <path d="M12 16.5v.5" />
                        <circle cx="12" cy="12" r="9" />
                      </svg>
                    )}
                  </span>
                  <div className="readiness-copy">
                    <strong>{item.title}</strong>
                    <p className="muted">{item.detail}</p>
                  </div>
                  {!item.ok && item.actionLabel ? (
                    <button
                      type="button"
                      className="small-action-button"
                      onClick={() => onResolveReadiness(item.key)}
                    >
                      {item.actionLabel}
                    </button>
                  ) : null}
                </li>
              ))}
            </ul>
          </article>
        ) : null}

        <article
          onDragEnter={handleDragEnter}
          onDragOver={handleDragOver}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
          className={
            isFileDragActive
              ? "glass-panel primary-panel capture-panel apple-panel is-file-drop-target"
              : "glass-panel primary-panel capture-panel apple-panel"
          }
        >
          <div className="section-header section-header-compact">
            <div>
              <p className="eyebrow">Capture</p>
              <h2>Quick Dictate</h2>
              <p className="muted capture-subtitle">{shortcut}</p>
            </div>
          </div>

          <div className="transport-cluster">
            <div className="transport-controls" role="group" aria-label="Manual recording controls">
              <button
                className={
                  isManualProcessing
                    ? "record-button is-processing"
                    : shouldDisableRecordButton
                      ? "record-button is-inactive"
                    : canStop
                      ? "record-button is-recording"
                      : "record-button"
                }
                disabled={shouldDisableRecordButton}
                aria-disabled={isBusy ? "true" : undefined}
                onClick={() => {
                  if (isManualProcessing || shouldDisableRecordButton) {
                    return;
                  }
                  if (canStop) {
                    onStopAndTranscribeRecording();
                    return;
                  }
                  onStartRecording();
                }}
                aria-label={
                  isManualProcessing
                    ? "Transcription in progress"
                    : canStop
                      ? "Stop recording and transcribe"
                      : "Start recording"
                }
                title={
                  isManualProcessing
                    ? "Transcription in progress"
                    : canStop
                      ? "Stop recording and transcribe"
                      : "Start recording"
                }
              >
                <span className="record-button-shell">
                  {isManualProcessing ? (
                    <span className="record-processing-ring" aria-hidden="true">
                      <span className="record-processing-core" />
                    </span>
                  ) : canStop ? (
                    <span className="record-stop-core" aria-hidden="true" />
                  ) : (
                    <span className="record-start-core" aria-hidden="true" />
                  )}
                </span>
              </button>

              <IconButton
                className="home-primary-icon-action"
                icon={isPttActive ? "microphoneActive" : "microphone"}
                label={
                  isPttActive
                    ? "Release to transcribe to clipboard"
                    : "Hold to dictate to clipboard"
                }
                state={isPttActive ? "selected" : "default"}
                disabled={pttDisabled}
                aria-pressed={isPttActive}
                onPointerDown={(event) => {
                  void handlePttPress(event);
                }}
                onPointerUp={() => {
                  void handlePttRelease();
                }}
                onPointerCancel={() => {
                  void handlePttRelease();
                }}
                onPointerLeave={(event) => {
                  // Only release on leave if the pointer was already captured
                  // (i.e. user is dragging away while holding). Plain hover-out
                  // when not pressed should not count.
                  if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                    void handlePttRelease();
                  }
                }}
                onContextMenu={(event) => event.preventDefault()}
              />

              <IconButton
                className="home-primary-icon-action"
                icon="upload"
                label="Upload audio files"
                onClick={() => onPickFiles(speakerCountHint)}
              />
              {settings?.fileDiarizationEnabled ? (
                <div className="speaker-hint-anchor">
                  <IconButton
                    className="speaker-hint-button"
                    icon={speakerCountHint === null ? "personAutomatic" : "personCount"}
                    badge={speakerCountHint ?? undefined}
                    label={speakerCountHint === null ? "Speaker hint: Automatic" : `Speaker hint: About ${speakerCountHint}`}
                    state={speakerHintOpen ? "selected" : "default"}
                    aria-haspopup="dialog"
                    aria-expanded={speakerHintOpen}
                    onClick={() => setSpeakerHintOpen((open) => !open)}
                  />
                  {speakerHintOpen ? (
                    <div className="speaker-hint-popover" role="dialog" aria-label="Speaker count hint">
                      <button type="button" className={speakerCountHint === null ? "is-selected" : ""} onClick={() => { onSpeakerCountHintChange(null); setSpeakerHintOpen(false); }}>Automatic</button>
                      <label>
                        <span>About this many speakers</span>
                        <input
                          aria-label="Approximate speaker count"
                          type="number"
                          min={1}
                          max={20}
                          value={speakerCountHint ?? 7}
                          onChange={(event) => onSpeakerCountHintChange(Math.max(1, Math.min(20, Number(event.target.value) || 1)))}
                        />
                      </label>
                      <p className="muted">Use only when you have a good estimate. The local runtime targets this count.</p>
                      <button type="button" onClick={() => { onSpeakerCountHintChange(speakerCountHint ?? 7); setSpeakerHintOpen(false); }}>Use estimate</button>
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>

            <div className="transport-secondary-actions">
              <IconButton
                icon="xCircle"
                label="Cancel recording"
                tone="danger"
                disabled={!canStop || isBusy}
                onClick={onCancelRecording}
              />
            </div>
          </div>

          <div
            className={`drop-pill ${isFileDragActive ? "drop-pill-expanded" : ""}`}
            role="status"
            aria-live="polite"
          >
            <div className="drop-pill-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24">
                <rect x="3" y="3" width="18" height="18" rx="3" strokeDasharray="4 3" />
                <path d="M12 8v8" />
                <path d="m8.5 12.5 3.5 3.5 3.5-3.5" />
              </svg>
            </div>
            <span className="drop-pill-label">
              {isFileDragActive ? "Drop to transcribe" : "Drop files here"}
            </span>
            <span className="drop-pill-hint">WAV, MP3, M4A, or OPUS</span>
          </div>

          {showRecordingMeta ? (
            <dl className="meta-list capture-meta-list">
              <div>
                <dt>Input device</dt>
                <dd>{recordingStatus?.activeInputDevice ?? "Unavailable"}</dd>
              </div>
              <div>
                <dt>Duration</dt>
                <dd>
                  {recordingStatus?.durationMs
                    ? `${(recordingStatus.durationMs / 1000).toFixed(1)}s`
                    : "0.0s"}
                </dd>
              </div>
            </dl>
          ) : null}

          {preview ? (
            <div className="preview-result glass-subtle">
              {preview.error ? (
                <p className="muted">
                  {preview.error.code}: {preview.error.message}
                </p>
              ) : (
                <>
                  {preview.result ? <InlineSpeakerTranscript result={preview.result} /> : null}
                  <p className="preview-model-footer muted">
                    Model: {preview.resolvedModel?.modelName ?? preview.result?.modelName ?? "Missing model"}
                  </p>
                </>
              )}
            </div>
          ) : null}

          {showManualTranscriptionCard ? (
            <div className="file-queue-card glass-subtle manual-transcription-card">
              <div className="file-queue-header">
                <div>
                  <p className="eyebrow">Manual dictation</p>
                  <p className="transcript-title">
                    {manualTranscriptionState.stage === "failed"
                      ? "Transcription failed"
                      : "Transcribing locally"}
                  </p>
                </div>
                <span
                  className={`status-pill progress-pill stage-${
                    manualTranscriptionState.stage === "failed" ? "failed" : "saving"
                  }`}
                >
                  {manualTranscriptionState.stage === "failed" ? "Failed" : "Processing"}
                </span>
              </div>

              {manualTranscriptionState.stage === "processing" ? (
                <>
                  <p className="progress-copy">{manualTranscriptionState.statusText}</p>
                  <div className="progress-track" aria-hidden="true">
                    <div className="progress-fill indeterminate" />
                  </div>
                  <div className="progress-meta">
                    <span>{manualTranscriptionState.statusText}</span>
                    <span>Elapsed {formatDuration(elapsedSeconds)}</span>
                    <span>Working</span>
                  </div>
                </>
              ) : null}

              {manualTranscriptionState.errorMessage ? (
                <p className="error-text">{manualTranscriptionState.errorMessage}</p>
              ) : null}
            </div>
          ) : null}

          {fileQueueItems.length > 0 ? (
            <div className="file-queue-list">
              {fileQueueItems.map((item) => (
                <div className="file-queue-card glass-subtle" key={item.id}>
                  <div className="file-queue-header">
                    <div>
                      <p className="eyebrow">Upload</p>
                      <div className="file-queue-title-row">
                        {item.result ? (
                          <IconButton
                            className={`disclosure-icon-button${item.isExpanded ? " is-expanded" : ""}`}
                            icon="disclosure"
                            label={`${item.isExpanded ? "Collapse" : "Expand"} transcript for ${item.sourceFile.originalName}`}
                            size="compact"
                            aria-expanded={item.isExpanded}
                            aria-controls={`file-transcript-${item.id}`}
                            onClick={() => onToggleFileTranscript(item.id)}
                          />
                        ) : null}
                        <p className="transcript-title">{item.sourceFile.originalName}</p>
                      </div>
                    </div>
                    <span className={`status-pill progress-pill stage-${item.stage}`}>
                      {fileStageLabel(item.stage)}
                    </span>
                  </div>

                  {isFileQueueWorking(item.stage) ? (
                    <>
                      <p className="progress-copy">{item.statusText}</p>
                      <div className="progress-track" aria-hidden="true">
                        <div
                          className={
                            item.stage === "queued" || item.stage === "preparing" || item.stage === "diarizing"
                              ? "progress-fill indeterminate"
                              : "progress-fill"
                          }
                          style={
                            item.stage === "queued" || item.stage === "preparing" || item.stage === "diarizing"
                              ? undefined
                              : { width: `${progressPercent(item)}%` }
                          }
                        />
                      </div>
                      <div className="progress-meta">
                        <span>ETA {item.etaSeconds != null ? formatDuration(item.etaSeconds) : "Estimating..."}</span>
                        <span>
                          {item.progressPercent != null
                            ? `${Math.round(progressPercent(item))}% complete`
                            : "Waiting"}
                        </span>
                      </div>
                    </>
                  ) : null}

                  {item.errorMessage ? <p className="error-text">{item.errorMessage}</p> : null}

                  {item.result ? (
                    <>
                      <div
                        className="text-surface compact-text-surface"
                        id={`file-transcript-${item.id}`}
                      >
                        {item.isExpanded ? <InlineSpeakerTranscript result={item.result.result} /> : <p className="clamped-text">{item.result.result.plainText}</p>}
                      </div>
                      <div className="toolbar">
                        <IconButton
                          icon={item.copyState === "copied" ? "check" : item.copyState === "error" ? "xmark" : "copy"}
                          label={
                            item.copyState === "copied"
                              ? "Transcript copied"
                              : item.copyState === "error"
                                ? "Copy failed"
                                : "Copy transcript"
                          }
                          state={item.copyState === "copied" ? "success" : item.copyState === "error" ? "error" : "default"}
                          onClick={() => onCopyFileTranscript(item.id, item.result!.result.plainText)}
                        />
                        <span className="language-chip">
                          {item.result.result.detectedLanguages.join(", ") || "No language tags"}
                        </span>
                      </div>
                    </>
                  ) : null}
                </div>
              ))}
            </div>
          ) : null}
        </article>

        <div className={`live-banner glass-subtle home-status-panel state-${liveState}`}>
          <div className="live-banner-copy">
            <p className="eyebrow">Live status</p>
            <strong>{liveTitle}</strong>
            <p className="muted">{liveCopy}</p>
          </div>
          <div className="live-banner-side">
            <span className={`status-pill status-pill-${liveState}`}>
              {liveStateLabel(liveState)}
            </span>
            {showResetAction ? (
              <IconButton
                icon="reset"
                label="Reset dictation"
                className="live-reset-button"
                onClick={onResetDictation}
              />
            ) : null}
          </div>
        </div>
      </div>
    </section>
  );
}


function useElapsedSeconds(startedAt: number | null) {
  const [elapsedSeconds, setElapsedSeconds] = useState(0);

  useEffect(() => {
    if (!startedAt) {
      setElapsedSeconds(0);
      return;
    }
    const update = () => {
      setElapsedSeconds(Math.max(0, Math.floor((Date.now() - startedAt) / 1000)));
    };
    update();
    const intervalId = window.setInterval(update, 1000);
    return () => window.clearInterval(intervalId);
  }, [startedAt]);

  return elapsedSeconds;
}

interface ReadinessRow {
  key: ReadinessItem;
  ok: boolean;
  title: string;
  detail: string;
  actionLabel?: string;
}

// Turn the backend readiness snapshot into a display checklist. The
// Accessibility row only appears when auto-paste is on and the OS actually
// gates keystroke synthesis (macOS) — otherwise it isn't a prerequisite.
function buildReadinessItems(
  readiness: DictationReadiness | null,
  isPollingAccessibility: boolean,
): ReadinessRow[] {
  if (!readiness) {
    return [];
  }
  const rows: ReadinessRow[] = [
    {
      key: "model",
      ok: readiness.hasModel,
      title: "Transcription model",
      detail: readiness.hasModel
        ? "A local model is installed and ready."
        : "No model is installed yet — dictation can't transcribe without one.",
      actionLabel: "Download a model",
    },
    {
      key: "shortcut",
      ok: readiness.shortcutRegistered,
      title: "Dictation shortcut",
      detail: readiness.shortcutRegistered
        ? "Your global shortcut is active."
        : "No global shortcut is active, so hands-free dictation won't trigger.",
      actionLabel: "Set a shortcut",
    },
  ];
  if (readiness.accessibilityRequired) {
    rows.push({
      key: "accessibility",
      ok: readiness.accessibilityGranted,
      title: "Auto-paste access",
      detail: readiness.accessibilityGranted
        ? "Accessibility is granted — results paste straight into your app."
        : "Grant Accessibility so results paste into your app. Until then, dictation is copied to your clipboard for a manual paste.",
      actionLabel: isPollingAccessibility ? "Check again" : "Grant access",
    });
  }
  return rows;
}

// Map every internal state to one consistent, human-readable label shown in
// the status pill — the same vocabulary used across the HUD and toasts.
function liveStateLabel(state: string) {
  switch (state) {
    case "listening":
      return "Listening";
    case "paused":
      return "Paused";
    case "processing":
      return "Transcribing";
    case "success":
    case "inserted":
      return "Pasted";
    case "clipboard_only":
      return "Copied";
    case "error":
      return "Needs attention";
    default:
      return "Ready";
  }
}

function isFileQueueWorking(stage: FileQueueItem["stage"]) {
  return stage === "queued" || stage === "preparing" || stage === "transcribing" || stage === "diarizing" || stage === "saving";
}

function progressPercent(item: FileQueueItem) {
  if (item.stage === "saving" || item.stage === "completed") {
    return 100;
  }
  if (item.stage === "failed" || item.stage === "canceled") {
    return Math.max(item.progressPercent ?? 0, 0);
  }
  return Math.min(Math.max(item.progressPercent ?? 0, 0), 100);
}


function fileStageLabel(stage: FileQueueItem["stage"]) {
  switch (stage) {
    case "queued":
      return "Queued";
    case "preparing":
      return "Preparing";
    case "transcribing":
      return "Transcribing";
    case "diarizing":
      return "Identifying speakers";
    case "saving":
      return "Saving";
    case "completed":
      return "Done";
    case "failed":
      return "Failed";
    case "canceled":
      return "Canceled";
  }
}

function InlineSpeakerTranscript({ result }: { result: TranscriptResult }) {
  const name = (id: string) => result.speakers.find((speaker) => speaker.speakerId === id)?.displayName ?? id;
  return <>{result.speakers.length === 0 || result.segments.length === 0
    ? <p className="muted">{result.plainText}</p>
    : <div className="speaker-segment-list">{result.segments.map((segment) => {
      const label = segment.speakerAttribution === "assigned" && segment.speakerId
        ? name(segment.speakerId)
        : segment.speakerAttribution === "likely" && segment.speakerId
          ? `${name(segment.speakerId)}?`
        : segment.speakerAttribution === "overlap"
          ? (segment.speakerIds ?? []).map(name).join(" + ")
          : segment.speakerAttribution === "uncertain"
            ? "Uncertain speaker"
            : "Unknown speaker";
      const speakerId = segment.speakerId ?? segment.speakerIds?.[0];
      const color = Math.max(0, result.speakers.find((speaker) => speaker.speakerId === speakerId)?.speakerOrder ?? 0) % 6;
      return <div className="speaker-segment" key={segment.id}><span className={`speaker-label speaker-color-${color}`}>{label}</span><p>{segment.text}</p></div>;
    })}</div>}
  {result.diarizationWarning ? <p className="warning-text">{result.diarizationWarning}</p> : null}</>;
}
