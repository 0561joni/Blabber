import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OverlayApp } from "./OverlayApp";

const mocks = vi.hoisted(() => ({ listener: null as null | ((event: { payload: unknown }) => void), cleanup: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (_: string, callback: typeof mocks.listener) => { mocks.listener = callback; return mocks.cleanup; }) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => ({ phase: "hidden", audioLevel: 0, revision: 0 })) }));
vi.mock("./lib/api", () => ({ getHealthCheck: vi.fn(async () => ({ platform: "macos" })) }));

describe("Dictation overlay ordering", () => {
  it("keeps Unicode source text while finishing and translating, and scrolls only when text arrives", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    const scroll = vi.fn();
    const oldScroll = HTMLElement.prototype.scrollTo;
    HTMLElement.prototype.scrollTo = scroll;
    const media = window.matchMedia;
    window.matchMedia = vi.fn(() => ({ matches: true })) as unknown as typeof window.matchMedia;
    const width = vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockReturnValue(2400);
    const client = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(240);
    const view = render(<OverlayApp />);
    const payload = { phase:"listening", sessionId:"one", revision:10, audioLevel:0.2, outputMode:"fr", streamingState:"listening", liveText:"Größe über die Straße · English and Deutsch" };
    await act(async () => mocks.listener?.({ payload }));
    expect(screen.getByLabelText("Live transcript").textContent).toContain("Größe");
    expect(screen.getByLabelText("Live transcript").className).toContain("has-overflow");
    expect(scroll).toHaveBeenLastCalledWith({ left:2400, behavior:"auto" });
    const calls = scroll.mock.calls.length;
    await act(async () => mocks.listener?.({ payload:{...payload, revision:11, audioLevel:0.9} }));
    expect(scroll.mock.calls.length).toBe(calls);
    await act(async () => mocks.listener?.({ payload:{...payload, revision:12, phase:"processing", streamingState:"translating", statusText:"Translating"} }));
    expect(screen.getByLabelText("Live transcript").textContent).toBe(payload.liveText);
    expect(screen.getByRole("status").textContent).toContain("Français");
    await act(async () => mocks.listener?.({ payload:{...payload, revision:9, liveText:"stale"} }));
    expect(screen.queryByText("stale")).toBeNull();
    view.unmount(); width.mockRestore(); client.mockRestore();
    HTMLElement.prototype.scrollTo = oldScroll; window.matchMedia = media;
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });
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
