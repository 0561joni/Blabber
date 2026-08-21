import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { FileTranscriptionResponse } from "../types/domain";
import { FileTranscribePanel } from "./FileTranscribePanel";

const transcription: FileTranscriptionResponse = {
  sourceFile: {
    filePath: "/tmp/audio.wav",
    originalName: "audio.wav",
    mimeType: "audio/wav",
    sizeBytes: 42,
    durationMs: 1_000,
    sha256: null,
  },
  resolvedModel: {
    id: "whisper-large-v3-turbo",
    engine: "whisper",
    modelName: "ggml-large-v3-turbo.bin",
    variant: "large-v3-turbo",
    localPath: "/tmp/ggml-large-v3-turbo.bin",
    sizeBytes: 1_624_000_000,
    isDefault: false,
    profile: "accurate",
  },
  result: {
    jobId: "job-1",
    modelName: "ggml-large-v3-turbo.bin",
    fullText: "Transcript text.",
    plainText: "Transcript text.",
    timestampedText: "[00:00] Transcript text.",
    detectedLanguages: ["en"],
    segments: [],
    qualityStatus: "clean",
    recoveredRegionCount: 0,
    warnings: [],
    diarizationStatus: "not_requested",
    diarizationModelId: null,
    diarizationWarning: null,
    diarizationPolicyVersion: null,
    diarizationClusteringThreshold: null,
    diarizationSpeakerCountHint: null,
    speakers: [],
    diarizationTurns: [],
  },
  savedTranscript: null,
};

describe("FileTranscribePanel model label", () => {
  it("uses the friendly model name for a completed transcription", () => {
    render(
      <FileTranscribePanel
        selectedFile={transcription.sourceFile}
        transcription={transcription}
        jobStatus={null}
        elapsedMs={null}
        errorMessage={null}
        onPickFile={vi.fn()}
        onTranscribe={vi.fn()}
      />,
    );

    expect(screen.getByText("Whisper Turbo")).toBeTruthy();
    expect(screen.queryByText(/ggml-large-v3-turbo\.bin/)).toBeNull();
  });
});
