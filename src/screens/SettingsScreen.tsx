import { useEffect, useRef, useState } from "react";
import { describeError, formatShortcutForDisplay, readableError } from "../lib/formatting";
import { ActionButton, Button, PageHeader } from "../components/Feedback";
import { IconButton } from "../components/IconButton";
import {
  ModelInfoButton,
  ModelPicker,
  ModelSummary,
} from "../components/ModelPicker";
import {
  formatModelSize,
  formatWorkflows,
  getFriendlyModelName,
  getModelPresentation,
} from "../lib/modelPresentation";
import {
  previewFeedbackSound,
  resetDictationStats,
  cancelModelDownload,
  cancelRecordingSession,
  deleteModel,
  getPlatformInfo,
  getRecordingInputLevel,
  getModelDownloadStatuses,
  getLivePairStatus,
  listDownloadableModels,
  listInputDevices,
  listenLivePairStatus,
  listenModelDownloadStatus,
  openModelsFolder,
  rescanModelsFolder,
  resumeShortcutCapture,
  startModelDownload,
  startRecordingSession,
  suspendShortcutCapture,
} from "../lib/api";
import type {
  AppSettings,
  DownloadableModel,
  InputDeviceOption,
  InstalledModel,
  LivePairStatus,
  ModelDownloadStatus,
  PlatformInfo,
  SettingsPatch,
} from "../types/domain";

const LIVE_PAIR_ID = "live-pair";
const R2T2_ID = "confucius4-r2t2-q8-0";

const LIVE_PAIR_STATE_LABELS: Record<LivePairStatus["state"], string> = {
  off: "Not loaded",
  preparing: "Preparing",
  ready: "Ready",
  unloaded: "Unloaded to free memory",
  error: "Needs attention",
};

interface SettingsScreenProps {
  initialSection?: string;
  onSectionChange?: (section: string) => void;
  settings: AppSettings | null;
  platform: string | null;
  installedModels: InstalledModel[];
  onSave: (patch: SettingsPatch) => Promise<void>;
  onReloadModelState: () => Promise<void>;
}

const DEFAULT_SHORTCUT = "CmdOrCtrl+Shift+Space";
/** Control-Option-L; matches translation::DEFAULT_SHORTCUT in the backend. */
const DEFAULT_LANGUAGE_SHORTCUT = "Ctrl+Alt+L";
/** Control-Option-V; matches settings::DEFAULT_PASTE_LAST_SHORTCUT in the backend. */
const DEFAULT_PASTE_LAST_SHORTCUT = "Ctrl+Alt+V";

type CaptureField = "shortcut" | "translationCycleShortcut" | "pasteLastShortcut";

/** Languages every dictation engine accepts as a fixed choice (Live dictation's list). */
const SPOKEN_LANGUAGES: Array<[string, string]> = [
  ["de", "German"],
  ["en", "English"],
  ["fr", "French"],
  ["es", "Spanish"],
  ["it", "Italian"],
  ["pt", "Portuguese"],
];

export function SettingsScreen({
  initialSection = "general",
  onSectionChange,
  settings,
  platform,
  installedModels,
  onSave,
  onReloadModelState,
}: SettingsScreenProps) {
  const [group, setGroup] = useState(initialSection);
  const [savedField, setSavedField] = useState<string | null>(null);
  const [savingField, setSavingField] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [isOpeningModelsFolder, setIsOpeningModelsFolder] = useState(false);
  const [isDownloadsExpanded, setIsDownloadsExpanded] = useState(false);
  const [isRecordEngineExpanded, setIsRecordEngineExpanded] = useState(false);
  const [confirmingDeleteId, setConfirmingDeleteId] = useState<string | null>(null);
  const [deletingModelId, setDeletingModelId] = useState<string | null>(null);
  const [isRescanningModels, setIsRescanningModels] = useState(false);
  const [downloadableModels, setDownloadableModels] = useState<
    DownloadableModel[]
  >([]);
  const [inputDevices, setInputDevices] = useState<InputDeviceOption[]>([]);
  const [modelDownloadStatuses, setModelDownloadStatuses] = useState<
    Record<string, ModelDownloadStatus>
  >({});
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [platformInfo, setPlatformInfo] = useState<PlatformInfo | null>(null);
  const [livePairStatus, setLivePairStatus] = useState<LivePairStatus | null>(null);
  const [isCapturingShortcut, setIsCapturingShortcut] = useState(false);
  const [capturingField, setCapturingField] = useState<CaptureField>("shortcut");
  const [isTestingMicrophone, setIsTestingMicrophone] = useState(false);
  const [microphoneTestLevel, setMicrophoneTestLevel] = useState(0);
  const [microphoneTestMessage, setMicrophoneTestMessage] = useState<
    string | null
  >(null);
  const microphoneTestPollerRef = useRef<number | null>(null);
  const isTestingMicrophoneRef = useRef(false);
  // Models whose download completed in this session count as installed until
  // the catalog says so (or until they are deleted).
  const completedModelsRef = useRef(new Set<string>());
  const reloadModelStateRef = useRef(onReloadModelState);
  reloadModelStateRef.current = onReloadModelState;
  const dictateToggleCommand =
    platformInfo?.dictateToggleCommand ?? "blabber --dictate-toggle";

  useEffect(() => {
    void getPlatformInfo()
      .then(setPlatformInfo)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    // Subscribe first so a slower snapshot never overwrites a newer event.
    void listenLivePairStatus((status) => {
      if (!disposed) setLivePairStatus(status);
    })
      .then(async (cleanup) => {
        if (disposed) {
          cleanup();
          return;
        }
        unlisten = cleanup;
        const snapshot = await getLivePairStatus();
        if (!disposed) setLivePairStatus((current) => current ?? snapshot);
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    void listInputDevices()
      .then(setInputDevices)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let catalogRevision = 0;
    const liveStatuses = new Set<string>();
    const completedModels = completedModelsRef.current;
    const downloadStages = new Map<string, ModelDownloadStatus["state"]>();

    const refreshCatalog = async () => {
      const revision = ++catalogRevision;
      const models = await listDownloadableModels();
      if (!disposed && revision === catalogRevision) {
        setDownloadableModels(
          models.map((model) => ({
            ...model,
            installed: model.installed || completedModels.has(model.id),
          })),
        );
      }
    };

    const acceptStatus = async (status: ModelDownloadStatus, live = false) => {
      if (disposed) return;
      const previous = downloadStages.get(status.modelId);
      downloadStages.set(status.modelId, status.state);
      setModelDownloadStatuses((current) => ({
        ...current,
        [status.modelId]: status,
      }));
      if (status.state !== "completed" || previous === "completed") return;

      // A live completion follows verified installation. Historical statuses
      // need a fresh catalog check in case the files were removed meanwhile.
      if (live) {
        completedModels.add(status.modelId);
        setDownloadableModels((models) =>
          models.map((model) =>
            model.id === status.modelId ? { ...model, installed: true } : model,
          ),
        );
      }
      const results = await Promise.allSettled([
        refreshCatalog(),
        reloadModelStateRef.current(),
      ]);
      const failed = results.find((result) => result.status === "rejected");
      if (!disposed && failed?.status === "rejected") {
        setErrorMessage(
          `The engine was downloaded, but the list could not be refreshed. ${describeError(failed.reason)}`,
        );
      }
    };

    void refreshCatalog().catch((error) => {
      if (!disposed) {
        setErrorMessage(describeError(error, "Failed to read available models."));
      }
    });
    // Subscribe before taking the snapshot. A slower snapshot must never
    // overwrite a newer completion/progress event.
    void listenModelDownloadStatus((status) => {
      liveStatuses.add(status.modelId);
      void acceptStatus(status, true);
    }).then(async (cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      unlisten = cleanup;
      const statuses = await getModelDownloadStatuses();
      await Promise.all(
        statuses.filter((status) => !liveStatuses.has(status.modelId))
          .map((status) => acceptStatus(status)),
      );
    }).catch((error) => {
      if (!disposed) {
        setErrorMessage(describeError(error, "Failed to follow model downloads."));
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  async function persist(patch: SettingsPatch, field: string) {
    setSavedField(null);
    setSavingField(field);
    await onSave(patch);
    setSavedField(field);
  }

  function fieldFeedback(field: string) {
    return (
      <span className="field-feedback" role="status">
        {savingField === field
          ? "Saving…"
          : savedField === field
            ? "Saved"
            : ""}
      </span>
    );
  }

  async function handleChange<K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ) {
    setIsSaving(true);
    setSavedField(null);
    setSavingField(key);
    setErrorMessage(null);
    try {
      await onSave({ [key]: value } as SettingsPatch);
      setSavedField(key);
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to save settings."),
      );
    } finally {
      setIsSaving(false);
      setSavingField(null);
    }
  }

  useEffect(() => {
    if (!isCapturingShortcut) {
      return;
    }

    function handleShortcutCapture(event: KeyboardEvent) {
      if (!isCapturingShortcut) {
        return;
      }

      if (
        event.key === "Escape" &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.shiftKey
      ) {
        event.preventDefault();
        event.stopPropagation();
        void cancelShortcutCapture();
        return;
      }

      const shortcut = acceleratorFromKeyboardEvent(event, platform === "macos");
      if (!shortcut) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();

      if (shortcut.kind === "unsupported") {
        setErrorMessage(shortcut.message);
        return;
      }

      setIsCapturingShortcut(false);
      void saveCapturedShortcut(shortcut.value);
    }

    window.addEventListener("keydown", handleShortcutCapture, true);
    return () =>
      window.removeEventListener("keydown", handleShortcutCapture, true);
  }, [isCapturingShortcut, settings?.shortcut, capturingField, platform]);

  useEffect(() => {
    if (!isTestingMicrophone) {
      return;
    }

    const pollLevel = () => {
      void getRecordingInputLevel()
        .then((level) => setMicrophoneTestLevel(level))
        .catch(() => setMicrophoneTestLevel(0));
    };

    pollLevel();
    microphoneTestPollerRef.current = window.setInterval(pollLevel, 80);

    return () => {
      if (microphoneTestPollerRef.current !== null) {
        window.clearInterval(microphoneTestPollerRef.current);
        microphoneTestPollerRef.current = null;
      }
    };
  }, [isTestingMicrophone]);

  useEffect(() => {
    isTestingMicrophoneRef.current = isTestingMicrophone;
  }, [isTestingMicrophone]);

  useEffect(() => {
    return () => {
      if (microphoneTestPollerRef.current !== null) {
        window.clearInterval(microphoneTestPollerRef.current);
      }
      if (isTestingMicrophoneRef.current) {
        void cancelRecordingSession().catch(() => undefined);
      }
    };
  }, []);

  if (!settings) {
    return (
      <section className="screen">
        <div className="glass-panel">Loading settings…</div>
      </section>
    );
  }

  const isMacOS = platform === "macos";
  const isWindows = platform === "windows";
  const modelsFolderAppName = isMacOS
    ? "Finder"
    : isWindows
      ? "Explorer"
      : "your file manager";
  const modelsFolderButtonLabel = isMacOS
    ? "Open in Finder"
    : isWindows
      ? "Open in Explorer"
      : "Open folder";
  const autoPasteDescription = isMacOS
    ? "Insert text directly while preserving your previous clipboard contents."
    : isWindows
      ? "Insert text directly while preserving your previous clipboard contents."
      : "Insert text directly when the platform allows simulated paste input.";
  const autoPasteEnabledLabel = isMacOS
    ? "On when Accessibility allows it"
    : isWindows
      ? "On when direct paste succeeds"
      : "On when direct paste is available";
  const displayedShortcut = formatShortcutForDisplay(
    settings.shortcut,
    platform,
  );
  const shortcutHint = isMacOS
    ? "Combine ⌘, ⌃, ⌥ or ⇧ with another key. The Fn/Globe key can't be used."
    : isWindows
      ? "Combine Ctrl, Alt or Shift with another key. The Windows key can't be used."
      : "Combine Ctrl, Alt or Shift with another key.";
  const selectedInputDeviceKnown =
    !settings.preferredInputDevice ||
    inputDevices.some((device) => device.id === settings.preferredInputDevice);
  const availableInputDevices = selectedInputDeviceKnown
    ? inputDevices
    : [
        ...inputDevices,
        {
          id: settings.preferredInputDevice ?? "__unavailable_input_device__",
          name: `${settings.preferredInputDevice ?? "Saved device"} (Unavailable)`,
          isDefault: false,
        },
      ];
  const diarizationModel = downloadableModels.find(
    (model) => model.capability === "diarization",
  );
  const speechModels = downloadableModels.filter(
    (model) => model.capability === "asr",
  );
  const translationModel = downloadableModels.find((model) => model.capability === "translation");
  const livePairInstalled = installedModels.some((model) => model.id === LIVE_PAIR_ID);
  const livePairSelected = settings.shortcutDictationSelectedModelId === LIVE_PAIR_ID;
  const dictationEngine = installedModels.find(
    (model) => model.id === settings.shortcutDictationSelectedModelId,
  );
  const recordEngine = installedModels.find(
    (model) => model.id === settings.quickDictateSelectedModelId,
  );
  // The record button normally follows the dictation engine; its own picker
  // only shows when they differ or when the user asks for it.
  const recordEngineDiffers = Boolean(
    recordEngine && dictationEngine && recordEngine.id !== dictationEngine.id,
  );
  const showRecordEngine = isRecordEngineExpanded || !recordEngine;
  const recordEngineSummary = !recordEngine
    ? "Choose an engine for the record button"
    : recordEngineDiffers
      ? `Uses ${getFriendlyModelName(recordEngine)}`
      : "Uses the dictation engine";
  const r2t2Installed = installedModels.some((model) => model.id === R2T2_ID);
  const translationDownload = translationModel ? modelDownloadStatuses[translationModel.id] : undefined;
  const diarizationReady = diarizationModel?.installed === true;
  const diarizationStatus = diarizationModel
    ? modelDownloadStatuses[diarizationModel.id]
    : undefined;
  const diarizationDownloading = diarizationStatus?.state === "downloading";
  const anotherDownloadActive = Object.values(modelDownloadStatuses).some(
    (status) =>
      status.modelId !== diarizationModel?.id && status.state === "downloading",
  );
  const diarizationProgress =
    diarizationStatus?.progressPercent === null
      ? null
      : Math.round(diarizationStatus?.progressPercent ?? 0);

  async function beginShortcutCapture(field: CaptureField = "shortcut") {
    setCapturingField(field);
    setErrorMessage(null);
    try {
      await suspendShortcutCapture();
      setIsCapturingShortcut(true);
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to start shortcut capture."),
      );
    }
  }

  async function cancelShortcutCapture() {
    setIsCapturingShortcut(false);
    try {
      await resumeShortcutCapture();
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to restore the active shortcut."),
      );
    }
  }

  async function saveCapturedShortcut(shortcut: string) {
    setIsSaving(true);
    setErrorMessage(null);
    try {
      await resumeShortcutCapture();
      await persist({ [capturingField]: shortcut }, capturingField);
    } catch (error) {
      try {
        await resumeShortcutCapture();
      } catch {
        // Preserve the save error as the primary message.
      }
      setErrorMessage(
        describeError(error, "Failed to save settings."),
      );
    } finally {
      setIsSaving(false);
      setSavingField(null);
    }
  }

  async function handleQuickDictateModelChange(modelId: string) {
    const selectedModel =
      installedModels.find((model) => model.id === modelId) ?? null;
    if (!selectedModel) {
      return;
    }

    setIsSaving(true);
    setErrorMessage(null);
    try {
      await persist(
        {
          quickDictateSelectedModelId: selectedModel.id,
          quickDictateModelProfile: selectedModel.profile,
        },
        "quickDictateSelectedModelId",
      );
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to save settings."),
      );
    } finally {
      setIsSaving(false);
      setSavingField(null);
    }
  }

  async function handleDictationEngineChange(modelId: string) {
    const selectedModel =
      installedModels.find((model) => model.id === modelId) ?? null;
    if (!selectedModel) {
      return;
    }
    // The record button follows the dictation engine unless the user gave it
    // its own engine, or this engine cannot run it (e.g. Live dictation).
    const recordFollows =
      (!recordEngine || recordEngine.id === dictationEngine?.id) &&
      (!selectedModel.capabilities ||
        selectedModel.capabilities.supportedContexts.includes("quick_dictate"));

    setIsSaving(true);
    setErrorMessage(null);
    try {
      await persist(
        {
          shortcutDictationSelectedModelId: selectedModel.id,
          shortcutDictationModelProfile: selectedModel.profile,
          ...(recordFollows
            ? {
                quickDictateSelectedModelId: selectedModel.id,
                quickDictateModelProfile: selectedModel.profile,
              }
            : {}),
        },
        "shortcutDictationSelectedModelId",
      );
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to save settings."),
      );
    } finally {
      setIsSaving(false);
      setSavingField(null);
    }
  }

  async function handleFileTranscribeModelChange(modelId: string) {
    const selectedModel =
      installedModels.find((model) => model.id === modelId) ?? null;
    if (!selectedModel) {
      return;
    }

    setIsSaving(true);
    setErrorMessage(null);
    try {
      await persist(
        {
          fileTranscribeSelectedModelId: selectedModel.id,
          fileTranscribeModelProfile: selectedModel.profile,
        },
        "fileTranscribeSelectedModelId",
      );
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to save settings."),
      );
    } finally {
      setIsSaving(false);
      setSavingField(null);
    }
  }

  async function handleLanguageChange(value: string) {
    setIsSaving(true);
    setErrorMessage(null);
    try {
      await persist(
        value === "auto"
          ? { languageMode: "auto", fixedLanguage: null }
          : { languageMode: "fixed", fixedLanguage: value },
        "languageMode",
      );
    } catch (error) {
      setErrorMessage(describeError(error, "Failed to save settings."));
    } finally {
      setIsSaving(false);
      setSavingField(null);
    }
  }

  async function handleOpenModelsFolder() {
    setIsOpeningModelsFolder(true);
    setErrorMessage(null);
    try {
      await openModelsFolder();
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to open the models folder."),
      );
    } finally {
      setIsOpeningModelsFolder(false);
    }
  }

  async function handleDownloadModel(modelId: string) {
    setErrorMessage(null);
    try {
      await startModelDownload(modelId);
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to download the selected model."),
      );
    }
  }

  async function refreshModelLists() {
    const [models] = await Promise.all([
      listDownloadableModels(),
      reloadModelStateRef.current(),
    ]);
    setDownloadableModels(
      models.map((model) => ({
        ...model,
        installed: model.installed || completedModelsRef.current.has(model.id),
      })),
    );
  }

  async function handleDeleteModel(modelId: string) {
    setErrorMessage(null);
    setDeletingModelId(modelId);
    try {
      await deleteModel(modelId);
      completedModelsRef.current.delete(modelId);
      setModelDownloadStatuses((current) => {
        const next = { ...current };
        delete next[modelId];
        return next;
      });
      await refreshModelLists();
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to delete the model."),
      );
    } finally {
      setDeletingModelId(null);
      setConfirmingDeleteId(null);
    }
  }

  async function handleRescanModels() {
    setErrorMessage(null);
    setIsRescanningModels(true);
    try {
      await rescanModelsFolder();
      await refreshModelLists();
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to rescan the models folder."),
      );
    } finally {
      setIsRescanningModels(false);
    }
  }

  function deleteControl(model: DownloadableModel, name: string) {
    const busy = Object.values(modelDownloadStatuses).some(
      (status) => status.modelId === model.id && status.state === "downloading",
    );
    return (
      <DeleteModelControl
        name={name}
        sizeBytes={model.sizeBytes}
        confirming={confirmingDeleteId === model.id}
        deleting={deletingModelId === model.id}
        disabled={busy || (deletingModelId !== null && deletingModelId !== model.id)}
        onAsk={() => setConfirmingDeleteId(model.id)}
        onCancel={() => setConfirmingDeleteId(null)}
        onConfirm={() => void handleDeleteModel(model.id)}
      />
    );
  }

  async function handleCancelModelDownload(modelId: string) {
    setErrorMessage(null);
    try {
      await cancelModelDownload(modelId);
    } catch (error) {
      setErrorMessage(
        describeError(error, "Failed to cancel the model download."),
      );
    }
  }

  async function stopMicrophoneTest(nextMessage?: string | null) {
    if (microphoneTestPollerRef.current !== null) {
      window.clearInterval(microphoneTestPollerRef.current);
      microphoneTestPollerRef.current = null;
    }
    if (isTestingMicrophone) {
      try {
        await cancelRecordingSession();
      } catch {
        // Ignore teardown failures when leaving test mode.
      }
    }
    setIsTestingMicrophone(false);
    setMicrophoneTestLevel(0);
    if (nextMessage !== undefined) {
      setMicrophoneTestMessage(nextMessage);
    }
  }

  async function toggleMicrophoneTest() {
    setErrorMessage(null);

    if (isTestingMicrophone) {
      await stopMicrophoneTest("Microphone test stopped.");
      return;
    }

    setMicrophoneTestLevel(0);
    setMicrophoneTestMessage("Starting microphone test…");
    try {
      await startRecordingSession(false);
      setIsTestingMicrophone(true);
      setMicrophoneTestMessage(
        "Speak now. Nothing is recorded or saved.",
      );
    } catch (error) {
      setIsTestingMicrophone(false);
      setMicrophoneTestLevel(0);
      setMicrophoneTestMessage(
        describeError(error, "Failed to start the microphone test."),
      );
    }
  }

  async function handlePreferredInputDeviceChange(nextValue: string) {
    if (isTestingMicrophone) {
      await stopMicrophoneTest(
        "Input device changed. Start the microphone test again.",
      );
    }

    await handleChange(
      "preferredInputDevice",
      nextValue.length === 0 ? null : nextValue,
    );
  }

  return (
    <section className="screen settings-screen">
      <PageHeader
        title="Settings"
        description="A few thoughtful adjustments. A smoother day."
      />
      <div className="settings-tabs" aria-label="Settings categories">
        {(
          [
            ["general", "General"],
            ["audio", "Dictation"],
            ["models", "Engines"],
            ["appearance", "Appearance & feedback"],
            ["advanced", "Advanced"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            aria-pressed={group === id}
            onClick={() => {
              if (!isCapturingShortcut && !isTestingMicrophone) {
                setGroup(id);
                onSectionChange?.(id);
              }
            }}
            disabled={
              (isCapturingShortcut || isTestingMicrophone) && group !== id
            }
          >
            {label}
          </button>
        ))}
      </div>
      {errorMessage ? (
        <p className="error-text" role="alert">
          {errorMessage}
        </p>
      ) : null}
      <section
        className="settings-section"
        hidden={group !== "general"}
        aria-label="General"
      >
        <div className="settings-section-heading">
          <h2>General</h2>
          <p className="muted">Make Blabber fit your day.</p>
        </div>
        <div className="settings-grid">
          <article className="settings-card">
            <label className="field-stack">
              <span>Start in</span>
              <select
                value={settings.defaultMode}
                disabled={isSaving}
                onChange={(event) =>
                  void handleChange(
                    "defaultMode",
                    event.target.value as AppSettings["defaultMode"],
                  )
                }
              >
                <option value="quick_dictate">Dictate</option>
                <option value="file_transcribe">Transcribe files</option>
              </select>
            </label>
            {fieldFeedback("defaultMode")}
          </article>
          <article className="settings-card settings-card-wide">
            <div className="setting-row">
              <div className="setting-copy">
                <p className="setting-title">Launch Blabber when you log in</p>
                <p className="muted">
                  Start the app automatically when your session begins.
                </p>
              </div>
              <div className="setting-control">
                <button
                  disabled={isSaving}
                  aria-label="Launch Blabber when you log in"
                  type="button"
                  className={
                    settings.launchAtLoginEnabled
                      ? "switch-button is-on"
                      : "switch-button"
                  }
                  aria-pressed={settings.launchAtLoginEnabled}
                  onClick={() =>
                    void handleChange(
                      "launchAtLoginEnabled",
                      !settings.launchAtLoginEnabled,
                    )
                  }
                >
                  <span className="switch-thumb" />
                </button>
                <span className="setting-state">
                  {settings.launchAtLoginEnabled ? "On at login" : "Off"}
                </span>
              </div>
              {fieldFeedback("launchAtLoginEnabled")}
            </div>
            <div className="setting-row">
              <div className="setting-copy">
                <p className="setting-title">Save transcripts to the Library</p>
                <p className="muted">
                  Keep finished dictations and file transcripts on this computer.
                </p>
              </div>
              <div className="setting-control">
                <button
                  disabled={isSaving}
                  aria-label="Save transcripts to the Library"
                  type="button"
                  className={
                    settings.saveHistory
                      ? "switch-button is-on"
                      : "switch-button"
                  }
                  aria-pressed={settings.saveHistory}
                  onClick={() =>
                    void handleChange("saveHistory", !settings.saveHistory)
                  }
                >
                  <span className="switch-thumb" />
                </button>
                <span className="setting-state">
                  {settings.saveHistory ? "On" : "Off"}
                </span>
              </div>
              {fieldFeedback("saveHistory")}
            </div>
          </article>
        </div>
      </section>
      <section
        className="settings-section"
        hidden={group !== "audio"}
        aria-label="Dictation"
      >
        <div className="settings-section-heading">
          <h2>Dictation</h2>
          <p className="muted">Your microphone, the shortcut, and what happens with your words.</p>
        </div>
        <div className="settings-grid">
          <article className="glass-subtle settings-card">
            <div className="field-stack">
              <span>Input device</span>
              <select
                aria-label="Input device"
                value={settings.preferredInputDevice || ""}
                disabled={inputDevices.length === 0}
                onChange={(event) =>
                  void handlePreferredInputDeviceChange(event.target.value)
                }
              >
                <option value="">
                  {inputDevices.length === 0
                    ? "No input devices found"
                    : "System default microphone"}
                </option>
                {availableInputDevices.map((device) => (
                  <option key={device.id} value={device.id}>
                    {device.isDefault
                      ? `${device.name} (Default)`
                      : device.name}
                  </option>
                ))}
              </select>
            </div>
            {fieldFeedback("preferredInputDevice")}
          </article>
          <article className="glass-subtle settings-card">
            <div className="field-stack">
              <span>Microphone test</span>
              <div className="microphone-test-panel">
                <p className="microphone-test-device">
                  {settings.preferredInputDevice ?? "System default microphone"}
                </p>
                <div
                  className={
                    isTestingMicrophone
                      ? "microphone-test-meter is-active"
                      : "microphone-test-meter"
                  }
                  aria-hidden="true"
                >
                  <div className="microphone-test-meter-track">
                    <div
                      className="microphone-test-meter-fill"
                      style={{
                        width: `${Math.max(0, microphoneTestLevel * 100)}%`,
                      }}
                    />
                  </div>
                  <span className="microphone-test-meter-label">
                    {isTestingMicrophone
                      ? `${Math.round(microphoneTestLevel * 100)}% input`
                      : "Idle"}
                  </span>
                </div>
                <p className="muted microphone-test-copy">
                  {microphoneTestMessage ??
                    "Speak and watch the meter. If it stays flat, Blabber can't hear this microphone."}
                </p>
                <IconButton
                  icon={isTestingMicrophone ? "stop" : "microphoneActive"}
                  label={
                    isTestingMicrophone
                      ? "Stop microphone test"
                      : "Start microphone test"
                  }
                  state={isTestingMicrophone ? "selected" : "default"}
                  onClick={() => {
                    void toggleMicrophoneTest();
                  }}
                />
              </div>
            </div>
          </article>
          <article className="glass-subtle settings-card settings-card-wide">
            <div className="field-stack">
              <span>Shortcut</span>
              <div className="shortcut-field">
                <div className="shortcut-display">
                  {isCapturingShortcut && capturingField === "shortcut"
                    ? "Listening for shortcut… Press Esc to cancel."
                    : displayedShortcut}
                </div>
                <div className="shortcut-actions">
                  <IconButton
                    icon="keyboardEdit"
                    label={
                      isCapturingShortcut
                        ? "Listening for shortcut"
                        : "Set custom shortcut"
                    }
                    state={isCapturingShortcut ? "busy" : "default"}
                    disabled={isSaving || isCapturingShortcut}
                    onClick={() => {
                      void beginShortcutCapture();
                    }}
                  />
                  <IconButton
                    icon={isCapturingShortcut ? "xCircle" : "reset"}
                    label={
                      isCapturingShortcut
                        ? "Cancel shortcut capture"
                        : "Reset shortcut to default"
                    }
                    tone={isCapturingShortcut ? "danger" : "default"}
                    disabled={
                      isSaving ||
                      (!isCapturingShortcut &&
                        settings.shortcut === DEFAULT_SHORTCUT)
                    }
                    onClick={() => {
                      if (isCapturingShortcut) {
                        void cancelShortcutCapture();
                        return;
                      }
                      void handleChange("shortcut", DEFAULT_SHORTCUT);
                    }}
                  />
                </div>
              </div>
              <p className="muted shortcut-hint">{shortcutHint}</p>
            </div>
            {fieldFeedback("shortcut")}
          </article>
          <article className="settings-card">
            <label className="field-stack">
              <span>Shortcut behavior</span>
              <select
                value={settings.shortcutMode}
                disabled={isSaving}
                onChange={(event) =>
                  void handleChange(
                    "shortcutMode",
                    event.target.value as AppSettings["shortcutMode"],
                  )
                }
              >
                <option value="push_to_talk">Hold to speak</option>
                <option value="toggle">Press to start and stop</option>
              </select>
            </label>
            {fieldFeedback("shortcutMode")}
          </article>
          <article className="settings-card">
            <label className="field-stack">
              <span>Language you speak</span>
              <select
                value={settings.languageMode === "fixed" && settings.fixedLanguage ? settings.fixedLanguage : "auto"}
                disabled={isSaving}
                onChange={(event) => void handleLanguageChange(event.target.value)}
              >
                <option value="auto">Automatic (recommended)</option>
                {SPOKEN_LANGUAGES.map(([code, name]) => (
                  <option key={code} value={code}>{name}</option>
                ))}
              </select>
            </label>
            <p className="muted">
              Automatic also follows you when you switch languages mid-sentence. Pick one if short dictations come out in the wrong language. MOSS and VibeVoice always detect the language themselves.
            </p>
            {fieldFeedback("languageMode")}
          </article>
          <article className="settings-card settings-card-wide">
            <div className="setting-row">
              <div className="setting-copy">
                <p className="setting-title">Auto paste after dictation</p>
                <p className="muted">{autoPasteDescription}</p>
              </div>
              <div className="setting-control">
                <button
                  disabled={isSaving}
                  aria-label="Auto paste after dictation"
                  type="button"
                  className={
                    settings.insertBehavior === "paste"
                      ? "switch-button is-on"
                      : "switch-button"
                  }
                  aria-pressed={settings.insertBehavior === "paste"}
                  onClick={() =>
                    void handleChange(
                      "insertBehavior",
                      settings.insertBehavior === "paste"
                        ? "clipboard_only"
                        : "paste",
                    )
                  }
                >
                  <span className="switch-thumb" />
                </button>
                <span className="setting-state">
                  {settings.insertBehavior === "paste"
                    ? autoPasteEnabledLabel
                    : "Off, copy to clipboard only"}
                </span>
              </div>
              {fieldFeedback("insertBehavior")}
            </div>
            {isMacOS || isWindows ? (
              <div className="setting-row">
                <div className="setting-copy">
                  <p className="setting-title">
                    Lower system audio during shortcut dictation
                  </p>
                  <p className="muted">
                    Drops output volume to 30% of its current level while
                    Blabber is listening, then restores it.
                  </p>
                </div>
                <div className="setting-control">
                  <button
                    disabled={isSaving}
                    aria-label="Lower system audio during dictation"
                    type="button"
                    className={
                      settings.volumeDuckingEnabled
                        ? "switch-button is-on"
                        : "switch-button"
                    }
                    aria-pressed={settings.volumeDuckingEnabled}
                    onClick={() =>
                      void handleChange(
                        "volumeDuckingEnabled",
                        !settings.volumeDuckingEnabled,
                      )
                    }
                  >
                    <span className="switch-thumb" />
                  </button>
                  <span className="setting-state">
                    {settings.volumeDuckingEnabled
                      ? "30% of current volume"
                      : "Disabled"}
                  </span>
                </div>
                {fieldFeedback("volumeDuckingEnabled")}
              </div>
            ) : null}
          </article>
          <article className="glass-subtle settings-card settings-card-wide" aria-label="Paste last dictation">
            <div className="field-stack">
              <span>Paste last dictation</span>
              <div className="shortcut-field">
                <span className="shortcut-display">
                  {isCapturingShortcut && capturingField === "pasteLastShortcut"
                    ? "Press a shortcut… Esc to cancel"
                    : formatShortcutForDisplay(settings.pasteLastShortcut ?? DEFAULT_PASTE_LAST_SHORTCUT, platform)}
                </span>
                <ActionButton
                  disabled={isCapturingShortcut || isSaving}
                  action={() => beginShortcutCapture("pasteLastShortcut")}
                >
                  Set paste-last shortcut
                </ActionButton>
                {isCapturingShortcut && capturingField === "pasteLastShortcut" ? (
                  <ActionButton action={cancelShortcutCapture}>Cancel capture</ActionButton>
                ) : (settings.pasteLastShortcut ?? DEFAULT_PASTE_LAST_SHORTCUT) !== DEFAULT_PASTE_LAST_SHORTCUT ? (
                  <IconButton
                    icon="reset"
                    label="Reset paste-last shortcut to default"
                    disabled={isSaving || isCapturingShortcut}
                    onClick={() => void handleChange("pasteLastShortcut", DEFAULT_PASTE_LAST_SHORTCUT)}
                  />
                ) : null}
              </div>
              <p className="muted">
                Pastes your last dictation again where your cursor is, for when the first paste landed in the wrong place. The menu-bar icon also has it, plus your recent dictations. Press Esc while dictating to cancel.
              </p>
            </div>
            {fieldFeedback("pasteLastShortcut")}
          </article>
          <article className="glass-subtle settings-card settings-card-wide" aria-label="Translation">
            <h3>Translate while you dictate <span className="muted">· Preview</span></h3>
            <p>Dictate in German or English, and Blabber pastes French or Argentinian Spanish.</p>
            <p className="muted">
              Needs a {formatModelSize(translationModel?.sizeBytes ?? 9_660_827_392)} download and an Apple Silicon Mac with 18 GB of memory or more. Everything stays on this computer.
            </p>
            <div className="setting-row">
              <div className="setting-copy">
                <p className="setting-title">Enable translation</p>
              </div>
              <div className="setting-control">
                <button
                  disabled={isSaving || !translationModel?.installed || translationModel.availability !== "available"}
                  aria-label="Enable translation"
                  type="button"
                  className={settings.translationEnabled ? "switch-button is-on" : "switch-button"}
                  aria-pressed={settings.translationEnabled ?? false}
                  onClick={() => void handleChange("translationEnabled", !settings.translationEnabled)}
                >
                  <span className="switch-thumb" />
                </button>
              </div>
              {fieldFeedback("translationEnabled")}
            </div>
            <div className="translation-download-actions">
              {translationDownload?.state === "downloading" ? <>
                <p role="status">Downloading the translation engine · {Math.round(translationDownload.progressPercent ?? 0)}%</p>
                <ActionButton action={() => cancelModelDownload(translationModel!.id).then(() => undefined)}>Cancel download</ActionButton>
              </> : <ActionButton disabled={!translationModel || translationModel.availability !== "available" || Object.values(modelDownloadStatuses).some((status) => status.state === "downloading")}
                action={() => startModelDownload(translationModel!.id).then(() => undefined)}>
                {translationModel?.installed ? "Check and repair" : "Download translation engine"}
              </ActionButton>}
              {translationModel?.installed && translationDownload?.state !== "downloading"
                ? deleteControl(translationModel, "the translation engine")
                : null}
            </div>
            {translationDownload?.errorMessage ? <p role="alert" className="warning-text">{readableError(translationDownload.errorMessage)}</p> : null}
            {translationModel?.availabilityReason ? <p className="muted">{translationModel.availabilityReason}</p> : null}
            <div className="field-stack">
              <span>Language shortcut</span>
              <div className="shortcut-field">
                <span className="shortcut-display">{isCapturingShortcut && capturingField === "translationCycleShortcut" ? "Press a shortcut… Esc to cancel" : formatShortcutForDisplay(settings.translationCycleShortcut ?? DEFAULT_LANGUAGE_SHORTCUT, platform)}</span>
                <ActionButton disabled={isCapturingShortcut || isSaving} action={() => beginShortcutCapture("translationCycleShortcut")}>Set language shortcut</ActionButton>
                {isCapturingShortcut && capturingField === "translationCycleShortcut" ? (
                  <ActionButton action={cancelShortcutCapture}>Cancel capture</ActionButton>
                ) : settings.translationCycleShortcut !== DEFAULT_LANGUAGE_SHORTCUT ? (
                  <IconButton
                    icon="reset"
                    label="Reset language shortcut to default"
                    disabled={isSaving || isCapturingShortcut}
                    onClick={() => void handleChange("translationCycleShortcut", DEFAULT_LANGUAGE_SHORTCUT)}
                  />
                ) : null}
              </div>
              <p className="muted">Switches Original → Français → Español (AR) between dictations. While translation is on, this key combination belongs to Blabber in every app.</p>
            </div>
            {fieldFeedback("translationCycleShortcut")}
            <p className="muted">Uses Google&apos;s TranslateGemma (converted by mradermacher), subject to the <a href="https://ai.google.dev/gemma/terms" target="_blank" rel="noreferrer">Gemma terms</a> and <a href="https://ai.google.dev/gemma/prohibited_use_policy" target="_blank" rel="noreferrer">use restrictions</a>. <a href="https://huggingface.co/google/translategemma-12b-it" target="_blank" rel="noreferrer">Model information</a></p>
          </article>
          {platformInfo && !platformInfo.globalShortcutSupported ? (
            <article className="settings-card">
              <p className="warning-text">
                Global shortcuts are unavailable in this session. Use the
                Dictate screen, or configure a system shortcut in Advanced.
              </p>
            </article>
          ) : null}
        </div>
      </section>
      <section
        className="settings-section"
        hidden={group !== "models"}
        aria-label="Engines"
      >
        <div className="settings-section-heading">
          <h2>Engines</h2>
          <p className="muted">
            The speech engines that turn your voice into text. All of them run on this computer.
          </p>
        </div>
        <div className="settings-grid">
          <article className="glass-subtle settings-card settings-card-wide" aria-label="Dictation engine">
            <div className="field-stack">
              <ModelPicker
                label="Dictation engine"
                value={settings.shortcutDictationSelectedModelId ?? ""}
                models={installedModels}
                context="shortcut_dictation"
                disabled={isSaving}
                onChange={handleDictationEngineChange}
              />
              <p className="muted">Used for the dictation shortcut and, when it can, the record button on the Dictate screen.</p>
            </div>
            {fieldFeedback("shortcutDictationSelectedModelId")}
            <div className="record-engine">
              <button
                type="button"
                className="downloads-accordion-button record-engine-toggle"
                aria-expanded={showRecordEngine}
                onClick={() => setIsRecordEngineExpanded((current) => !current)}
              >
                <span className="downloads-accordion-copy">
                  <span>Record button</span>
                  <span className="muted">{recordEngineSummary}</span>
                </span>
                <span className={showRecordEngine ? "downloads-chevron is-open" : "downloads-chevron"}>
                  <svg viewBox="0 0 20 20" aria-hidden="true">
                    <path d="m6 8 4 4 4-4" />
                  </svg>
                </span>
              </button>
              {showRecordEngine ? (
                <div className="field-stack">
                  <ModelPicker
                    label="Record button engine"
                    value={settings.quickDictateSelectedModelId ?? ""}
                    models={installedModels}
                    context="quick_dictate"
                    disabled={isSaving}
                    onChange={handleQuickDictateModelChange}
                  />
                  {fieldFeedback("quickDictateSelectedModelId")}
                </div>
              ) : null}
            </div>
            {livePairSelected && !settings.launchAtLoginEnabled ? (
              <div className="setting-row">
                <p className="muted">
                  Tip: launch Blabber at login so Live dictation is ready before your first dictation.
                </p>
                <ActionButton
                  disabled={isSaving}
                  action={() => handleChange("launchAtLoginEnabled", true)}
                >
                  Launch at login
                </ActionButton>
              </div>
            ) : null}
          </article>
          <article className="glass-subtle settings-card settings-card-wide" aria-label="File engine">
            <div className="field-stack">
              <ModelPicker
                label="File engine"
                value={settings.fileTranscribeSelectedModelId ?? ""}
                models={installedModels}
                context="file_transcription"
                disabled={isSaving}
                onChange={handleFileTranscribeModelChange}
              />
              <p className="muted">Used for audio and video files. Accuracy matters more than speed here.</p>
            </div>
            {fieldFeedback("fileTranscribeSelectedModelId")}
          </article>
          <article className="glass-subtle settings-card settings-card-wide">
            <div className="field-stack">
              <button
                type="button"
                className="downloads-accordion-button"
                aria-expanded={isDownloadsExpanded}
                onClick={() => setIsDownloadsExpanded((current) => !current)}
              >
                <div className="downloads-accordion-copy">
                  <span>Download engines</span>
                  <p className="muted">
                    {formatDownloadSummary(
                      speechModels.filter((model) => (model.origin ?? "catalog") === "catalog"),
                      installedModels,
                      modelDownloadStatuses,
                    )}
                    {speechModels.some((model) => model.origin === "retired" || model.origin === "custom")
                      ? ` · ${speechModels.filter((model) => model.origin === "retired" || model.origin === "custom").length} more installed`
                      : ""}
                  </p>
                </div>
                <span
                  className={
                    isDownloadsExpanded
                      ? "downloads-chevron is-open"
                      : "downloads-chevron"
                  }
                >
                  <svg viewBox="0 0 20 20" aria-hidden="true">
                    <path d="m6 8 4 4 4-4" />
                  </svg>
                </span>
              </button>
              {isDownloadsExpanded ? (
                <div className="downloadable-models">
                  {speechModels.map((model) => {
                    const presentation = getModelPresentation(model);
                    const isInstalled =
                      model.installed ||
                      installedModels.some(
                        (installed) => installed.id === model.id,
                      );
                    const isUnavailable = model.availability !== "available";
                    const downloadStatus = modelDownloadStatuses[model.id];
                    const isDownloading =
                      downloadStatus?.state === "downloading";
                    const anotherDownloadActive = Object.values(
                      modelDownloadStatuses,
                    ).some(
                      (status) =>
                        status.modelId !== model.id &&
                        status.state === "downloading",
                    );
                    const progressLabel =
                      downloadStatus?.totalBytes &&
                      downloadStatus.downloadedBytes > 0
                        ? `${formatModelSize(downloadStatus.downloadedBytes)} / ${formatModelSize(downloadStatus.totalBytes)}`
                        : isDownloading
                          ? `${formatModelSize(downloadStatus?.downloadedBytes ?? 0)} downloaded`
                          : null;
                    return (
                      <article
                        key={model.id}
                        className="downloadable-model-card"
                      >
                        <div className="downloadable-model-copy">
                          <div className="downloadable-model-heading">
                            <ModelSummary
                              presentation={presentation}
                              recommendation={presentation.recommendedFor.length > 0 ? "Recommended" : null}
                            />
                            <ModelInfoButton model={model} />
                          </div>
                          <p className="downloadable-model-meta">
                            {formatModelSize(model.sizeBytes)} · Works for: {formatWorkflows(model.capabilities)}
                          </p>
                          {model.availabilityReason ? (
                            <p className="downloadable-model-meta">
                              {model.availabilityReason}
                            </p>
                          ) : null}
                          {model.origin === "retired" ? (
                            <p className="downloadable-model-meta">
                              No longer offered for download. Keeps working while installed.
                            </p>
                          ) : null}
                          {model.origin === "custom" ? (
                            <p className="downloadable-model-meta warning-text">
                              Added to the models folder by hand · not verified by Blabber · use at your own risk
                            </p>
                          ) : null}
                          {model.licenseUrl ? <p className="downloadable-model-meta"><a href={model.licenseUrl} target="_blank" rel="noreferrer">NetEase model terms</a></p> : null}
                          {isDownloading ? (
                            <div className="model-download-progress">
                              <div className="model-download-progress-track">
                                <div
                                  className={
                                    downloadStatus.progressPercent === null
                                      ? "model-download-progress-bar is-indeterminate"
                                      : "model-download-progress-bar"
                                  }
                                  style={
                                    downloadStatus.progressPercent === null
                                      ? undefined
                                      : {
                                          width: `${downloadStatus.progressPercent}%`,
                                        }
                                  }
                                />
                              </div>
                              <div className="model-download-progress-meta">
                                <span>
                                  {downloadStatus.progressPercent === null
                                    ? "Downloading…"
                                    : `${Math.round(downloadStatus.progressPercent)}%`}
                                </span>
                                {progressLabel ? (
                                  <span>{progressLabel}</span>
                                ) : null}
                              </div>
                              {downloadStatus.currentArtifact ? (
                                <span className="downloadable-model-meta">
                                  File {downloadStatus.artifactIndex ?? 1} of{" "}
                                  {downloadStatus.artifactCount}
                                </span>
                              ) : null}
                            </div>
                          ) : null}
                          {downloadStatus?.state === "failed" &&
                          downloadStatus.errorMessage ? (
                            <p className="error-text model-download-error">
                              {readableError(downloadStatus.errorMessage)}
                            </p>
                          ) : null}
                        </div>
                        <div className="downloadable-model-actions">
                          <span
                            className={
                              isInstalled
                                ? "status-pill status-pill-success"
                                : isDownloading
                                  ? "status-pill status-pill-processing"
                                  : downloadStatus?.state === "failed"
                                    ? "status-pill status-pill-error"
                                    : downloadStatus?.state === "completed"
                                      ? "status-pill status-pill-success"
                                      : "status-pill status-pill-idle"
                            }
                          >
                            {isInstalled
                              ? "Installed"
                              : isUnavailable
                                ? "Unavailable"
                                : isDownloading
                                  ? "Downloading"
                                  : downloadStatus?.state === "failed"
                                    ? "Failed"
                                    : downloadStatus?.state === "completed"
                                      ? "Downloaded"
                                      : "Available"}
                          </span>
                          {model.installed ? deleteControl(model, presentation.friendlyName) : null}
                          {!isUnavailable && !isInstalled ? (
                            <IconButton
                              icon={
                                isDownloading
                                  ? "xCircle"
                                  : downloadStatus?.state === "failed"
                                    ? "retry"
                                    : "download"
                              }
                              label={
                                isDownloading
                                  ? `Cancel download of ${presentation.friendlyName}`
                                  : downloadStatus?.state === "failed"
                                    ? `Retry download of ${presentation.friendlyName}`
                                    : `Download ${presentation.friendlyName}`
                              }
                              tone={isDownloading ? "danger" : "default"}
                              disabled={!isDownloading && anotherDownloadActive}
                              onClick={() => {
                                if (isDownloading) {
                                  void handleCancelModelDownload(model.id);
                                } else {
                                  void handleDownloadModel(model.id);
                                }
                              }}
                            />
                          ) : null}
                        </div>
                      </article>
                    );
                  })}
                </div>
              ) : null}
            </div>
          </article>
          <article className="glass-subtle settings-card settings-card-wide">
            <div className="field-stack">
              <span>Who said what</span>
              <p className="muted" style={{ margin: 0 }}>
                Labels the speakers in transcribed files. MOSS and VibeVoice
                label speakers on their own and don&apos;t need this.
              </p>
              <div className="settings-option-list">
                <div className="setting-row">
                  <div className="setting-copy">
                    <p className="setting-title">Speaker identification</p>
                    <p className="muted">
                      {diarizationReady
                        ? "The local speaker model is installed."
                        : settings.fileDiarizationEnabled
                          ? diarizationDownloading
                            ? `Installing the speaker model${diarizationProgress === null ? "" : ` — ${diarizationProgress}%`}`
                            : diarizationStatus?.state === "failed"
                              ? "The speaker model could not be installed."
                              : "Preparing the speaker model download…"
                          : `Downloads ${formatModelSize(diarizationModel?.sizeBytes ?? 32_478_041)} the first time you turn this on.`}
                    </p>
                  </div>
                  <div className="setting-control">
                    <button
                      type="button"
                      className={
                        settings.fileDiarizationEnabled
                          ? "switch-button is-on"
                          : "switch-button"
                      }
                      aria-pressed={settings.fileDiarizationEnabled}
                      disabled={
                        isSaving ||
                        !diarizationModel ||
                        diarizationModel.availability !== "available" ||
                        (!settings.fileDiarizationEnabled &&
                          anotherDownloadActive)
                      }
                      onClick={() =>
                        void handleChange(
                          "fileDiarizationEnabled",
                          !settings.fileDiarizationEnabled,
                        )
                      }
                    >
                      <span className="switch-thumb" />
                    </button>
                    <span className="setting-state">
                      {settings.fileDiarizationEnabled
                        ? diarizationReady
                          ? "On"
                          : diarizationDownloading
                            ? "Installing"
                            : diarizationStatus?.state === "failed"
                              ? "Retry needed"
                              : "Starting"
                        : "Off"}
                    </span>
                  </div>
                </div>
                {diarizationReady && diarizationModel ? (
                  <div className="setting-row">
                    <p className="muted">Speaker model · {formatModelSize(diarizationModel.sizeBytes)} · deleting it turns speaker identification off</p>
                    {deleteControl(diarizationModel, "the speaker model")}
                  </div>
                ) : null}
                {settings.fileDiarizationEnabled && diarizationDownloading ? (
                  <div className="model-download-progress">
                    <div className="model-download-progress-track">
                      <div
                        className={
                          diarizationProgress === null
                            ? "model-download-progress-bar is-indeterminate"
                            : "model-download-progress-bar"
                        }
                        style={
                          diarizationProgress === null
                            ? undefined
                            : { width: `${diarizationProgress}%` }
                        }
                      />
                    </div>
                    <div className="model-download-progress-meta">
                      <span>
                        {diarizationProgress === null
                          ? "Installing…"
                          : `${diarizationProgress}%`}
                      </span>
                      {diarizationStatus?.totalBytes ? (
                        <span>
                          {formatModelSize(diarizationStatus.downloadedBytes)} /{" "}
                          {formatModelSize(diarizationStatus.totalBytes)}
                        </span>
                      ) : null}
                    </div>
                  </div>
                ) : null}
                {settings.fileDiarizationEnabled &&
                diarizationStatus?.state === "failed" ? (
                  <div className="setting-row">
                    <p className="error-text model-download-error">
                      {(diarizationStatus.errorMessage && readableError(diarizationStatus.errorMessage)) ??
                        "The speaker model download failed."}
                    </p>
                    <IconButton
                      icon="retry"
                      label="Retry speaker model download"
                      disabled={anotherDownloadActive}
                      onClick={() =>
                        diarizationModel &&
                        void handleDownloadModel(diarizationModel.id)
                      }
                    />
                  </div>
                ) : null}
                {!diarizationModel ||
                diarizationModel.availability !== "available" ? (
                  <p className="warning-text" style={{ margin: 0 }}>
                    {diarizationModel?.availabilityReason ??
                      "The speaker model is unavailable."}
                  </p>
                ) : null}
              </div>
            </div>
            {fieldFeedback("fileDiarizationEnabled")}
          </article>
        </div>
      </section>
      <section
        className="settings-section"
        hidden={group !== "appearance"}
        aria-label="Appearance & feedback"
      >
        <div className="settings-section-heading">
          <h2>Appearance & feedback</h2>
          <p className="muted">A workspace that feels like yours.</p>
        </div>
        <div className="settings-grid">
          <article className="settings-card">
            <label className="field-stack">
              <span>Appearance</span>
              <select
                value={settings.appearance}
                disabled={isSaving}
                onChange={(event) =>
                  void handleChange(
                    "appearance",
                    event.target.value as AppSettings["appearance"],
                  )
                }
              >
                <option value="system">Follow system</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>
            {fieldFeedback("appearance")}
          </article>
          <article className="settings-card">
            <label className="field-stack">
              <span>Motion</span>
              <select
                value={settings.motionPreference}
                disabled={isSaving}
                onChange={(event) =>
                  void handleChange(
                    "motionPreference",
                    event.target.value as AppSettings["motionPreference"],
                  )
                }
              >
                <option value="system">Follow system</option>
                <option value="reduced">Reduce motion</option>
              </select>
            </label>
            <p className="muted">
              System reduced-motion preferences are always respected.
            </p>
            {fieldFeedback("motionPreference")}
          </article>
          <article className="settings-card settings-card-wide">
            <div className="setting-row">
              <div className="setting-copy">
                <p className="setting-title">Play feedback sounds</p>
                <p className="muted">
                  Quiet cues for recording, task completion, and errors.
                </p>
              </div>
              <div className="setting-control">
                <button
                  disabled={isSaving}
                  aria-label="Play feedback sounds"
                  type="button"
                  className={
                    settings.soundsEnabled
                      ? "switch-button is-on"
                      : "switch-button"
                  }
                  aria-pressed={settings.soundsEnabled}
                  onClick={() =>
                    void handleChange("soundsEnabled", !settings.soundsEnabled)
                  }
                >
                  <span className="switch-thumb" />
                </button>
                <span className="setting-state">
                  {settings.soundsEnabled ? "Enabled" : "Disabled"}
                </span>
              </div>
              {fieldFeedback("soundsEnabled")}
            </div>
            <div className="sound-preview-row" aria-label="Preview sounds">
              {(["start", "stop", "complete", "error"] as const).map((cue) => (
                <ActionButton
                  key={cue}
                  action={() => previewFeedbackSound(cue)}
                  success=""
                  disabled={!settings.soundsEnabled}
                >
                  Preview {cue}
                </ActionButton>
              ))}
            </div>
          </article>
          <article className="settings-card settings-card-wide" aria-label="Personality">
            <div className="setting-row">
              <div className="setting-copy">
                <p className="setting-title">Serious mode</p>
                <p className="muted">
                  Turns off the Blabbermeter, playful messages and easter eggs.
                  Blabber never jokes about errors either way.
                </p>
              </div>
              <div className="setting-control">
                <button
                  disabled={isSaving}
                  aria-label="Serious mode"
                  type="button"
                  className={settings.seriousMode ? "switch-button is-on" : "switch-button"}
                  aria-pressed={settings.seriousMode ?? false}
                  onClick={() => void handleChange("seriousMode", !settings.seriousMode)}
                >
                  <span className="switch-thumb" />
                </button>
                <span className="setting-state">{settings.seriousMode ? "On" : "Off"}</span>
              </div>
              {fieldFeedback("seriousMode")}
            </div>
            <div className="setting-row">
              <div className="setting-copy">
                <p className="setting-title">Blabbermeter</p>
                <p className="muted">
                  Counts words, speaking time and streaks on this computer. It stores numbers only, never what you said.
                </p>
              </div>
              <ActionButton action={() => resetDictationStats()} success="Reset">
                Reset Blabbermeter
              </ActionButton>
            </div>
          </article>
        </div>
      </section>
      <section
        className="settings-section"
        hidden={group !== "advanced"}
        aria-label="Advanced"
      >
        <div className="settings-section-heading">
          <h2>Advanced</h2>
          <p className="muted">Device integration and technical controls.</p>
        </div>
        <div className="settings-grid">
          <article className="glass-subtle settings-card settings-card-wide">
            <div className="setting-row">
              <div className="setting-copy">
                <p className="setting-title">Models folder</p>
                <p className="muted">
                  Open the shared model directory in {modelsFolderAppName} to
                  add or manage model files. whisper.cpp models (.bin) you add
                  appear in the model lists after a rescan. Blabber does not
                  verify them, so use them at your own risk.
                </p>
              </div>
              <div className="setting-control">
                <IconButton
                  icon="retry"
                  label="Rescan models folder"
                  state={isRescanningModels ? "busy" : "default"}
                  disabled={isRescanningModels}
                  onClick={() => {
                    void handleRescanModels();
                  }}
                />
                <IconButton
                  icon="folder"
                  label={modelsFolderButtonLabel}
                  state={isOpeningModelsFolder ? "busy" : "default"}
                  disabled={isOpeningModelsFolder}
                  onClick={() => {
                    void handleOpenModelsFolder();
                  }}
                />
              </div>
            </div>
          </article>
          {livePairInstalled || r2t2Installed ? (
            <article className="glass-subtle settings-card settings-card-wide" aria-label="Live dictation tuning">
              <h3>Live dictation tuning <span className="muted">· Experimental</span></h3>
              {livePairInstalled ? <>
                <p className="muted" role="status">
                  Live dictation: {LIVE_PAIR_STATE_LABELS[livePairStatus?.state ?? "off"]}
                  {livePairStatus?.state === "ready" && livePairStatus.loadMs != null
                    ? ` · loaded in ${(livePairStatus.loadMs / 1000).toFixed(1)} s`
                    : ""}
                  {" · about 1.4 GB of memory while loaded"}
                </p>
                {livePairStatus?.message ? (
                  <p className={livePairStatus.state === "error" ? "warning-text" : "muted"}>{livePairStatus.message}</p>
                ) : null}
                <label className="field-stack">
                  <span>Live preview</span>
                  <select
                    value={settings.livePairChunkMs ?? 560}
                    disabled={isSaving}
                    onChange={(event) =>
                      void handleChange("livePairChunkMs", Number(event.target.value) as AppSettings["livePairChunkMs"])
                    }
                  >
                    <option value={560}>Faster · words appear sooner</option>
                    <option value={1120}>Steadier · calmer preview, better for German</option>
                  </select>
                </label>
                {fieldFeedback("livePairChunkMs")}
                <div className="setting-row">
                  <div className="setting-copy">
                    <p className="setting-title">Keep Live dictation loaded</p>
                    <p className="muted">
                      Ready the moment you press the shortcut. When off, the models load on each press and are released after a minute.
                    </p>
                  </div>
                  <div className="setting-control">
                    <button
                      disabled={isSaving}
                      aria-label="Keep Live dictation loaded"
                      type="button"
                      className={settings.livePairKeepLoaded ? "switch-button is-on" : "switch-button"}
                      aria-pressed={settings.livePairKeepLoaded}
                      onClick={() => void handleChange("livePairKeepLoaded", !settings.livePairKeepLoaded)}
                    >
                      <span className="switch-thumb" />
                    </button>
                    <span className="setting-state">{settings.livePairKeepLoaded ? "Loaded" : "On demand"}</span>
                  </div>
                  {fieldFeedback("livePairKeepLoaded")}
                </div>
              </> : null}
              {r2t2Installed ? <>
                <label className="field-stack">
                  <span>Keep R2T2 loaded after dictation</span>
                  <select
                    value={settings.r2t2IdleCache ?? "one_minute"}
                    disabled={isSaving}
                    onChange={(event) =>
                      void handleChange("r2t2IdleCache", event.target.value as AppSettings["r2t2IdleCache"])
                    }
                  >
                    <option value="one_minute">1 minute</option>
                    <option value="fifteen_minutes">15 minutes</option>
                    <option value="until_memory_pressure">Until memory runs low</option>
                  </select>
                </label>
                <p className="muted">A second dictation within this time starts without loading. Translation and model changes always release it.</p>
                <p className="warning-text">R2T2 writes each dictation in one language. If you switch languages within a dictation, it translates the other parts. For mixed-language dictation, use Live dictation or Qwen.</p>
                {fieldFeedback("r2t2IdleCache")}
              </> : null}
            </article>
          ) : null}
          {isMacOS || isWindows ? (
            <article className="settings-card settings-card-wide">
              <div className="setting-row">
                <div className="setting-copy">
                  <p className="setting-title">
                    {isMacOS
                      ? "Use Metal GPU acceleration"
                      : "Use CUDA GPU acceleration"}
                  </p>
                  <p className="muted">
                    {isMacOS
                      ? "Try Metal GPU acceleration when available, and fall back to CPU if not."
                      : "Try CUDA GPU acceleration when an NVIDIA GPU is available, and fall back to CPU if not."}
                  </p>
                </div>
                <div className="setting-control">
                  <button
                    disabled={isSaving}
                    aria-label="Use GPU acceleration"
                    type="button"
                    className={
                      settings.gpuEnabled
                        ? "switch-button is-on"
                        : "switch-button"
                    }
                    aria-pressed={settings.gpuEnabled}
                    onClick={() =>
                      void handleChange("gpuEnabled", !settings.gpuEnabled)
                    }
                  >
                    <span className="switch-thumb" />
                  </button>
                  <span className="setting-state">
                    {settings.gpuEnabled
                      ? isMacOS
                        ? "Try Metal when available"
                        : "Try CUDA when available"
                      : "CPU only"}
                  </span>
                </div>
                {fieldFeedback("gpuEnabled")}
              </div>
            </article>
          ) : null}
          {platformInfo && !platformInfo.globalShortcutSupported ? (
            <article className="glass-subtle settings-card settings-card-wide">
              <div className="field-stack">
                <strong>Bind a keyboard shortcut (Wayland)</strong>
                <p className="muted" style={{ margin: 0 }}>
                  Run{" "}
                  <code style={{ fontFamily: "monospace", fontSize: "0.85em" }}>
                    {dictateToggleCommand}
                  </code>{" "}
                  as a custom shortcut in your compositor. This path is resolved
                  from the running app, so it works even when Blabber is not on
                  your{" "}
                  <code style={{ fontFamily: "monospace", fontSize: "0.85em" }}>
                    PATH
                  </code>
                  :
                </p>
                <ul
                  className="muted"
                  style={{
                    margin: "4px 0 0 0",
                    paddingLeft: "1.4em",
                    lineHeight: 1.7,
                  }}
                >
                  <li>
                    <strong>GNOME:</strong> Settings → Keyboard → View and
                    Customise Shortcuts → Custom Shortcuts → add{" "}
                    <code
                      style={{ fontFamily: "monospace", fontSize: "0.85em" }}
                    >
                      {dictateToggleCommand}
                    </code>
                  </li>
                  <li>
                    <strong>KDE Plasma:</strong> System Settings → Shortcuts →
                    Custom Shortcuts → New → Command/URL → set command to{" "}
                    <code
                      style={{ fontFamily: "monospace", fontSize: "0.85em" }}
                    >
                      {dictateToggleCommand}
                    </code>
                  </li>
                  <li>
                    <strong>Sway:</strong>{" "}
                    <code
                      style={{ fontFamily: "monospace", fontSize: "0.85em" }}
                    >
                      bindsym $mod+Shift+Space exec {dictateToggleCommand}
                    </code>
                  </li>
                  <li>
                    <strong>Hyprland:</strong>{" "}
                    <code
                      style={{ fontFamily: "monospace", fontSize: "0.85em" }}
                    >
                      bind = $mainMod SHIFT, SPACE, exec, {dictateToggleCommand}
                    </code>
                  </li>
                </ul>
                <p className="muted" style={{ margin: "4px 0 0 0" }}>
                  Push-to-talk is not available via this method — the CLI
                  trigger always toggles. The first dictation after install may
                  show a system consent dialog for clipboard access.
                </p>
              </div>
            </article>
          ) : null}
          {platformInfo &&
          platformInfo.isGnome &&
          !platformInfo.hasAppindicatorHint ? (
            <article
              className="glass-subtle settings-card settings-card-wide"
              style={{
                borderColor: "var(--accent-warn)",
                background:
                  "linear-gradient(180deg, rgba(243, 174, 119, 0.18), rgba(243, 174, 119, 0.06))",
              }}
            >
              <div className="field-stack">
                <strong style={{ color: "#a85a1f" }}>
                  Tray icons are hidden by default in GNOME
                </strong>
                <p className="muted" style={{ margin: 0 }}>
                  Install the{" "}
                  <a
                    href="https://extensions.gnome.org/extension/615/appindicator-support/"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    AppIndicator and KStatusNotifierItem Support
                  </a>{" "}
                  extension to show Blabber&apos;s tray icon. Until then,
                  closing the main window shows an explanation first so the app
                  does not disappear without a visible tray entry.
                </p>
              </div>
            </article>
          ) : null}
        </div>
      </section>
    </section>
  );
}

function DeleteModelControl({
  name,
  sizeBytes,
  confirming,
  deleting,
  disabled,
  onAsk,
  onCancel,
  onConfirm,
}: {
  name: string;
  sizeBytes: number;
  confirming: boolean;
  deleting: boolean;
  disabled: boolean;
  onAsk: () => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  if (confirming || deleting) {
    return (
      <span className="model-delete-confirm" role="group" aria-label={`Delete ${name}`}>
        <span className="downloadable-model-meta">
          Delete {name} and free {formatModelSize(sizeBytes)}?
        </span>
        <Button variant="danger" busy={deleting} onClick={onConfirm}>
          Delete
        </Button>
        <Button variant="ghost" disabled={deleting} onClick={onCancel}>
          Keep
        </Button>
      </span>
    );
  }
  return (
    <IconButton
      icon="trash"
      label={`Delete ${name}`}
      tone="danger"
      disabled={disabled}
      onClick={onAsk}
    />
  );
}

function formatDownloadSummary(
  downloadableModels: DownloadableModel[],
  installedModels: InstalledModel[],
  statuses: Record<string, ModelDownloadStatus>,
) {
  const modelIds = new Set(downloadableModels.map((model) => model.id));
  const installedCount = downloadableModels.filter(
    (model) =>
      model.installed ||
      installedModels.some(
        (installed) =>
          installed.id === model.id || installed.modelName === model.modelName,
      ),
  ).length;
  const activeStatus = Object.values(statuses).find(
    (status) => modelIds.has(status.modelId) && status.state === "downloading",
  );
  if (activeStatus) {
    const activeModel = downloadableModels.find(
      (model) => model.id === activeStatus.modelId,
    );
    const activeName = activeModel
      ? getModelPresentation(activeModel).friendlyName
      : activeStatus.modelName;
    return `${installedCount} of ${downloadableModels.length} installed · Downloading ${activeName}`;
  }
  return `${installedCount} of ${downloadableModels.length} installed`;
}

type CapturedShortcut =
  { kind: "valid"; value: string } | { kind: "unsupported"; message: string };

function acceleratorFromKeyboardEvent(
  event: KeyboardEvent,
  isMacOS: boolean,
): CapturedShortcut | null {
  if (
    event.key === "Fn" ||
    event.key === "Globe" ||
    event.code === "Fn" ||
    event.code === "Globe" ||
    event.getModifierState("Fn")
  ) {
    return {
      kind: "unsupported",
      message:
        "The Fn/Globe key can't be used. Combine Cmd, Ctrl, Alt or Shift with another key.",
    };
  }

  const key = normalizeShortcutKey(event);
  if (!key) {
    return null;
  }

  const modifiers: string[] = [];
  // On macOS, Control is its own modifier; elsewhere Ctrl is the command key.
  if (isMacOS) {
    if (event.metaKey) modifiers.push("CmdOrCtrl");
    if (event.ctrlKey) modifiers.push("Ctrl");
  } else if (event.metaKey || event.ctrlKey) {
    modifiers.push("CmdOrCtrl");
  }
  if (event.altKey) {
    modifiers.push("Alt");
  }
  if (event.shiftKey) {
    modifiers.push("Shift");
  }

  if (modifiers.length === 0) {
    return null;
  }

  return { kind: "valid", value: [...modifiers, key].join("+") };
}

function normalizeShortcutKey(event: KeyboardEvent) {
  const { code, key } = event;
  if (code.startsWith("Key")) {
    return code.slice(3).toUpperCase();
  }
  if (code.startsWith("Digit")) {
    return code.slice(5);
  }
  if (code.startsWith("Numpad") && code.length > "Numpad".length) {
    return code;
  }
  if (/^F\d{1,2}$/.test(key)) {
    return key.toUpperCase();
  }

  switch (code) {
    case "Backquote":
      return "Backquote";
    case "Backslash":
      return "Backslash";
    case "BracketLeft":
      return "BracketLeft";
    case "BracketRight":
      return "BracketRight";
    case "Comma":
      return "Comma";
    case "Equal":
      return "Equal";
    case "Minus":
      return "Minus";
    case "Period":
      return "Period";
    case "Quote":
      return "Quote";
    case "Semicolon":
      return "Semicolon";
    case "Slash":
      return "Slash";
    case "Space":
      return "Space";
    case "Enter":
      return "Enter";
    case "Tab":
      return "Tab";
    case "Backspace":
      return "Backspace";
    case "CapsLock":
      return "CapsLock";
    case "Delete":
      return "Delete";
    case "Escape":
      return "Escape";
    case "ArrowUp":
      return "Up";
    case "ArrowDown":
      return "Down";
    case "ArrowLeft":
      return "Left";
    case "ArrowRight":
      return "Right";
    case "Home":
    case "End":
    case "PageUp":
    case "PageDown":
    case "Insert":
    case "PrintScreen":
    case "ScrollLock":
    case "NumLock":
      return code;
    case "AudioVolumeDown":
      return "AudioVolumeDown";
    case "AudioVolumeUp":
      return "AudioVolumeUp";
    case "AudioVolumeMute":
      return "AudioVolumeMute";
    default:
      break;
  }

  if (["Meta", "Control", "Shift", "Alt"].includes(key)) {
    return null;
  }
  if (key.length === 1) {
    return key.toUpperCase();
  }

  return null;
}
