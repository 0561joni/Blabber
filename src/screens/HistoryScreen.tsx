import { useEffect, useState } from "react";
import type { TranscriptSummary } from "../types/domain";

interface HistoryScreenProps {
  transcripts: TranscriptSummary[];
  onCopyTranscript: (text: string) => Promise<void>;
  onDelete: (transcriptId: string) => Promise<void>;
  onDeleteAll: () => Promise<void>;
}

export function HistoryScreen({
  transcripts,
  onCopyTranscript,
  onDelete,
  onDeleteAll,
}: HistoryScreenProps) {
  const total = transcripts.length;
  const [confirmDeleteAll, setConfirmDeleteAll] = useState(false);
  const [isDeletingAll, setIsDeletingAll] = useState(false);
  const [expandedIds, setExpandedIds] = useState<string[]>([]);
  const [copyStates, setCopyStates] = useState<Record<string, "copied" | "error">>({});

  useEffect(() => {
    if (Object.keys(copyStates).length === 0) {
      return;
    }
    const timeout = window.setTimeout(() => setCopyStates({}), 1800);
    return () => window.clearTimeout(timeout);
  }, [copyStates]);

  async function confirmDelete() {
    setIsDeletingAll(true);
    try {
      await onDeleteAll();
      setConfirmDeleteAll(false);
    } finally {
      setIsDeletingAll(false);
    }
  }

  function toggleExpanded(id: string) {
    setExpandedIds((current) =>
      current.includes(id) ? current.filter((entry) => entry !== id) : [...current, id],
    );
  }

  async function copyTranscript(id: string, text: string) {
    try {
      await onCopyTranscript(text);
      setCopyStates({ [id]: "copied" });
    } catch {
      setCopyStates({ [id]: "error" });
    }
  }

  return (
    <section className="screen">
      <div className="history-screen-layout">
        <article className="glass-panel history-toolbar-panel">
          <div className="section-header history-toolbar-header">
            <div>
              <p className="eyebrow">History</p>
              <h2>Saved transcripts</h2>
            </div>
            <span className="status-pill">{total} saved</span>
          </div>

          <div className="history-toolbar-actions">
            {transcripts.length > 0 ? (
              <button
                className="danger-button history-danger-button"
                disabled={isDeletingAll}
                onClick={() => setConfirmDeleteAll(true)}
              >
                Delete all history
              </button>
            ) : null}
          </div>

          {confirmDeleteAll ? (
            <div className="glass-subtle confirm-panel history-confirm-panel">
              <p className="transcript-title">Delete the entire history?</p>
              <p className="muted">
                This removes all saved transcripts and source-file references from local history.
              </p>
              <div className="toolbar action-segment">
                <button
                  className="danger-button"
                  disabled={isDeletingAll}
                  onClick={() => void confirmDelete()}
                >
                  {isDeletingAll ? "Deleting..." : "Yes, delete everything"}
                </button>
                <button disabled={isDeletingAll} onClick={() => setConfirmDeleteAll(false)}>
                  Cancel
                </button>
              </div>
            </div>
          ) : null}
        </article>

        <div className="history-list history-list-wide">
          {transcripts.length === 0 ? (
            <article className="glass-panel history-card empty-state">
              <div>
                <p className="eyebrow">No history yet</p>
                <h2>Your saved transcripts will appear here.</h2>
                <p className="muted">
                  Dictate once or transcribe an audio file to populate your saved history.
                </p>
              </div>
            </article>
          ) : (
            transcripts.map((transcript) => (
              <article className="glass-panel history-card history-card-wide" key={transcript.id}>
                <div className="history-card-main history-card-main-wide">
                  <div className="history-card-copy">
                    <p className="eyebrow">{transcript.sourceType.replace("_", " ")}</p>
                    <h2>{transcript.title}</h2>
                    <p className={expandedIds.includes(transcript.id) ? "muted" : "muted clamped-text"}>
                      {transcript.plainText}
                    </p>
                  </div>

                  <dl className="history-meta">
                    <div>
                      <dt>Created</dt>
                      <dd>{new Date(transcript.createdAt).toLocaleString()}</dd>
                    </div>
                    <div>
                      <dt>Status</dt>
                      <dd>{transcript.status}</dd>
                    </div>
                    <div>
                      <dt>Model</dt>
                      <dd>{transcript.modelName ?? "Unknown"}</dd>
                    </div>
                  </dl>
                </div>

                <div className="history-card-actions">
                  <button onClick={() => void copyTranscript(transcript.id, transcript.plainText)}>
                    {copyStates[transcript.id] === "copied"
                      ? "Copied"
                      : copyStates[transcript.id] === "error"
                        ? "Copy failed"
                        : "Copy transcript"}
                  </button>
                  <button onClick={() => toggleExpanded(transcript.id)}>
                    {expandedIds.includes(transcript.id) ? "Show less" : "Show full text"}
                  </button>
                  <span className="language-chip">
                    {transcript.durationMs ? `${(transcript.durationMs / 1000).toFixed(1)}s` : "n/a"}
                  </span>
                  <button onClick={() => void onDelete(transcript.id)}>Delete</button>
                </div>
              </article>
            ))
          )}
        </div>
      </div>
    </section>
  );
}
