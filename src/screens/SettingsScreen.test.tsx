import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AppSettings,
  DownloadableModel,
  ModelDownloadStatus,
  SettingsPatch,
} from "../types/domain";

const apiMocks = vi.hoisted(() => ({
  getModelDownloadStatuses: vi.fn(),
  getPlatformInfo: vi.fn(),
  listDownloadableModels: vi.fn(),
  listInputDevices: vi.fn(),
  listenModelDownloadStatus: vi.fn(),
  startModelDownload: vi.fn(),
}));

vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  getModelDownloadStatuses: apiMocks.getModelDownloadStatuses,
  getPlatformInfo: apiMocks.getPlatformInfo,
  listDownloadableModels: apiMocks.listDownloadableModels,
  listInputDevices: apiMocks.listInputDevices,
  listenModelDownloadStatus: apiMocks.listenModelDownloadStatus,
  startModelDownload: apiMocks.startModelDownload,
}));

import { SettingsScreen } from "./SettingsScreen";

const initialSettings: AppSettings = {
  defaultMode: "file_transcribe",
  shortcut: "CmdOrCtrl+Shift+Space",
  shortcutMode: "push_to_talk",
  languageMode: "auto",
  fixedLanguage: null,
  preferredInputDevice: null,
  insertBehavior: "paste",
  launchAtLoginEnabled: false,
  gpuEnabled: true,
  shortcutDictationModelProfile: "balanced",
  shortcutDictationSelectedModelId: null,
  quickDictateModelProfile: "balanced",
  quickDictateSelectedModelId: null,
  fileTranscribeModelProfile: "balanced",
  fileTranscribeSelectedModelId: null,
  saveHistory: true,
  soundsEnabled: true,
  volumeDuckingEnabled: true,
  fileDiarizationEnabled: false,
};

const asrModel: DownloadableModel = {
  id: "ggml-small-bin",
  engine: "whisper.cpp",
  modelName: "ggml-small.bin",
  description: "ASR model",
  sizeBytes: 487_601_967,
  profile: "balanced",
  availability: "available",
  availabilityReason: null,
  installed: true,
  requirements: null,
  artifactCount: 1,
  capability: "asr",
};

const diarizationModel: DownloadableModel = {
  id: "sherpa-diarization-pyannote3-eres2net-v1",
  engine: "sherpa-onnx",
  modelName: "Offline speaker diarization",
  description: "Local speaker separation",
  sizeBytes: 45_586_539,
  profile: "balanced",
  availability: "available",
  availabilityReason: null,
  installed: false,
  requirements: "CPU-only · approximately 46 MB download",
  artifactCount: 2,
  capability: "diarization",
};

function Harness({ onSave, onReload }: {
  onSave: (patch: SettingsPatch) => void;
  onReload: () => Promise<void>;
}) {
  const [settings, setSettings] = useState(initialSettings);
  return (
    <SettingsScreen
      settings={settings}
      platform="macos"
      installedModels={[]}
      onSave={async (patch) => {
        onSave(patch);
        setSettings((current) => ({ ...current, ...patch }));
      }}
      onReloadModelState={onReload}
    />
  );
}

function status(
  state: ModelDownloadStatus["state"],
  progressPercent: number | null,
): ModelDownloadStatus {
  return {
    modelId: diarizationModel.id,
    modelName: diarizationModel.modelName,
    state,
    downloadedBytes: progressPercent === null
      ? 0
      : Math.round((diarizationModel.sizeBytes * progressPercent) / 100),
    totalBytes: diarizationModel.sizeBytes,
    progressPercent,
    errorMessage: state === "failed" ? "Network unavailable" : null,
    currentArtifact: state === "downloading" ? "segmentation.onnx" : null,
    artifactIndex: state === "downloading" ? 1 : null,
    artifactCount: 2,
  };
}

describe("Settings speaker identification", () => {
  let downloadListener: ((status: ModelDownloadStatus) => void | Promise<void>) | undefined;

  beforeEach(() => {
    downloadListener = undefined;
    apiMocks.getModelDownloadStatuses.mockReset().mockResolvedValue([]);
    apiMocks.getPlatformInfo.mockReset().mockResolvedValue({
      os: "macos",
      isWayland: false,
      isGnome: false,
      hasAppindicatorHint: false,
      autoPasteSupported: true,
      globalShortcutSupported: true,
      dictateToggleExecutable: null,
      dictateToggleCommand: null,
    });
    apiMocks.listDownloadableModels.mockReset().mockResolvedValue([asrModel, diarizationModel]);
    apiMocks.listInputDevices.mockReset().mockResolvedValue([]);
    apiMocks.listenModelDownloadStatus.mockReset().mockImplementation(async (listener) => {
      downloadListener = listener;
      return () => undefined;
    });
    apiMocks.startModelDownload.mockReset().mockResolvedValue(status("downloading", 0));
  });

  it("uses one switch, shows progress, and refreshes immediately after installation", async () => {
    const onSave = vi.fn();
    const onReload = vi.fn().mockResolvedValue(undefined);
    render(<Harness onSave={onSave} onReload={onReload} />);

    const row = await screen.findByText("Speaker identification");
    expect(screen.queryByText("In-app Quick Dictate")).toBeNull();
    expect(screen.queryByText("Speaker count")).toBeNull();
    expect(screen.queryByText("Offline speaker diarization")).toBeNull();

    fireEvent.click(within(row.closest(".setting-row") as HTMLElement).getByRole("button"));
    await waitFor(() => {
      expect(onSave).toHaveBeenCalledWith({ fileDiarizationEnabled: true });
      expect(screen.getByText("Starting")).toBeTruthy();
    });

    await act(async () => {
      await downloadListener?.(status("downloading", 42));
    });
    expect(screen.getByText("Installing the speaker model — 42%")).toBeTruthy();

    apiMocks.listDownloadableModels.mockResolvedValue([
      asrModel,
      { ...diarizationModel, installed: true },
    ]);
    await act(async () => {
      await downloadListener?.(status("completed", 100));
    });
    await waitFor(() => {
      expect(apiMocks.listDownloadableModels).toHaveBeenCalledTimes(2);
      expect(onReload).toHaveBeenCalledTimes(1);
      expect(screen.getByText("The local speaker model is installed.")).toBeTruthy();
      expect(screen.getByText("On")).toBeTruthy();
    });
  });

  it("offers a retry without turning off the desired setting", async () => {
    render(<Harness onSave={vi.fn()} onReload={vi.fn().mockResolvedValue(undefined)} />);
    const row = await screen.findByText("Speaker identification");
    fireEvent.click(within(row.closest(".setting-row") as HTMLElement).getByRole("button"));

    await act(async () => {
      await downloadListener?.(status("failed", null));
    });
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(apiMocks.startModelDownload).toHaveBeenCalledWith(diarizationModel.id);
    expect(screen.getByText("Retry needed")).toBeTruthy();
  });

  it("turns the desired setting off while installation is in progress", async () => {
    const onSave = vi.fn();
    render(<Harness onSave={onSave} onReload={vi.fn().mockResolvedValue(undefined)} />);
    const row = await screen.findByText("Speaker identification");
    const settingRow = row.closest(".setting-row") as HTMLElement;
    const switchButton = within(settingRow).getByRole("button");
    fireEvent.click(switchButton);

    await act(async () => {
      await downloadListener?.(status("downloading", 42));
    });
    fireEvent.click(switchButton);

    await waitFor(() => {
      expect(onSave).toHaveBeenLastCalledWith({ fileDiarizationEnabled: false });
      expect(within(settingRow).getByText("Off")).toBeTruthy();
      expect(screen.queryByText("Installing the speaker model — 42%")).toBeNull();
    });
  });
});
