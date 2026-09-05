import { useRef } from "react";
import {
  ActionButton,
  Button,
  PageHeader,
  Progress,
} from "../components/Feedback";
import { AppIcon } from "../components/IconButton";
import { TranscriptReader } from "../components/TranscriptReader";
import { copyTextToClipboard } from "../lib/api";
import { copyReview } from "../lib/reviewApi";
import { formatDuration } from "../lib/formatting";
import type { FileQueueItem } from "../types/domain";

export const isFileWorking = (stage: FileQueueItem["stage"]) =>
  ["queued", "preparing", "transcribing", "diarizing", "saving"].includes(
    stage,
  );
const labels: Record<FileQueueItem["stage"], string> = {
  queued: "Queued",
  preparing: "Preparing",
  transcribing: "Transcribing",
  diarizing: "Identifying speakers",
  saving: "Saving",
  completed: "Transcript ready",
  failed: "Needs attention",
  canceled: "Canceled",
};

interface Props {
  modelReady?: boolean;
  speakerMode?: string;
  onReview?: (item: FileQueueItem) => void;
  onDismiss?: (id: string) => Promise<void>;
  onResolveModel?: () => void;
  items: FileQueueItem[];
  dragging: boolean;
  speakerCountHint: number | null;
  showSpeakerOptions: boolean;
  onSpeakerCountHintChange: (value: number | null) => void;
  onDragChange: (active: boolean) => void;
  onPick: () => Promise<void>;
  onDrop: (files: FileList) => Promise<void>;
  onToggle: (id: string) => void;
  onCancel: (id: string) => Promise<void>;
  onRetry: (id: string) => Promise<void>;
}

export function FilesScreen(props: Props) {
  const dragDepth = useRef(0);
  const activeCount = props.items.filter((item) =>
    isFileWorking(item.stage),
  ).length;
  return (
    <section className="screen files-screen">
      <PageHeader
        eyebrow="FROM AUDIO TO SOMETHING USEFUL"
        title="Transcribe files"
        description="Turn recordings into words you can work with."
      >
        <span className="local-badge">
          <span /> On-device transcription
        </span>
      </PageHeader>
      {props.modelReady === false ? (
        <aside className="setup-panel">
          <div className="setup-row">
            <span>Download a speech model before transcribing files.</span>
            <Button onClick={props.onResolveModel}>Download a model</Button>
          </div>
        </aside>
      ) : null}
      <div
        className={
          "file-dropzone surface" + (props.dragging ? " is-dragging" : "")
        }
        onDragEnter={(event) => {
          event.preventDefault();
          dragDepth.current += 1;
          props.onDragChange(true);
        }}
        onDragOver={(event) => event.preventDefault()}
        onDragLeave={(event) => {
          event.preventDefault();
          dragDepth.current = Math.max(0, dragDepth.current - 1);
          if (!dragDepth.current) props.onDragChange(false);
        }}
        onDrop={(event) => {
          event.preventDefault();
          dragDepth.current = 0;
          props.onDragChange(false);
          if (event.dataTransfer.files.length)
            void props.onDrop(event.dataTransfer.files);
        }}
      >
        <div className="dropzone-icon">
          <AppIcon name="upload" />
        </div>
        <h2>
          {props.dragging
            ? "Drop to start transcribing"
            : "Good conversations deserve a second look."}
        </h2>
        <p className="muted">
          Drop your audio files here, or choose them from your device.
        </p>
        <ActionButton
          variant="primary"
          icon="folder"
          action={props.onPick}
          success=""
          disabled={props.modelReady === false}
        >
          Choose files
        </ActionButton>
        <span className="file-types">WAV · MP3 · M4A · OPUS</span>
      </div>
      {props.speakerMode ? (
        <p className="muted" role="status">
          {props.speakerMode}
        </p>
      ) : null}
      {props.showSpeakerOptions ? (
        <details className="surface file-options">
          <summary>
            Speaker options{" "}
            <span className="muted">
              {props.speakerCountHint === null
                ? "Automatic"
                : props.speakerCountHint + " speakers (exact)"}
            </span>
          </summary>
          <label className="field-stack">
            <span>Known speaker count</span>
            <select
              aria-label="Known speaker count"
              value={props.speakerCountHint === null ? "auto" : "estimate"}
              onChange={(event) =>
                props.onSpeakerCountHintChange(
                  event.target.value === "auto" ? null : 2,
                )
              }
            >
              <option value="auto">Detect automatically</option>
              <option value="estimate">Use an exact count</option>
            </select>
          </label>
          {props.speakerCountHint !== null ? (
            <label className="field-stack">
              <span>Exact number of speakers</span>
              <input
                type="number"
                min={1}
                max={20}
                value={props.speakerCountHint}
                onChange={(event) =>
                  props.onSpeakerCountHintChange(
                    Math.min(20, Math.max(1, Number(event.target.value) || 1)),
                  )
                }
              />
            </label>
          ) : null}
          <p className="muted">
            Use an exact count only when you know how many people are speaking.
          </p>
        </details>
      ) : null}
      <div className="section-header queue-heading">
        <h2>
          Your queue <span className="count-label">{props.items.length}</span>
        </h2>
        <span className="muted">
          {activeCount ? activeCount + " in progress" : "Ready when you are"}
        </span>
      </div>
      {props.items.length === 0 ? (
        <div className="empty-state surface">
          <AppIcon name="folder" />
          <h3>A place for your recordings</h3>
          <p className="muted">
            Files you add will appear here with their progress and transcript.
          </p>
        </div>
      ) : (
        <div className="file-queue-list">
          {props.items.map((item) => (
            <article
              className={"surface queue-item stage-" + item.stage}
              key={item.id}
            >
              <div className="queue-item-heading">
                <span className="file-icon">
                  <AppIcon
                    name={item.stage === "completed" ? "check" : "folder"}
                  />
                </span>
                <div className="queue-item-copy">
                  <h3>{item.sourceFile.originalName}</h3>
                  <span className="muted">
                    {Math.max(0.1, item.sourceFile.sizeBytes / 1048576).toFixed(
                      1,
                    )}{" "}
                    MB
                    {item.sourceFile.durationMs
                      ? " · " +
                        formatDuration(item.sourceFile.durationMs / 1000)
                      : ""}
                  </span>
                </div>
                <span
                  className={"status-pill stage-" + item.stage}
                  role="status"
                >
                  {(item.result || item.reviewRef) && item.stage === "diarizing"
                    ? "Text ready · Identifying speakers"
                    : labels[item.stage]}
                </span>
              </div>
              {isFileWorking(item.stage) ? (
                <div className="queue-progress">
                  <div className="progress-meta">
                    <span>{item.statusText}</span>
                    <span>
                      {item.progressPercent == null
                        ? "Working…"
                        : Math.round(item.progressPercent) + "%"}
                      {item.etaSeconds != null
                        ? " · About " +
                          formatDuration(item.etaSeconds) +
                          " left"
                        : ""}
                    </span>
                  </div>
                  <Progress
                    value={item.progressPercent}
                    label={"Transcribing " + item.sourceFile.originalName}
                  />
                </div>
              ) : null}
              {item.stage === "failed" && item.errorMessage ? (
                <p className="error-text" role="alert">
                  {item.errorMessage}
                </p>
              ) : null}
              {item.stage === "canceled" ? (
                <p className="muted">
                  Transcription canceled. You can try again when you’re ready.
                </p>
              ) : null}
              <div className="queue-actions">
                {isFileWorking(item.stage) ? (
                  <ActionButton
                    variant="ghost"
                    action={() => props.onCancel(item.id)}
                    success="Cancellation requested"
                  >
                    {(item.result || item.reviewRef) &&
                    item.stage === "diarizing"
                      ? "Stop identifying speakers"
                      : "Cancel"}
                  </ActionButton>
                ) : null}
                {item.stage === "failed" || item.stage === "canceled" ? (
                  <ActionButton
                    icon="retry"
                    action={() => props.onRetry(item.id)}
                    success="Queued again"
                  >
                    Retry transcription
                  </ActionButton>
                ) : null}
                {item.result || (props.onReview && item.reviewRef) ? (
                  <>
                    <Button
                      icon={item.isExpanded ? "chevronLeft" : "disclosure"}
                      aria-expanded={
                        props.onReview ? undefined : item.isExpanded
                      }
                      onClick={() =>
                        props.onReview
                          ? props.onReview(item)
                          : props.onToggle(item.id)
                      }
                    >
                      {props.onReview
                        ? "Review transcript"
                        : item.isExpanded
                          ? "Hide transcript"
                          : "Read transcript"}
                    </Button>
                    <ActionButton
                      icon="copy"
                      action={() =>
                        item.reviewRef
                          ? copyReview(item.reviewRef, "plain")
                          : copyTextToClipboard(item.result!.result.plainText)
                      }
                      success="Copied"
                    >
                      Copy text
                    </ActionButton>
                    {item.reviewRef?.kind === "saved" ||
                    item.result?.savedTranscript ? (
                      <span className="muted">Saved to Library</span>
                    ) : (
                      <span className="muted">Not saved to Library</span>
                    )}
                  </>
                ) : null}
              </div>
              {props.onDismiss && !isFileWorking(item.stage) ? (
                <div className="queue-actions">
                  <ActionButton
                    variant="ghost"
                    action={() => props.onDismiss!(item.id)}
                    success=""
                  >
                    {item.reviewRef?.kind === "session" ||
                    (item.result && !item.result.savedTranscript)
                      ? "Dismiss session result"
                      : "Dismiss from queue"}
                  </ActionButton>
                  {item.reviewRef?.kind === "session" ||
                  (item.result && !item.result.savedTranscript) ? (
                    <span className="muted">
                      Dismissing removes this unsaved transcript.
                    </span>
                  ) : null}
                </div>
              ) : null}
              {!props.onReview && item.isExpanded && item.result ? (
                <TranscriptReader result={item.result.result} />
              ) : null}
            </article>
          ))}
        </div>
      )}
    </section>
  );
}
