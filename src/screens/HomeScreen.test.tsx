import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useState, type ComponentProps } from "react";
import type {
  AppSettings,
  FileQueueItem,
  RecordingStatusResponse,
  TranscriptResult,
} from "../types/domain";
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
  speakerCountHint: null,
  onSpeakerCountHintChange: vi.fn(),
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

const diarizationSettings: AppSettings = {
  defaultMode: "file_transcribe", shortcut: "CmdOrCtrl+Shift+Space", shortcutMode: "push_to_talk",
  languageMode: "auto", fixedLanguage: null, preferredInputDevice: null, insertBehavior: "paste",
  launchAtLoginEnabled: false, gpuEnabled: true, shortcutDictationModelProfile: "balanced",
  shortcutDictationSelectedModelId: null, quickDictateModelProfile: "balanced",
  quickDictateSelectedModelId: null, fileTranscribeModelProfile: "balanced",
  fileTranscribeSelectedModelId: null, saveHistory: true, soundsEnabled: true,
  volumeDuckingEnabled: true, fileDiarizationEnabled: true,
};

function SpeakerHintHarness({ onPickFiles }: { onPickFiles: (hint: number | null) => void }) {
  const [hint, setHint] = useState<number | null>(null);
  return <HomeScreen {...baseProps} settings={diarizationSettings} speakerCountHint={hint} onSpeakerCountHintChange={setHint} onPickFiles={onPickFiles} />;
}

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
  diarizationModelId: "sherpa-diarization-pyannote3-eres2net-voxceleb-v2",
  diarizationWarning:
    "Speaker identification stopped responding. The transcript was saved without speaker labels.",
  diarizationPolicyVersion: 1,
  diarizationClusteringThreshold: 1.1,
  diarizationSpeakerCountHint: null,
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

function recordingStatus(
  overrides: Partial<RecordingStatusResponse>,
): RecordingStatusResponse {
  return {
    state: "idle",
    currentSessionId: null,
    activeInputDevice: null,
    lastRecordingPath: null,
    lastErrorMessage: null,
    durationMs: null,
    sampleRateHz: null,
    channels: null,
    ...overrides,
  };
}

describe("HomeScreen manual recording controls", () => {
  it("hides cancel when there is no active recording", () => {
    render(<HomeScreen {...baseProps} />);

    expect(screen.queryByRole("button", { name: "Cancel recording" })).toBeNull();
  });

  it("shows an enabled cancel action while recording", () => {
    const onCancelRecording = vi.fn();
    render(
      <HomeScreen
        {...baseProps}
        recordingStatus={recordingStatus({
          state: "listening",
          currentSessionId: "recording-1",
        })}
        onCancelRecording={onCancelRecording}
      />,
    );

    const cancelButton = screen.getByRole("button", { name: "Cancel recording" });
    expect((cancelButton as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(cancelButton);
    expect(onCancelRecording).toHaveBeenCalledOnce();
  });

  it("keeps cancel available while a recording is paused", () => {
    render(
      <HomeScreen
        {...baseProps}
        recordingStatus={recordingStatus({
          state: "paused",
          currentSessionId: "recording-1",
        })}
      />,
    );

    const cancelButton = screen.getByRole("button", { name: "Cancel recording" });
    expect((cancelButton as HTMLButtonElement).disabled).toBe(false);
  });
});

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
  it("uses the drop area as the upload action and omits redundant controls", () => {
    render(<HomeScreen {...baseProps} settings={diarizationSettings} />);

    expect(screen.queryByRole("button", { name: "Hold to dictate to clipboard" })).toBeNull();
    const uploadArea = screen.getByRole("button", { name: "Upload audio files" });
    expect(uploadArea.textContent).toContain("Drop files here");
    expect(document.querySelector(".home-primary-icon-action")).toBeNull();
    const speakerHint = screen.getByRole("button", { name: "Speaker hint: Automatic" });
    expect(speakerHint.classList.contains("speaker-hint-select")).toBe(true);
    expect(speakerHint.textContent).toContain("Speakers");
    expect(speakerHint.textContent).toContain("Automatic");
  });

  it("captures a session-only speaker estimate when the drop area is clicked", () => {
    const onPickFiles = vi.fn();
    render(<SpeakerHintHarness onPickFiles={onPickFiles} />);
    fireEvent.click(screen.getByRole("button", { name: "Speaker hint: Automatic" }));
    fireEvent.change(screen.getByLabelText("Approximate speaker count"), { target: { value: "7" } });
    fireEvent.click(screen.getByRole("button", { name: "Use estimate" }));
    expect(screen.getByRole("button", { name: "Speaker hint: About 7" }).textContent).toContain("About 7");
    fireEvent.click(screen.getByRole("button", { name: "Upload audio files" }));

    expect(onPickFiles).toHaveBeenCalledWith(7);
  });

  it("keeps drag and drop active on the clickable upload area", () => {
    const onDropFiles = vi.fn();
    const onSetFileDragActive = vi.fn();
    const file = new File(["audio"], "sample.wav", { type: "audio/wav" });

    render(
      <HomeScreen
        {...baseProps}
        onDropFiles={onDropFiles}
        onSetFileDragActive={onSetFileDragActive}
      />,
    );

    const uploadArea = screen.getByRole("button", { name: "Upload audio files" });
    fireEvent.dragEnter(uploadArea, { dataTransfer: { files: [file] } });
    expect(onSetFileDragActive).toHaveBeenCalledWith(true);

    fireEvent.drop(uploadArea, { dataTransfer: { files: [file] } });
    expect(onSetFileDragActive).toHaveBeenLastCalledWith(false);
    expect(onDropFiles).toHaveBeenCalledOnce();
    expect(onDropFiles.mock.calls[0][0][0]).toBe(file);
    expect(onDropFiles.mock.calls[0][1]).toBeNull();
  });

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

describe("HomeScreen file transcript disclosure", () => {
  it("uses a compact title-level disclosure instead of a bottom text button", () => {
    const onToggleFileTranscript = vi.fn();
    render(
      <HomeScreen
        {...baseProps}
        fileQueueItems={[fileQueueItem({ isExpanded: false })]}
        onToggleFileTranscript={onToggleFileTranscript}
      />,
    );
    const disclosure = screen.getByRole("button", {
      name: "Expand transcript for audio.m4a",
    });

    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    expect(disclosure.closest(".file-queue-title-row")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Show full text" })).toBeNull();
    fireEvent.click(disclosure);
    expect(onToggleFileTranscript).toHaveBeenCalledWith("job-1");
  });

  it("keeps the collapse control linked from the header to expanded text", () => {
    render(<HomeScreen {...baseProps} fileQueueItems={[fileQueueItem({ isExpanded: true })]} />);
    const disclosure = screen.getByRole("button", {
      name: "Collapse transcript for audio.m4a",
    });
    const controlledId = disclosure.getAttribute("aria-controls");

    expect(disclosure.getAttribute("aria-expanded")).toBe("true");
    expect(disclosure.classList.contains("is-expanded")).toBe(true);
    expect(controlledId).toBe("file-transcript-job-1");
    expect(document.getElementById(controlledId!)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Show less" })).toBeNull();
  });
});
