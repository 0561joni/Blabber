import type {
  AppSettings,
  CreateVocabularyTermInput,
  DictationReadiness,
  DownloadableModel,
  FileTranscriptionRequest,
  FileTranscriptionStatusEvent,
  FileTranscriptionResponse,
  HealthCheckResponse,
  InputDeviceOption,
  InstalledModel,
  ModelDownloadStatus,
  PlatformInfo,
  QuickDictationStatusResponse,
  RecordingResult,
  RecordingStatusResponse,
  SelectedSourceFile,
  SettingsPatch,
  StartFileTranscriptionResponse,
  TrayUnavailableClosePayload,
  TranscriptionPreviewRequest,
  TranscriptionPreviewResponse,
  TranscriptSummary,
  TranscriptDetail,
  TranscriptCopyVariant,
  TranscriptExportFormat,
  TranscriptExportResult,
  UpdateVocabularyTermInput,
  VocabularyTerm,
} from "../types/domain";

const mockSettings: AppSettings = {
  defaultMode: "quick_dictate",
  shortcut: "CmdOrCtrl+Shift+Space",
  shortcutMode: "push_to_talk",
  languageMode: "auto",
  fixedLanguage: null,
  preferredInputDevice: null,
  insertBehavior: "paste",
  launchAtLoginEnabled: false,
  gpuEnabled: true,
  shortcutDictationModelProfile: "accurate",
  shortcutDictationSelectedModelId: "ggml-medium-bin",
  quickDictateModelProfile: "accurate",
  quickDictateSelectedModelId: "ggml-large-v3-turbo-q5_0-bin",
  fileTranscribeModelProfile: "accurate",
  fileTranscribeSelectedModelId: "ggml-large-v3-turbo-q5_0-bin",
  saveHistory: true,
  soundsEnabled: true,
  volumeDuckingEnabled: true,
  fileDiarizationEnabled: false,
  quickDictateDiarizationEnabled: false,
  diarizationSpeakerCount: null,
};

const mockTranscripts: TranscriptSummary[] = [];
const mockModels: InstalledModel[] = [
  {
    id: "ggml-tiny-bin",
    engine: "whisper.cpp",
    modelName: "ggml-tiny.bin",
    variant: "fast",
    localPath: "mock://models/ggml-tiny.bin",
    sizeBytes: 77_691_713,
    isDefault: true,
    profile: "fast",
  },
  {
    id: "ggml-small-bin",
    engine: "whisper.cpp",
    modelName: "ggml-small.bin",
    variant: "balanced",
    localPath: "mock://models/ggml-small.bin",
    sizeBytes: 487_601_967,
    isDefault: true,
    profile: "balanced",
  },
  {
    id: "ggml-medium-bin",
    engine: "whisper.cpp",
    modelName: "ggml-medium.bin",
    variant: "accurate",
    localPath: "mock://models/ggml-medium.bin",
    sizeBytes: 1_533_763_059,
    isDefault: false,
    profile: "accurate",
  },
  {
    id: "ggml-large-v3-turbo-bin",
    engine: "whisper.cpp",
    modelName: "ggml-large-v3-turbo.bin",
    variant: "accurate",
    localPath: "mock://models/ggml-large-v3-turbo.bin",
    sizeBytes: 1_624_555_275,
    isDefault: false,
    profile: "accurate",
  },
  {
    id: "ggml-large-v3-turbo-q5_0-bin",
    engine: "whisper.cpp",
    modelName: "ggml-large-v3-turbo-q5_0.bin",
    variant: "accurate",
    localPath: "mock://models/ggml-large-v3-turbo-q5_0.bin",
    sizeBytes: 574_041_195,
    isDefault: true,
    profile: "accurate",
  },
];
const mockDownloadableModels: DownloadableModel[] = [
  {
    id: "ggml-tiny-bin",
    engine: "whisper.cpp",
    modelName: "ggml-tiny.bin",
    description: "Smallest local model for quick tests and lightweight dictation.",
    sizeBytes: 77_691_713,
    profile: "fast",
    availability: "available",
    availabilityReason: null,
    installed: true,
    requirements: null,
    artifactCount: 1,
    capability: "asr",
  },
  {
    id: "ggml-small-bin",
    engine: "whisper.cpp",
    modelName: "ggml-small.bin",
    description: "Good balance when you want lower memory use with better quality than tiny.",
    sizeBytes: 487_601_967,
    profile: "balanced",
    availability: "available",
    availabilityReason: null,
    installed: true,
    requirements: null,
    artifactCount: 1,
    capability: "asr",
  },
  {
    id: "ggml-medium-bin",
    engine: "whisper.cpp",
    modelName: "ggml-medium.bin",
    description: "Strong default for shortcut dictation when you want better accuracy.",
    sizeBytes: 1_533_763_059,
    profile: "accurate",
    availability: "available",
    availabilityReason: null,
    installed: true,
    requirements: null,
    artifactCount: 1,
    capability: "asr",
  },
  {
    id: "ggml-large-v3-turbo-bin",
    engine: "whisper.cpp",
    modelName: "ggml-large-v3-turbo.bin",
    description: "Best full-size turbo model when you want top quality and speed.",
    sizeBytes: 1_624_555_275,
    profile: "accurate",
    availability: "available",
    availabilityReason: null,
    installed: true,
    requirements: null,
    artifactCount: 1,
    capability: "asr",
  },
  {
    id: "ggml-large-v3-turbo-q5_0-bin",
    engine: "whisper.cpp",
    modelName: "ggml-large-v3-turbo-q5_0.bin",
    description: "Quantized turbo model with lower memory use and a strong quality-speed tradeoff.",
    sizeBytes: 574_041_195,
    profile: "accurate",
    availability: "available",
    availabilityReason: null,
    installed: true,
    requirements: null,
    artifactCount: 1,
    capability: "asr",
  },
  {
    id: "qwen3-asr-1.7b-bf16",
    engine: "qwen3_asr_c",
    modelName: "Qwen3-ASR-1.7B",
    description:
      "High-quality multilingual and code-switch transcription with dictionary-aware spelling prompts. CPU-only.",
    sizeBytes: 4_703_041_355,
    profile: "accurate",
    availability: "available",
    availabilityReason: null,
    installed: true,
    requirements: "macOS or Linux · 16 GB RAM recommended · CPU-only",
    artifactCount: 7,
    capability: "asr",
  },
  {
    id: "sherpa-diarization-pyannote3-eres2net-v1",
    engine: "sherpa-onnx",
    modelName: "Offline speaker diarization",
    description: "Local speaker separation using pyannote segmentation and ERes2Net embeddings.",
    sizeBytes: 0,
    profile: "balanced",
    availability: "pending_license_review",
    availabilityReason:
      "Downloads remain disabled until the upstream model-weight licenses and immutable hashes are reviewed.",
    installed: false,
    requirements: "CPU-only · model redistribution review pending",
    artifactCount: 0,
    capability: "diarization",
  },
];
const mockModelDownloadListeners = new Set<(status: ModelDownloadStatus) => void>();
const mockModelDownloadStatuses = new Map<string, ModelDownloadStatus>(
  mockDownloadableModels.map((model) => [
    model.id,
    {
      modelId: model.id,
      modelName: model.modelName,
      state: "idle" as const,
      downloadedBytes: 0,
      totalBytes: model.sizeBytes,
      progressPercent: 0,
      errorMessage: null,
      currentArtifact: null,
      artifactIndex: null,
      artifactCount: model.artifactCount,
    },
  ]),
);
const mockVocabularyTerms: VocabularyTerm[] = [
  {
    id: "builtin-linkedin",
    canonical: "LinkedIn",
    normalizedCanonical: "linkedin",
    category: "brand",
    languageHint: "en",
    matchMode: "exact_only",
    isBuiltin: true,
    createdAt: "2026-03-14T00:00:00Z",
    updatedAt: "2026-03-14T00:00:00Z",
    aliases: [
      { id: "builtin-linkedin-linked-in", alias: "linked in", normalizedAlias: "linked in" },
      { id: "builtin-linkedin-linken", alias: "linken", normalizedAlias: "linken" },
    ],
  },
];
const mockSelectedFiles: SelectedSourceFile[] = [
  {
    filePath: "mock://uploads/demo-interview.m4a",
    originalName: "demo-interview.m4a",
    mimeType: "audio/mp4",
    sizeBytes: 2_460_000,
    durationMs: null,
    sha256: null,
  },
  {
    filePath: "mock://uploads/customer-call.mp3",
    originalName: "customer-call.mp3",
    mimeType: "audio/mpeg",
    sizeBytes: 4_820_000,
    durationMs: null,
    sha256: null,
  },
];
const mockInputDevices: InputDeviceOption[] = [
  { id: "Built-in Microphone", name: "Built-in Microphone", isDefault: true },
  { id: "AirPods Pro", name: "AirPods Pro", isDefault: false },
];

let mockRecordingStatus: RecordingStatusResponse = {
  state: "idle",
  currentSessionId: null,
  activeInputDevice: "Browser preview microphone",
  lastRecordingPath: null,
  lastErrorMessage: null,
  durationMs: null,
  sampleRateHz: null,
  channels: null,
};

let mockRecordingStartedAt = 0;
let mockRecordingAccumulatedMs = 0;
let mockQuickDictationStatus: QuickDictationStatusResponse = {
  state: "idle",
  registeredShortcut: mockSettings.shortcut,
  shortcutMode: mockSettings.shortcutMode,
  isRegistered: false,
  lastTranscriptText: null,
  lastTranscriptId: null,
  lastRecordingPath: null,
  lastErrorMessage:
    "Global shortcuts only work in the Tauri desktop app, not in the browser preview.",
  lastModelName: null,
  lastInsertOutcome: null,
  lastDurationMs: null,
};
const mockFileTranscriptionListeners = new Set<
  (event: FileTranscriptionStatusEvent) => void
>();
const mockFileTranscriptionStatuses = new Map<string, FileTranscriptionStatusEvent>();

function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(command: string, payload?: Record<string, unknown>) {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, payload);
}

export async function getHealthCheck(): Promise<HealthCheckResponse> {
  if (!isTauriRuntime()) {
    return {
      appName: "Blabber",
      appVersion: "0.1.0",
      platform: "browser-preview",
      dbPath: "mock://speech-to-text.sqlite",
      tempDir: "mock://temp",
      modelsDir: "mock://models",
      startupNotices: [],
    };
  }
  return invoke<HealthCheckResponse>("health_check");
}

export async function getPlatformInfo(): Promise<PlatformInfo> {
  if (!isTauriRuntime()) {
    return {
      os: "browser-preview",
      isWayland: false,
      isGnome: false,
      hasAppindicatorHint: true,
      autoPasteSupported: true,
      globalShortcutSupported: true,
      dictateToggleExecutable: null,
      dictateToggleCommand: null,
    };
  }
  return invoke<PlatformInfo>("get_platform_info");
}

export async function quitApp(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke<void>("quit_app");
}

export async function listenTrayUnavailableCloseRequested(
  handler: (payload: TrayUnavailableClosePayload) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }

  const { listen } = await import("@tauri-apps/api/event");
  return listen<TrayUnavailableClosePayload>("app://tray-unavailable-close-requested", (event) => {
    handler(event.payload);
  });
}

export async function dictatePress(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke<void>("dictate_press");
}

export async function dictateRelease(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke<void>("dictate_release");
}

export async function dictateToggle(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke<void>("dictate_toggle");
}

export async function openModelsFolder(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("open_models_folder");
}

export async function listDownloadableModels(): Promise<DownloadableModel[]> {
  if (!isTauriRuntime()) {
    return mockDownloadableModels;
  }
  return invoke<DownloadableModel[]>("list_downloadable_models");
}

export async function getModelDownloadStatuses(): Promise<ModelDownloadStatus[]> {
  if (!isTauriRuntime()) {
    return Array.from(mockModelDownloadStatuses.values());
  }
  return invoke<ModelDownloadStatus[]>("get_model_download_statuses");
}

export async function startModelDownload(modelId: string): Promise<ModelDownloadStatus> {
  if (!isTauriRuntime()) {
    const model = mockDownloadableModels.find((entry) => entry.id === modelId);
    if (!model) {
      throw new Error("Unsupported model download.");
    }
    if (model.availability !== "available") {
      throw new Error("This model is not available on the current platform.");
    }
    const current = mockModelDownloadStatuses.get(modelId);
    if (current?.state === "downloading") {
      return current;
    }

    const activeDownload = Array.from(mockModelDownloadStatuses.values()).find(
      (status) => status.state === "downloading",
    );
    if (activeDownload) {
      throw new Error("Another model download is already in progress.");
    }

    const totalBytes = model.sizeBytes;
    const initialStatus: ModelDownloadStatus = {
      modelId: model.id,
      modelName: model.modelName,
      state: "downloading",
      downloadedBytes: 0,
      totalBytes,
      progressPercent: 0,
      errorMessage: null,
      currentArtifact: model.artifactCount > 1 ? "config.json" : model.modelName,
      artifactIndex: 1,
      artifactCount: model.artifactCount,
    };
    mockModelDownloadStatuses.set(model.id, initialStatus);
    emitMockModelDownloadStatus(initialStatus);

    const steps = 8;
    for (let step = 1; step <= steps; step += 1) {
      window.setTimeout(() => {
        if (mockModelDownloadStatuses.get(model.id)?.state === "canceled") {
          return;
        }
        const isFinal = step === steps;
        const nextStatus: ModelDownloadStatus = {
          modelId: model.id,
          modelName: model.modelName,
          state: isFinal ? "completed" : "downloading",
          downloadedBytes: Math.round((totalBytes * step) / steps),
          totalBytes,
          progressPercent: Math.round((100 * step) / steps),
          errorMessage: null,
          currentArtifact: isFinal ? null : model.modelName,
          artifactIndex: isFinal ? model.artifactCount : 1,
          artifactCount: model.artifactCount,
        };
        mockModelDownloadStatuses.set(model.id, nextStatus);
        emitMockModelDownloadStatus(nextStatus);

        if (isFinal && !mockModels.some((installed) => installed.id === model.id)) {
          mockModels.push({
            id: model.id,
            engine: model.engine,
            modelName: model.modelName,
            variant: model.profile,
            localPath: `mock://models/${model.modelName}`,
            sizeBytes: model.sizeBytes,
            isDefault: false,
            profile: model.profile,
          });
        }
      }, step * 280);
    }

    return initialStatus;
  }
  return invoke<ModelDownloadStatus>("start_model_download", { modelId });
}

export async function cancelModelDownload(modelId: string): Promise<ModelDownloadStatus> {
  if (!isTauriRuntime()) {
    const current = mockModelDownloadStatuses.get(modelId);
    if (!current || current.state !== "downloading") {
      throw new Error("No active download exists for this model.");
    }
    const canceled: ModelDownloadStatus = {
      ...current,
      state: "canceled",
      currentArtifact: null,
      artifactIndex: null,
    };
    mockModelDownloadStatuses.set(modelId, canceled);
    emitMockModelDownloadStatus(canceled);
    return canceled;
  }
  return invoke<ModelDownloadStatus>("cancel_model_download", { modelId });
}

export async function listenModelDownloadStatus(
  handler: (status: ModelDownloadStatus) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    mockModelDownloadListeners.add(handler);
    return () => {
      mockModelDownloadListeners.delete(handler);
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ModelDownloadStatus>("model-download-status", (event) => {
    handler(event.payload);
  });
}

export async function getSettings(): Promise<AppSettings> {
  if (!isTauriRuntime()) {
    return mockSettings;
  }
  return invoke<AppSettings>("get_settings");
}

export async function listInputDevices(): Promise<InputDeviceOption[]> {
  if (!isTauriRuntime()) {
    return mockInputDevices;
  }
  return invoke<InputDeviceOption[]>("list_input_devices");
}

export async function updateSettings(patch: SettingsPatch): Promise<AppSettings> {
  if (!isTauriRuntime()) {
    Object.assign(mockSettings, patch);
    mockQuickDictationStatus = {
      ...mockQuickDictationStatus,
      registeredShortcut: mockSettings.shortcut,
      shortcutMode: mockSettings.shortcutMode,
    };
    return mockSettings;
  }
  return invoke<AppSettings>("update_settings", { patch });
}

export async function suspendShortcutCapture(): Promise<void> {
  if (!isTauriRuntime()) {
    mockQuickDictationStatus = {
      ...mockQuickDictationStatus,
      isRegistered: false,
      lastErrorMessage: null,
    };
    return;
  }
  await invoke("suspend_shortcut_capture");
}

export async function resumeShortcutCapture(): Promise<void> {
  if (!isTauriRuntime()) {
    mockQuickDictationStatus = {
      ...mockQuickDictationStatus,
      isRegistered: true,
      registeredShortcut: mockSettings.shortcut,
      shortcutMode: mockSettings.shortcutMode,
      lastErrorMessage: null,
    };
    return;
  }
  await invoke("resume_shortcut_capture");
}

export async function listTranscripts(query: string): Promise<TranscriptSummary[]> {
  if (!isTauriRuntime()) {
    const normalized = query.trim().toLowerCase();
    return normalized.length === 0
      ? mockTranscripts
      : mockTranscripts.filter((item) =>
          [item.title, item.plainText].some((field) =>
            field.toLowerCase().includes(normalized),
          ),
        );
  }
  return invoke<TranscriptSummary[]>("list_transcripts", {
    query: query.trim().length === 0 ? null : query,
  });
}

export async function deleteTranscript(transcriptId: string): Promise<void> {
  if (!isTauriRuntime()) {
    const index = mockTranscripts.findIndex((item) => item.id === transcriptId);
    if (index >= 0) {
      mockTranscripts.splice(index, 1);
    }
    return;
  }
  return invoke("delete_transcript", { transcriptId });
}

export async function deleteAllTranscripts(): Promise<void> {
  if (!isTauriRuntime()) {
    mockTranscripts.splice(0, mockTranscripts.length);
    return;
  }
  return invoke("delete_all_transcripts");
}

export async function getTranscript(transcriptId: string): Promise<TranscriptDetail> {
  if (!isTauriRuntime()) {
    const summary = mockTranscripts.find((item) => item.id === transcriptId);
    if (!summary) throw new Error("Transcript not found.");
    return {
      ...summary,
      fullText: summary.plainText,
      timestampedText: summary.plainText,
      transcriptionWarnings: [],
      diarizationModelId: null,
      diarizationWarning: null,
      diarizationPolicyVersion: null,
      segments: [{
        id: `${summary.id}:0`, startMs: 0, endMs: summary.durationMs ?? 0,
        text: summary.plainText, languageCode: summary.detectedLanguages[0] ?? "und",
        segmentOrder: 0, confidence: null, speakerId: null, speakerIds: null,
        speakerAttribution: "none", speakerConfidence: null,
      }],
      speakers: [],
      diarizationTurns: [],
    };
  }
  return invoke<TranscriptDetail>("get_transcript", { transcriptId });
}

export async function renameTranscript(transcriptId: string, title: string): Promise<TranscriptSummary> {
  if (!isTauriRuntime()) {
    const transcript = mockTranscripts.find((item) => item.id === transcriptId);
    if (!transcript) throw new Error("Transcript not found.");
    transcript.title = title.trim();
    return transcript;
  }
  return invoke<TranscriptSummary>("rename_transcript", { transcriptId, title });
}

export async function renameTranscriptSpeaker(
  transcriptId: string,
  speakerId: string,
  displayName: string,
): Promise<TranscriptDetail> {
  if (!isTauriRuntime()) return getTranscript(transcriptId);
  return invoke<TranscriptDetail>("rename_transcript_speaker", {
    transcriptId,
    speakerId,
    displayName,
  });
}

export async function copyTranscript(
  transcriptId: string,
  variant: TranscriptCopyVariant,
): Promise<void> {
  if (!isTauriRuntime()) {
    const transcript = await getTranscript(transcriptId);
    await navigator.clipboard.writeText(transcript.plainText);
    return;
  }
  return invoke("copy_transcript", { transcriptId, variant });
}

export async function exportTranscript(
  transcriptId: string,
  format: TranscriptExportFormat,
): Promise<TranscriptExportResult> {
  if (!isTauriRuntime()) return { path: null };
  return invoke<TranscriptExportResult>("export_transcript", { transcriptId, format });
}

export async function copyTextToClipboard(text: string): Promise<void> {
  if (!isTauriRuntime()) {
    await navigator.clipboard.writeText(text);
    return;
  }
  return invoke("copy_text_to_clipboard", { text });
}

export async function listInstalledModels(): Promise<InstalledModel[]> {
  if (!isTauriRuntime()) {
    return mockModels;
  }
  return invoke<InstalledModel[]>("list_installed_models");
}

export async function previewTranscription(
  request: TranscriptionPreviewRequest,
): Promise<TranscriptionPreviewResponse> {
  if (!isTauriRuntime()) {
    return {
      sourceKind: request.sourceKind,
      resolvedModel:
        mockModels.find((model) => model.id === request.selectedModelId) ??
        mockModels.find((model) => model.profile === request.profile) ??
        null,
      result: null,
      error: {
        code: "browser_preview",
        message: "Use the Tauri app to run real transcription.",
      },
    };
  }
  return invoke<TranscriptionPreviewResponse>("preview_transcription", { request });
}

export async function pickAudioFiles(): Promise<SelectedSourceFile[]> {
  if (!isTauriRuntime()) {
    return mockSelectedFiles;
  }
  return invoke<SelectedSourceFile[]>("pick_audio_files");
}

export async function prepareDroppedAudioFiles(
  paths: string[],
): Promise<SelectedSourceFile[]> {
  if (!isTauriRuntime()) {
    const files = paths
      .filter(isSupportedAudioPath)
      .map((filePath) => buildMockSelectedFile(filePath));
    if (files.length === 0) {
      throw new Error("Drop WAV, MP3, M4A, or OPUS files to transcribe.");
    }
    return files;
  }
  return invoke<SelectedSourceFile[]>("prepare_dropped_audio_files", { paths });
}

export async function startFileTranscription(
  request: FileTranscriptionRequest,
): Promise<StartFileTranscriptionResponse> {
  if (!isTauriRuntime()) {
    const jobId = request.jobId;
    const sourceFile = request.sourceFile;
    const transcript: TranscriptSummary | null = mockSettings.saveHistory
      ? {
          id: crypto.randomUUID(),
          createdAt: new Date().toISOString(),
          sourceType: "file_upload",
          title: sourceFile.originalName,
          plainText: "Hallo Lena, LinkedIn bleibt LinkedIn und Empanadas bleiben Empanadas.",
          status: "completed",
          detectedLanguages: ["de", "en", "es"],
          durationMs: 8_400,
          modelName: "browser-preview",
          qualityStatus: "clean",
          recoveredRegionCount: 0,
          diarizationStatus: "not_requested",
          speakerCount: null,
        }
      : null;

    if (transcript) {
      mockTranscripts.unshift(transcript);
    }

    const result: FileTranscriptionResponse = {
      sourceFile: {
        ...sourceFile,
        durationMs: 8_400,
        sha256: "mock-sha256",
      },
      resolvedModel: null,
      result: {
        jobId: crypto.randomUUID(),
        modelName: "browser-preview",
        fullText: "Hallo Lena, LinkedIn bleibt LinkedIn und Empanadas bleiben Empanadas.",
        plainText: "Hallo Lena, LinkedIn bleibt LinkedIn und Empanadas bleiben Empanadas.",
        timestampedText:
          "[00:00 - 00:04] de: Hallo Lena, LinkedIn bleibt LinkedIn.\n[00:04 - 00:08] es: Und Empanadas bleiben Empanadas.",
        detectedLanguages: ["de", "en", "es"],
        qualityStatus: "clean",
        recoveredRegionCount: 0,
        warnings: [],
        diarizationStatus: "not_requested",
        diarizationModelId: null,
        diarizationWarning: null,
        diarizationPolicyVersion: null,
        speakers: [],
        diarizationTurns: [],
        segments: [
          {
            id: crypto.randomUUID(),
            startMs: 0,
            endMs: 4000,
            text: "Hallo Lena, LinkedIn bleibt LinkedIn.",
            languageCode: "de",
            segmentOrder: 0,
            confidence: 0.89,
            speakerId: null, speakerIds: null, speakerAttribution: "none", speakerConfidence: null,
          },
          {
            id: crypto.randomUUID(),
            startMs: 4000,
            endMs: 8400,
            text: "Und Empanadas bleiben Empanadas.",
            languageCode: "es",
            segmentOrder: 1,
            confidence: 0.9,
            speakerId: null, speakerIds: null, speakerAttribution: "none", speakerConfidence: null,
          },
        ],
      },
      savedTranscript: transcript,
    };
    queueMicrotask(() => {
      emitMockFileTranscriptionStatus({
        jobId,
        sourceFile,
        stage: "queued",
        progressPercent: null,
        processedMs: null,
        totalMs: null,
        etaSeconds: null,
        statusText: "Queued for local transcription.",
        result: null,
        errorMessage: null,
        startedAtMs: Date.now(),
        updatedAtMs: Date.now(),
      });
      window.setTimeout(() => {
        emitMockFileTranscriptionStatus({
          jobId,
          sourceFile,
          stage: "preparing",
          progressPercent: null,
          processedMs: null,
          totalMs: null,
          etaSeconds: null,
          statusText: "Preparing audio and estimating workload...",
          result: null,
          errorMessage: null,
          startedAtMs: Date.now(),
          updatedAtMs: Date.now(),
        });
      }, 120);
      window.setTimeout(() => {
        emitMockFileTranscriptionStatus({
          jobId,
          sourceFile,
          stage: "transcribing",
          progressPercent: 42,
          processedMs: 3500,
          totalMs: 8400,
          etaSeconds: 4,
          statusText: `Transcribing ${sourceFile.originalName}...`,
          result: null,
          errorMessage: null,
          startedAtMs: Date.now(),
          updatedAtMs: Date.now(),
        });
      }, 420);
      window.setTimeout(() => {
        emitMockFileTranscriptionStatus({
          jobId,
          sourceFile,
          stage: "saving",
          progressPercent: 100,
          processedMs: 8400,
          totalMs: 8400,
          etaSeconds: 0,
          statusText: "Saving transcript to local history...",
          result: null,
          errorMessage: null,
          startedAtMs: Date.now(),
          updatedAtMs: Date.now(),
        });
      }, 920);
      window.setTimeout(() => {
        emitMockFileTranscriptionStatus({
          jobId,
          sourceFile,
          stage: "completed",
          progressPercent: 100,
          processedMs: 8400,
          totalMs: 8400,
          etaSeconds: 0,
          statusText: "Done.",
          result,
          errorMessage: null,
          startedAtMs: Date.now(),
          updatedAtMs: Date.now(),
        });
      }, 1200);
    });
    return { jobId };
  }
  return invoke<StartFileTranscriptionResponse>("start_file_transcription", { request });
}

export async function getFileTranscriptionStatuses(): Promise<FileTranscriptionStatusEvent[]> {
  if (!isTauriRuntime()) {
    return Array.from(mockFileTranscriptionStatuses.values()).sort(
      (left, right) => right.startedAtMs - left.startedAtMs,
    );
  }
  return invoke<FileTranscriptionStatusEvent[]>("get_file_transcription_statuses");
}

export async function cancelFileTranscription(jobId: string): Promise<void> {
  if (!isTauriRuntime()) {
    const current = mockFileTranscriptionStatuses.get(jobId);
    if (current) {
      emitMockFileTranscriptionStatus({
        ...current,
        stage: "canceled",
        progressPercent: current.progressPercent,
        statusText: "File transcription canceled.",
        errorMessage: "The file transcription was canceled by the user.",
        result: null,
      });
    }
    return;
  }
  return invoke<void>("cancel_file_transcription", { jobId });
}

function buildMockSelectedFile(filePath: string): SelectedSourceFile {
  const matched = mockSelectedFiles.find((file) => file.filePath === filePath);
  if (matched) {
    return matched;
  }
  const originalName = filePath.split("/").pop() || "upload.wav";
  const extension = originalName.split(".").pop()?.toLowerCase();
  return {
    filePath,
    originalName,
    mimeType:
      extension === "mp3"
        ? "audio/mpeg"
        : extension === "m4a"
          ? "audio/mp4"
          : extension === "opus"
            ? "audio/ogg"
            : "audio/wav",
    sizeBytes: 1_024_000,
    durationMs: null,
    sha256: null,
  };
}

function isSupportedAudioPath(filePath: string) {
  const extension = filePath.split(".").pop()?.toLowerCase();
  return (
    extension === "wav" ||
    extension === "mp3" ||
    extension === "m4a" ||
    extension === "opus"
  );
}

export async function getRecordingStatus(): Promise<RecordingStatusResponse> {
  if (!isTauriRuntime()) {
    if (mockRecordingStatus.state === "listening") {
      mockRecordingStatus = {
        ...mockRecordingStatus,
        durationMs: mockRecordingAccumulatedMs + (Date.now() - mockRecordingStartedAt),
      };
    }
    return mockRecordingStatus;
  }
  return invoke<RecordingStatusResponse>("get_recording_status");
}

export async function getRecordingInputLevel(): Promise<number> {
  if (!isTauriRuntime()) {
    if (mockRecordingStatus.state !== "listening") {
      return 0;
    }
    const elapsed = Date.now() - mockRecordingStartedAt;
    return Math.max(0, Math.min(1, (Math.sin(elapsed / 180) + 1) / 2));
  }
  return invoke<number>("get_recording_input_level");
}

export async function startRecordingSession(): Promise<RecordingStatusResponse> {
  if (!isTauriRuntime()) {
    mockRecordingStartedAt = Date.now();
    mockRecordingAccumulatedMs = 0;
    mockRecordingStatus = {
      state: "listening",
      currentSessionId: crypto.randomUUID(),
      activeInputDevice: "Browser preview microphone",
      lastRecordingPath: mockRecordingStatus.lastRecordingPath,
      lastErrorMessage: null,
      durationMs: 0,
      sampleRateHz: 48000,
      channels: 2,
    };
    return mockRecordingStatus;
  }
  return invoke<RecordingStatusResponse>("start_recording_session");
}

export async function stopRecordingSession(): Promise<RecordingResult> {
  if (!isTauriRuntime()) {
    const durationMs =
      mockRecordingStatus.state === "listening"
        ? mockRecordingAccumulatedMs + (Date.now() - mockRecordingStartedAt)
        : mockRecordingAccumulatedMs;
    const result: RecordingResult = {
      sessionId: mockRecordingStatus.currentSessionId ?? crypto.randomUUID(),
      filePath: "mock://temp/browser-preview.wav",
      durationMs,
      sampleRateHz: 16000,
      channels: 1,
      sampleCount: 32000,
    };
    mockRecordingStatus = {
      state: "success",
      currentSessionId: result.sessionId,
      activeInputDevice: "Browser preview microphone",
      lastRecordingPath: result.filePath,
      lastErrorMessage: null,
      durationMs: result.durationMs,
      sampleRateHz: result.sampleRateHz,
      channels: result.channels,
    };
    mockRecordingAccumulatedMs = 0;
    return result;
  }
  return invoke<RecordingResult>("stop_recording_session");
}

export async function cancelRecordingSession(): Promise<RecordingStatusResponse> {
  if (!isTauriRuntime()) {
    mockRecordingAccumulatedMs = 0;
    mockRecordingStatus = {
      ...mockRecordingStatus,
      state: "idle",
      currentSessionId: null,
      durationMs: null,
      sampleRateHz: null,
      channels: null,
      lastErrorMessage: null,
    };
    return mockRecordingStatus;
  }
  return invoke<RecordingStatusResponse>("cancel_recording_session");
}

export async function getQuickDictateStatus(): Promise<QuickDictationStatusResponse> {
  if (!isTauriRuntime()) {
    return mockQuickDictationStatus;
  }
  return invoke<QuickDictationStatusResponse>("get_quick_dictate_status");
}

/**
 * Force dictation back to a clean Idle state. Recovers a wedged audio worker,
 * hides the overlay, restores volume, and re-arms the shortcut. Surfaced in the
 * UI as the "Reset" action when dictation looks unresponsive.
 */
export async function resetQuickDictation(): Promise<QuickDictationStatusResponse> {
  if (!isTauriRuntime()) {
    return mockQuickDictationStatus;
  }
  return invoke<QuickDictationStatusResponse>("reset_quick_dictation");
}

/** Snapshot of the prerequisites for shortcut dictation (model, shortcut,
 * Accessibility). Drives the Home readiness checklist. */
export async function getDictationReadiness(): Promise<DictationReadiness> {
  if (!isTauriRuntime()) {
    return {
      hasModel: true,
      shortcutRegistered: true,
      autoPasteEnabled: true,
      accessibilityRequired: false,
      accessibilityGranted: true,
    };
  }
  return invoke<DictationReadiness>("get_dictation_readiness");
}

/** Open the OS pane where the user grants Accessibility access (macOS). */
export async function openAccessibilitySettings(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke<void>("open_accessibility_settings");
}

export async function listenQuickDictateStatus(
  handler: (status: QuickDictationStatusResponse) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) {
    return () => undefined;
  }

  const { listen } = await import("@tauri-apps/api/event");
  return listen<QuickDictationStatusResponse>("quick-dictate-status", (event) => {
    handler(event.payload);
  });
}

export async function listenFileTranscriptionStatus(
  handler: (status: FileTranscriptionStatusEvent) => void,
) {
  if (!isTauriRuntime()) {
    mockFileTranscriptionListeners.add(handler);
    return () => {
      mockFileTranscriptionListeners.delete(handler);
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<FileTranscriptionStatusEvent>("file-transcription-status", (event) => {
    handler(event.payload);
  });
}

function emitMockFileTranscriptionStatus(event: FileTranscriptionStatusEvent) {
  const current = mockFileTranscriptionStatuses.get(event.jobId);
  mockFileTranscriptionStatuses.set(event.jobId, {
    ...event,
    startedAtMs: current?.startedAtMs ?? event.startedAtMs,
    updatedAtMs: Date.now(),
  });
  const nextEvent = mockFileTranscriptionStatuses.get(event.jobId)!;
  for (const listener of mockFileTranscriptionListeners) {
    listener(nextEvent);
  }
}

function emitMockModelDownloadStatus(status: ModelDownloadStatus) {
  for (const listener of mockModelDownloadListeners) {
    listener(status);
  }
}

export async function listVocabularyTerms(): Promise<VocabularyTerm[]> {
  if (!isTauriRuntime()) {
    return mockVocabularyTerms;
  }
  return invoke<VocabularyTerm[]>("list_vocabulary_terms");
}

export async function createVocabularyTerm(
  input: CreateVocabularyTermInput,
): Promise<VocabularyTerm> {
  if (!isTauriRuntime()) {
    const now = new Date().toISOString();
    const term: VocabularyTerm = {
      id: crypto.randomUUID(),
      canonical: input.canonical,
      normalizedCanonical: input.canonical.trim().toLowerCase(),
      category: input.category?.trim() || "custom",
      languageHint: input.languageHint?.trim() || null,
      matchMode: input.matchMode,
      isBuiltin: false,
      createdAt: now,
      updatedAt: now,
      aliases: input.aliases
        .filter((alias) => alias.trim().length > 0)
        .map((alias) => ({
          id: crypto.randomUUID(),
          alias,
          normalizedAlias: alias.trim().toLowerCase(),
        })),
    };
    mockVocabularyTerms.push(term);
    return term;
  }
  return invoke<VocabularyTerm>("create_vocabulary_term", { input });
}

export async function updateVocabularyTerm(
  termId: string,
  input: UpdateVocabularyTermInput,
): Promise<VocabularyTerm> {
  if (!isTauriRuntime()) {
    const index = mockVocabularyTerms.findIndex((term) => term.id === termId);
    if (index < 0) {
      throw new Error(`Vocabulary term ${termId} not found`);
    }
    const current = mockVocabularyTerms[index];
    const updated: VocabularyTerm = {
      ...current,
      canonical: input.canonical,
      normalizedCanonical: input.canonical.trim().toLowerCase(),
      category: input.category?.trim() || "custom",
      languageHint: input.languageHint?.trim() || null,
      matchMode: input.matchMode,
      updatedAt: new Date().toISOString(),
      aliases: input.aliases
        .filter((alias) => alias.trim().length > 0)
        .map((alias) => ({
          id: crypto.randomUUID(),
          alias,
          normalizedAlias: alias.trim().toLowerCase(),
        })),
    };
    mockVocabularyTerms[index] = updated;
    return updated;
  }
  return invoke<VocabularyTerm>("update_vocabulary_term", { termId, input });
}

export async function deleteVocabularyTerm(termId: string): Promise<void> {
  if (!isTauriRuntime()) {
    const index = mockVocabularyTerms.findIndex((term) => term.id === termId);
    if (index >= 0) {
      mockVocabularyTerms.splice(index, 1);
    }
    return;
  }
  return invoke("delete_vocabulary_term", { termId });
}
