import { useCallback, useEffect, useRef, useState } from "react";
import {
  deleteAllTranscripts,
  deleteTranscript,
  getFileTranscriptionStatuses,
  getHealthCheck,
  getDictationReadiness,
  getQuickDictateStatus,
  getRecordingStatus,
  getSettings,
  openAccessibilitySettings,
  listInstalledModels,
  listTranscripts,
  listVocabularyTerms,
  listenFileTranscriptionStatus,
  listenQuickDictateStatus,
  listenTrayUnavailableCloseRequested,
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
  copyTextToClipboard,
} from "./lib/api";
import { HomeScreen } from "./screens/HomeScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { HistoryScreen } from "./screens/HistoryScreen";
import { VocabularyScreen } from "./screens/VocabularyScreen";
import { formatPasteShortcutForDisplay } from "./lib/formatting";
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

type ScreenId = "home" | "settings" | "vocabulary" | "history";

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

const NAV_ITEMS: Array<{ id: ScreenId; label: string; icon: NavIconName }> = [
  { id: "home", label: "Home", icon: "home" },
  { id: "settings", label: "Settings", icon: "gear" },
  { id: "vocabulary", label: "Vocabulary", icon: "book" },
  { id: "history", label: "History", icon: "clock" },
];

export function App() {
  const [screen, setScreen] = useState<ScreenId>("home");
  const [sidebarExpanded, setSidebarExpanded] = useState(true);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [health, setHealth] = useState<HealthCheckResponse | null>(null);
  const [installedModels, setInstalledModels] = useState<InstalledModel[]>([]);
  const [transcripts, setTranscripts] = useState<TranscriptSummary[]>([]);
  const [vocabularyTerms, setVocabularyTerms] = useState<VocabularyTerm[]>([]);
  const [preview, setPreview] = useState<TranscriptionPreviewResponse | null>(null);
  const [recordingStatus, setRecordingStatus] = useState<RecordingStatusResponse | null>(null);
  const [manualTranscriptionState, setManualTranscriptionState] =
    useState<ManualTranscriptionUiState>({
      stage: "idle",
      statusText: "",
      startedAt: null,
      errorMessage: null,
    });
  const [quickDictationStatus, setQuickDictationStatus] =
    useState<QuickDictationStatusResponse | null>(null);
  const [readiness, setReadiness] = useState<DictationReadiness | null>(null);
  const [fileQueueItems, setFileQueueItems] = useState<FileQueueItem[]>([]);
  const [isFileDragActive, setIsFileDragActive] = useState(false);
  const currentScreenRef = useRef<ScreenId>("home");
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [trayClosePrompt, setTrayClosePrompt] =
    useState<TrayUnavailableClosePayload | null>(null);
  const toastIdRef = useRef(0);
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
      const message = errorMessage(error, "Drop WAV, MP3, M4A, or OPUS files to transcribe.");
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

  const copyTranscriptToClipboard = useCallback(
    async (text: string) => {
      const pasteShortcut = formatPasteShortcutForDisplay(health?.platform ?? null);
      try {
        await copyTextToClipboard(text);
        pushToast({
          kind: "success",
          message: "Copied to clipboard",
          hint: `Press ${pasteShortcut} to paste`,
          durationMs: 2500,
        });
      } catch (error) {
        pushToast({
          kind: "error",
          message: "Copy failed",
          hint: errorMessage(error, "Could not copy transcript."),
          durationMs: 4000,
        });
        throw error;
      }
    },
    [health?.platform, pushToast],
  );

  useEffect(() => {
    void loadAppState();
  }, []);

  useEffect(() => {
    if (recordingStatus?.state !== "listening") {
      return;
    }
    const interval = window.setInterval(() => {
      void getRecordingStatus().then(setRecordingStatus).catch(() => undefined);
    }, 400);
    return () => window.clearInterval(interval);
  }, [recordingStatus?.state]);

  useEffect(() => {
    currentScreenRef.current = screen;
    if (screen !== "home") {
      setIsFileDragActive(false);
    }
  }, [screen]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void listenQuickDictateStatus(async (nextStatus) => {
      setQuickDictationStatus(nextStatus);

      const previousState = lastDictationStateRef.current;
      lastDictationStateRef.current = nextStatus.state;
      if (
        previousState !== nextStatus.state &&
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
          console.error(errorMessage(error, "Failed to refresh shortcut transcripts."));
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
          const cleanup = await getCurrentWindow().onFocusChanged(({ payload }) => {
            if (disposed) {
              return;
            }
            if (payload) {
              refreshVisibleState();
            }
          });

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
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void listenFileTranscriptionStatus(async (event) => {
      setFileQueueItems((current) => mergeFileStatusIntoQueue(current, event));
      if (event.stage === "completed" && event.result?.savedTranscript) {
        setTranscripts((current) => prependTranscriptUnique(current, event.result!.savedTranscript!));
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
            if (currentScreenRef.current === "home") {
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
            if (currentScreenRef.current !== "home") return;
            try {
              const files = await prepareDroppedAudioFiles(event.payload);
              enqueueSelectedFiles(files);
            } catch (error) {
              showDroppedAudioError(error);
            }
          }),
        );
      })
      .catch((error) => {
        console.error("[drag-drop] Failed to set up drag-drop listeners:", error);
      });

    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [showDroppedAudioError]);

  useEffect(() => {
    if (
      !fileQueueItems.some(
        (item) =>
          item.stage === "queued" ||
          item.stage === "preparing" ||
          item.stage === "transcribing" ||
          item.stage === "saving",
      )
    ) {
      return;
    }

    const interval = window.setInterval(() => {
      void refreshFileStatuses();
    }, 1500);

    return () => {
      window.clearInterval(interval);
    };
  }, [fileQueueItems]);

  async function loadAppState() {
    try {
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
      setScreen("home");
      setTranscripts(nextTranscripts);
      setVocabularyTerms(nextVocabularyTerms);
      setRecordingStatus(nextRecordingStatus);
      setQuickDictationStatus(nextQuickDictateStatus);
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
    } catch (error) {
      console.error(errorMessage(error, "Failed to load app state."));
    }
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
      console.error(errorMessage(error, "Failed to refresh shortcut output state."));
    }
  }

  async function refreshFileStatuses() {
    try {
      const statuses = await getFileTranscriptionStatuses();
      setFileQueueItems((current) => mergeFileStatusesIntoQueue(current, statuses));
      for (const status of statuses) {
        if (status.stage === "completed" && status.result?.savedTranscript) {
          setTranscripts((current) =>
            prependTranscriptUnique(current, status.result!.savedTranscript!),
          );
        }
      }
    } catch (error) {
      console.error(errorMessage(error, "Failed to refresh file transcription statuses."));
    }
  }

  const refreshReadiness = useCallback(async () => {
    try {
      setReadiness(await getDictationReadiness());
    } catch (error) {
      console.error(errorMessage(error, "Failed to check dictation readiness."));
    }
  }, []);

  async function saveSettings(patch: SettingsPatch) {
    const nextSettings = await updateSettings(patch);
    setSettings(nextSettings);
    setQuickDictationStatus(await getQuickDictateStatus());
    void refreshReadiness();
  }

  const reloadModelState = useCallback(async () => {
    const [nextSettings, nextInstalledModels] = await Promise.all([getSettings(), listInstalledModels()]);
    setSettings(nextSettings);
    setInstalledModels(nextInstalledModels);
    void refreshReadiness();
  }, [refreshReadiness]);

  const handleResolveReadiness = useCallback(
    async (item: "model" | "shortcut" | "accessibility") => {
      if (item === "accessibility") {
        await openAccessibilitySettings();
        // Re-check shortly after, once the user has had a chance to grant it.
        window.setTimeout(() => void refreshReadiness(), 1200);
        return;
      }
      setScreen("settings");
    },
    [refreshReadiness],
  );

  async function removeTranscript(transcriptId: string) {
    await deleteTranscript(transcriptId);
    setTranscripts((current) => current.filter((item) => item.id !== transcriptId));
  }

  async function removeAllTranscripts() {
    await deleteAllTranscripts();
    setTranscripts([]);
  }

  async function beginManualRecording() {
    setPreview(null);
    setManualTranscriptionState({
      stage: "idle",
      statusText: "",
      startedAt: null,
      errorMessage: null,
    });
    setRecordingStatus(await startRecordingSession());
  }

  async function stopAndPreviewManualRecording() {
    if (!settings) {
      return;
    }
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
      setPreview(nextPreview);
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
      setPreview(null);
      setManualTranscriptionState({
        stage: "failed",
        statusText: "Manual dictation failed.",
        startedAt,
        errorMessage: errorMessage(error, "Manual dictation failed."),
      });
    } finally {
      setRecordingStatus(await getRecordingStatus());
    }
  }

  async function cancelManualRecording() {
    setPreview(null);
    setManualTranscriptionState({
      stage: "idle",
      statusText: "",
      startedAt: null,
      errorMessage: null,
    });
    setRecordingStatus(await cancelRecordingSession());
  }

  async function resetDictation() {
    try {
      const status = await resetQuickDictation();
      setQuickDictationStatus(status);
      setRecordingStatus(await getRecordingStatus());
      pushToast({
        kind: "success",
        message: "Dictation reset",
        hint: "Audio engine restarted and the shortcut is ready again.",
        durationMs: 4000,
      });
    } catch (error) {
      pushToast({
        kind: "error",
        message: "Reset failed",
        hint: errorMessage(error, "Could not reset dictation."),
        durationMs: 6000,
      });
    }
  }

  async function createVocabulary(input: Parameters<typeof createVocabularyTerm>[0]) {
    const term = await createVocabularyTerm(input);
    setVocabularyTerms((current) =>
      [...current, term].sort((left, right) => left.canonical.localeCompare(right.canonical)),
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
    setVocabularyTerms((current) => current.filter((term) => term.id !== termId));
  }

  async function enqueueFiles() {
    const files = await pickAudioFiles();
    enqueueSelectedFiles(files);
  }

  function enqueueSelectedFiles(files: FileQueueItem["sourceFile"][]) {
    if (files.length === 0) {
      return;
    }
    const queuedItems = files.map((sourceFile) => createQueuedFileItem(crypto.randomUUID(), sourceFile));
    setFileQueueItems((current) => mergeFileStatusesIntoQueue([...queuedItems, ...current], []));

    for (const item of queuedItems) {
      void startFileTranscription({
        jobId: item.id,
        sourceFile: item.sourceFile,
      }).catch((error) => {
        const message = errorMessage(error, "Failed to transcribe the selected file.");
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

  async function handleDroppedFiles(fileList: FileList) {
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
      enqueueSelectedFiles(files);
    } catch (error) {
      showDroppedAudioError(error);
    }
  }

  function toggleQueuedFile(itemId: string) {
    setFileQueueItems((current) =>
      current.map((item) => (item.id === itemId ? { ...item, isExpanded: !item.isExpanded } : item)),
    );
  }

  async function copyQueuedFile(itemId: string, text: string) {
    try {
      await copyTranscriptToClipboard(text);
      setFileQueueItems((current) =>
        current.map((item) => (item.id === itemId ? { ...item, copyState: "copied" } : item)),
      );
      window.setTimeout(() => {
        setFileQueueItems((current) =>
          current.map((item) => (item.id === itemId ? { ...item, copyState: "idle" } : item)),
        );
      }, 1800);
    } catch {
      setFileQueueItems((current) =>
        current.map((item) => (item.id === itemId ? { ...item, copyState: "error" } : item)),
      );
      window.setTimeout(() => {
        setFileQueueItems((current) =>
          current.map((item) => (item.id === itemId ? { ...item, copyState: "idle" } : item)),
        );
      }, 1800);
    }
  }

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
      <div className={sidebarExpanded ? "app-shell" : "app-shell sidebar-collapsed"}>
        <aside className="sidebar glass-panel glass-nav">
          <button
            type="button"
            className="sidebar-toggle"
            aria-label={sidebarExpanded ? "Collapse sidebar" : "Expand sidebar"}
            title={sidebarExpanded ? "Collapse sidebar" : "Expand sidebar"}
            onClick={() => setSidebarExpanded((current) => !current)}
          >
            <span className="nav-icon">
              <NavIcon name={sidebarExpanded ? "chevron_left" : "chevron_right"} />
            </span>
            {sidebarExpanded ? <span className="nav-item-label">Collapse</span> : null}
          </button>

          <nav className="nav-list">
            {NAV_ITEMS.map((item) => (
              <button
                key={item.id}
                className={screen === item.id ? "nav-item active" : "nav-item"}
                onClick={() => setScreen(item.id)}
                aria-label={item.label}
                title={item.label}
              >
                <span className="nav-icon">
                  <NavIcon name={item.icon} />
                </span>
                <span className="nav-item-label">{item.label}</span>
              </button>
            ))}
          </nav>
        </aside>

        <main className="main-content">
          <div className="content-frame">
            {screen === "home" ? (
              <HomeScreen
                settings={settings}
                platform={health?.platform ?? null}
                preview={preview}
                recordingStatus={recordingStatus}
                manualTranscriptionState={manualTranscriptionState}
                quickDictationStatus={quickDictationStatus}
                readiness={readiness}
                onResolveReadiness={handleResolveReadiness}
                fileQueueItems={fileQueueItems}
                isFileDragActive={isFileDragActive}
                onStartRecording={beginManualRecording}
                onStopAndTranscribeRecording={stopAndPreviewManualRecording}
                onCancelRecording={cancelManualRecording}
                onResetDictation={resetDictation}
                onPickFiles={enqueueFiles}
                onDropFiles={handleDroppedFiles}
                onSetFileDragActive={setIsFileDragActive}
                onToggleFileTranscript={toggleQueuedFile}
                onCopyFileTranscript={copyQueuedFile}
              />
            ) : null}

            {screen === "settings" ? (
              <SettingsScreen
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

            {screen === "history" ? (
              <HistoryScreen
                transcripts={transcripts}
                onCopyTranscript={copyTranscriptToClipboard}
                onDelete={removeTranscript}
                onDeleteAll={removeAllTranscripts}
              />
            ) : null}
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
    <div
      role="region"
      aria-label="Notifications"
      style={{
        position: "fixed",
        left: "50%",
        bottom: 24,
        transform: "translateX(-50%)",
        display: "flex",
        flexDirection: "column",
        gap: 8,
        zIndex: 9999,
        pointerEvents: "none",
        maxWidth: "min(560px, calc(100vw - 32px))",
      }}
    >
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
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="tray-close-title"
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 9998,
        display: "grid",
        placeItems: "center",
        padding: 18,
        background: "rgba(31, 45, 61, 0.18)",
        backdropFilter: "blur(10px)",
        WebkitBackdropFilter: "blur(10px)",
      }}
    >
      <div
        className="glass-panel"
        style={{
          width: "min(520px, calc(100vw - 36px))",
          display: "grid",
          gap: 16,
        }}
      >
        <div className="field-stack">
          <h2 id="tray-close-title" style={{ margin: 0 }}>
            {payload.title}
          </h2>
          <p className="muted" style={{ margin: 0 }}>
            {payload.message}
          </p>
        </div>
        <div className="toolbar" style={{ justifyContent: "flex-end", flexWrap: "wrap" }}>
          <button type="button" className="secondary-inline-button" onClick={onKeepOpen}>
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
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    // Trigger the slide-up + fade-in on the next frame.
    const handle = requestAnimationFrame(() => setMounted(true));
    return () => cancelAnimationFrame(handle);
  }, []);

  const accent =
    toast.kind === "success"
      ? "var(--accent-success)"
      : toast.kind === "error"
        ? "var(--accent-error)"
        : "var(--accent-live)";
  const tint =
    toast.kind === "success"
      ? "rgba(130, 199, 162, 0.12)"
      : toast.kind === "error"
        ? "rgba(239, 143, 131, 0.16)"
        : "rgba(121, 174, 244, 0.14)";

  return (
    <div
      role={toast.kind === "error" ? "alert" : "status"}
      onClick={() => onDismiss(toast.id)}
      style={{
        pointerEvents: "auto",
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "12px 18px",
        borderRadius: 999,
        border: `1px solid ${accent}`,
        background: `linear-gradient(180deg, #ffffffe6, #f6f9fdcc), ${tint}`,
        boxShadow: "0 12px 28px rgba(45, 66, 94, 0.18)",
        backdropFilter: "blur(18px)",
        WebkitBackdropFilter: "blur(18px)",
        color: "var(--text-primary)",
        fontSize: "0.94rem",
        fontWeight: 600,
        opacity: mounted ? 1 : 0,
        transform: mounted ? "translateY(0)" : "translateY(8px)",
        transition: "opacity 180ms ease, transform 180ms ease",
      }}
      title="Click to dismiss"
    >
      <span
        aria-hidden="true"
        style={{
          width: 8,
          height: 8,
          borderRadius: 999,
          background: accent,
          flex: "0 0 auto",
        }}
      />
      <span>{toast.message}</span>
      {toast.hint ? (
        <span style={{ color: "var(--text-secondary)", fontWeight: 450 }}>
          {toast.hint}
        </span>
      ) : null}
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

function createQueuedFileItem(id: string, sourceFile: FileQueueItem["sourceFile"]): FileQueueItem {
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

  return Array.from(merged.values()).sort((left, right) => {
    const rightStarted = right.startedAt ?? 0;
    const leftStarted = left.startedAt ?? 0;
    return rightStarted - leftStarted;
  });
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
  return {
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

type NavIconName = "home" | "book" | "gear" | "clock" | "chevron_left" | "chevron_right";

function NavIcon({ name }: { name: NavIconName }) {
  switch (name) {
    case "home":
      return (
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M4 11.5 12 5l8 6.5" />
          <path d="M6.5 10.5V19h11v-8.5" />
        </svg>
      );
    case "book":
      return (
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M6 5.5A2.5 2.5 0 0 1 8.5 3H18v16H8.5A2.5 2.5 0 0 0 6 21.5Z" />
          <path d="M6 5.5V21.5" />
          <path d="M9.5 7.5H15" />
          <path d="M9.5 11H15" />
        </svg>
      );
    case "gear":
      return (
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m12 4 1.2 2.2 2.5.5.7 2.4 2 1.6-.8 2.4.8 2.4-2 1.6-.7 2.4-2.5.5L12 20l-1.2-2.2-2.5-.5-.7-2.4-2-1.6.8-2.4-.8-2.4 2-1.6.7-2.4 2.5-.5Z" />
          <circle cx="12" cy="12" r="3.1" />
        </svg>
      );
    case "clock":
      return (
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="12" cy="12" r="8.5" />
          <path d="M12 7.8v4.6l3 1.8" />
        </svg>
      );
    case "chevron_left":
      return (
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m14.5 6-6 6 6 6" />
        </svg>
      );
    case "chevron_right":
      return (
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="m9.5 6 6 6-6 6" />
        </svg>
      );
  }
}
