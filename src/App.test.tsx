import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DictationReadiness } from "./types/domain";

const apiMocks = vi.hoisted(() => ({
  getDictationReadiness: vi.fn<() => Promise<DictationReadiness>>(),
}));

vi.mock("./lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./lib/api")>()),
  getDictationReadiness: apiMocks.getDictationReadiness,
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
