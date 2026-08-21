import type { ModelProfile } from "../types/domain";

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
  "qwen3-asr-1.7b-bf16": {
    friendlyName: "Qwen ASR",
    speed: 2,
    accuracy: 5,
    description:
      "High-quality multilingual model optimized for accurate transcription of longer audio files and mixed-language speech.",
    technicalDetails: "Qwen3-ASR 1.7B · BF16 · CPU inference",
    recommendedFor: ["quick_dictate", "file_transcription"],
    technicalNames: ["Qwen3-ASR-1.7B", "qwen3-asr-1.7b-bf16"],
  },
  "ggml-large-v3-turbo-q5_0-bin": {
    friendlyName: "Whisper Turbo Compact",
    speed: 5,
    accuracy: 4,
    description:
      "A smaller, quantized Turbo model with excellent speed and strong accuracy when storage or memory is limited.",
    technicalDetails: "Whisper large-v3-turbo · Q5_0 quantized · whisper.cpp",
    recommendedFor: [],
    technicalNames: ["ggml-large-v3-turbo-q5_0.bin"],
  },
  "ggml-large-v3-turbo-bin": {
    friendlyName: "Whisper Turbo",
    speed: 5,
    accuracy: 5,
    description: "Fast, high-quality multilingual transcription for everyday dictation.",
    technicalDetails: "Whisper large-v3-turbo · F16 · whisper.cpp",
    recommendedFor: ["shortcut_dictation"],
    technicalNames: ["ggml-large-v3-turbo.bin"],
  },
  "ggml-medium-bin": {
    friendlyName: "Whisper Precision",
    speed: 3,
    accuracy: 4,
    description: "A detailed multilingual model for users who favor accuracy over speed.",
    technicalDetails: "Whisper medium · F16 · whisper.cpp",
    recommendedFor: [],
    technicalNames: ["ggml-medium.bin"],
  },
  "ggml-small-bin": {
    friendlyName: "Whisper Balanced",
    speed: 4,
    accuracy: 3,
    description: "A practical middle ground for everyday transcription on modest hardware.",
    technicalDetails: "Whisper small · F16 · whisper.cpp",
    recommendedFor: [],
    technicalNames: ["ggml-small.bin"],
  },
  "ggml-tiny-bin": {
    friendlyName: "Whisper Fast",
    speed: 5,
    accuracy: 2,
    description: "The lightest model for quickest responses, with lower accuracy on difficult audio.",
    technicalDetails: "Whisper tiny · F16 · whisper.cpp",
    recommendedFor: [],
    technicalNames: ["ggml-tiny.bin"],
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

export function formatRating(value: number): string {
  const rating = Math.max(0, Math.min(5, Math.round(value)));
  return `${"●".repeat(rating)}${"○".repeat(5 - rating)}`;
}

export function formatRatingLine(presentation: Pick<ModelPresentation, "speed" | "accuracy">) {
  return `Speed ${formatRating(presentation.speed)} Accuracy ${formatRating(presentation.accuracy)}`;
}

export function formatModelSize(sizeBytes: number) {
  if (sizeBytes >= 1_000_000_000) {
    return `${(sizeBytes / 1_000_000_000).toFixed(1)} GB`;
  }
  return `${Math.round(sizeBytes / 1_000_000)} MB`;
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
