import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OverlayApp } from "./OverlayApp";

const mocks = vi.hoisted(() => ({ listener: null as null | ((event: { payload: unknown }) => void), cleanup: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (_: string, callback: typeof mocks.listener) => { mocks.listener = callback; return mocks.cleanup; }) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => ({ phase: "hidden", audioLevel: 0, revision: 0 })) }));
vi.mock("./lib/api", () => ({ getHealthCheck: vi.fn(async () => ({ platform: "macos" })) }));

describe("Dictation overlay ordering", () => {
  it("keeps a newer processing language visible when a stale hide or level arrives", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    const view = render(<OverlayApp />);
    await waitFor(() => expect(mocks.listener).not.toBeNull());
    await act(async () => mocks.listener?.({ payload: { phase: "processing", outputMode: "es-AR", audioLevel: 0, statusText: "Translating · 1/2", revision: 4 } }));
    expect(screen.getByRole("status").textContent).toContain("Español (AR)");
    await act(async () => mocks.listener?.({ payload: { phase: "hidden", audioLevel: 0, revision: 3 } }));
    await act(async () => mocks.listener?.({ payload: { phase: "listening", audioLevel: 0.9, revision: 2 } }));
    expect(screen.getByRole("status").textContent).toContain("Translating · 1/2");
    view.unmount();
    expect(mocks.cleanup).toHaveBeenCalled();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
});
