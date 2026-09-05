import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AppSettings,
  DownloadableModel,
  InstalledModel,
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
  appearance: "system",
  motionPreference: "system",
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
  id: "sherpa-diarization-pyannote3-eres2net-voxceleb-v2",
  engine: "sherpa-onnx",
  modelName: "Offline speaker diarization",
  description: "Local speaker separation",
  sizeBytes: 32_478_041,
  profile: "balanced",
  availability: "available",
  availabilityReason: null,
  installed: false,
  requirements: "CPU-only · approximately 46 MB download",
  artifactCount: 2,
  capability: "diarization",
};

function Harness({
  onSave,
  onReload,
  installedModels = [],
}: {
  onSave: (patch: SettingsPatch) => void;
  onReload: () => Promise<void>;
  installedModels?: InstalledModel[];
}) {
  const [settings, setSettings] = useState(initialSettings);
  return (
    <SettingsScreen
      settings={settings}
      platform="macos"
      installedModels={installedModels}
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
    downloadedBytes:
      progressPercent === null
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
  let downloadListener:
    ((status: ModelDownloadStatus) => void | Promise<void>) | undefined;

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
    apiMocks.listDownloadableModels
      .mockReset()
      .mockResolvedValue([asrModel, diarizationModel]);
    apiMocks.listInputDevices.mockReset().mockResolvedValue([]);
    apiMocks.listenModelDownloadStatus
      .mockReset()
      .mockImplementation(async (listener) => {
        downloadListener = listener;
        return () => undefined;
      });
    apiMocks.startModelDownload
      .mockReset()
      .mockResolvedValue(status("downloading", 0));
  });

  it("does not confirm a failed appearance save and retains the saved preference", async () => {
    const onSave = vi.fn().mockRejectedValue(new Error("Disk is read-only"));
    render(
      <SettingsScreen
        settings={initialSettings}
        platform="macos"
        installedModels={[]}
        onSave={onSave}
        onReloadModelState={vi.fn()}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Appearance & feedback" }),
    );
    const appearance = screen.getByRole("combobox", {
      name: "Appearance",
    }) as HTMLSelectElement;
    fireEvent.change(appearance, { target: { value: "dark" } });
    await screen.findByText("Disk is read-only");
    expect(onSave).toHaveBeenCalledWith({ appearance: "dark" });
    expect(appearance.value).toBe("system");
    expect(appearance.disabled).toBe(false);
    expect(screen.queryByText("Saved")).toBeNull();
  });

  it("uses one switch, shows progress, and refreshes immediately after installation", async () => {
    const onSave = vi.fn();
    const onReload = vi.fn().mockResolvedValue(undefined);
    render(<Harness onSave={onSave} onReload={onReload} />);

    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    const row = await screen.findByText("Speaker identification");
    expect(screen.queryByText("In-app Quick Dictate")).toBeNull();
    expect(screen.queryByText("Speaker count")).toBeNull();
    expect(screen.queryByText("Offline speaker diarization")).toBeNull();

    fireEvent.click(
      within(row.closest(".setting-row") as HTMLElement).getByRole("button"),
    );
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
      expect(
        screen.getByText("The local speaker model is installed."),
      ).toBeTruthy();
      expect(screen.getByText("On")).toBeTruthy();
    });
  });

  it("uses named symbols for microphone, folder, and shortcut actions", async () => {
    render(
      <Harness
        onSave={vi.fn()}
        onReload={vi.fn().mockResolvedValue(undefined)}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Audio & shortcuts" }));
    await screen.findByText("Speaker identification");
    expect(
      screen.getByRole("button", { name: "Start microphone test" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Set custom shortcut" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Reset shortcut to default" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Advanced" }));
    expect(screen.getByRole("button", { name: "Open in Finder" })).toBeTruthy();
  });

  it("uses friendly two-line pickers and saves all three model contexts", async () => {
    const installed: InstalledModel[] = [
      {
        id: "ggml-large-v3-turbo-bin",
        engine: "whisper.cpp",
        modelName: "ggml-large-v3-turbo.bin",
        variant: "accurate",
        localPath: "/models/ggml-large-v3-turbo.bin",
        sizeBytes: 1_624_555_275,
        isDefault: true,
        profile: "accurate",
      },
      {
        id: "qwen3-asr-1.7b-bf16",
        engine: "qwen3_asr_c",
        modelName: "Qwen3-ASR-1.7B",
        variant: "1.7B BF16",
        localPath: "/models/qwen3-asr-1.7b-bf16",
        sizeBytes: 4_703_041_355,
        isDefault: false,
        profile: "accurate",
      },
    ];
    const onSave = vi.fn();
    render(
      <Harness
        onSave={onSave}
        onReload={vi.fn().mockResolvedValue(undefined)}
        installedModels={installed}
      />,
    );
    await screen.findByText("Speaker identification");

    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Shortcut Dictation model" }),
    );
    expect(
      within(screen.getByRole("listbox")).getByText("Recommended"),
    ).toBeTruthy();
    fireEvent.click(
      within(screen.getByRole("listbox")).getAllByRole("option")[0],
    );
    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        shortcutDictationSelectedModelId: installed[0].id,
        shortcutDictationModelProfile: "accurate",
      }),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Quick Dictate model" }),
    );
    const quickListbox = screen.getByRole("listbox");
    const qwenOption = within(quickListbox).getByRole("option", {
      name: /Qwen ASR.*Recommended/,
    });
    fireEvent.click(qwenOption);
    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        quickDictateSelectedModelId: installed[1].id,
        quickDictateModelProfile: "accurate",
      }),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "File Transcription model" }),
    );
    fireEvent.click(
      within(screen.getByRole("listbox")).getByRole("option", {
        name: /Qwen ASR.*Recommended/,
      }),
    );
    await waitFor(() =>
      expect(onSave).toHaveBeenCalledWith({
        fileTranscribeSelectedModelId: installed[1].id,
        fileTranscribeModelProfile: "accurate",
      }),
    );
  });

  it("keeps download cards simple and exposes technical details through information", async () => {
    apiMocks.listDownloadableModels.mockResolvedValue([
      { ...asrModel, installed: false },
    ]);
    render(
      <Harness
        onSave={vi.fn()}
        onReload={vi.fn().mockResolvedValue(undefined)}
      />,
    );
    await screen.findByText("Speaker identification");
    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    fireEvent.click(screen.getByRole("button", { name: /Download models/ }));

    expect(screen.getByText("Whisper Balanced")).toBeTruthy();
    expect(screen.queryByText("ggml-small.bin")).toBeNull();
    expect(screen.getByLabelText("Speed ●●●●○ Accuracy ●●●○○")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Download Whisper Balanced" }),
    ).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", { name: "About Whisper Balanced" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Whisper Balanced" });
    expect(within(dialog).getByText("ggml-small.bin")).toBeTruthy();
    expect(within(dialog).getByText("488 MB")).toBeTruthy();
  });

  it("offers a retry without turning off the desired setting", async () => {
    render(
      <Harness
        onSave={vi.fn()}
        onReload={vi.fn().mockResolvedValue(undefined)}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    const row = await screen.findByText("Speaker identification");
    fireEvent.click(
      within(row.closest(".setting-row") as HTMLElement).getByRole("button"),
    );

    await act(async () => {
      await downloadListener?.(status("failed", null));
    });
    fireEvent.click(
      screen.getByRole("button", { name: "Retry speaker model download" }),
    );
    expect(apiMocks.startModelDownload).toHaveBeenCalledWith(
      diarizationModel.id,
    );
    expect(screen.getByText("Retry needed")).toBeTruthy();
  });

  it("turns the desired setting off while installation is in progress", async () => {
    const onSave = vi.fn();
    render(
      <Harness
        onSave={onSave}
        onReload={vi.fn().mockResolvedValue(undefined)}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    const row = await screen.findByText("Speaker identification");
    const settingRow = row.closest(".setting-row") as HTMLElement;
    const switchButton = within(settingRow).getByRole("button");
    fireEvent.click(switchButton);

    await act(async () => {
      await downloadListener?.(status("downloading", 42));
    });
    fireEvent.click(switchButton);

    await waitFor(() => {
      expect(onSave).toHaveBeenLastCalledWith({
        fileDiarizationEnabled: false,
      });
      expect(within(settingRow).getByText("Off")).toBeTruthy();
      expect(
        screen.queryByText("Installing the speaker model — 42%"),
      ).toBeNull();
    });
  });
});
