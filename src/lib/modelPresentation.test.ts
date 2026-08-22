import { describe, expect, it } from "vitest";
import type { InstalledModel } from "../types/domain";
import {
  formatRating,
  formatRatingLine,
  getFriendlyModelName,
  getModelPresentation,
  isModelRecommended,
} from "./modelPresentation";

const MODEL_CASES = [
  ["moss-transcribe-diarize-0.9b-f16", "MOSS Transcribe + Diarize 0.9B F16", "MOSS Transcribe + Diarize", 3, 5],
  ["vibevoice-asr-8bit-mlx", "VibeVoice-ASR 8-bit MLX", "VibeVoice ASR", 2, 5],
  ["qwen3-asr-1.7b-bf16", "Qwen3-ASR-1.7B", "Qwen ASR", 2, 5],
  ["ggml-large-v3-turbo-q5_0-bin", "ggml-large-v3-turbo-q5_0.bin", "Whisper Turbo Compact", 5, 4],
  ["ggml-large-v3-turbo-bin", "ggml-large-v3-turbo.bin", "Whisper Turbo", 5, 5],
  ["ggml-medium-bin", "ggml-medium.bin", "Whisper Precision", 3, 4],
  ["ggml-small-bin", "ggml-small.bin", "Whisper Balanced", 4, 3],
] as const;

function installedModel(id: string, modelName: string): InstalledModel {
  return {
    id,
    engine: id.startsWith("qwen") ? "qwen3_asr_c" : "whisper.cpp",
    modelName,
    variant: "fixture",
    localPath: `/models/${modelName}`,
    sizeBytes: 1_600_000_000,
    isDefault: false,
    profile: "accurate",
  };
}

describe("model presentation", () => {
  it.each(MODEL_CASES)("maps %s to its friendly metadata", (id, technicalName, friendlyName, speed, accuracy) => {
    const presentation = getModelPresentation(installedModel(id, technicalName));
    expect(presentation.friendlyName).toBe(friendlyName);
    expect(presentation.technicalName).toBe(technicalName);
    expect(presentation.speed).toBe(speed);
    expect(presentation.accuracy).toBe(accuracy);
    expect(formatRatingLine(presentation)).toContain("Speed");
    expect(formatRatingLine(presentation)).toContain("Accuracy");
  });

  it("uses the requested recommendation contexts", () => {
    const qwen = getModelPresentation(installedModel("qwen3-asr-1.7b-bf16", "Qwen3-ASR-1.7B"));
    const turbo = getModelPresentation(installedModel("ggml-large-v3-turbo-bin", "ggml-large-v3-turbo.bin"));
    expect(isModelRecommended(qwen, "quick_dictate")).toBe(true);
    expect(isModelRecommended(qwen, "file_transcription")).toBe(true);
    expect(isModelRecommended(qwen, "shortcut_dictation")).toBe(false);
    expect(isModelRecommended(turbo, "shortcut_dictation")).toBe(true);
  });

  it("renders five rating circles", () => {
    expect(formatRating(3)).toBe("●●●○○");
    expect(formatRating(9)).toBe("●●●●●");
    expect(formatRating(-1)).toBe("○○○○○");
  });

  it("keeps custom filenames available while generating a readable fallback", () => {
    const custom = installedModel("custom-model", "ggml-base.en.bin");
    custom.engine = "whisper.cpp";
    custom.profile = "balanced";
    const presentation = getModelPresentation(custom);
    expect(presentation.friendlyName).toBe("Whisper Base EN");
    expect(presentation.technicalName).toBe("ggml-base.en.bin");
    expect(presentation.speed).toBe(4);
    expect(presentation.accuracy).toBe(3);
    expect(presentation.recommendedFor).toEqual([]);
    expect(getFriendlyModelName("ggml-medium.bin")).toBe("Whisper Precision");
  });
});
