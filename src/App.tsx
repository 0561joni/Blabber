import { useCallback, useEffect, useRef, useState } from "react";
import {
  reportManualFeedback,
  dismissFileTranscription,
  invalidateFileReview,
  cancelFileTranscription,
  deleteAllTranscripts,
  deleteTranscript,
  getFileTranscriptionStatuses,
  getModelDownloadStatuses,
  listenModelDownloadStatus,
  getStartupStatus,
  getHealthCheck,
  getDictationReadiness,
  getQuickDictateStatus,
  getRecordingStatus,
  getSettings,
  openAccessibilitySettings,
  listInstalledModels,
  listDownloadableModels,
  listTranscripts,
  listVocabularyTerms,
  listenFileTranscriptionStatus,
  listenQuickDictateStatus,
  listenTrayUnavailableCloseRequested,
  listenStartupStatus,
  pickAudioFiles,
  prepareDroppedAudioFiles,
  previewTranscription,
  quitApp,
  startFileTranscription,
  startRecordingSession,
  stopRecordingSession,
  cancelRecordingSession,
  resetQuickDictation,
  updateSettings,
  createVocabularyTerm,
  updateVocabularyTerm,
  deleteVocabularyTerm,
  frontendStartupComplete,
  reportStartupFailure,
} from "./lib/api";
import { ReviewWorkspace } from "./screens/ReviewWorkspace";
import { useReviewJobs } from "./hooks/useReviewJobs";
import { reviewKey, isReviewJobActive } from "./lib/reviewApi";
import type { ReviewRef } from "./types/domain";
import { DictateScreen } from "./screens/DictateScreen";
import { FilesScreen, isFileWorking } from "./screens/FilesScreen";
import { applyAppearance } from "./lib/appearance";
import { SettingsScreen } from "./screens/SettingsScreen";
import { HistoryScreen } from "./screens/HistoryScreen";
import { VocabularyScreen } from "./screens/VocabularyScreen";
import { AppIcon, IconButton, type AppIconName } from "./components/IconButton";
import { formatPasteShortcutForDisplay } from "./lib/formatting";
import { useAccessibilityReadinessPolling } from "./hooks/useAccessibilityReadinessPolling";
import type {
  AppSettings,
  DictationReadiness,
  FileQueueItem,
  FileTranscriptionStatusEvent,
  HealthCheckResponse,
  InstalledModel,
  ManualTranscriptionUiState,
  QuickDictationStatusResponse,
  RecordingStatusResponse,
  SettingsPatch,
  TranscriptSummary,
  TrayUnavailableClosePayload,
  TranscriptionPreviewResponse,
  VocabularyTerm,
} from "./types/domain";

type ScreenId = "dictate" | "files" | "settings" | "vocabulary" | "history";

type ToastKind = "success" | "info" | "error";

interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  hint?: string;
  durationMs: number;
}

const TERMINAL_DICTATION_STATES = new Set([
  "inserted",
  "clipboard_only",
  "error",
]);

const NAV_ITEMS: Array<{ id: ScreenId; label: string; icon: AppIconName }> = [
  { id: "dictate", label: "Dictate", icon: "microphone" },
  { id: "files", label: "Transcribe files", icon: "folder" },
  { id: "history", label: "Library", icon: "clock" },
  { id: "vocabulary", label: "Vocabulary", icon: "book" },
  { id: "settings", label: "Settings", icon: "gear" },
];

export function App() {
  const [reviewTarget, setReviewTarget] = useState<{
    reference: ReviewRef;
    originLabel: string;
    scroll: number;
    anchor: HTMLElement | null;
  } | null>(null);
  const [screen, setScreen] = useState<ScreenId>("dictate");
  const [settingsSection, setSettingsSection] = useState("general");
  const [downloadCount, setDownloadCount] = useState(0);
  const downloadStages = useRef(new Map<string, string>());
  const currentSettingsSection = useRef(settingsSection);
  currentSettingsSection.current = settingsSection;
  const [libraryVisited, setLibraryVisited] = useState(false);
  const [sidebarExpanded, setSidebarExpanded] = useState(true);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [health, setHealth] = useState<HealthCheckResponse | null>(null);
  const { jobs: reviewJobs, accept: acceptReviewJob } = useReviewJobs(
    Boolean(health),
  );
  const [speakerModelReady, setSpeakerModelReady] = useState<boolean | null>(
    null,
  );
  const [installedModels, setInstalledModels] = useState<InstalledModel[]>([]);
  const [transcripts, setTranscripts] = useState<TranscriptSummary[]>([]);
  const [vocabularyTerms, setVocabularyTerms] = useState<VocabularyTerm[]>([]);
  const [preview, setPreview] = useState<TranscriptionPreviewResponse | null>(
    null,
  );
  const [recordingStatus, setRecordingStatus] =
    useState<RecordingStatusResponse | null>(null);
  const [manualTranscriptionState, setManualTranscriptionState] =
    useState<ManualTranscriptionUiState>({
      stage: "idle",
      statusText: "",
      startedAt: null,
      errorMessage: null,
    });
  const [quickDictationStatus, setQuickDictationStatus] =
    useState<QuickDictationStatusResponse | null>(null);
  const [dictationError, setDictationError] = useState<string | null>(null);
  const [readiness, setReadiness] = useState<DictationReadiness | null>(null);
  const [fileQueueItems, setFileQueueItems] = useState<FileQueueItem[]>([]);
  const [isFileDragActive, setIsFileDragActive] = useState(false);
  const [speakerCountHint, setSpeakerCountHint] = useState<number | null>(null);
  const speakerCountHintRef = useRef<number | null>(null);
  speakerCountHintRef.current = speakerCountHint;
  const currentScreenRef = useRef<ScreenId>("dictate");
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [trayClosePrompt, setTrayClosePrompt] =
    useState<TrayUnavailableClosePayload | null>(null);
  const toastIdRef = useRef(0);
  const mainRef = useRef<HTMLElement>(null);
  const manualGeneration = useRef(0);
  const fileEventStages = useRef(new Map<string, string>());
  const fileModelReady = useRef(false);
  fileModelReady.current = installedModels.some(
    (model) =>
      !model.capabilities ||
      model.capabilities.supportedContexts.includes("file_transcription"),
  );
  const statusRefreshBusy = useRef(false);
  useEffect(() => {
    let disposed = false;
    void listDownloadableModels()
      .then((models) => {
        if (!disposed)
          setSpeakerModelReady(
            models.find((m) => m.capability === "diarization")?.installed ??
              false,
          );
      })
      .catch(() => {});
    return () => {
      disposed = true;
    };
  }, [installedModels, settings?.fileDiarizationEnabled]);
  const fileHints = useRef(new Map<string, number | null>());
  const lastDictationStateRef = useRef<string | null>(null);

  const dismissToast = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const pushToast = useCallback(
    (toast: Omit<Toast, "id">) => {
      const id = ++toastIdRef.current;
      setToasts((current) => [...current, { ...toast, id }]);
      if (toast.durationMs > 0) {
        window.setTimeout(() => dismissToast(id), toast.durationMs);
      }
    },
    [dismissToast],
  );

  const showDroppedAudioError = useCallback(
    (error: unknown) => {
      const message = errorMessage(
        error,
        "Drop WAV, MP3, M4A, or OPUS files to transcribe.",
      );
      console.error(message);
      pushToast({
        kind: "error",
        message: "Unsupported audio file",
        hint: message,
        durationMs: 5000,
      });
    },
    [pushToast],
  );

  const refreshReadiness =
    useCallback(async (): Promise<DictationReadiness | null> => {
      try {
        const nextReadiness = await getDictationReadiness();
        setReadiness(nextReadiness);
        return nextReadiness;
      } catch (error) {
        console.error(
          errorMessage(error, "Failed to check dictation readiness."),
        );
        return null;
      }
    }, []);

  const {
    isPolling: isPollingAccessibility,
    startPolling: startAccessibilityPolling,
    stopPolling: stopAccessibilityPolling,
  } = useAccessibilityReadinessPolling(refreshReadiness);

  useEffect(() => {
    let disposed = false;
    let startupStarted = false;
    let unlisten: (() => void) | null = null;

    const handleStartupStatus = async (
      status: Awaited<ReturnType<typeof getStartupStatus>>,
    ) => {
      if (
        disposed ||
        startupStarted ||
        (status.phase !== "workspace" && status.phase !== "ready")
      ) {
        return;
      }
      startupStarted = true;
      try {
        await loadAppState();
        if (!disposed) {
          await frontendStartupComplete();
        }
      } catch (error) {
        const message = errorMessage(
          error,
          "Failed to load the Blabber workspace.",
        );
        console.error(message);
        await reportStartupFailure(message).catch(() => undefined);
      }
    };

    void listenStartupStatus((status) => {
      void handleStartupStatus(status);
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
      } else {
        unlisten = cleanup;
      }
    });
    void getStartupStatus()
      .then(handleStartupStatus)
      .catch(async (error) => {
        const message = errorMessage(error, "Could not read startup status.");
        console.error(message);
        await reportStartupFailure(message).catch(() => undefined);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (
      readiness &&
      (!readiness.accessibilityRequired || readiness.accessibilityGranted)
    ) {
      stopAccessibilityPolling();
    }
  }, [
    readiness?.accessibilityGranted,
    readiness?.accessibilityRequired,
    stopAccessibilityPolling,
  ]);

  useEffect(() => {
    if (
      recordingStatus?.state !== "listening" &&
      quickDictationStatus?.state !== "listening"
    ) {
      return;
    }
    const interval = window.setInterval(() => {
      void getRecordingStatus()
        .then(setRecordingStatus)
        .catch(() => undefined);
    }, 400);
    return () => window.clearInterval(interval);
  }, [recordingStatus?.state, quickDictationStatus?.state]);

  useEffect(() => {
    currentScreenRef.current = screen;
    mainRef.current?.scrollTo?.({ top: 0 });
    if (screen === "history") setLibraryVisited(true);
    if (screen !== "files") {
      setIsFileDragActive(false);
    }
  }, [screen]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void listenQuickDictateStatus(async (nextStatus) => {
      setQuickDictationStatus(nextStatus);
      void getRecordingStatus()
        .then(setRecordingStatus)
        .catch(() => undefined);
      if (nextStatus.state === "error")
        setDictationError(nextStatus.lastErrorMessage ?? "Dictation failed.");
      else if (nextStatus.state === "listening") setDictationError(null);

      const previousState = lastDictationStateRef.current;
      lastDictationStateRef.current = nextStatus.state;
      if (
        previousState !== null &&
        previousState !== nextStatus.state &&
        currentScreenRef.current !== "dictate" &&
        document.visibilityState === "visible" &&
        TERMINAL_DICTATION_STATES.has(nextStatus.state)
      ) {
        switch (nextStatus.state) {
          case "inserted":
            pushToast({
              kind: "success",
              message: "Pasted ✓",
              durationMs: 1800,
            });
            break;
          case "clipboard_only":
            pushToast({
              kind: "info",
              message: "Copied to clipboard",
              hint: `Press ${formatPasteShortcutForDisplay(health?.platform ?? null)} to paste`,
              durationMs: 3500,
            });
            break;
          case "error":
            pushToast({
              kind: "error",
              message: "Dictation failed",
              hint: nextStatus.lastErrorMessage ?? undefined,
              durationMs: 0, // persistent until dismissed
            });
            break;
        }
      }

      if (
        nextStatus.lastTranscriptId ||
        nextStatus.state === "inserted" ||
        nextStatus.state === "clipboard_only"
      ) {
        try {
          setTranscripts(await listTranscripts(""));
        } catch (error) {
          console.error(
            errorMessage(error, "Failed to refresh shortcut transcripts."),
          );
        }
      }
    }).then((cleanup) => {
      unlisten = cleanup;
    });
    return () => {
      unlisten?.();
    };
  }, [health?.platform, pushToast]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void listenTrayUnavailableCloseRequested((payload) => {
      setTrayClosePrompt(payload);
    }).then((cleanup) => {
      unlisten = cleanup;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const refreshVisibleState = () => {
      void refreshShortcutOutputs();
      void refreshFileStatuses();
      void refreshReadiness();
    };

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        refreshVisibleState();
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("focus", refreshVisibleState);

    let disposed = false;
    let unlistenFocus: (() => void) | null = null;

    if ("__TAURI_INTERNALS__" in window) {
      void import("@tauri-apps/api/window")
        .then(async ({ getCurrentWindow }) => {
          const cleanup = await getCurrentWindow().onFocusChanged(
            ({ payload }) => {
              if (disposed) {
                return;
              }
              if (payload) {
                refreshVisibleState();
              }
            },
          );

          if (disposed) {
            cleanup();
            return;
          }
          unlistenFocus = cleanup;
        })
        .catch(() => undefined);
    }

    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", refreshVisibleState);
      unlistenFocus?.();
    };
  }, [refreshReadiness]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void listenFileTranscriptionStatus(async (event) => {
      const previousStage = fileEventStages.current.get(event.jobId);
      fileEventStages.current.set(event.jobId, event.stage);
      if (
        previousStage &&
        previousStage !== event.stage &&
        (event.stage === "completed" || event.stage === "failed") &&
        currentScreenRef.current !== "files"
      ) {
        pushToast({
          kind: event.stage === "failed" ? "error" : "success",
          message:
            event.stage === "failed"
              ? "File needs attention"
              : "Transcript ready",
          hint: event.sourceFile.originalName,
          durationMs: event.stage === "failed" ? 0 : 4000,
        });
      }
      setFileQueueItems((current) => mergeFileStatusIntoQueue(current, event));
      if (event.result?.savedTranscript) {
        setTranscripts((current) =>
          prependTranscriptUnique(current, event.result!.savedTranscript!),
        );
      }
    }).then((cleanup) => {
      unlisten = cleanup;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  // Listen for drag-drop events emitted from the Rust backend via on_webview_event.
  // This bypasses Tauri's onDragDropEvent JS API which has known issues on macOS,
  // and uses the same global event system that file-transcription-status events use.
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }

    let disposed = false;
    const unlisteners: (() => void)[] = [];

    void import("@tauri-apps/api/event")
      .then(async ({ listen }) => {
        if (disposed) return;

        unlisteners.push(
          await listen("app://file-drag-enter", () => {
            if (currentScreenRef.current === "files") {
              setIsFileDragActive(true);
            }
          }),
        );

        unlisteners.push(
          await listen("app://file-drag-leave", () => {
            setIsFileDragActive(false);
          }),
        );

        unlisteners.push(
          await listen<string[]>("app://file-drop", async (event) => {
            setIsFileDragActive(false);
            if (currentScreenRef.current !== "files") return;
            try {
              const files = await prepareDroppedAudioFiles(event.payload);
              enqueueSelectedFiles(files, speakerCountHintRef.current);
            } catch (error) {
              showDroppedAudioError(error);
            }
          }),
        );
      })
      .catch((error) => {
        console.error(
          "[drag-drop] Failed to set up drag-drop listeners:",
          error,
        );
      });

    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [showDroppedAudioError]);

  const hasActiveFiles = fileQueueItems.some((item) =>
    isFileWorking(item.stage),
  );
  useEffect(() => {
    if (!hasActiveFiles) return;
    const interval = window.setInterval(() => {
      void refreshFileStatuses();
    }, 2000);
    return () => window.clearInterval(interval);
  }, [hasActiveFiles]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const updateCount = () =>
      setDownloadCount(
        [...downloadStages.current.values()].filter(
          (stage) => stage === "downloading",
        ).length,
      );
    void getModelDownloadStatuses()
      .then((statuses) => {
        if (disposed) return;
        statuses.forEach((status) => {
          if (!downloadStages.current.has(status.modelId))
            downloadStages.current.set(status.modelId, status.state);
        });
        updateCount();
      })
      .catch(() => undefined);
    void listenModelDownloadStatus((status) => {
      if (disposed) return;
      const previous = downloadStages.current.get(status.modelId);
      downloadStages.current.set(status.modelId, status.state);
      updateCount();
      if (
        previous === "downloading" &&
        (status.state === "completed" || status.state === "failed") &&
        (currentScreenRef.current !== "settings" ||
          currentSettingsSection.current !== "models")
      ) {
        pushToast({
          kind: status.state === "failed" ? "error" : "success",
          message:
            status.state === "failed"
              ? "Model download needs attention"
              : "Model installed",
          hint: status.errorMessage ?? status.modelName,
          durationMs: status.state === "failed" ? 0 : 4000,
        });
      }
      if (status.state === "completed") void reloadModelState();
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [pushToast]);

  async function loadAppState() {
    const [
      nextHealth,
      nextSettings,
      nextInstalledModels,
      nextTranscripts,
      nextVocabularyTerms,
      nextRecordingStatus,
      nextQuickDictateStatus,
      nextFileStatuses,
    ] = await Promise.all([
      getHealthCheck(),
      getSettings(),
      listInstalledModels(),
      listTranscripts(""),
      listVocabularyTerms(),
      getRecordingStatus(),
      getQuickDictateStatus(),
      getFileTranscriptionStatuses(),
    ]);

    setHealth(nextHealth);
    setSettings(nextSettings);
    setInstalledModels(nextInstalledModels);
    setScreen(
      nextSettings.defaultMode === "file_transcribe" ? "files" : "dictate",
    );
    applyAppearance(nextSettings);
    lastDictationStateRef.current = nextQuickDictateStatus.state;
    setTranscripts(nextTranscripts);
    setVocabularyTerms(nextVocabularyTerms);
    setRecordingStatus(nextRecordingStatus);
    setQuickDictationStatus(nextQuickDictateStatus);
    nextFileStatuses.forEach((event) =>
      fileEventStages.current.set(event.jobId, event.stage),
    );
    setFileQueueItems(mergeFileStatusesIntoQueue([], nextFileStatuses));
    for (const notice of nextHealth.startupNotices) {
      pushToast({
        kind: "info",
        message: "Model selection updated",
        hint: notice,
        durationMs: 8000,
      });
    }
    void refreshReadiness();
  }

  async function refreshShortcutOutputs() {
    try {
      const [nextQuickDictateStatus, nextTranscripts] = await Promise.all([
        getQuickDictateStatus(),
        listTranscripts(""),
      ]);
      setQuickDictationStatus(nextQuickDictateStatus);
      setTranscripts(nextTranscripts);
    } catch (error) {
      console.error(
        errorMessage(error, "Failed to refresh shortcut output state."),
      );
    }
  }

  async function refreshFileStatuses() {
    if (statusRefreshBusy.current) return;
    statusRefreshBusy.current = true;
    try {
      const statuses = await getFileTranscriptionStatuses();
      setFileQueueItems((current) =>
        mergeFileStatusesIntoQueue(current, statuses),
      );
      for (const status of statuses) {
        if (status.result?.savedTranscript) {
          setTranscripts((current) =>
            prependTranscriptUnique(current, status.result!.savedTranscript!),
          );
        }
      }
    } catch (error) {
      console.error(
        errorMessage(error, "Failed to refresh file transcription statuses."),
      );
    } finally {
      statusRefreshBusy.current = false;
    }
  }

  async function saveSettings(patch: SettingsPatch) {
    const nextSettings = await updateSettings(patch);
    setSettings(nextSettings);
    applyAppearance(nextSettings);
    void getQuickDictateStatus()
      .then(setQuickDictationStatus)
      .catch(() => undefined);
    void refreshReadiness();
  }

  const reloadModelState = useCallback(async () => {
    const [nextSettings, nextInstalledModels] = await Promise.all([
      getSettings(),
      listInstalledModels(),
    ]);
    setSettings(nextSettings);
    setInstalledModels(nextInstalledModels);
    void refreshReadiness();
  }, [refreshReadiness]);

  const handleResolveReadiness = useCallback(
    async (item: "model" | "shortcut" | "accessibility") => {
      if (item === "accessibility") {
        if (isPollingAccessibility) {
          await refreshReadiness();
          return;
        }
        await openAccessibilitySettings();
        startAccessibilityPolling();
        return;
      }
      setSettingsSection(item === "model" ? "models" : "audio");
      setScreen("settings");
    },
    [isPollingAccessibility, refreshReadiness, startAccessibilityPolling],
  );

  async function removeTranscript(transcriptId: string) {
    await deleteTranscript(transcriptId);
    setTranscripts((current) =>
      current.filter((item) => item.id !== transcriptId),
    );
  }

  async function removeAllTranscripts() {
    await deleteAllTranscripts();
    setTranscripts([]);
  }

  async function beginManualRecording() {
    manualGeneration.current += 1;
    try {
      const nextRecording = await startRecordingSession();
      setRecordingStatus(nextRecording);
      setPreview(null);
      setManualTranscriptionState({
        stage: "idle",
        statusText: "",
        startedAt: null,
        errorMessage: null,
      });
    } catch (error) {
      void reportManualFeedback(crypto.randomUUID(), true).catch(
        () => undefined,
      );
      throw error;
    }
  }

  async function stopAndPreviewManualRecording() {
    if (!settings) {
      return;
    }
    const generation = manualGeneration.current;
    const startedAt = Date.now();
    setPreview(null);
    setManualTranscriptionState({
      stage: "processing",
      statusText: "Finishing your recording...",
      startedAt,
      errorMessage: null,
    });
    setRecordingStatus({
      state: "processing",
      currentSessionId: recordingStatus?.currentSessionId ?? null,
      activeInputDevice: recordingStatus?.activeInputDevice ?? null,
      lastRecordingPath: recordingStatus?.lastRecordingPath ?? null,
      lastErrorMessage: null,
      durationMs: recordingStatus?.durationMs ?? null,
      sampleRateHz: recordingStatus?.sampleRateHz ?? null,
      channels: recordingStatus?.channels ?? null,
    });

    try {
      const result = await stopRecordingSession();
      if (generation !== manualGeneration.current) return;
      setManualTranscriptionState({
        stage: "processing",
        statusText: "Transcribing your recording locally...",
        startedAt,
        errorMessage: null,
      });
      setRecordingStatus({
        state: "processing",
        currentSessionId: result.sessionId,
        activeInputDevice: recordingStatus?.activeInputDevice ?? null,
        lastRecordingPath: result.filePath,
        lastErrorMessage: null,
        durationMs: result.durationMs,
        sampleRateHz: result.sampleRateHz,
        channels: result.channels,
      });

      const nextPreview = await previewTranscription({
        sourceKind: "quick_dictate",
        profile: settings.quickDictateModelProfile,
        selectedModelId: settings.quickDictateSelectedModelId,
        languageMode: settings.languageMode,
        fixedLanguage: settings.fixedLanguage,
        timestamps: true,
        preferGpu: settings.gpuEnabled,
        filePath: result.filePath,
      });
      if (generation !== manualGeneration.current) return;
      setPreview(nextPreview);
      void reportManualFeedback(
        result.sessionId,
        Boolean(nextPreview.error),
      ).catch(() => undefined);
      if (nextPreview.error) {
        setManualTranscriptionState({
          stage: "failed",
          statusText: "Manual dictation failed.",
          startedAt,
          errorMessage: `${nextPreview.error.code}: ${nextPreview.error.message}`,
        });
      } else {
        setManualTranscriptionState({
          stage: "idle",
          statusText: "",
          startedAt: null,
          errorMessage: null,
        });
      }
    } catch (error) {
      if (generation !== manualGeneration.current) return;
      void reportManualFeedback(
        recordingStatus?.currentSessionId ?? crypto.randomUUID(),
        true,
      ).catch(() => undefined);
      setPreview(null);
      setManualTranscriptionState({
        stage: "failed",
        statusText: "Manual dictation failed.",
        startedAt,
        errorMessage: errorMessage(error, "Manual dictation failed."),
      });
    } finally {
      const status = await getRecordingStatus().catch(() => null);
      if (status && generation === manualGeneration.current)
        setRecordingStatus(status);
    }
  }

  async function cancelManualRecording() {
    const status = await cancelRecordingSession();
    manualGeneration.current += 1;
    setPreview(null);
    setManualTranscriptionState({
      stage: "idle",
      statusText: "Recording canceled",
      startedAt: null,
      errorMessage: null,
    });
    setRecordingStatus(status);
  }

  async function resetDictation() {
    await resetQuickDictation().then((status) => {
      manualGeneration.current += 1;
      setDictationError(null);
      setManualTranscriptionState({
        stage: "idle",
        statusText: "Dictation reset",
        startedAt: null,
        errorMessage: null,
      });
      setQuickDictationStatus(status);
    });
    setRecordingStatus(await getRecordingStatus());
  }

  async function createVocabulary(
    input: Parameters<typeof createVocabularyTerm>[0],
  ) {
    const term = await createVocabularyTerm(input);
    setVocabularyTerms((current) =>
      [...current, term].sort((left, right) =>
        left.canonical.localeCompare(right.canonical),
      ),
    );
  }

  async function updateVocabulary(
    termId: string,
    input: Parameters<typeof updateVocabularyTerm>[1],
  ) {
    const updated = await updateVocabularyTerm(termId, input);
    setVocabularyTerms((current) =>
      current
        .map((term) => (term.id === termId ? updated : term))
        .sort((left, right) => left.canonical.localeCompare(right.canonical)),
    );
  }

  async function removeVocabulary(termId: string) {
    await deleteVocabularyTerm(termId);
    setVocabularyTerms((current) =>
      current.filter((term) => term.id !== termId),
    );
  }

  async function enqueueFiles(speakerCountHint: number | null) {
    const files = await pickAudioFiles();
    enqueueSelectedFiles(files, speakerCountHint);
  }

  async function retryFile(itemId: string) {
    const item = fileQueueItems.find((entry) => entry.id === itemId);
    if (!item) return;
    let files: FileQueueItem["sourceFile"][];
    try {
      files = await prepareDroppedAudioFiles([item.sourceFile.filePath]);
    } catch {
      files = await pickAudioFiles();
      if (!files.length)
        throw new Error("Choose the original audio file to retry.");
    }
    enqueueSelectedFiles(
      files.slice(0, 1),
      fileHints.current.get(itemId) ?? null,
    );
  }

  async function cancelFile(itemId: string) {
    await cancelFileTranscription(itemId);
    await refreshFileStatuses();
  }

  function enqueueSelectedFiles(
    files: FileQueueItem["sourceFile"][],
    speakerCountHint: number | null,
  ) {
    if (files.length === 0) {
      return;
    }
    if (!fileModelReady.current) {
      pushToast({
        kind: "error",
        message: "A speech model is needed",
        hint: "Open Settings → Models to download a model for file transcription.",
        durationMs: 0,
      });
      return;
    }
    const queuedItems = files.map((sourceFile) =>
      createQueuedFileItem(crypto.randomUUID(), sourceFile),
    );
    setFileQueueItems((current) =>
      mergeFileStatusesIntoQueue([...queuedItems, ...current], []),
    );

    for (const item of queuedItems) {
      fileHints.current.set(item.id, speakerCountHint);
      fileEventStages.current.set(item.id, "queued");
      void startFileTranscription({
        jobId: item.id,
        sourceFile: item.sourceFile,
        speakerCountHint,
      }).catch((error) => {
        const message = errorMessage(
          error,
          "Failed to transcribe the selected file.",
        );
        setFileQueueItems((current) =>
          current.map((entry) =>
            entry.id === item.id
              ? {
                  ...entry,
                  stage: "failed",
                  statusText: "File transcription failed.",
                  errorMessage: message,
                }
              : entry,
          ),
        );
      });
    }
  }

  async function handleDroppedFiles(
    fileList: FileList,
    speakerCountHint: number | null,
  ) {
    // Extract file paths from the HTML5 FileList.
    // In Tauri's webview, File objects from drag-and-drop carry the native path.
    // In a plain browser, we fall back to the file name.
    const paths: string[] = [];
    for (let i = 0; i < fileList.length; i++) {
      const file = fileList[i];
      // Tauri webview exposes the native file path on the File object
      const filePath = (file as File & { path?: string }).path || file.name;
      paths.push(filePath);
    }
    try {
      const files = await prepareDroppedAudioFiles(paths);
      enqueueSelectedFiles(files, speakerCountHint);
    } catch (error) {
      showDroppedAudioError(error);
    }
  }

  function toggleQueuedFile(itemId: string) {
    setFileQueueItems((current) =>
      current.map((item) =>
        item.id === itemId ? { ...item, isExpanded: !item.isExpanded } : item,
      ),
    );
  }

  const openReview = (reference: ReviewRef, originLabel: string) => {
    setReviewTarget({
      reference,
      originLabel,
      scroll: mainRef.current?.scrollTop ?? 0,
      anchor: document.activeElement as HTMLElement,
    });
    requestAnimationFrame(() => {
      if (mainRef.current) mainRef.current.scrollTop = 0;
    });
  };
  const closeReview = () => {
    const previous = reviewTarget;
    setReviewTarget(null);
    requestAnimationFrame(() => {
      if (mainRef.current) mainRef.current.scrollTop = previous?.scroll ?? 0;
      previous?.anchor?.focus({ preventScroll: true });
    });
  };
  const reviewUpdated = useCallback((summary: TranscriptSummary) => {
    setTranscripts((current) => {
      const previous = current.find((s) => s.id === summary.id);
      if (previous && JSON.stringify(previous) === JSON.stringify(summary))
        return current;
      return prependTranscriptUnique(current, summary);
    });
    invalidateFileReview({ kind: "saved", id: summary.id });
  }, []);
  const resolvedFileModel =
    installedModels.find(
      (model) => model.id === settings?.fileTranscribeSelectedModelId,
    ) ??
    installedModels.find(
      (model) =>
        model.profile === settings?.fileTranscribeModelProfile &&
        model.isDefault,
    ) ??
    installedModels.find(
      (model) => model.profile === settings?.fileTranscribeModelProfile,
    );
  const activeReviewFile = reviewTarget
    ? fileQueueItems.find((item) => {
        const reference =
          item.reviewRef ??
          (item.result?.savedTranscript
            ? { kind: "saved" as const, id: item.result.savedTranscript.id }
            : { kind: "session" as const, id: item.id });
        return reviewKey(reference) === reviewKey(reviewTarget.reference);
      })
    : undefined;

  return (
    <div className="app-scene">
      <ToastStack toasts={toasts} onDismiss={dismissToast} />
      {trayClosePrompt ? (
        <TrayClosePrompt
          payload={trayClosePrompt}
          onKeepOpen={() => setTrayClosePrompt(null)}
          onQuit={() => {
            void quitApp();
          }}
        />
      ) : null}
      <div
        className={
          sidebarExpanded ? "app-shell" : "app-shell sidebar-collapsed"
        }
      >
        <aside className="sidebar">
          <div className="sidebar-brand">
            <span className="brand-mark">
              <AppIcon name="microphone" />
            </span>
            <span className="brand-wordmark">
              blabber<span>Space for your words</span>
            </span>
          </div>
          <IconButton
            className="sidebar-toggle"
            icon={sidebarExpanded ? "chevronLeft" : "chevronRight"}
            label={sidebarExpanded ? "Collapse sidebar" : "Expand sidebar"}
            tooltipPlacement="bottom"
            onClick={() => setSidebarExpanded((current) => !current)}
          />

          <nav className="nav-list" aria-label="Main navigation">
            {NAV_ITEMS.map((item) => (
              <button
                key={item.id}
                className={
                  "nav-item" +
                  (screen === item.id ? " active" : "") +
                  (item.id === "settings" ? " nav-settings" : "")
                }
                aria-current={screen === item.id ? "page" : undefined}
                onClick={() => {
                  if (item.id === "settings") setSettingsSection("general");
                  setReviewTarget(null);
                  setScreen(item.id);
                }}
                aria-label={item.label}
                title={item.label}
              >
                <span className="nav-icon">
                  <AppIcon name={item.icon} />
                </span>
                <span className="nav-item-label">{item.label}</span>
                {item.id === "settings" && downloadCount > 0 ? (
                  <span
                    className="nav-count"
                    aria-label="Model downloads in progress"
                  >
                    {downloadCount}
                  </span>
                ) : null}
                {item.id === "files" &&
                fileQueueItems.some((entry) => isFileWorking(entry.stage)) ? (
                  <span className="nav-count" aria-label="Files in progress">
                    {
                      fileQueueItems.filter((entry) =>
                        isFileWorking(entry.stage),
                      ).length
                    }
                  </span>
                ) : null}
                {item.id === "dictate" &&
                (recordingStatus?.state === "listening" ||
                  manualTranscriptionState.stage === "processing" ||
                  quickDictationStatus?.state === "listening" ||
                  quickDictationStatus?.state === "processing") ? (
                  <span
                    className="nav-activity"
                    aria-label="Dictation in progress"
                  />
                ) : null}
              </button>
            ))}
          </nav>
          <div className="sidebar-footer">
            <span className="privacy-dot" />
            <span>Local. Private. Yours.</span>
          </div>
        </aside>

        <main className="main-content" ref={mainRef}>
          <div className="content-frame">
            {!reviewTarget
              ? reviewJobs.filter(isReviewJobActive).map((job) => (
                  <div
                    className="review-job surface"
                    key={job.jobId}
                    role="status"
                  >
                    <div>
                      <strong>
                        Identifying speakers ·{" "}
                        {transcripts.find((t) => t.id === job.reference.id)
                          ?.title ?? "Session transcript"}
                      </strong>
                      <p className="muted">{job.statusText}</p>
                    </div>
                    <button
                      className="secondary-inline-button"
                      onClick={() =>
                        openReview(
                          job.reference,
                          {
                            dictate: "Dictate",
                            files: "Files",
                            history: "Library",
                            vocabulary: "Vocabulary",
                            settings: "Settings",
                          }[screen],
                        )
                      }
                    >
                      Review and manage
                    </button>
                  </div>
                ))
              : null}
            {reviewTarget ? (
              <ReviewWorkspace
                key={reviewKey(reviewTarget.reference)}
                reference={reviewTarget.reference}
                originLabel={reviewTarget.originLabel}
                onBack={closeReview}
                onUpdated={reviewUpdated}
                onDelete={removeTranscript}
                jobs={reviewJobs}
                onJobStarted={acceptReviewJob}
                initialJob={activeReviewFile}
                onStopInitial={
                  activeReviewFile
                    ? () => cancelFile(activeReviewFile.id)
                    : undefined
                }
                onResolveModel={() => {
                  setReviewTarget(null);
                  setSettingsSection("models");
                  setScreen("settings");
                }}
              />
            ) : null}
            <div hidden={Boolean(reviewTarget)}>
              {screen === "dictate" ? (
                <DictateScreen
                  settings={settings}
                  platform={health?.platform ?? null}
                  preview={preview}
                  recordingStatus={recordingStatus}
                  manualTranscriptionState={manualTranscriptionState}
                  quickDictationStatus={quickDictationStatus}
                  dictationError={dictationError}
                  readiness={readiness}
                  isPollingAccessibility={isPollingAccessibility}
                  onResolveReadiness={handleResolveReadiness}
                  onStartRecording={beginManualRecording}
                  onStopAndTranscribeRecording={stopAndPreviewManualRecording}
                  onCancelRecording={cancelManualRecording}
                  onResetDictation={resetDictation}
                />
              ) : null}
              {screen === "files" ? (
                <FilesScreen
                  modelReady={fileModelReady.current}
                  onResolveModel={() => {
                    setSettingsSection("models");
                    setScreen("settings");
                  }}
                  items={fileQueueItems}
                  dragging={isFileDragActive}
                  speakerCountHint={speakerCountHint}
                  showSpeakerOptions={Boolean(
                    settings?.fileDiarizationEnabled &&
                      !resolvedFileModel?.capabilities?.nativeDiarization,
                  )}
                  onSpeakerCountHintChange={setSpeakerCountHint}
                  onDragChange={setIsFileDragActive}
                  onPick={() => enqueueFiles(speakerCountHint)}
                  onDrop={(files) =>
                    handleDroppedFiles(files, speakerCountHint)
                  }
                  onToggle={toggleQueuedFile}
                  speakerMode={
                    resolvedFileModel?.capabilities?.nativeDiarization
                      ? "Built into the selected speech model"
                      : settings?.fileDiarizationEnabled
                        ? speakerModelReady === false
                          ? "Speaker identification enabled · model installing or unavailable"
                          : "Speaker identification enabled"
                        : "Speaker identification off"
                  }
                  onReview={(item) =>
                    openReview(
                      item.reviewRef ??
                        (item.result?.savedTranscript
                          ? {
                              kind: "saved",
                              id: item.result.savedTranscript.id,
                            }
                          : { kind: "session", id: item.id }),
                      "Files",
                    )
                  }
                  onDismiss={async (id) => {
                    await dismissFileTranscription(id);
                    setFileQueueItems((current) =>
                      current.filter((item) => item.id !== id),
                    );
                    fileHints.current.delete(id);
                  }}
                  onCancel={cancelFile}
                  onRetry={retryFile}
                />
              ) : null}

              {screen === "settings" ? (
                <SettingsScreen
                  initialSection={settingsSection}
                  onSectionChange={setSettingsSection}
                  settings={settings}
                  platform={health?.platform ?? null}
                  installedModels={installedModels}
                  onSave={saveSettings}
                  onReloadModelState={reloadModelState}
                />
              ) : null}

              {screen === "vocabulary" ? (
                <VocabularyScreen
                  vocabularyTerms={vocabularyTerms}
                  onCreateVocabularyTerm={createVocabulary}
                  onUpdateVocabularyTerm={updateVocabulary}
                  onDeleteVocabularyTerm={removeVocabulary}
                />
              ) : null}

              {libraryVisited || screen === "history" ? (
                <div hidden={screen !== "history"}>
                  <HistoryScreen
                    transcripts={transcripts}
                    onReview={(id) =>
                      openReview({ kind: "saved", id }, "Library")
                    }
                    onTranscriptUpdated={(updated) =>
                      setTranscripts((current) =>
                        current.map((item) =>
                          item.id === updated.id ? updated : item,
                        ),
                      )
                    }
                    onDelete={removeTranscript}
                    onDeleteAll={removeAllTranscripts}
                  />
                </div>
              ) : null}
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}

function ToastStack({
  toasts,
  onDismiss,
}: {
  toasts: Toast[];
  onDismiss: (id: number) => void;
}) {
  if (toasts.length === 0) return null;
  return (
    <div role="region" aria-label="Notifications" className="toast-stack">
      {toasts.map((toast) => (
        <ToastChip key={toast.id} toast={toast} onDismiss={onDismiss} />
      ))}
    </div>
  );
}

function TrayClosePrompt({
  payload,
  onKeepOpen,
  onQuit,
}: {
  payload: TrayUnavailableClosePayload;
  onKeepOpen: () => void;
  onQuit: () => void;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const onKeepOpenRef = useRef(onKeepOpen);
  onKeepOpenRef.current = onKeepOpen;
  useEffect(() => {
    const previousFocus = document.activeElement as HTMLElement | null;
    const dialog = dialogRef.current;
    dialog?.querySelector<HTMLButtonElement>("button")?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onKeepOpenRef.current();
      }
      if (event.key !== "Tab" || !dialog) return;
      const controls = Array.from(
        dialog.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"),
      );
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, []);
  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-modal="true"
      aria-labelledby="tray-close-title"
      className="modal-backdrop"
    >
      <div className="surface modal-panel">
        <div className="field-stack">
          <h2 id="tray-close-title" style={{ margin: 0 }}>
            {payload.title}
          </h2>
          <p className="muted" style={{ margin: 0 }}>
            {payload.message}
          </p>
        </div>
        <div
          className="toolbar"
          style={{ justifyContent: "flex-end", flexWrap: "wrap" }}
        >
          <button
            type="button"
            className="secondary-inline-button"
            onClick={onKeepOpen}
          >
            Keep open
          </button>
          <button type="button" className="danger-button" onClick={onQuit}>
            Quit Blabber
          </button>
        </div>
      </div>
    </div>
  );
}

function ToastChip({
  toast,
  onDismiss,
}: {
  toast: Toast;
  onDismiss: (id: number) => void;
}) {
  return (
    <div
      role={toast.kind === "error" ? "alert" : "status"}
      className={"toast-chip toast-" + toast.kind}
    >
      <AppIcon
        name={
          toast.kind === "error"
            ? "info"
            : toast.kind === "success"
              ? "check"
              : "info"
        }
      />
      <div className="toast-copy">
        <strong>{toast.message}</strong>
        {toast.hint ? <p>{toast.hint}</p> : null}
      </div>
      <IconButton
        icon="xmark"
        size="compact"
        label="Dismiss notification"
        onClick={() => onDismiss(toast.id)}
      />
    </div>
  );
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }
  return fallback;
}

function createQueuedFileItem(
  id: string,
  sourceFile: FileQueueItem["sourceFile"],
): FileQueueItem {
  return {
    id,
    sourceFile,
    stage: "queued",
    progressPercent: null,
    processedMs: null,
    totalMs: null,
    etaSeconds: null,
    statusText: "Queued for local transcription.",
    result: null,
    errorMessage: null,
    startedAt: null,
    isExpanded: false,
    copyState: "idle",
  };
}

function mergeFileStatusesIntoQueue(
  current: FileQueueItem[],
  statuses: FileTranscriptionStatusEvent[],
): FileQueueItem[] {
  const merged = new Map<string, FileQueueItem>();
  const currentById = new Map(current.map((item) => [item.id, item]));

  for (const status of statuses) {
    const existing = currentById.get(status.jobId);
    merged.set(status.jobId, mergeStatusWithQueueItem(existing, status));
  }

  for (const item of current) {
    if (!merged.has(item.id)) {
      merged.set(item.id, item);
    }
  }

  const next = Array.from(merged.values()).sort((left, right) => {
    const rightStarted = right.startedAt ?? 0;
    const leftStarted = left.startedAt ?? 0;
    return rightStarted - leftStarted;
  });
  return next.length === current.length &&
    next.every((item, index) => item === current[index])
    ? current
    : next;
}

function mergeFileStatusIntoQueue(
  current: FileQueueItem[],
  status: FileTranscriptionStatusEvent,
): FileQueueItem[] {
  return mergeFileStatusesIntoQueue(current, [status]);
}

function mergeStatusWithQueueItem(
  existing: FileQueueItem | undefined,
  status: FileTranscriptionStatusEvent,
): FileQueueItem {
  if (
    existing?.updatedAt &&
    (status.updatedAtMs < existing.updatedAt ||
      (status.updatedAtMs === existing.updatedAt &&
        existing.result === status.result))
  )
    return existing;
  return {
    reviewRef: status.reviewRef,
    resultRevision: status.resultRevision,
    updatedAt: status.updatedAtMs,
    id: status.jobId,
    sourceFile: status.result?.sourceFile ?? status.sourceFile,
    stage: status.stage,
    progressPercent: status.progressPercent,
    processedMs: status.processedMs,
    totalMs: status.totalMs,
    etaSeconds: status.etaSeconds,
    statusText: status.statusText,
    result: status.result ?? existing?.result ?? null,
    errorMessage: status.errorMessage,
    startedAt: status.startedAtMs,
    isExpanded: existing?.isExpanded ?? false,
    copyState: existing?.copyState ?? "idle",
  };
}

function prependTranscriptUnique(
  current: TranscriptSummary[],
  next: TranscriptSummary,
): TranscriptSummary[] {
  return [next, ...current.filter((item) => item.id !== next.id)];
}
