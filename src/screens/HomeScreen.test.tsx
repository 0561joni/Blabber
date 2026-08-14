import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import { HomeScreen } from "./HomeScreen";

const baseProps: ComponentProps<typeof HomeScreen> = {
  settings: null,
  platform: "macos",
  preview: null,
  recordingStatus: null,
  manualTranscriptionState: {
    stage: "idle",
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
    accessibilityGranted: false,
  },
  isPollingAccessibility: false,
  onResolveReadiness: vi.fn(),
  fileQueueItems: [],
  isFileDragActive: false,
  onStartRecording: vi.fn(),
  onStopAndTranscribeRecording: vi.fn(),
  onCancelRecording: vi.fn(),
  onResetDictation: vi.fn(),
  onPickFiles: vi.fn(),
  onDropFiles: vi.fn(),
  onSetFileDragActive: vi.fn(),
  onToggleFileTranscript: vi.fn(),
  onCopyFileTranscript: vi.fn(),
};

describe("HomeScreen Accessibility readiness action", () => {
  it("opens setup with a Grant access action before polling starts", () => {
    render(<HomeScreen {...baseProps} />);

    expect(screen.getByRole("button", { name: "Grant access" })).toBeTruthy();
  });

  it("offers an immediate Check again action while polling", () => {
    const onResolveReadiness = vi.fn();
    render(
      <HomeScreen
        {...baseProps}
        isPollingAccessibility
        onResolveReadiness={onResolveReadiness}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Check again" }));

    expect(onResolveReadiness).toHaveBeenCalledWith("accessibility");
  });
});
