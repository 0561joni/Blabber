import { useEffect, useRef, useState } from "react";
import { formatShortcutForDisplay } from "../lib/formatting";
import { ActionButton, PageHeader } from "../components/Feedback";
import { IconButton } from "../components/IconButton";
import {
  ModelInfoButton,
  ModelPicker,
  ModelSummary,
} from "../components/ModelPicker";
import {
  formatModelSize,
  getModelPresentation,
} from "../lib/modelPresentation";
import {
  previewFeedbackSound,
  cancelModelDownload,
  cancelRecordingSession,
  getPlatformInfo,
  getRecordingInputLevel,
  getModelDownloadStatuses,
  listDownloadableModels,
  listInputDevices,
  listenModelDownloadStatus,
  openModelsFolder,
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
  ModelDownloadStatus,
  PlatformInfo,
  SettingsPatch,
} from "../types/domain";

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
  const [downloadableModels, setDownloadableModels] = useState<
    DownloadableModel[]
  >([]);
  const [inputDevices, setInputDevices] = useState<InputDeviceOption[]>([]);
  const [modelDownloadStatuses, setModelDownloadStatuses] = useState<
    Record<string, ModelDownloadStatus>
  >({});
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [platformInfo, setPlatformInfo] = useState<PlatformInfo | null>(null);
  const [isCapturingShortcut, setIsCapturingShortcut] = useState(false);
  const [isTestingMicrophone, setIsTestingMicrophone] = useState(false);
  const [microphoneTestLevel, setMicrophoneTestLevel] = useState(0);
  const [microphoneTestMessage, setMicrophoneTestMessage] = useState<
    string | null
  >(null);
  const microphoneTestPollerRef = useRef<number | null>(null);
  const isTestingMicrophoneRef = useRef(false);
  const dictateToggleCommand =
    platformInfo?.dictateToggleCommand ?? "blabber --dictate-toggle";

  useEffect(() => {
    void listDownloadableModels()
      .then(setDownloadableModels)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    void getPlatformInfo()
      .then(setPlatformInfo)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    void listInputDevices()
      .then(setInputDevices)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    void getModelDownloadStatuses()
      .then((statuses) => {
        setModelDownloadStatuses(
          Object.fromEntries(
            statuses.map((status) => [status.modelId, status]),
          ),
        );
      })
      .catch(() => undefined);

    let unlisten: (() => void) | null = null;
    void listenModelDownloadStatus(async (status) => {
      setModelDownloadStatuses((current) => ({
        ...current,
        [status.modelId]: status,
      }));
      if (status.state === "completed") {
        const [models] = await Promise.all([
          listDownloadableModels(),
          onReloadModelState(),
        ]);
        setDownloadableModels(models);
      }
    }).then((cleanup) => {
      unlisten = cleanup;
    });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [onReloadModelState]);

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
        error instanceof Error ? error.message : "Failed to save settings.",
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

      const shortcut = acceleratorFromKeyboardEvent(event);
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
  }, [isCapturingShortcut, settings?.shortcut]);

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
        <div className="glass-panel">Loading settings...</div>
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
    ? "Use ⌘, ⌃, ⌥, or ⇧ with another key. Fn/Globe is not supported as a global shortcut on this Tauri path."
    : isWindows
      ? "Use Ctrl, Alt, or Shift with another key. The Windows key is not supported as a global shortcut on this Tauri path."
      : "Use Cmd, Ctrl, Alt, or Shift with another key. Fn/Globe is not supported as a global shortcut on this Tauri path.";
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

  async function beginShortcutCapture() {
    setErrorMessage(null);
    try {
      await suspendShortcutCapture();
      setIsCapturingShortcut(true);
    } catch (error) {
      setErrorMessage(
        error instanceof Error
          ? error.message
          : "Failed to start shortcut capture.",
      );
    }
  }

  async function cancelShortcutCapture() {
    setIsCapturingShortcut(false);
    try {
      await resumeShortcutCapture();
    } catch (error) {
      setErrorMessage(
        error instanceof Error
          ? error.message
          : "Failed to restore the active shortcut.",
      );
    }
  }

  async function saveCapturedShortcut(shortcut: string) {
    setIsSaving(true);
    setErrorMessage(null);
    try {
      await persist({ shortcut }, "shortcut");
    } catch (error) {
      try {
        await resumeShortcutCapture();
      } catch {
        // Preserve the save error as the primary message.
      }
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to save settings.",
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
        error instanceof Error ? error.message : "Failed to save settings.",
      );
    } finally {
      setIsSaving(false);
      setSavingField(null);
    }
  }

  async function handleShortcutDictationModelChange(modelId: string) {
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
          shortcutDictationSelectedModelId: selectedModel.id,
          shortcutDictationModelProfile: selectedModel.profile,
        },
        "shortcutDictationSelectedModelId",
      );
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "Failed to save settings.",
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
        error instanceof Error ? error.message : "Failed to save settings.",
      );
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
        error instanceof Error
          ? error.message
          : "Failed to open the models folder.",
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
        error instanceof Error
          ? error.message
          : "Failed to download the selected model.",
      );
    }
  }

  async function handleCancelModelDownload(modelId: string) {
    setErrorMessage(null);
    try {
      await cancelModelDownload(modelId);
    } catch (error) {
      setErrorMessage(
        error instanceof Error
          ? error.message
          : "Failed to cancel the model download.",
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
    setMicrophoneTestMessage("Starting microphone test...");
    try {
      await startRecordingSession(false);
      setIsTestingMicrophone(true);
      setMicrophoneTestMessage(
        "Speak into the microphone. This test uses the selected input device and does not save audio.",
      );
    } catch (error) {
      setIsTestingMicrophone(false);
      setMicrophoneTestLevel(0);
      setMicrophoneTestMessage(
        error instanceof Error
          ? error.message
          : "Failed to start the microphone test.",
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
        eyebrow="MAKE IT YOURS"
        title="Settings"
        description="A few thoughtful adjustments. A smoother day."
      />
      <div className="settings-tabs" aria-label="Settings categories">
        {(
          [
            ["general", "General"],
            ["audio", "Audio & shortcuts"],
            ["models", "Models"],
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
                <p className="setting-title">Save transcripts to history</p>
                <p className="muted">
                  Keep finished dictation and file transcripts in local history.
                </p>
              </div>
              <div className="setting-control">
                <button
                  disabled={isSaving}
                  aria-label="Save transcripts to history"
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
                  {settings.saveHistory ? "Enabled by default" : "Disabled"}
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
        aria-label="Audio & shortcuts"
      >
        <div className="settings-section-heading">
          <h2>Audio & shortcuts</h2>
          <p className="muted">Choose your microphone and how you dictate.</p>
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
                    "Run a live test before using the shortcut. If the meter stays flat while you speak, Blabber is not receiving signal from the selected microphone."}
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
                  {isCapturingShortcut
                    ? "Listening for shortcut... Press Esc to cancel."
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
        aria-label="Models"
      >
        <div className="settings-section-heading">
          <h2>Models</h2>
          <p className="muted">
            Choose the right balance of speed and accuracy.
          </p>
        </div>
        <div className="settings-grid">
          <article className="glass-subtle settings-card">
            <div className="field-stack">
              <ModelPicker
                label="Shortcut Dictation model"
                value={settings.shortcutDictationSelectedModelId ?? ""}
                models={installedModels}
                context="shortcut_dictation"
                disabled={isSaving}
                onChange={handleShortcutDictationModelChange}
              />
            </div>
            {fieldFeedback("shortcutDictationSelectedModelId")}
          </article>
          <article className="glass-subtle settings-card">
            <div className="field-stack">
              <ModelPicker
                label="Quick Dictate model"
                value={settings.quickDictateSelectedModelId ?? ""}
                models={installedModels}
                context="quick_dictate"
                disabled={isSaving}
                onChange={handleQuickDictateModelChange}
              />
            </div>
            {fieldFeedback("quickDictateSelectedModelId")}
          </article>
          <article className="glass-subtle settings-card">
            <div className="field-stack">
              <ModelPicker
                label="File Transcription model"
                value={settings.fileTranscribeSelectedModelId ?? ""}
                models={installedModels}
                context="file_transcription"
                disabled={isSaving}
                onChange={handleFileTranscribeModelChange}
              />
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
                  <span>Download models</span>
                  <p className="muted">
                    {formatDownloadSummary(
                      speechModels,
                      installedModels,
                      modelDownloadStatuses,
                    )}
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
                            <ModelSummary presentation={presentation} />
                            <ModelInfoButton model={model} />
                          </div>
                          {model.availabilityReason ? (
                            <p className="downloadable-model-meta">
                              {model.availabilityReason}
                            </p>
                          ) : null}
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
                                    ? "Downloading..."
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
                              {downloadStatus.errorMessage}
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
              <span>
                Identify speakers for models without built-in speakers
              </span>
              <p className="muted" style={{ margin: 0 }}>
                Runs a local post-process after Whisper or Qwen. MOSS and
                VibeVoice preserve their built-in speaker labels automatically.
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
                      {diarizationStatus.errorMessage ??
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
            <div className="sound-preview-row">
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
                  add or manage model files.
                </p>
              </div>
              <div className="setting-control">
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
        "Fn/Globe cannot be used as a global shortcut here. Use Cmd, Ctrl, Alt, or Shift with another key.",
    };
  }

  const key = normalizeShortcutKey(event);
  if (!key) {
    return null;
  }

  const modifiers: string[] = [];
  if (event.metaKey || event.ctrlKey) {
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
