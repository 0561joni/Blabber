import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DownloadableModel, InstalledModel, ModelDownloadStatus } from "./types/domain";
import { settingsFixture } from "./test/fixtures";

const mocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  listInstalledModels: vi.fn(),
  listDownloadableModels: vi.fn(),
  updateSettings: vi.fn(),
  listeners: new Set<(status: ModelDownloadStatus) => void>(),
}));
vi.mock("./lib/api", async (original) => ({
  ...(await original<typeof import("./lib/api")>()),
  getSettings: mocks.getSettings,
  listInstalledModels: mocks.listInstalledModels,
  listDownloadableModels: mocks.listDownloadableModels,
  updateSettings: mocks.updateSettings,
  getModelDownloadStatuses: async () => [],
  listenModelDownloadStatus: async (listener: (status: ModelDownloadStatus) => void) => {
    mocks.listeners.add(listener);
    return () => { mocks.listeners.delete(listener); };
  },
  getStartupStatus: async () => ({ phase: "workspace", step: 6, totalSteps: 6 }),
  listenStartupStatus: async () => () => undefined,
}));

import { App } from "./App";

const r2t2: DownloadableModel = {
  id: "confucius4-r2t2-q8-0",
  engine: "r2t2",
  modelName: "R2T2",
  description: "Live shortcut dictation",
  sizeBytes: 2_477_512_064,
  profile: "accurate",
  availability: "available",
  availabilityReason: null,
  installed: false,
  requirements: "Apple Silicon",
  artifactCount: 1,
  capability: "asr",
  capabilities: {
    supportedContexts: ["shortcut_dictation"],
    streamingTranscription: true,
    nativeDiarization: false,
    timestampedSegments: false,
    contextSupport: true,
    languageControl: "automatic_and_fixed",
    maximumAudioDurationMs: 300_000,
  },
};
const installedR2t2: InstalledModel = {
  ...r2t2,
  variant: "Q8_0 · streaming",
  localPath: "/models/r2t2-q8-0",
  isDefault: false,
};
const completion: ModelDownloadStatus = {
  modelId: r2t2.id,
  modelName: r2t2.modelName,
  state: "completed",
  downloadedBytes: r2t2.sizeBytes,
  totalBytes: r2t2.sizeBytes,
  progressPercent: 100,
  errorMessage: null,
  currentArtifact: null,
  artifactIndex: 1,
  artifactCount: 1,
};

describe("Model installation in the workspace", () => {
  beforeEach(() => {
    mocks.listeners.clear();
    mocks.getSettings.mockReset().mockResolvedValue(settingsFixture);
    mocks.listInstalledModels.mockReset().mockResolvedValue([]);
    mocks.listDownloadableModels.mockReset().mockResolvedValue([r2t2]);
    mocks.updateSettings.mockReset().mockImplementation(async (patch) => ({ ...settingsFixture, ...patch }));
  });

  it("makes a downloaded R2T2 selectable immediately without navigation or restart", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.listInstalledModels).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    fireEvent.click(screen.getByRole("button", { name: /Download models/ }));
    await screen.findByRole("button", { name: /Download R2T2/ });
    await waitFor(() => expect(mocks.listeners.size).toBe(2));

    mocks.listInstalledModels.mockClear().mockResolvedValue([installedR2t2]);
    mocks.listDownloadableModels.mockResolvedValue([{ ...r2t2, installed: true }]);
    await act(async () => {
      for (const listener of mocks.listeners) listener(completion);
    });
    expect(mocks.listInstalledModels).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: /Download R2T2/ })).toBeNull();
    expect(screen.getByText("Installed")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Shortcut Dictation model" }));
    fireEvent.click(within(screen.getByRole("listbox")).getByRole("option", { name: /R2T2/ }));
    await waitFor(() => expect(mocks.updateSettings).toHaveBeenCalledWith({
      shortcutDictationSelectedModelId: r2t2.id,
      shortcutDictationModelProfile: "accurate",
    }));
    expect(screen.getByRole("button", { name: "Quick Dictate model" })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: "File Transcription model" })).toHaveProperty("disabled", true);
  });

  it("updates the installed picker even if the settings refresh fails", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.listInstalledModels).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    await waitFor(() => expect(mocks.listeners.size).toBe(2));
    mocks.getSettings.mockRejectedValue(new Error("Settings temporarily unavailable"));
    mocks.listInstalledModels.mockResolvedValue([installedR2t2]);
    await act(async () => {
      for (const listener of mocks.listeners) listener(completion);
    });
    expect(screen.getByRole("alert").textContent).toContain("Settings temporarily unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Shortcut Dictation model" }));
    expect(within(screen.getByRole("listbox")).getByRole("option", { name: /R2T2/ })).toBeTruthy();
  });

  it("reads again when completion arrives during an older model refresh", async () => {
    render(<App />);
    await waitFor(() => expect(mocks.listInstalledModels).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Models" }));
    await waitFor(() => expect(mocks.listeners.size).toBe(2));

    let finishOlderRead!: (models: InstalledModel[]) => void;
    mocks.listInstalledModels.mockClear()
      .mockImplementationOnce(() => new Promise<InstalledModel[]>((resolve) => { finishOlderRead = resolve; }))
      .mockResolvedValue([installedR2t2]);
    await act(async () => {
      for (const listener of mocks.listeners) listener({ ...completion, modelId: "another-model" });
    });
    expect(mocks.listInstalledModels).toHaveBeenCalledTimes(1);

    await act(async () => {
      for (const listener of mocks.listeners) listener(completion);
      finishOlderRead([]);
    });
    await waitFor(() => expect(mocks.listInstalledModels).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole("button", { name: "Shortcut Dictation model" }));
    expect(within(screen.getByRole("listbox")).getByRole("option", { name: /R2T2/ })).toBeTruthy();
  });
});
