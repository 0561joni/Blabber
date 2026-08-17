import { useEffect, useState } from "react";
import { copyTextToClipboard } from "../lib/api";
import { IconButton } from "./IconButton";
import type {
  FileTranscriptionJobStage,
  FileTranscriptionStatusEvent,
  FileTranscriptionResponse,
  SelectedSourceFile,
} from "../types/domain";

interface FileTranscribePanelProps {
  selectedFile: SelectedSourceFile | null;
  transcription: FileTranscriptionResponse | null;
  jobStatus: FileTranscriptionStatusEvent | null;
  elapsedMs: number | null;
  errorMessage: string | null;
  onPickFile: () => Promise<void>;
  onTranscribe: () => Promise<void>;
}

export function FileTranscribePanel({
  selectedFile,
  transcription,
  jobStatus,
  elapsedMs,
  errorMessage,
  onPickFile,
  onTranscribe,
}: FileTranscribePanelProps) {
  const [showFullTranscript, setShowFullTranscript] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const isTranscribing =
    jobStatus?.stage === "queued" ||
    jobStatus?.stage === "preparing" ||
    jobStatus?.stage === "transcribing" ||
    jobStatus?.stage === "saving";

  useEffect(() => {
    setShowFullTranscript(false);
    setCopyState("idle");
  }, [transcription?.result.jobId]);

  useEffect(() => {
    if (copyState === "idle") {
      return;
    }
    const timeoutId = window.setTimeout(() => setCopyState("idle"), 1800);
    return () => window.clearTimeout(timeoutId);
  }, [copyState]);

  async function handleCopyTranscript(text: string) {
    try {
      await copyTextToClipboard(text);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
  }

  return (
    <div className="file-transcribe-panel">
      <div className="section-header section-header-compact">
        <div>
          <p className="eyebrow">File transcribe</p>
          <h2>Transcribe one local audio file.</h2>
        </div>
      </div>

      <div className="file-panel-actions">
        <IconButton
          icon="folder"
          label="Choose audio file"
          disabled={isTranscribing}
          onClick={() => void onPickFile()}
        />
        <IconButton
          icon="microphoneActive"
          label={isTranscribing ? "Transcribing file" : "Transcribe file"}
          state={isTranscribing ? "busy" : "default"}
          disabled={!selectedFile || isTranscribing}
          onClick={() => void onTranscribe()}
        />
      </div>

      {jobStatus ? (
        <div className={`job-progress-panel glass-subtle ${jobStatus.stage}`}>
          <div className="progress-header">
            <span className={`status-pill progress-pill stage-${jobStatus.stage}`}>
              {stageLabel(jobStatus.stage)}
            </span>
            <span className="muted">
              {jobStatus.stage === "completed"
                ? "Local pass finished"
                : jobStatus.stage === "failed"
                  ? "Review the error below"
                  : "Running locally"}
            </span>
          </div>
          <p className="progress-copy">{jobStatus.statusText}</p>
          <div className="progress-track" aria-hidden="true">
            <div
              className={
                jobStatus.stage === "queued" || jobStatus.stage === "preparing"
                  ? "progress-fill indeterminate"
                  : "progress-fill"
              }
              style={
                jobStatus.stage === "queued" || jobStatus.stage === "preparing"
                  ? undefined
                  : { width: `${resolvedProgress(jobStatus)}%` }
              }
            />
          </div>
          <div className="progress-meta">
            <span>Elapsed {formatDuration(elapsedMs)}</span>
            <span>
              ETA{" "}
              {jobStatus.etaSeconds != null ? formatEta(jobStatus.etaSeconds) : "Estimating..."}
            </span>
            <span>
              {jobStatus.progressPercent != null
                ? `${Math.round(resolvedProgress(jobStatus))}% complete`
                : "Analyzing file"}
            </span>
          </div>
        </div>
      ) : null}

      {selectedFile ? (
        <div className="file-summary glass-subtle">
          <div>
            <p className="eyebrow">Selected file</p>
            <p className="transcript-title">{selectedFile.originalName}</p>
            <p className="muted">{selectedFile.filePath}</p>
          </div>
          <dl className="meta-list">
            <div>
              <dt>Type</dt>
              <dd>{selectedFile.mimeType}</dd>
            </div>
            <div>
              <dt>Size</dt>
              <dd>{formatBytes(selectedFile.sizeBytes)}</dd>
            </div>
            <div>
              <dt>Status</dt>
              <dd>{isTranscribing ? "Running locally" : "Ready to transcribe"}</dd>
            </div>
          </dl>
        </div>
      ) : (
        <div className="glass-subtle compact-panel">
          <p className="muted">
            No file selected yet. Choose a local audio file to start a one-shot transcription.
          </p>
        </div>
      )}

      {errorMessage ? <p className="error-text">{errorMessage}</p> : null}

      {transcription ? (
        <div className="file-transcribe-result-stack">
          <div className="result-panel glass-subtle embedded-result-panel">
            <div className="section-header section-header-compact">
              <div>
                <p className="eyebrow">Transcript</p>
                <h2>{transcription.sourceFile.originalName}</h2>
              </div>
              <span className="language-chip">
                {transcription.resolvedModel?.modelName ?? transcription.result.modelName}
              </span>
            </div>
            <div className="text-surface">
              <p className={showFullTranscript ? undefined : "clamped-text"}>
                {transcription.result.plainText}
              </p>
            </div>
            {transcription.result.qualityStatus !== "clean" ? (
              <div
                className={`transcript-quality-notice quality-${transcription.result.qualityStatus}`}
                role="status"
              >
                <strong>
                  {transcription.result.qualityStatus === "partial"
                    ? "Completed with a section that needs review"
                    : `Recovered ${transcription.result.recoveredRegionCount} problematic ${
                        transcription.result.recoveredRegionCount === 1 ? "section" : "sections"
                      }`}
                </strong>
                <p>
                  {transcription.result.qualityStatus === "partial"
                    ? "Blabber preserved the rest of the transcript and inserted a timestamped marker where decoding could not be recovered."
                    : "Blabber reset the decoder and continued without changing the selected model."}
                </p>
                {transcription.result.warnings.length > 0 ? (
                  <ul>
                    {transcription.result.warnings.map((warning, index) => (
                      <li key={`${warning.startMs}:${warning.endMs}:${index}`}>
                        {formatTimestampRange(warning.startMs, warning.endMs)} · {warning.outcome.split("_").join(" ")}
                      </li>
                    ))}
                  </ul>
                ) : null}
              </div>
            ) : null}
            <div className="toolbar">
              <IconButton
                icon={copyState === "copied" ? "check" : copyState === "error" ? "xmark" : "copy"}
                label={copyState === "copied" ? "Transcript copied" : copyState === "error" ? "Copy failed" : "Copy transcript"}
                state={copyState === "copied" ? "success" : copyState === "error" ? "error" : "default"}
                onClick={() => void handleCopyTranscript(transcription.result.plainText)}
              />
              <IconButton
                className="disclosure-icon-button"
                icon="disclosure"
                label={showFullTranscript ? "Show less" : "Show full text"}
                aria-expanded={showFullTranscript}
                onClick={() => setShowFullTranscript((current) => !current)}
              />
              <span className="language-chip">
                {transcription.result.detectedLanguages.join(", ") || "No language tags"}
              </span>
            </div>
            {showFullTranscript ? (
              <pre className="preview-block">{transcription.result.timestampedText}</pre>
            ) : null}
          </div>

          <div className="support-panel glass-subtle embedded-support-panel">
            <div className="section-header section-header-compact">
              <div>
                <p className="eyebrow">Storage</p>
                <h2>Result status</h2>
              </div>
            </div>
            <div className="meta-list">
              <div>
                <dt>Duration</dt>
                <dd>
                  {transcription.sourceFile.durationMs
                    ? `${(transcription.sourceFile.durationMs / 1000).toFixed(1)}s`
                    : "Unknown"}
                </dd>
              </div>
              <div>
                <dt>History</dt>
                <dd>
                  {transcription.savedTranscript
                    ? "Saved to local history"
                    : "Not saved because history is disabled"}
                </dd>
              </div>
              <div>
                <dt>SHA-256</dt>
                <dd>{transcription.sourceFile.sha256 ?? "Not available"}</dd>
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function formatBytes(value: number) {
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}

function resolvedProgress(jobStatus: FileTranscriptionStatusEvent) {
  if (jobStatus.stage === "saving" || jobStatus.stage === "completed") {
    return 100;
  }
  if (jobStatus.stage === "failed") {
    return Math.max(jobStatus.progressPercent ?? 0, 0);
  }
  return Math.min(Math.max(jobStatus.progressPercent ?? 0, 0), 100);
}

function stageLabel(stage: FileTranscriptionJobStage) {
  switch (stage) {
    case "queued":
      return "Queued";
    case "preparing":
      return "Preparing";
    case "transcribing":
      return "Transcribing";
    case "saving":
      return "Saving";
    case "completed":
      return "Done";
    case "failed":
      return "Failed";
  }
}

function formatDuration(value: number | null) {
  if (!value || value <= 0) {
    return "0s";
  }
  const totalSeconds = Math.max(1, Math.floor(value / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return minutes > 0 ? `${minutes}m ${seconds}s` : `${seconds}s`;
}

function formatEta(seconds: number) {
  if (seconds <= 0) {
    return "0s";
  }
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return minutes > 0 ? `${minutes}m ${remainder}s` : `${remainder}s`;
}

function formatTimestampRange(startMs: number, endMs: number) {
  return `${formatClock(startMs)}–${formatClock(endMs)}`;
}

function formatClock(valueMs: number) {
  const totalSeconds = Math.max(0, Math.floor(valueMs / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
    : `${minutes}:${seconds.toString().padStart(2, "0")}`;
}
