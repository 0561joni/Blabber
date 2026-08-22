export type SourceType = "quick_dictate" | "file_upload";

export type TranscriptStatus =
  | "queued"
  | "recording"
  | "processing"
  | "completed"
  | "failed"
  | "canceled";

export type TranscriptQualityStatus = "clean" | "recovered" | "partial";

export type LanguageMode = "auto" | "fixed";
export type InsertBehavior = "paste" | "clipboard_only";
export type ModelProfile = "fast" | "balanced" | "accurate";
export type ModelCapability = "asr" | "vad" | "diarization";
export type ModelUseContext = "shortcut_dictation" | "quick_dictate" | "file_transcription";
export type ModelLanguageControl = "automatic_and_fixed" | "automatic_only";
export type DiarizationSource = "none" | "native_model" | "post_process";
export type DiarizationStatus = "not_requested" | "pending" | "running" | "completed" | "completed_with_uncertainty" | "failed" | "canceled" | "not_enough_speech";
export type SpeakerAttribution = "none" | "assigned" | "likely" | "uncertain" | "overlap";
export type ShortcutMode = "push_to_talk" | "toggle";

export interface HealthCheckResponse {
  appName: string;
  appVersion: string;
  platform: string;
  dbPath: string;
  tempDir: string;
  modelsDir: string;
  startupNotices: string[];
}

export interface PlatformInfo {
  os: string;
  isWayland: boolean;
  isGnome: boolean;
  hasAppindicatorHint: boolean;
  autoPasteSupported: boolean;
  globalShortcutSupported: boolean;
  dictateToggleExecutable: string | null;
  dictateToggleCommand: string | null;
}

export interface TrayUnavailableClosePayload {
  title: string;
  message: string;
}

export interface TranscriptSummary {
  id: string;
  createdAt: string;
  sourceType: SourceType;
  title: string;
  plainText: string;
  status: TranscriptStatus;
  detectedLanguages: string[];
  durationMs: number | null;
  modelName: string | null;
  qualityStatus: TranscriptQualityStatus;
  recoveredRegionCount: number;
  diarizationStatus: DiarizationStatus;
  speakerCount: number | null;
}

export interface AppSettings {
  defaultMode: "quick_dictate" | "file_transcribe";
  shortcut: string;
  shortcutMode: ShortcutMode;
  languageMode: LanguageMode;
  fixedLanguage: string | null;
  preferredInputDevice: string | null;
  insertBehavior: InsertBehavior;
  launchAtLoginEnabled: boolean;
  gpuEnabled: boolean;
  shortcutDictationModelProfile: ModelProfile;
  shortcutDictationSelectedModelId: string | null;
  quickDictateModelProfile: ModelProfile;
  quickDictateSelectedModelId: string | null;
  fileTranscribeModelProfile: ModelProfile;
  fileTranscribeSelectedModelId: string | null;
  saveHistory: boolean;
  soundsEnabled: boolean;
  volumeDuckingEnabled: boolean;
  fileDiarizationEnabled: boolean;
}

export interface SettingsPatch {
  defaultMode?: "quick_dictate" | "file_transcribe";
  shortcut?: string;
  shortcutMode?: ShortcutMode;
  languageMode?: LanguageMode;
  fixedLanguage?: string | null;
  preferredInputDevice?: string | null;
  insertBehavior?: InsertBehavior;
  launchAtLoginEnabled?: boolean;
  gpuEnabled?: boolean;
  shortcutDictationModelProfile?: ModelProfile;
  shortcutDictationSelectedModelId?: string | null;
  quickDictateModelProfile?: ModelProfile;
  quickDictateSelectedModelId?: string | null;
  fileTranscribeModelProfile?: ModelProfile;
  fileTranscribeSelectedModelId?: string | null;
  saveHistory?: boolean;
  soundsEnabled?: boolean;
  volumeDuckingEnabled?: boolean;
  fileDiarizationEnabled?: boolean;
}

export interface InstalledModel {
  id: string;
  engine: string;
  modelName: string;
  variant: string;
  localPath: string;
  sizeBytes: number;
  isDefault: boolean;
  profile: ModelProfile;
  capabilities?: ModelCapabilities;
}

export interface ModelCapabilities {
  supportedContexts: ModelUseContext[];
  nativeDiarization: boolean;
  timestampedSegments: boolean;
  contextSupport: boolean;
  languageControl: ModelLanguageControl;
  maximumAudioDurationMs: number | null;
}

export interface DownloadableModel {
  id: string;
  engine: string;
  modelName: string;
  description: string;
  sizeBytes: number;
  profile: ModelProfile;
  availability: "available" | "unsupported_platform";
  availabilityReason: string | null;
  installed: boolean;
  requirements: string | null;
  artifactCount: number;
  capability: ModelCapability;
  capabilities?: ModelCapabilities;
}

export type ModelDownloadState =
  | "idle"
  | "downloading"
  | "completed"
  | "canceled"
  | "failed";

export interface ModelDownloadStatus {
  modelId: string;
  modelName: string;
  state: ModelDownloadState;
  downloadedBytes: number;
  totalBytes: number | null;
  progressPercent: number | null;
  errorMessage: string | null;
  currentArtifact: string | null;
  artifactIndex: number | null;
  artifactCount: number;
}

export type PreviewSourceKind = "quick_dictate" | "file_upload";

export interface TranscriptSegment {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
  languageCode: string;
  segmentOrder: number;
  confidence: number | null;
  speakerId: string | null;
  speakerIds: string[] | null;
  speakerAttribution: SpeakerAttribution;
  speakerConfidence: number | null;
}

export interface DiarizationTurn { id: string; startMs: number; endMs: number; speakerIds: string[]; confidence: number | null; isOverlap: boolean; isUncertain: boolean; turnOrder: number; }
export interface TranscriptSpeaker { speakerId: string; displayName: string; speakerOrder: number; }

export interface TranscriptResult {
  jobId: string;
  modelName: string;
  fullText: string;
  plainText: string;
  timestampedText: string;
  detectedLanguages: string[];
  segments: TranscriptSegment[];
  qualityStatus: TranscriptQualityStatus;
  recoveredRegionCount: number;
  warnings: TranscriptWarning[];
  diarizationStatus: DiarizationStatus;
  diarizationModelId: string | null;
  diarizationSource?: DiarizationSource;
  diarizationWarning: string | null;
  diarizationPolicyVersion: number | null;
  diarizationClusteringThreshold: number | null;
  diarizationSpeakerCountHint: number | null;
  speakers: TranscriptSpeaker[];
  diarizationTurns: DiarizationTurn[];
}

export interface TranscriptDetail extends TranscriptSummary {
  fullText: string;
  timestampedText: string;
  transcriptionWarnings: TranscriptWarning[];
  diarizationModelId: string | null;
  diarizationSource?: DiarizationSource;
  diarizationWarning: string | null;
  diarizationPolicyVersion: number | null;
  diarizationClusteringThreshold: number | null;
  diarizationSpeakerCountHint: number | null;
  segments: TranscriptSegment[];
  speakers: TranscriptSpeaker[];
  diarizationTurns: DiarizationTurn[];
}

export type TranscriptExportFormat = "txt" | "md" | "srt" | "vtt" | "json";
export type TranscriptCopyVariant = "speaker_aware" | "plain";
export interface TranscriptExportResult { path: string | null; }

export interface TranscriptWarning {
  startMs: number;
  endMs: number;
  reason: string;
  attempts: number;
  outcome: string;
}

export interface EngineErrorPayload {
  code: string;
  message: string;
}

export interface TranscriptionPreviewRequest {
  sourceKind: PreviewSourceKind;
  profile: ModelProfile;
  selectedModelId?: string | null;
  languageMode: LanguageMode;
  fixedLanguage: string | null;
  timestamps: boolean;
  preferGpu: boolean;
  filePath?: string | null;
}

export interface TranscriptionPreviewResponse {
  sourceKind: PreviewSourceKind;
  resolvedModel: InstalledModel | null;
  result: TranscriptResult | null;
  error: EngineErrorPayload | null;
}

export interface SelectedSourceFile {
  filePath: string;
  originalName: string;
  mimeType: string;
  sizeBytes: number;
  durationMs: number | null;
  sha256: string | null;
}

export interface InputDeviceOption {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface FileTranscriptionRequest {
  jobId: string;
  sourceFile: SelectedSourceFile;
  speakerCountHint: number | null;
}

export interface RediarizationRequest {
  jobId: string;
  transcriptId: string;
  sourceFile: SelectedSourceFile | null;
  speakerCountHint: number | null;
}

export type RediarizationStage = "queued" | "validating" | "diarizing" | "saving" | "completed" | "failed" | "canceled";
export interface RediarizationStatusEvent {
  jobId: string;
  transcriptId: string;
  stage: RediarizationStage;
  statusText: string;
  errorMessage: string | null;
}

export type FileTranscriptionJobStage =
  | "queued"
  | "preparing"
  | "transcribing"
  | "diarizing"
  | "saving"
  | "completed"
  | "failed"
  | "canceled";

export interface StartFileTranscriptionResponse {
  jobId: string;
}

export interface FileTranscriptionResponse {
  sourceFile: SelectedSourceFile;
  resolvedModel: InstalledModel | null;
  result: TranscriptResult;
  savedTranscript: TranscriptSummary | null;
}

export interface FileTranscriptionStatusEvent {
  jobId: string;
  sourceFile: SelectedSourceFile;
  stage: FileTranscriptionJobStage;
  progressPercent: number | null;
  processedMs: number | null;
  totalMs: number | null;
  etaSeconds: number | null;
  statusText: string;
  result: FileTranscriptionResponse | null;
  errorMessage: string | null;
  startedAtMs: number;
  updatedAtMs: number;
}

export type FileQueueCopyState = "idle" | "copied" | "error";

export interface FileQueueItem {
  id: string;
  sourceFile: SelectedSourceFile;
  stage: FileTranscriptionJobStage;
  progressPercent: number | null;
  processedMs: number | null;
  totalMs: number | null;
  etaSeconds: number | null;
  statusText: string;
  result: FileTranscriptionResponse | null;
  errorMessage: string | null;
  startedAt: number | null;
  isExpanded: boolean;
  copyState: FileQueueCopyState;
}

export type RecordingOverlayState =
  | "idle"
  | "listening"
  | "paused"
  | "processing"
  | "success"
  | "error";

export interface RecordingStatusResponse {
  state: RecordingOverlayState;
  currentSessionId: string | null;
  activeInputDevice: string | null;
  lastRecordingPath: string | null;
  lastErrorMessage: string | null;
  durationMs: number | null;
  sampleRateHz: number | null;
  channels: number | null;
}

export interface RecordingResult {
  sessionId: string;
  filePath: string;
  durationMs: number;
  sampleRateHz: number;
  channels: number;
  sampleCount: number;
}

export type ManualTranscriptionStage = "idle" | "processing" | "failed";

export interface ManualTranscriptionUiState {
  stage: ManualTranscriptionStage;
  statusText: string;
  startedAt: number | null;
  errorMessage: string | null;
}

export type QuickDictationState =
  | "idle"
  | "listening"
  | "processing"
  | "inserted"
  | "clipboard_only"
  | "error";

export type InsertionOutcome = "pasted" | "clipboard_only";

export interface QuickDictationStatusResponse {
  state: QuickDictationState;
  registeredShortcut: string | null;
  shortcutMode: ShortcutMode;
  isRegistered: boolean;
  lastTranscriptText: string | null;
  lastTranscriptId: string | null;
  lastRecordingPath: string | null;
  lastErrorMessage: string | null;
  lastModelName: string | null;
  lastInsertOutcome: InsertionOutcome | null;
  lastDurationMs: number | null;
}

export interface DictationReadiness {
  hasModel: boolean;
  shortcutRegistered: boolean;
  autoPasteEnabled: boolean;
  accessibilityRequired: boolean;
  accessibilityGranted: boolean;
}

export interface VocabularyAlias {
  id: string;
  alias: string;
  normalizedAlias: string;
}

export type VocabularyMatchMode = "exact_only" | "exact_and_fuzzy";

export interface VocabularyTerm {
  id: string;
  canonical: string;
  normalizedCanonical: string;
  matchMode: VocabularyMatchMode;
  isBuiltin: boolean;
  createdAt: string;
  updatedAt: string;
  aliases: VocabularyAlias[];
}

export interface CreateVocabularyTermInput {
  canonical: string;
  aliases: string[];
  matchMode: VocabularyMatchMode;
}

export interface UpdateVocabularyTermInput {
  canonical: string;
  aliases: string[];
  matchMode: VocabularyMatchMode;
}
