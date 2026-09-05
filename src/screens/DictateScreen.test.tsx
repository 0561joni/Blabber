import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DictateScreen } from "./DictateScreen";
import { resultFixture, settingsFixture } from "../test/fixtures";

const mocks = vi.hoisted(() => ({ level: vi.fn(), copy: vi.fn() }));
vi.mock("../lib/api", () => ({
  getRecordingInputLevel: mocks.level,
  copyTextToClipboard: mocks.copy,
}));

const props = () => ({
  settings: settingsFixture,
  platform: "macos",
  preview: null,
  recordingStatus: null,
  manualTranscriptionState: {
    stage: "idle" as const,
    statusText: "",
    startedAt: null,
    errorMessage: null,
  },
  quickDictationStatus: null,
  readiness: {
    hasModel: true,
    shortcutRegistered: true,
    autoPasteEnabled: true,
    accessibilityRequired: true,
    accessibilityGranted: true,
  },
  isPollingAccessibility: false,
  onResolveReadiness: vi.fn(),
  onStartRecording: vi.fn().mockResolvedValue(undefined),
  onStopAndTranscribeRecording: vi.fn().mockResolvedValue(undefined),
  onCancelRecording: vi.fn().mockResolvedValue(undefined),
  onResetDictation: vi.fn().mockResolvedValue(undefined),
});

describe("Dictation workspace", () => {
  beforeEach(() => {
    mocks.level.mockReset().mockResolvedValue(0.6);
    mocks.copy.mockReset().mockResolvedValue(undefined);
  });
  it("starts once, shows a pending control, and keeps a failed start actionable", async () => {
    const current = props();
    let reject!: (error: Error) => void;
    current.onStartRecording.mockReturnValue(
      new Promise((_, fail) => {
        reject = fail;
      }),
    );
    render(<DictateScreen {...current} />);
    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));
    expect(current.onStartRecording).toHaveBeenCalledTimes(1);
    expect(
      screen.getByRole("button", { name: "Start recording" }),
    ).toHaveProperty("disabled", true);
    await act(async () => reject(new Error("Microphone permission denied")));
    expect(screen.getByRole("alert").textContent).toContain(
      "Microphone permission denied",
    );
    expect(
      screen.getByRole("button", { name: "Start recording" }),
    ).toHaveProperty("disabled", false);
  });
  it("uses actual input levels and stops polling after leaving the screen", async () => {
    const current = props();
    const view = render(
      <DictateScreen
        {...current}
        recordingStatus={{
          state: "listening",
          currentSessionId: "session",
          activeInputDevice: "Microphone",
          lastRecordingPath: null,
          lastErrorMessage: null,
          durationMs: 12500,
          sampleRateHz: 16000,
          channels: 1,
        }}
      />,
    );
    await waitFor(() =>
      expect(screen.getByRole("meter").getAttribute("aria-valuenow")).toBe(
        "60",
      ),
    );
    expect(screen.getByText("00:12")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel recording" }));
    await waitFor(() =>
      expect(current.onCancelRecording).toHaveBeenCalledTimes(1),
    );
    view.unmount();
    const calls = mocks.level.mock.calls.length;
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(mocks.level).toHaveBeenCalledTimes(calls);
  });
  it("shows truthful processing without a fabricated percentage or paste outcome", () => {
    render(
      <DictateScreen
        {...props()}
        manualTranscriptionState={{
          stage: "processing",
          statusText: "Transcribing locally",
          startedAt: Date.now(),
          errorMessage: null,
        }}
      />,
    );
    expect(screen.getByRole("progressbar").hasAttribute("aria-valuenow")).toBe(
      false,
    );
    expect(
      screen.getByRole("button", { name: "Transcribing…" }),
    ).toHaveProperty("disabled", true);
    expect(screen.queryByText("Pasted")).toBeNull();
  });
  it("confirms clipboard success only after copying and retains copy errors", async () => {
    render(
      <DictateScreen
        {...props()}
        preview={{
          sourceKind: "quick_dictate",
          resolvedModel: null,
          result: resultFixture,
          error: null,
        }}
      />,
    );
    expect(screen.getByText("Transcript ready")).toBeTruthy();
    mocks.copy.mockRejectedValueOnce(new Error("Clipboard unavailable"));
    fireEvent.click(screen.getByRole("button", { name: "Copy text" }));
    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      "Clipboard unavailable",
    );
    expect(screen.queryByRole("button", { name: "Copied" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Copy text" }));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
    expect(mocks.copy).toHaveBeenLastCalledWith("A useful thought.");
  });
  it("keeps manual dictation available without accessibility permission and links setup", () => {
    const current = props();
    render(
      <DictateScreen
        {...current}
        readiness={{ ...current.readiness, accessibilityGranted: false }}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Start recording" }),
    ).toHaveProperty("disabled", false);
    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));
    expect(current.onResolveReadiness).toHaveBeenCalledWith("accessibility");
  });
});
