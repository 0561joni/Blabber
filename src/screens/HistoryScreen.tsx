import { useEffect, useState } from "react";
import { copyTranscript, exportTranscript, getTranscript, renameTranscript, renameTranscriptSpeaker } from "../lib/api";
import type { TranscriptDetail, TranscriptExportFormat, TranscriptSegment, TranscriptSpeaker, TranscriptSummary } from "../types/domain";

interface HistoryScreenProps {
  transcripts: TranscriptSummary[];
  onTranscriptUpdated: (transcript: TranscriptSummary) => void;
  onDelete: (transcriptId: string) => Promise<void>;
  onDeleteAll: () => Promise<void>;
}

export function HistoryScreen({ transcripts, onTranscriptUpdated, onDelete, onDeleteAll }: HistoryScreenProps) {
  const [confirmDeleteAll, setConfirmDeleteAll] = useState(false);
  const [isDeletingAll, setIsDeletingAll] = useState(false);
  const [expandedIds, setExpandedIds] = useState<string[]>([]);
  const [details, setDetails] = useState<Record<string, TranscriptDetail>>({});
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [messages, setMessages] = useState<Record<string, string>>({});

  useEffect(() => {
    if (Object.keys(messages).length === 0) return;
    const timeout = window.setTimeout(() => setMessages({}), 2200);
    return () => window.clearTimeout(timeout);
  }, [messages]);

  async function confirmDelete() {
    setIsDeletingAll(true);
    try { await onDeleteAll(); setConfirmDeleteAll(false); } finally { setIsDeletingAll(false); }
  }

  async function toggleExpanded(transcriptId: string) {
    if (expandedIds.includes(transcriptId)) {
      setExpandedIds((current) => current.filter((id) => id !== transcriptId));
      return;
    }
    setExpandedIds((current) => [...current, transcriptId]);
    if (!details[transcriptId]) {
      setBusy((current) => ({ ...current, [transcriptId]: true }));
      try {
        const detail = await getTranscript(transcriptId);
        setDetails((current) => ({ ...current, [transcriptId]: detail }));
      } catch (error) {
        setMessages({ [transcriptId]: error instanceof Error ? error.message : "Failed to load transcript." });
      } finally {
        setBusy((current) => ({ ...current, [transcriptId]: false }));
      }
    }
  }

  async function handleRenameTitle(transcript: TranscriptSummary) {
    const title = window.prompt("Transcript title", transcript.title);
    if (title === null || title.trim() === transcript.title) return;
    try {
      const updated = await renameTranscript(transcript.id, title);
      onTranscriptUpdated(updated);
      setDetails((current) => current[transcript.id] ? { ...current, [transcript.id]: { ...current[transcript.id], ...updated } } : current);
    } catch (error) {
      setMessages({ [transcript.id]: error instanceof Error ? error.message : "Rename failed." });
    }
  }

  async function handleRenameSpeaker(transcriptId: string, speaker: TranscriptSpeaker) {
    const displayName = window.prompt("Speaker name", speaker.displayName);
    if (displayName === null || displayName.trim() === speaker.displayName) return;
    try {
      const detail = await renameTranscriptSpeaker(transcriptId, speaker.speakerId, displayName);
      setDetails((current) => ({ ...current, [transcriptId]: detail }));
    } catch (error) {
      setMessages({ [transcriptId]: error instanceof Error ? error.message : "Rename failed." });
    }
  }

  async function handleCopy(transcriptId: string, variant: "speaker_aware" | "plain") {
    try {
      await copyTranscript(transcriptId, variant);
      setMessages({ [transcriptId]: variant === "plain" ? "Plain text copied" : "Speaker transcript copied" });
    } catch (error) {
      setMessages({ [transcriptId]: error instanceof Error ? error.message : "Copy failed." });
    }
  }

  async function handleExport(transcriptId: string, format: TranscriptExportFormat) {
    if (!format) return;
    try {
      const result = await exportTranscript(transcriptId, format);
      if (result.path) setMessages({ [transcriptId]: `Exported ${format.toUpperCase()}` });
    } catch (error) {
      setMessages({ [transcriptId]: error instanceof Error ? error.message : "Export failed." });
    }
  }

  return (
    <section className="screen"><div className="history-screen-layout">
      <article className="glass-panel history-toolbar-panel">
        <div className="section-header history-toolbar-header"><div><p className="eyebrow">History</p><h2>Saved transcripts</h2></div><span className="status-pill">{transcripts.length} saved</span></div>
        <div className="history-toolbar-actions">{transcripts.length > 0 ? <button className="danger-button history-danger-button" disabled={isDeletingAll} onClick={() => setConfirmDeleteAll(true)}>Delete all history</button> : null}</div>
        {confirmDeleteAll ? <div className="glass-subtle confirm-panel history-confirm-panel"><p className="transcript-title">Delete the entire history?</p><p className="muted">This removes all saved transcripts and speaker metadata from local history.</p><div className="toolbar action-segment"><button className="danger-button" disabled={isDeletingAll} onClick={() => void confirmDelete()}>{isDeletingAll ? "Deleting..." : "Yes, delete everything"}</button><button disabled={isDeletingAll} onClick={() => setConfirmDeleteAll(false)}>Cancel</button></div></div> : null}
      </article>

      <div className="history-list history-list-wide">
        {transcripts.length === 0 ? <article className="glass-panel history-card empty-state"><div><p className="eyebrow">No history yet</p><h2>Your saved transcripts will appear here.</h2><p className="muted">Dictate once or transcribe an audio file to populate your saved history.</p></div></article> : transcripts.map((transcript) => {
          const expanded = expandedIds.includes(transcript.id);
          const detail = details[transcript.id];
          return <article className="glass-panel history-card history-card-wide" key={transcript.id}>
            <div className="history-card-main history-card-main-wide"><div className="history-card-copy"><p className="eyebrow">{transcript.sourceType.replace("_", " ")}</p><div className="transcript-title-row"><h2>{transcript.title}</h2><button className="small-action-button" onClick={() => void handleRenameTitle(transcript)}>Rename</button></div>{!expanded ? <p className="muted clamped-text">{transcript.plainText}</p> : null}</div><dl className="history-meta"><div><dt>Created</dt><dd>{new Date(transcript.createdAt).toLocaleString()}</dd></div><div><dt>Status</dt><dd>{transcript.status}{transcript.qualityStatus !== "clean" ? ` · ${transcript.qualityStatus}` : ""}</dd></div><div><dt>Speakers</dt><dd>{transcript.speakerCount ?? "Not identified"}</dd></div></dl></div>
            {expanded ? <div className="speaker-transcript-viewer">{busy[transcript.id] ? <p className="muted">Loading speaker transcript...</p> : detail ? <>{detail.diarizationWarning ? <p className="warning-text">{detail.diarizationWarning}</p> : null}{detail.speakers.length > 0 ? <div className="speaker-roster">{detail.speakers.map((speaker) => <button key={speaker.speakerId} className={`speaker-chip speaker-color-${speaker.speakerOrder % 6}`} onClick={() => void handleRenameSpeaker(transcript.id, speaker)}>{speaker.displayName} · rename</button>)}</div> : null}{detail.segments.length > 0 ? <div className="speaker-segment-list">{groupSegments(detail.segments).map((group, index) => <div className="speaker-segment" key={`${group[0].id}:${index}`}><span className={`speaker-label speaker-color-${speakerColor(group[0], detail.speakers)}`}>{segmentLabel(group[0], detail.speakers)}</span><div><span className="speaker-time">{formatTime(group[0].startMs)}</span><p>{group.map((segment) => segment.text.trim()).join(" ")}</p></div></div>)}</div> : <p className="muted">{detail.plainText}</p>}</> : <p className="muted">{messages[transcript.id] ?? transcript.plainText}</p>}</div> : null}
            <div className="history-card-actions"><button onClick={() => void handleCopy(transcript.id, "speaker_aware")}>Copy with speakers</button><button onClick={() => void handleCopy(transcript.id, "plain")}>Copy plain</button><select aria-label="Export format" defaultValue="" onChange={(event) => { const format = event.target.value as TranscriptExportFormat; event.target.value = ""; void handleExport(transcript.id, format); }}><option value="" disabled>Export…</option><option value="txt">TXT</option><option value="md">Markdown</option><option value="srt">SRT</option><option value="vtt">VTT</option><option value="json">JSON</option></select><button onClick={() => void toggleExpanded(transcript.id)}>{expanded ? "Hide details" : "View transcript"}</button><span className={`status-pill diarization-${transcript.diarizationStatus}`}>{formatDiarizationStatus(transcript)}</span><button onClick={() => void onDelete(transcript.id)}>Delete</button>{messages[transcript.id] ? <span className="muted">{messages[transcript.id]}</span> : null}</div>
          </article>;
        })}
      </div>
    </div></section>
  );
}

function groupSegments(segments: TranscriptSegment[]): TranscriptSegment[][] {
  const groups: TranscriptSegment[][] = [];
  for (const segment of segments) {
    const previous = groups[groups.length - 1];
    if (previous && segmentLabelKey(previous[0]) === segmentLabelKey(segment)) previous.push(segment);
    else groups.push([segment]);
  }
  return groups;
}

function segmentLabelKey(segment: TranscriptSegment) { return `${segment.speakerAttribution}:${segment.speakerId ?? ""}:${(segment.speakerIds ?? []).join(",")}`; }
function segmentLabel(segment: TranscriptSegment, speakers: TranscriptSpeaker[]) {
  const name = (id: string) => speakers.find((speaker) => speaker.speakerId === id)?.displayName ?? id;
  if (segment.speakerAttribution === "assigned" && segment.speakerId) return name(segment.speakerId);
  if (segment.speakerAttribution === "overlap") return (segment.speakerIds ?? []).map(name).join(" + ") || "Overlapping speakers";
  if (segment.speakerAttribution === "uncertain") return `Uncertain${segment.speakerIds?.length ? `: ${segment.speakerIds.map(name).join(" / ")}` : ""}`;
  return "Unknown speaker";
}
function speakerColor(segment: TranscriptSegment, speakers: TranscriptSpeaker[]) { const id = segment.speakerId ?? segment.speakerIds?.[0]; return Math.max(0, speakers.find((speaker) => speaker.speakerId === id)?.speakerOrder ?? 0) % 6; }
function formatTime(ms: number) { const seconds = Math.max(0, Math.floor(ms / 1000)); return `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`; }
function formatDiarizationStatus(transcript: TranscriptSummary) {
  if (transcript.diarizationStatus === "completed") return `${transcript.speakerCount ?? 0} speakers`;
  if (transcript.diarizationStatus === "completed_with_uncertainty") return `${transcript.speakerCount ?? 0} speakers · review`;
  if (transcript.diarizationStatus === "not_enough_speech") return "Not enough speech";
  if (transcript.diarizationStatus === "failed") return "Speakers unavailable";
  return transcript.diarizationStatus === "not_requested" ? "No diarization" : transcript.diarizationStatus.replace(/_/g, " ");
}
