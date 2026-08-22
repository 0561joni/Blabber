import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TranscriptDetail, TranscriptExportResult, TranscriptSummary } from "../types/domain";

const apiMocks = vi.hoisted(() => ({
  exportTranscript: vi.fn(),
  getTranscript: vi.fn(),
  rediarizeTranscript: vi.fn(),
  cancelRediarization: vi.fn(),
  listenRediarizationStatus: vi.fn(),
  renameTranscript: vi.fn(),
  renameTranscriptSpeaker: vi.fn(),
}));

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  exportTranscript: apiMocks.exportTranscript,
  getTranscript: apiMocks.getTranscript,
  rediarizeTranscript: apiMocks.rediarizeTranscript,
  cancelRediarization: apiMocks.cancelRediarization,
  listenRediarizationStatus: apiMocks.listenRediarizationStatus,
  renameTranscript: apiMocks.renameTranscript,
  renameTranscriptSpeaker: apiMocks.renameTranscriptSpeaker,
}));

import { HistoryScreen } from "./HistoryScreen";

const transcript: TranscriptSummary = {
  id: "transcript-1",
  createdAt: "2026-08-16T00:00:00Z",
  sourceType: "file_upload",
  title: "Meeting",
  plainText: "Meeting transcript",
  status: "completed",
  detectedLanguages: ["en"],
  durationMs: 1_000,
  modelName: "Local model",
  qualityStatus: "clean",
  recoveredRegionCount: 0,
  diarizationStatus: "completed",
  speakerCount: 2,
};

const detail: TranscriptDetail = {
  ...transcript,
  fullText: transcript.plainText,
  timestampedText: transcript.plainText,
  transcriptionWarnings: [],
  diarizationModelId: "sherpa-diarization-pyannote3-eres2net-voxceleb-v2",
  diarizationSource: "post_process",
  diarizationWarning: null,
  diarizationPolicyVersion: 2,
  diarizationClusteringThreshold: 1.1,
  diarizationSpeakerCountHint: null,
  segments: [],
  speakers: [
    { speakerId: "speaker_0", displayName: "Speaker 1", speakerOrder: 0 },
    { speakerId: "speaker_1", displayName: "Speaker 2", speakerOrder: 1 },
  ],
  diarizationTurns: [],
};

function renderHistory() {
  const onTranscriptUpdated = vi.fn();
  return {
    onTranscriptUpdated,
    ...render(
      <HistoryScreen
        transcripts={[transcript]}
        onTranscriptUpdated={onTranscriptUpdated}
        onDelete={vi.fn().mockResolvedValue(undefined)}
        onDeleteAll={vi.fn().mockResolvedValue(undefined)}
      />,
    ),
  };
}

describe("HistoryScreen transcript export", () => {
  beforeEach(() => {
    apiMocks.exportTranscript.mockReset();
    apiMocks.getTranscript.mockReset().mockResolvedValue(detail);
    apiMocks.rediarizeTranscript.mockReset();
    apiMocks.cancelRediarization.mockReset().mockResolvedValue(undefined);
    apiMocks.listenRediarizationStatus.mockReset().mockResolvedValue(() => undefined);
    apiMocks.renameTranscript.mockReset().mockResolvedValue({ ...transcript, title: "New meeting title" });
    apiMocks.renameTranscriptSpeaker.mockReset().mockResolvedValue(detail);
  });

  it("renames a transcript through an in-app editor", async () => {
    const { onTranscriptUpdated } = renderHistory();
    fireEvent.click(screen.getByRole("button", { name: "Rename transcript" }));

    const input = screen.getByRole("textbox", { name: "Transcript title" });
    expect((input as HTMLInputElement).value).toBe("Meeting");
    fireEvent.change(input, { target: { value: "New meeting title" } });
    fireEvent.click(screen.getByRole("button", { name: "Save transcript title" }));

    await waitFor(() => expect(apiMocks.renameTranscript).toHaveBeenCalledWith(transcript.id, "New meeting title"));
    expect(onTranscriptUpdated).toHaveBeenCalledWith({ ...transcript, title: "New meeting title" });
    expect(screen.getByText("Transcript renamed")).toBeTruthy();
  });

  it("renames speakers from both the roster and their transcript labels", async () => {
    const detailWithSegment: TranscriptDetail = {
      ...detail,
      segments: [{
        id: "segment-1",
        startMs: 0,
        endMs: 1_000,
        text: "Hello there.",
        languageCode: "en",
        segmentOrder: 0,
        confidence: null,
        speakerId: "speaker_0",
        speakerIds: ["speaker_0"],
        speakerAttribution: "assigned",
        speakerConfidence: 0.9,
      }],
    };
    const aliceDetail = {
      ...detailWithSegment,
      speakers: [{ ...detailWithSegment.speakers[0], displayName: "Alice" }, detailWithSegment.speakers[1]],
    };
    const bobDetail = {
      ...aliceDetail,
      speakers: [{ ...aliceDetail.speakers[0], displayName: "Bob" }, aliceDetail.speakers[1]],
    };
    apiMocks.getTranscript.mockResolvedValue(detailWithSegment);
    apiMocks.renameTranscriptSpeaker.mockResolvedValueOnce(aliceDetail).mockResolvedValueOnce(bobDetail);
    renderHistory();
    fireEvent.click(screen.getByRole("button", { name: "Expand transcript for Meeting" }));

    const rosterRename = await screen.findByRole("button", { name: "Rename Speaker 1" });
    fireEvent.click(rosterRename);
    fireEvent.change(screen.getByRole("textbox", { name: "Speaker name" }), { target: { value: "Alice" } });
    fireEvent.click(screen.getByRole("button", { name: "Save speaker name" }));
    await waitFor(() => expect(apiMocks.renameTranscriptSpeaker).toHaveBeenLastCalledWith(transcript.id, "speaker_0", "Alice"));

    fireEvent.click(await screen.findByRole("button", { name: "Rename Alice in transcript" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Speaker name" }), { target: { value: "Bob" } });
    fireEvent.click(screen.getByRole("button", { name: "Save speaker name" }));
    await waitFor(() => expect(apiMocks.renameTranscriptSpeaker).toHaveBeenLastCalledWith(transcript.id, "speaker_0", "Bob"));
    expect(await screen.findByRole("button", { name: "Rename Bob in transcript" })).toBeTruthy();
  });

  it("lets each named speaker in an overlap label open the rename editor", async () => {
    apiMocks.getTranscript.mockResolvedValue({
      ...detail,
      segments: [{
        id: "overlap-1",
        startMs: 0,
        endMs: 1_000,
        text: "We spoke together.",
        languageCode: "en",
        segmentOrder: 0,
        confidence: null,
        speakerId: null,
        speakerIds: ["speaker_0", "speaker_1"],
        speakerAttribution: "overlap",
        speakerConfidence: 0.8,
      }],
    });
    renderHistory();
    fireEvent.click(screen.getByRole("button", { name: "Expand transcript for Meeting" }));

    fireEvent.click(await screen.findByRole("button", { name: "Rename Speaker 2 in transcript" }));
    expect((screen.getByRole("textbox", { name: "Speaker name" }) as HTMLInputElement).value).toBe("Speaker 2");
  });

  it("exposes distinct symbolic transcript actions and keeps deletion confirmation textual", () => {
    renderHistory();
    expect(screen.getByRole("button", { name: "Rename transcript" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Copy with speakers" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Copy plain text" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Delete Meeting" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Delete all history" }));
    expect(screen.getByText("Delete the entire history?")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Yes, delete everything" })).toBeTruthy();
  });

  it("offers automatic and estimated diarization-only retries", () => {
    renderHistory();
    fireEvent.click(screen.getByRole("button", { name: "Retry speaker identification" }));

    const popover = screen.getByRole("dialog", { name: "Retry speakers for Meeting" });
    expect(within(popover).getByRole("button", { name: "Automatic" })).toBeTruthy();
    expect(within(popover).getByLabelText(/About/)).toHaveProperty("min", "1");
    expect(within(popover).getByLabelText(/About/)).toHaveProperty("max", "20");
  });

  it("runs a retry without ASR and exposes cancellation while it is active", async () => {
    apiMocks.rediarizeTranscript.mockReturnValue(new Promise(() => undefined));
    renderHistory();
    fireEvent.click(screen.getByRole("button", { name: "Retry speaker identification" }));
    fireEvent.click(screen.getByRole("button", { name: "Automatic" }));

    await waitFor(() => expect(apiMocks.rediarizeTranscript).toHaveBeenCalled());
    expect(apiMocks.rediarizeTranscript.mock.calls[0][0]).toMatchObject({
      transcriptId: transcript.id,
      sourceFile: null,
      speakerCountHint: null,
    });
    fireEvent.click(screen.getByRole("button", { name: "Cancel speaker retry" }));
    expect(apiMocks.cancelRediarization).toHaveBeenCalledTimes(1);
  });

  it("renders likely attribution with a question mark and shows clustering provenance", async () => {
    apiMocks.getTranscript.mockResolvedValue({
      ...detail,
      segments: [{
        id: "segment-1", startMs: 0, endMs: 1_000, text: "Probably me.", languageCode: "en",
        segmentOrder: 0, confidence: null, speakerId: "speaker_0", speakerIds: ["speaker_0", "speaker_1"],
        speakerAttribution: "likely", speakerConfidence: 0.72,
      }],
    });
    renderHistory();
    fireEvent.click(screen.getByRole("button", { name: "Expand transcript for Meeting" }));

    expect(await screen.findByText("Speaker 1?")).toBeTruthy();
    expect(screen.getByText("Speaker clustering: Automatic · threshold 1.10")).toBeTruthy();
  });

  it("shows native speaker provenance without clustering thresholds", async () => {
    apiMocks.getTranscript.mockResolvedValue({
      ...detail,
      modelName: "MOSS Transcribe + Diarize 0.9B F16",
      diarizationModelId: "moss-transcribe-diarize-0.9b-f16",
      diarizationSource: "native_model",
      diarizationClusteringThreshold: null,
    });
    renderHistory();
    fireEvent.click(screen.getByRole("button", { name: "Expand transcript for Meeting" }));

    expect(await screen.findByText("Built into MOSS Transcribe + Diarize")).toBeTruthy();
    expect(screen.queryByText(/Speaker clustering/)).toBeNull();
  });

  it("opens an Apple-style export menu with every format", () => {
    renderHistory();

    fireEvent.click(screen.getByRole("button", { name: "Export Meeting" }));

    expect(screen.getByRole("menu", { name: "Export Meeting as" })).toBeTruthy();
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Plain textTXT",
      "MarkdownMD",
      "SubRip subtitlesSRT",
      "WebVTT subtitlesVTT",
      "Structured dataJSON",
    ]);
  });

  it("prevents overlapping exports and restores the share button after success", async () => {
    let finishExport: (result: TranscriptExportResult) => void = () => undefined;
    apiMocks.exportTranscript.mockReturnValue(
      new Promise<TranscriptExportResult>((resolve) => {
        finishExport = resolve;
      }),
    );
    renderHistory();
    const shareButton = screen.getByRole("button", { name: "Export Meeting" });

    fireEvent.click(shareButton);
    fireEvent.click(screen.getByRole("menuitem", { name: /Plain text/ }));

    await waitFor(() => expect((shareButton as HTMLButtonElement).disabled).toBe(true));
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.click(shareButton);
    expect(apiMocks.exportTranscript).toHaveBeenCalledTimes(1);

    await act(async () => finishExport({ path: "/tmp/Meeting.txt" }));

    await waitFor(() => expect((shareButton as HTMLButtonElement).disabled).toBe(false));
    expect(screen.getByText("Exported TXT")).toBeTruthy();
  });

  it("clears the busy state when the native save dialog is canceled", async () => {
    apiMocks.exportTranscript.mockResolvedValue({ path: null });
    renderHistory();
    const shareButton = screen.getByRole("button", { name: "Export Meeting" });

    fireEvent.click(shareButton);
    fireEvent.click(screen.getByRole("menuitem", { name: /Markdown/ }));

    await waitFor(() => expect((shareButton as HTMLButtonElement).disabled).toBe(false));
    expect(screen.getByText("Export canceled")).toBeTruthy();
  });

  it("clears the busy state and reports an export error", async () => {
    apiMocks.exportTranscript.mockRejectedValue(new Error("Destination is unavailable."));
    renderHistory();
    const shareButton = screen.getByRole("button", { name: "Export Meeting" });

    fireEvent.click(shareButton);
    fireEvent.click(screen.getByRole("menuitem", { name: /Structured data/ }));

    await waitFor(() => expect((shareButton as HTMLButtonElement).disabled).toBe(false));
    expect(screen.getByText("Destination is unavailable.")).toBeTruthy();
  });

  it("dismisses the export menu with Escape and an outside click", () => {
    renderHistory();
    const shareButton = screen.getByRole("button", { name: "Export Meeting" });

    fireEvent.click(shareButton);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(shareButton);

    fireEvent.click(shareButton);
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
