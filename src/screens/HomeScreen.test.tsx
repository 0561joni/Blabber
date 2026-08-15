import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import type { FileQueueItem, TranscriptResult } from "../types/domain";
import { HomeScreen } from "./HomeScreen";

const baseProps: ComponentProps<typeof HomeScreen> = {
  settings: null,
  platform: "macos",
  preview: null,
  recordingStatus: null,
  manualTranscriptionState: {
    stage: "idle",
    statusText: "",
    startedAt: null,
    errorMessage: null,
  },
  quickDictationStatus: null,
  readiness: {
    hasModel: true,
    shortcutRegistered: true,
    autoPasteEnabled: true,
    accessibilityRequired: true,
    accessibilityGranted: false,
  },
  isPollingAccessibility: false,
  onResolveReadiness: vi.fn(),
  fileQueueItems: [],
  isFileDragActive: false,
  onStartRecording: vi.fn(),
  onStopAndTranscribeRecording: vi.fn(),
  onCancelRecording: vi.fn(),
  onResetDictation: vi.fn(),
  onPickFiles: vi.fn(),
  onDropFiles: vi.fn(),
  onSetFileDragActive: vi.fn(),
  onToggleFileTranscript: vi.fn(),
  onCopyFileTranscript: vi.fn(),
};

const transcriptResult: TranscriptResult = {
  jobId: "job-1",
  modelName: "Local model",
  fullText: "A completed transcript.",
  plainText: "A completed transcript.",
  timestampedText: "[00:00] A completed transcript.",
  detectedLanguages: ["en"],
  segments: [],
  qualityStatus: "clean",
  recoveredRegionCount: 0,
  warnings: [],
  diarizationStatus: "failed",
  diarizationModelId: "sherpa-diarization-pyannote3-eres2net-v1",
  diarizationWarning:
    "Speaker identification stopped responding. The transcript was saved without speaker labels.",
  diarizationPolicyVersion: 1,
  speakers: [],
  diarizationTurns: [],
};

function fileQueueItem(overrides: Partial<FileQueueItem>): FileQueueItem {
  return {
    id: "job-1",
    sourceFile: {
      filePath: "/tmp/audio.m4a",
      originalName: "audio.m4a",
      mimeType: "audio/mp4",
      sizeBytes: 42,
      durationMs: 5_897_877,
      sha256: null,
    },
    stage: "completed",
    progressPercent: 100,
    processedMs: 5_897_877,
    totalMs: 5_897_877,
    etaSeconds: 0,
    statusText: "Transcription completed.",
    result: {
      sourceFile: {
        filePath: "/tmp/audio.m4a",
        originalName: "audio.m4a",
        mimeType: "audio/mp4",
        sizeBytes: 42,
        durationMs: 5_897_877,
        sha256: null,
      },
      resolvedModel: null,
      result: transcriptResult,
      savedTranscript: null,
    },
    errorMessage: null,
    startedAt: 1,
    isExpanded: true,
    copyState: "idle",
    ...overrides,
  };
}

describe("HomeScreen Accessibility readiness action", () => {
  it("opens setup with a Grant access action before polling starts", () => {
    render(<HomeScreen {...baseProps} />);

    expect(screen.getByRole("button", { name: "Grant access" })).toBeTruthy();
  });

  it("offers an immediate Check again action while polling", () => {
    const onResolveReadiness = vi.fn();
    render(
      <HomeScreen
        {...baseProps}
        isPollingAccessibility
        onResolveReadiness={onResolveReadiness}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Check again" }));

    expect(onResolveReadiness).toHaveBeenCalledWith("accessibility");
  });
});

describe("HomeScreen file diarization status", () => {
  it("keeps diarization visibly active and indeterminate", () => {
    render(
      <HomeScreen
        {...baseProps}
        fileQueueItems={[
          fileQueueItem({
            stage: "diarizing",
            progressPercent: null,
            statusText: "Identifying speakers locally...",
            result: null,
            isExpanded: false,
          }),
        ]}
      />,
    );

    expect(screen.getByText("Identifying speakers")).toBeTruthy();
    expect(screen.getByText("Identifying speakers locally...")).toBeTruthy();
    expect(document.querySelector(".progress-fill.indeterminate")).toBeTruthy();
  });

  it("shows a diarization fallback warning without speaker results", () => {
    render(<HomeScreen {...baseProps} fileQueueItems={[fileQueueItem({})]} />);

    expect(
      screen.getByText(
        "Speaker identification stopped responding. The transcript was saved without speaker labels.",
      ),
    ).toBeTruthy();
    expect(screen.getByText("A completed transcript.")).toBeTruthy();
  });
});
