import { useEffect, useRef, useState } from "react";
import {
  listTranscripts,
  cancelRediarization,
  copyTranscript,
  exportTranscript,
  getTranscript,
  listenRediarizationStatus,
  pickAudioFiles,
  rediarizeTranscript,
  renameTranscript,
  renameTranscriptSpeaker,
} from "../lib/api";
import { Button, PageHeader } from "../components/Feedback";
import { AppIcon, IconButton } from "../components/IconButton";
import { getFriendlyModelName } from "../lib/modelPresentation";
import type {
  TranscriptDetail,
  TranscriptExportFormat,
  TranscriptSegment,
  TranscriptSpeaker,
  TranscriptSummary,
} from "../types/domain";

interface HistoryScreenProps {
  transcripts: TranscriptSummary[];
  onTranscriptUpdated: (transcript: TranscriptSummary) => void;
  onDelete: (transcriptId: string) => Promise<void>;
  onDeleteAll: () => Promise<void>;
}

type RenameEditorState =
  | {
      kind: "transcript";
      transcriptId: string;
      anchorKey: string;
      value: string;
      originalValue: string;
    }
  | {
      kind: "speaker";
      transcriptId: string;
      speakerId: string;
      anchorKey: string;
      value: string;
      originalValue: string;
    };

export function HistoryScreen({
  transcripts,
  onTranscriptUpdated,
  onDelete,
  onDeleteAll,
}: HistoryScreenProps) {
  const [query, setQuery] = useState("");
  const [sourceFilter, setSourceFilter] = useState<
    "all" | "quick_dictate" | "file_upload"
  >("all");
  const [searchResults, setSearchResults] = useState<
    TranscriptSummary[] | null
  >(null);
  const [searchError, setSearchError] = useState("");
  const [searching, setSearching] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const readerRef = useRef<HTMLDivElement>(null);
  const listRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  useEffect(() => {
    if (selectedId) readerRef.current?.focus({ preventScroll: true });
  }, [selectedId]);
  const [confirmDeleteAll, setConfirmDeleteAll] = useState(false);
  const [isDeletingAll, setIsDeletingAll] = useState(false);
  const [expandedIds, setExpandedIds] = useState<string[]>([]);
  const [details, setDetails] = useState<Record<string, TranscriptDetail>>({});
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [messages, setMessages] = useState<Record<string, string>>({});
  const [exportingTranscriptId, setExportingTranscriptId] = useState<
    string | null
  >(null);
  const [openExportMenuId, setOpenExportMenuId] = useState<string | null>(null);
  const [openRetryMenuId, setOpenRetryMenuId] = useState<string | null>(null);
  const [retryCounts, setRetryCounts] = useState<Record<string, number>>({});
  const [rediarizingId, setRediarizingId] = useState<string | null>(null);
  const [rediarizationJobId, setRediarizationJobId] = useState<string | null>(
    null,
  );
  const [renameEditor, setRenameEditor] = useState<RenameEditorState | null>(
    null,
  );
  const [isSavingRename, setIsSavingRename] = useState(false);
  const exportInFlight = useRef(false);
  const exportButtonRefs = useRef<Record<string, HTMLButtonElement | null>>({});

  useEffect(() => {
    let disposed = false;
    if (!query.trim()) {
      setSearchResults(null);
      setSearching(false);
      setSearchError("");
      return;
    }
    setSearching(true);
    const timeout = window.setTimeout(() => {
      void listTranscripts(query)
        .then((results) => {
          if (!disposed) {
            setSearchResults(results);
            setSearchError("");
          }
        })
        .catch((error) => {
          if (!disposed)
            setSearchError(
              error instanceof Error ? error.message : "Search unavailable.",
            );
        })
        .finally(() => {
          if (!disposed) setSearching(false);
        });
    }, 200);
    return () => {
      disposed = true;
      window.clearTimeout(timeout);
    };
  }, [query, transcripts]);

  const visibleTranscripts = (searchResults ?? transcripts).filter(
    (item) =>
      (sourceFilter === "all" || item.sourceType === sourceFilter) &&
      (!query.trim() ||
        (item.title + " " + item.plainText)
          .toLowerCase()
          .includes(query.trim().toLowerCase())),
  );
  const selectedTranscript =
    transcripts.find((item) => item.id === selectedId) ??
    searchResults?.find((item) => item.id === selectedId);

  useEffect(() => {
    if (
      selectedId &&
      !transcripts.some((item) => item.id === selectedId) &&
      !searchResults?.some((item) => item.id === selectedId)
    )
      setSelectedId(null);
  }, [transcripts, searchResults, selectedId]);

  useEffect(() => {
    if (!openExportMenuId) return;
    const closeOnOutsideClick = (event: MouseEvent) => {
      if (
        !(event.target instanceof Element) ||
        !event.target.closest("[data-export-menu-root]")
      ) {
        setOpenExportMenuId(null);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setOpenExportMenuId(null);
      exportButtonRefs.current[openExportMenuId]?.focus();
    };
    document.addEventListener("mousedown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("mousedown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [openExportMenuId]);

  useEffect(() => {
    if (!openRetryMenuId) return;
    const close = (event: MouseEvent) => {
      if (
        !(event.target instanceof Element) ||
        !event.target.closest("[data-retry-speakers-root]")
      ) {
        setOpenRetryMenuId(null);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpenRetryMenuId(null);
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [openRetryMenuId]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenRediarizationStatus((status) => {
      setMessages({
        [status.transcriptId]: status.errorMessage ?? status.statusText,
      });
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  async function confirmDelete() {
    setIsDeletingAll(true);
    try {
      await onDeleteAll();
      setConfirmDeleteAll(false);
      setSelectedId(null);
      setSearchResults(null);
    } catch (error) {
      setSearchError(
        error instanceof Error
          ? error.message
          : "Could not delete transcripts.",
      );
    } finally {
      setIsDeletingAll(false);
    }
  }

  async function toggleExpanded(transcriptId: string) {
    setSelectedId(transcriptId);
    setExpandedIds([transcriptId]);
    setRenameEditor(null);
    setOpenExportMenuId(null);
    setOpenRetryMenuId(null);
    if (!details[transcriptId]) {
      setBusy((current) => ({ ...current, [transcriptId]: true }));
      try {
        const detail = await getTranscript(transcriptId);
        setDetails((current) => ({ ...current, [transcriptId]: detail }));
      } catch (error) {
        setMessages({
          [transcriptId]:
            error instanceof Error
              ? error.message
              : "Failed to load transcript.",
        });
      } finally {
        setBusy((current) => ({ ...current, [transcriptId]: false }));
      }
    }
  }

  function beginRenameTitle(transcript: TranscriptSummary) {
    setRenameEditor({
      kind: "transcript",
      transcriptId: transcript.id,
      anchorKey: `title:${transcript.id}`,
      value: transcript.title,
      originalValue: transcript.title,
    });
  }

  function beginRenameSpeaker(
    transcriptId: string,
    speaker: TranscriptSpeaker,
    anchorKey: string,
  ) {
    setRenameEditor({
      kind: "speaker",
      transcriptId,
      speakerId: speaker.speakerId,
      anchorKey,
      value: speaker.displayName,
      originalValue: speaker.displayName,
    });
  }

  async function saveRename() {
    if (!renameEditor || isSavingRename) return;
    const value = renameEditor.value.trim();
    if (!value) return;
    if (value === renameEditor.originalValue) {
      setRenameEditor(null);
      return;
    }
    setIsSavingRename(true);
    try {
      if (renameEditor.kind === "transcript") {
        const updated = await renameTranscript(
          renameEditor.transcriptId,
          value,
        );
        onTranscriptUpdated(updated);
        setDetails((current) =>
          current[renameEditor.transcriptId]
            ? {
                ...current,
                [renameEditor.transcriptId]: {
                  ...current[renameEditor.transcriptId],
                  ...updated,
                },
              }
            : current,
        );
        setMessages({ [renameEditor.transcriptId]: "Transcript renamed" });
      } else {
        const detail = await renameTranscriptSpeaker(
          renameEditor.transcriptId,
          renameEditor.speakerId,
          value,
        );
        setDetails((current) => ({
          ...current,
          [renameEditor.transcriptId]: detail,
        }));
        setMessages({ [renameEditor.transcriptId]: "Speaker renamed" });
      }
      setRenameEditor(null);
    } catch (error) {
      setMessages({
        [renameEditor.transcriptId]:
          error instanceof Error ? error.message : "Rename failed.",
      });
    } finally {
      setIsSavingRename(false);
    }
  }

  async function handleCopy(
    transcriptId: string,
    variant: "speaker_aware" | "plain",
  ) {
    try {
      await copyTranscript(transcriptId, variant);
      setMessages({
        [transcriptId]:
          variant === "plain"
            ? "Plain text copied"
            : "Speaker transcript copied",
      });
    } catch (error) {
      setMessages({
        [transcriptId]: error instanceof Error ? error.message : "Copy failed.",
      });
    }
  }

  async function handleExport(
    transcriptId: string,
    format: TranscriptExportFormat,
  ) {
    if (!format || exportInFlight.current) return;
    setOpenExportMenuId(null);
    exportInFlight.current = true;
    setExportingTranscriptId(transcriptId);
    try {
      const result = await exportTranscript(transcriptId, format);
      setMessages({
        [transcriptId]: result.path
          ? `Exported ${format.toUpperCase()}`
          : "Export canceled",
      });
    } catch (error) {
      setMessages({
        [transcriptId]:
          error instanceof Error ? error.message : "Export failed.",
      });
    } finally {
      exportInFlight.current = false;
      setExportingTranscriptId(null);
    }
  }

  async function loadDetail(transcriptId: string) {
    if (details[transcriptId]) return details[transcriptId];
    const detail = await getTranscript(transcriptId);
    setDetails((current) => ({ ...current, [transcriptId]: detail }));
    return detail;
  }

  async function handleRetrySpeakers(
    transcript: TranscriptSummary,
    speakerCountHint: number | null,
  ) {
    if (rediarizingId) return;
    setOpenRetryMenuId(null);
    setMessages({ [transcript.id]: "Checking the source audio…" });
    try {
      const currentDetail = await loadDetail(transcript.id);
      const renamed = currentDetail.speakers.some(
        (speaker) =>
          speaker.displayName !== `Speaker ${speaker.speakerOrder + 1}`,
      );
      if (
        renamed &&
        !window.confirm(
          "Retrying speaker identification will reset your speaker names. Continue?",
        )
      ) {
        setMessages({ [transcript.id]: "Speaker retry canceled" });
        return;
      }
      setRediarizingId(transcript.id);
      const jobId = crypto.randomUUID();
      setRediarizationJobId(jobId);
      setMessages({ [transcript.id]: "Identifying speakers locally…" });
      let updated: TranscriptDetail;
      try {
        updated = await rediarizeTranscript({
          jobId,
          transcriptId: transcript.id,
          sourceFile: null,
          speakerCountHint,
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (!message.includes("SOURCE_FILE_REQUIRED")) throw error;
        const [sourceFile] = await pickAudioFiles();
        if (!sourceFile) {
          setMessages({ [transcript.id]: "No source audio selected" });
          return;
        }
        const replacementJobId = crypto.randomUUID();
        setRediarizationJobId(replacementJobId);
        updated = await rediarizeTranscript({
          jobId: replacementJobId,
          transcriptId: transcript.id,
          sourceFile,
          speakerCountHint,
        });
      }
      setDetails((current) => ({ ...current, [transcript.id]: updated }));
      onTranscriptUpdated(updated);
      setMessages({
        [transcript.id]: `Speaker identification updated · ${updated.speakerCount ?? 0} speakers`,
      });
    } catch (error) {
      const raw = error instanceof Error ? error.message : String(error);
      const message = raw.replace(/^SOURCE_FILE_(?:REQUIRED|MISMATCH):\s*/, "");
      setMessages({
        [transcript.id]: message || "Speaker identification failed.",
      });
    } finally {
      setRediarizingId(null);
      setRediarizationJobId(null);
    }
  }

  async function handleCancelSpeakerRetry(transcriptId: string) {
    if (!rediarizationJobId) return;
    try {
      await cancelRediarization(rediarizationJobId);
      setMessages({ [transcriptId]: "Canceling speaker retry…" });
    } catch (error) {
      setMessages({
        [transcriptId]:
          error instanceof Error
            ? error.message
            : "Could not cancel speaker retry.",
      });
    }
  }

  return (
    <section className="screen library-screen">
      <div className="history-screen-layout">
        <article className="glass-panel history-toolbar-panel">
          <PageHeader
            eyebrow="A HOME FOR YOUR WORDS"
            title="Library"
            description="Find a thought. Pick up where you left off."
          >
            <span className="status-pill">{transcripts.length} saved</span>
          </PageHeader>
          <div className="history-toolbar-actions">
            <label className="search-field">
              <AppIcon name="book" />
              <input
                type="search"
                aria-label="Search transcripts"
                placeholder="Search your transcripts…"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
            <div className="segmented-control" aria-label="Filter transcripts">
              {(
                [
                  ["all", "All"],
                  ["quick_dictate", "Dictation"],
                  ["file_upload", "Files"],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  aria-pressed={sourceFilter === value}
                  onClick={() => setSourceFilter(value)}
                >
                  {label}
                </button>
              ))}
            </div>
            {transcripts.length > 0 ? (
              <IconButton
                icon="trashMultiple"
                label="Delete all history"
                tone="danger"
                state={isDeletingAll ? "busy" : "default"}
                disabled={isDeletingAll}
                onClick={() => setConfirmDeleteAll(true)}
              />
            ) : null}
          </div>
          {confirmDeleteAll ? (
            <div className="glass-subtle confirm-panel history-confirm-panel">
              <p className="transcript-title">Delete the entire history?</p>
              <p className="muted">
                This removes all saved transcripts and speaker metadata from
                local history.
              </p>
              <div className="toolbar action-segment">
                <button
                  className="danger-button"
                  disabled={isDeletingAll}
                  onClick={() => void confirmDelete()}
                >
                  {isDeletingAll ? "Deleting..." : "Yes, delete everything"}
                </button>
                <button
                  disabled={isDeletingAll}
                  onClick={() => setConfirmDeleteAll(false)}
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : null}
        </article>

        {searchError ? (
          <p className="error-text" role="alert">
            {searchError}
          </p>
        ) : null}
        <div
          className={"library-layout" + (selectedId ? " has-selection" : "")}
        >
          <aside
            className="library-list surface"
            aria-label="Saved transcripts"
          >
            <div className="library-list-header" role="status">
              {searching
                ? "Searching…"
                : visibleTranscripts.length + " transcripts"}
            </div>
            {visibleTranscripts.map((item) => (
              <button
                className={
                  "library-row" + (selectedId === item.id ? " is-selected" : "")
                }
                key={item.id}
                ref={(node) => {
                  listRefs.current[item.id] = node;
                }}
                aria-current={selectedId === item.id ? "true" : undefined}
                aria-label={"Open transcript " + item.title}
                onClick={() => void toggleExpanded(item.id)}
              >
                <span className="library-row-meta">
                  <AppIcon
                    name={
                      item.sourceType === "file_upload"
                        ? "folder"
                        : "microphone"
                    }
                  />
                  {new Date(item.createdAt).toLocaleDateString(undefined, {
                    month: "short",
                    day: "numeric",
                  })}
                </span>
                <strong>{item.title}</strong>
                <span className="clamped-text muted">{item.plainText}</span>
              </button>
            ))}
            {visibleTranscripts.length === 0 ? (
              <div className="empty-state">
                <AppIcon name="book" />
                <h3>
                  {query || sourceFilter !== "all"
                    ? "No matching transcripts"
                    : "Your words belong here"}
                </h3>
                <p className="muted">
                  {query || sourceFilter !== "all"
                    ? "Try a different search or filter."
                    : "Saved dictations and files will appear here."}
                </p>
              </div>
            ) : null}
          </aside>
          <div
            className="library-reader"
            ref={readerRef}
            tabIndex={-1}
            role="region"
            aria-label="Transcript reader"
          >
            {selectedId ? (
              <Button
                className="library-back"
                icon="chevronLeft"
                onClick={() => {
                  const id = selectedId;
                  setSelectedId(null);
                  window.requestAnimationFrame(() =>
                    listRefs.current[id]?.focus(),
                  );
                }}
              >
                Back to transcripts
              </Button>
            ) : null}
            {!selectedTranscript ? (
              <article className="glass-panel history-card empty-state">
                <AppIcon name="book" />
                <div>
                  <h2>A little space to reflect.</h2>
                  <p className="muted">
                    Choose a transcript to read, copy, or export.
                  </p>
                </div>
              </article>
            ) : (
              [selectedTranscript].map((transcript) => {
                const expanded = expandedIds.includes(transcript.id);
                const detail = details[transcript.id];
                return (
                  <article
                    className="glass-panel history-card history-card-wide"
                    key={transcript.id}
                  >
                    <div className="history-card-main history-card-main-wide">
                      <div className="history-card-copy">
                        <p className="eyebrow">
                          {transcript.sourceType === "file_upload"
                            ? "File transcript"
                            : "Dictation"}
                        </p>
                        <div className="transcript-title-row title-with-actions">
                          {renameEditor?.anchorKey ===
                          `title:${transcript.id}` ? (
                            <InlineRenameEditor
                              inputLabel="Transcript title"
                              value={renameEditor.value}
                              isSaving={isSavingRename}
                              onChange={(value) =>
                                setRenameEditor((current) =>
                                  current ? { ...current, value } : current,
                                )
                              }
                              onSave={() => void saveRename()}
                              onCancel={() => setRenameEditor(null)}
                            />
                          ) : (
                            <>
                              <h2>{transcript.title}</h2>
                              <IconButton
                                icon="pencil"
                                size="compact"
                                label="Rename transcript"
                                onClick={() => beginRenameTitle(transcript)}
                              />
                            </>
                          )}
                        </div>
                        {!expanded ? (
                          <p className="muted clamped-text">
                            {transcript.plainText}
                          </p>
                        ) : null}
                      </div>
                      <dl className="history-meta">
                        <div>
                          <dt>Created</dt>
                          <dd>
                            {new Date(transcript.createdAt).toLocaleString()}
                          </dd>
                        </div>
                        <div>
                          <dt>Status</dt>
                          <dd>
                            {transcript.status === "completed"
                              ? "Ready"
                              : transcript.status.replace(/_/g, " ")}
                            {transcript.qualityStatus !== "clean"
                              ? ` · ${transcript.qualityStatus}`
                              : ""}
                          </dd>
                        </div>
                        <div>
                          <dt>Speakers</dt>
                          <dd>{transcript.speakerCount ?? "Not identified"}</dd>
                        </div>
                      </dl>
                    </div>

                    {expanded ? (
                      <div
                        className="speaker-transcript-viewer"
                        id={`history-transcript-${transcript.id}`}
                      >
                        {busy[transcript.id] ? (
                          <p className="muted">Loading speaker transcript...</p>
                        ) : detail ? (
                          <>
                            {detail.diarizationWarning ? (
                              <p className="warning-text">
                                {detail.diarizationWarning}
                              </p>
                            ) : null}
                            {detail.diarizationModelId ? (
                              <p className="muted diarization-provenance">
                                {detail.diarizationSource === "native_model"
                                  ? `Built into ${getFriendlyModelName(detail.modelName)}`
                                  : `Speaker clustering: ${detail.diarizationSpeakerCountHint === null ? `Automatic · threshold ${detail.diarizationClusteringThreshold?.toFixed(2) ?? "unknown"}` : `About ${detail.diarizationSpeakerCountHint} speakers · exact target`}`}
                              </p>
                            ) : null}
                            {detail.speakers.length > 0 ? (
                              <div className="speaker-roster">
                                {detail.speakers.map((speaker) => {
                                  const anchorKey = `roster:${transcript.id}:${speaker.speakerId}`;
                                  return renameEditor?.anchorKey ===
                                    anchorKey ? (
                                    <InlineRenameEditor
                                      key={speaker.speakerId}
                                      inputLabel="Speaker name"
                                      value={renameEditor.value}
                                      isSaving={isSavingRename}
                                      compact
                                      onChange={(value) =>
                                        setRenameEditor((current) =>
                                          current
                                            ? { ...current, value }
                                            : current,
                                        )
                                      }
                                      onSave={() => void saveRename()}
                                      onCancel={() => setRenameEditor(null)}
                                    />
                                  ) : (
                                    <button
                                      key={speaker.speakerId}
                                      className={`speaker-chip speaker-color-${speaker.speakerOrder % 6}`}
                                      aria-label={`Rename ${speaker.displayName}`}
                                      title={`Rename ${speaker.displayName}`}
                                      onClick={() =>
                                        beginRenameSpeaker(
                                          transcript.id,
                                          speaker,
                                          anchorKey,
                                        )
                                      }
                                    >
                                      <span>{speaker.displayName}</span>
                                      <AppIcon name="pencil" />
                                    </button>
                                  );
                                })}
                              </div>
                            ) : null}
                            {detail.segments.length > 0 ? (
                              <div className="speaker-segment-list">
                                {groupSegments(detail.segments).map(
                                  (group, index) => {
                                    const speaker = renameableSegmentSpeaker(
                                      group[0],
                                      detail.speakers,
                                    );
                                    const candidateSpeakers =
                                      segmentCandidateSpeakers(
                                        group[0],
                                        detail.speakers,
                                      );
                                    const anchorPrefix = `segment:${transcript.id}:${group[0].id}:${index}`;
                                    const isEditingThisSegment =
                                      renameEditor?.anchorKey.startsWith(
                                        `${anchorPrefix}:`,
                                      ) ?? false;
                                    return (
                                      <div
                                        className={
                                          "speaker-segment" +
                                          (detail.speakers.length === 0
                                            ? " no-speaker"
                                            : "")
                                        }
                                        key={`${group[0].id}:${index}`}
                                      >
                                        {detail.speakers.length ===
                                        0 ? null : renameEditor &&
                                          isEditingThisSegment ? (
                                          <InlineRenameEditor
                                            inputLabel="Speaker name"
                                            value={renameEditor.value}
                                            isSaving={isSavingRename}
                                            compact
                                            onChange={(value) =>
                                              setRenameEditor((current) =>
                                                current
                                                  ? { ...current, value }
                                                  : current,
                                              )
                                            }
                                            onSave={() => void saveRename()}
                                            onCancel={() =>
                                              setRenameEditor(null)
                                            }
                                          />
                                        ) : speaker ? (
                                          <button
                                            type="button"
                                            className={`speaker-label speaker-label-button speaker-color-${speakerColor(group[0], detail.speakers)}`}
                                            aria-label={`Rename ${speaker.displayName} in transcript`}
                                            title={`Rename ${speaker.displayName}`}
                                            onClick={() =>
                                              beginRenameSpeaker(
                                                transcript.id,
                                                speaker,
                                                `${anchorPrefix}:${speaker.speakerId}`,
                                              )
                                            }
                                          >
                                            {segmentLabel(
                                              group[0],
                                              detail.speakers,
                                            )}
                                          </button>
                                        ) : candidateSpeakers.length > 0 ? (
                                          <span
                                            className={`speaker-label speaker-label-compound speaker-color-${speakerColor(group[0], detail.speakers)}`}
                                          >
                                            {group[0].speakerAttribution ===
                                            "uncertain"
                                              ? "Uncertain: "
                                              : ""}
                                            {candidateSpeakers.map(
                                              (candidate, candidateIndex) => (
                                                <span key={candidate.speakerId}>
                                                  {candidateIndex > 0
                                                    ? group[0]
                                                        .speakerAttribution ===
                                                      "overlap"
                                                      ? " + "
                                                      : " / "
                                                    : null}
                                                  <button
                                                    type="button"
                                                    className="speaker-name-inline"
                                                    aria-label={`Rename ${candidate.displayName} in transcript`}
                                                    onClick={() =>
                                                      beginRenameSpeaker(
                                                        transcript.id,
                                                        candidate,
                                                        `${anchorPrefix}:${candidate.speakerId}`,
                                                      )
                                                    }
                                                  >
                                                    {candidate.displayName}
                                                  </button>
                                                </span>
                                              ),
                                            )}
                                          </span>
                                        ) : (
                                          <span
                                            className={`speaker-label speaker-color-${speakerColor(group[0], detail.speakers)}`}
                                          >
                                            {segmentLabel(
                                              group[0],
                                              detail.speakers,
                                            )}
                                          </span>
                                        )}
                                        <div>
                                          <span className="speaker-time">
                                            {formatTime(group[0].startMs)}
                                          </span>
                                          <p>
                                            {group
                                              .map((segment) =>
                                                segment.text.trim(),
                                              )
                                              .join(" ")}
                                          </p>
                                        </div>
                                      </div>
                                    );
                                  },
                                )}
                              </div>
                            ) : (
                              <p className="muted">{detail.plainText}</p>
                            )}
                          </>
                        ) : (
                          <p className="muted">
                            {messages[transcript.id] ?? transcript.plainText}
                          </p>
                        )}
                      </div>
                    ) : null}

                    <div className="history-card-actions icon-action-group">
                      <IconButton
                        icon="copySpeakers"
                        label="Copy with speakers"
                        onClick={() =>
                          void handleCopy(transcript.id, "speaker_aware")
                        }
                      />
                      <IconButton
                        icon="copyPlain"
                        label="Copy plain text"
                        onClick={() => void handleCopy(transcript.id, "plain")}
                      />
                      <div className="export-menu-anchor" data-export-menu-root>
                        <IconButton
                          ref={(element) => {
                            exportButtonRefs.current[transcript.id] = element;
                          }}
                          icon="share"
                          label={
                            exportingTranscriptId === transcript.id
                              ? `Exporting ${transcript.title}`
                              : `Export ${transcript.title}`
                          }
                          state={
                            exportingTranscriptId === transcript.id
                              ? "busy"
                              : openExportMenuId === transcript.id
                                ? "selected"
                                : "default"
                          }
                          aria-haspopup="menu"
                          aria-expanded={openExportMenuId === transcript.id}
                          aria-controls={`export-menu-${transcript.id}`}
                          disabled={exportingTranscriptId !== null}
                          onClick={() =>
                            setOpenExportMenuId((current) =>
                              current === transcript.id ? null : transcript.id,
                            )
                          }
                        />
                        {openExportMenuId === transcript.id ? (
                          <div
                            id={`export-menu-${transcript.id}`}
                            className="export-format-menu"
                            role="menu"
                            aria-label={`Export ${transcript.title} as`}
                            onKeyDown={handleExportMenuKeyDown}
                          >
                            {EXPORT_OPTIONS.map((option) => (
                              <button
                                type="button"
                                role="menuitem"
                                className="export-format-option"
                                key={option.format}
                                onClick={() =>
                                  void handleExport(
                                    transcript.id,
                                    option.format,
                                  )
                                }
                              >
                                <span>{option.label}</span>
                                <span className="export-format-extension">
                                  {option.extension}
                                </span>
                              </button>
                            ))}
                          </div>
                        ) : null}
                      </div>
                      {transcript.sourceType === "file_upload" ? (
                        <div
                          className="retry-speakers-anchor"
                          data-retry-speakers-root
                        >
                          {rediarizingId === transcript.id ? (
                            <IconButton
                              icon="xCircle"
                              label="Cancel speaker retry"
                              tone="danger"
                              onClick={() =>
                                void handleCancelSpeakerRetry(transcript.id)
                              }
                            />
                          ) : (
                            <IconButton
                              icon="retrySpeakers"
                              label="Retry speaker identification"
                              state={
                                openRetryMenuId === transcript.id
                                  ? "selected"
                                  : "default"
                              }
                              aria-haspopup="dialog"
                              aria-expanded={openRetryMenuId === transcript.id}
                              onClick={() =>
                                setOpenRetryMenuId((current) =>
                                  current === transcript.id
                                    ? null
                                    : transcript.id,
                                )
                              }
                            />
                          )}
                          {openRetryMenuId === transcript.id ? (
                            <div
                              className="retry-speakers-popover"
                              role="dialog"
                              aria-label={`Retry speakers for ${transcript.title}`}
                            >
                              <button
                                type="button"
                                onClick={() =>
                                  void handleRetrySpeakers(transcript, null)
                                }
                              >
                                Automatic
                              </button>
                              <label>
                                About{" "}
                                <input
                                  type="number"
                                  min={1}
                                  max={20}
                                  value={retryCounts[transcript.id] ?? 7}
                                  onChange={(event) =>
                                    setRetryCounts((current) => ({
                                      ...current,
                                      [transcript.id]: Math.min(
                                        20,
                                        Math.max(
                                          1,
                                          Number(event.target.value) || 1,
                                        ),
                                      ),
                                    }))
                                  }
                                />{" "}
                                speakers
                              </label>
                              <button
                                type="button"
                                onClick={() =>
                                  void handleRetrySpeakers(
                                    transcript,
                                    retryCounts[transcript.id] ?? 7,
                                  )
                                }
                              >
                                Use estimate
                              </button>
                              <p>The estimate becomes the clustering target.</p>
                            </div>
                          ) : null}
                        </div>
                      ) : null}
                      <span
                        className={`status-pill diarization-${transcript.diarizationStatus}`}
                      >
                        {formatDiarizationStatus(transcript)}
                      </span>
                      <IconButton
                        icon="trash"
                        label={`Delete ${transcript.title}`}
                        tone="danger"
                        onClick={() => {
                          void onDelete(transcript.id)
                            .then(() => {
                              setSelectedId(null);
                              setSearchResults(null);
                            })
                            .catch((error) =>
                              setMessages({
                                [transcript.id]:
                                  error instanceof Error
                                    ? error.message
                                    : "Delete failed.",
                              }),
                            );
                        }}
                      />
                      {messages[transcript.id] ? (
                        <span className="inline-feedback" role="status">
                          {messages[transcript.id]}
                        </span>
                      ) : null}
                    </div>
                  </article>
                );
              })
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function InlineRenameEditor({
  inputLabel,
  value,
  isSaving,
  compact = false,
  onChange,
  onSave,
  onCancel,
}: {
  inputLabel: "Transcript title" | "Speaker name";
  value: string;
  isSaving: boolean;
  compact?: boolean;
  onChange: (value: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const subject =
    inputLabel === "Transcript title" ? "transcript title" : "speaker name";
  return (
    <form
      className={`inline-rename-editor${compact ? " is-compact" : ""}`}
      aria-label={`Rename ${subject}`}
      onSubmit={(event) => {
        event.preventDefault();
        onSave();
      }}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        onCancel();
      }}
    >
      <input
        aria-label={inputLabel}
        value={value}
        maxLength={inputLabel === "Transcript title" ? 200 : 80}
        autoFocus
        disabled={isSaving}
        onFocus={(event) => event.currentTarget.select()}
        onChange={(event) => onChange(event.target.value)}
      />
      <IconButton
        icon="check"
        label={`Save ${subject}`}
        size="compact"
        state={isSaving ? "busy" : "success"}
        disabled={isSaving || value.trim().length === 0}
        type="submit"
      />
      <IconButton
        icon="xmark"
        label={`Cancel renaming ${subject}`}
        size="compact"
        disabled={isSaving}
        onClick={onCancel}
      />
    </form>
  );
}

const EXPORT_OPTIONS: Array<{
  format: TranscriptExportFormat;
  label: string;
  extension: string;
}> = [
  { format: "txt", label: "Plain text", extension: "TXT" },
  { format: "md", label: "Markdown", extension: "MD" },
  { format: "srt", label: "SubRip subtitles", extension: "SRT" },
  { format: "vtt", label: "WebVTT subtitles", extension: "VTT" },
  { format: "json", label: "Structured data", extension: "JSON" },
];

function handleExportMenuKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  event.preventDefault();
  const options = Array.from(
    event.currentTarget.querySelectorAll<HTMLButtonElement>(
      '[role="menuitem"]',
    ),
  );
  const currentIndex = options.indexOf(
    document.activeElement as HTMLButtonElement,
  );
  const direction = event.key === "ArrowDown" ? 1 : -1;
  const nextIndex =
    currentIndex < 0
      ? direction > 0
        ? 0
        : options.length - 1
      : (currentIndex + direction + options.length) % options.length;
  options[nextIndex]?.focus();
}

function groupSegments(segments: TranscriptSegment[]): TranscriptSegment[][] {
  const groups: TranscriptSegment[][] = [];
  for (const segment of segments) {
    const previous = groups[groups.length - 1];
    if (previous && segmentLabelKey(previous[0]) === segmentLabelKey(segment))
      previous.push(segment);
    else groups.push([segment]);
  }
  return groups;
}

function segmentLabelKey(segment: TranscriptSegment) {
  return `${segment.speakerAttribution}:${segment.speakerId ?? ""}:${(segment.speakerIds ?? []).join(",")}`;
}
function segmentLabel(
  segment: TranscriptSegment,
  speakers: TranscriptSpeaker[],
) {
  const name = (id: string) =>
    speakers.find((speaker) => speaker.speakerId === id)?.displayName ?? id;
  if (segment.speakerAttribution === "assigned" && segment.speakerId)
    return name(segment.speakerId);
  if (segment.speakerAttribution === "likely" && segment.speakerId)
    return `${name(segment.speakerId)}?`;
  if (segment.speakerAttribution === "overlap")
    return (
      (segment.speakerIds ?? []).map(name).join(" + ") || "Overlapping speakers"
    );
  if (segment.speakerAttribution === "uncertain")
    return `Uncertain${segment.speakerIds?.length ? `: ${segment.speakerIds.map(name).join(" / ")}` : ""}`;
  return "Unknown speaker";
}
function renameableSegmentSpeaker(
  segment: TranscriptSegment,
  speakers: TranscriptSpeaker[],
): TranscriptSpeaker | null {
  if (
    (segment.speakerAttribution !== "assigned" &&
      segment.speakerAttribution !== "likely") ||
    !segment.speakerId
  ) {
    return null;
  }
  return (
    speakers.find((speaker) => speaker.speakerId === segment.speakerId) ?? null
  );
}
function segmentCandidateSpeakers(
  segment: TranscriptSegment,
  speakers: TranscriptSpeaker[],
): TranscriptSpeaker[] {
  if (
    segment.speakerAttribution !== "overlap" &&
    segment.speakerAttribution !== "uncertain"
  ) {
    return [];
  }
  const candidateIds = Array.from(new Set(segment.speakerIds ?? []));
  return candidateIds.flatMap((speakerId) => {
    const speaker = speakers.find(
      (candidate) => candidate.speakerId === speakerId,
    );
    return speaker ? [speaker] : [];
  });
}
function speakerColor(
  segment: TranscriptSegment,
  speakers: TranscriptSpeaker[],
) {
  const id = segment.speakerId ?? segment.speakerIds?.[0];
  return (
    Math.max(
      0,
      speakers.find((speaker) => speaker.speakerId === id)?.speakerOrder ?? 0,
    ) % 6
  );
}
function formatTime(ms: number) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
}
function formatDiarizationStatus(transcript: TranscriptSummary) {
  if (transcript.diarizationStatus === "completed")
    return `${transcript.speakerCount ?? 0} speakers`;
  if (transcript.diarizationStatus === "completed_with_uncertainty")
    return `${transcript.speakerCount ?? 0} speakers · review`;
  if (transcript.diarizationStatus === "not_enough_speech")
    return "Not enough speech";
  if (transcript.diarizationStatus === "failed") return "Speakers unavailable";
  return transcript.diarizationStatus === "not_requested"
    ? "Speaker labels off"
    : transcript.diarizationStatus.replace(/_/g, " ");
}
