import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DictationReadiness, StartupStatus } from "./types/domain";

const apiMocks = vi.hoisted(() => ({
  getDictationReadiness: vi.fn<() => Promise<DictationReadiness>>(),
  getStartupStatus: vi.fn<() => Promise<StartupStatus>>(),
  frontendStartupComplete: vi.fn<() => Promise<void>>(),
  reportStartupFailure: vi.fn<(message: string) => Promise<void>>(),
  startupListener: null as ((status: StartupStatus) => void) | null,
}));

vi.mock("./lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./lib/api")>()),
  getDictationReadiness: apiMocks.getDictationReadiness,
  getStartupStatus: apiMocks.getStartupStatus,
  frontendStartupComplete: apiMocks.frontendStartupComplete,
  reportStartupFailure: apiMocks.reportStartupFailure,
  listenStartupStatus: vi.fn(async (listener: (status: StartupStatus) => void) => {
    apiMocks.startupListener = listener;
    return () => {
      apiMocks.startupListener = null;
    };
  }),
}));

vi.mock("./screens/HomeScreen", () => ({
  HomeScreen: ({ readiness }: { readiness: DictationReadiness | null }) => (
    <output data-testid="accessibility-readiness">
      {String(readiness?.accessibilityGranted ?? false)}
    </output>
  ),
}));

import { App } from "./App";

const accessMissing: DictationReadiness = {
  hasModel: true,
  shortcutRegistered: true,
  autoPasteEnabled: true,
  accessibilityRequired: true,
  accessibilityGranted: false,
};

describe("App readiness lifecycle", () => {
  beforeEach(() => {
    apiMocks.getDictationReadiness.mockReset().mockResolvedValue(accessMissing);
    apiMocks.getStartupStatus.mockReset().mockResolvedValue({
      phase: "workspace",
      step: 6,
      totalSteps: 6,
    });
    apiMocks.frontendStartupComplete.mockReset().mockResolvedValue();
    apiMocks.reportStartupFailure.mockReset().mockResolvedValue();
    apiMocks.startupListener = null;
  });

  it("waits for backend readiness before loading the workspace", async () => {
    apiMocks.getStartupStatus.mockResolvedValue({ phase: "models", step: 2, totalSteps: 6 });
    render(<App />);

    await waitFor(() => expect(apiMocks.getStartupStatus).toHaveBeenCalledTimes(1));
    expect(apiMocks.getDictationReadiness).not.toHaveBeenCalled();
    expect(apiMocks.frontendStartupComplete).not.toHaveBeenCalled();

    act(() => {
      apiMocks.startupListener?.({ phase: "workspace", step: 6, totalSteps: 6 });
    });
    await waitFor(() => expect(apiMocks.frontendStartupComplete).toHaveBeenCalledTimes(1));
    expect(apiMocks.getDictationReadiness).toHaveBeenCalledTimes(1);
  });

  it("refreshes Accessibility readiness when Blabber regains focus", async () => {
    render(<App />);

    await waitFor(() => {
      expect(apiMocks.getDictationReadiness).toHaveBeenCalledTimes(1);
      expect(screen.getByTestId("accessibility-readiness").textContent).toBe("false");
    });

    apiMocks.getDictationReadiness.mockResolvedValue({
      ...accessMissing,
      accessibilityGranted: true,
    });

    act(() => {
      window.dispatchEvent(new Event("focus"));
    });

    await waitFor(() => {
      expect(apiMocks.getDictationReadiness).toHaveBeenCalledTimes(2);
      expect(screen.getByTestId("accessibility-readiness").textContent).toBe("true");
    });
  });
});
