import { formatBytes } from "./formatting";
import type { ModelCapabilities, ModelProfile } from "../types/domain";

export type ModelPickerContext =
  | "shortcut_dictation"
  | "quick_dictate"
  | "file_transcription";

export interface PresentableModel {
  id: string;
  engine: string;
  modelName: string;
  sizeBytes: number;
  profile: ModelProfile;
  description?: string;
  requirements?: string | null;
  variant?: string;
  capabilities?: ModelCapabilities;
  licenseUrl?: string | null;
}

export interface ModelPresentation {
  id: string;
  friendlyName: string;
  technicalName: string;
  sizeBytes: number;
  speed: number;
  accuracy: number;
  description: string;
  technicalDetails: string;
  requirements: string | null;
  recommendedFor: ModelPickerContext[];
}

interface CatalogEntry {
  friendlyName: string;
  speed: number;
  accuracy: number;
  description: string;
  technicalDetails: string;
  recommendedFor: ModelPickerContext[];
  technicalNames: string[];
}

const MODEL_CATALOG: Record<string, CatalogEntry> = {
  "confucius4-r2t2-q8-0": {
    friendlyName: "R2T2 · Experimental", speed: 4.5, accuracy: 4,
    description: "Local live preview for German and English shortcut dictation. Text is pasted once after you stop. Acceptance testing is still in progress.",
    technicalDetails: "Confucius4-R2T2 · Q8_0 GGUF · Metal · five-minute limit · separate NetEase model license",
    recommendedFor: [], technicalNames: ["R2T2 Q8 · Experimental"],
  },
  "qwen3-asr-1.7b-bf16": {
    friendlyName: "Qwen ASR",
    speed: 2.5,
    accuracy: 4.5,
    description: "Top accuracy in German, English and Spanish, and handles switching between them well. Runs on the CPU, so it is slower than Whisper.",
    technicalDetails: "Qwen3-ASR 1.7B · BF16 · CPU inference",
    recommendedFor: ["quick_dictate", "file_transcription"],
    technicalNames: ["Qwen3-ASR-1.7B", "qwen3-asr-1.7b-bf16"],
  },
  "moss-transcribe-diarize-0.9b-f16": {
    friendlyName: "MOSS Transcribe + Diarize",
    speed: 2,
    accuracy: 4,
    description: "Long recordings with speaker labels, timestamps, sound events and vocabulary hotwords. Runs on the CPU at around twice real time, too slow for dictation.",
    technicalDetails: "MOSS Transcribe-Diarize 0.9B · F16 GGUF · isolated CPU worker",
    recommendedFor: [],
    technicalNames: ["MOSS Transcribe + Diarize 0.9B F16"],
  },
  "vibevoice-asr-8bit-mlx": {
    friendlyName: "VibeVoice ASR",
    speed: 1,
    accuracy: 5,
    description: "The most accurate model for mixed-language audio, with speaker labels, timestamps and vocabulary context for up to 60 minutes. Slow; best for files.",
    technicalDetails: "VibeVoice-ASR · 8-bit MLX · Apple Silicon",
    recommendedFor: ["file_transcription"],
    technicalNames: ["VibeVoice-ASR 8-bit MLX"],
  },
  "ggml-large-v3-turbo-q5_0-bin": {
    friendlyName: "Whisper Turbo Compact",
    speed: 5,
    accuracy: 3,
    description: "The smallest, fastest Turbo. Slightly less accurate than Whisper Turbo and also weak when you switch languages.",
    technicalDetails: "Whisper large-v3-turbo · Q5_0 quantized · whisper.cpp",
    recommendedFor: [],
    technicalNames: ["ggml-large-v3-turbo-q5_0.bin"],
  },
  "ggml-large-v3-turbo-bin": {
    friendlyName: "Whisper Turbo",
    speed: 4.5,
    accuracy: 3.5,
    description: "Very fast and accurate for single-language dictation. Tends to drop or translate words when you switch languages mid-recording.",
    technicalDetails: "Whisper large-v3-turbo · F16 · whisper.cpp",
    recommendedFor: [],
    technicalNames: ["ggml-large-v3-turbo.bin"],
  },
  "ggml-medium-bin": {
    friendlyName: "Whisper Medium",
    speed: 4,
    accuracy: 3.5,
    description: "The best Whisper model when you mix languages, and still fast enough for everyday dictation.",
    technicalDetails: "Whisper medium · F16 · whisper.cpp",
    recommendedFor: ["shortcut_dictation"],
    technicalNames: ["ggml-medium.bin"],
  },
  "ggml-small-bin": {
    friendlyName: "Whisper Small",
    speed: 5,
    accuracy: 2,
    description: "Tiny and very fast, but noticeably less accurate, especially in German.",
    technicalDetails: "Whisper small · F16 · whisper.cpp",
    recommendedFor: [],
    technicalNames: ["ggml-small.bin"],
  },
};

const CATALOG_BY_TECHNICAL_NAME = new Map<string, [string, CatalogEntry]>();
for (const [id, entry] of Object.entries(MODEL_CATALOG)) {
  for (const technicalName of entry.technicalNames) {
    CATALOG_BY_TECHNICAL_NAME.set(normalizeTechnicalName(technicalName), [id, entry]);
  }
}

export function getModelPresentation(
  model: PresentableModel,
): ModelPresentation {
  const directEntry = MODEL_CATALOG[model.id];
  const technicalEntry = CATALOG_BY_TECHNICAL_NAME.get(normalizeTechnicalName(model.modelName));
  const entry = directEntry ?? technicalEntry?.[1];
  const canonicalId = directEntry ? model.id : technicalEntry?.[0] ?? model.id;

  if (entry) {
    return {
      id: canonicalId,
      friendlyName: entry.friendlyName,
      technicalName: model.modelName,
      sizeBytes: model.sizeBytes,
      speed: entry.speed,
      accuracy: entry.accuracy,
      description: entry.description,
      technicalDetails: entry.technicalDetails,
      requirements: model.requirements ?? null,
      recommendedFor: entry.recommendedFor,
    };
  }

  const ratings = ratingsForProfile(model.profile);
  return {
    id: model.id,
    friendlyName: humanizeTechnicalName(model.modelName, model.engine),
    technicalName: model.modelName,
    sizeBytes: model.sizeBytes,
    speed: ratings.speed,
    accuracy: ratings.accuracy,
    description: "A custom local transcription model added to Blabber.",
    technicalDetails: [model.engine, model.variant].filter(Boolean).join(" · "),
    requirements: model.requirements ?? null,
    recommendedFor: [],
  };
}

export function getFriendlyModelName(
  model: Pick<PresentableModel, "id" | "engine" | "modelName" | "sizeBytes" | "profile"> | string | null | undefined,
): string {
  if (!model) return "Missing model";
  if (typeof model !== "string") return getModelPresentation(model).friendlyName;

  const entry = CATALOG_BY_TECHNICAL_NAME.get(normalizeTechnicalName(model));
  if (entry) return entry[1].friendlyName;
  return humanizeTechnicalName(model, "");
}

export function isModelRecommended(
  presentation: ModelPresentation,
  context: ModelPickerContext,
): boolean {
  return presentation.recommendedFor.includes(context);
}

/** Ratings run 0–5 in half steps; 0 means "not rated". */
export type RatingCircle = "full" | "half" | "empty";

export function normalizeRating(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(5, Math.round(value * 2) / 2));
}

export function ratingCircles(value: number): RatingCircle[] {
  const rating = normalizeRating(value);
  return Array.from({ length: 5 }, (_, index) => {
    if (rating >= index + 1) return "full";
    if (rating >= index + 0.5) return "half";
    return "empty";
  });
}

export function formatRating(value: number): string {
  if (value === 0) return "Not rated";
  const glyphs: Record<RatingCircle, string> = { full: "●", half: "◐", empty: "○" };
  return ratingCircles(value).map((circle) => glyphs[circle]).join("");
}

export function formatRatingValue(label: string, value: number): string {
  if (value === 0) return `${label} not rated`;
  return `${label} ${normalizeRating(value)} of 5`;
}

export function formatRatingLine(presentation: Pick<ModelPresentation, "speed" | "accuracy">) {
  return `${formatRatingValue("Speed", presentation.speed)}, ${formatRatingValue("Accuracy", presentation.accuracy)}`;
}

export function formatModelSize(sizeBytes: number) {
  return formatBytes(sizeBytes);
}

export function recommendationLabel(context: ModelPickerContext) {
  switch (context) {
    case "shortcut_dictation":
      return "Shortcut Dictation";
    case "quick_dictate":
      return "Quick Dictate";
    case "file_transcription":
      return "File Transcription";
  }
}

function ratingsForProfile(profile: ModelProfile) {
  switch (profile) {
    case "fast":
      return { speed: 5, accuracy: 2 };
    case "balanced":
      return { speed: 4, accuracy: 3 };
    case "accurate":
      return { speed: 3, accuracy: 4 };
  }
}

function normalizeTechnicalName(value: string) {
  return value.trim().toLocaleLowerCase();
}

function humanizeTechnicalName(modelName: string, engine: string) {
  const fileName = modelName.split(/[\\/]/).pop() ?? modelName;
  const cleaned = fileName
    .replace(/\.bin$/i, "")
    .replace(/^ggml[-_]/i, "")
    .replace(/\.en$/i, " EN")
    .replace(/[-_]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  const titled = cleaned
    .split(" ")
    .map((part) => {
      if (/^(q\d|f\d|bf\d|fp\d|en)$/i.test(part)) return part.toUpperCase();
      return `${part.charAt(0).toUpperCase()}${part.slice(1)}`;
    })
    .join(" ");
  if (engine.toLocaleLowerCase().includes("whisper") && !/^whisper\b/i.test(titled)) {
    return `Whisper ${titled}`;
  }
  return titled || modelName;
}

export const modelPresentationCatalog = MODEL_CATALOG;
